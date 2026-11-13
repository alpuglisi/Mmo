//! End-to-end: start the server on a random port in fast mode, register two
//! players, found lineages, play over WebSockets.

use std::net::SocketAddr;
use std::time::Duration;

use evo_server::game::WorldSettings;
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

async fn server() -> RunningServer {
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        world: WorldSettings { seed: 11, width: 64, height: 64, wild: true, ..Default::default() },
        ..Default::default()
    }
    .fast();
    start(cfg).await.expect("server starts")
}

/// A server whose world ticks only when the test calls `step`, and applies
/// commands as they arrive: no race against a ticker, so a run is exactly
/// reproducible.
async fn manual_server() -> RunningServer {
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        world: WorldSettings { seed: 11, width: 64, height: 64, wild: true, ..Default::default() },
        ..Default::default()
    }
    .fast()
    .manual();
    start(cfg).await.expect("server starts")
}

async fn http(
    addr: SocketAddr,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Bytes) {
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
    (status, res.into_body().collect().await.unwrap().to_bytes())
}

async fn json_req(
    addr: SocketAddr,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (s, b) = http(addr, method, path, token, body).await;
    (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

async fn register(addr: SocketAddr, name: &str) -> String {
    let (s, v) =
        json_req(addr, Method::POST, "/api/register", None, Some(json!({"name": name, "password": "a long password"})))
            .await;
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
        "founders": 12
    })
}

async fn ws(addr: SocketAddr, token: &str) -> Ws {
    let (ws, _) = connect_async(format!("ws://{addr}/ws?token={token}")).await.expect("websocket");
    ws
}

/// Read messages until one satisfies `pred`.
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

async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string())).await.unwrap();
}

fn find_org(state: &Value, id: u64) -> Option<&Value> {
    state["organisms"].as_array().unwrap().iter().find(|o| o["id"] == id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn play_over_websocket() {
    let srv = server().await;
    let addr = srv.addr;

    // Accounts.
    let ann = register(addr, "ann").await;
    let bob = register(addr, "bob").await;
    let (s, _) =
        json_req(addr, Method::POST, "/api/register", None, Some(json!({"name": "ann", "password": "whatever123"})))
            .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, v) =
        json_req(addr, Method::POST, "/api/login", None, Some(json!({"name": "ann", "password": "a long password"})))
            .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["player"], 1);
    let (s, _) =
        json_req(addr, Method::POST, "/api/login", None, Some(json!({"name": "ann", "password": "nope nope nope"})))
            .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Founding: budget errors come back as JSON, one lineage per player.
    let mut over = design("Too much");
    over["traits"] =
        json!({"wings": 1.0, "armor_plates": 1.0, "toxins": 1.0, "muscle": 1.0, "claws": 1.0, "fangs": 1.0});
    over["founders"] = json!(300);
    let (s, v) = json_req(addr, Method::POST, "/api/lineage", Some(&ann), Some(over)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["code"], "over_budget");
    assert!(v["cost"].as_f64().unwrap() > v["budget"].as_f64().unwrap());
    let (s, _) = json_req(addr, Method::POST, "/api/lineage", None, Some(design("Anon"))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v) = json_req(addr, Method::POST, "/api/lineage", Some(&ann), Some(design("Ann's"))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["founders"].as_array().unwrap().len(), 12);
    let (s, v) = json_req(addr, Method::POST, "/api/lineage", Some(&bob), Some(design("Bob's"))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let (s, v) = json_req(addr, Method::POST, "/api/lineage", Some(&ann), Some(design("Again"))).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["code"], "already_founded");

    // WebSocket: welcome, then state with our organisms only in `organisms`.
    let (s, _) = http(addr, Method::GET, "/ws?token=bogus", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let mut wa = ws(addr, &ann).await;
    let welcome = wait_for(&mut wa, "welcome", |v| v["type"] == "welcome").await;
    assert_eq!(welcome["player"], 1);
    assert!(welcome["lineage"].is_u64());
    let state = wait_for(&mut wa, "state", |v| v["type"] == "state").await;
    let orgs = state["organisms"].as_array().unwrap();
    assert_eq!(orgs.len(), 12);
    assert!(state["nearby"].as_array().unwrap().iter().all(|o| o["owner"] != 1));
    assert!(state["clock"]["season"].is_string());
    assert!(state["weather"]["mean_wind"].is_number());

    // Possess one organism and walk it toward the map center.
    let org = &orgs[0];
    let id = org["id"].as_u64().unwrap();
    let (x0, y0) = (org["x"].as_f64().unwrap(), org["y"].as_f64().unwrap());
    send(&mut wa, json!({"id": 1, "type": "possess", "organism": id})).await;
    let r = wait_for(&mut wa, "possess result", |v| v["type"] == "result" && v["id"] == 1).await;
    assert_eq!(r["ok"], true, "{r}");
    let (dx, dy) = ((32.0 - x0).signum() * 6.0, (32.0 - y0).signum() * 6.0);
    let (tx, ty) = (x0 + dx, y0 + dy);
    send(&mut wa, json!({"id": 2, "type": "order_move", "organism": id, "x": tx, "y": ty})).await;
    let r = wait_for(&mut wa, "order result", |v| v["type"] == "result" && v["id"] == 2).await;
    assert_eq!(r["ok"], true, "{r}");
    let start_dist = ((tx - x0).powi(2) + (ty - y0).powi(2)).sqrt();
    let moved = wait_for(&mut wa, "the organism to move", |v| {
        v["type"] == "state"
            && find_org(v, id).is_some_and(|o| {
                let (x, y) = (o["x"].as_f64().unwrap(), o["y"].as_f64().unwrap());
                o["possessed"] == true && ((tx - x).powi(2) + (ty - y).powi(2)).sqrt() < start_dist - 1.0
            })
    })
    .await;
    let o = find_org(&moved, id).unwrap();
    assert_eq!(o["action"], "ordered");
    // The view follows the possessed organism.
    assert!((moved["view"]["x"].as_f64().unwrap() - o["x"].as_f64().unwrap()).abs() < 1e-3);

    // Errors come back on the socket.
    send(&mut wa, json!({"id": 3, "type": "possess", "organism": 999_999})).await;
    let r = wait_for(&mut wa, "error result", |v| v["type"] == "result" && v["id"] == 3).await;
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "not_found");
    send(&mut wa, json!({"type": "fly_to_the_moon"})).await;
    let r = wait_for(&mut wa, "parse error", |v| v["type"] == "result" && v["ok"] == false).await;
    assert_eq!(r["error"]["code"], "bad_message");

    // Read-only endpoints.
    let (s, board) = json_req(addr, Method::GET, "/api/leaderboard", None, None).await;
    assert_eq!(s, StatusCode::OK);
    let names: Vec<&str> = board["rows"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"ann") && names.contains(&"bob"), "{board}");
    let (s, png) = http(addr, Method::GET, "/api/map.png?scale=2", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let (s, w) = json_req(addr, Method::GET, "/api/world", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(w["width"], 64);

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn directed_mating_consent_round_trip() {
    // Manual clock: the world only moves when we step it, so nothing can
    // resolve the offer behind our back (between "accepted" and "cancel", or
    // a natural conception between request and answer).
    let srv = manual_server().await;
    let addr = srv.addr;
    let ann = register(addr, "ann").await;
    let bob = register(addr, "bob").await;
    for (t, n) in [(&ann, "Ann's"), (&bob, "Bob's")] {
        let (s, v) = json_req(addr, Method::POST, "/api/lineage", Some(t), Some(design(n))).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
    }
    let tick0 = srv.step(1).await;
    let mut wa = ws(addr, &ann).await;
    let mut wb = ws(addr, &bob).await;
    // Connecting pushes a state at once; no tick needed.
    let sa = wait_for(&mut wa, "ann's state", |v| v["type"] == "state").await;
    let sb = wait_for(&mut wb, "bob's state", |v| v["type"] == "state").await;
    let pick = |s: &Value, sex: &str| {
        let ok = |o: &&Value| o["sex"] == sex && o["matured"] == true && o["pregnant"] == false;
        s["organisms"].as_array().unwrap().iter().find(ok).unwrap()["id"].as_u64().unwrap()
    };
    let (mine, partner) = (pick(&sa, "female"), pick(&sb, "male"));

    // Ann asks; Bob is asked.
    send(&mut wa, json!({"id": 10, "type": "request_mating", "mine": mine, "partner": partner, "price": 25})).await;
    let r = wait_for(&mut wa, "request result", |v| v["type"] == "result" && v["id"] == 10).await;
    assert_eq!(r["ok"], true, "{r}");
    let req = wait_for(&mut wb, "mating request", |v| v["type"] == "mating_request").await;
    let offer = &req["offer"];
    assert_eq!(offer["from_name"], "ann");
    assert_eq!(offer["partner"], partner);
    assert_eq!(offer["price"], 25);
    assert_eq!(offer["status"], "pending");
    let offer_id = offer["id"].as_u64().unwrap();
    // The open offer shows in both players' state (pushed after a step; a
    // pending offer doesn't change on a tick).
    assert_eq!(srv.step(1).await, tick0 + 1);
    let s =
        wait_for(&mut wb, "offer in state", |v| v["type"] == "state" && !v["offers"].as_array().unwrap().is_empty())
            .await;
    assert_eq!(s["offers"][0]["id"], offer_id);
    assert_eq!(s["offers"][0]["status"], "pending");

    // Ann can't answer her own offer; Bob accepts.
    send(&mut wa, json!({"id": 11, "type": "respond_mating", "request": offer_id, "accept": true})).await;
    let r = wait_for(&mut wa, "bad respond", |v| v["type"] == "result" && v["id"] == 11).await;
    assert_eq!(r["error"]["code"], "unknown_offer");
    send(&mut wb, json!({"id": 20, "type": "respond_mating", "request": offer_id, "accept": true})).await;
    let r = wait_for(&mut wb, "respond result", |v| v["type"] == "result" && v["id"] == 20).await;
    assert_eq!(r["ok"], true, "{r}");

    // Both sides hear the outcome: the founders live apart, so the agreement
    // stands (accepted, too far) until they meet. If they happen to be close
    // it completes at once; if the rules refuse it, it fails. With the manual
    // clock the result is fixed by the seed, but all three are valid.
    let upd = wait_for(&mut wa, "mating update", |v| v["type"] == "mating_update").await;
    let status = upd["offer"]["status"].as_str().unwrap().to_string();
    assert!(["accepted", "completed", "failed"].contains(&status.as_str()), "{upd}");
    let u = wait_for(&mut wb, "bob's update", |v| v["type"] == "mating_update").await;
    assert_eq!(u["offer"]["status"], status.as_str());
    if status == "accepted" {
        assert_eq!(upd["error"]["code"], "too_far");
        // Ann withdraws; both hear it. No tick has run since the answer, so
        // the agreement is still open.
        send(&mut wa, json!({"id": 12, "type": "cancel_mating", "request": offer_id})).await;
        let r = wait_for(&mut wa, "cancel result", |v| v["type"] == "result" && v["id"] == 12).await;
        assert_eq!(r["ok"], true, "{r}");
        let u = wait_for(&mut wb, "cancelled", |v| v["type"] == "mating_update" && v["offer"]["status"] == "cancelled")
            .await;
        assert_eq!(u["offer"]["id"], offer_id);
        // Later ticks don't revive it.
        srv.step(5).await;
        let s = wait_for(&mut wb, "state after cancel", |v| v["type"] == "state").await;
        assert!(s["offers"].as_array().unwrap().iter().all(|o| o["id"] != offer_id), "{}", s["offers"]);
    } else if status == "completed" {
        assert!(upd["outcome"]["conceived"].is_boolean());
    } else {
        assert!(upd["error"]["code"].is_string());
    }
    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metrics_count_ticks_connections_and_traffic() {
    let srv = manual_server().await;
    let addr = srv.addr;
    let token = register(addr, "meg").await;
    let mut sock = ws(addr, &token).await;
    wait_for(&mut sock, "state", |v| v["type"] == "state").await;
    srv.step(3).await;
    wait_for(&mut sock, "state", |v| v["type"] == "state").await;
    let (s, m) = json_req(addr, Method::GET, "/api/metrics", None, None).await;
    assert_eq!(s, StatusCode::OK, "{m}");
    assert_eq!(m["ticks"], 3);
    assert_eq!(m["conns"], 1);
    assert_eq!(m["state_pushes"], 1, "one push at the end of the step: {m}");
    assert_eq!(m["tick_ms"]["n"], 3);
    assert!(m["states_ms"]["p50"].as_f64().unwrap() >= 0.0);
    assert_eq!(m["ws_opened"], 1);
    // welcome + the connect state + the step's state, all counted.
    assert!(m["msgs_out"].as_u64().unwrap() >= 3, "{m}");
    assert!(m["state_bytes"].as_u64().unwrap() > 0 && m["bytes_out"].as_u64() >= m["state_bytes"].as_u64());
    assert_eq!(m["dropped_conns"], 0);
    if cfg!(target_os = "linux") {
        assert!(m["rss_bytes"].as_u64().unwrap() > 0);
    }
    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serves_the_static_client() {
    let dir = std::env::temp_dir().join(format!("evo-static-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><title>evo</title>").unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        world: WorldSettings { seed: 3, width: 64, height: 64, wild: false, ..Default::default() },
        static_dir: Some(dir.clone()),
        ..Default::default()
    }
    .fast();
    let srv = start(cfg).await.unwrap();
    let addr = srv.addr;
    let (s, body) = http(addr, Method::GET, "/", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("<title>evo</title>"));
    let (s, body) = http(addr, Method::GET, "/assets/app.js", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(&body[..], b"console.log(1)");
    // Unknown paths fall back to the app; the API keeps its routes.
    let (s, body) = http(addr, Method::GET, "/world", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("<title>evo</title>"));
    let (s, v) = json_req(addr, Method::GET, "/api/world", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["width"], 64);
    srv.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Register, found, connect and possess an organism ordered 6 tiles toward
/// the map center; returns the socket, its id, the target and the distance
/// to it from the organism's position in the last state.
async fn possess_and_order(addr: SocketAddr) -> (Ws, u64, (f64, f64), f64) {
    let ann = register(addr, "ann").await;
    let (s, v) = json_req(addr, Method::POST, "/api/lineage", Some(&ann), Some(design("Ann's"))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let mut wa = ws(addr, &ann).await;
    let state =
        wait_for(&mut wa, "state", |v| v["type"] == "state" && !v["organisms"].as_array().unwrap().is_empty()).await;
    let org = &state["organisms"][0];
    let id = org["id"].as_u64().unwrap();
    let (x0, y0) = (org["x"].as_f64().unwrap(), org["y"].as_f64().unwrap());
    send(&mut wa, json!({"id": 1, "type": "possess", "organism": id})).await;
    let r = wait_for(&mut wa, "possess result", |v| v["type"] == "result" && v["id"] == 1).await;
    assert_eq!(r["ok"], true, "{r}");
    let (tx, ty) = (x0 + (32.0 - x0).signum() * 6.0, y0 + (32.0 - y0).signum() * 6.0);
    send(&mut wa, json!({"id": 2, "type": "order_move", "organism": id, "x": tx, "y": ty})).await;
    let r = wait_for(&mut wa, "order result", |v| v["type"] == "result" && v["id"] == 2).await;
    assert_eq!(r["ok"], true, "{r}");
    (wa, id, (tx, ty), ((tx - x0).powi(2) + (ty - y0).powi(2)).sqrt())
}

fn dist_to(state: &Value, id: u64, (tx, ty): (f64, f64)) -> f64 {
    let o = find_org(state, id).expect("organism in state");
    ((tx - o["x"].as_f64().unwrap()).powi(2) + (ty - o["y"].as_f64().unwrap()).powi(2)).sqrt()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn possessed_organism_moves_between_ticks() {
    // Manual clock: no tick at all, yet a sub-step moves the possessed
    // organism toward its fresh order and shows the owner.
    let srv = manual_server().await;
    let tick0 = srv.step(1).await;
    let (mut wa, id, target, d0) = possess_and_order(srv.addr).await;
    assert_eq!(srv.substep().await, 2);
    let s = wait_for(&mut wa, "sub-step state", |v| v["type"] == "state" && v["clock"]["tick"] == tick0).await;
    let d1 = dist_to(&s, id, target);
    assert!(d1 < d0, "no closer after a sub-step: {d0} -> {d1}");
    assert_eq!(find_org(&s, id).unwrap()["action"], "ordered");
    // The tick's own move is the last of the tick's four sub-steps.
    assert_eq!(srv.substep().await, 1);
    assert_eq!(srv.substep().await, 0);
    assert_eq!(srv.substep().await, 0);
    assert_eq!(srv.step(1).await, tick0 + 1);
    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ticking_server_shows_possessed_movement_within_a_tick() {
    // A slow clock (600 ms ticks) and rare state pushes (once a minute):
    // the owner still sees the organism move several times within one tick.
    let mut cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        world: WorldSettings { seed: 11, width: 64, height: 64, wild: false, ..Default::default() },
        ..Default::default()
    };
    cfg.runner.tick = Duration::from_millis(600);
    cfg.runner.state_interval = Duration::from_secs(60);
    let srv = start(cfg).await.expect("server starts");
    let (mut wa, id, target, _) = possess_and_order(srv.addr).await;
    let mut seen: Vec<(u64, f64)> = Vec::new();
    // Two sub-step states in the same tick, the second closer.
    let _ = wait_for(&mut wa, "two moves within a tick", |v| {
        if v["type"] != "state" {
            return false;
        }
        let (tick, d) = (v["clock"]["tick"].as_u64().unwrap(), dist_to(v, id, target));
        let done = seen.iter().any(|&(t, e)| t == tick && d < e - 1e-4);
        seen.push((tick, d));
        done
    })
    .await;
    srv.shutdown().await;
}

/// The next state on a socket: a protocol 1 JSON state, or a protocol 2
/// binary frame (other messages are skipped).
async fn next_frame(ws: &mut Ws) -> Message {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(20), ws.next())
            .await
            .expect("a state in time")
            .expect("socket open")
            .expect("frame");
        match &msg {
            Message::Binary(_) => return msg,
            Message::Text(t) if serde_json::from_str::<Value>(t).unwrap()["type"] == "state" => return msg,
            _ => {}
        }
    }
}

/// Check a protocol 2 frame against the protocol 1 state of the same push;
/// returns the decoded state and the frame kind.
fn same_state(
    json_frame: Message,
    delta_frame: Message,
    dec: &mut evo_server::wire::DeltaDecoder,
) -> (evo_server::protocol::StateView, u8) {
    let Message::Text(t) = json_frame else { panic!("protocol 1 states are text") };
    let mut v: Value = serde_json::from_str(&t).unwrap();
    v.as_object_mut().unwrap().remove("type");
    let full: evo_server::protocol::StateView = serde_json::from_value(v).unwrap();
    let Message::Binary(b) = delta_frame else { panic!("protocol 2 states are binary") };
    let got = dec.apply(&b).expect("decodes");
    assert_eq!(got, evo_server::wire::canonical(&full));
    (got, b[0])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delta_states_match_json_states_and_resync() {
    use evo_server::wire::{DeltaDecoder, DELTA, KEYFRAME};
    let srv = manual_server().await;
    let addr = srv.addr;
    let (mut old, id, _, _) = possess_and_order(addr).await;
    // The same player on protocol 2 (and on protocol 1 without asking).
    let creds = json!({"name": "ann", "password": "a long password"});
    let (s, v) = json_req(addr, Method::POST, "/api/login", None, Some(creds)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let token = v["token"].as_str().unwrap();
    let (mut new, _) = connect_async(format!("ws://{addr}/ws?token={token}&proto=2")).await.expect("websocket");
    let welcome = wait_for(&mut new, "welcome", |v| v["type"] == "welcome").await;
    assert_eq!(welcome["proto"], 2);
    let mut plain = ws(addr, token).await;
    let welcome = wait_for(&mut plain, "welcome", |v| v["type"] == "welcome").await;
    assert!(welcome.get("proto").is_none(), "{welcome}");
    // Line both sockets up on the same push (the first is a keyframe).
    let tick = srv.step(1).await;
    let mut dec = DeltaDecoder::new();
    let mut new_state = next_frame(&mut new).await;
    loop {
        let Message::Text(t) = next_frame(&mut old).await else { panic!("text") };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["clock"]["tick"] == tick {
            // The protocol 2 socket's first state may be older: skip to this push.
            loop {
                let Message::Binary(b) = &new_state else { panic!("binary") };
                let s = dec.apply(b).expect("decodes");
                if s.clock.tick == tick {
                    break;
                }
                new_state = next_frame(&mut new).await;
            }
            break;
        }
    }
    let mut kinds = Vec::new();
    let mut moved = false;
    for round in 0..6 {
        if round == 3 {
            send(&mut new, json!({"type": "resync"})).await;
            // Wait until the reader has handled it (counted) before the push.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
            while json_req(addr, Method::GET, "/api/metrics", None, None).await.1["resyncs"] != 1 {
                assert!(tokio::time::Instant::now() < deadline, "the resync was never handled");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        if srv.substep().await > 0 {
            let (s, k) = same_state(next_frame(&mut old).await, next_frame(&mut new).await, &mut dec);
            moved |= s.organisms.iter().any(|o| o.id == id && o.possessed);
            kinds.push(k);
        }
        srv.step(1).await;
        let (_, k) = same_state(next_frame(&mut old).await, next_frame(&mut new).await, &mut dec);
        kinds.push(k);
    }
    assert!(moved);
    assert!(kinds.contains(&DELTA), "{kinds:?}");
    assert_eq!(kinds.iter().filter(|&&k| k == KEYFRAME).count(), 1, "one keyframe, after the resync: {kinds:?}");
    let (_, m) = json_req(addr, Method::GET, "/api/metrics", None, None).await;
    assert_eq!(m["resyncs"], 1, "{m}");
    assert_eq!(m["delta_conns"], 1, "{m}");
    assert!(m["keyframes_out"].as_u64().unwrap() >= 2, "{m}");
    assert!(m["state_encode_ns"].as_u64().unwrap() > 0, "{m}");
    drop(plain);
    srv.shutdown().await;
}
