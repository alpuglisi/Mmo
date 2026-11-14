//! Accounts and login sessions.
//!
//! Passwords are hashed with argon2id (random salt, PHC string). A login
//! returns a random 256-bit bearer token. Sessions live in memory only, so a
//! restart logs everyone out. A session ends after [`SessionPolicy::idle`]
//! without use (any authenticated request, or an open WebSocket, which
//! sends commands) or [`SessionPolicy::max_age`] after login,
//! whichever comes first. Each account owns one [`PlayerId`], assigned in
//! registration order starting at 1 (0 is the wild ecosystem).
//!
//! Hashing or verifying a password costs 19 MiB and tens of milliseconds of
//! CPU, so both go through a [`HashLimiter`]: at most
//! [`HashPolicy::concurrency`] at once on the blocking pool, at most
//! [`HashPolicy::queue`] more waiting, and anything past that refused at
//! once with [`AuthError::Busy`] (HTTP 503 `busy` with `retry_after_ms`).
//! Without the bound a login burst ran hundreds of hashes at once on
//! Tokio's blocking pool (up to 512 threads) and starved the world threads.
//! Token checks (every authenticated request, the WebSocket) never hash.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use evo_sim::PlayerId;
use rand::RngCore;
use tokio::sync::Semaphore;

use crate::metrics::HashStats;
use crate::store::{AccountRecord, Store};

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    BadName,
    WeakPassword,
    NameTaken,
    BadCredentials,
    /// Too many password hashes running and queued: try again after about
    /// this long.
    Busy(Duration),
    Internal(String),
}

impl AuthError {
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::BadName => "bad_name",
            AuthError::WeakPassword => "weak_password",
            AuthError::NameTaken => "name_taken",
            AuthError::BadCredentials => "bad_credentials",
            AuthError::Busy(_) => "busy",
            AuthError::Internal(_) => "internal",
        }
    }

    pub fn message(&self) -> String {
        match self {
            AuthError::BadName => "names are 3 to 24 letters, digits, '_' or '-'".into(),
            AuthError::WeakPassword => {
                format!("passwords need at least {MIN_PASSWORD} characters and at most {MAX_PASSWORD_BYTES} bytes")
            }
            AuthError::NameTaken => "that name is taken".into(),
            AuthError::BadCredentials => "wrong name or password".into(),
            AuthError::Busy(_) => "the server is busy with other logins; try again shortly".into(),
            AuthError::Internal(e) => e.clone(),
        }
    }
}

pub const MIN_PASSWORD: usize = 8;
/// Longest password accepted, in bytes: argon2 hashes any length, so without
/// a bound one request could make the server hash megabytes.
pub const MAX_PASSWORD_BYTES: usize = 1024;

/// A hash to verify against when the name is unknown, so a login for a
/// missing account costs as much as one with a wrong password and the
/// response time does not tell which names exist.
fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default().hash_password(b"not a real password", &salt).map(|h| h.to_string()).unwrap_or_default()
    })
}

/// How many password hashes run at once and how many may wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashPolicy {
    /// Hashes (or verifications) running at once.
    pub concurrency: usize,
    /// Requests allowed to wait for a slot; more are refused as busy.
    pub queue: usize,
}

/// Requests allowed to wait per hashing slot by default.
pub const HASH_QUEUE_PER_SLOT: usize = 16;

impl HashPolicy {
    /// Half the cores (at least 1) at once.
    pub fn for_cores(cores: usize) -> Self {
        Self::with_concurrency(cores / 2)
    }

    /// `concurrency` at once (at least 1) and [`HASH_QUEUE_PER_SLOT`]
    /// waiting per slot.
    pub fn with_concurrency(concurrency: usize) -> Self {
        let concurrency = concurrency.max(1);
        Self { concurrency, queue: concurrency * HASH_QUEUE_PER_SLOT }
    }
}

impl Default for HashPolicy {
    fn default() -> Self {
        Self::for_cores(std::thread::available_parallelism().map_or(2, |n| n.get()))
    }
}

/// Bounds concurrent password hashing ([`HashPolicy`]) and counts it.
pub struct HashLimiter {
    policy: HashPolicy,
    shared: Arc<HashShared>,
}

struct HashShared {
    slots: Arc<Semaphore>,
    /// Running plus waiting.
    admitted: AtomicUsize,
    running: AtomicUsize,
    peak_running: AtomicUsize,
    done: AtomicU64,
    rejected: AtomicU64,
    /// Time spent hashing, over `done` hashes.
    hash_ns: AtomicU64,
}

/// An admitted request, released when dropped: when its hash finishes, or
/// when it is cancelled while waiting (the client went away).
struct Admission(Arc<HashShared>);

impl Drop for Admission {
    fn drop(&mut self) {
        self.0.admitted.fetch_sub(1, Ordering::AcqRel);
    }
}

impl HashLimiter {
    pub fn new(policy: HashPolicy) -> Self {
        let policy = HashPolicy { concurrency: policy.concurrency.max(1), queue: policy.queue };
        Self {
            policy,
            shared: Arc::new(HashShared {
                slots: Arc::new(Semaphore::new(policy.concurrency)),
                admitted: AtomicUsize::new(0),
                running: AtomicUsize::new(0),
                peak_running: AtomicUsize::new(0),
                done: AtomicU64::new(0),
                rejected: AtomicU64::new(0),
                hash_ns: AtomicU64::new(0),
            }),
        }
    }

    pub fn policy(&self) -> HashPolicy {
        self.policy
    }

    /// Run `f` (a hash or a verification) on the blocking pool once a slot
    /// is free, or refuse at once with [`AuthError::Busy`] when the queue is
    /// full. The blocking task itself holds the slot, so a request cancelled
    /// mid-hash never lets more than `concurrency` hashes run.
    pub async fn run<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Result<T, AuthError> {
        let sh = &self.shared;
        let prior = sh.admitted.fetch_add(1, Ordering::AcqRel);
        let admission = Admission(sh.clone());
        if prior >= self.policy.concurrency + self.policy.queue {
            drop(admission);
            sh.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(AuthError::Busy(self.retry_after()));
        }
        let permit = sh.slots.clone().acquire_owned().await.map_err(|e| AuthError::Internal(e.to_string()))?;
        let shared = sh.clone();
        tokio::task::spawn_blocking(move || {
            let _admission = admission;
            let _permit = permit;
            let running = shared.running.fetch_add(1, Ordering::AcqRel) + 1;
            shared.peak_running.fetch_max(running, Ordering::AcqRel);
            let t = Instant::now();
            let out = f();
            shared.hash_ns.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
            shared.done.fetch_add(1, Ordering::Relaxed);
            shared.running.fetch_sub(1, Ordering::AcqRel);
            out
        })
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))
    }

    /// Mean time of one hash so far (`None` before the first).
    fn mean_hash(&self) -> Option<Duration> {
        let done = self.shared.done.load(Ordering::Relaxed);
        (done > 0).then(|| Duration::from_nanos(self.shared.hash_ns.load(Ordering::Relaxed) / done))
    }

    /// About how long the queue takes to drain, within 0.25–5 s.
    fn retry_after(&self) -> Duration {
        let queued = self.shared.admitted.load(Ordering::Relaxed).min(u32::MAX as usize) as u32;
        let per_hash = self.mean_hash().unwrap_or(Duration::from_millis(50));
        (per_hash * queued / self.policy.concurrency as u32).clamp(Duration::from_millis(250), Duration::from_secs(5))
    }

    /// Counters for `GET /api/metrics`.
    pub fn stats(&self) -> HashStats {
        let sh = &self.shared;
        let admitted = sh.admitted.load(Ordering::Relaxed);
        let running = sh.running.load(Ordering::Relaxed);
        HashStats {
            concurrency: self.policy.concurrency,
            queue: self.policy.queue,
            in_flight: running,
            waiting: admitted.saturating_sub(running),
            peak_in_flight: sh.peak_running.load(Ordering::Relaxed),
            hashes: sh.done.load(Ordering::Relaxed),
            rejected: sh.rejected.load(Ordering::Relaxed),
            mean_ms: self.mean_hash().map_or(0.0, |d| d.as_secs_f32() * 1e3),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub player: PlayerId,
    pub name: String,
}

/// How long a login lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionPolicy {
    /// A session unused for this long ends.
    pub idle: Duration,
    /// A session ends this long after login, used or not.
    pub max_age: Duration,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self { idle: Duration::from_secs(60 * 60), max_age: Duration::from_secs(24 * 60 * 60) }
    }
}

struct SessionEntry {
    session: Session,
    created: Instant,
    last_seen: Instant,
}

impl SessionEntry {
    fn expired(&self, policy: SessionPolicy, now: Instant) -> bool {
        now.saturating_duration_since(self.last_seen) >= policy.idle
            || now.saturating_duration_since(self.created) >= policy.max_age
    }
}

pub struct Accounts {
    store: Arc<dyn Store>,
    policy: SessionPolicy,
    hashing: HashLimiter,
    inner: Mutex<Inner>,
}

struct Inner {
    by_name: HashMap<String, AccountRecord>,
    /// Names reserved by registrations still hashing or persisting.
    pending: Vec<String>,
    sessions: HashMap<String, SessionEntry>,
    next_player: PlayerId,
}

impl Inner {
    /// Start a session, dropping expired ones.
    fn open_session(&mut self, session: Session, policy: SessionPolicy) -> String {
        let now = Instant::now();
        self.sessions.retain(|_, e| !e.expired(policy, now));
        let token = new_token();
        self.sessions.insert(token.clone(), SessionEntry { session, created: now, last_seen: now });
        token
    }
}

fn valid_name(name: &str) -> bool {
    (3..=24).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Accounts {
    pub fn new(store: Arc<dyn Store>, existing: Vec<AccountRecord>) -> Self {
        Self::with_policy(store, existing, SessionPolicy::default())
    }

    pub fn with_policy(store: Arc<dyn Store>, existing: Vec<AccountRecord>, policy: SessionPolicy) -> Self {
        Self::with_policies(store, existing, policy, HashPolicy::default())
    }

    pub fn with_policies(
        store: Arc<dyn Store>,
        existing: Vec<AccountRecord>,
        policy: SessionPolicy,
        hashing: HashPolicy,
    ) -> Self {
        let next_player = existing.iter().map(|a| a.player).max().unwrap_or(0) + 1;
        let by_name = existing.into_iter().map(|a| (a.name.clone(), a)).collect();
        Self {
            store,
            policy,
            hashing: HashLimiter::new(hashing),
            inner: Mutex::new(Inner { by_name, pending: Vec::new(), sessions: HashMap::new(), next_player }),
        }
    }

    pub fn policy(&self) -> SessionPolicy {
        self.policy
    }

    /// Password hashing counters (`GET /api/metrics`).
    pub fn hash_stats(&self) -> HashStats {
        self.hashing.stats()
    }

    pub fn name_of(&self, player: PlayerId) -> Option<String> {
        let g = self.inner.lock().unwrap();
        g.by_name.values().find(|a| a.player == player).map(|a| a.name.clone())
    }

    /// Create an account and log it in.
    pub async fn register(&self, name: &str, password: &str) -> Result<(String, Session), AuthError> {
        if !valid_name(name) {
            return Err(AuthError::BadName);
        }
        if password.chars().count() < MIN_PASSWORD || password.len() > MAX_PASSWORD_BYTES {
            return Err(AuthError::WeakPassword);
        }
        if self.name_in_use(name) {
            return Err(AuthError::NameTaken);
        }
        // Hash before reserving the name and a player id, so a request
        // refused as busy (or cancelled while it waits) costs neither.
        let password = password.to_string();
        let password_hash = self
            .hashing
            .run(move || {
                let salt = SaltString::generate(&mut OsRng);
                Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string())
            })
            .await?
            .map_err(|e| AuthError::Internal(e.to_string()))?;
        let player = {
            let mut g = self.inner.lock().unwrap();
            if g.by_name.contains_key(name) || g.pending.iter().any(|n| n == name) {
                return Err(AuthError::NameTaken);
            }
            g.pending.push(name.to_string());
            let p = g.next_player;
            g.next_player += 1;
            p
        };
        let result = self.persist(name, password_hash, player).await;
        let mut g = self.inner.lock().unwrap();
        g.pending.retain(|n| n != name);
        let record = result?;
        g.by_name.insert(name.to_string(), record);
        let session = Session { player, name: name.to_string() };
        let token = g.open_session(session.clone(), self.policy);
        Ok((token, session))
    }

    /// Whether the name has an account or a registration persisting.
    fn name_in_use(&self, name: &str) -> bool {
        let g = self.inner.lock().unwrap();
        g.by_name.contains_key(name) || g.pending.iter().any(|n| n == name)
    }

    async fn persist(&self, name: &str, password_hash: String, player: PlayerId) -> Result<AccountRecord, AuthError> {
        let created_unix = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let record = AccountRecord { name: name.to_string(), player, password_hash, created_unix };
        self.store.insert_account(&record).await.map_err(|e| AuthError::Internal(e.to_string()))?;
        Ok(record)
    }

    pub async fn login(&self, name: &str, password: &str) -> Result<(String, Session), AuthError> {
        if password.len() > MAX_PASSWORD_BYTES {
            // No account has one this long; don't hash it.
            return Err(AuthError::BadCredentials);
        }
        let record = self.inner.lock().unwrap().by_name.get(name).cloned();
        let password = password.to_string();
        let hash = record.as_ref().map(|r| r.password_hash.clone());
        let ok = self
            .hashing
            .run(move || {
                // The dummy hash is made on first use: here, on the blocking
                // pool, not on the async runtime.
                let hash = hash.as_deref().unwrap_or_else(|| dummy_hash());
                PasswordHash::new(hash)
                    .is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
            })
            .await?;
        let Some(record) = record.filter(|_| ok) else {
            return Err(AuthError::BadCredentials);
        };
        let session = Session { player: record.player, name: record.name };
        let token = self.inner.lock().unwrap().open_session(session.clone(), self.policy);
        Ok((token, session))
    }

    /// The live session of a token, marking it used.
    pub fn session(&self, token: &str) -> Option<Session> {
        self.session_at(token, Instant::now())
    }

    /// As [`Accounts::session`] at a given time (tests).
    pub fn session_at(&self, token: &str, now: Instant) -> Option<Session> {
        self.lookup(token, now, true)
    }

    /// Whether the token's session is still live, without marking it used
    /// (an open WebSocket checks this periodically).
    pub fn is_live(&self, token: &str) -> bool {
        self.lookup(token, Instant::now(), false).is_some()
    }

    fn lookup(&self, token: &str, now: Instant, touch: bool) -> Option<Session> {
        let mut g = self.inner.lock().unwrap();
        let e = g.sessions.get_mut(token)?;
        if e.expired(self.policy, now) {
            g.sessions.remove(token);
            return None;
        }
        if touch {
            e.last_seen = e.last_seen.max(now);
        }
        Some(e.session.clone())
    }

    /// Living sessions (expired ones not yet dropped count too).
    pub fn session_count(&self) -> usize {
        self.inner.lock().unwrap().sessions.len()
    }

    pub fn logout(&self, token: &str) {
        self.inner.lock().unwrap().sessions.remove(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;

    #[tokio::test]
    async fn register_login_and_sessions() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
        let acc = Accounts::new(store.clone(), vec![]);
        let (t1, s1) = acc.register("alice", "correct horse").await.unwrap();
        assert_eq!(s1.player, 1);
        assert_eq!(acc.register("alice", "another one").await.unwrap_err(), AuthError::NameTaken);
        assert_eq!(acc.register("a!", "whatever123").await.unwrap_err(), AuthError::BadName);
        assert_eq!(acc.register("bob", "short").await.unwrap_err(), AuthError::WeakPassword);
        let (_, s2) = acc.register("bob", "bob's password").await.unwrap();
        assert_eq!(s2.player, 2);
        assert_eq!(acc.login("alice", "wrong password").await.unwrap_err(), AuthError::BadCredentials);
        let (t3, s3) = acc.login("alice", "correct horse").await.unwrap();
        assert_eq!(s3.player, 1);
        assert_ne!(t1, t3);
        assert_eq!(acc.session(&t3).unwrap().name, "alice");
        // Persisted hashes survive a restart.
        let stored = store.load_accounts().await.unwrap();
        assert!(stored[0].password_hash.starts_with("$argon2"));
        let acc2 = Accounts::new(store, stored);
        assert!(acc2.session(&t3).is_none());
        assert_eq!(acc2.login("bob", "bob's password").await.unwrap().1.player, 2);
        assert_eq!(acc2.register("carol", "carol's password").await.unwrap().1.player, 3);
    }

    #[tokio::test]
    async fn passwords_are_bounded_and_hashed_with_argon2id() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
        let acc = Accounts::new(store.clone(), vec![]);
        let long = "x".repeat(MAX_PASSWORD_BYTES + 1);
        assert_eq!(acc.register("erin", &long).await.unwrap_err(), AuthError::WeakPassword);
        let longest = "y".repeat(MAX_PASSWORD_BYTES);
        acc.register("erin", &longest).await.unwrap();
        assert!(acc.login("erin", &longest).await.is_ok());
        assert_eq!(acc.login("erin", &long).await.unwrap_err(), AuthError::BadCredentials);
        // Unknown names fail like wrong passwords (after a dummy verify).
        assert_eq!(acc.login("nobody", "whatever123").await.unwrap_err(), AuthError::BadCredentials);
        assert!(dummy_hash().starts_with("$argon2id$"));
        // The stored hash is argon2id with at least OWASP's minimum cost.
        let stored = store.load_accounts().await.unwrap();
        let h = PasswordHash::new(&stored[0].password_hash).unwrap();
        assert_eq!(h.algorithm.as_str(), "argon2id");
        let params = argon2::Params::try_from(&h).unwrap();
        assert!(params.m_cost() >= 19 * 1024 && params.t_cost() >= 2, "{params:?}");
        assert!(h.salt.is_some_and(|s| s.len() >= 16));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_hash_limiter_bounds_concurrency_and_refuses_past_its_queue() {
        let limiter = HashLimiter::new(HashPolicy { concurrency: 3, queue: 5 });
        // Concurrency seen by the jobs themselves, independent of the limiter.
        let now = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        // `join_all` polls every request once before any job finishes, so
        // admission is deterministic: 3 run, 5 wait, the rest are refused.
        let results = futures_util::future::join_all((0..40).map(|_| {
            let (now, peak) = (now.clone(), peak.clone());
            limiter.run(move || {
                let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(n, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(30));
                now.fetch_sub(1, Ordering::SeqCst);
            })
        }))
        .await;
        let ok = results.iter().filter(|r| r.is_ok()).count();
        let busy: Vec<Duration> =
            results.iter().filter_map(|r| if let Err(AuthError::Busy(d)) = r { Some(*d) } else { None }).collect();
        assert_eq!(ok, 8);
        assert_eq!(busy.len(), 32);
        assert!(busy.iter().all(|d| (Duration::from_millis(250)..=Duration::from_secs(5)).contains(d)), "{busy:?}");
        assert_eq!(peak.load(Ordering::SeqCst), 3, "never more than 3 at once, and the slots were used");
        let s = limiter.stats();
        assert_eq!((s.concurrency, s.queue, s.in_flight, s.waiting), (3, 5, 0, 0));
        assert_eq!((s.peak_in_flight, s.hashes, s.rejected), (3, 8, 32));
        assert!(s.mean_ms >= 30.0, "{s:?}");
        // Everything was released: a new request runs.
        assert!(limiter.run(|| ()).await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancelled_waiter_frees_its_place_in_the_queue() {
        let limiter = Arc::new(HashLimiter::new(HashPolicy { concurrency: 1, queue: 1 }));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let running = {
            let l = limiter.clone();
            tokio::spawn(async move { l.run(move || rx.recv().unwrap()).await })
        };
        while limiter.stats().in_flight == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        // A waiter that gives up (a client that disconnected).
        assert!(tokio::time::timeout(Duration::from_millis(20), limiter.run(|| ())).await.is_err());
        assert_eq!(limiter.stats().waiting, 0);
        // Its place is free again; a second waiter beyond it is refused.
        let waiter = {
            let l = limiter.clone();
            tokio::spawn(async move { l.run(|| ()).await })
        };
        while limiter.stats().waiting == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(matches!(limiter.run(|| ()).await, Err(AuthError::Busy(_))));
        tx.send(()).unwrap();
        running.await.unwrap().unwrap();
        waiter.await.unwrap().unwrap();
        assert_eq!(limiter.stats().rejected, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_registrations_never_exceed_the_hash_bound() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
        let policy = HashPolicy { concurrency: 2, queue: 4 };
        let acc = Accounts::with_policies(store.clone(), vec![], SessionPolicy::default(), policy);
        let names: Vec<String> = (0..30).map(|i| format!("user{i}")).collect();
        let results = futures_util::future::join_all(names.iter().map(|n| acc.register(n, "a long password"))).await;
        let ok: Vec<PlayerId> = results.iter().filter_map(|r| r.as_ref().ok().map(|(_, s)| s.player)).collect();
        let busy = results.iter().filter(|r| matches!(r, Err(AuthError::Busy(_)))).count();
        assert_eq!((ok.len(), busy), (6, 24));
        let s = acc.hash_stats();
        assert_eq!((s.peak_in_flight, s.hashes, s.rejected), (2, 6, 24));
        // Refused registrations reserved neither a name nor a player id.
        let mut ids = ok.clone();
        ids.sort_unstable();
        assert_eq!(ids, (1..=6).collect::<Vec<_>>());
        assert_eq!(store.load_accounts().await.unwrap().len(), 6);
        let (_, s) = acc.register(&names[29], "a long password").await.unwrap();
        assert_eq!(s.player, 7);
        // Logins go through the same bound.
        let logins = futures_util::future::join_all((0..20).map(|_| acc.login("user29", "a long password"))).await;
        assert_eq!(logins.iter().filter(|r| r.is_ok()).count(), 6);
        assert_eq!(logins.iter().filter(|r| matches!(r, Err(AuthError::Busy(_)))).count(), 14);
        assert_eq!(acc.hash_stats().peak_in_flight, 2);
        // Token checks don't hash.
        let (token, _) = acc.login("user29", "a long password").await.unwrap();
        let hashes = acc.hash_stats().hashes;
        for _ in 0..100 {
            assert!(acc.session(&token).is_some() && acc.is_live(&token));
        }
        assert_eq!(acc.hash_stats().hashes, hashes);
    }

    #[test]
    fn hash_policy_defaults_to_half_the_cores() {
        assert_eq!(HashPolicy::for_cores(1), HashPolicy { concurrency: 1, queue: 16 });
        assert_eq!(HashPolicy::for_cores(4), HashPolicy { concurrency: 2, queue: 32 });
        assert_eq!(HashPolicy::for_cores(9).concurrency, 4);
        assert_eq!(HashPolicy::with_concurrency(0).concurrency, 1);
    }

    #[test]
    fn tokens_are_256_random_bits() {
        let a = new_token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, new_token());
    }

    #[tokio::test]
    async fn sessions_expire_when_idle_or_too_old() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
        let policy = SessionPolicy { idle: Duration::from_secs(60), max_age: Duration::from_secs(300) };
        let acc = Accounts::with_policy(store, vec![], policy);
        let (t, _) = acc.register("dave", "dave's password").await.unwrap();
        let t0 = Instant::now();
        let s = |dt: u64| acc.session_at(&t, t0 + Duration::from_secs(dt)).is_some();
        // Used every 50 s: stays alive past the idle timeout...
        for k in 1..=5 {
            assert!(s(50 * k), "at {}s", 50 * k);
        }
        // ...until the maximum age.
        assert!(!s(301));
        assert!(acc.session(&t).is_none(), "expired sessions are dropped");
        // Idle for longer than the timeout.
        let (t2, _) = acc.login("dave", "dave's password").await.unwrap();
        let t1 = Instant::now();
        assert!(acc.session_at(&t2, t1 + Duration::from_secs(30)).is_some());
        assert!(acc.session_at(&t2, t1 + Duration::from_secs(91)).is_none());
        // Logins drop expired sessions.
        let (_, _) = acc.login("dave", "dave's password").await.unwrap();
        assert_eq!(acc.session_count(), 1);
    }
}
