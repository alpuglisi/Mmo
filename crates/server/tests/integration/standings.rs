//! Cached standings (`evo_server::standings`): the ranked season under way
//! follows score changes and rollovers, finished seasons appear once
//! written (no stale "not ended" or empty list), worlds keep their own, and
//! WebSocket watchers get each change pushed.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use evo_server::game::{GameConfig, WorldSettings};
use evo_server::store::MemoryStore;
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

/// A manual-clock server with worlds `main` and `isle` and 2-day ranked
/// seasons.
async fn server() -> RunningServer {
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
        worlds: vec![WorldSpec::new("main"), WorldSpec { seed: Some(23), size: Some(48), ..WorldSpec::new("isle") }],
        game: GameConfig { season_days: 2, ..Default::default() },
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

/// Register and found a lineage in `world`; the token.
async fn player(addr: SocketAddr, world: &str, name: &str) -> String {
    let creds = json!({"name": name, "password": "a long password"});
    let (s, v) = json_req(addr, Method::POST, "/api/register", None, Some(creds)).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let token = v["token"].as_str().unwrap().to_string();
    let design = json!({
        "name": format!("{name}'s"), "thermo": "warm", "birth": "live", "litter_size": 2,
        "traits": {"body_size": 0.4, "fur": 0.4, "keen_nose": 0.3}, "founders": 10
    });
    let (s, v) = json_req(addr, Method::POST, &format!("/api/lineage?world={world}"), Some(&token), Some(design)).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    token
}

/// Wait (the store writes in the background) until `path` answers `status`.
async fn until_status(addr: SocketAddr, path: &str, status: StatusCode) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (s, v) = json_req(addr, Method::GET, path, None, None).await;
        if s == status {
            return v;
        }
        assert!(tokio::time::Instant::now() < deadline, "{path} still answers {s}: {v}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn until_total(addr: SocketAddr, world: &str, total: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while get(addr, &format!("/api/seasons?world={world}")).await["total"] != total {
        assert!(tokio::time::Instant::now() < deadline, "{world} never listed {total} seasons");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The current season's standings change with each counted day and with
/// the rollover; the finished season's record, the list and the player's
/// history (each read, and so cached, before it existed) show it as soon
/// as it is written; the other world's standings and records don't move.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cached_standings_follow_days_rollovers_and_worlds() {
    let srv = server().await;
    let addr = srv.addr;
    player(addr, "main", "ann").await;
    player(addr, "isle", "bob").await;
    let cur = |w: &'static str| async move { get(addr, &format!("/api/seasons/current?world={w}&player=1")).await };

    let main0 = cur("main").await;
    let isle0 = cur("isle").await;
    assert_eq!((main0["season"].as_u64(), main0["days"].as_u64()), (Some(1), Some(0)), "{main0}");
    // Read (and cache) everything about season 1 before it ends.
    let (s, _) = json_req(addr, Method::GET, "/api/seasons/1?world=main", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(get(addr, "/api/seasons?world=main").await["total"], 0);
    assert!(get(addr, "/api/seasons/player/1?world=main").await["seasons"].as_array().unwrap().is_empty());
    assert_eq!(cur("main").await, main0, "unchanged standings read the same");

    // A counted day changes the standings (legacy shares and days).
    srv.step_world("main", DAY + 1).await;
    let main1 = cur("main").await;
    assert_eq!(main1["season"], 1);
    assert_eq!(main1["days"], 2, "days 0 and 1 counted: {main1}");
    assert_ne!(main1["standings"], main0["standings"], "the day's legacy share is counted");
    let legacy = &main1["standings"][0];
    assert_eq!((legacy["category"].as_str(), legacy["rows"][0]["player"].as_u64()), (Some("legacy"), Some(1)));
    assert_eq!(cur("isle").await, isle0, "the other world's standings don't move");

    // Past the season's end: season 2 is under way, season 1 is stored.
    srv.step_world("main", 2 * DAY).await;
    let main2 = cur("main").await;
    assert_eq!((main2["season"].as_u64(), main2["first_day"].as_u64()), (Some(2), Some(2)), "{main2}");
    let rec = until_status(addr, "/api/seasons/1?world=main&player=1", StatusCode::OK).await;
    assert_eq!(rec["season"], 1);
    assert_eq!(rec["standings"][0]["rows"][0]["player"], 1);
    until_total(addr, "main", 1).await;
    let mine = get(addr, "/api/seasons/player/1?world=main").await;
    assert_eq!(mine["seasons"].as_array().unwrap().len(), 1, "{mine}");

    // The isle still has no finished season, under the same map-less key.
    assert_eq!(cur("isle").await, isle0);
    assert_eq!(get(addr, "/api/seasons?world=isle").await["total"], 0);
    let (s, _) = json_req(addr, Method::GET, "/api/seasons/1?world=isle", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Its own rollover is its own: listed there, main's untouched.
    srv.step_world("isle", 3 * DAY + 1).await;
    let isle_rec = until_status(addr, "/api/seasons/1?world=isle", StatusCode::OK).await;
    until_total(addr, "isle", 1).await;
    assert_ne!(isle_rec["world_id"], rec["world_id"], "each world's own map");
    assert_eq!(get(addr, "/api/seasons?world=main").await["total"], 1);
    assert_eq!(get(addr, "/api/seasons/1?world=main&player=1").await, rec);
    assert_eq!(cur("isle").await["season"], 2);
    srv.shutdown().await;
}

async fn next_text(ws: &mut Ws) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("a message in time")
            .expect("socket open")
            .expect("frame");
        if let Message::Text(t) = msg {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

/// Send a command with `id` and collect the `season` messages that arrive
/// before its result.
async fn seasons_before_result(ws: &mut Ws, id: u64, msg: Value) -> Vec<Value> {
    let mut msg = msg;
    msg["id"] = json!(id);
    ws.send(Message::Text(msg.to_string())).await.unwrap();
    let mut seasons = Vec::new();
    loop {
        let v = next_text(ws).await;
        match v["type"].as_str() {
            Some("season") => seasons.push(v),
            Some("result") if v["id"] == id => {
                assert_eq!(v["ok"], true, "{v}");
                return seasons;
            }
            _ => {}
        }
    }
}

/// `watch_season` sends the standings at once and then each change (a
/// counted day, the rollover), with the watcher's own row; the pushes are
/// the published standings (what `/api/seasons/current` serves); `on:
/// false` stops them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watchers_get_each_change_pushed() {
    let srv = server().await;
    let addr = srv.addr;
    let token = player(addr, "main", "ann").await;
    let (mut ws, _) = connect_async(format!("ws://{addr}/ws?token={token}&world=main")).await.expect("websocket");
    let watch = json!({"type": "watch_season", "on": true, "limit": 3});
    // The answer comes first, then the standings.
    ws.send(Message::Text(json!({"type": "watch_season", "on": true, "limit": 3, "id": 1}).to_string())).await.unwrap();
    let first = loop {
        let v = next_text(&mut ws).await;
        if v["type"] == "season" {
            break v;
        }
    };
    assert_eq!(
        (first["season"].as_u64(), first["days"].as_u64(), first["limit"].as_u64()),
        (Some(1), Some(0), Some(3))
    );

    // Nothing changed: a no-op command brings no push.
    let none = seasons_before_result(&mut ws, 2, json!({"type": "ledger", "limit": 1})).await;
    assert!(none.is_empty(), "{none:?}");
    // Counted days (0 and 1): a push for each, the last the same standings
    // the API serves.
    srv.step_world("main", DAY + 1).await;
    let pushed = seasons_before_result(&mut ws, 3, json!({"type": "ledger", "limit": 1})).await;
    assert_eq!(pushed.iter().map(|p| p["days"].as_u64().unwrap()).collect::<Vec<_>>(), vec![1, 2], "{pushed:?}");
    let mut api = get(addr, "/api/seasons/current?world=main&limit=3&player=1").await;
    api["type"] = json!("season");
    assert_eq!(pushed[1], api);
    assert_eq!(pushed[1]["standings"][0]["rows"][0]["player"], 1, "the watcher's own row");
    // The rollover.
    srv.step_world("main", 2 * DAY).await;
    let pushed = seasons_before_result(&mut ws, 4, json!({"type": "ledger", "limit": 1})).await;
    assert_eq!(pushed.last().unwrap()["season"], 2, "{pushed:?}");
    // Watching again just resends; unwatching stops the pushes.
    let again = seasons_before_result(&mut ws, 5, watch).await;
    assert!(again.is_empty(), "the standings come after the answer");
    assert_eq!(next_text(&mut ws).await["type"], "season");
    seasons_before_result(&mut ws, 6, json!({"type": "watch_season", "on": false})).await;
    srv.step_world("main", 3 * DAY).await;
    let none = seasons_before_result(&mut ws, 7, json!({"type": "ledger", "limit": 1})).await;
    assert!(none.is_empty(), "{none:?}");
    srv.shutdown().await;
}
