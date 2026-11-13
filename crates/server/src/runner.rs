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
//!    the interval is split into `possess_substeps` parts (4 by default):
//!    at each inner boundary the queue is applied (in arrival order) and
//!    possessed organisms with a move order take a movement sub-step
//!    (`Game::substep`), their owners getting a fresh state, so steering
//!    answers within a quarter tick instead of a whole one;
//! 2. apply the queued requests in arrival order;
//! 3. `World::step`, then `Game::after_tick` (mating agreements, offer
//!    expiry, daily records);
//! 4. publish the summary/leaderboard and, when the state interval has
//!    passed, push a state message to every connection;
//! 5. on a new simulated day, save the game: capture a copy of it
//!    (`crate::snapshot::Capture`, a clone) and hand it to the background
//!    saver (`crate::saver`), which encodes it and passes the bytes to the
//!    store task, so encoding never stalls a tick. [`Request::Shutdown`]
//!    saves once more, waits for the saver to encode that last save and
//!    stops the thread.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use evo_sim::{FounderDesign, PlayerId};
use tokio::sync::{mpsc, oneshot, watch};

use crate::game::{ConnId, FoundError, FoundOk, Game, Outbox, PersistEvent, Target};
use crate::protocol::{ChronicleView, ClientMsg, Leaderboard, LineageStats, ServerMsg, WorldSummary};
use crate::saver::Saver;
use crate::snapshot::Capture;
use crate::terrain::{dynamic_raw, terrain_raw, DynamicRaw, TerrainRaw};

pub enum Request {
    Found {
        player: PlayerId,
        name: String,
        design: FounderDesign,
        reply: oneshot::Sender<Result<FoundOk, FoundError>>,
    },
    Connect {
        conn: ConnId,
        player: PlayerId,
        name: String,
        tx: mpsc::Sender<ServerMsg>,
    },
    Disconnect {
        conn: ConnId,
    },
    Command {
        conn: ConnId,
        player: PlayerId,
        id: Option<u64>,
        msg: ClientMsg,
    },
    /// `render_rgb` at `scale` pixels per tile: (width, height, rgb).
    Snapshot {
        scale: usize,
        reply: oneshot::Sender<(usize, usize, Vec<u8>)>,
    },
    /// The tile grid: every layer, or only the dynamic ones.
    Terrain {
        full: bool,
        reply: oneshot::Sender<TerrainReply>,
    },
    /// The genetics dashboard of a player's lineage.
    Stats {
        player: PlayerId,
        reply: oneshot::Sender<LineageStats>,
    },
    /// Species, eras, catastrophes and achievements.
    Chronicle {
        reply: oneshot::Sender<Box<ChronicleView>>,
    },
    /// Save the game and stop the world thread (graceful shutdown). Queued
    /// world changes are dropped. The reply comes once the saver has
    /// encoded the last save and sent it on to the store task.
    Shutdown {
        reply: oneshot::Sender<()>,
    },
    /// Apply the queued requests, advance the world `ticks` ticks and push
    /// state once at the end; replies with the world tick afterwards. Used by
    /// tests in manual mode ([`RunnerConfig::manual`]) to drive the clock.
    Step {
        ticks: u32,
        reply: oneshot::Sender<u64>,
    },
    /// Apply the queued requests and take one possession sub-step
    /// (`Game::substep`, as a ticking server does between ticks); replies
    /// with the sub-steps left before the next tick. Manual mode (tests).
    Substep {
        reply: oneshot::Sender<u32>,
    },
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
    /// Manual clock (tests): the world never ticks on its own, only on
    /// [`Request::Step`], and world-changing requests apply as soon as they
    /// arrive (in arrival order) instead of at the next tick, so a test sees
    /// exactly the ticks it asks for and nothing races a timer.
    pub manual: bool,
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
    /// Day of the last saved game.
    saved_day: u64,
    /// Encodes saves off this thread.
    saver: Saver,
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
        let saved_day = game.world.day();
        let saver = Saver::spawn(persist.clone());
        let runner = Runner {
            game,
            cfg,
            rx,
            conns: BTreeMap::new(),
            queued: Vec::new(),
            published,
            persist,
            last_state: Instant::now(),
            saved_day,
            saver,
        };
        (runner, RunnerHandles { tx, published: published_rx, persist: persist_rx })
    }

    /// Run on a dedicated OS thread until every request sender is dropped.
    pub fn spawn(self) -> std::thread::JoinHandle<()> {
        std::thread::Builder::new().name("world".into()).spawn(move || self.run()).expect("spawn world thread")
    }

    fn run(mut self) {
        if self.cfg.manual {
            return self.run_manual();
        }
        // Possession sub-steps, evenly spaced between ticks (none when
        // running as fast as possible).
        let subs = if self.cfg.tick.is_zero() { 1 } else { self.game.world.config.possess_substeps.max(1) };
        let mut next = Instant::now() + self.cfg.tick;
        loop {
            let start = next.checked_sub(self.cfg.tick).unwrap_or(next);
            for s in 1..subs {
                // Gather until the sub-step is due, apply what came in
                // arrival order (so a move order takes effect now, not at
                // the tick), then move possessed organisms and show their
                // owners.
                if !self.gather(start + self.cfg.tick * s / subs) {
                    return;
                }
                self.apply_queued();
                let out = self.game.substep();
                self.dispatch(out);
            }
            // 1. Gather requests until the tick is due.
            if !self.gather(next) {
                return;
            }
            // 2. Apply world-changing requests in arrival order.
            self.apply_queued();
            // 3. Advance the world; 4. publish.
            let push_state = self.last_state.elapsed() >= self.cfg.state_interval;
            self.tick(push_state);
            let now = Instant::now();
            next += self.cfg.tick;
            if next < now {
                // Running behind (or as fast as possible): don't try to catch up.
                next = now;
            }
        }
    }

    /// Answer reads and queue changes until `due`. False once the thread
    /// should stop (shut down, or every sender dropped).
    fn gather(&mut self, due: Instant) -> bool {
        loop {
            let now = Instant::now();
            let req = if now >= due {
                match self.rx.try_recv() {
                    Ok(r) => r,
                    Err(TryRecvError::Empty) => return true,
                    Err(TryRecvError::Disconnected) => return false,
                }
            } else {
                match self.rx.recv_timeout(due - now) {
                    Ok(r) => r,
                    Err(RecvTimeoutError::Timeout) => return true,
                    Err(RecvTimeoutError::Disconnected) => return false,
                }
            };
            if let Request::Shutdown { reply } = req {
                self.shutdown(reply);
                return false;
            }
            self.receive(req);
        }
    }

    /// The manual-clock loop: wait for requests forever, tick only on `Step`.
    fn run_manual(mut self) {
        while let Ok(req) = self.rx.recv() {
            if let Request::Shutdown { reply } = req {
                return self.shutdown(reply);
            }
            self.receive(req);
            self.apply_queued();
        }
    }

    fn apply_queued(&mut self) {
        for req in std::mem::take(&mut self.queued) {
            self.apply(req);
        }
    }

    /// Advance the world one tick, run the bookkeeping and publish; push
    /// state to every connection if `push_state`.
    fn tick(&mut self, push_state: bool) {
        self.game.world.step();
        let out = self.game.after_tick();
        self.dispatch(out);
        for ev in self.game.take_persist() {
            let _ = self.persist.send(ev);
        }
        self.published
            .send_replace(Arc::new(Published { summary: self.game.summary(), leaderboard: self.game.leaderboard() }));
        if push_state {
            self.last_state = Instant::now();
            let states = self.game.states();
            self.dispatch(states);
        }
        // Save once per simulated day.
        if self.game.world.day() != self.saved_day {
            self.save();
        }
    }

    /// Capture the game and hand it to the saver, which encodes it and
    /// sends it to the store task. The world thread pays only for the copy.
    fn save(&mut self) {
        let started = Instant::now();
        let capture = Capture::new(&self.game);
        self.saved_day = self.game.world.day();
        tracing::info!(
            "captured the game at tick {} (day {}) for saving in {:.1} ms",
            capture.tick(),
            self.saved_day,
            started.elapsed().as_secs_f64() * 1000.0
        );
        self.saver.submit(capture);
    }

    /// Save once more, wait until the saver has encoded it and sent it on
    /// to the store task, then reply.
    fn shutdown(&mut self, reply: oneshot::Sender<()>) {
        self.save();
        self.saver.finish();
        let _ = reply.send(());
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
                let _ = reply.send(if full {
                    TerrainReply::Full(Box::new(terrain_raw(w)))
                } else {
                    TerrainReply::Dynamic(Box::new(dynamic_raw(w)))
                });
            }
            Request::Stats { player, reply } => {
                let _ = reply.send(self.game.lineage_stats(player));
            }
            Request::Chronicle { reply } => {
                let _ = reply.send(Box::new(self.game.chronicle()));
            }
            Request::Step { ticks, reply } => {
                self.apply_queued();
                for i in 0..ticks {
                    self.tick(i + 1 == ticks);
                }
                let _ = reply.send(self.game.world.tick());
            }
            Request::Substep { reply } => {
                self.apply_queued();
                let out = self.game.substep();
                self.dispatch(out);
                let _ = reply.send(self.game.world.substeps_left());
            }
            req @ (Request::Found { .. } | Request::Command { .. }) => self.queued.push(req),
            // Handled by `run` before it gets here.
            Request::Shutdown { reply } => drop(reply),
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
