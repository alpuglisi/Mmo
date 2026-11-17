//! permessage-deflate on the game WebSocket: negotiated when the client
//! offers it (as every browser does) unless the server turns it off, and a
//! compressing client plays as any other (its compressed commands are read,
//! the compressed states decode).

use std::net::SocketAddr;
use std::time::Duration;

use evo_server::game::WorldSettings;
use evo_server::http::WsCompression;
use evo_server::wire::DeltaDecoder;
use evo_server::{start, ServerConfig};
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use yawc::frame::OpCode;

fn config(ws_compression: WsCompression) -> ServerConfig {
    ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        world: WorldSettings { seed: 5, width: 48, height: 48, wild: false, ..Default::default() },
        ws_compression,
        ..Default::default()
    }
    .fast()
}

async fn post(addr: SocketAddr, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let mut r = Request::builder().method(Method::POST).uri(format!("http://{addr}{path}"));
    r = r.header("content-type", "application/json");
    if let Some(t) = token {
        r = r.header("authorization", format!("Bearer {t}"));
    }
    let res = client.request(r.body(Full::new(Bytes::from(body.to_string()))).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn register(addr: SocketAddr, name: &str) -> String {
    let (s, v) = post(addr, "/api/register", None, json!({"name": name, "password": "a long password"})).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v["token"].as_str().unwrap().to_string()
}

/// The response head of a WebSocket handshake offering `extensions`.
async fn handshake(addr: SocketAddr, token: &str, extensions: Option<&str>) -> String {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    let offer = extensions.map_or(String::new(), |e| format!("Sec-WebSocket-Extensions: {e}\r\n"));
    let req = format!(
        "GET /ws?token={token}&proto=2 HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{offer}\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf)).await.unwrap().unwrap();
        assert!(n > 0, "closed during the handshake");
        head.extend_from_slice(&buf[..n]);
    }
    String::from_utf8_lossy(&head).to_lowercase()
}

/// What browsers send (Chromium, Firefox and WebKit alike).
const BROWSER_OFFER: &str = "permessage-deflate; client_max_window_bits";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deflate_is_negotiated_when_offered() {
    let srv = start(config(WsCompression::Context)).await.unwrap();
    let token = register(srv.addr, "ann").await;
    let head = handshake(srv.addr, &token, Some(BROWSER_OFFER)).await;
    assert!(head.starts_with("http/1.1 101"), "{head}");
    assert!(head.contains("sec-websocket-extensions: permessage-deflate"), "{head}");
    // No offer, no extension.
    let head = handshake(srv.addr, &token, None).await;
    assert!(head.starts_with("http/1.1 101") && !head.contains("sec-websocket-extensions"), "{head}");
    srv.shutdown().await;

    // Per-message mode tells the client not to keep its context either.
    let srv = start(config(WsCompression::Message)).await.unwrap();
    let token = register(srv.addr, "ann").await;
    let head = handshake(srv.addr, &token, Some(BROWSER_OFFER)).await;
    assert!(head.contains("server_no_context_takeover") && head.contains("client_no_context_takeover"), "{head}");
    srv.shutdown().await;

    // Off: plain frames even when offered.
    let srv = start(config(WsCompression::Off)).await.unwrap();
    let token = register(srv.addr, "ann").await;
    let head = handshake(srv.addr, &token, Some(BROWSER_OFFER)).await;
    assert!(head.starts_with("http/1.1 101") && !head.contains("sec-websocket-extensions"), "{head}");
    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_compressing_client_plays() {
    let srv = start(config(WsCompression::Context)).await.unwrap();
    let addr = srv.addr;
    let token = register(addr, "ann").await;
    let design =
        json!({"name": "Ann's", "thermo": "warm", "birth": "live", "litter_size": 2, "traits": {}, "founders": 6});
    assert_eq!(post(addr, "/api/lineage", Some(&token), design).await.0, StatusCode::CREATED);
    let url = format!("ws://{addr}/ws?token={token}&proto=2").parse().unwrap();
    let mut ws = yawc::WebSocket::connect(url)
        .with_options(yawc::Options::default().with_low_latency_compression())
        .await
        .expect("connects");
    let mut decoder = DeltaDecoder::new();
    let (mut welcome, mut states, mut result) = (false, 0, None);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while (states < 3 || result.is_none()) && tokio::time::Instant::now() < deadline {
        let Ok(Some(frame)) = tokio::time::timeout(Duration::from_secs(5), ws.next()).await else { break };
        match frame.opcode() {
            OpCode::Text => {
                let v: Value = serde_json::from_slice(frame.payload()).unwrap();
                if v["type"] == "welcome" {
                    welcome = true;
                    // A command, compressed by the client.
                    let msg = json!({"type": "ledger", "id": 7, "limit": 5}).to_string();
                    ws.send(yawc::Frame::text(msg)).await.unwrap();
                }
                if v["type"] == "result" && v["id"] == 7 {
                    result = Some(v);
                }
            }
            OpCode::Binary => {
                let s = decoder.apply(frame.payload()).expect("a compressed state decodes");
                assert_eq!(s.organisms.len(), 6);
                states += 1;
            }
            _ => {}
        }
    }
    assert!(welcome && states >= 3, "welcome {welcome}, {states} states");
    let result = result.expect("the command was answered");
    assert_eq!(result["ok"], true, "{result}");
    srv.shutdown().await;
}
