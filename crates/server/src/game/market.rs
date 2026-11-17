//! An index of the marketplace listings, kept up to date as they change,
//! so a `browse_market` page costs what it shows instead of a pass over
//! every listing.
//!
//! For each order a page can take (`MarketSort`), the index holds the
//! listings' sort keys, exactly the keys [`super::market_key`] gives them
//! when the order needs no previews. It also counts the listings by kind
//! and role, overall and per owner, and keeps each owner's listings, the
//! listing of each organism and the renown each bidder holds on auctions.
//! A page then walks one order from its cursor, testing each listing
//! against the filters as the full scan does, until it has a page and one
//! more. The counts give the total, the number of matches.
//!
//! Every change to `Economy::listings` goes through the game, which tells
//! the index ([`MarketIndex::insert`], [`MarketIndex::remove`],
//! [`MarketIndex::update`]). A replaced economy (a restored or seeded game)
//! is indexed again ([`MarketIndex::rebuild`]). Each change records the
//! listings' count and the next listing id it saw. When those don't match
//! the economy, the index is out of date and the game falls back to the
//! scan ([`MarketIndex::in_sync`]). In tests every page is also made by the
//! scan, and the two must agree.
//!
//! The counts are exact only when every listing's organism is alive and
//! still its owner's. `Game::maintain_listings` makes that so at every
//! tick: organisms only die in a tick, never change hands, and a listing
//! is only created for a living organism. The index remembers the tick
//! ([`MarketIndex::validated`]). Before the first maintenance after a
//! rebuild, pages use the scan.

use std::collections::{BTreeSet, HashMap};
use std::ops::Bound;

use evo_sim::organism::Sex;
use evo_sim::{PlayerId, World};

use crate::economy::{Economy, Listing};
use crate::protocol::{ListingKind, ListingRole, MarketSort, OrgId};

#[cfg(test)]
thread_local! {
    /// Pages made from the index on this thread (tests).
    pub(super) static INDEXED_PAGES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Compare every indexed page with the scan (off for benchmarks).
    pub(super) static CHECK_PAGES: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// A sort key, as `market_key` makes it: (key, newest first).
pub(super) type Key = (u64, u64);

/// What the index keeps of a listing, so it can drop it again.
#[derive(Clone, Debug)]
struct Entry {
    owner: PlayerId,
    organism: OrgId,
    kind: usize,
    /// None when its organism was gone at indexing (it is then not counted).
    role: Option<usize>,
    price: u64,
    expires: Option<u64>,
    /// The highest bid's bidder and amount.
    bid: Option<(PlayerId, u64)>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct MarketIndex {
    newest: BTreeSet<Key>,
    price_asc: BTreeSet<Key>,
    price_desc: BTreeSet<Key>,
    ending: BTreeSet<Key>,
    entries: HashMap<u64, Entry>,
    by_organism: HashMap<OrgId, u64>,
    owned: HashMap<PlayerId, BTreeSet<u64>>,
    /// Counted listings by [kind][role], all and per owner.
    all: [[usize; 2]; 2],
    per_owner: HashMap<PlayerId, [[usize; 2]; 2]>,
    bids: HashMap<PlayerId, u64>,
    /// `Economy::listings.len()` and `next_listing` at the last change.
    seen: (usize, u64),
    /// The tick at which every listing's organism was last known to be
    /// alive (see the module docs).
    pub(super) validated: Option<u64>,
}

fn kind_ix(l: &Listing) -> usize {
    usize::from(l.auction.is_some())
}

fn kind_of(k: ListingKind) -> usize {
    match k {
        ListingKind::Fixed => 0,
        ListingKind::Auction => 1,
    }
}

fn role_of(r: ListingRole) -> usize {
    match r {
        ListingRole::Stud => 0,
        ListingRole::Dam => 1,
    }
}

/// The role a listing of an organism of this sex has (`Game::market`).
pub(super) fn role_ix(sex: Sex) -> usize {
    if sex == Sex::Male {
        0
    } else {
        1
    }
}

fn high_bid(l: &Listing) -> Option<(PlayerId, u64)> {
    l.auction.as_ref()?.high.as_ref().map(|b| (b.bidder, b.amount))
}

impl Entry {
    fn keys(&self, id: u64) -> [Key; 4] {
        let newest = u64::MAX - id;
        [(0, newest), (self.price, newest), (u64::MAX - self.price, newest), (self.expires.unwrap_or(u64::MAX), newest)]
    }
}

impl MarketIndex {
    /// An index of `economy`'s listings, reading the organisms' roles from
    /// `world`.
    pub(super) fn build(economy: &Economy, world: &World) -> Self {
        let mut m = MarketIndex::default();
        m.rebuild(economy, world);
        m
    }

    /// Index `economy` from scratch (not validated until the next
    /// maintenance).
    pub(super) fn rebuild(&mut self, economy: &Economy, world: &World) {
        *self = MarketIndex::default();
        for l in economy.listings.values() {
            let role = world.organism(l.organism).map(|o| role_ix(o.sex));
            self.add(l, role);
        }
        self.saw(economy);
    }

    /// Whether the index matches `economy` (as far as it can tell).
    pub(super) fn in_sync(&self, economy: &Economy) -> bool {
        self.seen == (economy.listings.len(), economy.next_listing)
    }

    /// Whether pages can be made from the index at `tick`.
    pub(super) fn usable(&self, economy: &Economy, tick: u64) -> bool {
        self.in_sync(economy) && self.validated == Some(tick)
    }

    fn saw(&mut self, economy: &Economy) {
        self.seen = (economy.listings.len(), economy.next_listing);
    }

    fn add(&mut self, l: &Listing, role: Option<usize>) {
        let e = Entry {
            owner: l.owner,
            organism: l.organism,
            kind: kind_ix(l),
            role,
            price: l.price(),
            expires: l.expires_tick,
            bid: high_bid(l),
        };
        let [a, b, c, d] = e.keys(l.id);
        self.newest.insert(a);
        self.price_asc.insert(b);
        self.price_desc.insert(c);
        self.ending.insert(d);
        self.by_organism.insert(l.organism, l.id);
        self.owned.entry(l.owner).or_default().insert(l.id);
        if let Some(r) = role {
            self.all[e.kind][r] += 1;
            self.per_owner.entry(l.owner).or_default()[e.kind][r] += 1;
        }
        if let Some((bidder, amount)) = e.bid {
            *self.bids.entry(bidder).or_default() += amount;
        }
        self.entries.insert(l.id, e);
    }

    fn drop_entry(&mut self, id: u64) -> Option<Entry> {
        let e = self.entries.remove(&id)?;
        let [a, b, c, d] = e.keys(id);
        self.newest.remove(&a);
        self.price_asc.remove(&b);
        self.price_desc.remove(&c);
        self.ending.remove(&d);
        if self.by_organism.get(&e.organism) == Some(&id) {
            self.by_organism.remove(&e.organism);
        }
        if let Some(ids) = self.owned.get_mut(&e.owner) {
            ids.remove(&id);
            if ids.is_empty() {
                self.owned.remove(&e.owner);
            }
        }
        if let Some(r) = e.role {
            self.all[e.kind][r] -= 1;
            if let Some(c) = self.per_owner.get_mut(&e.owner) {
                c[e.kind][r] -= 1;
                if c.iter().flatten().all(|&n| n == 0) {
                    self.per_owner.remove(&e.owner);
                }
            }
        }
        if let Some((bidder, amount)) = e.bid {
            if let Some(held) = self.bids.get_mut(&bidder) {
                *held -= amount;
                if *held == 0 {
                    self.bids.remove(&bidder);
                }
            }
        }
        Some(e)
    }

    /// A listing was just added to `economy` (`l`, of an organism of `sex`).
    pub(super) fn insert(&mut self, economy: &Economy, l: &Listing, sex: Option<Sex>) {
        self.add(l, sex.map(role_ix));
        self.saw(economy);
    }

    /// Listing `id` was just removed from `economy`.
    pub(super) fn remove(&mut self, economy: &Economy, id: u64) {
        self.drop_entry(id);
        self.saw(economy);
    }

    /// Listing `l` changed (a bid: its price and highest bidder).
    pub(super) fn update(&mut self, economy: &Economy, l: &Listing) {
        let role = self.drop_entry(l.id).and_then(|e| e.role);
        self.add(l, role);
        self.saw(economy);
    }

    /// The keys of an order, ascending (the order of a page).
    pub(super) fn order(&self, sort: MarketSort) -> &BTreeSet<Key> {
        match sort {
            MarketSort::Newest | MarketSort::Fertility => &self.newest,
            MarketSort::PriceAsc => &self.price_asc,
            MarketSort::PriceDesc => &self.price_desc,
            MarketSort::Ending => &self.ending,
        }
    }

    /// The keys of `sort` after `cursor` (all without one), ascending,
    /// with the listing id each belongs to.
    pub(super) fn walk(&self, sort: MarketSort, cursor: Option<Key>) -> impl Iterator<Item = (Key, u64)> + '_ {
        let set = self.order(sort);
        let lower = cursor.map_or(Bound::Unbounded, Bound::Excluded);
        set.range((lower, Bound::Unbounded)).map(|&k| (k, u64::MAX - k.1))
    }

    /// Listings that pass the kind and role filters, without `player`'s
    /// own when `hide_own`.
    pub(super) fn count(
        &self,
        player: PlayerId,
        kind: Option<ListingKind>,
        role: Option<ListingRole>,
        hide_own: bool,
    ) -> usize {
        let own = self.per_owner.get(&player).filter(|_| hide_own);
        let mut n = 0;
        for k in 0..2 {
            for r in 0..2 {
                if kind.is_some_and(|x| kind_of(x) != k) || role.is_some_and(|x| role_of(x) != r) {
                    continue;
                }
                n += self.all[k][r] - own.map_or(0, |c| c[k][r]);
            }
        }
        n
    }

    /// `player`'s listings, oldest first.
    pub(super) fn owned_by(&self, player: PlayerId) -> impl DoubleEndedIterator<Item = u64> + '_ {
        self.owned.get(&player).into_iter().flatten().copied()
    }

    /// How many listings `player` has open.
    pub(super) fn listings_of(&self, player: PlayerId) -> usize {
        self.owned.get(&player).map_or(0, BTreeSet::len)
    }

    /// The listing of `organism`, if it is listed.
    pub(super) fn listing_of(&self, organism: OrgId) -> Option<u64> {
        self.by_organism.get(&organism).copied()
    }

    /// Renown `player` holds as the highest bid on auctions.
    pub(super) fn bids_held(&self, player: PlayerId) -> u64 {
        self.bids.get(&player).copied().unwrap_or(0)
    }

    /// Tests: the same as an index built afresh from `economy` and `world`.
    #[cfg(test)]
    pub(super) fn matches(&self, economy: &Economy, world: &World) -> bool {
        let fresh = MarketIndex::build(economy, world);
        self.newest == fresh.newest
            && self.price_asc == fresh.price_asc
            && self.price_desc == fresh.price_desc
            && self.ending == fresh.ending
            && self.by_organism == fresh.by_organism
            && self.owned == fresh.owned
            && self.bids == fresh.bids
            && self.seen == fresh.seen
            // A listing indexed while its organism lived keeps its role
            // after the organism dies (until maintenance drops it).
            && fresh.entries.iter().all(|(id, e)| e.role.is_none() || self.entries[id].role == e.role)
    }
}
