//! Rate limits: token buckets per key (a client IP for logins and commands,
//! a connection id for commands).
//!
//! A bucket holds up to `burst` tokens and refills at `per_sec`; each
//! request takes one. An empty bucket refuses with the time until the next
//! token. Time is passed in, so the limiter is tested without sleeping.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rate {
    /// Tokens added per second.
    pub per_sec: f64,
    /// Bucket size: requests allowed at once after a quiet spell.
    pub burst: f64,
}

impl Rate {
    pub const fn new(per_sec: f64, burst: f64) -> Self {
        Self { per_sec, burst }
    }

    /// No limit.
    pub const UNLIMITED: Rate = Rate { per_sec: f64::INFINITY, burst: f64::INFINITY };
}

#[derive(Clone, Copy, Debug)]
pub struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    pub fn new(rate: Rate, now: Instant) -> Self {
        Self { tokens: rate.burst, last: now }
    }

    /// Take a token, or say how long until one is available.
    pub fn take(&mut self, rate: Rate, now: Instant) -> Result<(), Duration> {
        if rate.per_sec.is_infinite() {
            return Ok(());
        }
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + dt * rate.per_sec).min(rate.burst);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else if rate.per_sec > 0.0 {
            Err(Duration::from_secs_f64((1.0 - self.tokens) / rate.per_sec))
        } else {
            Err(Duration::from_secs(3600))
        }
    }

    fn is_full(&self, rate: Rate, now: Instant) -> bool {
        self.tokens + now.saturating_duration_since(self.last).as_secs_f64() * rate.per_sec >= rate.burst
    }
}

/// Buckets by key behind a mutex.
pub struct KeyedLimiter<K> {
    rate: Rate,
    buckets: Mutex<HashMap<K, Bucket>>,
}

/// Keys kept before full (idle) buckets are dropped.
const PRUNE_AT: usize = 4096;

impl<K: Hash + Eq + Clone> KeyedLimiter<K> {
    pub fn new(rate: Rate) -> Self {
        Self { rate, buckets: Mutex::new(HashMap::new()) }
    }

    pub fn check_at(&self, key: &K, now: Instant) -> Result<(), Duration> {
        let mut map = self.buckets.lock().unwrap();
        if map.len() >= PRUNE_AT {
            let rate = self.rate;
            map.retain(|_, b| !b.is_full(rate, now));
        }
        map.entry(key.clone()).or_insert_with(|| Bucket::new(self.rate, now)).take(self.rate, now)
    }

    pub fn check(&self, key: &K) -> Result<(), Duration> {
        self.check_at(key, Instant::now())
    }
}

/// The server's limits (`ServerConfig::limits`).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Logins and registrations per client IP.
    pub logins: Rate,
    /// WebSocket commands per connection.
    pub commands: Rate,
    /// WebSocket commands per client IP over all its connections.
    pub ip_commands: Rate,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // 10 attempts at once, then one every 6 s.
            logins: Rate::new(10.0 / 60.0, 10.0),
            // A client clicking around sends a few a second.
            commands: Rate::new(20.0, 40.0),
            ip_commands: Rate::new(60.0, 120.0),
        }
    }
}

impl Limits {
    pub fn unlimited() -> Self {
        Self { logins: Rate::UNLIMITED, commands: Rate::UNLIMITED, ip_commands: Rate::UNLIMITED }
    }
}

/// The shared limiters of a running server.
pub struct Limiters {
    pub limits: Limits,
    pub logins: KeyedLimiter<IpAddr>,
    pub ip_commands: KeyedLimiter<IpAddr>,
}

impl Limiters {
    pub fn new(limits: Limits) -> Self {
        Self { limits, logins: KeyedLimiter::new(limits.logins), ip_commands: KeyedLimiter::new(limits.ip_commands) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_allows_a_burst_then_refills() {
        let rate = Rate::new(2.0, 3.0);
        let t0 = Instant::now();
        let mut b = Bucket::new(rate, t0);
        for _ in 0..3 {
            assert!(b.take(rate, t0).is_ok());
        }
        let wait = b.take(rate, t0).unwrap_err();
        assert!((wait.as_secs_f64() - 0.5).abs() < 1e-6, "{wait:?}");
        // Half a second later one token is back, not two.
        let t1 = t0 + Duration::from_millis(500);
        assert!(b.take(rate, t1).is_ok());
        assert!(b.take(rate, t1).is_err());
        // A long pause refills only up to the burst.
        let t2 = t1 + Duration::from_secs(60);
        for _ in 0..3 {
            assert!(b.take(rate, t2).is_ok());
        }
        assert!(b.take(rate, t2).is_err());
        let mut free = Bucket::new(Rate::UNLIMITED, t0);
        for _ in 0..1000 {
            assert!(free.take(Rate::UNLIMITED, t0).is_ok());
        }
    }

    #[test]
    fn keys_are_limited_separately_and_pruned() {
        let lim = KeyedLimiter::new(Rate::new(1.0, 1.0));
        let t = Instant::now();
        assert!(lim.check_at(&1u32, t).is_ok());
        assert!(lim.check_at(&1u32, t).is_err());
        assert!(lim.check_at(&2u32, t).is_ok());
        for k in 0..PRUNE_AT as u32 + 10 {
            let _ = lim.check_at(&(k + 10), t);
        }
        // Much later every bucket is full again and the map was pruned.
        let later = t + Duration::from_secs(10);
        assert!(lim.check_at(&1u32, later).is_ok());
        assert!(lim.buckets.lock().unwrap().len() <= 2);
    }
}
