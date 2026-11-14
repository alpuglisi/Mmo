//! Regression tests for the security review: request size limits, static
//! files, rate limits on reads, WebSocket connection and frame limits,
//! malformed messages and tokens.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use evo_server::accounts::HashPolicy;
use evo_server::game::WorldSettings;
use evo_server::http::{map_scale, MAX_BODY, MAX_CLIENT_MESSAGE, MAX_CONNS_PER_PLAYER, MAX_MAP_SIDE};
use evo_server::limits::{Limits, Rate, TrustedProxies};
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

async fn get_forwarded(addr: SocketAddr, path: &str, forwarded_for: &str) -> StatusCode {
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let r = Request::get(format!("http://{addr}{path}")).header("x-forwarded-for", forwarded_for);
    client.request(r.body(Full::new(Bytes::new())).unwrap()).await.expect("request").status()
}

/// Behind a trusted reverse proxy (`--trusted-proxy`) each client gets its
/// own limit by `X-Forwarded-For`; from an untrusted peer the header is
/// ignored, so it can't be used to dodge the limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trusted_proxy_forwards_the_client_address_for_the_limits() {
    for trusted in [true, false] {
        let mut cfg = config();
        cfg.limits = Limits { reads: Rate::new(0.01, 2.0), ..Limits::unlimited() };
        if trusted {
            cfg.trusted_proxies = TrustedProxies::parse("127.0.0.0/8").unwrap();
        }
        let srv = server(cfg).await;
        let addr = srv.addr;
        for _ in 0..2 {
            assert_eq!(get_forwarded(addr, "/api/species", "203.0.113.1").await, StatusCode::OK);
        }
        assert_eq!(get_forwarded(addr, "/api/species", "203.0.113.1").await, StatusCode::TOO_MANY_REQUESTS);
        // A spoofed first entry doesn't help: the proxy's (last) one counts.
        assert_eq!(
            get_forwarded(addr, "/api/species", "198.51.100.7, 203.0.113.1").await,
            StatusCode::TOO_MANY_REQUESTS
        );
        let other = get_forwarded(addr, "/api/species", "203.0.113.2").await;
        let expect = if trusted { StatusCode::OK } else { StatusCode::TOO_MANY_REQUESTS };
        assert_eq!(other, expect, "trusted: {trusted}");
        srv.shutdown().await;
    }
}

/// A login burst past the hash queue gets 503 `busy` with a retry time at
/// once; the metrics show the bound held; tokens (requests, the WebSocket)
/// never hash.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn password_hashing_is_bounded_and_a_burst_gets_busy() {
    let mut cfg = config();
    cfg.limits = Limits::unlimited();
    cfg.hashing = HashPolicy { concurrency: 1, queue: 1 };
    let srv = server(cfg).await;
    let addr = srv.addr;
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let burst = (0..16).map(|i| {
        let client = client.clone();
        async move {
            let body = json!({"name": format!("burst{i}"), "password": "a long password"}).to_string();
            let r = Request::post(format!("http://{addr}/api/register"))
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(body)))
                .unwrap();
            let res = client.request(r).await.expect("request");
            let (status, retry) = (res.status(), res.headers().get("retry-after").cloned());
            let body: Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
            (status, retry, body)
        }
    });
    let results = futures_util::future::join_all(burst).await;
    let created = results.iter().filter(|r| r.0 == StatusCode::CREATED).count();
    let busy: Vec<_> = results.iter().filter(|r| r.0 == StatusCode::SERVICE_UNAVAILABLE).collect();
    assert_eq!(created + busy.len(), 16, "{results:?}");
    assert!(created >= 2 && !busy.is_empty(), "{created} created, {} busy", busy.len());
    for (_, retry, body) in &busy {
        assert_eq!(body["code"], "busy");
        assert!(body["retry_after_ms"].as_u64().is_some_and(|ms| (250..=5000).contains(&ms)), "{body}");
        assert!(retry.is_some(), "Retry-After header");
    }
    let metrics = |path: &'static str| async move {
        let (s, b) = req(addr, Method::GET, path, None, Vec::new()).await;
        assert_eq!(s, StatusCode::OK);
        serde_json::from_slice::<Value>(&b).unwrap()["hashing"].clone()
    };
    let h = metrics("/api/metrics").await;
    assert_eq!((h["concurrency"].as_u64(), h["queue"].as_u64()), (Some(1), Some(1)), "{h}");
    assert_eq!(h["peak_in_flight"], 1, "{h}");
    assert_eq!(h["rejected"].as_u64(), Some(busy.len() as u64), "{h}");
    assert_eq!(h["hashes"].as_u64(), Some(created as u64), "{h}");
    assert_eq!((h["in_flight"].as_u64(), h["waiting"].as_u64()), (Some(0), Some(0)), "{h}");

    // A retry after the burst works, and the token then never hashes.
    let token = register(addr, "after").await;
    let hashes = metrics("/api/metrics").await["hashes"].as_u64().unwrap();
    let (s, _) = req(addr, Method::GET, "/api/lineage/stats", Some(&token), Vec::new()).await;
    assert_ne!(s, StatusCode::UNAUTHORIZED);
    let (ws, _) = connect_async(format!("ws://{addr}/ws?token={token}")).await.expect("socket opens");
    drop(ws);
    assert_eq!(metrics("/api/metrics").await["hashes"].as_u64(), Some(hashes));
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
    // The server frees the slot once its side of the connection winds down
    // (up to a couple of seconds, longer on a busy machine): poll for it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut ws = loop {
        if let Ok((ws, _)) = connect_async(format!("ws://{addr}/ws?token={token}")).await {
            break ws;
        }
        assert!(tokio::time::Instant::now() < deadline, "a slot frees up after a close");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };

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

/// Read until the server closes the connection (or resets it); `false` if it
/// is still open after `wait`.
async fn closed_within(s: &mut tokio::net::TcpStream, wait: Duration) -> bool {
    let mut buf = Vec::new();
    tokio::time::timeout(wait, s.read_to_end(&mut buf)).await.is_ok()
}

/// A request head trickled in (slowloris) or an idle keep-alive connection
/// is closed after the header timeout; one client can hold only so many
/// connections at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_and_idle_connections_are_closed_and_capped_per_client() {
    let srv = server(ServerConfig { header_timeout: Duration::from_millis(300), ..config() }).await;
    let mut slow = tokio::net::TcpStream::connect(srv.addr).await.unwrap();
    slow.write_all(b"GET /api/world HTTP/1.1\r\nHost: localhost\r\n").await.unwrap();
    assert!(closed_within(&mut slow, Duration::from_secs(10)).await, "an unfinished request head is dropped");
    let mut idle = tokio::net::TcpStream::connect(srv.addr).await.unwrap();
    assert!(closed_within(&mut idle, Duration::from_secs(10)).await, "a silent connection is dropped");
    srv.shutdown().await;

    let mut cfg = config();
    cfg.limits.conns_per_ip = 4;
    let srv = server(cfg).await;
    let addr = srv.addr;
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(tokio::net::TcpStream::connect(addr).await.unwrap());
    }
    // The fifth is closed at once, while the four stay usable.
    let mut extra = tokio::net::TcpStream::connect(addr).await.unwrap();
    assert!(closed_within(&mut extra, Duration::from_secs(10)).await, "a connection past the cap is closed");
    let s = &mut held[0];
    s.write_all(b"GET /api/world HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut out = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), s.read_to_end(&mut out)).await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&out).starts_with("HTTP/1.1 200"), "{}", String::from_utf8_lossy(&out));
    // Closing them frees the slots.
    drop(held);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !raw_get(addr, "/api/world").await.starts_with("HTTP/1.1 200") {
        assert!(tokio::time::Instant::now() < deadline, "slots free up after a close");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    srv.shutdown().await;
}

/// A trusted reverse proxy carries every player's connections: the
/// per-address connection cap doesn't apply to it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trusted_proxy_is_not_capped_per_address() {
    let mut cfg = config();
    cfg.limits.conns_per_ip = 2;
    cfg.trusted_proxies = TrustedProxies::parse("127.0.0.1").unwrap();
    let srv = server(cfg).await;
    let mut held = Vec::new();
    for _ in 0..6 {
        held.push(tokio::net::TcpStream::connect(srv.addr).await.unwrap());
    }
    let s = &mut held[5];
    s.write_all(b"GET /api/world HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut out = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), s.read_to_end(&mut out)).await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&out).starts_with("HTTP/1.1 200"), "{}", String::from_utf8_lossy(&out));
    srv.shutdown().await;
}

/// `?map=` names a cached season record: a long one would let a client fill
/// the cache with huge keys.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn season_map_ids_are_bounded() {
    let srv = server(config()).await;
    let long = "a".repeat(10_000);
    let (s, b) = req(srv.addr, Method::GET, &format!("/api/seasons/1?map={long}"), None, Vec::new()).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{}", String::from_utf8_lossy(&b));
    assert_eq!(serde_json::from_slice::<Value>(&b).unwrap()["code"], "bad_map");
    let (s, _) = req(srv.addr, Method::GET, "/api/seasons/1?map=0123456789abcdef", None, Vec::new()).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    srv.shutdown().await;
}

/// Opening a WebSocket (the world thread builds a full state for it) and
/// founding cost the world thread: both are rate-limited per client.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_upgrades_and_founding_are_rate_limited() {
    let mut cfg = config();
    cfg.limits = Limits { reads: Rate::new(0.01, 2.0), ..Limits::unlimited() };
    let srv = server(cfg).await;
    let addr = srv.addr;
    let token = register(addr, "dora").await;
    let mut socks = Vec::new();
    for _ in 0..2 {
        socks.push(connect_async(format!("ws://{addr}/ws?token={token}")).await.expect("within the rate").0);
    }
    match connect_async(format!("ws://{addr}/ws?token={token}")).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(res)) => {
            assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS)
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    let design =
        json!({"name": "A", "thermo": "warm", "birth": "live", "litter_size": 2, "traits": {}, "founders": 10});
    let (s, _) = req(addr, Method::POST, "/api/lineage", Some(&token), design.to_string().into_bytes()).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    drop(socks);
    srv.shutdown().await;
}

/// Malformed messages count against the command rate like any other.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_websocket_messages_count_against_the_command_rate() {
    let mut cfg = config();
    cfg.limits = Limits { commands: Rate::new(0.01, 3.0), ..Limits::unlimited() };
    let srv = server(cfg).await;
    let addr = srv.addr;
    let token = register(addr, "erin").await;
    let (mut ws, _) = connect_async(format!("ws://{addr}/ws?token={token}")).await.unwrap();
    for _ in 0..6 {
        ws.send(Message::Text("{not json".into())).await.unwrap();
    }
    let (mut bad, mut limited) = (0, 0);
    while bad + limited < 6 {
        let Some(Ok(Message::Text(t))) = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.unwrap() else {
            continue;
        };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "result" {
            match v["error"]["code"].as_str() {
                Some("bad_message") => bad += 1,
                Some("rate_limited") => limited += 1,
                other => panic!("unexpected {other:?}"),
            }
        }
    }
    assert_eq!((bad, limited), (3, 3));
    srv.shutdown().await;
}

/// Pages and API answers carry headers that stop sniffing, framing
/// (clickjacking) and referrer leaks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_carry_security_headers() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("evo-headers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("index.html"), "<html>the client</html>").unwrap();
    let srv = server(ServerConfig { static_dir: Some(root.clone()), ..config() }).await;
    for path in ["/", "/index.html", "/some/route", "/api/world"] {
        let res = raw_get(srv.addr, path).await.to_ascii_lowercase();
        let head = res.split("\r\n\r\n").next().unwrap_or_default().to_string();
        assert!(head.starts_with("http/1.1 200"), "{path}: {head}");
        for want in [
            "x-content-type-options: nosniff",
            "x-frame-options: deny",
            "referrer-policy: no-referrer",
            "content-security-policy: frame-ancestors 'none'",
        ] {
            assert!(head.contains(want), "{path} lacks {want}: {head}");
        }
    }
    srv.shutdown().await;
    let _ = std::fs::remove_dir_all(&root);
}
