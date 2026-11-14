//! Delivering one encrypted message to one subscription: the push service
//! request (RFC 8030) with VAPID authorization, and what its answer means.
//!
//! - 2xx: delivered (accepted by the push service).
//! - 404, 410: the subscription is gone (expired or unsubscribed): the
//!   caller deletes it.
//! - 429: rate limited: wait `Retry-After` (seconds; a backoff step if
//!   absent) and try again, unless that is longer than `max_retry_wait`.
//! - 5xx, timeouts and connection errors: try again after an exponential
//!   backoff (`retry_base`, doubling, ±20 % jitter).
//! - Any other status (400, 401, 403, 413): rejected, not retried.
//!
//! At most `max_attempts` requests per message. The native senders
//! (`super::fcm`, `super::apns`) retry the same way ([`with_retries`]).

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand::Rng;
use reqwest::header::{HeaderMap, RETRY_AFTER};

use super::vapid::VapidKey;
use super::{PushConfig, PushStats};

/// How a delivery ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    /// 404 or 410: delete the subscription.
    Gone(u16),
    /// A status that retrying won't fix.
    Rejected(u16),
    /// Attempts used up (or a retry wait too long); the last problem.
    Failed(String),
}

/// What to do after one attempt (shared by the Web Push, FCM and APNs
/// senders).
pub(crate) enum Attempt {
    Done(Outcome),
    Retry { wait: Option<Duration>, why: String },
}

/// The HTTPS client for a push service: the configured timeout, no
/// redirects; `http2_only` for services that speak nothing else (APNs).
pub(crate) fn client(cfg: &PushConfig, http2_only: bool) -> Result<reqwest::Client, String> {
    let mut b = reqwest::Client::builder().timeout(cfg.request_timeout).redirect(reqwest::redirect::Policy::none());
    if http2_only {
        b = b.http2_prior_knowledge();
    }
    b.build().map_err(|e| format!("push client: {e}"))
}

fn backoff(cfg: &PushConfig, attempt: u32) -> Duration {
    let base = cfg.retry_base.saturating_mul(1 << attempt.min(16));
    base.mul_f64(rand::thread_rng().gen_range(0.8..1.2))
}

/// Run `attempt` (given the attempt number) until it is done, at most
/// `max_attempts` times, waiting as the module docs say in between.
pub(crate) async fn with_retries<F, Fut>(cfg: &PushConfig, stats: &PushStats, mut attempt: F) -> Outcome
where
    F: FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = Attempt>,
{
    let attempts = cfg.max_attempts.max(1);
    let mut last = String::new();
    for n in 0..attempts {
        match attempt(n).await {
            Attempt::Done(outcome) => return outcome,
            Attempt::Retry { wait, why } => {
                last = why;
                if n + 1 == attempts {
                    break;
                }
                let wait = wait.unwrap_or_else(|| backoff(cfg, n));
                if wait > cfg.max_retry_wait {
                    return Outcome::Failed(format!("{last}; asked to wait {} s", wait.as_secs()));
                }
                stats.retries.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(wait).await;
            }
        }
    }
    Outcome::Failed(last)
}

/// Valid for 12 hours (the most RFC 8292 recommends is 24); reused while
/// more than an hour is left.
const JWT_LIFETIME: u64 = 12 * 3600;
const JWT_MIN_LEFT: u64 = 3600;

pub struct Sender {
    client: reqwest::Client,
    key: VapidKey,
    cfg: Arc<PushConfig>,
    stats: Arc<PushStats>,
    /// Authorization headers by audience, with their expiry.
    tokens: Mutex<HashMap<String, (String, u64)>>,
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Sender {
    pub fn new(key: VapidKey, cfg: Arc<PushConfig>, stats: Arc<PushStats>) -> Result<Self, String> {
        let client = client(&cfg, false)?;
        Ok(Sender { client, key, cfg, stats, tokens: Mutex::new(HashMap::new()) })
    }

    pub fn key(&self) -> &VapidKey {
        &self.key
    }

    fn authorization(&self, audience: &str) -> String {
        let now = unix_now();
        let mut tokens = self.tokens.lock().unwrap();
        if let Some((header, exp)) = tokens.get(audience) {
            if *exp > now + JWT_MIN_LEFT {
                return header.clone();
            }
        }
        let exp = now + JWT_LIFETIME;
        let header = self.key.authorization(audience, exp, &self.cfg.subject);
        tokens.insert(audience.to_string(), (header.clone(), exp));
        header
    }

    /// Deliver an encrypted body (`aes128gcm`) to `endpoint`, retrying as
    /// the module docs say. `urgency`: `high`, `normal` or `low`.
    pub async fn send(&self, endpoint: &str, body: &[u8], urgency: &str) -> Outcome {
        let url = match reqwest::Url::parse(endpoint) {
            Ok(u) => u,
            Err(e) => return Outcome::Failed(format!("the endpoint does not parse: {e}")),
        };
        let audience = url.origin().ascii_serialization();
        with_retries(&self.cfg, &self.stats, |_| self.attempt(&url, &audience, body, urgency)).await
    }

    async fn attempt(&self, url: &reqwest::Url, audience: &str, body: &[u8], urgency: &str) -> Attempt {
        let req = self
            .client
            .post(url.clone())
            .header("TTL", self.cfg.ttl.as_secs().to_string())
            .header("Urgency", urgency)
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("Authorization", self.authorization(audience))
            .body(body.to_vec());
        let res = match req.send().await {
            Ok(r) => r,
            Err(e) => return Attempt::Retry { wait: None, why: format!("request failed: {e}") },
        };
        let status = res.status().as_u16();
        match status {
            200..=299 => Attempt::Done(Outcome::Delivered),
            404 | 410 => Attempt::Done(Outcome::Gone(status)),
            429 => Attempt::Retry { wait: retry_after(res.headers()), why: "429 rate limited".into() },
            500..=599 => {
                Attempt::Retry { wait: retry_after(res.headers()), why: format!("{status} from the push service") }
            }
            _ => {
                let text = res.text().await.unwrap_or_default();
                tracing::warn!(
                    "push service refused a message with {status}: {}",
                    text.chars().take(200).collect::<String>()
                );
                Attempt::Done(Outcome::Rejected(status))
            }
        }
    }
}

/// `Retry-After` in seconds (an HTTP date is ignored: backoff instead).
pub(crate) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let secs: u64 = headers.get(RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(secs))
}
