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
//! - [`design`]: the JSON form of a founder design.

pub mod accounts;
pub mod appearance;
pub mod design;
pub mod game;
pub mod http;
pub mod limits;
pub mod protocol;
pub mod runner;
pub mod seasons;
pub mod snapshot;
pub mod store;
pub mod terrain;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use std::sync::mpsc::SyncSender;

use evo_sim::World;
use tokio::sync::{mpsc, oneshot};

use crate::accounts::{Accounts, SessionPolicy};
use crate::limits::{Limiters, Limits};
use crate::game::{build_world, world_config, Game, GameConfig, PersistEvent, SavedGame, WorldSettings, HISTORY_DAYS};
use crate::runner::{Request, Runner, RunnerConfig};
use crate::store::{FileStore, MemoryStore, Store};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    /// Directory for the file store; `None` keeps everything in memory.
    pub data_dir: Option<PathBuf>,
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
            world: WorldSettings::default(),
            runner: RunnerConfig { tick: Duration::from_secs(1), state_interval: Duration::from_secs(1), manual: false },
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
        self.runner = RunnerConfig { tick: Duration::from_millis(20), state_interval: Duration::from_millis(100), manual: false };
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
        let sent = tokio::task::spawn_blocking(move || world.send(Request::Shutdown { reply }).is_ok()).await.unwrap_or(false);
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
    let store: Arc<dyn Store> = match &cfg.data_dir {
        Some(dir) => Arc::new(FileStore::open(dir).await?),
        None => Arc::new(MemoryStore::default()),
    };
    let accounts = store.load_accounts().await?;
    let mut designs = store.load_designs().await?;
    designs.sort_by_key(|d| d.player);

    let restored = if cfg.fresh { None } else { load_saved_game(&*store, &cfg.world).await? };
    let mut game = match restored {
        Some((world, saved)) => {
            let history = store.recent_daily(HISTORY_DAYS).await?;
            Game::resume(world, cfg.game.clone(), saved, history)
        }
        None => Game::new(build_world(&cfg.world), cfg.game.clone()),
    };
    for a in &accounts {
        game.set_name(a.player, &a.name);
    }
    let unfounded: Vec<_> = designs.iter().filter(|d| game.lineage_of(d.player).is_none()).collect();
    for d in unfounded {
        match d.design.to_design() {
            Ok(design) => match game.found(d.player, &design) {
                Ok(ok) => tracing::info!("re-founded {:?} for player {} as lineage {}", d.design.name, d.player, ok.lineage),
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

/// The newest saved game that decodes, if any (older ones are fallbacks).
/// A saved world for another map or calendar (seed, size, ticks per day,
/// days per year, level of detail) is an error: start with the settings it
/// was made with, or `--fresh`. Other changed settings (balance, caps,
/// catastrophes) apply to the restored world.
async fn load_saved_game(store: &dyn Store, settings: &WorldSettings) -> Result<Option<(World, SavedGame)>, BoxError> {
    let wanted = world_config(settings);
    for tick in store.snapshot_ticks().await? {
        let bytes = match store.load_snapshot(tick).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("could not read the saved game of tick {tick}: {e}");
                continue;
            }
        };
        let started = std::time::Instant::now();
        let decoded = tokio::task::spawn_blocking(move || snapshot::decode(&bytes)).await?;
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
    Ok(None)
}

async fn persist_task(store: Arc<dyn Store>, mut rx: mpsc::UnboundedReceiver<PersistEvent>) {
    while let Some(ev) = rx.recv().await {
        let result = match &ev {
            PersistEvent::Daily(r) => store.append_daily(r).await,
            PersistEvent::Trade(r) => store.append_trade(r).await,
            PersistEvent::Snapshot(s) => store.put_snapshot(s.tick, &s.bytes).await,
        };
        if let Err(e) = result {
            tracing::error!("could not persist {ev:?}: {e}");
        }
    }
}
