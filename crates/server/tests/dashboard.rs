//! End-to-end: the terrain, lineage-stats and seasonal-leaderboard endpoints,
//! rate limits and session expiry.

use std::net::SocketAddr;
use std::time::Duration;

use base64::Engine;
use evo_server::accounts::SessionPolicy;
use evo_server::game::WorldSettings;
use evo_server::limits::{Limits, Rate};
use evo_server::{start, RunningServer, ServerConfig};
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

fn config() -> ServerConfig {
    ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        world: WorldSettings { seed: 11, width: 64, height: 48, wild: true, days_per_year: 8, lod: false },
        ..Default::default()
    }
    .fast()
}

async fn server(cfg: ServerConfig) -> RunningServer {
    start(cfg).await.expect("server starts")
}

async fn http(addr: SocketAddr, method: Method, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, hyper::HeaderMap, Bytes) {
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
    let (status, headers) = (res.status(), res.headers().clone());
    (status, headers, res.into_body().collect().await.unwrap().to_bytes())
}

async fn json_req(addr: SocketAddr, method: Method, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let (s, _, b) = http(addr, method, path, token, body).await;
    (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

async fn register(addr: SocketAddr, name: &str) -> String {
    let (s, v) = json_req(addr, Method::POST, "/api/register", None, Some(json!({"name": name, "password": "a long password"}))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v["token"].as_str().unwrap().to_string()
}

async fn found(addr: SocketAddr, token: &str, name: &str) {
    let design = json!({"name": name, "thermo": "warm", "birth": "live", "litter_size": 2,
        "traits": {"body_size": 0.4, "fur": 0.4}, "founders": 12});
    let (s, v) = json_req(addr, Method::POST, "/api/lineage", Some(token), Some(design)).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
}

async fn ws(addr: SocketAddr, token: &str) -> Ws {
    connect_async(format!("ws://{addr}/ws?token={token}")).await.expect("websocket").0
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

fn decode(v: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(v.as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terrain_stats_and_seasons() {
    let srv = server(config()).await;
    let addr = srv.addr;

    // Terrain: every layer has one byte per tile.
    let (s, t) = json_req(addr, Method::GET, "/api/terrain", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!((t["width"].as_u64(), t["height"].as_u64()), (Some(64), Some(48)));
    for k in ["biome", "elevation", "river", "base_water"] {
        assert_eq!(decode(&t[k]).len(), 64 * 48, "{k}");
    }
    assert!(decode(&t["biome"]).iter().all(|&b| b < 8));
    for k in ["water", "ice", "snow", "veg", "wetness"] {
        assert_eq!(decode(&t["dynamic"][k]).len(), 64 * 48, "{k}");
    }
    let (s, d) = json_req(addr, Method::GET, "/api/terrain/dynamic", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(d["tick"].as_u64().unwrap() >= t["dynamic"]["tick"].as_u64().unwrap());
    assert!(d.get("biome").is_none(), "the dynamic layer leaves the fixed layers out");
    assert_eq!(decode(&d["wetness"]).len(), 64 * 48);

    // Lineage stats need a session; without a lineage they are empty.
    let (s, _) = json_req(addr, Method::GET, "/api/lineage/stats", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let ann = register(addr, "ann").await;
    let (s, v) = json_req(addr, Method::GET, "/api/lineage/stats", Some(&ann), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["lineage"].is_null() && v["alive"] == 0);
    found(addr, &ann, "Ann's").await;
    // Wait for two simulated days (about a second each in fast mode).
    let mut stats = Value::Null;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stats = json_req(addr, Method::GET, "/api/lineage/stats", Some(&ann), None).await.1;
        if stats["history_days"].as_array().is_some_and(|d| d.len() >= 2) {
            break;
        }
    }
    assert!(stats["history_days"].as_array().unwrap().len() >= 2, "{stats}");
    assert_eq!(stats["name"], "Ann's");
    assert!(stats["alive"].as_u64().unwrap() > 0);
    let bands: u64 = stats["age_bands"].as_array().unwrap().iter().map(|b| b["female"].as_u64().unwrap() + b["male"].as_u64().unwrap()).sum();
    assert_eq!(bands, stats["alive"].as_u64().unwrap());
    let genes = stats["genes"].as_array().unwrap();
    assert!(genes.len() > 30);
    let fur = genes.iter().find(|g| g["gene"] == "fur").unwrap();
    assert_eq!(fur["history"].as_array().unwrap().len(), stats["history_days"].as_array().unwrap().len());
    assert!(!stats["vitals"].as_array().unwrap().is_empty());
    assert!(!stats["legacy"].as_array().unwrap().is_empty());

    // Seasonal boards from the stored history (8-day years: 2-day seasons).
    let (s, boards) = json_req(addr, Method::GET, "/api/leaderboard/seasons?limit=4", None, None).await;
    assert_eq!(s, StatusCode::OK);
    let boards = boards.as_array().unwrap();
    assert!(!boards.is_empty() && boards.len() <= 4);
    assert_eq!(boards[0]["current"], true, "{boards:?}");
    let rows = boards[0]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["name"] == "ann") && rows.iter().any(|r| r["name"] == "wild"), "{rows:?}");
    let means: Vec<f64> = rows.iter().map(|r| r["mean_share"].as_f64().unwrap()).collect();
    assert!(means.windows(2).all(|w| w[0] >= w[1]));
    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logins_and_commands_are_rate_limited() {
    let mut cfg = config();
    cfg.limits = Limits { logins: Rate::new(0.01, 3.0), commands: Rate::new(0.01, 5.0), ip_commands: Rate::UNLIMITED };
    let srv = server(cfg).await;
    let addr = srv.addr;
    let ann = register(addr, "ann").await;
    let creds = json!({"name": "ann", "password": "a long password"});
    for _ in 0..2 {
        let (s, _) = json_req(addr, Method::POST, "/api/login", None, Some(creds.clone())).await;
        assert_eq!(s, StatusCode::OK);
    }
    // The fourth attempt from this IP (register counted) is refused, even
    // with the right password, before any hashing.
    let (s, h, b) = http(addr, Method::POST, "/api/login", None, Some(creds.clone())).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    let v: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["code"], "rate_limited");
    assert!(v["retry_after_ms"].as_u64().unwrap() > 1000);
    assert!(h.get("retry-after").is_some());
    let (s, _) = json_req(addr, Method::POST, "/api/register", None, Some(json!({"name": "bob", "password": "a long password"}))).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);

    // Commands: five go through, the rest are answered `rate_limited`.
    let mut w = ws(addr, &ann).await;
    wait_for(&mut w, "welcome", |v| v["type"] == "welcome").await;
    for id in 1..=8 {
        w.send(Message::Text(json!({"id": id, "type": "clear_view"}).to_string())).await.unwrap();
    }
    // Refusals come straight from the connection, ahead of the tick's results.
    let mut limited = 0;
    for _ in 1..=8 {
        let r = wait_for(&mut w, "result", |v| v["type"] == "result").await;
        if r["ok"] == false {
            assert_eq!(r["error"]["code"], "rate_limited");
            assert_eq!(r["command"], "clear_view");
            limited += 1;
        }
    }
    assert_eq!(limited, 3);
    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_sessions_expire_and_close_the_socket() {
    let mut cfg = config();
    cfg.sessions = SessionPolicy { idle: Duration::from_millis(600), max_age: Duration::from_secs(60) };
    let srv = server(cfg).await;
    let addr = srv.addr;
    let ann = register(addr, "ann").await;
    let mut w = ws(addr, &ann).await;
    wait_for(&mut w, "welcome", |v| v["type"] == "welcome").await;
    // Commands keep the session alive past the idle timeout.
    for id in 0..4 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        w.send(Message::Text(json!({"id": id, "type": "clear_view"}).to_string())).await.unwrap();
        let r = wait_for(&mut w, "result", |v| v["type"] == "result" && v["id"] == id).await;
        assert_eq!(r["ok"], true, "{r}");
    }
    // Then idle: the server says so and closes the socket.
    let ended = wait_for(&mut w, "session_ended", |v| v["type"] == "session_ended").await;
    assert_eq!(ended["reason"], "expired");
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match w.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return true,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert_eq!(closed, Ok(true));
    let (s, _) = json_req(addr, Method::GET, "/api/lineage/stats", Some(&ann), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = http(addr, Method::GET, &format!("/ws?token={ann}"), None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    srv.shutdown().await;
}
