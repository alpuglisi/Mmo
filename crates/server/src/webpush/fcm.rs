//! Firebase Cloud Messaging, HTTP v1 API: notifications to the Android
//! app's registration tokens.
//!
//! Authentication is a Google service account (the JSON key file from the
//! Firebase console): the server signs a JWT with the account's RSA key
//! (RS256, `ring`), trades it at the account's `token_uri` for an OAuth2
//! access token (scope `firebase.messaging`, about an hour; reused until
//! five minutes are left) and sends each message with it to
//! `POST /v1/projects/<project>/messages:send`.
//!
//! Answers:
//! - 2xx: delivered.
//! - 404 `UNREGISTERED`, 400 `INVALID_ARGUMENT` about the registration
//!   token, 403 `SENDER_ID_MISMATCH`: the token is no good (the app was
//!   uninstalled, or the token belongs to another Firebase project): the
//!   caller deletes it.
//! - 401: the access token was refused: fetch a new one and retry.
//! - 429, 5xx, timeouts: retry (`Retry-After`, else backoff), as Web Push.
//! - Anything else: rejected, logged.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ring::rand::SystemRandom;
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use serde::Deserialize;
use serde_json::json;

use super::native::{self, fit, pem_der};
use super::sender::{client, retry_after, unix_now, with_retries, Attempt, Outcome};
use super::service::Payload;
use super::{b64_encode, PushConfig, PushStats};

/// The OAuth2 scope for sending messages.
pub const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
/// Where messages go (tests point it at a mock).
pub const DEFAULT_API: &str = "https://fcm.googleapis.com";
/// Google's token endpoint, when the key file names none.
pub const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
/// The Android notification channel the app creates.
pub const ANDROID_CHANNEL: &str = "evo";
/// An access token is replaced once less than this is left.
const TOKEN_MIN_LEFT: Duration = Duration::from_secs(300);

/// The fields of a service-account key file the server needs.
#[derive(Clone, Deserialize)]
pub struct ServiceAccount {
    pub project_id: String,
    pub client_email: String,
    /// PKCS#8 PEM (`-----BEGIN PRIVATE KEY-----`).
    pub private_key: String,
    #[serde(default)]
    pub private_key_id: Option<String>,
    #[serde(default = "default_token_uri")]
    pub token_uri: String,
}

fn default_token_uri() -> String {
    DEFAULT_TOKEN_URI.into()
}

impl std::fmt::Debug for ServiceAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceAccount")
            .field("project_id", &self.project_id)
            .field("client_email", &self.client_email)
            .field("token_uri", &self.token_uri)
            .finish_non_exhaustive()
    }
}

impl ServiceAccount {
    /// Parse a service-account key file (its JSON text).
    pub fn from_json(text: &str) -> Result<Self, String> {
        let v: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("the FCM service account is not JSON: {e}"))?;
        if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
            if t != "service_account" {
                return Err(format!("the FCM credentials are a {t:?}, not a service account key"));
            }
        }
        let account: ServiceAccount = serde_json::from_value(v).map_err(|e| format!("the FCM service account: {e}"))?;
        if account.project_id.trim().is_empty() || account.client_email.trim().is_empty() {
            return Err("the FCM service account has no project_id or client_email".into());
        }
        Ok(account)
    }
}

/// FCM settings (`PushConfig::fcm`; `--fcm-service-account`).
#[derive(Clone, Debug)]
pub struct FcmConfig {
    pub account: ServiceAccount,
    /// The API origin, [`DEFAULT_API`] unless testing.
    pub api: String,
}

impl FcmConfig {
    pub fn new(account: ServiceAccount) -> Self {
        FcmConfig { account, api: DEFAULT_API.into() }
    }
}

/// RS256 over a service account's key.
pub struct RsaSigner {
    key: RsaKeyPair,
    rng: SystemRandom,
}

impl RsaSigner {
    pub fn from_pem(pem: &str) -> Result<Self, String> {
        let der = pem_der(pem, "PRIVATE KEY")?;
        let key =
            RsaKeyPair::from_pkcs8(&der).map_err(|e| format!("the FCM private key is not a PKCS#8 RSA key: {e}"))?;
        Ok(RsaSigner { key, rng: SystemRandom::new() })
    }

    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let mut sig = vec![0; self.key.public().modulus_len()];
        self.key.sign(&RSA_PKCS1_SHA256, &self.rng, msg, &mut sig).expect("RSA signing");
        sig
    }

    /// The public key (DER `RSAPublicKey`), for checking signatures.
    pub fn public_der(&self) -> &[u8] {
        self.key.public().as_ref()
    }
}

/// The signed assertion traded for an access token (RFC 7523): issued by
/// the service account for `scope`, to the token endpoint, valid an hour
/// from `iat`.
pub fn assertion(account: &ServiceAccount, signer: &RsaSigner, iat: u64) -> String {
    let mut header = json!({ "alg": "RS256", "typ": "JWT" });
    if let Some(kid) = &account.private_key_id {
        header["kid"] = kid.clone().into();
    }
    let claims = json!({
        "iss": account.client_email,
        "scope": SCOPE,
        "aud": account.token_uri,
        "iat": iat,
        "exp": iat + 3600,
    });
    let input = format!("{}.{}", b64_encode(header.to_string().as_bytes()), b64_encode(claims.to_string().as_bytes()));
    let sig = signer.sign(input.as_bytes());
    format!("{input}.{}", b64_encode(&sig))
}

/// The form body of the token request.
pub fn token_request_body(assertion: &str) -> String {
    // The grant type, form-encoded; the JWT is base64url and dots only.
    format!("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}")
}

/// The `messages:send` body for one registration token.
pub fn message(token: &str, p: &Payload, ttl: Duration) -> serde_json::Value {
    json!({
        "message": {
            "token": token,
            "notification": { "title": p.title, "body": fit(&p.body, native::MAX_BODY_BYTES) },
            "data": native::data(p),
            "android": {
                "priority": if native::urgent(p) { "HIGH" } else { "NORMAL" },
                "ttl": format!("{}s", ttl.as_secs()),
                // A newer notification of the same world replaces it, in
                // the queue and in the tray.
                "collapse_key": p.tag,
                // The app's notification channel (`client/src/nativePush.ts`);
                // Android falls back to a default one if it doesn't exist.
                "notification": { "tag": p.tag, "channel_id": ANDROID_CHANNEL },
            },
        }
    })
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// Sends to FCM; one per server.
pub struct Fcm {
    client: reqwest::Client,
    cfg: FcmConfig,
    push: Arc<PushConfig>,
    stats: Arc<PushStats>,
    signer: RsaSigner,
    /// The access token and when to stop using it.
    token: tokio::sync::Mutex<Option<(String, Instant)>>,
}

impl Fcm {
    pub fn new(cfg: FcmConfig, push: Arc<PushConfig>, stats: Arc<PushStats>) -> Result<Self, String> {
        let signer = RsaSigner::from_pem(&cfg.account.private_key)?;
        let client = client(&push, false)?;
        Ok(Fcm { client, cfg, push, stats, signer, token: tokio::sync::Mutex::new(None) })
    }

    pub fn project(&self) -> &str {
        &self.cfg.account.project_id
    }

    /// A valid access token, fetched when there is none or it runs out soon.
    async fn access_token(&self) -> Result<String, Attempt> {
        let mut slot = self.token.lock().await;
        if let Some((t, until)) = &*slot {
            if *until > Instant::now() {
                return Ok(t.clone());
            }
        }
        let body = token_request_body(&assertion(&self.cfg.account, &self.signer, unix_now()));
        let res = self
            .client
            .post(&self.cfg.account.token_uri)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|e| Attempt::Retry { wait: None, why: format!("FCM token request failed: {e}") })?;
        let status = res.status().as_u16();
        let text = res.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            let why = format!("the FCM token request got {status}: {}", text.chars().take(200).collect::<String>());
            return Err(if status >= 500 || status == 429 {
                Attempt::Retry { wait: None, why }
            } else {
                // Wrong credentials: retrying won't help.
                tracing::error!("push: {why}");
                Attempt::Done(Outcome::Failed(why))
            });
        }
        let answer: TokenAnswer = serde_json::from_str(&text)
            .map_err(|e| Attempt::Done(Outcome::Failed(format!("the FCM token answer: {e}"))))?;
        let life = Duration::from_secs(answer.expires_in.unwrap_or(3600));
        let until = Instant::now() + life.saturating_sub(TOKEN_MIN_LEFT);
        *slot = Some((answer.access_token.clone(), until));
        Ok(answer.access_token)
    }

    /// Send `p` to one registration token, retrying as the module docs say.
    pub async fn send(&self, token: &str, p: &Payload) -> Outcome {
        let url = format!("{}/v1/projects/{}/messages:send", self.cfg.api.trim_end_matches('/'), self.project());
        let body = message(token, p, self.push.ttl).to_string();
        with_retries(&self.push, &self.stats, |_| self.attempt(&url, &body)).await
    }

    async fn attempt(&self, url: &str, body: &str) -> Attempt {
        let access = match self.access_token().await {
            Ok(t) => t,
            Err(a) => return a,
        };
        let req = self
            .client
            .post(url)
            .bearer_auth(&access)
            .header("Content-Type", "application/json; charset=utf-8")
            .body(body.to_string());
        let res = match req.send().await {
            Ok(r) => r,
            Err(e) => return Attempt::Retry { wait: None, why: format!("FCM request failed: {e}") },
        };
        let status = res.status().as_u16();
        let wait = retry_after(res.headers());
        if (200..300).contains(&status) {
            return Attempt::Done(Outcome::Delivered);
        }
        let text = res.text().await.unwrap_or_default();
        let (code, message) = error_of(&text);
        match (status, code.as_str()) {
            (401, _) => {
                *self.token.lock().await = None;
                Attempt::Retry { wait: Some(Duration::ZERO), why: "401 from FCM: the access token was refused".into() }
            }
            (404, _) | (_, "UNREGISTERED") | (403, "SENDER_ID_MISMATCH") => Attempt::Done(Outcome::Gone(status)),
            (400, "INVALID_ARGUMENT") if message.to_ascii_lowercase().contains("registration token") => {
                Attempt::Done(Outcome::Gone(status))
            }
            (429, _) | (500..=599, _) => Attempt::Retry { wait, why: format!("{status} from FCM ({code})") },
            _ => {
                tracing::warn!("push: FCM refused a message with {status} {code}: {}", fit(&message, 200));
                Attempt::Done(Outcome::Rejected(status))
            }
        }
    }
}

/// The FCM error code (`details[].errorCode`, else `error.status`) and
/// message of an error answer.
pub fn error_of(text: &str) -> (String, String) {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let e = &v["error"];
    let detail = e["details"].as_array().and_then(|d| d.iter().find_map(|x| x["errorCode"].as_str()));
    let code = detail.or_else(|| e["status"].as_str()).unwrap_or_default().to_string();
    (code, e["message"].as_str().unwrap_or_default().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::PushKind;
    use ring::signature::{UnparsedPublicKey, RSA_PKCS1_2048_8192_SHA256};

    /// A throwaway 2048-bit key, made for the test.
    pub(crate) fn test_key_pem() -> String {
        use rsa::pkcs8::EncodePrivateKey;
        let key = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
        key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap().to_string()
    }

    fn account(pem: String) -> ServiceAccount {
        let json = json!({
            "type": "service_account",
            "project_id": "evo-test",
            "private_key_id": "kid-1",
            "private_key": pem,
            "client_email": "push@evo-test.iam.gserviceaccount.com",
            "client_id": "1",
            "token_uri": "https://oauth2.googleapis.com/token",
        });
        ServiceAccount::from_json(&json.to_string()).unwrap()
    }

    fn part(jwt: &str, i: usize) -> serde_json::Value {
        let seg = jwt.split('.').nth(i).unwrap();
        serde_json::from_slice(&super::super::b64_decode(seg).unwrap()).unwrap()
    }

    #[test]
    fn the_assertion_is_an_rs256_jwt_for_the_token_endpoint() {
        let a = account(test_key_pem());
        let signer = RsaSigner::from_pem(&a.private_key).unwrap();
        let jwt = assertion(&a, &signer, 1_900_000_000);
        assert_eq!(part(&jwt, 0), json!({ "alg": "RS256", "typ": "JWT", "kid": "kid-1" }));
        let claims = part(&jwt, 1);
        assert_eq!(claims["iss"], "push@evo-test.iam.gserviceaccount.com");
        assert_eq!(claims["scope"], SCOPE);
        assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!((claims["iat"].as_u64(), claims["exp"].as_u64()), (Some(1_900_000_000), Some(1_900_003_600)));
        let (input, sig) = jwt.rsplit_once('.').unwrap();
        let sig = super::super::b64_decode(sig).unwrap();
        assert_eq!(sig.len(), 256);
        let public = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, signer.public_der());
        public.verify(input.as_bytes(), &sig).expect("the signature verifies");
        assert!(public.verify(b"something else", &sig).is_err());
        let body = token_request_body(&jwt);
        assert!(body.starts_with("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion=ey"));
    }

    #[test]
    fn key_files_are_checked() {
        assert!(ServiceAccount::from_json("{").is_err());
        assert!(ServiceAccount::from_json(r#"{"type": "authorized_user"}"#).is_err());
        let no_project = json!({"project_id": "", "client_email": "a@b", "private_key": "x"});
        assert!(ServiceAccount::from_json(&no_project.to_string()).is_err());
        let minimal = json!({"project_id": "p", "client_email": "a@b", "private_key": "x"});
        let a = ServiceAccount::from_json(&minimal.to_string()).unwrap();
        assert_eq!(a.token_uri, DEFAULT_TOKEN_URI);
        assert!(!format!("{a:?}").contains("private_key: "), "the key stays out of logs");
        assert!(RsaSigner::from_pem("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----").is_err());
    }

    #[test]
    fn messages_carry_the_notification_and_its_click_data() {
        let p = Payload {
            title: "Mating offer · Main".into(),
            body: "ann asks to mate.".into(),
            world: "main".into(),
            tab: "matings".into(),
            kind: PushKind::Mating,
            count: 1,
            tag: "evo-main".into(),
            organism: Some(42),
        };
        let m = message("tok", &p, Duration::from_secs(43200));
        assert_eq!(
            m,
            json!({ "message": {
                "token": "tok",
                "notification": { "title": "Mating offer · Main", "body": "ann asks to mate." },
                "data": { "world": "main", "tab": "matings", "kind": "mating", "count": "1", "tag": "evo-main", "organism": "42" },
                "android": { "priority": "HIGH", "ttl": "43200s", "collapse_key": "evo-main", "notification": { "tag": "evo-main", "channel_id": "evo" } },
            }})
        );
        let calm = Payload { kind: PushKind::Season, organism: None, body: "x".repeat(5000), ..p };
        let m = message("tok", &calm, Duration::from_secs(60));
        assert_eq!(m["message"]["android"]["priority"], "NORMAL");
        assert!(m["message"]["data"].get("organism").is_none());
        assert!(m["message"]["notification"]["body"].as_str().unwrap().len() <= native::MAX_BODY_BYTES);
    }

    #[test]
    fn error_answers_name_their_code() {
        let unregistered = r#"{"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND",
            "details":[{"@type":"type.googleapis.com/google.firebase.fcm.v1.FcmError","errorCode":"UNREGISTERED"}]}}"#;
        assert_eq!(error_of(unregistered), ("UNREGISTERED".into(), "Requested entity was not found.".into()));
        let plain = r#"{"error":{"code":400,"message":"bad","status":"INVALID_ARGUMENT"}}"#;
        assert_eq!(error_of(plain), ("INVALID_ARGUMENT".into(), "bad".into()));
        assert_eq!(error_of("<html>"), (String::new(), String::new()));
    }
}
