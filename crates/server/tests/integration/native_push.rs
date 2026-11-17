//! Native push (FCM for Android, APNs for iOS) against mock services: one
//! axum server on 127.0.0.1 plays Google's token endpoint, the FCM v1 API
//! and APNs (over HTTP/2, h2c), records every request and answers each
//! device token with scripted statuses. No real push service is contacted
//! and no real credentials exist: the keys are made for the test, so this
//! checks the requests the server makes and how it reads the answers, not
//! delivery to a phone.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::{HeaderMap, Method, StatusCode, Uri, Version};
use axum::response::{IntoResponse, Response};
use axum::Router;
use evo_server::game::WorldSettings;
use evo_server::protocol::{NativeService, PushKind};
use evo_server::store::{MemoryStore, Store};
use evo_server::webpush::apns::{self, ApnsConfig};
use evo_server::webpush::fcm::{FcmConfig, RsaSigner, ServiceAccount, SCOPE};
use evo_server::webpush::{b64_decode, Limits, PushConfig};
use evo_server::{start, ServerConfig, SharedStore};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::pkcs8::EncodePrivateKey as _;
use ring::signature::{UnparsedPublicKey, RSA_PKCS1_2048_8192_SHA256};
use serde_json::{json, Value};

/// One request the mock received.
#[derive(Clone, Debug)]
struct Got {
    version: Version,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Got {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).map(|v| v.to_str().unwrap()).unwrap_or("")
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

#[derive(Default)]
struct MockInner {
    /// Answers (status, JSON body) per device token, in order; success once
    /// used up.
    script: HashMap<String, VecDeque<(u16, Value)>>,
    /// FCM messages and APNs notifications by device token.
    got: HashMap<String, Vec<Got>>,
    /// Token requests (form bodies), and the access tokens handed out.
    token_requests: Vec<String>,
}

#[derive(Clone, Default)]
struct Mock(Arc<Mutex<MockInner>>);

impl Mock {
    async fn serve() -> (Mock, SocketAddr) {
        let mock = Mock::default();
        let m = mock.clone();
        let app = Router::new().fallback(move |method, uri, version, headers, body| {
            let m = m.clone();
            async move { m.handle(method, uri, version, headers, body) }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (mock, addr)
    }

    fn handle(
        &self,
        method: Method,
        uri: Uri,
        version: Version,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> Response {
        assert_eq!(method, Method::POST);
        let mut g = self.0.lock().unwrap();
        let path = uri.path();
        if path == "/token" {
            g.token_requests.push(String::from_utf8(body.to_vec()).unwrap());
            let n = g.token_requests.len();
            let answer = json!({ "access_token": format!("at-{n}"), "expires_in": 3599, "token_type": "Bearer" });
            return axum::Json(answer).into_response();
        }
        let token = if let Some(t) = path.strip_prefix("/3/device/") {
            t.to_string()
        } else if path == "/v1/projects/evo-test/messages:send" {
            let v: Value = serde_json::from_slice(&body).unwrap();
            v["message"]["token"].as_str().unwrap().to_string()
        } else {
            return StatusCode::NOT_FOUND.into_response();
        };
        g.got.entry(token.clone()).or_default().push(Got { version, headers, body: body.to_vec() });
        match g.script.get_mut(&token).and_then(|q| q.pop_front()) {
            Some((status, body)) => (StatusCode::from_u16(status).unwrap(), axum::Json(body)).into_response(),
            None if path.starts_with("/3/") => StatusCode::OK.into_response(),
            None => axum::Json(json!({ "name": "projects/evo-test/messages/1" })).into_response(),
        }
    }

    fn script(&self, token: &str, answers: Vec<(u16, Value)>) {
        self.0.lock().unwrap().script.insert(token.into(), answers.into());
    }

    fn got(&self, token: &str) -> Vec<Got> {
        self.0.lock().unwrap().got.get(token).cloned().unwrap_or_default()
    }

    fn token_requests(&self) -> Vec<String> {
        self.0.lock().unwrap().token_requests.clone()
    }
}

fn rsa_pem() -> String {
    let key = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
    key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap().to_string()
}

fn p8_pem() -> String {
    let key = p256::SecretKey::random(&mut rand::rngs::OsRng);
    key.to_pkcs8_pem(p256::pkcs8::LineEnding::LF).unwrap().to_string()
}

fn fcm_config(mock: SocketAddr, pem: &str) -> FcmConfig {
    let key_file = json!({
        "type": "service_account",
        "project_id": "evo-test",
        "private_key_id": "kid-1",
        "private_key": pem,
        "client_email": "push@evo-test.iam.gserviceaccount.com",
        "token_uri": format!("http://{mock}/token"),
    });
    FcmConfig {
        api: format!("http://{mock}"),
        ..FcmConfig::new(ServiceAccount::from_json(&key_file.to_string()).unwrap())
    }
}

fn apns_config(mock: SocketAddr, pem: &str) -> ApnsConfig {
    ApnsConfig {
        key_pem: pem.into(),
        key_id: "KEY1234567".into(),
        team_id: "TEAM765432".into(),
        topic: "org.evolutionmmo.client".into(),
        sandbox: true,
        api: Some(format!("http://{mock}")),
    }
}

fn config(fcm: Option<FcmConfig>, apns: Option<ApnsConfig>) -> PushConfig {
    PushConfig {
        limits: Limits { window: Duration::from_millis(50), min_gap: Duration::ZERO, per_hour: 60, keep: 4 },
        retry_base: Duration::from_millis(20),
        max_retry_wait: Duration::from_secs(2),
        subject: "mailto:tests@example.org".into(),
        fcm,
        apns,
        ..Default::default()
    }
}

async fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !ok() {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn jwt_part(jwt: &str, i: usize) -> Value {
    serde_json::from_slice(&b64_decode(jwt.split('.').nth(i).unwrap()).unwrap()).unwrap()
}

/// The FCM path: a service-account JWT traded for an access token, one
/// `messages:send` per token with it; gone tokens are deleted, a refused
/// access token is replaced, 5xx retried, other refusals given up.
#[tokio::test]
async fn fcm_requests_and_answers() {
    let (mock, addr) = Mock::serve().await;
    let pem = rsa_pem();
    let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
    let push =
        evo_server::webpush::start(store.clone(), config(Some(fcm_config(addr, &pem)), None)).await.unwrap().unwrap();
    assert_eq!(push.native_services(), vec![NativeService::Fcm]);
    let not_found = json!({"error": {"code": 404, "message": "Requested entity was not found.", "status": "NOT_FOUND",
        "details": [{"@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": "UNREGISTERED"}]}});
    let invalid = json!({"error": {"code": 400, "status": "INVALID_ARGUMENT",
        "message": "The registration token is not a valid FCM registration token"}});
    let mismatch = json!({"error": {"code": 403, "status": "PERMISSION_DENIED", "message": "SenderId mismatch",
        "details": [{"@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": "SENDER_ID_MISMATCH"}]}});
    let bad =
        json!({"error": {"code": 400, "status": "INVALID_ARGUMENT", "message": "Invalid JSON payload received."}});
    let unavailable = json!({"error": {"code": 503, "status": "UNAVAILABLE", "message": "try later"}});
    let unauthenticated = json!({"error": {"code": 401, "status": "UNAUTHENTICATED", "message": "expired"}});
    mock.script("unregistered", vec![(404, not_found)]);
    mock.script("invalid", vec![(400, invalid)]);
    mock.script("mismatch", vec![(403, mismatch)]);
    mock.script("bad", vec![(400, bad)]);
    mock.script("flaky", vec![(503, unavailable.clone()), (500, unavailable)]);
    mock.script("reauth", vec![(401, unauthenticated)]);
    let tokens = ["ok", "unregistered", "invalid", "mismatch", "bad", "flaky", "reauth"];
    for t in tokens {
        push.register_device(1, NativeService::Fcm, t, Some("Pixel".into())).await.unwrap();
    }
    push.notifier("main", "Main").notify(1, PushKind::Mating, "ann asks to mate.".into(), Some(42));
    let stats = push.stats();
    eventually("every delivery to end", || {
        stats.get(&stats.delivered) + stats.get(&stats.gone) + stats.get(&stats.failed) == 7
    })
    .await;
    assert_eq!(stats.get(&stats.delivered), 3, "ok, flaky and reauth");
    assert_eq!(stats.get(&stats.gone), 3, "unregistered, invalid and mismatch");
    assert_eq!(stats.get(&stats.failed), 1, "bad");
    let left: Vec<String> = store.load_push_devices(1).await.unwrap().into_iter().map(|d| d.token).collect();
    assert_eq!(left.len(), 4);
    for gone in ["unregistered", "invalid", "mismatch"] {
        assert!(!left.iter().any(|t| t == gone), "{gone} was deleted");
    }
    let counts: Vec<usize> = tokens.iter().map(|t| mock.got(t).len()).collect();
    assert_eq!(counts, vec![1, 1, 1, 1, 1, 3, 2]);

    // The token request: an RS256 assertion from the service account.
    let reqs = mock.token_requests();
    assert!(reqs.len() >= 2, "a refused access token is replaced: {reqs:?}");
    let assertion = reqs[0]
        .strip_prefix("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion=")
        .expect("a JWT bearer grant");
    assert_eq!(jwt_part(assertion, 0), json!({ "alg": "RS256", "typ": "JWT", "kid": "kid-1" }));
    let claims = jwt_part(assertion, 1);
    assert_eq!(claims["iss"], "push@evo-test.iam.gserviceaccount.com");
    assert_eq!(claims["scope"], SCOPE);
    assert_eq!(claims["aud"], format!("http://{addr}/token"));
    assert_eq!(claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap(), 3600);
    let (input, sig) = assertion.rsplit_once('.').unwrap();
    let public = RsaSigner::from_pem(&pem).unwrap();
    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public.public_der())
        .verify(input.as_bytes(), &b64_decode(sig).unwrap())
        .expect("the assertion is signed with the service account's key");

    // The message.
    let got = &mock.got("ok")[0];
    assert!(got.header("authorization").starts_with("Bearer at-"));
    assert_eq!(got.header("content-type"), "application/json; charset=utf-8");
    assert_eq!(
        got.json(),
        json!({ "message": {
            "token": "ok",
            "notification": { "title": "Mating offer · Main", "body": "ann asks to mate." },
            "data": { "world": "main", "tab": "matings", "kind": "mating", "count": "1", "tag": "evo-main", "organism": "42" },
            "android": { "priority": "HIGH", "ttl": "43200s", "collapse_key": "evo-main", "notification": { "tag": "evo-main", "channel_id": "evo" } },
        }})
    );
    // The retry after a 401 carried a new access token.
    let reauth = mock.got("reauth");
    assert_ne!(reauth[0].header("authorization"), reauth[1].header("authorization"));
}

/// The APNs path: HTTP/2, an ES256 provider token, the alert and click
/// data; gone tokens are deleted, an expired provider token is replaced,
/// 429 retried, other refusals given up.
#[tokio::test]
async fn apns_requests_and_answers() {
    let (mock, addr) = Mock::serve().await;
    let pem = p8_pem();
    let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
    let push =
        evo_server::webpush::start(store.clone(), config(None, Some(apns_config(addr, &pem)))).await.unwrap().unwrap();
    assert_eq!(push.native_services(), vec![NativeService::Apns]);
    let tok = |c: char| c.to_string().repeat(64);
    let (ok, gone, bad, topic, expired, throttled, big) =
        (tok('a'), tok('b'), tok('c'), tok('d'), tok('e'), tok('f'), tok('0'));
    mock.script(&gone, vec![(410, json!({"reason": "Unregistered", "timestamp": 1}))]);
    mock.script(&bad, vec![(400, json!({"reason": "BadDeviceToken"}))]);
    mock.script(&topic, vec![(400, json!({"reason": "DeviceTokenNotForTopic"}))]);
    mock.script(&expired, vec![(403, json!({"reason": "ExpiredProviderToken"}))]);
    mock.script(&throttled, vec![(429, json!({"reason": "TooManyRequests"}))]);
    mock.script(&big, vec![(413, json!({"reason": "PayloadTooLarge"}))]);
    let tokens = [&ok, &gone, &bad, &topic, &expired, &throttled, &big];
    for t in tokens {
        // Tokens arrive upper-case from some libraries; they are stored lower-case.
        push.register_device(7, NativeService::Apns, &t.to_uppercase(), None).await.unwrap();
    }
    push.notifier("isle", "Isle").notify(7, PushKind::Season, "Season 3 has ended.".into(), None);
    let stats = push.stats();
    eventually("every delivery to end", || {
        stats.get(&stats.delivered) + stats.get(&stats.gone) + stats.get(&stats.failed) == 7
    })
    .await;
    assert_eq!(stats.get(&stats.delivered), 3, "ok, expired and throttled");
    assert_eq!(stats.get(&stats.gone), 3, "gone, bad and topic");
    assert_eq!(stats.get(&stats.failed), 1, "big");
    assert_eq!(stats.get(&stats.retries), 2);
    let left: Vec<String> = store.load_push_devices(7).await.unwrap().into_iter().map(|d| d.token).collect();
    assert_eq!(left.len(), 4);
    assert!(![&gone, &bad, &topic].iter().any(|t| left.contains(t)));
    let counts: Vec<usize> = tokens.iter().map(|t| mock.got(t).len()).collect();
    assert_eq!(counts, vec![1, 1, 1, 1, 2, 2, 1]);

    let got = &mock.got(&ok)[0];
    assert_eq!(got.version, Version::HTTP_2, "APNs speaks HTTP/2");
    assert_eq!(got.header("apns-topic"), "org.evolutionmmo.client");
    assert_eq!(got.header("apns-push-type"), "alert");
    assert_eq!(got.header("apns-priority"), "5");
    assert_eq!(got.header("apns-collapse-id"), "evo-isle");
    let expiration: u64 = got.header("apns-expiration").parse().unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert!(expiration.abs_diff(now + 43200) < 60, "expires after the TTL");
    assert_eq!(
        got.json(),
        json!({
            "aps": { "alert": { "title": "Season results · Isle", "body": "Season 3 has ended." }, "sound": "default", "thread-id": "evo-isle" },
            "world": "isle", "tab": "seasons", "kind": "season", "count": "1", "tag": "evo-isle",
        })
    );
    let jwt = got.header("authorization").strip_prefix("bearer ").expect("a bearer provider token");
    assert_eq!(jwt_part(jwt, 0), json!({ "alg": "ES256", "kid": "KEY1234567" }));
    assert_eq!(jwt_part(jwt, 1)["iss"], "TEAM765432");
    assert!(jwt_part(jwt, 1)["iat"].as_u64().unwrap().abs_diff(now) < 60);
    let (input, sig) = jwt.rsplit_once('.').unwrap();
    let key = apns::signing_key(&pem).unwrap();
    let sig = Signature::from_slice(&b64_decode(sig).unwrap()).unwrap();
    VerifyingKey::from(&key).verify(input.as_bytes(), &sig).expect("signed with the .p8 key");
    let _: &SigningKey = &key;
}

// --- Through the server: the HTTP API.

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

async fn server(push: PushConfig, store: Arc<MemoryStore>) -> evo_server::RunningServer {
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        store: Some(SharedStore(store)),
        world: WorldSettings { seed: 3, width: 32, height: 32, wild: false, ..Default::default() },
        push,
        ..Default::default()
    }
    .manual();
    start(cfg).await.unwrap()
}

#[tokio::test]
async fn devices_register_over_http_only_for_configured_services() {
    let (mock, maddr) = Mock::serve().await;
    let store = Arc::new(MemoryStore::default());
    let srv = server(config(Some(fcm_config(maddr, &rsa_pem())), None), store.clone()).await;
    let addr = srv.addr;
    let (_, key) = req(addr, "GET", "/api/push/key", None, None).await;
    assert_eq!(key["native"], json!(["fcm"]));

    let (s, v) =
        req(addr, "POST", "/api/register", None, Some(json!({"name": "dee", "password": "a long password"}))).await;
    assert_eq!(s, 201, "{v}");
    let (token, player) = (v["token"].as_str().unwrap().to_string(), v["player"].as_u64().unwrap() as u32);
    let device = json!({ "service": "fcm", "token": "fcm-token:1", "label": "Pixel 9" });
    assert_eq!(req(addr, "POST", "/api/push/native/subscribe", None, Some(device.clone())).await.0, 401);
    let (s, v) = req(addr, "POST", "/api/push/native/subscribe", Some(&token), Some(device.clone())).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((&v["service"], &v["label"]), (&json!("fcm"), &json!("Pixel 9")));
    let bad = json!({ "service": "fcm", "token": "has spaces" });
    let (s, v) = req(addr, "POST", "/api/push/native/subscribe", Some(&token), Some(bad)).await;
    assert_eq!((s, v["code"].as_str()), (400, Some("bad_subscription")));
    let ios = json!({ "service": "apns", "token": "ab".repeat(32) });
    let (s, v) = req(addr, "POST", "/api/push/native/subscribe", Some(&token), Some(ios)).await;
    assert_eq!((s, v["code"].as_str()), (503, Some("push_disabled")), "no APNs credentials");
    let (s, _) =
        req(addr, "POST", "/api/push/native/subscribe", Some(&token), Some(json!({"service": "x", "token": "t"})))
            .await;
    assert!(s == 400 || s == 422, "unknown service: {s}");

    let (_, list) = req(addr, "GET", "/api/push/subscriptions", Some(&token), None).await;
    assert_eq!(list["subscriptions"], json!([]));
    assert_eq!(list["devices"].as_array().unwrap().len(), 1);
    assert_eq!(list["devices"][0]["service"], "fcm");
    assert!(list["devices"][0].get("token").is_none(), "tokens stay on the server");

    assert!(mock.token_requests().is_empty(), "no access token before the first message");

    let gone = json!({ "service": "fcm", "token": "fcm-token:1" });
    assert_eq!(
        req(addr, "POST", "/api/push/native/unsubscribe", Some(&token), Some(gone.clone())).await.1["removed"],
        true
    );
    assert_eq!(req(addr, "POST", "/api/push/native/unsubscribe", Some(&token), Some(gone)).await.1["removed"], false);
    assert!(store.load_push_devices(player).await.unwrap().is_empty());
    srv.shutdown().await;
}

/// Without credentials nothing native is offered: the key view is as it
/// was, and registering is refused.
#[tokio::test]
async fn without_credentials_native_push_is_off() {
    let store = Arc::new(MemoryStore::default());
    let srv = server(config(None, None), store).await;
    let (_, key) = req(srv.addr, "GET", "/api/push/key", None, None).await;
    assert_eq!(key["enabled"], true);
    assert!(key.get("native").is_none(), "{key}");
    let (_, v) =
        req(srv.addr, "POST", "/api/register", None, Some(json!({"name": "eve", "password": "a long password"}))).await;
    let token = v["token"].as_str().unwrap().to_string();
    let device = json!({ "service": "fcm", "token": "abc" });
    let (s, v) = req(srv.addr, "POST", "/api/push/native/subscribe", Some(&token), Some(device)).await;
    assert_eq!((s, v["code"].as_str()), (503, Some("push_disabled")));
    let (_, list) = req(srv.addr, "GET", "/api/push/subscriptions", Some(&token), None).await;
    assert!(list.get("devices").is_none(), "{list}");
    srv.shutdown().await;
}
