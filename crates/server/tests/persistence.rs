//! The file store, and a server restart on the same data directory.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use evo_server::design::{BirthSpec, DesignSpec, ThermoSpec};
use evo_server::game::WorldSettings;
use evo_server::store::{AccountRecord, DailyRecord, DesignRecord, FileStore, Store};
use evo_server::{start, ServerConfig};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::Message;

fn data_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("evo-server-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
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

#[tokio::test]
async fn file_store_round_trips() {
    let dir = data_dir("store");
    {
        let s = FileStore::open(&dir).await.unwrap();
        let a = AccountRecord { name: "ann".into(), player: 1, password_hash: "h".into(), created_unix: 0 };
        s.insert_account(&a).await.unwrap();
        assert!(s.insert_account(&a).await.is_err());
        s.put_design(&DesignRecord { player: 1, design: spec(), founded_day: 0, founded_unix: 0 }).await.unwrap();
        for day in 0..5 {
            let r = DailyRecord { day, year: 0, season: "spring".into(), breeding_adults: 0, legacy: vec![], lineages: vec![] };
            s.append_daily(&r).await.unwrap();
        }
    }
    let s = FileStore::open(&dir).await.unwrap();
    assert_eq!(s.load_accounts().await.unwrap().len(), 1);
    assert_eq!(s.load_designs().await.unwrap()[0].design, spec());
    let recent = s.recent_daily(2).await.unwrap();
    assert_eq!(recent.iter().map(|r| r.day).collect::<Vec<_>>(), vec![3, 4]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_keeps_accounts_and_refounds_designs() {
    let dir = data_dir("restart");
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: Some(dir.clone()),
        world: WorldSettings { seed: 5, width: 64, height: 64, wild: false, ..Default::default() },
        ..Default::default()
    }
    .fast();

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
    let _ = std::fs::remove_dir_all(&dir);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_continues_the_saved_world() {
    let dir = data_dir("continue");
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: Some(dir.clone()),
        // Level of detail off: with it, nobody connected right after a restart
        // folds the lineage into cohorts, whose members come back with new
        // ids (the sim's snapshot test covers LOD state).
        world: WorldSettings { seed: 9, width: 64, height: 64, wild: true, ..Default::default() },
        ..Default::default()
    }
    .fast();

    // First run: two players found lineages; the world runs past two days
    // (a saved game each day), then a graceful stop saves it once more.
    let (lineage, ids_before, tick_before) = {
        let srv = start(cfg.clone()).await.unwrap();
        let client = reqwest_like::Client::new(srv.addr);
        let mut tokens = Vec::new();
        for name in ["ann", "bob"] {
            let v = client.post_json("/api/register", &format!(r#"{{"name":"{name}","password":"a long password"}}"#), None).await;
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
    let store = FileStore::open(&dir).await.unwrap();
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

    // --fresh ignores the saved game: a new world from tick 0.
    let srv = start(ServerConfig { fresh: true, ..cfg }).await.unwrap();
    let client = reqwest_like::Client::new(srv.addr);
    let (tick, _) = world_clock(&client).await;
    assert!(tick < saved_tick, "fresh world at tick {tick}");
    srv.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
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
            let mut req = hyper::Request::post(format!("http://{}{path}", self.addr)).header("content-type", "application/json");
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
