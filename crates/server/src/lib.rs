//! Authoritative game server for the Evolution MMO (build step 2 of
//! `docs/game-design.md`).
//!
//! - [`runner`]: the world thread. It owns the `World` and ticks it at a
//!   fixed rate; everything else talks to it through channels.
//! - [`game`]: game state around the world (lineages, mating offers and the
//!   consent flow, per-connection views and interest management).
//! - [`http`]: Axum routes (accounts, founding, leaderboards, terrain,
//!   lineage stats, map PNG) and the WebSocket session.
//! - [`protocol`]: JSON wire types.
//! - [`push`]: each connection's outgoing queue, `state` messages
//!   coalesced so a slow reader gets the latest one and every event.
//! - [`wire`]: protocol 2's binary delta states, encoded per connection
//!   against what it was last sent (and the decoder the tests use).
//! - [`terrain`]: the tile grid as compact byte layers for the client map.
//! - [`seasons`]: leaderboards per weather season from the daily history.
//! - [`league`]: ranked seasons: standings tracked over a fixed number of
//!   days, frozen into stored season records at each rollover.
//! - [`standings`]: caches for leaderboard and season standings reads.
//! - [`limits`]: token-bucket rate limits on logins and commands.
//! - [`metrics`]: tick, state-push and save timings, queue and traffic
//!   counters, process CPU and memory (`GET /api/metrics`).
//! - [`accounts`]: argon2 password hashes and bearer-token sessions with
//!   idle and maximum-age expiry.
//! - [`store`]: persistence behind the [`store::Store`] trait (JSON files
//!   now, PostgreSQL later).
//! - [`snapshot`]: saved games (world snapshot plus game state), written
//!   daily and on shutdown, restored on startup.
//! - [`saver`]: the background thread that encodes saved games, so a save
//!   costs the world thread only a copy of the game.
//! - [`design`]: the JSON form of a founder design.
//! - [`genetics`]: the dashboard's per-gene distributions and allele
//!   origins (whose alleles a lineage carries).
//! - [`appearance`]: the coarse visible body plan sent with each organism
//!   (`look`), for the client's sprites.
//! - [`economy`]: renown balances, the ledger, escrow and the stud
//!   marketplace's listings.
//! - [`worlds`]: the worlds (shards) one process hosts, each with its own
//!   world thread, saver, store task and metrics; accounts are shared.
//! - [`webpush`]: Web Push notifications (VAPID, RFC 8291 encryption,
//!   coalescing and rate limits, delivery with retries) for players whose
//!   game tab is closed, and native push to the apps (FCM, APNs).

pub mod accounts;
pub mod appearance;
pub mod design;
pub mod economy;
pub mod game;
pub mod genetics;
pub mod http;
pub mod league;
pub mod limits;
pub mod metrics;
pub mod protocol;
pub mod push;
pub mod runner;
pub mod saver;
pub mod seasons;
pub mod snapshot;
pub mod standings;
pub mod store;
pub mod terrain;
pub mod webpush;
pub mod wire;
pub mod worlds;

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::sync::mpsc::SyncSender;

use evo_sim::terrain::Terrain;
use evo_sim::World;
use tokio::sync::{mpsc, oneshot};

use crate::accounts::{Accounts, HashPolicy, SessionPolicy};
use crate::game::{build_world, world_config, Game, GameConfig, PersistEvent, SavedGame, WorldSettings, HISTORY_DAYS};
use crate::http::WorldHandle;
use crate::limits::{Limiters, Limits};
use crate::metrics::{Metrics, NetStats};
use crate::runner::{Request, Runner, RunnerConfig};
use crate::store::{AccountRecord, FileStore, MemoryStore, Store, WorldStore};
use crate::worlds::{WorldSpec, DEFAULT_WORLD};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A store handed to the server directly ([`ServerConfig::store`]).
#[derive(Clone)]
pub struct SharedStore(pub Arc<dyn Store>);

impl std::fmt::Debug for SharedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedStore")
    }
}

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    /// Directory for the file store; `None` keeps everything in memory.
    pub data_dir: Option<PathBuf>,
    /// PostgreSQL URL (`postgres://user:pass@host/db`): keep everything in
    /// the database instead of `data_dir` (needs the `postgres` feature).
    pub database_url: Option<String>,
    /// Use this store instead of `database_url` / `data_dir` (tests: a
    /// memory store that outlives one server).
    pub store: Option<SharedStore>,
    /// The settings of every world; each world in `worlds` may replace the
    /// seed and size.
    pub world: WorldSettings,
    /// The worlds this process hosts, the first one the default. Empty: one
    /// world, `main` ([`DEFAULT_WORLD`]), with `world`'s settings.
    pub worlds: Vec<WorldSpec>,
    pub runner: RunnerConfig,
    pub game: GameConfig,
    /// Serve the files in this directory (the built web client, `client/dist`)
    /// on every path the API doesn't use.
    pub static_dir: Option<PathBuf>,
    /// Rate limits on logins and commands.
    pub limits: Limits,
    /// Session idle timeout and maximum age.
    pub sessions: SessionPolicy,
    /// Concurrent password hashes and how many may wait
    /// (`--hash-concurrency`, `--hash-queue`).
    pub hashing: HashPolicy,
    /// Ignore saved games: generate new worlds and re-found the stored
    /// designs (`--fresh`). Applies to every world.
    pub fresh: bool,
    /// Web Push notifications (`--no-push`, `--vapid-*`).
    pub push: webpush::PushConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 8080)),
            data_dir: Some(PathBuf::from("data")),
            database_url: None,
            store: None,
            world: WorldSettings::default(),
            worlds: Vec::new(),
            runner: RunnerConfig {
                tick: Duration::from_secs(1),
                state_interval: Duration::from_secs(1),
                manual: false,
                phase: Duration::ZERO,
            },
            game: GameConfig::default(),
            static_dir: None,
            limits: Limits::default(),
            sessions: SessionPolicy::default(),
            hashing: HashPolicy::default(),
            fresh: false,
            push: webpush::PushConfig::default(),
        }
    }
}

impl ServerConfig {
    /// Fast mode for tests and demos: 20 ms ticks, state 10 times a second.
    pub fn fast(mut self) -> Self {
        self.runner = RunnerConfig {
            tick: Duration::from_millis(20),
            state_interval: Duration::from_millis(100),
            manual: false,
            phase: Duration::ZERO,
        };
        self
    }

    /// Manual clock for deterministic tests: the worlds tick only when the
    /// test calls [`RunningServer::step`], and commands apply on arrival.
    pub fn manual(mut self) -> Self {
        self.runner.manual = true;
        self
    }

    /// The worlds to host: `worlds`, or the single default world.
    pub fn world_specs(&self) -> Vec<WorldSpec> {
        if self.worlds.is_empty() {
            vec![WorldSpec::new(DEFAULT_WORLD)]
        } else {
            self.worlds.clone()
        }
    }
}

/// A running world's handles kept by [`RunningServer`].
struct RunningWorld {
    id: String,
    tx: SyncSender<Request>,
    persist: tokio::task::JoinHandle<()>,
}

pub struct RunningServer {
    pub addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    serve: tokio::task::JoinHandle<()>,
    worlds: Vec<RunningWorld>,
}

impl RunningServer {
    /// Graceful stop: stop accepting connections, wait for the HTTP server,
    /// save every world one last time (in parallel), stop the world threads
    /// and wait until the store tasks have written everything.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = (&mut self.serve).await;
        let mut saved = Vec::new();
        for w in &self.worlds {
            let (reply, rx) = oneshot::channel();
            let world = w.tx.clone();
            let sent = tokio::task::spawn_blocking(move || world.send(Request::Shutdown { reply }).is_ok());
            saved.push((sent, rx));
        }
        for (sent, rx) in saved {
            if sent.await.unwrap_or(false) {
                let _ = rx.await;
            }
        }
        for w in self.worlds.drain(..) {
            drop(w.tx);
            let _ = w.persist.await;
        }
    }

    /// Wait until the server stops (e.g. on Ctrl-C).
    pub async fn wait(mut self) {
        let _ = (&mut self.serve).await;
    }

    /// The ids of the hosted worlds, the default first.
    pub fn world_ids(&self) -> Vec<String> {
        self.worlds.iter().map(|w| w.id.clone()).collect()
    }

    fn world_tx(&self, id: &str) -> &SyncSender<Request> {
        &self.worlds.iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no world {id:?}")).tx
    }

    /// Advance the default world `ticks` ticks (after applying anything
    /// queued) and push state to its connections once at the end; returns
    /// the world tick. Meant for [`ServerConfig::manual`] servers, where
    /// nothing else moves the clock; on a ticking server it adds extra
    /// ticks.
    pub async fn step(&self, ticks: u32) -> u64 {
        self.step_world(&self.worlds[0].id, ticks).await
    }

    /// [`RunningServer::step`] for the world `id`.
    pub async fn step_world(&self, id: &str, ticks: u32) -> u64 {
        let (reply, rx) = oneshot::channel();
        self.world_tx(id).send(runner::Request::Step { ticks, reply }).expect("world thread running");
        rx.await.expect("world thread replies")
    }

    /// Take one possession sub-step of the default world (after applying
    /// anything queued), as a ticking server does between ticks: possessed
    /// organisms with a move order move a fraction of a tick and their
    /// owners get a state. Returns the sub-steps left before the next tick.
    /// For manual servers.
    pub async fn substep(&self) -> u32 {
        let (reply, rx) = oneshot::channel();
        self.worlds[0].tx.send(runner::Request::Substep { reply }).expect("world thread running");
        rx.await.expect("world thread replies")
    }

    /// A sender that stops the server when used or dropped.
    pub fn take_shutdown(&mut self) -> Option<oneshot::Sender<()>> {
        self.shutdown.take()
    }
}

/// Load persisted state, restore every configured world from its newest
/// readable saved game (unless `fresh`) or generate it from its seed, start
/// a world thread per world and serve. Every world is loaded before any
/// starts, so a world that can't load stops the server from starting.
pub async fn start(cfg: ServerConfig) -> Result<RunningServer, BoxError> {
    let specs = cfg.world_specs();
    worlds::validate(&specs)?;
    let store = open_store(&cfg).await?;
    let accounts = store.load_accounts().await?;
    let mut games = Vec::new();
    for spec in &specs {
        let wstore = WorldStore::new(store.clone(), &spec.id);
        let game = load_world(&cfg, spec, &wstore, &accounts)
            .await
            .map_err(|e| -> BoxError { format!("world {:?}: {e}", spec.id).into() })?;
        games.push((spec, wstore, game));
    }
    let accounts = Arc::new(Accounts::with_policies(store.clone(), accounts, cfg.sessions, cfg.hashing));
    let push = webpush::start(store.clone(), cfg.push.clone()).await?.map(Arc::new);

    let mut handles = Vec::new();
    let mut running = Vec::new();
    let n_worlds = games.len();
    for (i, (spec, wstore, game)) in games.into_iter().enumerate() {
        let map_id: Arc<str> = game.world_id().into();
        // Spread the worlds' ticks evenly over the tick (see
        // `RunnerConfig::phase`). Started together, every world ticked in
        // the same instant and they queued for the cores and the shared sim
        // pool: each world's tick grew by ~25 ms per world in the process
        // (see "Several worlds: capacity" in the README).
        let phase = runner::world_phase(cfg.runner.tick, i, n_worlds);
        let rcfg = RunnerConfig { phase, ..cfg.runner.clone() };
        let (mut runner, h) = Runner::new(game, rcfg);
        if let Some(p) = &push {
            runner.set_notifier(p.notifier(&spec.id, &spec.display_name()));
        }
        runner.spawn_named(format!("world-{}", spec.id));
        let persist = tokio::spawn(persist_task(wstore.clone(), h.persist, h.metrics.clone()));
        running.push(RunningWorld { id: spec.id.clone(), tx: h.tx.clone(), persist });
        handles.push(WorldHandle {
            id: spec.id.as_str().into(),
            name: spec.display_name().into(),
            tx: h.tx,
            published: h.published,
            metrics: h.metrics,
            net: Arc::new(NetStats::default()),
            store: wstore,
            map_id,
        });
    }

    let state = http::AppState {
        worlds: Arc::new(handles),
        accounts,
        store,
        next_conn: Arc::new(AtomicU64::new(1)),
        limiters: Arc::new(Limiters::new(cfg.limits)),
        ws_conns: Default::default(),
        push,
    };
    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    let addr = listener.local_addr()?;
    let (shutdown, stop) = oneshot::channel::<()>();
    if let Some(dir) = &cfg.static_dir {
        if !dir.join("index.html").is_file() {
            tracing::warn!("{} has no index.html (run `npm run build` in client/)", dir.display());
        }
    }
    let app = http::router(state, cfg.static_dir.as_deref());
    let serve = tokio::spawn(async move {
        let result = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async move {
                let _ = stop.await;
            })
            .await;
        if let Err(e) = result {
            tracing::error!("server error: {e}");
        }
    });
    tracing::info!(
        "listening on http://{addr} with {} world(s): {}",
        specs.len(),
        specs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ")
    );
    Ok(RunningServer { addr, shutdown: Some(shutdown), serve, worlds: running })
}

/// One world's game: restored from its newest readable saved game (unless
/// `fresh`) or generated from its seed. Stored designs whose player has no
/// lineage in the world (all of them in a new world) are founded in player
/// order.
async fn load_world(
    cfg: &ServerConfig,
    spec: &WorldSpec,
    store: &WorldStore,
    accounts: &[AccountRecord],
) -> Result<Game, BoxError> {
    let settings = spec.settings(&cfg.world);
    let mut designs = store.load_designs().await?;
    designs.sort_by_key(|d| d.player);

    let restored = if cfg.fresh { None } else { load_saved_game(store, &settings).await? };
    // The world `--fresh` discards: its open agreements' escrow is refunded.
    let discarded = if cfg.fresh { load_discarded_game(store).await? } else { None };
    let mut game = match restored {
        Some((world, saved)) => {
            let history = store.recent_daily(HISTORY_DAYS).await?;
            Game::resume(world, cfg.game.clone(), saved, history)
        }
        None => Game::new(build_world(&settings), cfg.game.clone()),
    };
    // A new world, or a saved game from before the economy: balances and
    // recent ledger entries come from the store.
    if !game.economy_restored() {
        let balances: Vec<_> = store.load_balances().await?.into_iter().map(|b| (b.player, b.balance)).collect();
        let ledger = store.recent_ledger(None, 2000).await?.into_iter().map(|r| r.entry).collect();
        game.seed_economy(&balances, ledger);
    }
    if cfg.fresh {
        if let Some(old) = discarded {
            let ledger: Vec<_> = store.recent_ledger(None, DISCARDED_LEDGER_SCAN).await?;
            for (player, amount) in game.refund_discarded(&old, &ledger) {
                tracing::info!(
                    "refunded {amount} renown of escrow in the discarded world {} to player {player}",
                    spec.id
                );
            }
            // Write the refunds before the discarded saves go: a crash in
            // between refunds nothing twice (see `Game::refund_discarded`).
            for ev in game.take_persist() {
                persist_one(store, &ev, &mut BTreeSet::new()).await?;
            }
        }
        // The discarded world's saves have higher ticks than the new world's
        // will for a long time, and the newest tick is what a restart
        // restores: drop them, or the next plain start would bring the old
        // world (and its escrow, already refunded) back.
        store.clear_snapshots().await?;
    }
    // What the restored game has to write before anything else: the last
    // finished ranked season again, in case the store missed it.
    for ev in game.take_persist() {
        if let Err(e) = persist_one(store, &ev, &mut BTreeSet::new()).await {
            tracing::error!("could not persist {ev:?} in world {}: {e}", spec.id);
        }
    }
    for a in accounts {
        game.set_name(a.player, &a.name);
    }
    let unfounded: Vec<_> = designs.iter().filter(|d| game.lineage_of(d.player).is_none()).collect();
    for d in unfounded {
        match d.design.to_design() {
            Ok(design) => match game.found(d.player, &design) {
                Ok(ok) => tracing::info!(
                    "re-founded {:?} for player {} as lineage {} in world {}",
                    d.design.name,
                    d.player,
                    ok.lineage,
                    spec.id
                ),
                Err(e) => tracing::warn!("could not re-found the design of player {} in {}: {e:?}", d.player, spec.id),
            },
            Err(e) => tracing::warn!("stored design of player {} no longer validates: {}", d.player, e.message()),
        }
    }
    Ok(game)
}

/// The store passed in the config, else the database if `database_url` is
/// set, else the data directory, else memory.
async fn open_store(cfg: &ServerConfig) -> Result<Arc<dyn Store>, BoxError> {
    if let Some(s) = &cfg.store {
        return Ok(s.0.clone());
    }
    if let Some(url) = &cfg.database_url {
        #[cfg(feature = "postgres")]
        {
            let store = store::PgStore::connect(url).await?;
            tracing::info!("persisting to PostgreSQL");
            return Ok(Arc::new(store));
        }
        #[cfg(not(feature = "postgres"))]
        {
            let _ = url;
            return Err("a database URL needs evo-server built with the `postgres` feature".into());
        }
    }
    Ok(match &cfg.data_dir {
        Some(dir) => Arc::new(FileStore::open(dir).await?),
        None => Arc::new(MemoryStore::default()),
    })
}

/// The terrain a saved game names, from the store: `Ok(None)` for one that
/// carries its own; an error if it can't be had (missing, unreadable).
async fn load_terrain(store: &WorldStore, bytes: &[u8]) -> Result<Option<Arc<Terrain>>, String> {
    let Some((world_id, hash)) = snapshot::terrain_key(bytes).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let blob = match store.load_terrain(&world_id, hash).await {
        Ok(Some(b)) => b,
        Ok(None) => return Err(format!("its terrain {hash:016x} (world {world_id}) is not in the store")),
        Err(e) => return Err(format!("could not read its terrain {hash:016x}: {e}")),
    };
    let decoded = tokio::task::spawn_blocking(move || Terrain::decode(&blob)).await.map_err(|e| e.to_string())?;
    match decoded {
        Ok(t) if t.hash() == hash => Ok(Some(Arc::new(t))),
        Ok(t) => Err(format!("the stored terrain {hash:016x} hashes to {:016x}", t.hash())),
        Err(e) => Err(format!("its terrain {hash:016x} does not decode: {e}")),
    }
}

/// The newest saved game that decodes, if any (older ones are fallbacks).
/// A saved world for another map or calendar (seed, size, ticks per day,
/// days per year, level of detail) is an error: start with the settings it
/// was made with, or `--fresh`. Other changed settings (balance, caps,
/// catastrophes) apply to the restored world. So is a store whose saved
/// games all name a terrain it lacks: starting a new world would soon
/// replace them.
async fn load_saved_game(store: &WorldStore, settings: &WorldSettings) -> Result<Option<(World, SavedGame)>, BoxError> {
    let wanted = world_config(settings);
    let mut no_terrain = None;
    for tick in store.snapshot_ticks().await? {
        let bytes = match store.load_snapshot(tick).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("could not read the saved game of tick {tick}: {e}");
                continue;
            }
        };
        let started = std::time::Instant::now();
        let terrain = match load_terrain(store, &bytes).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("skipping the saved game of tick {tick}: {e}");
                no_terrain.get_or_insert(format!("the saved game of tick {tick}: {e}"));
                continue;
            }
        };
        let decoded = tokio::task::spawn_blocking(move || snapshot::decode(&bytes, terrain)).await?;
        let (mut world, saved) = match decoded {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("skipping the saved game of tick {tick}: {e}");
                continue;
            }
        };
        if !world.config.snapshot_compatible(&wanted) {
            let c = &world.config;
            return Err(format!(
                "the saved world (seed {}, {}x{}, {} ticks/day, {} days/year, lod {}) does not match the settings; \
                 start with those settings, or with --fresh to begin a new world",
                c.seed, c.width, c.height, c.ticks_per_day, c.days_per_year, c.lod.enabled
            )
            .into());
        }
        if world.config.snapshot_hash() != wanted.snapshot_hash() {
            tracing::info!("world settings changed since the saved game; continuing under the new ones");
            world.config = wanted.clone();
        }
        tracing::info!(
            "restored the world from tick {tick} (day {}, {} organisms) in {:.0} ms",
            world.day(),
            world.population(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        return Ok(Some((world, saved)));
    }
    if let Some(e) = no_terrain {
        return Err(format!(
            "no saved game restores, and {e}; put the terrain back, or start with --fresh to begin a new world"
        )
        .into());
    }
    Ok(None)
}

/// Ledger records read back when `--fresh` refunds a discarded world's
/// escrow: enough to cover everything written after its last saved game.
const DISCARDED_LEDGER_SCAN: usize = 100_000;

/// The game-side state of the newest saved game that decodes (its world is
/// not restored): what `--fresh` discards.
async fn load_discarded_game(store: &WorldStore) -> Result<Option<SavedGame>, BoxError> {
    for tick in store.snapshot_ticks().await? {
        let Ok(bytes) = store.load_snapshot(tick).await else { continue };
        match tokio::task::spawn_blocking(move || snapshot::decode_game(&bytes)).await? {
            Ok(saved) => return Ok(Some(saved)),
            Err(e) => tracing::warn!("skipping the saved game of tick {tick}: {e}"),
        }
    }
    Ok(None)
}

/// Write one event. A saved game's terrain goes first, unless `stored`
/// (world id and hash of the terrains written by this task) says it is in
/// the store already; if it can't be written, neither is the saved game,
/// which would not restore without it.
async fn persist_one(
    store: &WorldStore,
    ev: &PersistEvent,
    stored: &mut BTreeSet<(String, u64)>,
) -> store::StoreResult<()> {
    match ev {
        PersistEvent::Daily(r) => store.append_daily(r).await,
        PersistEvent::Trade(r) => store.append_trade(r).await,
        PersistEvent::Balance(r) => store.put_balance(r).await,
        PersistEvent::Ledger(r) => store.append_ledger(r).await,
        PersistEvent::Auction(r) => store.append_auction(r).await,
        PersistEvent::Season(r) => store.put_season(r).await,
        PersistEvent::Snapshot(s) => {
            let t = &s.terrain;
            let key = (t.world_id.clone(), t.hash);
            if !stored.contains(&key) {
                store.put_terrain(&t.world_id, t.hash, &t.bytes).await?;
                stored.insert(key);
            }
            store.put_snapshot(s.tick, s.day, &s.bytes).await
        }
    }
}

async fn persist_task(store: WorldStore, mut rx: mpsc::UnboundedReceiver<PersistEvent>, metrics: Arc<Mutex<Metrics>>) {
    let mut stored = BTreeSet::new();
    while let Some(ev) = rx.recv().await {
        let started = std::time::Instant::now();
        let result = persist_one(&store, &ev, &mut stored).await;
        if matches!(ev, PersistEvent::Snapshot(_)) {
            let mut m = metrics.lock().unwrap();
            m.snapshots_pending = m.snapshots_pending.saturating_sub(1);
            if result.is_ok() {
                m.saves += 1;
                m.save_write_ms = (started.elapsed().as_secs_f64() * 1000.0) as f32;
            }
        }
        if let Err(e) = result {
            tracing::error!("could not persist {ev:?}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The saver's first save of a world stores its terrain before the
    /// saved game that names it, and that save restores from the store.
    #[tokio::test]
    async fn the_first_save_stores_the_terrain_and_restores() {
        let store = WorldStore::new(Arc::new(MemoryStore::default()), DEFAULT_WORLD);
        let settings = WorldSettings { seed: 3, width: 32, height: 32, wild: false, ..Default::default() };
        let mut game = Game::new(build_world(&settings), GameConfig::default());
        game.world.step();

        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut saver = saver::Saver::spawn(tx);
        saver.submit(snapshot::Capture::new(&game));
        saver.finish();
        let mut stored = BTreeSet::new();
        while let Ok(ev) = rx.try_recv() {
            persist_one(&store, &ev, &mut stored).await.unwrap();
        }
        let hash = game.world.map.terrain().hash();
        let world_id = game.saved().world_id().to_string();
        assert!(stored.contains(&(world_id.clone(), hash)));
        let blob = store.load_terrain(&world_id, hash).await.unwrap().expect("terrain stored");
        assert!(Terrain::decode(&blob).unwrap() == **game.world.map.terrain());

        assert_eq!(store.snapshot_ticks().await.unwrap()[0], game.world.tick());
        let (again, _) = load_saved_game(&store, &settings).await.unwrap().expect("restored");
        assert_eq!(again.tick(), game.world.tick());
        assert!(again.snapshot() == game.world.snapshot());
    }

    /// A saved game from before world snapshot version 6 (no terrain hash in
    /// its header) is skipped like any other format version: no error, and
    /// a new world is generated.
    #[tokio::test]
    async fn saves_of_an_older_format_are_skipped() {
        let settings = WorldSettings { seed: 4, width: 32, height: 32, wild: false, ..Default::default() };
        let game = Game::new(build_world(&settings), GameConfig::default());
        let mut bytes = snapshot::encode(&game);
        // The world snapshot starts after the magic, the JSON length and the
        // JSON; its format version follows its own magic.
        let json = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
        let at = 16 + json + 8;
        bytes[at..at + 4].copy_from_slice(&5u32.to_le_bytes());
        assert_eq!(snapshot::terrain_key(&bytes).unwrap(), None);
        let store = WorldStore::new(Arc::new(MemoryStore::default()), DEFAULT_WORLD);
        store.put_snapshot(game.world.tick(), 0, &bytes).await.unwrap();
        assert!(load_saved_game(&store, &settings).await.unwrap().is_none());
    }

    /// Saved games whose terrain the store lacks don't restore, and the
    /// server refuses to start a new world over them.
    #[tokio::test]
    async fn a_missing_terrain_is_an_error_not_a_new_world() {
        let settings = WorldSettings { seed: 4, width: 32, height: 32, wild: false, ..Default::default() };
        let game = Game::new(build_world(&settings), GameConfig::default());
        let bytes = snapshot::encode(&game);
        let store = WorldStore::new(Arc::new(MemoryStore::default()), DEFAULT_WORLD);
        store.put_snapshot(game.world.tick(), 0, &bytes).await.unwrap();
        let e = load_saved_game(&store, &settings).await.err().expect("refused").to_string();
        assert!(e.contains("terrain") && e.contains("--fresh"), "{e}");
        let decoded = snapshot::decode(&bytes, None).err().expect("refused");
        assert!(
            matches!(decoded, snapshot::DecodeError::World(evo_sim::world::SnapshotError::MissingTerrain { .. })),
            "{decoded}"
        );
        // With the terrain back it restores.
        let t = snapshot::terrain_blob(&game.world, game.saved().world_id());
        store.put_terrain(&t.world_id, t.hash, &t.bytes).await.unwrap();
        assert!(load_saved_game(&store, &settings).await.unwrap().is_some());
    }
}
