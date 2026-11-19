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
//!   Plus a **breeder's royalty** ([`royalty`]): [`ROYALTY_PER_POINT`]
//!   renown per percentage point of all scored alleles that are the
//!   player's and live in other players' lineages (not the wild), so genes
//!   that spread through trades earn their breeder renown.
//!
//! **Spending**: a directed request with a price, or buying a stud
//! marketplace listing, moves the price from the payer's balance into
//! escrow on the agreement. When the mating runs (`completed`, conceived or
//! not) the escrow goes to the partner's owner, less the
//! [`MARKET_FEE_PERCENT`] market fee, which is destroyed (a sink); any
//! other ending (declined, cancelled, failed, expired) refunds it in full.
//!
//! **The renown shop** (sinks; renown is earned in play, so each perk is
//! modest and priced to rise): an extra directed-mating slot for the rest
//! of the weather season ([`slot_price`], doubling with each one bought,
//! at most [`MAX_BOUGHT_SLOTS`]); and **reinforcements** for a small
//! lineage ([`reinforce_price`]): [`REINFORCE_SIZE`] adults copied from its
//! own living adults join it, only while it has at most
//! [`REINFORCE_MAX_SHARE`] of the soft lineage cap alive, at a price that
//! doubles with each purchase in a ranked season. Together they give the
//! mating market its reason: a large lineage near its soft cap spreads its
//! genes by paying others' dams (fees, royalty), a small lineage earns
//! renown by hosting them and spends it on catching up.
//!
//! **Auctions** ([`Auction`]) are listings that sell one mating to the
//! highest bid at a deadline. A bid moves its amount from the bidder's
//! balance into escrow on the auction (a bidder raising their own bid pays
//! only the difference); an outbid bidder is refunded at once. At the
//! deadline the highest bid (never below the reserve) becomes an accepted
//! agreement with the bid as its escrowed price, settled like any other
//! (paid when the mating runs, refunded otherwise); with no bid the lot
//! returns to the seller.
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
/// Longest auction, in simulated days.
pub const MAX_AUCTION_DAYS: u32 = 30;
/// Share of every stud fee or price paid out that is destroyed, percent.
pub const MARKET_FEE_PERCENT: u64 = 10;
/// Renown a day per percentage point of all scored alleles that are a
/// player's and live in other players' lineages ([`royalty`]; alleles in
/// the wild don't pay).
pub const ROYALTY_PER_POINT: f64 = 5.0;
/// Price of the first extra mating slot of a weather season; each further
/// one costs twice the last ([`slot_price`]).
pub const SLOT_BASE_PRICE: u64 = 30;
/// Most extra mating slots one player may buy in a weather season.
pub const MAX_BOUGHT_SLOTS: u32 = 6;
/// Adults one reinforcement adds.
pub const REINFORCE_SIZE: u32 = 10;
/// Price of a ranked season's first reinforcement; each further one costs
/// twice the last ([`reinforce_price`]).
pub const REINFORCE_BASE_PRICE: u64 = 150;
/// A provision: the lineage's hungriest or thirstiest adults get
/// [`PROVISION_AMOUNT`] off their hunger and thirst, as a cache found or a
/// spring reached would; at most [`MAX_PROVISIONS_PER_SEASON`] within a
/// ranked season's length. A small lineage (at most a fifth of the soft cap
/// alive, as reinforcements) provisions [`PROVISION_SIZE`] adults at
/// [`PROVISION_PER_ANIMAL`] each; a larger one [`PROVISION_SIZE`] per fifth
/// of the cap it holds (at most [`PROVISION_MAX_SIZE`]) at
/// [`PROVISION_PER_ANIMAL_LARGE`] each: a sink that scales with the leaders'
/// renown (playtest 5: 6,788 unspent) while feeding a small share of them.
/// Fewer adults fed cost less (playtest 5: feeding 12 cost the full 40).
pub const PROVISION_SIZE: usize = 20;
pub const PROVISION_MAX_SIZE: usize = 200;
pub const PROVISION_PER_ANIMAL: u64 = 2;
pub const PROVISION_PER_ANIMAL_LARGE: u64 = 4;
/// A full small provision (20 adults).
pub const PROVISION_PRICE: u64 = PROVISION_SIZE as u64 * PROVISION_PER_ANIMAL;
pub const PROVISION_AMOUNT: f32 = 50.0;
pub const MAX_PROVISIONS_PER_SEASON: u32 = 3;

/// A survey (`survey`): where rivals' groups and territories are within
/// [`SURVEY_RADIUS`] tiles of your main group, coarsely (8-tile cells), at
/// that moment. Information, not share: a sink for leaders' renown that
/// scales with the lineage, [`SURVEY_BASE`] plus a renown per
/// [`SURVEY_PER_ANIMALS`] animals alive (playtest 6: 4,800–10,000 unspent).
pub const SURVEY_BASE: u64 = 30;
pub const SURVEY_PER_ANIMALS: u64 = 10;
pub const SURVEY_RADIUS: f32 = 64.0;

/// Renown to name (or rename) one of your animals (`name_animal`): a
/// cosmetic sink, shown on its listings and in your own views (playtest 7:
/// 600–4,700 renown unspent).
pub const ANIMAL_NAME_PRICE: u64 = 50;

/// The price of a survey for a lineage of `alive` animals.
pub fn survey_price(alive: usize) -> u64 {
    SURVEY_BASE + alive as u64 / SURVEY_PER_ANIMALS
}

/// Renown for a first nameplate (a cosmetic sink for leaders).
pub const NAMEPLATE_PRICE: u64 = 500;
/// Renown to change a nameplate afterwards.
pub const NAMEPLATE_CHANGE_PRICE: u64 = 100;
/// Longest nameplate title, in characters.
pub const MAX_TITLE_LEN: usize = 24;
/// The nameplate palette.
pub const NAMEPLATE_COLORS: [&str; 8] = ["amber", "crimson", "emerald", "azure", "violet", "ivory", "jet", "copper"];
/// Renown to name a species your lineage founded (cosmetic: the name shows
/// in the species list, the chronicle and the family tree).
pub const SPECIES_NAME_PRICE: u64 = 300;

/// Most reinforcements one player may buy within a ranked season's length
/// (a rolling window, not reset at a season boundary), whatever lineages
/// they had in it: retiring and re-founding neither resets the price nor
/// the count (playtest 2: 4,650 hoarded renown bought +50 adults right
/// after a re-found; playtest 3: +40 across a season boundary).
pub const MAX_REINFORCEMENTS_PER_SEASON: u32 = 2;
/// A lineage may be reinforced only while at most this share of the soft
/// lineage cap (`WorldConfig::lineage_cap`) is alive.
pub const REINFORCE_MAX_SHARE: f64 = 0.2;

/// The market fee kept from a payout of `amount`: [`MARKET_FEE_PERCENT`]
/// rounded to the nearest renown (halves up), and none below 10: 1–9 pay
/// none, 10–14 pay 1, 15–24 pay 2. (Playtest 5: with "at
/// least 1" a 2-renown sale lost half and 3–9 lost 11–33%; listings are
/// at least 5 renown now, so small prices no longer dodge it.)
pub fn market_fee(amount: u64) -> u64 {
    // Below 10 none (playtest 6: a 5-renown sale lost a fifth).
    if amount < 10 {
        return 0;
    }
    (amount * MARKET_FEE_PERCENT + 50) / 100
}

/// Renown a day from alleles living in other players' lineages:
/// [`ROYALTY_PER_POINT`] per percentage point of all scored alleles,
/// rounded (none for an extinct lineage).
pub fn royalty(exported_share: f64, alive: bool) -> u64 {
    if !alive {
        return 0;
    }
    (100.0 * ROYALTY_PER_POINT * exported_share.clamp(0.0, 1.0)).round() as u64
}

/// Price of the next extra mating slot after `bought` this weather season
/// (none at [`MAX_BOUGHT_SLOTS`]).
pub fn slot_price(bought: u32) -> Option<u64> {
    (bought < MAX_BOUGHT_SLOTS).then(|| SLOT_BASE_PRICE << bought)
}

/// Price of the next reinforcement after `bought` this ranked season.
pub fn reinforce_price(bought: u32) -> u64 {
    REINFORCE_BASE_PRICE.saturating_mul(1u64.checked_shl(bought.min(40)).unwrap_or(u64::MAX))
}

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
    /// Set for an auction: one mating goes to the highest bid at the
    /// deadline (`expires_tick`). None for a fixed-price listing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auction: Option<Auction>,
}

impl Listing {
    /// What it costs now: the fee, or an auction's highest bid (its reserve
    /// before the first bid).
    pub fn price(&self) -> u64 {
        match &self.auction {
            Some(a) => a.high.as_ref().map_or(a.reserve, |b| b.amount),
            None => self.fee,
        }
    }
}

/// The highest bid on an auction, held in escrow from the bidder's balance.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bid {
    pub bidder: PlayerId,
    pub amount: u64,
    /// The bidder's organism for the mating if the bid wins.
    pub mine: OrgId,
    pub tick: u64,
}

/// An auction listing's state. The deadline is the listing's `expires_tick`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Auction {
    /// The lowest winning bid.
    pub reserve: u64,
    /// Only the highest bid is kept: an outbid bidder is refunded at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<Bid>,
    /// Bids placed so far.
    #[serde(default)]
    pub bids: u32,
}

impl Auction {
    /// The smallest bid accepted now: the reserve (at least 1) for the
    /// first bid, then the highest bid plus 5% (at least 1 more).
    pub fn min_bid(&self) -> u64 {
        match &self.high {
            None => self.reserve.max(1),
            Some(b) => b.amount.saturating_add((b.amount / 20).max(1)),
        }
    }
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
    /// Reinforcements bought per player: (ranked season, how many in it).
    /// Absent from older saves (none bought). Superseded by
    /// `reinforce_days` (kept so older saves read).
    pub reinforcements: BTreeMap<PlayerId, (u32, u32)>,
    /// The simulated day of each player's recent reinforcements: the cap
    /// and the price count those within a ranked season's length
    /// (a rolling window; playtest 3 bought +40 adults across a season
    /// boundary in two minutes).
    pub reinforce_days: BTreeMap<PlayerId, Vec<u64>>,
    /// The simulated day of each player's recent provisions (`provision`).
    pub provision_days: BTreeMap<PlayerId, Vec<u64>>,
    /// The day each player's last lineage ended, retired or died out
    /// (re-founding soon after costs founders: `Game::refound_terms`).
    pub retired_day: BTreeMap<PlayerId, u64>,
    /// The simulated days of each player's recent re-foundings within the
    /// window (`Game::refound_terms`: each one doubles the next's fee).
    #[serde(default)]
    pub refound_days: BTreeMap<PlayerId, Vec<u64>>,
    /// Re-founding fees not yet paid (the balance was short): taken from
    /// the player's next renown as it comes in (playtest 7: spending the
    /// renown first made a re-founding free).
    #[serde(default)]
    pub refound_debt: BTreeMap<PlayerId, u64>,
    /// Names players gave their animals (`name_animal`: cosmetic).
    #[serde(default)]
    pub animal_names: BTreeMap<OrgId, String>,
    /// Players whose last lineage died out (rather than was retired), for
    /// the wording.
    pub died_out: BTreeSet<PlayerId>,
    /// Bought nameplates (`set_nameplate`): a color from [`NAMEPLATE_COLORS`]
    /// and a short title shown with the player's name. Cosmetic only.
    pub nameplates: BTreeMap<PlayerId, Nameplate>,
    /// Species named by a player (`name_species`): species id → player.
    pub named_species: BTreeMap<u32, PlayerId>,
    /// Each player's renown by kind since their first entry counted
    /// (`LedgerView::totals`; saves from before it count from the restart).
    pub totals: BTreeMap<PlayerId, crate::protocol::LedgerTotals>,
}

/// A player's bought nameplate: cosmetic only.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Nameplate {
    pub color: String,
    pub title: String,
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
        self.totals
            .entry(entry.player)
            .or_insert(crate::protocol::LedgerTotals { since_day: entry.day, ..Default::default() })
            .add(&entry);
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
        let entry =
            LedgerEntry { seq: self.next_seq, player, tick, day, delta, balance: after, kind, text, offer, listing };
        self.next_seq += 1;
        self.push(entry.clone());
        Ok(entry)
    }

    /// Recent entries of a player, newest first.
    pub fn entries(&self, player: PlayerId, limit: usize) -> Vec<LedgerEntry> {
        self.ledger.get(&player).map(|l| l.iter().rev().take(limit).cloned().collect()).unwrap_or_default()
    }

    pub fn add_listing(
        &mut self,
        owner: PlayerId,
        organism: OrgId,
        fee: u64,
        created_tick: u64,
        expires_tick: Option<u64>,
    ) -> Listing {
        let id = self.next_listing.max(1);
        self.next_listing = id + 1;
        let l = Listing { id, owner, organism, fee, created_tick, expires_tick, sales: 0, auction: None };
        self.listings.insert(id, l.clone());
        l
    }

    /// An auction of one mating with `organism`, settling at `deadline`.
    pub fn add_auction(
        &mut self,
        owner: PlayerId,
        organism: OrgId,
        reserve: u64,
        created_tick: u64,
        deadline: u64,
    ) -> Listing {
        let id = self.next_listing.max(1);
        self.next_listing = id + 1;
        let l = Listing {
            id,
            owner,
            organism,
            fee: reserve,
            created_tick,
            expires_tick: Some(deadline),
            sales: 0,
            auction: Some(Auction { reserve, high: None, bids: 0 }),
        };
        self.listings.insert(id, l.clone());
        l
    }

    /// Renown of `player`'s held as the highest bid on open auctions.
    pub fn bids_held(&self, player: PlayerId) -> u64 {
        self.listings
            .values()
            .filter_map(|l| l.auction.as_ref()?.high.as_ref())
            .filter(|b| b.bidder == player)
            .map(|b| b.amount)
            .sum()
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
    fn shop_prices_rise_and_fees_and_royalties_round() {
        assert_eq!(
            (market_fee(0), market_fee(5), market_fee(9), market_fee(10), market_fee(14), market_fee(15)),
            (0, 0, 0, 1, 1, 2)
        );
        assert_eq!(market_fee(40), 4);
        assert_eq!(royalty(0.025, true), 13);
        assert_eq!(royalty(0.5, false), 0);
        assert_eq!(slot_price(0), Some(30));
        assert_eq!(slot_price(2), Some(120));
        assert_eq!(slot_price(MAX_BOUGHT_SLOTS), None);
        assert_eq!(reinforce_price(0), 150);
        assert_eq!(reinforce_price(3), 1200);
        assert!(reinforce_price(80) > 1_000_000_000_000, "never wraps");
    }

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
        assert_eq!(
            e.post(1, -21, LedgerKind::Escrow, "too much".into(), None, None, 11, 0),
            Err(Insufficient { balance: 20, needed: 21 })
        );
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
        assert_eq!((a.price(), a.auction.is_none()), (5, true));
    }

    #[test]
    fn auctions_price_at_the_highest_bid_and_step_up_by_five_percent() {
        let mut e = Economy::new();
        let l = e.add_auction(1, 100, 40, 10, 58);
        assert_eq!((l.id, l.expires_tick, l.price()), (1, Some(58), 40));
        let a = l.auction.clone().unwrap();
        assert_eq!(a.min_bid(), 40);
        assert_eq!(Auction { reserve: 0, high: None, bids: 0 }.min_bid(), 1, "a bid is at least 1");
        let bid = |amount| Some(Bid { bidder: 2, amount, mine: 7, tick: 11 });
        let high = Auction { high: bid(40), bids: 1, ..a.clone() };
        assert_eq!(high.min_bid(), 42);
        assert_eq!(Auction { high: bid(10), ..a.clone() }.min_bid(), 11, "at least one more");
        assert_eq!(Auction { high: bid(u64::MAX), ..a }.min_bid(), u64::MAX);
        e.listings.get_mut(&1).unwrap().auction = Some(high);
        assert_eq!((e.listings[&1].price(), e.bids_held(2), e.bids_held(1)), (40, 40, 0));
        // Auctions count against the listing limit like fixed listings.
        assert_eq!(e.listings_of(1), 1);
        // Saved games from before auctions read as fixed listings.
        let old = r#"{"id":3,"owner":1,"organism":5,"fee":2,"created_tick":0}"#;
        let l: Listing = serde_json::from_str(old).unwrap();
        assert_eq!((l.auction.is_none(), l.sales, l.price()), (true, 0, 2));
    }
}
