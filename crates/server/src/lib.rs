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
//! - [`design`]: the JSON form of a founder design.

pub mod accounts;
pub mod design;
pub mod game;
pub mod http;
pub mod limits;
pub mod protocol;
pub mod runner;
pub mod seasons;
pub mod store;
pub mod terrain;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::accounts::{Accounts, SessionPolicy};
use crate::limits::{Limiters, Limits};
use crate::game::{build_world, Game, GameConfig, PersistEvent, WorldSettings};
use crate::runner::{Runner, RunnerConfig};
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
    world: std::sync::mpsc::SyncSender<runner::Request>,
    shutdown: Option<oneshot::Sender<()>>,
    serve: tokio::task::JoinHandle<()>,
}

impl RunningServer {
    /// Stop accepting connections and wait for the HTTP server to finish.
    /// The world thread stops once the last handler lets go of it.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = (&mut self.serve).await;
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

/// Load persisted state, build the world, start the world thread and serve.
///
/// World state is not persisted yet, so every start generates a fresh world
/// from the seed and re-founds every stored design in player order.
pub async fn start(cfg: ServerConfig) -> Result<RunningServer, BoxError> {
    let store: Arc<dyn Store> = match &cfg.data_dir {
        Some(dir) => Arc::new(FileStore::open(dir).await?),
        None => Arc::new(MemoryStore::default()),
    };
    let accounts = store.load_accounts().await?;
    let mut designs = store.load_designs().await?;
    designs.sort_by_key(|d| d.player);

    let world = build_world(&cfg.world);
    let mut game = Game::new(world, cfg.game.clone());
    for a in &accounts {
        game.set_name(a.player, &a.name);
    }
    for d in &designs {
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
    tokio::spawn(persist_task(store.clone(), handles.persist));

    let world = handles.tx.clone();
    let state = http::AppState {
        world: handles.tx,
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
    Ok(RunningServer { addr, world, shutdown: Some(shutdown), serve })
}

async fn persist_task(store: Arc<dyn Store>, mut rx: mpsc::UnboundedReceiver<PersistEvent>) {
    while let Some(ev) = rx.recv().await {
        let result = match &ev {
            PersistEvent::Daily(r) => store.append_daily(r).await,
            PersistEvent::Trade(r) => store.append_trade(r).await,
        };
        if let Err(e) = result {
            tracing::error!("could not persist {ev:?}: {e}");
        }
    }
}
