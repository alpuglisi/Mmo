//! Regression tests for the security review: request size limits, static
//! files, rate limits on reads, WebSocket connection and frame limits,
//! malformed messages and tokens.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use evo_server::game::WorldSettings;
use evo_server::http::{map_scale, MAX_BODY, MAX_CLIENT_MESSAGE, MAX_CONNS_PER_PLAYER, MAX_MAP_SIDE};
use evo_server::limits::{Limits, Rate};
use evo_server::{start, RunningServer, ServerConfig};
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

fn config() -> ServerConfig {
    ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        world: WorldSettings {
            seed: 5,
            width: 32,
            height: 32,
            wild: false,
            days_per_year: 8,
            lod: false,
            catastrophes: false,
        },
        ..Default::default()
    }
    .fast()
}

async fn server(cfg: ServerConfig) -> RunningServer {
    start(cfg).await.expect("server starts")
}

async fn req(addr: SocketAddr, method: Method, path: &str, token: Option<&str>, body: Vec<u8>) -> (StatusCode, Bytes) {
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let mut r = Request::builder().method(method).uri(format!("http://{addr}{path}"));
    if let Some(t) = token {
        r = r.header("authorization", format!("Bearer {t}"));
    }
    if !body.is_empty() {
        r = r.header("content-type", "application/json");
    }
    let res = client.request(r.body(Full::new(Bytes::from(body))).unwrap()).await.expect("request");
    let status = res.status();
    (status, res.into_body().collect().await.unwrap().to_bytes())
}

async fn register(addr: SocketAddr, name: &str) -> String {
    let body = json!({"name": name, "password": "a long password"}).to_string().into_bytes();
    let (s, b) = req(addr, Method::POST, "/api/register", None, body).await;
    assert_eq!(s, StatusCode::CREATED, "{}", String::from_utf8_lossy(&b));
    let v: Value = serde_json::from_slice(&b).unwrap();
    v["token"].as_str().unwrap().to_string()
}

/// A raw HTTP/1.1 GET (no client-side path normalization).
async fn raw_get(addr: SocketAddr, path: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
    String::from_utf8_lossy(&out).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_and_malformed_bodies_are_refused() {
    let srv = server(config()).await;
    let addr = srv.addr;
    // A body over the limit is refused before it is parsed or hashed.
    let password = "p".repeat(MAX_BODY);
    let body = json!({"name": "mallory", "password": password}).to_string().into_bytes();
    let (s, _) = req(addr, Method::POST, "/api/register", None, body).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    // A password over the account limit (but under the body limit).
    let body = json!({"name": "mallory", "password": "p".repeat(2000)}).to_string().into_bytes();
    let (s, b) = req(addr, Method::POST, "/api/register", None, body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{}", String::from_utf8_lossy(&b));
    // Malformed JSON and wrong types: client errors, not panics.
    for body in [&b"{"[..], b"[]", br#"{"name": 5, "password": []}"#, b"\xff\xfe"] {
        let (s, _) = req(addr, Method::POST, "/api/login", None, body.to_vec()).await;
        assert!(s.is_client_error(), "{s}");
    }
    let token = register(addr, "alice").await;
    for body in [&br#"{"name": "x"}"#[..], br#"{"name":"A","thermo":"warm","birth":"live","litter_size":4294967295}"#] {
        let (s, _) = req(addr, Method::POST, "/api/lineage", Some(&token), body.to_vec()).await;
        assert!(s.is_client_error(), "{s}");
    }
    // The server still answers.
    let (s, _) = req(addr, Method::GET, "/api/world", None, Vec::new()).await;
    assert_eq!(s, StatusCode::OK);
    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn static_files_stay_inside_their_directory() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("evo-static-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>the client</html>").unwrap();
    std::fs::write(root.join("secret.txt"), "TOP SECRET").unwrap();
    let srv = server(ServerConfig { static_dir: Some(dist.clone()), ..config() }).await;
    let addr = srv.addr;
    assert!(raw_get(addr, "/index.html").await.contains("the client"));
    for path in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/..%2fsecret.txt",
        "/%2e%2e%2fsecret.txt",
        "/x/../../secret.txt",
        "/..\\secret.txt",
        "/%5c..%5csecret.txt",
        "//etc/passwd",
    ] {
        let res = raw_get(addr, path).await;
        assert!(!res.contains("TOP SECRET"), "{path} leaked: {res}");
        assert!(!res.contains("root:"), "{path} leaked: {res}");
    }
    srv.shutdown().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expensive_reads_are_rate_limited_per_client() {
    let mut cfg = config();
    cfg.limits = Limits { reads: Rate::new(0.01, 3.0), ..Limits::unlimited() };
    let srv = server(cfg).await;
    let addr = srv.addr;
    for path in ["/api/species", "/api/terrain/dynamic", "/api/eras"] {
        let (s, _) = req(addr, Method::GET, path, None, Vec::new()).await;
        assert_eq!(s, StatusCode::OK, "{path}");
    }
    for path in ["/api/species", "/api/map.png", "/api/terrain", "/api/leaderboard/seasons"] {
        let (s, b) = req(addr, Method::GET, path, None, Vec::new()).await;
        assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{path}");
        assert_eq!(serde_json::from_slice::<Value>(&b).unwrap()["code"], "rate_limited");
    }
    // Cheap reads of published state are not limited.
    for _ in 0..5 {
        let (s, _) = req(addr, Method::GET, "/api/world", None, Vec::new()).await;
        assert_eq!(s, StatusCode::OK);
    }
    srv.shutdown().await;
}

#[test]
fn the_map_image_stays_small() {
    assert_eq!(map_scale(None, 128), 4);
    assert_eq!(map_scale(Some(0), 128), 1);
    assert_eq!(map_scale(Some(usize::MAX), 128), 8);
    assert_eq!(map_scale(Some(8), 1024), MAX_MAP_SIDE / 1024);
    assert_eq!(map_scale(Some(8), 100_000), 1);
    for tiles in [32, 128, 256, 512, 1024] {
        assert!(map_scale(Some(8), tiles) * tiles <= MAX_MAP_SIDE.max(tiles));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_tokens_connections_and_frames_are_bounded() {
    let srv = server(config()).await;
    let addr = srv.addr;
    // No token, an unknown one, or a logged-out one: 401 before upgrading.
    let token = register(addr, "bob").await;
    let (s, _) = req(addr, Method::GET, "/ws", None, Vec::new()).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(connect_async(format!("ws://{addr}/ws?token={}", "0".repeat(64))).await.is_err());
    let other = register(addr, "carol").await;
    let (s, _) = req(addr, Method::POST, "/api/logout", Some(&other), Vec::new()).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(connect_async(format!("ws://{addr}/ws?token={other}")).await.is_err());
    let (s, _) = req(addr, Method::GET, "/api/lineage/stats", Some(&other), Vec::new()).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Up to the per-player limit of connections, then refused.
    let mut socks = Vec::new();
    for _ in 0..MAX_CONNS_PER_PLAYER {
        socks.push(connect_async(format!("ws://{addr}/ws?token={token}")).await.expect("within the limit").0);
    }
    match connect_async(format!("ws://{addr}/ws?token={token}")).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(res)) => {
            assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS)
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    // Closing one frees its slot.
    let mut closed = socks.pop().unwrap();
    closed.close(None).await.unwrap();
    while closed.next().await.is_some() {}
    let mut freed = None;
    for _ in 0..50 {
        if let Ok((ws, _)) = connect_async(format!("ws://{addr}/ws?token={token}")).await {
            freed = Some(ws);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut ws = freed.expect("a slot frees up after a close");

    // A malformed message gets an error and the connection stays usable.
    ws.send(Message::Text("{not json".into())).await.unwrap();
    ws.send(Message::Text(json!({"type": "ledger", "id": 7}).to_string())).await.unwrap();
    let (mut bad, mut ok) = (false, false);
    while !(bad && ok) {
        let Some(Ok(Message::Text(t))) = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.unwrap() else {
            panic!("socket closed")
        };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "result" {
            bad |= v["error"]["code"] == "bad_message";
            ok |= v["id"] == 7 && v["ok"] == true;
        }
    }
    // A frame over the message limit closes that connection only.
    let big = "x".repeat(MAX_CLIENT_MESSAGE * 4);
    let _ = ws.send(Message::Text(big)).await;
    let ended = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "an oversized frame ends the connection");
    let (s, _) = req(addr, Method::GET, "/api/world", None, Vec::new()).await;
    assert_eq!(s, StatusCode::OK);
    drop(socks);
    srv.shutdown().await;
}
