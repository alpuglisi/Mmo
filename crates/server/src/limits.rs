//! Rate limits: token buckets per key (a client address for logins, reads
//! and commands, a connection for commands).
//!
//! Client addresses are keyed by [`client_key`]: an IPv4 address, or the /64
//! network of an IPv6 one (one subscriber usually holds a whole /64, so
//! keying single IPv6 addresses would give each client 2^64 buckets).
//!
//! A bucket holds up to `burst` tokens and refills at `per_sec`; each
//! request takes one. An empty bucket refuses with the time until the next
//! token. Time is passed in, so the limiter is tested without sleeping.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, Ipv6Addr};
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
    buckets: Mutex<Buckets<K>>,
}

struct Buckets<K> {
    map: HashMap<K, Bucket>,
    /// Size at which the next new key prunes: twice what the last pruning
    /// left (at least [`PRUNE_AT`]), so pruning costs O(1) per new key
    /// however many keys are busy.
    prune_at: usize,
}

/// Keys kept before full (idle) buckets are dropped.
const PRUNE_AT: usize = 4096;
/// Most keys kept at once. Past it (many clients busy at once, or one
/// spraying addresses), buckets that still have tokens are dropped too, and
/// as a last resort all of them: memory stays bounded, at the price of
/// briefly forgetting some limits.
const MAX_KEYS: usize = 65_536;

/// The rate-limit key of a client address: IPv4 addresses (also when
/// written IPv4-mapped in IPv6) as they are, IPv6 addresses by their /64.
pub fn client_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
    }
}

impl<K: Hash + Eq + Clone> KeyedLimiter<K> {
    pub fn new(rate: Rate) -> Self {
        Self { rate, buckets: Mutex::new(Buckets { map: HashMap::new(), prune_at: PRUNE_AT }) }
    }

    pub fn check_at(&self, key: &K, now: Instant) -> Result<(), Duration> {
        let mut g = self.buckets.lock().unwrap();
        let b = &mut *g;
        if b.map.len() >= b.prune_at.min(MAX_KEYS) && !b.map.contains_key(key) {
            let rate = self.rate;
            b.map.retain(|_, x| !x.is_full(rate, now));
            if b.map.len() >= MAX_KEYS / 2 {
                b.map.retain(|_, x| x.tokens < 1.0);
            }
            if b.map.len() >= MAX_KEYS / 2 {
                b.map.clear();
            }
            b.prune_at = (2 * b.map.len()).max(PRUNE_AT);
        }
        b.map.entry(key.clone()).or_insert_with(|| Bucket::new(self.rate, now)).take(self.rate, now)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.buckets.lock().unwrap().map.len()
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
    /// Group changes (claim or abandon a territory, split or merge groups)
    /// per connection, on top of `commands`: each rewrites standing orders
    /// for a whole herd, so a client has no reason to send many.
    pub group_commands: Rate,
    /// Public reads that cost the world thread or the store (terrain, map,
    /// chronicle, history, lineage stats) per client IP.
    pub reads: Rate,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // 10 attempts at once, then one every 6 s.
            logins: Rate::new(10.0 / 60.0, 10.0),
            // A client clicking around sends a few a second.
            commands: Rate::new(20.0, 40.0),
            ip_commands: Rate::new(60.0, 120.0),
            // A player splitting a herd and claiming ground for each half.
            group_commands: Rate::new(1.0, 6.0),
            // The client polls a few of these every few seconds; a dashboard
            // opens with a handful at once.
            reads: Rate::new(10.0, 60.0),
        }
    }
}

impl Limits {
    pub fn unlimited() -> Self {
        Self {
            logins: Rate::UNLIMITED,
            commands: Rate::UNLIMITED,
            ip_commands: Rate::UNLIMITED,
            group_commands: Rate::UNLIMITED,
            reads: Rate::UNLIMITED,
        }
    }
}

/// Reverse proxies whose `X-Forwarded-For` header is believed
/// (`--trusted-proxy`): addresses or CIDR networks. Behind a proxy every
/// client connects from the proxy's address, so without this the per-client
/// limits would be one limit for everybody.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrustedProxies(Vec<(IpAddr, u8)>);

impl TrustedProxies {
    /// A comma-separated list of addresses (`10.0.0.2`, `::1`) and networks
    /// (`172.30.89.0/24`, `fd00::/8`); blanks are skipped.
    pub fn parse(list: &str) -> Result<Self, String> {
        let mut nets = Vec::new();
        for item in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (addr, bits) = match item.split_once('/') {
                Some((a, b)) => (a, Some(b)),
                None => (item, None),
            };
            let ip: IpAddr = addr.parse().map_err(|_| format!("{item:?} is not an IP address or network"))?;
            let max = if ip.is_ipv4() { 32 } else { 128 };
            let bits = match bits {
                None => max,
                Some(b) => match b.parse::<u8>() {
                    Ok(n) if n <= max => n,
                    _ => return Err(format!("{item:?}: the prefix length must be in 0..={max}")),
                },
            };
            nets.push((ip, bits));
        }
        Ok(Self(nets))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `ip` is one of the proxies (IPv4-mapped IPv6 counts as IPv4).
    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = canonical(ip);
        self.0.iter().any(|&(net, bits)| match (canonical(net), ip) {
            (IpAddr::V4(n), IpAddr::V4(a)) => prefix_eq(&n.octets(), &a.octets(), bits),
            (IpAddr::V6(n), IpAddr::V6(a)) => prefix_eq(&n.octets(), &a.octets(), bits),
            _ => false,
        })
    }

    /// The client's address: `peer`, or when `peer` is a trusted proxy, the
    /// last address in `forwarded_for` (`X-Forwarded-For`, one or more
    /// headers in order) that is not itself a trusted proxy. Entries left of
    /// that one were written by the client and are ignored. A malformed
    /// entry ends the walk at the last trusted hop.
    pub fn client_addr<'a>(&self, peer: IpAddr, forwarded_for: impl DoubleEndedIterator<Item = &'a str>) -> IpAddr {
        if !self.contains(peer) {
            return peer;
        }
        let mut addr = peer;
        for entry in forwarded_for.rev().flat_map(|h| h.rsplit(',')) {
            match entry.trim().parse::<IpAddr>() {
                Ok(ip) => {
                    addr = ip;
                    if !self.contains(ip) {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        addr
    }
}

fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

fn prefix_eq(a: &[u8], b: &[u8], bits: u8) -> bool {
    let (whole, rest) = (usize::from(bits / 8), bits % 8);
    if a[..whole] != b[..whole] {
        return false;
    }
    rest == 0 || (a[whole] ^ b[whole]) >> (8 - rest) == 0
}

/// The shared limiters of a running server.
pub struct Limiters {
    pub limits: Limits,
    pub logins: KeyedLimiter<IpAddr>,
    pub ip_commands: KeyedLimiter<IpAddr>,
    pub reads: KeyedLimiter<IpAddr>,
    /// Proxies whose `X-Forwarded-For` names the client.
    pub proxies: TrustedProxies,
}

impl Limiters {
    pub fn new(limits: Limits, proxies: TrustedProxies) -> Self {
        Self {
            limits,
            logins: KeyedLimiter::new(limits.logins),
            ip_commands: KeyedLimiter::new(limits.ip_commands),
            reads: KeyedLimiter::new(limits.reads),
            proxies,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_proxies_parse_addresses_and_networks() {
        let p = TrustedProxies::parse(" 172.30.89.0/24, 10.0.0.2 ,fd00::/8,").unwrap();
        assert!(p.contains("172.30.89.7".parse().unwrap()));
        assert!(!p.contains("172.30.90.7".parse().unwrap()));
        assert!(p.contains("10.0.0.2".parse().unwrap()));
        assert!(p.contains("::ffff:10.0.0.2".parse().unwrap()));
        assert!(!p.contains("10.0.0.3".parse().unwrap()));
        assert!(p.contains("fd12::1".parse().unwrap()));
        assert!(!p.contains("fe80::1".parse().unwrap()));
        assert!(TrustedProxies::parse("").unwrap().is_empty());
        assert!(TrustedProxies::parse("0.0.0.0/0").unwrap().contains("8.8.8.8".parse().unwrap()));
        assert!(TrustedProxies::parse("10.0.0.0/33").is_err());
        assert!(TrustedProxies::parse("example.org").is_err());
        let odd = TrustedProxies::parse("10.0.0.0/7").unwrap();
        assert!(odd.contains("11.255.0.1".parse().unwrap()));
        assert!(!odd.contains("12.0.0.1".parse().unwrap()));
    }

    #[test]
    fn forwarded_for_is_believed_only_from_a_trusted_proxy() {
        let p = TrustedProxies::parse("172.30.89.0/24").unwrap();
        let proxy: IpAddr = "172.30.89.2".parse().unwrap();
        let outsider: IpAddr = "198.51.100.9".parse().unwrap();
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        // Not from the proxy: the header is the client's own claim.
        assert_eq!(p.client_addr(outsider, ["203.0.113.5"].into_iter()), outsider);
        // From the proxy: the last entry not a proxy itself.
        assert_eq!(p.client_addr(proxy, ["203.0.113.5"].into_iter()), ip("203.0.113.5"));
        assert_eq!(p.client_addr(proxy, ["1.2.3.4, 203.0.113.5"].into_iter()), ip("203.0.113.5"));
        assert_eq!(p.client_addr(proxy, ["1.2.3.4", "203.0.113.5, 172.30.89.3"].into_iter()), ip("203.0.113.5"));
        assert_eq!(p.client_addr(proxy, ["2001:db8::1"].into_iter()), ip("2001:db8::1"));
        // No header, or garbage: the proxy's own address.
        assert_eq!(p.client_addr(proxy, std::iter::empty()), proxy);
        assert_eq!(p.client_addr(proxy, ["unknown"].into_iter()), proxy);
        // Nothing trusted: always the peer.
        assert_eq!(TrustedProxies::default().client_addr(proxy, ["203.0.113.5"].into_iter()), proxy);
    }

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
        let old = lim.len();
        assert_eq!(old, PRUNE_AT + 12, "all still busy: nothing pruned");
        // Much later every old bucket is full again: the next pruning (when
        // new keys double the map) drops them.
        let later = t + Duration::from_secs(10);
        assert!(lim.check_at(&1u32, later).is_ok());
        let fresh = PRUNE_AT as u32 + 100;
        for k in 0..fresh {
            let _ = lim.check_at(&(k + 1_000_000), later);
        }
        assert!(lim.len() <= fresh as usize + 1, "{} kept", lim.len());
    }

    #[test]
    fn ipv6_clients_are_keyed_by_their_slash_64() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff:1:2:3".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(client_key(a), client_key(b));
        assert_ne!(client_key(a), client_key(other));
        let v4: IpAddr = "192.0.2.7".parse().unwrap();
        assert_eq!(client_key(v4), v4);
        assert_eq!(client_key("::ffff:192.0.2.7".parse().unwrap()), v4);
        // Rotating addresses inside one /64 doesn't refill the bucket.
        let lim = KeyedLimiter::new(Rate::new(1.0, 2.0));
        let t = Instant::now();
        let mut ok = 0;
        for k in 0..10u16 {
            let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 2, k, k, k, k));
            ok += lim.check_at(&client_key(ip), t).is_ok() as u32;
        }
        assert_eq!(ok, 2);
    }

    #[test]
    fn many_busy_keys_stay_bounded() {
        let lim = KeyedLimiter::new(Rate::new(0.001, 1.0));
        let t = Instant::now();
        // Every key spends its token, so none is full and pruning alone
        // would keep them all.
        for k in 0..(MAX_KEYS as u32 + 100) {
            let _ = lim.check_at(&k, t);
        }
        assert!(lim.len() <= MAX_KEYS);
    }
}
