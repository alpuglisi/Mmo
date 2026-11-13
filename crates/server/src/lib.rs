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
//! - [`terrain`]: the tile grid as compact byte layers for the client map.
//! - [`seasons`]: seasonal leaderboards from the daily history.
//! - [`limits`]: token-bucket rate limits on logins and commands.
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

pub mod accounts;
pub mod appearance;
pub mod design;
pub mod economy;
pub mod game;
pub mod genetics;
pub mod http;
pub mod limits;
pub mod protocol;
pub mod runner;
pub mod saver;
pub mod seasons;
pub mod snapshot;
pub mod store;
pub mod terrain;

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use std::sync::mpsc::SyncSender;

use evo_sim::terrain::Terrain;
use evo_sim::World;
use tokio::sync::{mpsc, oneshot};

use crate::accounts::{Accounts, SessionPolicy};
use crate::game::{build_world, world_config, Game, GameConfig, PersistEvent, SavedGame, WorldSettings, HISTORY_DAYS};
use crate::limits::{Limiters, Limits};
use crate::runner::{Request, Runner, RunnerConfig};
use crate::store::{FileStore, MemoryStore, Store};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    /// Directory for the file store; `None` keeps everything in memory.
    pub data_dir: Option<PathBuf>,
    /// PostgreSQL URL (`postgres://user:pass@host/db`): keep everything in
    /// the database instead of `data_dir` (needs the `postgres` feature).
    pub database_url: Option<String>,
    pub world: WorldSettings,
    pub runner: RunnerConfig,
    pub game: GameConfig,
    /// Serve the files in this directory (the built web client, `client/dist`)
    /// on every path the API doesn't use.
    pub static_dir: Option<PathBuf>,
    /// Rate limits on logins and commands.
    pub limits: Limits,
    /// Session idle timeout and maximum age.
    pub sessions: SessionPolicy,
    /// Ignore saved games: generate a new world and re-found the stored
    /// designs (`--fresh`).
    pub fresh: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 8080)),
            data_dir: Some(PathBuf::from("data")),
            database_url: None,
            world: WorldSettings::default(),
            runner: RunnerConfig {
                tick: Duration::from_secs(1),
                state_interval: Duration::from_secs(1),
                manual: false,
            },
            game: GameConfig::default(),
            static_dir: None,
            limits: Limits::default(),
            sessions: SessionPolicy::default(),
            fresh: false,
        }
    }
}

impl ServerConfig {
    /// Fast mode for tests and demos: 20 ms ticks, state 10 times a second.
    pub fn fast(mut self) -> Self {
        self.runner =
            RunnerConfig { tick: Duration::from_millis(20), state_interval: Duration::from_millis(100), manual: false };
        self
    }

    /// Manual clock for deterministic tests: the world ticks only when the
    /// test calls [`RunningServer::step`], and commands apply on arrival.
    pub fn manual(mut self) -> Self {
        self.runner.manual = true;
        self
    }
}

pub struct RunningServer {
    pub addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    serve: tokio::task::JoinHandle<()>,
    world: SyncSender<Request>,
    persist: tokio::task::JoinHandle<()>,
}

impl RunningServer {
    /// Graceful stop: stop accepting connections, wait for the HTTP server,
    /// save the game one last time, stop the world thread and wait until the
    /// store task has written everything.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = (&mut self.serve).await;
        let (reply, saved) = oneshot::channel();
        let world = self.world.clone();
        let sent =
            tokio::task::spawn_blocking(move || world.send(Request::Shutdown { reply }).is_ok()).await.unwrap_or(false);
        drop(self.world);
        if sent {
            let _ = saved.await;
        }
        let _ = self.persist.await;
    }

    /// Wait until the server stops (e.g. on Ctrl-C).
    pub async fn wait(mut self) {
        let _ = (&mut self.serve).await;
    }

    /// Advance the world `ticks` ticks (after applying anything queued) and
    /// push state to every connection once at the end; returns the world
    /// tick. Meant for [`ServerConfig::manual`] servers, where nothing else
    /// moves the clock; on a ticking server it adds extra ticks.
    pub async fn step(&self, ticks: u32) -> u64 {
        let (reply, rx) = oneshot::channel();
        self.world.send(runner::Request::Step { ticks, reply }).expect("world thread running");
        rx.await.expect("world thread replies")
    }

    /// Take one possession sub-step (after applying anything queued), as a
    /// ticking server does between ticks: possessed organisms with a move
    /// order move a fraction of a tick and their owners get a state. Returns
    /// the sub-steps left before the next tick. For manual servers.
    pub async fn substep(&self) -> u32 {
        let (reply, rx) = oneshot::channel();
        self.world.send(runner::Request::Substep { reply }).expect("world thread running");
        rx.await.expect("world thread replies")
    }

    /// A sender that stops the server when used or dropped.
    pub fn take_shutdown(&mut self) -> Option<oneshot::Sender<()>> {
        self.shutdown.take()
    }
}

/// Load persisted state, restore the world from the newest readable saved
/// game (unless `fresh`) or generate one from the seed, start the world
/// thread and serve. Stored designs whose player has no lineage in the world
/// (all of them in a new world) are founded in player order.
pub async fn start(cfg: ServerConfig) -> Result<RunningServer, BoxError> {
    let store = open_store(&cfg).await?;
    let accounts = store.load_accounts().await?;
    let mut designs = store.load_designs().await?;
    designs.sort_by_key(|d| d.player);

    let restored = if cfg.fresh { None } else { load_saved_game(&*store, &cfg.world).await? };
    // The world `--fresh` discards: its open agreements' escrow is refunded.
    let discarded = if cfg.fresh { load_discarded_game(&*store).await? } else { None };
    let mut game = match restored {
        Some((world, saved)) => {
            let history = store.recent_daily(HISTORY_DAYS).await?;
            Game::resume(world, cfg.game.clone(), saved, history)
        }
        None => Game::new(build_world(&cfg.world), cfg.game.clone()),
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
                tracing::info!("refunded {amount} renown of escrow in the discarded world to player {player}");
            }
            // Write the refunds before the discarded saves go: a crash in
            // between refunds nothing twice (see `Game::refund_discarded`).
            for ev in game.take_persist() {
                persist_one(&*store, &ev, &mut BTreeSet::new()).await?;
            }
        }
        // The discarded world's saves have higher ticks than the new world's
        // will for a long time, and the newest tick is what a restart
        // restores: drop them, or the next plain start would bring the old
        // world (and its escrow, already refunded) back.
        store.clear_snapshots().await?;
    }
    for a in &accounts {
        game.set_name(a.player, &a.name);
    }
    let unfounded: Vec<_> = designs.iter().filter(|d| game.lineage_of(d.player).is_none()).collect();
    for d in unfounded {
        match d.design.to_design() {
            Ok(design) => match game.found(d.player, &design) {
                Ok(ok) => {
                    tracing::info!("re-founded {:?} for player {} as lineage {}", d.design.name, d.player, ok.lineage)
                }
                Err(e) => tracing::warn!("could not re-found the design of player {}: {e:?}", d.player),
            },
            Err(e) => tracing::warn!("stored design of player {} no longer validates: {}", d.player, e.message()),
        }
    }
    let accounts = Arc::new(Accounts::with_policy(store.clone(), accounts, cfg.sessions));

    let (runner, handles) = Runner::new(game, cfg.runner.clone());
    runner.spawn();
    let persist = tokio::spawn(persist_task(store.clone(), handles.persist));

    let state = http::AppState {
        world: handles.tx.clone(),
        accounts,
        store,
        published: handles.published,
        next_conn: Arc::new(AtomicU64::new(1)),
        limiters: Arc::new(Limiters::new(cfg.limits)),
        ws_conns: Default::default(),
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
    tracing::info!("listening on http://{addr}");
    Ok(RunningServer { addr, shutdown: Some(shutdown), serve, world: handles.tx, persist })
}

/// The database if `database_url` is set, else the data directory, else
/// memory.
async fn open_store(cfg: &ServerConfig) -> Result<Arc<dyn Store>, BoxError> {
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
async fn load_terrain(store: &dyn Store, bytes: &[u8]) -> Result<Option<Arc<Terrain>>, String> {
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
async fn load_saved_game(store: &dyn Store, settings: &WorldSettings) -> Result<Option<(World, SavedGame)>, BoxError> {
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
async fn load_discarded_game(store: &dyn Store) -> Result<Option<SavedGame>, BoxError> {
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
    store: &dyn Store,
    ev: &PersistEvent,
    stored: &mut BTreeSet<(String, u64)>,
) -> store::StoreResult<()> {
    match ev {
        PersistEvent::Daily(r) => store.append_daily(r).await,
        PersistEvent::Trade(r) => store.append_trade(r).await,
        PersistEvent::Balance(r) => store.put_balance(r).await,
        PersistEvent::Ledger(r) => store.append_ledger(r).await,
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

async fn persist_task(store: Arc<dyn Store>, mut rx: mpsc::UnboundedReceiver<PersistEvent>) {
    let mut stored = BTreeSet::new();
    while let Some(ev) = rx.recv().await {
        if let Err(e) = persist_one(&*store, &ev, &mut stored).await {
            tracing::error!("could not persist {ev:?}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saved game around the sim's version 4 world snapshot fixture
    /// (terrain embedded; seed 3, 32x32), with empty game state.
    fn v4_saved_game() -> Vec<u8> {
        let world = include_bytes!("../../sim/src/world/snapshot/v4-32x32.snap");
        let mut out = b"EVOSAVE\0".to_vec();
        out.extend_from_slice(&2u64.to_le_bytes());
        out.extend_from_slice(b"{}");
        out.extend_from_slice(world);
        out
    }

    /// A version 4 saved game restores without a stored terrain, and the
    /// first save of the restored world stores its terrain before the saved
    /// game that names it.
    #[tokio::test]
    async fn version_4_saves_restore_and_their_terrain_is_stored_on_the_next_save() {
        let store = MemoryStore::default();
        store.put_snapshot(55, 1, &v4_saved_game()).await.unwrap();
        let settings = WorldSettings { seed: 3, width: 32, height: 32, wild: false, ..Default::default() };
        let (world, saved) = load_saved_game(&store, &settings).await.unwrap().expect("restored");
        assert_eq!(world.tick(), 55);
        let mut game = Game::resume(world, GameConfig::default(), saved, Vec::new());
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

        // The new save names it and restores from the store.
        assert_eq!(store.snapshot_ticks().await.unwrap()[0], 56);
        let (again, _) = load_saved_game(&store, &settings).await.unwrap().expect("restored");
        assert_eq!(again.tick(), 56);
        assert!(again.snapshot() == game.world.snapshot());
    }

    /// Saved games whose terrain the store lacks don't restore, and the
    /// server refuses to start a new world over them.
    #[tokio::test]
    async fn a_missing_terrain_is_an_error_not_a_new_world() {
        let settings = WorldSettings { seed: 4, width: 32, height: 32, wild: false, ..Default::default() };
        let game = Game::new(build_world(&settings), GameConfig::default());
        let bytes = snapshot::encode(&game);
        let store = MemoryStore::default();
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
