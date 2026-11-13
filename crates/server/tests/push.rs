//! Web Push against a mock push service: an axum server on 127.0.0.1 in the
//! test that records what it receives and answers each subscription with
//! scripted statuses. No real push service is ever contacted (endpoints are
//! local, `allow_any_endpoint` lets them through).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use evo_server::game::WorldSettings;
use evo_server::protocol::{AlertKind, AlertPrefs, PushKind, PushPrefs};
use evo_server::store::{AlertPrefsRecord, MemoryStore, PushPrefsRecord, Store};
use evo_server::webpush::service::Payload;
use evo_server::webpush::{b64_encode, ece, vapid, Limits, PushConfig, PushService};
use evo_server::{start, ServerConfig, SharedStore};
use futures_util::StreamExt;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

/// One request the mock push service received.
#[derive(Clone)]
struct Got {
    headers: HeaderMap,
    body: Vec<u8>,
}

#[derive(Default)]
struct MockInner {
    /// Statuses (and `Retry-After` seconds) to answer, per subscription id,
    /// in order; 201 once used up.
    script: HashMap<String, VecDeque<(u16, Option<u64>)>>,
    got: HashMap<String, Vec<Got>>,
}

#[derive(Clone, Default)]
struct Mock(Arc<Mutex<MockInner>>);

impl Mock {
    async fn serve() -> (Mock, SocketAddr) {
        let mock = Mock::default();
        let app = Router::new().route("/push/:id", post(receive)).with_state(mock.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (mock, addr)
    }

    fn script(&self, id: &str, answers: &[(u16, Option<u64>)]) {
        self.0.lock().unwrap().script.insert(id.into(), answers.iter().copied().collect());
    }

    fn got(&self, id: &str) -> Vec<Got> {
        self.0.lock().unwrap().got.get(id).cloned().unwrap_or_default()
    }
}

async fn receive(
    State(mock): State<Mock>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let mut g = mock.0.lock().unwrap();
    g.got.entry(id.clone()).or_default().push(Got { headers, body: body.to_vec() });
    let (status, retry) = g.script.get_mut(&id).and_then(|q| q.pop_front()).unwrap_or((201, None));
    let mut res = StatusCode::from_u16(status).unwrap().into_response();
    if let Some(s) = retry {
        res.headers_mut().insert("retry-after", s.into());
    }
    res
}

/// A browser's side of a subscription: its key pair and auth secret.
struct Device {
    id: String,
    key: SecretKey,
    auth: [u8; 16],
    endpoint: String,
}

impl Device {
    fn new(mock: SocketAddr, id: &str) -> Self {
        let key = SecretKey::random(&mut rand::rngs::OsRng);
        let auth: [u8; 16] = rand::random();
        Device { id: id.into(), key, auth, endpoint: format!("http://{mock}/push/{id}") }
    }

    fn p256dh(&self) -> String {
        b64_encode(self.key.public_key().to_encoded_point(false).as_bytes())
    }

    fn json(&self) -> Value {
        json!({ "endpoint": self.endpoint, "keys": { "p256dh": self.p256dh(), "auth": b64_encode(&self.auth) } })
    }

    async fn subscribe(&self, push: &PushService, player: u32) {
        push.subscribe(player, &self.endpoint, &self.p256dh(), &b64_encode(&self.auth), None).await.unwrap();
    }

    /// Decrypt what the mock received for this device.
    fn open(&self, got: &Got) -> Payload {
        let plain = ece::decrypt(&self.key, &self.auth, &got.body).expect("decrypts");
        serde_json::from_slice(&plain).expect("a payload")
    }
}

fn config(limits: Limits) -> PushConfig {
    PushConfig {
        limits,
        retry_base: Duration::from_millis(20),
        max_retry_wait: Duration::from_secs(2),
        allow_any_endpoint: true,
        subject: "mailto:tests@example.org".into(),
        ..Default::default()
    }
}

fn quick() -> Limits {
    Limits { window: Duration::from_millis(50), min_gap: Duration::ZERO, per_hour: 60, keep: 4 }
}

async fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !ok() {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn header<'a>(g: &'a Got, name: &str) -> &'a str {
    g.headers.get(name).map(|v| v.to_str().unwrap()).unwrap_or("")
}

/// 201 delivers; 404 and 410 delete the subscription; 429 waits
/// `Retry-After` and retries; 5xx backs off and retries; a `Retry-After`
/// past the limit and a 400 give up.
#[tokio::test]
async fn deliveries_follow_the_push_service_answers() {
    let (mock, addr) = Mock::serve().await;
    let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
    let push = evo_server::webpush::start(store.clone(), config(quick())).await.unwrap().expect("push on");
    let devices: Vec<Device> = ["ok", "gone404", "gone410", "limited", "flaky", "throttled", "bad"]
        .iter()
        .map(|id| Device::new(addr, id))
        .collect();
    mock.script("gone404", &[(404, None)]);
    mock.script("gone410", &[(410, None)]);
    mock.script("limited", &[(429, Some(0)), (429, Some(1))]);
    mock.script("flaky", &[(500, None), (503, None)]);
    mock.script("throttled", &[(429, Some(3600))]);
    mock.script("bad", &[(400, None)]);
    for d in &devices {
        d.subscribe(&push, 1).await;
    }
    assert_eq!(store.load_push_subscriptions(1).await.unwrap().len(), 7);

    push.notifier("main", "Main").notify(1, PushKind::Death, "Founder #3 starved.".into(), Some(3));
    let stats = push.stats();
    eventually("every delivery to end", || {
        stats.get(&stats.delivered) + stats.get(&stats.gone) + stats.get(&stats.failed) == 7
    })
    .await;
    assert_eq!(stats.get(&stats.delivered), 3, "ok, limited and flaky");
    assert_eq!(stats.get(&stats.gone), 2);
    assert_eq!(stats.get(&stats.failed), 2, "throttled and bad");
    assert_eq!(stats.get(&stats.retries), 4, "limited twice, flaky twice");
    assert_eq!(stats.get(&stats.batches), 1);

    let left: Vec<String> = store.load_push_subscriptions(1).await.unwrap().into_iter().map(|s| s.endpoint).collect();
    assert_eq!(left.len(), 5);
    assert!(!left.iter().any(|e| e.ends_with("/gone404") || e.ends_with("/gone410")));
    let counts: Vec<usize> = devices.iter().map(|d| mock.got(&d.id).len()).collect();
    assert_eq!(counts, vec![1, 1, 1, 3, 3, 1, 1]);

    // What arrived: an aes128gcm message for the device, a VAPID token for
    // this server's key and the push service's origin.
    let got = &mock.got("ok")[0];
    assert_eq!(header(got, "content-encoding"), "aes128gcm");
    assert_eq!(header(got, "ttl"), "43200");
    assert_eq!(header(got, "urgency"), "high");
    let claims = vapid::verify(header(got, "authorization"), push.public_key()).expect("valid VAPID");
    assert_eq!(claims["aud"], format!("http://{addr}"));
    assert_eq!(claims["sub"], "mailto:tests@example.org");
    let p = devices[0].open(got);
    assert_eq!(p.title, "Death · Main");
    assert_eq!(p.body, "Founder #3 starved.");
    assert_eq!((p.world.as_str(), p.tab.as_str(), p.kind, p.count), ("main", "genetics", PushKind::Death, 1));
    assert_eq!(p.organism, Some(3));
    // Every retry carried the same message.
    for g in mock.got("flaky") {
        assert_eq!(devices[4].open(&g), p);
    }
}

/// The per-kind push switches and the alert preferences filter notices; a
/// burst goes out as one notification; the hourly cap holds the rest.
#[tokio::test]
async fn preferences_filter_and_bursts_coalesce_under_the_cap() {
    let (mock, addr) = Mock::serve().await;
    let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
    let limits = Limits { window: Duration::from_millis(200), min_gap: Duration::ZERO, per_hour: 2, keep: 3 };
    let push = evo_server::webpush::start(store.clone(), config(limits)).await.unwrap().unwrap();
    let phone = Device::new(addr, "phone");
    phone.subscribe(&push, 1).await;
    store
        .put_push_prefs(&PushPrefsRecord { player: 1, prefs: PushPrefs { off: vec![PushKind::Season] } })
        .await
        .unwrap();
    let alerts = AlertPrefs { hidden: vec![AlertKind::Market], toast: vec![] };
    store.put_alert_prefs(&AlertPrefsRecord { player: 1, prefs: alerts }).await.unwrap();
    push.invalidate(1);

    let main = push.notifier("main", "Main");
    let isle = push.notifier("isle", "Isle");
    main.notify(1, PushKind::Season, "Season 3 has ended.".into(), None);
    main.notify(1, PushKind::Market, "Outbid on auction #4.".into(), Some(9));
    main.notify(1, PushKind::Catastrophe, "Catastrophe: a flood has begun.".into(), None);
    isle.notify(1, PushKind::Mating, "bob asks to mate.".into(), Some(5));
    main.notify(1, PushKind::Death, "Possessed #8 was eaten.".into(), Some(8));
    // Player 2 has no subscription.
    main.notify(2, PushKind::Death, "Founder #1 died.".into(), Some(1));
    let stats = push.stats();
    eventually("the first notification", || stats.get(&stats.delivered) == 1).await;
    assert_eq!(stats.get(&stats.filtered), 3);
    let got = mock.got("phone");
    assert_eq!(got.len(), 1);
    let p = phone.open(&got[0]);
    assert_eq!(p.count, 3);
    assert_eq!(p.title, "3 events in 2 worlds");
    assert_eq!(
        p.body,
        "[Isle] bob asks to mate.\n[Main] Possessed #8 was eaten.\n[Main] Catastrophe: a flood has begun."
    );
    assert_eq!((p.world.as_str(), p.tab.as_str(), p.kind), ("isle", "matings", PushKind::Mating));

    // Turning a kind off applies at once (the HTTP handlers invalidate).
    store
        .put_push_prefs(&PushPrefsRecord { player: 1, prefs: PushPrefs { off: vec![PushKind::Death] } })
        .await
        .unwrap();
    push.invalidate(1);
    main.notify(1, PushKind::Death, "Founder #2 starved.".into(), Some(2));
    main.notify(1, PushKind::Species, "A new species arose.".into(), None);
    eventually("the second notification", || stats.get(&stats.delivered) == 2).await;
    assert_eq!(phone.open(&mock.got("phone")[1]).body, "A new species arose.");

    // Two an hour: a third waits.
    main.notify(1, PushKind::Catastrophe, "Catastrophe: a drought has begun.".into(), None);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(stats.get(&stats.batches), 2);
    assert_eq!(mock.got("phone").len(), 2);
}

/// A world thread never waits on push: notices beyond the queue are dropped.
#[tokio::test]
async fn a_full_queue_drops_notices_without_waiting() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
    let push = evo_server::webpush::start(store, config(quick())).await.unwrap().unwrap();
    let n = push.notifier("main", "Main");
    let total = evo_server::webpush::service::QUEUE + 5000;
    let started = std::time::Instant::now();
    // On this (current-thread) runtime the service can't run until we
    // yield, so the queue fills.
    for i in 0..total {
        n.notify(1, PushKind::Death, format!("#{i}"), None);
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    let stats = push.stats();
    assert_eq!(stats.get(&stats.notices), total as u64);
    assert_eq!(stats.get(&stats.dropped), 5000);
}

/// The VAPID key is generated once and kept in the store; a configured key
/// wins; push can be switched off.
#[tokio::test]
async fn the_vapid_key_is_generated_once_and_kept() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
    let a = evo_server::webpush::start(store.clone(), config(quick())).await.unwrap().unwrap();
    let b = evo_server::webpush::start(store.clone(), config(quick())).await.unwrap().unwrap();
    assert_eq!(a.public_key(), b.public_key());
    let key = vapid::VapidKey::generate();
    let cfg = PushConfig { vapid_private_key: Some(key.private_b64()), ..config(quick()) };
    let c = evo_server::webpush::start(store.clone(), cfg).await.unwrap().unwrap();
    assert_eq!(c.public_key(), key.public_b64());
    let off = PushConfig { enabled: false, ..config(quick()) };
    assert!(evo_server::webpush::start(store, off).await.unwrap().is_none());
}

// --- Through the server: the HTTP API, and notices from the game.

async fn req(addr: SocketAddr, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;
    use hyper_util::client::legacy::Client;
    use hyper_util::rt::TokioExecutor;
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let mut r = hyper::Request::builder().method(method).uri(format!("http://{addr}{path}"));
    if let Some(t) = token {
        r = r.header("authorization", format!("Bearer {t}"));
    }
    if body.is_some() {
        r = r.header("content-type", "application/json");
    }
    let body = Full::new(Bytes::from(body.map(|b| b.to_string()).unwrap_or_default()));
    let res = client.request(r.body(body).unwrap()).await.unwrap();
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn register(addr: SocketAddr, name: &str) -> (String, u32) {
    let (s, v) =
        req(addr, "POST", "/api/register", None, Some(json!({"name": name, "password": "a long password"}))).await;
    assert_eq!(s, 201, "{v}");
    (v["token"].as_str().unwrap().into(), v["player"].as_u64().unwrap() as u32)
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn first_state(addr: SocketAddr, token: &str) -> (Ws, Value) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?token={token}")).await.unwrap();
    loop {
        let Some(Ok(Message::Text(t))) = ws.next().await else { panic!("socket closed") };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "state" {
            return (ws, v);
        }
    }
}

fn pick(state: &Value, sex: &str) -> u64 {
    let ok = |o: &&Value| o["sex"] == sex && o["matured"] == true && o["pregnant"] == false;
    state["organisms"].as_array().unwrap().iter().find(ok).unwrap()["id"].as_u64().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscriptions_over_http_and_a_mating_offer_reaches_an_offline_player() {
    let (mock, maddr) = Mock::serve().await;
    let store = Arc::new(MemoryStore::default());
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        store: Some(SharedStore(store.clone())),
        world: WorldSettings { seed: 11, width: 64, height: 64, wild: true, ..Default::default() },
        push: config(quick()),
        ..Default::default()
    }
    .fast()
    .manual();
    let srv = start(cfg).await.unwrap();
    let addr = srv.addr;

    let (s, key) = req(addr, "GET", "/api/push/key", None, None).await;
    assert_eq!(s, 200);
    assert_eq!(key["enabled"], true);
    assert_eq!(evo_server::webpush::b64_decode(key["public_key"].as_str().unwrap()).unwrap().len(), 65);
    assert_eq!(key["kinds"].as_array().unwrap().len(), PushKind::ALL.len());

    let (ann, _) = register(addr, "ann").await;
    let (bob, bob_id) = register(addr, "bob").await;
    let phone = Device::new(maddr, "bob-phone");
    let laptop = Device::new(maddr, "ann-laptop");

    // Subscribing needs a session and valid keys.
    assert_eq!(req(addr, "POST", "/api/push/subscribe", None, Some(phone.json())).await.0, 401);
    let mut bad = phone.json();
    bad["keys"]["auth"] = json!("AAAA");
    let (s, v) = req(addr, "POST", "/api/push/subscribe", Some(&bob), Some(bad)).await;
    assert_eq!((s, v["code"].as_str()), (400, Some("bad_subscription")));
    let mut labelled = phone.json();
    labelled["label"] = json!("Bob's phone");
    let (s, v) = req(addr, "POST", "/api/push/subscribe", Some(&bob), Some(labelled)).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["label"], "Bob's phone");
    assert_eq!(req(addr, "POST", "/api/push/subscribe", Some(&ann), Some(laptop.json())).await.0, 200);
    let (_, list) = req(addr, "GET", "/api/push/subscriptions", Some(&bob), None).await;
    assert_eq!(list["subscriptions"].as_array().unwrap().len(), 1);
    assert_eq!(list["subscriptions"][0]["endpoint"], phone.endpoint);
    assert!(list["subscriptions"][0].get("p256dh").is_none(), "keys stay on the server");

    // Preferences: every kind on by default; saved normalized.
    let (_, prefs) = req(addr, "GET", "/api/push/prefs", Some(&bob), None).await;
    assert_eq!(prefs, json!({ "off": [] }));
    let (s, prefs) =
        req(addr, "POST", "/api/push/prefs", Some(&bob), Some(json!({ "off": ["season", "market", "season"] }))).await;
    assert_eq!((s, prefs), (200, json!({ "off": ["market", "season"] })));
    assert_eq!(req(addr, "GET", "/api/push/prefs", Some(&bob), None).await.1, json!({ "off": ["market", "season"] }));

    // Both found; bob looks once, then closes the game; ann stays.
    for (t, n) in [(&ann, "Ann's"), (&bob, "Bob's")] {
        let design = json!({"name": n, "thermo": "warm", "birth": "live", "litter_size": 2,
            "traits": {"body_size": 0.4, "fur": 0.4, "keen_nose": 0.3}, "founders": 12});
        assert_eq!(req(addr, "POST", "/api/lineage", Some(t), Some(design)).await.0, 201);
    }
    srv.step(1).await;
    let (mut wa, sa) = first_state(addr, &ann).await;
    let (wb, sb) = first_state(addr, &bob).await;
    let (ann_m, bob_m) = (pick(&sa, "male"), pick(&sb, "male"));
    drop(wb);
    let mut online = 2;
    for _ in 0..200 {
        srv.step(1).await;
        online = req(addr, "GET", "/api/worlds", None, None).await.1["worlds"][0]["players_online"].as_u64().unwrap();
        if online == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(online, 1, "bob's socket closed");
    // Bob, briefly back, asks ann (online: no push for her).
    let (mut bob_ws, sb) = first_state(addr, &bob).await;
    let m = json!({"id": 1, "type": "request_mating", "mine": pick(&sb, "female"), "partner": ann_m});
    futures_util::SinkExt::send(&mut bob_ws, Message::Text(m.to_string())).await.unwrap();
    loop {
        let Some(Ok(Message::Text(t))) = bob_ws.next().await else { panic!("socket closed") };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "result" && v["id"] == 1 {
            assert_eq!(v["ok"], true, "{v}");
            break;
        }
    }
    drop(bob_ws);
    for _ in 0..200 {
        srv.step(1).await;
        let n = req(addr, "GET", "/api/worlds", None, None).await.1["worlds"][0]["players_online"].as_u64().unwrap();
        if n == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Ann asks bob, who is offline: his phone gets it. (A female of hers
    // that is not pregnant now.)
    let tick = srv.step(1).await;
    let ann_f = loop {
        let Some(Ok(Message::Text(t))) = wa.next().await else { panic!("socket closed") };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "state" && v["clock"]["tick"] == tick {
            break pick(&v, "female");
        }
    };
    let m = json!({"id": 2, "type": "request_mating", "mine": ann_f, "partner": bob_m, "price": 5});
    futures_util::SinkExt::send(&mut wa, Message::Text(m.to_string())).await.unwrap();
    loop {
        let Some(Ok(Message::Text(t))) = wa.next().await else { panic!("socket closed") };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "result" && v["id"] == 2 {
            assert_eq!(v["ok"], true, "{v}");
            break;
        }
    }
    eventually("bob's push", || !mock.got("bob-phone").is_empty()).await;
    let p = phone.open(&mock.got("bob-phone")[0]);
    assert_eq!((p.kind, p.world.as_str(), p.tab.as_str()), (PushKind::Mating, "main", "matings"));
    assert_eq!(p.title, "Mating offer · Main");
    assert_eq!(p.body, format!("ann asks to mate their #{ann_f} with your #{bob_m} for 5 renown."));
    assert_eq!(p.organism, Some(bob_m));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(mock.got("ann-laptop").is_empty(), "ann was online");

    // Unsubscribing: only your own.
    let gone = json!({ "endpoint": phone.endpoint });
    assert_eq!(req(addr, "POST", "/api/push/unsubscribe", Some(&ann), Some(gone.clone())).await.1["removed"], false);
    assert_eq!(req(addr, "POST", "/api/push/unsubscribe", Some(&bob), Some(gone)).await.1["removed"], true);
    assert!(store.load_push_subscriptions(bob_id).await.unwrap().is_empty());
    srv.shutdown().await;
}

#[tokio::test]
async fn push_can_be_switched_off() {
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        world: WorldSettings { seed: 3, width: 32, height: 32, wild: false, ..Default::default() },
        push: PushConfig { enabled: false, ..Default::default() },
        ..Default::default()
    }
    .manual();
    let srv = start(cfg).await.unwrap();
    let (s, key) = req(srv.addr, "GET", "/api/push/key", None, None).await;
    assert_eq!((s, &key["enabled"]), (200, &json!(false)));
    assert!(key.get("public_key").is_none());
    let (token, _) = register(srv.addr, "cat").await;
    let dev = Device::new(SocketAddr::from(([127, 0, 0, 1], 9)), "x");
    let (s, v) = req(srv.addr, "POST", "/api/push/subscribe", Some(&token), Some(dev.json())).await;
    assert_eq!((s, v["code"].as_str()), (503, Some("push_disabled")));
    srv.shutdown().await;
}
