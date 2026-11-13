//! The push service task: preferences, coalescing, delivery.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use evo_sim::PlayerId;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Semaphore};

use super::coalesce::{Batch, Coalescer};
use super::sender::{Outcome, Sender};
use super::vapid::VapidKey;
use super::{b64_decode, b64_encode, check_endpoint, ece, Msg, PushConfig, PushStats, WorldNotifier};
use crate::protocol::{AlertPrefs, PushKind, PushPrefs};
use crate::store::{PushSubscriptionRecord, Store, StoreError};

/// Notices queued between the world threads and the service.
pub const QUEUE: usize = 8192;
/// How long a player's preferences are trusted before they are read again
/// (changes made through this server apply at once; this covers other
/// servers sharing the database).
const FILTER_TTL: Duration = Duration::from_secs(300);
/// The name of the generated VAPID key in the store's secrets.
pub const VAPID_SECRET: &str = "vapid_private_key";
/// Longest notification body.
const MAX_BODY_CHARS: usize = 1000;
/// Longest device label kept.
const MAX_LABEL: usize = 120;

/// What decides whether a player gets a notice.
#[derive(Clone)]
struct Filter {
    push: PushPrefs,
    alerts: AlertPrefs,
    subscribed: bool,
    loaded: Instant,
}

impl Filter {
    fn allows(&self, kind: PushKind) -> bool {
        self.subscribed && self.push.wants(kind) && kind.alert_kind().is_none_or(|a| self.alerts.shows(a))
    }
}

/// The decrypted message a service worker receives.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    pub title: String,
    pub body: String,
    /// The world of the most important notice (the one a click opens).
    pub world: String,
    /// The client tab a click opens.
    pub tab: String,
    pub kind: PushKind,
    /// Notices coalesced into this notification.
    pub count: usize,
    /// One notification per world replaces the previous one.
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organism: Option<u64>,
}

fn label(kind: PushKind) -> &'static str {
    match kind {
        PushKind::Mating => "Mating offer",
        PushKind::Death => "Death",
        PushKind::Market => "Market",
        PushKind::Species => "New species",
        PushKind::Season => "Season results",
        PushKind::Catastrophe => "Catastrophe",
    }
}

/// The notification for a batch: the most important notice leads (its
/// world and tab are what a click opens), the others follow, one a line.
pub fn payload(batch: &Batch) -> Payload {
    let lead = &batch.notes[0];
    let mut worlds: Vec<&str> = batch.notes.iter().map(|n| &*n.world).collect();
    worlds.sort_unstable();
    worlds.dedup();
    let title = if batch.total == 1 {
        format!("{} · {}", label(lead.kind), lead.world_name)
    } else if worlds.len() == 1 {
        format!("{} events · {}", batch.total, lead.world_name)
    } else {
        format!("{} events in {} worlds", batch.total, worlds.len())
    };
    let mut lines: Vec<String> = batch
        .notes
        .iter()
        .map(|n| if worlds.len() > 1 { format!("[{}] {}", n.world_name, n.text) } else { n.text.clone() })
        .collect();
    if batch.total > batch.notes.len() {
        lines.push(format!("…and {} more", batch.total - batch.notes.len()));
    }
    let mut body = lines.join("\n");
    if body.chars().count() > MAX_BODY_CHARS {
        body = body.chars().take(MAX_BODY_CHARS - 1).collect::<String>() + "…";
    }
    Payload {
        title,
        body,
        world: lead.world.to_string(),
        tab: lead.kind.tab().to_string(),
        kind: lead.kind,
        count: batch.total,
        tag: format!("evo-{}", lead.world),
        organism: lead.organism,
    }
}

/// Why a subscription was refused.
#[derive(Debug)]
pub enum SubscribeError {
    Invalid(String),
    Store(StoreError),
}

/// What the HTTP handlers, the notifiers and the service task share.
struct Shared {
    key: VapidKey,
    cfg: Arc<PushConfig>,
    stats: Arc<PushStats>,
    store: Arc<dyn Store>,
    filters: Mutex<HashMap<PlayerId, Filter>>,
}

/// The running push service (shared by the HTTP handlers and the world
/// threads' notifiers). The task stops once the service and every notifier
/// are dropped.
pub struct PushService {
    tx: mpsc::Sender<Msg>,
    shared: Arc<Shared>,
}

impl Shared {
    fn invalidate(&self, player: PlayerId) {
        self.filters.lock().unwrap().remove(&player);
    }

    async fn filter(&self, player: PlayerId) -> Result<Filter, StoreError> {
        if let Some(f) = self.filters.lock().unwrap().get(&player) {
            if f.loaded.elapsed() < FILTER_TTL {
                return Ok(f.clone());
            }
        }
        let push = self.store.load_push_prefs(player).await?.map(|r| r.prefs.normalized()).unwrap_or_default();
        let alerts = self.store.load_alert_prefs(player).await?.map(|r| r.prefs.normalized()).unwrap_or_default();
        let subscribed = !self.store.load_push_subscriptions(player).await?.is_empty();
        let f = Filter { push, alerts, subscribed, loaded: Instant::now() };
        self.filters.lock().unwrap().insert(player, f.clone());
        Ok(f)
    }
}

impl PushService {
    /// The VAPID public key (base64url): the browser's `applicationServerKey`.
    pub fn public_key(&self) -> &str {
        self.shared.key.public_b64()
    }

    pub fn config(&self) -> &PushConfig {
        &self.shared.cfg
    }

    pub fn stats(&self) -> &PushStats {
        &self.shared.stats
    }

    /// A notifier for one world's thread.
    pub fn notifier(&self, world: &str, world_name: &str) -> WorldNotifier {
        WorldNotifier {
            tx: self.tx.clone(),
            world: world.into(),
            world_name: world_name.into(),
            stats: self.shared.stats.clone(),
        }
    }

    /// Forget what is cached about a player (their preferences changed).
    pub fn invalidate(&self, player: PlayerId) {
        self.shared.invalidate(player);
    }

    /// Validate and store a subscription for `player`; an account over
    /// [`PushConfig::max_subscriptions`] loses its oldest.
    pub async fn subscribe(
        &self,
        player: PlayerId,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        label: Option<String>,
    ) -> Result<PushSubscriptionRecord, SubscribeError> {
        let cfg = &self.shared.cfg;
        let store = &self.shared.store;
        check_endpoint(endpoint, cfg.allow_any_endpoint).map_err(SubscribeError::Invalid)?;
        let key = b64_decode(p256dh).ok_or_else(|| SubscribeError::Invalid("p256dh is not base64url".into()))?;
        let secret = b64_decode(auth).ok_or_else(|| SubscribeError::Invalid("auth is not base64url".into()))?;
        ece::check_keys(&key, &secret).map_err(|e| SubscribeError::Invalid(e.to_string()))?;
        let label = label.map(|l| l.trim().chars().take(MAX_LABEL).collect::<String>()).filter(|l| !l.is_empty());
        let record = PushSubscriptionRecord {
            player,
            endpoint: endpoint.to_string(),
            p256dh: b64_encode(&key),
            auth: b64_encode(&secret),
            created_unix: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            label,
        };
        store.put_push_subscription(&record).await.map_err(SubscribeError::Store)?;
        let subs = store.load_push_subscriptions(player).await.map_err(SubscribeError::Store)?;
        let excess = subs.len().saturating_sub(cfg.max_subscriptions.max(1));
        for old in subs.iter().filter(|s| s.endpoint != record.endpoint).take(excess) {
            store.delete_push_subscription(&old.endpoint).await.map_err(SubscribeError::Store)?;
        }
        // The endpoint may have belonged to another account (a shared
        // device): forget every cached filter, not just this player's.
        self.shared.filters.lock().unwrap().clear();
        Ok(record)
    }

    /// Delete one of `player`'s subscriptions; false if they have none with
    /// that endpoint.
    pub async fn unsubscribe(&self, player: PlayerId, endpoint: &str) -> Result<bool, StoreError> {
        let store = &self.shared.store;
        let owned = store.load_push_subscriptions(player).await?.iter().any(|s| s.endpoint == endpoint);
        if !owned {
            return Ok(false);
        }
        let removed = store.delete_push_subscription(endpoint).await?;
        self.invalidate(player);
        Ok(removed)
    }
}

/// Start the push service: the VAPID key from the config, else the one in
/// the store, else a new one (stored). `None` when push is disabled.
pub async fn start(store: Arc<dyn Store>, cfg: PushConfig) -> Result<Option<PushService>, String> {
    if !cfg.enabled {
        return Ok(None);
    }
    let key = match &cfg.vapid_private_key {
        Some(k) => VapidKey::from_private_b64(k)?,
        None => {
            let fresh = VapidKey::generate();
            let stored = store
                .put_secret_if_absent(VAPID_SECRET, &fresh.private_b64())
                .await
                .map_err(|e| format!("could not store the VAPID key: {e}"))?;
            let key = VapidKey::from_private_b64(&stored).map_err(|e| format!("the stored VAPID key: {e}"))?;
            if key.public_b64() == fresh.public_b64() {
                tracing::info!("generated a VAPID key for push notifications (kept in the store)");
            }
            key
        }
    };
    if cfg.subject == PushConfig::default().subject {
        tracing::warn!(
            "push notifications use the placeholder VAPID subject; set --vapid-subject mailto:you@your.domain"
        );
    }
    let cfg = Arc::new(cfg);
    let stats = Arc::new(PushStats::default());
    let sender = Arc::new(Sender::new(key.clone(), cfg.clone(), stats.clone())?);
    let (tx, rx) = mpsc::channel(QUEUE);
    let shared = Arc::new(Shared { key, cfg, stats, store, filters: Mutex::default() });
    tokio::spawn(run(shared.clone(), sender, rx));
    tracing::info!("push notifications on (VAPID public key {})", shared.key.public_b64());
    Ok(Some(PushService { tx, shared }))
}

async fn run(shared: Arc<Shared>, sender: Arc<Sender>, mut rx: mpsc::Receiver<Msg>) {
    let mut coalescer = Coalescer::new(shared.cfg.limits);
    let permits = Arc::new(Semaphore::new(shared.cfg.concurrency.max(1)));
    loop {
        let due = coalescer.next_due();
        let wake = tokio::time::Instant::from_std(due.unwrap_or_else(|| Instant::now() + Duration::from_secs(3600)));
        tokio::select! {
            msg = rx.recv() => {
                let Some(Msg::Note(note)) = msg else { break };
                let allowed = match shared.filter(note.player).await {
                    Ok(f) => f.allows(note.kind),
                    Err(e) => {
                        tracing::warn!("push: could not read the preferences of player {}: {e}", note.player);
                        false
                    }
                };
                if allowed {
                    coalescer.add(note, Instant::now());
                } else {
                    shared.stats.filtered.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ = tokio::time::sleep_until(wake), if due.is_some() => {
                for batch in coalescer.take_due(Instant::now()) {
                    shared.stats.batches.fetch_add(1, Ordering::Relaxed);
                    let (shared, sender, permits) = (shared.clone(), sender.clone(), permits.clone());
                    tokio::spawn(async move {
                        let Ok(_permit) = permits.acquire_owned().await else { return };
                        deliver(&shared, &sender, batch).await;
                    });
                }
            }
        }
    }
}

/// Send a batch to every subscription of its player.
async fn deliver(shared: &Shared, sender: &Sender, batch: Batch) {
    let player = batch.player;
    let subs = match shared.store.load_push_subscriptions(player).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("push: could not read the subscriptions of player {player}: {e}");
            return;
        }
    };
    let p = payload(&batch);
    let urgency = if matches!(p.kind, PushKind::Mating | PushKind::Death) { "high" } else { "normal" };
    let json = serde_json::to_vec(&p).expect("payload serializes");
    let sends = subs.iter().map(|s| {
        let json = &json;
        async move {
            let body = match (b64_decode(&s.p256dh), b64_decode(&s.auth)) {
                (Some(k), Some(a)) => ece::encrypt(&k, &a, json),
                _ => Err(ece::EceError::BadPublicKey),
            };
            let outcome = match body {
                Ok(body) => sender.send(&s.endpoint, &body, urgency).await,
                Err(e) => Outcome::Failed(e.to_string()),
            };
            (s, outcome)
        }
    });
    let stats = &shared.stats;
    for (s, outcome) in futures_util::future::join_all(sends).await {
        match outcome {
            Outcome::Delivered => {
                stats.delivered.fetch_add(1, Ordering::Relaxed);
            }
            Outcome::Gone(status) => {
                stats.gone.fetch_add(1, Ordering::Relaxed);
                tracing::info!("push: a subscription of player {player} is gone ({status}); deleting it");
                if let Err(e) = shared.store.delete_push_subscription(&s.endpoint).await {
                    tracing::warn!("push: could not delete a gone subscription: {e}");
                }
                shared.invalidate(player);
            }
            Outcome::Rejected(status) => {
                stats.failed.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("push: a message to player {player} was refused ({status})");
            }
            Outcome::Failed(why) => {
                stats.failed.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("push: a message to player {player} failed: {why}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::AlertKind;
    use crate::webpush::Note;

    fn note(world: &str, kind: PushKind, text: &str) -> Note {
        let name = world.to_uppercase();
        Note { player: 1, world: world.into(), world_name: name.into(), kind, text: text.into(), organism: Some(7) }
    }

    #[test]
    fn payload_leads_with_the_most_important_notice() {
        let one = Batch { player: 1, notes: vec![note("main", PushKind::Mating, "B asks to mate.")], total: 1 };
        let p = payload(&one);
        assert_eq!(p.title, "Mating offer · MAIN");
        assert_eq!((p.body.as_str(), p.world.as_str(), p.tab.as_str()), ("B asks to mate.", "main", "matings"));
        assert_eq!((p.count, p.tag.as_str(), p.organism), (1, "evo-main", Some(7)));

        let notes = vec![
            note("main", PushKind::Death, "Founder #3 starved."),
            note("main", PushKind::Season, "Season 2 ended."),
        ];
        let p = payload(&Batch { player: 1, notes, total: 5 });
        assert_eq!(p.title, "5 events · MAIN");
        assert_eq!(p.body, "Founder #3 starved.\nSeason 2 ended.\n…and 3 more");
        assert_eq!((p.kind, p.tab.as_str()), (PushKind::Death, "genetics"));

        let notes = vec![note("isle", PushKind::Market, "Outbid."), note("main", PushKind::Catastrophe, "Flood.")];
        let p = payload(&Batch { player: 1, notes, total: 2 });
        assert_eq!(p.title, "2 events in 2 worlds");
        assert_eq!(p.body, "[ISLE] Outbid.\n[MAIN] Flood.");
        assert_eq!((p.world.as_str(), p.tab.as_str()), ("isle", "market"));

        let long = Batch { player: 1, notes: vec![note("main", PushKind::Market, &"x".repeat(5000))], total: 1 };
        assert_eq!(payload(&long).body.chars().count(), MAX_BODY_CHARS);
    }

    #[test]
    fn filters_follow_push_and_alert_preferences() {
        let f = Filter {
            push: PushPrefs { off: vec![PushKind::Season] },
            alerts: AlertPrefs { hidden: vec![AlertKind::Market, AlertKind::World], toast: vec![] },
            subscribed: true,
            loaded: Instant::now(),
        };
        assert!(f.allows(PushKind::Mating));
        assert!(f.allows(PushKind::Death));
        assert!(f.allows(PushKind::Species));
        // Switched off for push.
        assert!(!f.allows(PushKind::Season));
        // Hidden alert kinds are never pushed.
        assert!(!f.allows(PushKind::Market));
        assert!(!f.allows(PushKind::Catastrophe));
        // Nothing without a subscription.
        assert!(!Filter { subscribed: false, ..f }.allows(PushKind::Mating));
    }
}
