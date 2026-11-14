//! Apple Push Notification service, token-based: notifications to the iOS
//! app's device tokens.
//!
//! The provider token is a JWT signed with the team's APNs key (the `.p8`
//! file from the Apple Developer account, a P-256 key: ES256 with `p256`),
//! naming the key id and the team; Apple wants it replaced at most every 20
//! minutes and at least every hour, so one is reused for 40 minutes. Each
//! message is `POST /3/device/<token>` over HTTP/2 (APNs speaks nothing
//! else) to `api.push.apple.com`, or `api.sandbox.push.apple.com` for
//! development builds (`--apns-sandbox`), with the app's bundle id as the
//! topic.
//!
//! Answers:
//! - 200: delivered.
//! - 410 (`Unregistered`), 400 `BadDeviceToken` or `DeviceTokenNotForTopic`:
//!   the token is no good: the caller deletes it.
//! - 403 `ExpiredProviderToken`: sign a new provider token and retry.
//! - 429, 5xx, timeouts: retry (`Retry-After`, else backoff), as Web Push.
//! - Anything else (another 400 or 403, 413): rejected, logged.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::DecodePrivateKey;
use serde_json::json;

use super::native::{self, fit};
use super::sender::{client, retry_after, unix_now, with_retries, Attempt, Outcome};
use super::service::Payload;
use super::{b64_encode, PushConfig, PushStats};

pub const PRODUCTION: &str = "https://api.push.apple.com";
pub const SANDBOX: &str = "https://api.sandbox.push.apple.com";
/// The Capacitor app's bundle id (`client/capacitor.config.ts`).
pub const DEFAULT_TOPIC: &str = "org.evolutionmmo.client";
/// How long a provider token is reused.
const TOKEN_REUSE: u64 = 40 * 60;
/// `apns-collapse-id` is at most 64 bytes.
const MAX_COLLAPSE_ID: usize = 64;

/// APNs settings (`PushConfig::apns`; `--apns-key` and friends).
#[derive(Clone)]
pub struct ApnsConfig {
    /// The `.p8` key: PKCS#8 PEM of a P-256 key.
    pub key_pem: String,
    /// The key's 10-character id (Apple Developer, Keys).
    pub key_id: String,
    /// The team id (the JWT's issuer).
    pub team_id: String,
    /// The app's bundle id.
    pub topic: String,
    /// The development environment (apps signed for development).
    pub sandbox: bool,
    /// The API origin instead of Apple's (tests).
    pub api: Option<String>,
}

impl std::fmt::Debug for ApnsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApnsConfig")
            .field("key_id", &self.key_id)
            .field("team_id", &self.team_id)
            .field("topic", &self.topic)
            .field("sandbox", &self.sandbox)
            .finish_non_exhaustive()
    }
}

impl ApnsConfig {
    pub fn api(&self) -> &str {
        match &self.api {
            Some(a) => a,
            None if self.sandbox => SANDBOX,
            None => PRODUCTION,
        }
    }
}

/// The ES256 key from a `.p8` file.
pub fn signing_key(pem: &str) -> Result<SigningKey, String> {
    let pem = pem.replace("\\n", "\n");
    SigningKey::from_pkcs8_pem(pem.trim()).map_err(|e| format!("the APNs key is not a PKCS#8 P-256 key: {e}"))
}

/// A provider token issued by `team_id` at `iat`, naming `key_id`.
pub fn provider_token(key: &SigningKey, key_id: &str, team_id: &str, iat: u64) -> String {
    let header = json!({ "alg": "ES256", "kid": key_id });
    let claims = json!({ "iss": team_id, "iat": iat });
    let input = format!("{}.{}", b64_encode(header.to_string().as_bytes()), b64_encode(claims.to_string().as_bytes()));
    let sig: Signature = key.sign(input.as_bytes());
    format!("{input}.{}", b64_encode(&sig.to_bytes()))
}

/// The JSON body for a notification: the alert in `aps`, the click data
/// (world, tab, kind, count, tag, organism) beside it.
pub fn body(p: &Payload) -> serde_json::Value {
    let mut v = json!({
        "aps": {
            "alert": { "title": p.title, "body": fit(&p.body, native::MAX_BODY_BYTES) },
            "sound": "default",
            "thread-id": p.tag,
        }
    });
    for (k, val) in native::data(p) {
        v[k] = val;
    }
    v
}

/// The request headers besides the authorization: (name, value).
pub fn headers(p: &Payload, topic: &str, ttl: Duration, now: u64) -> Vec<(&'static str, String)> {
    vec![
        ("apns-topic", topic.to_string()),
        ("apns-push-type", "alert".into()),
        ("apns-priority", if native::urgent(p) { "10" } else { "5" }.into()),
        ("apns-expiration", (now + ttl.as_secs()).to_string()),
        ("apns-collapse-id", fit(&p.tag, MAX_COLLAPSE_ID)),
    ]
}

/// Sends to APNs; one per server.
pub struct Apns {
    client: reqwest::Client,
    cfg: ApnsConfig,
    push: Arc<PushConfig>,
    stats: Arc<PushStats>,
    key: SigningKey,
    /// The provider token and when it was issued.
    token: Mutex<Option<(String, u64)>>,
}

impl Apns {
    pub fn new(cfg: ApnsConfig, push: Arc<PushConfig>, stats: Arc<PushStats>) -> Result<Self, String> {
        let key = signing_key(&cfg.key_pem)?;
        if cfg.key_id.trim().is_empty() || cfg.team_id.trim().is_empty() || cfg.topic.trim().is_empty() {
            return Err("APNs needs a key id, a team id and a topic (the app's bundle id)".into());
        }
        let client = client(&push, true)?;
        Ok(Apns { client, cfg, push, stats, key, token: Mutex::new(None) })
    }

    pub fn config(&self) -> &ApnsConfig {
        &self.cfg
    }

    fn provider_token(&self) -> String {
        let now = unix_now();
        let mut slot = self.token.lock().unwrap();
        if let Some((t, iat)) = &*slot {
            if now < iat + TOKEN_REUSE {
                return t.clone();
            }
        }
        let t = provider_token(&self.key, &self.cfg.key_id, &self.cfg.team_id, now);
        *slot = Some((t.clone(), now));
        t
    }

    /// Send `p` to one device token, retrying as the module docs say.
    pub async fn send(&self, token: &str, p: &Payload) -> Outcome {
        let url = format!("{}/3/device/{token}", self.cfg.api().trim_end_matches('/'));
        let body = body(p).to_string();
        with_retries(&self.push, &self.stats, |_| self.attempt(&url, p, &body)).await
    }

    async fn attempt(&self, url: &str, p: &Payload, body: &str) -> Attempt {
        let mut req = self
            .client
            .post(url)
            .header("authorization", format!("bearer {}", self.provider_token()))
            .header("content-type", "application/json")
            .body(body.to_string());
        for (k, v) in headers(p, &self.cfg.topic, self.push.ttl, unix_now()) {
            req = req.header(k, v);
        }
        let res = match req.send().await {
            Ok(r) => r,
            Err(e) => return Attempt::Retry { wait: None, why: format!("APNs request failed: {e}") },
        };
        let status = res.status().as_u16();
        let wait = retry_after(res.headers());
        if status == 200 {
            return Attempt::Done(Outcome::Delivered);
        }
        let text = res.text().await.unwrap_or_default();
        let reason = reason_of(&text);
        match (status, reason.as_str()) {
            (410, _) | (400, "BadDeviceToken" | "DeviceTokenNotForTopic") => Attempt::Done(Outcome::Gone(status)),
            (403, "ExpiredProviderToken") => {
                *self.token.lock().unwrap() = None;
                Attempt::Retry { wait: Some(Duration::ZERO), why: "403 ExpiredProviderToken from APNs".into() }
            }
            (429, _) | (500..=599, _) => Attempt::Retry { wait, why: format!("{status} from APNs ({reason})") },
            _ => {
                tracing::warn!("push: APNs refused a message with {status} {reason}");
                Attempt::Done(Outcome::Rejected(status))
            }
        }
    }
}

/// The `reason` of an APNs error answer.
pub fn reason_of(text: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    v["reason"].as_str().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::PushKind;
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::VerifyingKey;
    use p256::pkcs8::EncodePrivateKey;

    pub(crate) fn test_key_pem() -> String {
        let key = p256::SecretKey::random(&mut rand::rngs::OsRng);
        key.to_pkcs8_pem(p256::pkcs8::LineEnding::LF).unwrap().to_string()
    }

    fn part(jwt: &str, i: usize) -> serde_json::Value {
        let seg = jwt.split('.').nth(i).unwrap();
        serde_json::from_slice(&super::super::b64_decode(seg).unwrap()).unwrap()
    }

    #[test]
    fn the_provider_token_is_an_es256_jwt_naming_key_and_team() {
        let pem = test_key_pem();
        let key = signing_key(&pem).unwrap();
        // A key pasted into one line with escaped line breaks works too.
        assert!(signing_key(&pem.replace('\n', "\\n")).is_ok());
        let jwt = provider_token(&key, "ABC123DEFG", "TEAM456789", 1_900_000_000);
        assert_eq!(part(&jwt, 0), json!({ "alg": "ES256", "kid": "ABC123DEFG" }));
        assert_eq!(part(&jwt, 1), json!({ "iss": "TEAM456789", "iat": 1_900_000_000u64 }));
        let (input, sig) = jwt.rsplit_once('.').unwrap();
        let sig = Signature::from_slice(&super::super::b64_decode(sig).unwrap()).unwrap();
        VerifyingKey::from(&key).verify(input.as_bytes(), &sig).expect("the signature verifies");
        assert!(signing_key("not a key").is_err());
    }

    #[test]
    fn requests_carry_the_alert_the_click_data_and_the_headers() {
        let p = Payload {
            title: "Death · Main".into(),
            body: "Founder #3 starved.".into(),
            world: "main".into(),
            tab: "genetics".into(),
            kind: PushKind::Death,
            count: 2,
            tag: "evo-main".into(),
            organism: Some(3),
        };
        assert_eq!(
            body(&p),
            json!({
                "aps": { "alert": { "title": "Death · Main", "body": "Founder #3 starved." }, "sound": "default", "thread-id": "evo-main" },
                "world": "main", "tab": "genetics", "kind": "death", "count": "2", "tag": "evo-main", "organism": "3",
            })
        );
        let h = headers(&p, "org.example.app", Duration::from_secs(600), 1_000);
        let get = |n: &str| h.iter().find(|(k, _)| *k == n).map(|(_, v)| v.as_str());
        assert_eq!(get("apns-topic"), Some("org.example.app"));
        assert_eq!(get("apns-push-type"), Some("alert"));
        assert_eq!(get("apns-priority"), Some("10"));
        assert_eq!(get("apns-expiration"), Some("1600"));
        assert_eq!(get("apns-collapse-id"), Some("evo-main"));
        let calm = Payload { kind: PushKind::Market, tag: format!("evo-{}", "w".repeat(80)), ..p };
        let h = headers(&calm, "t", Duration::ZERO, 0);
        assert_eq!(h[2].1, "5");
        assert!(h[4].1.len() <= MAX_COLLAPSE_ID);
    }

    #[test]
    fn the_environment_picks_the_host() {
        let cfg = ApnsConfig {
            key_pem: String::new(),
            key_id: "k".into(),
            team_id: "t".into(),
            topic: DEFAULT_TOPIC.into(),
            sandbox: false,
            api: None,
        };
        assert_eq!(cfg.api(), PRODUCTION);
        assert_eq!(ApnsConfig { sandbox: true, ..cfg.clone() }.api(), SANDBOX);
        assert_eq!(ApnsConfig { api: Some("http://127.0.0.1:9".into()), ..cfg.clone() }.api(), "http://127.0.0.1:9");
        assert!(!format!("{cfg:?}").contains("key_pem"));
        assert_eq!(reason_of(r#"{"reason":"BadDeviceToken"}"#), "BadDeviceToken");
        assert_eq!(reason_of(""), "");
    }
}
