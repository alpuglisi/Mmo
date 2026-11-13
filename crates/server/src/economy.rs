//! The in-game economy behind mating prices: **renown**, a resource earned
//! from play (never bought with real money) and spent on stud fees and
//! directed-mating prices. Mating slots per season stay the scarce resource;
//! renown only decides who gets to use someone else's genetics.
//!
//! Pure bookkeeping, no world access: [`crate::game::Game`] calls it and
//! turns its ledger entries into persistence events and alerts.
//!
//! **Earning** (see [`daily_income`] and [`achievement_bonus`]):
//!
//! - a one-time founding grant of [`FOUNDING_GRANT`] when a player first
//!   founds a lineage;
//! - every simulated day, at the legacy snapshot: [`DAILY_STIPEND`] while the
//!   lineage has living members, plus one renown per percentage point of the
//!   mean of the player's raw and weighted legacy share, rounded:
//!   `income = 2 + round(100 · (share + weighted_share) / 2)` (a lineage with
//!   10% raw and 6% weighted share earns 10 a day; an extinct one nothing);
//! - achievements: species founded 100, niche dominance 40, survived a
//!   catastrophe 25, each year of lineage longevity 20.
//!
//! **Spending**: a directed request with a price, or buying a stud
//! marketplace listing, moves the price from the payer's balance into
//! escrow on the agreement. When the mating runs (`completed`, conceived or
//! not) the escrow goes to the partner's owner; any other ending (declined,
//! cancelled, failed, expired) refunds it.
//!
//! Every change is a [`LedgerEntry`] with a sequence number that is unique
//! per world; the store keeps the entries and each player's balance.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use evo_sim::PlayerId;
use serde::{Deserialize, Serialize};

use crate::protocol::{LedgerEntry, LedgerKind, OrgId};

/// Renown given once, the first time a player founds a lineage.
pub const FOUNDING_GRANT: u64 = 50;
/// Daily renown for a lineage with living members.
pub const DAILY_STIPEND: u64 = 2;
/// Highest price or fee accepted.
pub const MAX_PRICE: u64 = 1_000_000;
/// Ledger entries kept in memory (and in saved games) per player.
pub const LEDGER_KEPT: usize = 100;
/// Open marketplace listings one player may have.
pub const MAX_LISTINGS: usize = 8;
/// Longest listing expiry, in simulated days.
pub const MAX_LISTING_DAYS: u32 = 365;

/// Renown for one simulated day: the stipend while the lineage lives, plus
/// one per percentage point of the mean of raw and weighted legacy share.
pub fn daily_income(share: f64, weighted_share: f64, alive: bool) -> u64 {
    if !alive {
        return 0;
    }
    let points = (100.0 * (share.clamp(0.0, 1.0) + weighted_share.clamp(0.0, 1.0)) / 2.0).round();
    DAILY_STIPEND + points as u64
}

/// Renown for an achievement (`AchievementKind::label`).
pub fn achievement_bonus(kind: &str) -> u64 {
    match kind {
        "species_founded" => 100,
        "niche_dominance" => 40,
        "survived_catastrophe" => 25,
        "lineage_longevity" => 20,
        _ => 0,
    }
}

/// A stud (or dam) on the marketplace: anyone may buy one mating with it for
/// `fee`, which is the owner's consent given in advance.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Listing {
    pub id: u64,
    pub owner: PlayerId,
    pub organism: OrgId,
    pub fee: u64,
    pub created_tick: u64,
    /// The listing closes at this tick (none: until withdrawn or the
    /// organism dies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_tick: Option<u64>,
    /// Matings bought from it so far.
    #[serde(default)]
    pub sales: u32,
}

/// Balances, the recent ledger and the marketplace listings. Saved with the
/// game; balances and ledger entries also go to the store.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Economy {
    pub balances: BTreeMap<PlayerId, u64>,
    /// Newest last, at most [`LEDGER_KEPT`] per player.
    pub ledger: BTreeMap<PlayerId, VecDeque<LedgerEntry>>,
    /// Sequence number of the next ledger entry (unique per world).
    pub next_seq: u64,
    /// Players who already had their founding grant.
    pub granted: BTreeSet<PlayerId>,
    pub listings: BTreeMap<u64, Listing>,
    pub next_listing: u64,
}

/// Not enough renown for a payment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Insufficient {
    pub balance: u64,
    pub needed: u64,
}

impl Economy {
    pub fn new() -> Self {
        Self { next_seq: 1, next_listing: 1, ..Default::default() }
    }

    /// Start from balances (and recent ledger entries, oldest first) kept by
    /// the store: a new world, or a saved game from before the economy.
    /// Players with a stored balance already had their founding grant.
    pub fn from_store(balances: &[(PlayerId, u64)], ledger: Vec<LedgerEntry>) -> Self {
        let mut e = Economy::new();
        for &(p, b) in balances {
            e.balances.insert(p, b);
            e.granted.insert(p);
        }
        e.next_seq = ledger.iter().map(|l| l.seq + 1).max().unwrap_or(1).max(1);
        for l in ledger {
            e.push(l);
        }
        e
    }

    pub fn balance(&self, player: PlayerId) -> u64 {
        self.balances.get(&player).copied().unwrap_or(0)
    }

    fn push(&mut self, entry: LedgerEntry) {
        let list = self.ledger.entry(entry.player).or_default();
        list.push_back(entry);
        while list.len() > LEDGER_KEPT {
            list.pop_front();
        }
    }

    /// Change a balance and record why. A debit larger than the balance is
    /// refused (nothing changes).
    #[allow(clippy::too_many_arguments)]
    pub fn post(
        &mut self,
        player: PlayerId,
        delta: i64,
        kind: LedgerKind,
        text: String,
        offer: Option<u64>,
        listing: Option<u64>,
        tick: u64,
        day: u64,
    ) -> Result<LedgerEntry, Insufficient> {
        let balance = self.balance(player);
        let after = if delta >= 0 {
            balance.saturating_add(delta as u64)
        } else {
            balance.checked_sub(delta.unsigned_abs()).ok_or(Insufficient { balance, needed: delta.unsigned_abs() })?
        };
        self.balances.insert(player, after);
        let entry = LedgerEntry { seq: self.next_seq, player, tick, day, delta, balance: after, kind, text, offer, listing };
        self.next_seq += 1;
        self.push(entry.clone());
        Ok(entry)
    }

    /// Recent entries of a player, newest first.
    pub fn entries(&self, player: PlayerId, limit: usize) -> Vec<LedgerEntry> {
        self.ledger.get(&player).map(|l| l.iter().rev().take(limit).cloned().collect()).unwrap_or_default()
    }

    pub fn add_listing(&mut self, owner: PlayerId, organism: OrgId, fee: u64, created_tick: u64, expires_tick: Option<u64>) -> Listing {
        let id = self.next_listing.max(1);
        self.next_listing = id + 1;
        let l = Listing { id, owner, organism, fee, created_tick, expires_tick, sales: 0 };
        self.listings.insert(id, l.clone());
        l
    }

    pub fn listing_of(&self, organism: OrgId) -> Option<&Listing> {
        self.listings.values().find(|l| l.organism == organism)
    }

    pub fn listings_of(&self, owner: PlayerId) -> usize {
        self.listings.values().filter(|l| l.owner == owner).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn income_follows_the_legacy_share() {
        assert_eq!(daily_income(0.0, 0.0, false), 0);
        assert_eq!(daily_income(0.5, 0.5, false), 0, "an extinct lineage earns nothing");
        assert_eq!(daily_income(0.0, 0.0, true), DAILY_STIPEND);
        assert_eq!(daily_income(0.10, 0.06, true), 2 + 8);
        assert_eq!(daily_income(0.25, 0.25, true), 2 + 25);
        assert_eq!(daily_income(1.0, 1.0, true), 102);
        // Out-of-range shares are clamped.
        assert_eq!(daily_income(3.0, -1.0, true), 2 + 50);
        assert_eq!(achievement_bonus("species_founded"), 100);
        assert_eq!(achievement_bonus("unknown"), 0);
    }

    #[test]
    fn ledger_records_every_change_and_refuses_overdrafts() {
        let mut e = Economy::new();
        let a = e.post(1, 50, LedgerKind::Grant, "grant".into(), None, None, 0, 0).unwrap();
        assert_eq!((a.seq, a.balance), (1, 50));
        let b = e.post(1, -30, LedgerKind::Escrow, "hold".into(), Some(4), None, 10, 0).unwrap();
        assert_eq!((b.seq, b.balance, b.offer), (2, 20, Some(4)));
        assert_eq!(e.post(1, -21, LedgerKind::Escrow, "too much".into(), None, None, 11, 0), Err(Insufficient { balance: 20, needed: 21 }));
        assert_eq!(e.balance(1), 20);
        assert_eq!(e.next_seq, 3, "a refused debit uses no sequence number");
        e.post(2, 7, LedgerKind::Income, "day".into(), None, None, 48, 1).unwrap();
        let mine = e.entries(1, 10);
        assert_eq!(mine.iter().map(|x| x.seq).collect::<Vec<_>>(), vec![2, 1], "newest first");
        assert_eq!(e.entries(1, 1).len(), 1);
        for k in 0..(LEDGER_KEPT as u64 + 5) {
            e.post(3, 1, LedgerKind::Income, format!("{k}"), None, None, k, k).unwrap();
        }
        assert_eq!(e.ledger[&3].len(), LEDGER_KEPT);
        assert_eq!(e.balance(3), LEDGER_KEPT as u64 + 5, "the balance counts every entry, kept or not");
    }

    #[test]
    fn restoring_from_the_store_continues_the_sequence() {
        let entry = |seq, player| LedgerEntry {
            seq,
            player,
            tick: 0,
            day: 0,
            delta: 1,
            balance: 1,
            kind: LedgerKind::Income,
            text: String::new(),
            offer: None,
            listing: None,
        };
        let e = Economy::from_store(&[(1, 40), (2, 3)], vec![entry(7, 1), entry(9, 2)]);
        assert_eq!((e.balance(1), e.balance(2), e.balance(3)), (40, 3, 0));
        assert_eq!(e.next_seq, 10);
        assert!(e.granted.contains(&1) && !e.granted.contains(&3));
        assert_eq!(e.entries(2, 5)[0].seq, 9);
    }

    #[test]
    fn listings_are_numbered_and_found_by_organism() {
        let mut e = Economy::new();
        let a = e.add_listing(1, 100, 5, 0, None);
        let b = e.add_listing(1, 101, 0, 3, Some(99));
        assert_eq!((a.id, b.id), (1, 2));
        assert_eq!(e.listing_of(101).map(|l| l.id), Some(2));
        assert_eq!(e.listings_of(1), 2);
        assert_eq!(e.listings_of(2), 0);
    }
}
