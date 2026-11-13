//! The world thread: owns the [`Game`] (and so the `World`) exclusively and
//! runs the fixed-rate tick loop.
//!
//! Network handlers never touch the world. They send [`Request`]s over a
//! bounded channel; replies go back on oneshot channels and pushes on each
//! connection's own bounded queue. Each tick:
//!
//! 1. wait for the tick deadline, queueing world-changing requests (commands,
//!    foundings) in arrival order and answering reads (connect, map snapshot,
//!    terrain, lineage stats, chronicle)
//!    at once, since nothing changes the world between ticks;
//! 2. apply the queued requests in arrival order;
//! 3. `World::step`, then `Game::after_tick` (mating agreements, offer
//!    expiry, daily records);
//! 4. publish the summary/leaderboard and, when the state interval has
//!    passed, push a state message to every connection.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use evo_sim::{FounderDesign, PlayerId};
use tokio::sync::{mpsc, oneshot, watch};

use crate::game::{ConnId, FoundError, FoundOk, Game, Outbox, PersistEvent, Target};
use crate::protocol::{ChronicleView, ClientMsg, Leaderboard, LineageStats, ServerMsg, WorldSummary};
use crate::terrain::{dynamic_raw, terrain_raw, DynamicRaw, TerrainRaw};

pub enum Request {
    Found { player: PlayerId, name: String, design: FounderDesign, reply: oneshot::Sender<Result<FoundOk, FoundError>> },
    Connect { conn: ConnId, player: PlayerId, name: String, tx: mpsc::Sender<ServerMsg> },
    Disconnect { conn: ConnId },
    Command { conn: ConnId, player: PlayerId, id: Option<u64>, msg: ClientMsg },
    /// `render_rgb` at `scale` pixels per tile: (width, height, rgb).
    Snapshot { scale: usize, reply: oneshot::Sender<(usize, usize, Vec<u8>)> },
    /// The tile grid: every layer, or only the dynamic ones.
    Terrain { full: bool, reply: oneshot::Sender<TerrainReply> },
    /// The genetics dashboard of a player's lineage.
    Stats { player: PlayerId, reply: oneshot::Sender<LineageStats> },
    /// Species, eras, catastrophes and achievements.
    Chronicle { reply: oneshot::Sender<Box<ChronicleView>> },
}

pub enum TerrainReply {
    Full(Box<TerrainRaw>),
    Dynamic(Box<DynamicRaw>),
}

/// What the world thread publishes after every tick for cheap reads.
#[derive(Clone, Debug)]
pub struct Published {
    pub summary: WorldSummary,
    pub leaderboard: Leaderboard,
}

#[derive(Clone, Debug)]
pub struct RunnerConfig {
    /// Wall time per tick. Zero runs as fast as the simulation allows.
    pub tick: Duration,
    /// Wall time between state pushes to each connection.
    pub state_interval: Duration,
}

/// Queue depth of the request channel; a full queue answers "busy".
pub const REQUEST_QUEUE: usize = 4096;

pub struct Runner {
    game: Game,
    cfg: RunnerConfig,
    rx: Receiver<Request>,
    conns: BTreeMap<ConnId, (PlayerId, mpsc::Sender<ServerMsg>)>,
    queued: Vec<Request>,
    published: watch::Sender<Arc<Published>>,
    persist: mpsc::UnboundedSender<PersistEvent>,
    last_state: Instant,
}

/// Handles for the rest of the server.
pub struct RunnerHandles {
    pub tx: SyncSender<Request>,
    pub published: watch::Receiver<Arc<Published>>,
    pub persist: mpsc::UnboundedReceiver<PersistEvent>,
}

impl Runner {
    pub fn new(game: Game, cfg: RunnerConfig) -> (Self, RunnerHandles) {
        let (tx, rx) = std::sync::mpsc::sync_channel(REQUEST_QUEUE);
        let initial = Arc::new(Published { summary: game.summary(), leaderboard: game.leaderboard() });
        let (published, published_rx) = watch::channel(initial);
        let (persist, persist_rx) = mpsc::unbounded_channel();
        let runner = Runner { game, cfg, rx, conns: BTreeMap::new(), queued: Vec::new(), published, persist, last_state: Instant::now() };
        (runner, RunnerHandles { tx, published: published_rx, persist: persist_rx })
    }

    /// Run on a dedicated OS thread until every request sender is dropped.
    pub fn spawn(self) -> std::thread::JoinHandle<()> {
        std::thread::Builder::new().name("world".into()).spawn(move || self.run()).expect("spawn world thread")
    }

    fn run(mut self) {
        let mut next = Instant::now() + self.cfg.tick;
        loop {
            // 1. Gather requests until the tick is due.
            loop {
                let now = Instant::now();
                let req = if now >= next {
                    match self.rx.try_recv() {
                        Ok(r) => r,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                } else {
                    match self.rx.recv_timeout(next - now) {
                        Ok(r) => r,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                };
                self.receive(req);
            }
            // 2. Apply world-changing requests in arrival order.
            for req in std::mem::take(&mut self.queued) {
                self.apply(req);
            }
            // 3. Advance the world.
            self.game.world.step();
            let out = self.game.after_tick();
            self.dispatch(out);
            for ev in self.game.take_persist() {
                let _ = self.persist.send(ev);
            }
            // 4. Publish.
            self.published.send_replace(Arc::new(Published { summary: self.game.summary(), leaderboard: self.game.leaderboard() }));
            if self.last_state.elapsed() >= self.cfg.state_interval {
                self.last_state = Instant::now();
                let states = self.game.states();
                self.dispatch(states);
            }
            let now = Instant::now();
            next += self.cfg.tick;
            if next < now {
                // Running behind (or as fast as possible): don't try to catch up.
                next = now;
            }
        }
    }

    /// Answer reads now; queue changes for the tick boundary.
    fn receive(&mut self, req: Request) {
        match req {
            Request::Connect { conn, player, name, tx } => {
                self.conns.insert(conn, (player, tx));
                let out = self.game.connect(conn, player, &name);
                self.dispatch(out);
            }
            Request::Disconnect { conn } => {
                self.conns.remove(&conn);
                self.game.disconnect(conn);
            }
            Request::Snapshot { scale, reply } => {
                let _ = reply.send(evo_sim::render::render_rgb(&self.game.world, scale));
            }
            Request::Terrain { full, reply } => {
                let w = &self.game.world;
                let _ = reply.send(if full { TerrainReply::Full(Box::new(terrain_raw(w))) } else { TerrainReply::Dynamic(Box::new(dynamic_raw(w))) });
            }
            Request::Stats { player, reply } => {
                let _ = reply.send(self.game.lineage_stats(player));
            }
            Request::Chronicle { reply } => {
                let _ = reply.send(Box::new(self.game.chronicle()));
            }
            req @ (Request::Found { .. } | Request::Command { .. }) => self.queued.push(req),
        }
    }

    fn apply(&mut self, req: Request) {
        match req {
            Request::Found { player, name, design, reply } => {
                self.game.set_name(player, &name);
                let _ = reply.send(self.game.found(player, &design));
            }
            Request::Command { conn, player, id, msg } => {
                // A connection that went away before the tick: drop its commands.
                if self.game.conn_player(conn) == Some(player) {
                    let out = self.game.command(conn, player, id, msg);
                    self.dispatch(out);
                }
            }
            other => self.receive(other),
        }
    }

    fn dispatch(&mut self, out: Outbox) {
        for (target, msg) in out {
            let conns: Vec<ConnId> = match target {
                Target::Conn(c) => vec![c],
                Target::Player(p) => self.conns.iter().filter(|(_, (q, _))| *q == p).map(|(&c, _)| c).collect(),
            };
            for c in conns {
                let Some((_, tx)) = self.conns.get(&c) else { continue };
                match tx.try_send(msg.clone()) {
                    Ok(()) => {}
                    // A slow client misses a state frame; the next one replaces it.
                    Err(mpsc::error::TrySendError::Full(ServerMsg::State(_))) => {}
                    // Too far behind on replies and events, or gone: drop it,
                    // which closes its socket.
                    Err(_) => {
                        self.conns.remove(&c);
                        self.game.disconnect(c);
                    }
                }
            }
        }
    }
}
