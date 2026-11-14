//! Web Push notifications: players hear about important events (a mating
//! offer, an auction outbid or won, a death of a possessed organism or a
//! founder, a species founded, a season or era ended, a catastrophe) while
//! the game's tab is closed.
//!
//! Standard Web Push: the browser subscribes with the server's VAPID public
//! key (`GET /api/push/key`) and hands the subscription to the server
//! (`POST /api/push/subscribe`); the server encrypts each message for it
//! (RFC 8291, [`ece`]) and POSTs it to the subscription's push service with
//! a VAPID signature (RFC 8292, [`vapid`]); the client's service worker
//! shows it. Subscriptions and push preferences belong to the account,
//! across worlds (`crate::store`); every notification names its world.
//!
//! The world thread never waits on any of it: the game records what is
//! worth a push (`Game::take_notices`), the runner drops those of players
//! connected to that world (they see it in the game) and hands the rest to
//! a [`WorldNotifier`], a non-blocking send on a bounded channel (full:
//! the notice is dropped and counted). The [`service`] task then filters by
//! preferences (the per-kind push switches and the alert preferences: an
//! alert kind hidden from the feed is never pushed), coalesces and rate
//! limits per player ([`coalesce`]: by default one notification collects
//! 30 s of notices, at least 5 minutes apart and at most 4 an hour) and
//! delivers each batch to every subscription of the player in its own task
//! ([`sender`]: retries, 404/410 deletes the subscription).
//!
//! The Android and iOS apps get the same notifications natively ([`native`]):
//! their device tokens go to Firebase Cloud Messaging ([`fcm`]) and the Apple
//! Push Notification service ([`apns`]), each on only when its credentials
//! are configured ([`PushConfig::fcm`], [`PushConfig::apns`]).

pub mod apns;
pub mod coalesce;
pub mod ece;
pub mod fcm;
pub mod native;
pub mod sender;
pub mod service;
pub mod vapid;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use evo_sim::PlayerId;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::protocol::PushKind;

pub use coalesce::Limits;
pub use service::{start, PushService};

/// Base64url without padding (keys, JWTs).
pub fn b64_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Base64url, padded or not; the standard alphabet is accepted too
/// (browsers and libraries differ).
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let s: String = s
        .trim()
        .trim_end_matches('=')
        .chars()
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            c => c,
        })
        .collect();
    URL_SAFE_NO_PAD.decode(s).ok()
}

/// Something worth a push for one player.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub player: PlayerId,
    /// The configured world it happened in (`crate::worlds`).
    pub world: Arc<str>,
    pub world_name: Arc<str>,
    pub kind: PushKind,
    pub text: String,
    pub organism: Option<u64>,
}

/// Push settings (`ServerConfig::push`).
#[derive(Clone)]
pub struct PushConfig {
    /// Off: no service, `GET /api/push/key` says so and subscribing is
    /// refused (`--no-push`).
    pub enabled: bool,
    /// The VAPID private key, base64url (`--vapid-private-key`,
    /// `EVO_VAPID_PRIVATE_KEY`). Without one the server generates a key on
    /// first start and keeps it in the store (`server_secrets`), so
    /// subscriptions survive restarts.
    pub vapid_private_key: Option<String>,
    /// The contact in every VAPID token, `mailto:` or `https:`
    /// (`--vapid-subject`, `EVO_VAPID_SUBJECT`).
    pub subject: String,
    /// Coalescing and rate limits per player.
    pub limits: Limits,
    /// How long a push service keeps an undelivered message (`TTL`).
    pub ttl: Duration,
    /// Requests per message, retries included.
    pub max_attempts: u32,
    /// First retry delay (doubling).
    pub retry_base: Duration,
    /// A `Retry-After` longer than this gives up instead.
    pub max_retry_wait: Duration,
    pub request_timeout: Duration,
    /// Deliveries running at once.
    pub concurrency: usize,
    /// Subscriptions an account keeps; subscribing one more drops the oldest.
    pub max_subscriptions: usize,
    /// Accept endpoints outside the known browser push services, plain
    /// `http` included (tests, private push servers;
    /// `--push-allow-any-endpoint`). Off, an endpoint must be `https` on a
    /// known push service host ([`KNOWN_PUSH_HOSTS`]), so the server can't
    /// be made to POST to arbitrary addresses.
    pub allow_any_endpoint: bool,
    /// Firebase Cloud Messaging for the Android app (`--fcm-service-account`);
    /// `None`: Android devices can't register.
    pub fcm: Option<fcm::FcmConfig>,
    /// The Apple Push Notification service for the iOS app (`--apns-key`,
    /// `--apns-key-id`, `--apns-team-id`); `None`: iOS devices can't register.
    pub apns: Option<apns::ApnsConfig>,
}

impl std::fmt::Debug for PushConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushConfig")
            .field("enabled", &self.enabled)
            .field("vapid_private_key", &self.vapid_private_key.as_ref().map(|_| "<set>"))
            .field("subject", &self.subject)
            .field("limits", &self.limits)
            .field("allow_any_endpoint", &self.allow_any_endpoint)
            .field("fcm", &self.fcm)
            .field("apns", &self.apns)
            .finish_non_exhaustive()
    }
}

impl Default for PushConfig {
    fn default() -> Self {
        PushConfig {
            enabled: true,
            vapid_private_key: None,
            subject: "mailto:admin@example.org".into(),
            limits: Limits { window: Duration::from_secs(30), min_gap: Duration::from_secs(300), per_hour: 4, keep: 4 },
            ttl: Duration::from_secs(12 * 3600),
            max_attempts: 4,
            retry_base: Duration::from_secs(2),
            max_retry_wait: Duration::from_secs(600),
            request_timeout: Duration::from_secs(15),
            concurrency: 16,
            max_subscriptions: 10,
            allow_any_endpoint: false,
            fcm: None,
            apns: None,
        }
    }
}

/// Counters (`GET /api/push/key` reports them to nobody; tests and logs).
#[derive(Debug, Default, Serialize)]
pub struct PushStats {
    /// Notices the world threads handed over.
    pub notices: AtomicU64,
    /// Notices dropped because the channel was full.
    pub dropped: AtomicU64,
    /// Notices the preferences (or no subscription) filtered out.
    pub filtered: AtomicU64,
    /// Coalesced notifications released.
    pub batches: AtomicU64,
    /// Messages a push service accepted.
    pub delivered: AtomicU64,
    /// Subscriptions (and device tokens) the push services reported gone.
    pub gone: AtomicU64,
    /// Messages given up on (rejected, or attempts used up).
    pub failed: AtomicU64,
    /// Retried requests.
    pub retries: AtomicU64,
}

impl PushStats {
    pub fn get(&self, field: &AtomicU64) -> u64 {
        field.load(Ordering::Relaxed)
    }
}

/// Push service hosts of the major browsers (the host, or a subdomain of
/// it): Chrome, Edge and other Chromium (FCM), Firefox (Mozilla autopush),
/// Windows (WNS), Safari (Apple).
pub const KNOWN_PUSH_HOSTS: &[&str] = &[
    "fcm.googleapis.com",
    "android.googleapis.com",
    "push.services.mozilla.com",
    "notify.windows.com",
    "push.apple.com",
];

/// Longest endpoint accepted.
pub const MAX_ENDPOINT: usize = 2048;

/// Check a subscription endpoint (see [`PushConfig::allow_any_endpoint`]).
pub fn check_endpoint(endpoint: &str, allow_any: bool) -> Result<(), String> {
    if endpoint.len() > MAX_ENDPOINT {
        return Err("the endpoint is too long".into());
    }
    let url = reqwest::Url::parse(endpoint).map_err(|e| format!("the endpoint is not a URL: {e}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("the endpoint carries credentials".into());
    }
    if allow_any {
        return match url.scheme() {
            "https" | "http" if url.host_str().is_some() => Ok(()),
            _ => Err("the endpoint must be an http(s) URL".into()),
        };
    }
    if url.scheme() != "https" {
        return Err("the endpoint must be https".into());
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let known = KNOWN_PUSH_HOSTS.iter().any(|k| host == *k || host.ends_with(&format!(".{k}")));
    if !known || url.port().is_some_and(|p| p != 443) {
        return Err(format!("{host} is not a known push service"));
    }
    Ok(())
}

pub(crate) enum Msg {
    Note(Note),
}

/// What a world thread holds: hands notices to the push service without
/// waiting.
#[derive(Clone)]
pub struct WorldNotifier {
    tx: mpsc::Sender<Msg>,
    world: Arc<str>,
    world_name: Arc<str>,
    stats: Arc<PushStats>,
}

impl WorldNotifier {
    /// Queue a notice for `player`; dropped (and counted) if the queue is
    /// full or the service has stopped.
    pub fn notify(&self, player: PlayerId, kind: PushKind, text: String, organism: Option<u64>) {
        let note =
            Note { player, world: self.world.clone(), world_name: self.world_name.clone(), kind, text, organism };
        self.stats.notices.fetch_add(1, Ordering::Relaxed);
        if self.tx.try_send(Msg::Note(note)).is_err() {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_must_be_known_push_services_unless_any_is_allowed() {
        let ok = [
            "https://fcm.googleapis.com/fcm/send/abc:def",
            "https://updates.push.services.mozilla.com/wpush/v2/gAAA",
            "https://wns2-par02p.notify.windows.com/w/?token=x",
            "https://web.push.apple.com/QGuQ",
        ];
        for e in ok {
            assert_eq!(check_endpoint(e, false), Ok(()), "{e}");
        }
        let bad = [
            "http://fcm.googleapis.com/fcm/send/x",
            "https://evil.example.com/fcm.googleapis.com",
            "https://fcm.googleapis.com.evil.example/x",
            "https://notfcm.googleapis.com/x",
            "https://127.0.0.1/push",
            "https://fcm.googleapis.com:8443/x",
            "https://user:pw@fcm.googleapis.com/x",
            "ftp://fcm.googleapis.com/x",
            "not a url",
        ];
        for e in bad {
            assert!(check_endpoint(e, false).is_err(), "{e}");
        }
        assert_eq!(check_endpoint("http://127.0.0.1:9/push/1", true), Ok(()));
        assert!(check_endpoint("file:///etc/passwd", true).is_err());
        assert!(check_endpoint(&format!("https://fcm.googleapis.com/{}", "x".repeat(MAX_ENDPOINT)), false).is_err());
    }

    #[test]
    fn base64_accepts_both_alphabets_and_padding() {
        assert_eq!(b64_decode("-_8").unwrap(), vec![0xfb, 0xff]);
        assert_eq!(b64_decode("+/8=").unwrap(), vec![0xfb, 0xff]);
        assert_eq!(b64_encode(&[0xfb, 0xff]), "-_8");
        assert!(b64_decode("!!").is_none());
    }
}
