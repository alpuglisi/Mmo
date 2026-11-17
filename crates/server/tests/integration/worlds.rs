//! Several worlds in one process: the world list, choosing a world on the
//! API and the WebSocket, and isolation (commands, the market, leaderboards,
//! balances and stored records stay in their world; accounts are shared).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use evo_server::game::WorldSettings;
use evo_server::store::{MemoryStore, Store};
use evo_server::worlds::WorldSpec;
use evo_server::{start, RunningServer, ServerConfig, SharedStore};
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Ticks per simulated day.
const DAY: u32 = 48;

fn two_worlds() -> Vec<WorldSpec> {
    vec![
        WorldSpec { name: Some("Mainland".into()), ..WorldSpec::new("main") },
        WorldSpec { name: Some("The Isle".into()), seed: Some(23), size: Some(48), ..WorldSpec::new("isle") },
    ]
}

/// A manual-clock server hosting `main` (64², seed 11) and `isle` (48²,
/// seed 23) on `store`.
async fn server(store: Arc<dyn Store>) -> RunningServer {
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        store: Some(SharedStore(store)),
        world: WorldSettings {
            seed: 11,
            width: 64,
            height: 64,
            wild: false,
            days_per_year: 4000,
            ..Default::default()
        },
        worlds: two_worlds(),
        ..Default::default()
    }
    .manual();
    start(cfg).await.expect("server starts")
}

async fn json_req(
    addr: SocketAddr,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let mut req = Request::builder().method(method).uri(format!("http://{addr}{path}"));
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Full::new(Bytes::from(v.to_string()))
        }
        None => Full::new(Bytes::new()),
    };
    let res = client.request(req.body(body).unwrap()).await.expect("request");
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn get(addr: SocketAddr, path: &str) -> Value {
    let (s, v) = json_req(addr, Method::GET, path, None, None).await;
    assert_eq!(s, StatusCode::OK, "{path}: {v}");
    v
}

async fn register(addr: SocketAddr, name: &str) -> String {
    let creds = json!({"name": name, "password": "a long password"});
    let (s, v) = json_req(addr, Method::POST, "/api/register", None, Some(creds)).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v["token"].as_str().unwrap().to_string()
}

fn design(name: &str) -> Value {
    json!({
        "name": name,
        "thermo": "warm",
        "birth": "live",
        "litter_size": 2,
        "traits": {"body_size": 0.4, "fur": 0.4, "keen_nose": 0.3},
        "founders": 10
    })
}

async fn found(addr: SocketAddr, world: &str, token: &str, name: &str) -> (StatusCode, Value) {
    json_req(addr, Method::POST, &format!("/api/lineage?world={world}"), Some(token), Some(design(name))).await
}

async fn ws(addr: SocketAddr, world: &str, token: &str) -> Ws {
    let (ws, _) = connect_async(format!("ws://{addr}/ws?token={token}&world={world}")).await.expect("websocket");
    ws
}

async fn wait_for(ws: &mut Ws, what: &str, mut pred: impl FnMut(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let msg = tokio::time::timeout_at(deadline, ws.next())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("socket open")
            .expect("frame");
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if pred(&v) {
                return v;
            }
        }
    }
}

/// Send a command and wait for its result.
async fn command(ws: &mut Ws, id: u64, msg: Value) -> Value {
    let mut msg = msg;
    msg["id"] = json!(id);
    ws.send(Message::Text(msg.to_string())).await.unwrap();
    wait_for(ws, "a result", |v| v["type"] == "result" && v["id"] == id).await
}

/// The ids of the player's own organisms in the next state that has any.
async fn own_ids(ws: &mut Ws) -> Vec<u64> {
    let st = wait_for(ws, "a state with organisms", |v| {
        v["type"] == "state" && v["organisms"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    st["organisms"].as_array().unwrap().iter().map(|o| o["id"].as_u64().unwrap()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worlds_are_listed_and_chosen_per_request() {
    let srv = server(Arc::new(MemoryStore::default())).await;
    assert_eq!(srv.world_ids(), ["main", "isle"]);
    let list = get(srv.addr, "/api/worlds").await;
    assert_eq!(list["default"], "main");
    let worlds = list["worlds"].as_array().unwrap();
    assert_eq!(worlds.iter().map(|w| w["id"].as_str().unwrap()).collect::<Vec<_>>(), ["main", "isle"]);
    assert_eq!(worlds[1]["name"], "The Isle");
    assert_eq!((worlds[0]["width"].as_u64(), worlds[1]["width"].as_u64()), (Some(64), Some(48)));
    assert_eq!(worlds[1]["players_online"], 0);

    // No world named: the default; each world answers for itself.
    assert_eq!(get(srv.addr, "/api/world").await["width"], 64);
    assert_eq!(get(srv.addr, "/api/world?world=main").await["width"], 64);
    assert_eq!(get(srv.addr, "/api/world?world=isle").await["width"], 48);
    let t = get(srv.addr, "/api/terrain?world=isle").await;
    assert_eq!((t["width"].as_u64(), t["height"].as_u64()), (Some(48), Some(48)));
    // The clocks are separate.
    srv.step_world("isle", 5).await;
    assert_eq!(get(srv.addr, "/api/world?world=isle").await["clock"]["tick"], 5);
    assert_eq!(get(srv.addr, "/api/world").await["clock"]["tick"], 0);

    // An unknown world is a 404 everywhere, the WebSocket included.
    for path in ["/api/world?world=nowhere", "/api/leaderboard?world=nowhere", "/api/metrics?world=x"] {
        let (s, v) = json_req(srv.addr, Method::GET, path, None, None).await;
        assert_eq!((s, v["code"].as_str()), (StatusCode::NOT_FOUND, Some("unknown_world")), "{path}: {v}");
    }
    let token = register(srv.addr, "ann").await;
    let e = connect_async(format!("ws://{}/ws?token={token}&world=nowhere", srv.addr)).await.expect_err("refused");
    assert!(e.to_string().contains("404"), "{e}");

    // Metrics per world: connections count where they are.
    let mut sock = ws(srv.addr, "isle", &token).await;
    wait_for(&mut sock, "welcome", |v| v["type"] == "welcome").await;
    srv.step_world("isle", 1).await;
    srv.step(1).await;
    let isle = get(srv.addr, "/api/metrics?world=isle").await;
    let main = get(srv.addr, "/api/metrics").await;
    assert_eq!(
        (isle["world"].as_str(), isle["conns"].as_u64(), isle["players_online"].as_u64()),
        (Some("isle"), Some(1), Some(1))
    );
    assert_eq!((main["world"].as_str(), main["conns"].as_u64()), (Some("main"), Some(0)));
    assert_eq!(get(srv.addr, "/api/worlds").await["worlds"][1]["players_online"], 1);
    drop(sock);
    srv.shutdown().await;
}

/// One account plays in both worlds with a lineage in each; nothing done in
/// one world shows in the other.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commands_market_and_leaderboards_stay_in_their_world() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
    let srv = server(store.clone()).await;
    let addr = srv.addr;
    let ann = register(addr, "ann").await;
    let bob = register(addr, "bob").await;

    // Ann founds in main only; Bob in both. Founding is per world: Bob's
    // second lineage is no `already_founded`.
    assert_eq!(found(addr, "main", &ann, "Ann's").await.0, StatusCode::CREATED);
    assert_eq!(found(addr, "main", &bob, "Bob's").await.0, StatusCode::CREATED);
    assert_eq!(found(addr, "isle", &bob, "Bob's isle").await.0, StatusCode::CREATED);
    let (s, v) = found(addr, "main", &bob, "again").await;
    assert_eq!((s, v["code"].as_str()), (StatusCode::CONFLICT, Some("already_founded")), "{v}");

    // A day in each world, for a leaderboard and income.
    srv.step_world("main", DAY + 1).await;
    srv.step_world("isle", DAY + 1).await;

    // Leaderboards: Ann only in main.
    let names = |board: &Value| -> Vec<String> {
        board["rows"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap().to_string()).collect()
    };
    let main_board = get(addr, "/api/leaderboard?world=main").await;
    let isle_board = get(addr, "/api/leaderboard?world=isle").await;
    assert!(names(&main_board).contains(&"ann".to_string()), "{main_board}");
    assert!(!names(&isle_board).contains(&"ann".to_string()), "{isle_board}");
    assert!(names(&isle_board).contains(&"bob".to_string()), "{isle_board}");
    let history = get(addr, "/api/leaderboard/history?world=isle").await;
    assert!(history.as_array().unwrap().iter().all(|d| d["legacy"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["player"] != 1)));

    // Ann's sockets: welcome in main carries her lineage, in isle none.
    let mut ann_main = ws(addr, "main", &ann).await;
    let w = wait_for(&mut ann_main, "welcome", |v| v["type"] == "welcome").await;
    assert!(w["lineage"].is_u64(), "{w}");
    let mut ann_isle = ws(addr, "isle", &ann).await;
    let w = wait_for(&mut ann_isle, "welcome", |v| v["type"] == "welcome").await;
    assert!(w["lineage"].is_null(), "{w}");
    srv.step(1).await;
    let ann_ids = own_ids(&mut ann_main).await;

    // Commands about Ann's main organisms don't reach them from the isle.
    for (k, &id) in ann_ids.iter().take(3).enumerate() {
        let r = command(&mut ann_isle, 10 + k as u64, json!({"type": "possess", "organism": id})).await;
        assert_eq!(r["ok"], false, "possessed {id} of main from the isle: {r}");
    }
    let r = command(&mut ann_main, 20, json!({"type": "possess", "organism": ann_ids[0]})).await;
    assert_eq!(r["ok"], true, "{r}");

    // The market: a listing in main is not on the isle's market.
    let mut listed = None;
    for (k, &id) in ann_ids.iter().enumerate() {
        let r =
            command(&mut ann_main, 30 + k as u64, json!({"type": "create_listing", "organism": id, "fee": 3})).await;
        if r["ok"] == true {
            listed = r["data"]["listing"]["id"].as_u64();
            break;
        }
    }
    let listed = listed.expect("one of Ann's founders can be listed");
    // And an auction in main (closing in 10 ticks).
    let mut auctioned = None;
    for (k, &id) in ann_ids.iter().enumerate() {
        let msg = json!({"type": "create_auction", "organism": id, "reserve": 1, "ticks": 10});
        let r = command(&mut ann_main, 50 + k as u64, msg).await;
        if r["ok"] == true {
            auctioned = r["data"]["listing"]["id"].as_u64();
            break;
        }
    }
    let auctioned = auctioned.expect("one of Ann's founders can be auctioned");
    let mut bob_main = ws(addr, "main", &bob).await;
    let mut bob_isle = ws(addr, "isle", &bob).await;
    let m = command(&mut bob_main, 1, json!({"type": "browse_market"})).await;
    let main_ids: Vec<u64> =
        m["data"]["listings"].as_array().unwrap().iter().filter_map(|l| l["id"].as_u64()).collect();
    assert!(main_ids.contains(&listed) && main_ids.contains(&auctioned), "{m}");
    let m = command(&mut bob_isle, 1, json!({"type": "browse_market"})).await;
    assert!(m["data"]["listings"].as_array().unwrap().is_empty(), "{m}");
    let m = command(&mut bob_isle, 3, json!({"type": "browse_market", "kind": "auction"})).await;
    assert!(m["data"]["listings"].as_array().unwrap().is_empty(), "{m}");
    // Buying the listing or bidding on the auction by id from the isle
    // fails: the isle has no such listing (Bob has renown to bid there).
    srv.step_world("isle", 1).await;
    let bob_isle_ids = own_ids(&mut bob_isle).await;
    let r = command(&mut bob_isle, 2, json!({"type": "buy_listing", "listing": listed, "mine": bob_isle_ids[0]})).await;
    assert_eq!(r["ok"], false, "{r}");
    let isle_balance = command(&mut bob_isle, 4, json!({"type": "ledger"})).await["data"]["balance"].as_u64().unwrap();
    assert!(isle_balance >= 5, "Bob has {isle_balance} on the isle");
    for (k, &mine) in bob_isle_ids.iter().take(4).enumerate() {
        let msg = json!({"type": "bid", "listing": auctioned, "amount": 5, "mine": mine});
        let r = command(&mut bob_isle, 5 + k as u64, msg).await;
        assert_eq!((r["ok"].as_bool(), r["error"]["code"].as_str()), (Some(false), Some("unknown_listing")), "{r}");
    }
    let r = command(&mut bob_isle, 9, json!({"type": "ledger"})).await;
    assert_eq!(r["data"]["balance"].as_u64(), Some(isle_balance), "nothing escrowed on the isle: {r}");
    // The auction closes in main (unsold: no bids) and is recorded there.
    srv.step_world("main", 12).await;

    // Balances and ledgers are per world: Ann has none on the isle.
    let main_ledger = command(&mut ann_main, 40, json!({"type": "ledger"})).await;
    let isle_ledger = command(&mut ann_isle, 40, json!({"type": "ledger"})).await;
    assert!(main_ledger["data"]["balance"].as_u64().unwrap() > 0, "{main_ledger}");
    assert_eq!(isle_ledger["data"]["balance"], 0, "{isle_ledger}");
    assert!(isle_ledger["data"]["entries"].as_array().unwrap().is_empty(), "{isle_ledger}");
    drop((ann_main, ann_isle, bob_main, bob_isle));
    srv.shutdown().await;

    // The store keeps them apart too.
    let designs = |shard: &'static str| {
        let store = store.clone();
        async move { store.load_designs(shard).await.unwrap().iter().map(|d| d.player).collect::<Vec<_>>() }
    };
    assert_eq!(designs("main").await, [1, 2]);
    assert_eq!(designs("isle").await, [2]);
    let isle_balances = store.load_balances("isle").await.unwrap();
    assert!(isle_balances.iter().all(|b| b.player == 2), "{isle_balances:?}");
    assert!(store.load_balances("main").await.unwrap().iter().any(|b| b.player == 1));
    assert!(store.recent_ledger("isle", Some(1), 100).await.unwrap().is_empty());
    assert!(!store.recent_ledger("main", Some(1), 100).await.unwrap().is_empty());
    let (main_days, isle_days) =
        (store.recent_daily("main", 100).await.unwrap(), store.recent_daily("isle", 100).await.unwrap());
    assert!(!main_days.is_empty() && !isle_days.is_empty());
    assert!(main_days.iter().all(|d| isle_days.iter().all(|e| e.world_id != d.world_id)));
    let main_auctions = store.recent_auctions("main", 100).await.unwrap();
    assert_eq!(main_auctions.iter().map(|a| a.listing).collect::<Vec<_>>(), [auctioned], "{main_auctions:?}");
    assert!(store.recent_auctions("isle", 100).await.unwrap().is_empty());
    assert!(!store.snapshot_ticks("main").await.unwrap().is_empty());
    assert!(!store.snapshot_ticks("isle").await.unwrap().is_empty());

    // A restart restores each world from its own saved game.
    let srv = server(store.clone()).await;
    assert!(get(srv.addr, "/api/world?world=isle").await["clock"]["tick"].as_u64().unwrap() > DAY as u64);
    let (s, v) = found(srv.addr, "isle", &register(srv.addr, "cat").await, "Cat's").await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let login = json!({"name": "bob", "password": "a long password"});
    let (_, v) = json_req(srv.addr, Method::POST, "/api/login", None, Some(login)).await;
    let mut bob_isle = ws(srv.addr, "isle", v["token"].as_str().unwrap()).await;
    let w = wait_for(&mut bob_isle, "welcome", |v| v["type"] == "welcome").await;
    assert!(w["lineage"].is_u64(), "{w}");
    drop(bob_isle);
    srv.shutdown().await;
}

/// A world list that doesn't validate stops the server from starting.
#[tokio::test]
async fn bad_world_lists_are_refused() {
    for worlds in [vec![WorldSpec::new("a"), WorldSpec::new("a")], vec![WorldSpec::new("Bad Id")]] {
        let cfg =
            ServerConfig { bind: SocketAddr::from(([127, 0, 0, 1], 0)), data_dir: None, worlds, ..Default::default() };
        assert!(start(cfg).await.is_err());
    }
}

/// Ranked seasons are per world: a season that ends in one world is not in
/// another's list, the current season is the asked world's, and a finished
/// season is found under its own world only.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ranked_seasons_are_per_world() {
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        store: Some(SharedStore(Arc::new(MemoryStore::default()))),
        world: WorldSettings {
            seed: 11,
            width: 64,
            height: 64,
            wild: false,
            days_per_year: 4000,
            ..Default::default()
        },
        worlds: two_worlds(),
        game: evo_server::game::GameConfig { season_days: 2, ..Default::default() },
        ..Default::default()
    }
    .manual();
    let srv = start(cfg).await.expect("server starts");
    let addr = srv.addr;
    let ann = register(addr, "ann").await;
    assert_eq!(found(addr, "main", &ann, "Ann's").await.0, StatusCode::CREATED);
    // Main runs past its first season; the isle stays at day 0.
    srv.step_world("main", 3 * DAY).await;
    let wait = tokio::time::Instant::now() + Duration::from_secs(10);
    while get(addr, "/api/seasons?world=main").await["total"] == 0 {
        assert!(tokio::time::Instant::now() < wait, "main's season was never stored");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(get(addr, "/api/seasons").await["total"], 1);
    assert_eq!(get(addr, "/api/seasons?world=isle").await["total"], 0);
    let main_cur = get(addr, "/api/seasons/current?world=main").await;
    let isle_cur = get(addr, "/api/seasons/current?world=isle").await;
    assert_ne!(main_cur, isle_cur);
    assert_eq!(get(addr, "/api/seasons/1?world=main").await["season"], 1);
    let (s, v) = json_req(addr, Method::GET, "/api/seasons/1?world=isle", None, None).await;
    assert_eq!((s, v["code"].as_str()), (StatusCode::NOT_FOUND, Some("no_season")), "{v}");
    assert!(!get(addr, "/api/seasons/player/1?world=main").await["seasons"].as_array().unwrap().is_empty());
    assert!(get(addr, "/api/seasons/player/1?world=isle").await["seasons"].as_array().unwrap().is_empty());
    let (s, v) = json_req(addr, Method::GET, "/api/seasons?world=nowhere", None, None).await;
    assert_eq!((s, v["code"].as_str()), (StatusCode::NOT_FOUND, Some("unknown_world")), "{v}");
    srv.shutdown().await;
}
