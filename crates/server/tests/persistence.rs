//! The stores, and a server restart on the same data, against every store:
//! the file store always, PostgreSQL when `DATABASE_URL` names a database
//! (each test in its own schema). Without one the PostgreSQL tests pass
//! after saying they skipped, unless `EVO_REQUIRE_DATABASE` is set (CI).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use evo_server::design::{BirthSpec, DesignSpec, ThermoSpec};
use evo_server::game::WorldSettings;
use evo_server::protocol::{LedgerEntry, LedgerKind};
use evo_server::store::{
    AccountRecord, BalanceRecord, DailyRecord, DesignRecord, FileStore, LedgerRecord, MemoryStore, Store, TradeRecord,
    KEEP_SNAPSHOTS,
};
use evo_server::{start, ServerConfig};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::Message;

fn data_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("evo-server-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Where a test's server keeps its data.
enum Backend {
    File(PathBuf),
    /// A database URL whose connections use a schema of their own.
    #[cfg(feature = "postgres")]
    Pg {
        url: String,
        base: String,
        schema: String,
    },
}

impl Backend {
    fn file(name: &str) -> Self {
        Backend::File(data_dir(name))
    }

    /// A fresh schema in `DATABASE_URL`'s database, or `None` (skip) if the
    /// variable is unset.
    #[cfg(feature = "postgres")]
    async fn pg(name: &str) -> Option<Self> {
        let base = match std::env::var("DATABASE_URL") {
            Ok(u) if !u.is_empty() => u,
            _ => {
                assert!(
                    std::env::var_os("EVO_REQUIRE_DATABASE").is_none(),
                    "EVO_REQUIRE_DATABASE is set but DATABASE_URL is not"
                );
                eprintln!("skipping the PostgreSQL run of {name}: DATABASE_URL is not set");
                return None;
            }
        };
        let schema = format!("evo_test_{name}_{}", std::process::id());
        let pool = sqlx::PgPool::connect(&base).await.expect("connect to DATABASE_URL");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&pool).await.unwrap();
        pool.close().await;
        let sep = if base.contains('?') { '&' } else { '?' };
        let url = format!("{base}{sep}options%5Bsearch_path%5D={schema}");
        Some(Backend::Pg { url, base, schema })
    }

    #[cfg(not(feature = "postgres"))]
    async fn pg(_name: &str) -> Option<Self> {
        None
    }

    fn config(&self, cfg: ServerConfig) -> ServerConfig {
        match self {
            Backend::File(dir) => ServerConfig { data_dir: Some(dir.clone()), database_url: None, ..cfg },
            #[cfg(feature = "postgres")]
            Backend::Pg { url, .. } => ServerConfig { data_dir: None, database_url: Some(url.clone()), ..cfg },
        }
    }

    async fn open(&self) -> Box<dyn Store> {
        match self {
            Backend::File(dir) => Box::new(FileStore::open(dir).await.unwrap()),
            #[cfg(feature = "postgres")]
            Backend::Pg { url, .. } => Box::new(evo_server::store::PgStore::connect(url).await.unwrap()),
        }
    }

    async fn cleanup(self) {
        match self {
            Backend::File(dir) => {
                let _ = std::fs::remove_dir_all(&dir);
            }
            #[cfg(feature = "postgres")]
            Backend::Pg { base, schema, .. } => {
                let pool = sqlx::PgPool::connect(&base).await.unwrap();
                sqlx::query(&format!("DROP SCHEMA {schema} CASCADE")).execute(&pool).await.unwrap();
                pool.close().await;
            }
        }
    }
}

/// `$name` runs `$body(backend)` on the file store; `$pg` on PostgreSQL.
macro_rules! on_every_store {
    ($body:ident, $file:ident, $pg:ident) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn $file() {
            $body(Backend::file(stringify!($file))).await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn $pg() {
            if let Some(b) = Backend::pg(stringify!($body)).await {
                $body(b).await;
            }
        }
    };
}

fn spec() -> DesignSpec {
    DesignSpec {
        name: "Stored".into(),
        thermo: ThermoSpec::Warm,
        birth: BirthSpec::Live,
        litter_size: 2,
        traits: [("fur".to_string(), 0.4)].into_iter().collect(),
        founders: 10,
    }
}

fn day(world: &str, day: u64, adults: usize) -> DailyRecord {
    DailyRecord {
        world_id: world.into(),
        day,
        year: 0,
        season: "spring".into(),
        breeding_adults: adults,
        legacy: vec![evo_server::store::LegacyEntry { player: 1, share: 0.5, weighted_share: 0.25 }],
        lineages: vec![],
    }
}

/// Every record type written to `s`, read back.
async fn round_trip(s: &dyn Store) {
    let a = AccountRecord { name: "ann".into(), player: 1, password_hash: "h".into(), created_unix: 7 };
    s.insert_account(&a).await.unwrap();
    assert!(s.insert_account(&a).await.is_err());
    s.insert_account(&AccountRecord { name: "bob".into(), player: 2, ..a.clone() }).await.unwrap();
    s.put_design(&DesignRecord { player: 1, design: spec(), founded_day: 0, founded_unix: 0 }).await.unwrap();
    let renamed = DesignSpec { name: "Renamed".into(), ..spec() };
    s.put_design(&DesignRecord { player: 1, design: renamed.clone(), founded_day: 3, founded_unix: 9 }).await.unwrap();
    for d in 0..5 {
        s.append_daily(&day("w1", d, 0)).await.unwrap();
    }
    // A restart from an older saved game replays days 3 and 4.
    s.append_daily(&day("w1", 3, 30)).await.unwrap();
    s.append_daily(&day("w1", 4, 40)).await.unwrap();
    s.append_daily(&day("w1", 5, 50)).await.unwrap();
    let trade = TradeRecord {
        offer: 1,
        from_player: 1,
        to_player: 2,
        mine: 10,
        partner: 11,
        price: Some(5),
        status: "done".into(),
        conceived: Some(true),
        day: 4,
        listing: Some(3),
        settlement: Some("paid".into()),
    };
    s.append_trade(&trade).await.unwrap();
    s.append_trade(&TradeRecord {
        offer: 2,
        price: None,
        conceived: None,
        status: "expired".into(),
        listing: None,
        settlement: None,
        ..trade.clone()
    })
    .await
    .unwrap();
    s.put_balance(&BalanceRecord { player: 1, balance: 50, day: 0 }).await.unwrap();
    s.put_balance(&BalanceRecord { player: 2, balance: 9, day: 1 }).await.unwrap();
    s.put_balance(&BalanceRecord { player: 1, balance: 41, day: 3 }).await.unwrap();
    for seq in 1..=4 {
        s.append_ledger(&ledger("w1", seq, if seq % 2 == 1 { 1 } else { 2 }, seq as i64)).await.unwrap();
    }
    // A replayed entry (same world and sequence number) replaces the first.
    s.append_ledger(&ledger("w1", 3, 1, -30)).await.unwrap();
    s.append_ledger(&ledger("w2", 1, 1, 7)).await.unwrap();
    for tick in [100u64, 200, 300, 400] {
        s.put_snapshot(tick, tick / 100, &tick.to_le_bytes()).await.unwrap();
    }
    s.put_snapshot(400, 4, b"newer").await.unwrap();

    let accounts = s.load_accounts().await.unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts.iter().find(|x| x.name == "ann"), Some(&a));
    let designs = s.load_designs().await.unwrap();
    assert_eq!(designs.len(), 1);
    assert_eq!((designs[0].design.clone(), designs[0].founded_day), (renamed, 3));
    let recent = s.recent_daily(3).await.unwrap();
    assert_eq!(recent.iter().map(|r| (r.day, r.breeding_adults)).collect::<Vec<_>>(), vec![(3, 30), (4, 40), (5, 50)]);
    assert_eq!(recent[0], day("w1", 3, 30));
    assert_eq!(s.recent_daily(100).await.unwrap().len(), 6);
    let trades = s.load_trades().await.unwrap();
    assert_eq!(trades.len(), 2);
    assert_eq!(trades[0], trade);
    assert_eq!((trades[1].price, trades[1].conceived, trades[1].settlement.clone()), (None, None, None));
    let mut balances = s.load_balances().await.unwrap();
    balances.sort_by_key(|b| b.player);
    assert_eq!(
        balances,
        vec![BalanceRecord { player: 1, balance: 41, day: 3 }, BalanceRecord { player: 2, balance: 9, day: 1 }]
    );
    let all = s.recent_ledger(None, 100).await.unwrap();
    assert_eq!(
        all.iter().map(|r| (r.world_id.as_str(), r.entry.seq)).collect::<Vec<_>>(),
        vec![("w1", 1), ("w1", 2), ("w1", 3), ("w1", 4), ("w2", 1)]
    );
    assert_eq!(all[2], ledger("w1", 3, 1, -30));
    let ann = s.recent_ledger(Some(1), 2).await.unwrap();
    assert_eq!(
        ann.iter().map(|r| (r.world_id.as_str(), r.entry.seq, r.entry.delta)).collect::<Vec<_>>(),
        vec![("w1", 3, -30), ("w2", 1, 7)]
    );
    assert_eq!(s.recent_ledger(Some(2), 10).await.unwrap().len(), 2);
    let ticks = s.snapshot_ticks().await.unwrap();
    assert_eq!(ticks.len(), KEEP_SNAPSHOTS);
    assert_eq!(ticks, vec![400, 300, 200]);
    assert_eq!(s.load_snapshot(400).await.unwrap(), b"newer");
    assert_eq!(s.load_snapshot(200).await.unwrap(), 200u64.to_le_bytes());
    assert!(s.load_snapshot(100).await.is_err());
}

fn ledger(world: &str, seq: u64, player: u32, delta: i64) -> LedgerRecord {
    LedgerRecord {
        world_id: world.into(),
        entry: LedgerEntry {
            seq,
            player,
            tick: seq * 10,
            day: seq,
            delta,
            balance: 100,
            kind: if delta < 0 { LedgerKind::Escrow } else { LedgerKind::Income },
            text: format!("entry {seq}"),
            offer: (delta < 0).then_some(4),
            listing: None,
        },
    }
}

#[tokio::test]
async fn memory_store_round_trips() {
    round_trip(&MemoryStore::default()).await;
}

/// Round trip, then the same checks through a newly opened store.
async fn store_round_trips(backend: Backend) {
    round_trip(&*backend.open().await).await;
    let s = backend.open().await;
    assert_eq!(s.load_accounts().await.unwrap().len(), 2);
    assert_eq!(s.load_designs().await.unwrap()[0].design.name, "Renamed");
    let recent = s.recent_daily(2).await.unwrap();
    assert_eq!(recent.iter().map(|r| r.day).collect::<Vec<_>>(), vec![4, 5]);
    assert_eq!(s.load_trades().await.unwrap().len(), 2);
    assert_eq!(s.snapshot_ticks().await.unwrap(), vec![400, 300, 200]);
    assert_eq!(s.load_balances().await.unwrap().len(), 2);
    assert_eq!(s.recent_ledger(None, 100).await.unwrap().len(), 5);
    drop(s);
    backend.cleanup().await;
}

on_every_store!(store_round_trips, file_store_round_trips, pg_store_round_trips);

on_every_store!(restart_keeps_accounts_and_refounds_designs, file_restart_keeps_accounts, pg_restart_keeps_accounts);

async fn restart_keeps_accounts_and_refounds_designs(backend: Backend) {
    let cfg = backend.config(
        ServerConfig {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            world: WorldSettings { seed: 5, width: 64, height: 64, wild: false, ..Default::default() },
            ..Default::default()
        }
        .fast(),
    );

    // First run: an account and a design go to disk.
    {
        let srv = start(cfg.clone()).await.unwrap();
        let client = reqwest_like::Client::new(srv.addr);
        let token = client.post_json("/api/register", r#"{"name":"ann","password":"a long password"}"#, None).await;
        let token = token["token"].as_str().unwrap().to_string();
        let body = serde_json::to_string(&spec()).unwrap();
        let v = client.post_json("/api/lineage", &body, Some(&token)).await;
        assert!(v["lineage"].is_u64(), "{v}");
        srv.shutdown().await;
    }
    // Let the world thread and store task wind down.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Second run, ignoring the saved game: log in again; the lineage was
    // re-founded in the new world.
    let srv = start(ServerConfig { fresh: true, ..cfg }).await.unwrap();
    let client = reqwest_like::Client::new(srv.addr);
    let v = client.post_json("/api/login", r#"{"name":"ann","password":"a long password"}"#, None).await;
    assert_eq!(v["player"], 1, "{v}");
    let token = v["token"].as_str().unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws?token={token}", srv.addr)).await.unwrap();
    let mut saw_state = false;
    while let Some(Ok(Message::Text(t))) = ws.next().await {
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "welcome" {
            assert!(v["lineage"].is_u64(), "{v}");
        }
        if v["type"] == "state" {
            assert_eq!(v["organisms"].as_array().unwrap().len(), 10);
            saw_state = true;
            break;
        }
    }
    assert!(saw_state);
    srv.shutdown().await;
    backend.cleanup().await;
}

async fn world_clock(client: &reqwest_like::Client) -> (u64, u64) {
    let v = client.get_json("/api/world").await;
    (v["clock"]["tick"].as_u64().unwrap(), v["clock"]["day"].as_u64().unwrap())
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A player's socket and the lineage in its welcome.
async fn connect(addr: SocketAddr, token: &str) -> (Socket, u64) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?token={token}")).await.unwrap();
    while let Some(Ok(Message::Text(t))) = ws.next().await {
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "welcome" {
            return (ws, v["lineage"].as_u64().expect("lineage"));
        }
    }
    panic!("socket closed before the welcome");
}

/// Own organism ids in the next state message that has any.
async fn own_ids(ws: &mut Socket) -> Vec<u64> {
    while let Some(Ok(Message::Text(t))) = ws.next().await {
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "state" {
            let ids: Vec<u64> = v["organisms"].as_array().unwrap().iter().map(|o| o["id"].as_u64().unwrap()).collect();
            if !ids.is_empty() {
                return ids;
            }
        }
    }
    panic!("socket closed before a state with organisms");
}

on_every_store!(
    restart_continues_the_saved_world,
    file_restart_continues_the_saved_world,
    pg_restart_continues_the_saved_world
);

async fn restart_continues_the_saved_world(backend: Backend) {
    let cfg = backend.config(
        ServerConfig {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            // Level of detail off: with it, nobody connected right after a
            // restart folds the lineage into cohorts, whose members come back
            // with new ids (the sim's snapshot test covers LOD state).
            world: WorldSettings { seed: 9, width: 64, height: 64, wild: true, ..Default::default() },
            ..Default::default()
        }
        .fast(),
    );

    // First run: two players found lineages; the world runs past two days
    // (a saved game each day), then a graceful stop saves it once more.
    let (lineage, ids_before, tick_before) = {
        let srv = start(cfg.clone()).await.unwrap();
        let client = reqwest_like::Client::new(srv.addr);
        let mut tokens = Vec::new();
        for name in ["ann", "bob"] {
            let v = client
                .post_json("/api/register", &format!(r#"{{"name":"{name}","password":"a long password"}}"#), None)
                .await;
            let token = v["token"].as_str().unwrap().to_string();
            let design = DesignSpec { name: format!("{name}'s"), ..spec() };
            let v = client.post_json("/api/lineage", &serde_json::to_string(&design).unwrap(), Some(&token)).await;
            assert!(v["lineage"].is_u64(), "{v}");
            tokens.push(token);
        }
        let (mut ws, lineage) = connect(srv.addr, &tokens[0]).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        while world_clock(&client).await.1 < 2 {
            assert!(tokio::time::Instant::now() < deadline, "world too slow");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let ids = own_ids(&mut ws).await;
        let (tick, _) = world_clock(&client).await;
        drop(ws);
        srv.shutdown().await;
        (lineage, ids, tick)
    };
    assert!(!ids_before.is_empty());

    // Daily saves plus the one at shutdown, the newest few kept.
    let store = backend.open().await;
    let history = store.recent_daily(100).await.unwrap();
    assert!(history.len() >= 2 && history.iter().all(|r| r.world_id == history[0].world_id), "{history:?}");
    let ticks = store.snapshot_ticks().await.unwrap();
    assert!(ticks.len() >= 2, "{ticks:?}");
    let saved_tick = ticks[0];
    assert!(saved_tick >= tick_before, "saved at {saved_tick}, world was at {tick_before}");
    let bytes = store.load_snapshot(saved_tick).await.unwrap();
    assert_eq!(evo_server::snapshot::header(&bytes).unwrap().tick, saved_tick);
    let (saved_world, _) = evo_server::snapshot::decode(&bytes).unwrap();
    drop(store);

    // Second run: the same world carries on from the saved tick, with the
    // same lineage and organisms, instead of a regenerated one.
    let srv = start(cfg.clone()).await.unwrap();
    let client = reqwest_like::Client::new(srv.addr);
    let (tick, day) = world_clock(&client).await;
    assert!(tick >= saved_tick && day >= 2, "restarted at tick {tick} day {day}, saved at {saved_tick}");
    let v = client.post_json("/api/login", r#"{"name":"ann","password":"a long password"}"#, None).await;
    let token = v["token"].as_str().unwrap().to_string();
    let (mut ws, lineage_after) = connect(srv.addr, &token).await;
    let ids_after = own_ids(&mut ws).await;
    drop(ws);
    assert_eq!(lineage_after, lineage);
    let saved_ids: std::collections::BTreeSet<u64> =
        saved_world.organisms.iter().filter(|o| o.alive && o.lineage as u64 == lineage).map(|o| o.id).collect();
    assert!(ids_after.iter().any(|id| saved_ids.contains(id)), "none of {ids_after:?} in the saved world");
    assert!(ids_before.iter().any(|id| ids_after.contains(id)), "{ids_before:?} vs {ids_after:?}");
    // Founding again is refused: the lineage exists.
    let again = client.post_json("/api/lineage", &serde_json::to_string(&spec()).unwrap(), Some(&token)).await;
    assert_eq!(again["code"], "already_founded", "{again}");
    srv.shutdown().await;
    // The continued world kept its id: one history record per day.
    let store = backend.open().await;
    let history = store.recent_daily(100).await.unwrap();
    let days: Vec<u64> = history.iter().map(|r| r.day).collect();
    assert!(days.windows(2).all(|w| w[0] < w[1]), "{days:?}");
    assert!(history.iter().all(|r| r.world_id == history[0].world_id));
    drop(store);

    // --fresh ignores the saved game: a new world from tick 0.
    let srv = start(ServerConfig { fresh: true, ..cfg.clone() }).await.unwrap();
    let client = reqwest_like::Client::new(srv.addr);
    let (tick, _) = world_clock(&client).await;
    assert!(tick < saved_tick, "fresh world at tick {tick}");
    srv.shutdown().await;
    // The discarded world's saves are gone: a plain restart continues the
    // fresh world, not the older one with higher ticks.
    let srv = start(cfg).await.unwrap();
    let client = reqwest_like::Client::new(srv.addr);
    let (tick, _) = world_clock(&client).await;
    assert!(tick < saved_tick, "restarted the discarded world (tick {tick})");
    srv.shutdown().await;
    backend.cleanup().await;
}

async fn next_state(ws: &mut Socket) -> serde_json::Value {
    while let Some(Ok(Message::Text(t))) = ws.next().await {
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "state" {
            return v;
        }
    }
    panic!("socket closed before a state");
}

/// The renown balance and escrow in the next state message.
async fn balance(ws: &mut Socket) -> (u64, u64) {
    let v = next_state(ws).await;
    (v["balance"].as_u64().unwrap(), v["escrow"].as_u64().unwrap())
}

on_every_store!(restart_keeps_balances, file_restart_keeps_balances, pg_restart_keeps_balances);

/// Balances survive a restart from the saved game (with the escrow of an
/// open agreement), and a `--fresh` world starts from the balances in the
/// store without paying the founding grant again.
async fn restart_keeps_balances(backend: Backend) {
    let cfg = backend.config(
        ServerConfig {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            world: WorldSettings {
                seed: 11,
                width: 64,
                height: 64,
                wild: false,
                days_per_year: 4000,
                ..Default::default()
            },
            ..Default::default()
        }
        .manual(),
    );
    let grant = evo_server::economy::FOUNDING_GRANT;
    let (ann_before, offer) = {
        let srv = start(cfg.clone()).await.unwrap();
        let client = reqwest_like::Client::new(srv.addr);
        let mut tokens = Vec::new();
        for name in ["ann", "bob"] {
            let v = client
                .post_json("/api/register", &format!(r#"{{"name":"{name}","password":"a long password"}}"#), None)
                .await;
            let token = v["token"].as_str().unwrap().to_string();
            let design = DesignSpec { name: format!("{name}'s"), ..spec() };
            let v = client.post_json("/api/lineage", &serde_json::to_string(&design).unwrap(), Some(&token)).await;
            assert!(v["lineage"].is_u64(), "{v}");
            tokens.push(token);
        }
        // Two days of income.
        srv.step(2 * 48 + 1).await;
        let (mut ws, _) = connect(srv.addr, &tokens[0]).await;
        let st = next_state(&mut ws).await;
        let (ann, escrow) = (st["balance"].as_u64().unwrap(), st["escrow"].as_u64().unwrap());
        assert!(ann > grant && escrow == 0, "ann has {ann}");
        // Ann asks for one of Bob's organisms at a price: escrowed.
        let mine: Vec<u64> = st["organisms"].as_array().unwrap().iter().map(|o| o["id"].as_u64().unwrap()).collect();
        let (mut bws, _) = connect(srv.addr, &tokens[1]).await;
        let theirs = own_ids(&mut bws).await;
        let mut offer = None;
        'pairs: for &m in &mine {
            for &p in &theirs {
                let msg = serde_json::json!({"type": "request_mating", "id": 1, "mine": m, "partner": p, "price": 5});
                futures_util::SinkExt::send(&mut ws, Message::Text(msg.to_string())).await.unwrap();
                while let Some(Ok(Message::Text(t))) = ws.next().await {
                    let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                    if v["type"] == "result" {
                        if v["ok"] == true {
                            offer = v["data"]["offer"]["id"].as_u64();
                            break 'pairs;
                        }
                        break;
                    }
                }
            }
        }
        let offer = offer.expect("some pair can be asked");
        srv.step(1).await;
        assert_eq!(balance(&mut ws).await, (ann - 5, 5));
        drop((ws, bws));
        srv.shutdown().await;
        (ann - 5, offer)
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let store = backend.open().await;
    let stored: Vec<BalanceRecord> = store.load_balances().await.unwrap();
    assert_eq!(stored.iter().find(|b| b.player == 1).map(|b| b.balance), Some(ann_before));
    let ledger = store.recent_ledger(Some(1), 100).await.unwrap();
    assert_eq!(ledger.first().map(|r| r.entry.kind), Some(LedgerKind::Grant));
    assert_eq!(ledger.last().map(|r| (r.entry.kind, r.entry.offer)), Some((LedgerKind::Escrow, Some(offer))));
    drop(store);

    // From the saved game: the same balance and the escrowed agreement.
    {
        let srv = start(cfg.clone()).await.unwrap();
        let client = reqwest_like::Client::new(srv.addr);
        let v = client.post_json("/api/login", r#"{"name":"ann","password":"a long password"}"#, None).await;
        let (mut ws, _) = connect(srv.addr, v["token"].as_str().unwrap()).await;
        assert_eq!(balance(&mut ws).await, (ann_before, 5));
        drop(ws);
        srv.shutdown().await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    // A fresh world: balances from the store, no second founding grant, and
    // the escrow of the discarded world's open agreement refunded.
    let srv = start(ServerConfig { fresh: true, ..cfg.clone() }).await.unwrap();
    let client = reqwest_like::Client::new(srv.addr);
    let v = client.post_json("/api/login", r#"{"name":"ann","password":"a long password"}"#, None).await;
    let (mut ws, _) = connect(srv.addr, v["token"].as_str().unwrap()).await;
    assert_eq!(balance(&mut ws).await, (ann_before + 5, 0));
    drop(ws);
    srv.shutdown().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let store = backend.open().await;
    let last = store.recent_ledger(Some(1), 1).await.unwrap().pop().expect("a refund entry").entry;
    assert_eq!((last.kind, last.delta, last.balance, last.offer), (LedgerKind::Refund, 5, ann_before + 5, None));
    assert!(last.text.contains(&format!("offer #{offer} of world")), "{}", last.text);
    let stored = store.load_balances().await.unwrap();
    assert_eq!(stored.iter().find(|b| b.player == 1).map(|b| b.balance), Some(ann_before + 5));
    drop(store);

    // The fresh world saved on shutdown: starting fresh again discards that
    // one (no open agreements), so nothing is refunded twice.
    let srv = start(ServerConfig { fresh: true, ..cfg }).await.unwrap();
    let client = reqwest_like::Client::new(srv.addr);
    let v = client.post_json("/api/login", r#"{"name":"ann","password":"a long password"}"#, None).await;
    let (mut ws, _) = connect(srv.addr, v["token"].as_str().unwrap()).await;
    assert_eq!(balance(&mut ws).await, (ann_before + 5, 0));
    drop(ws);
    srv.shutdown().await;
    backend.cleanup().await;
}

/// A tiny JSON-over-HTTP helper on hyper.
mod reqwest_like {
    use std::net::SocketAddr;

    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;
    use hyper_util::client::legacy::Client as Hyper;
    use hyper_util::rt::TokioExecutor;

    pub struct Client {
        addr: SocketAddr,
        inner: Hyper<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>,
    }

    impl Client {
        pub fn new(addr: SocketAddr) -> Self {
            Self { addr, inner: Hyper::builder(TokioExecutor::new()).build_http() }
        }

        pub async fn post_json(&self, path: &str, body: &str, token: Option<&str>) -> serde_json::Value {
            let mut req =
                hyper::Request::post(format!("http://{}{path}", self.addr)).header("content-type", "application/json");
            if let Some(t) = token {
                req = req.header("authorization", format!("Bearer {t}"));
            }
            let res = self.inner.request(req.body(Full::new(Bytes::from(body.to_string()))).unwrap()).await.unwrap();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice(&bytes).unwrap()
        }

        pub async fn get_json(&self, path: &str) -> serde_json::Value {
            let req = hyper::Request::get(format!("http://{}{path}", self.addr));
            let res = self.inner.request(req.body(Full::new(Bytes::new())).unwrap()).await.unwrap();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice(&bytes).unwrap()
        }
    }
}
