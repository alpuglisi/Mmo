//! State messages: what each connection sees of the world, built on the
//! world thread for every state push (once a tick) and for the steering
//! players at every possession sub-step (three times a tick by default).
//!
//! A push builds a [`StateIndex`] (living organisms by owner and by 16-tile
//! cell, with each one's owner and position, open offers by player, the
//! shared clock, weather and leaderboard) in one pass over the organisms;
//! each state then costs only what it contains. The index lives in a
//! [`StateCache`] with what the states were built from: the view of every
//! organism shown as a neighbor (shared by every window it is in) and, for
//! players who steer, their own organisms' views. Between ticks the world
//! changes only through sub-steps (possessed organisms with a move order
//! walk a little) and the commands applied before each, so a sub-step
//! doesn't rebuild any of it: it brings forward exactly what could have
//! changed and reuses the rest.
//!
//! - Organisms a sub-step can move (the possessed ones, and any a command
//!   possessed or ordered since) and organisms a command may change (one it
//!   names, the pair of a directed mating, every organism of a player who
//!   gave a group order) are *dirty*: their place in the index is updated
//!   (a cell change moves one entry) and their cached views are dropped.
//! - Per player, the cached list of own organism views is patched for the
//!   dirty organisms among them (dropped whole after a group order).
//! - Everything else in a state (view window, which neighbors are in it and
//!   the nearest cut, offers, balances, escrow, groups, slots, population,
//!   the leaderboard cut) is recomputed for every state from the index, as
//!   it is cheap once no organism has to be read for it.
//! - A founding (new organisms) or a new tick drops the cache: the next
//!   push or sub-step builds it again.
//!
//! What a client receives is the same, byte for byte, as when every state
//! was built from scratch (`tests::cached_substep_states_match_the_old_build`
//! compares both over a busy world with commands between sub-steps).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use evo_sim::math::Vec2;
use evo_sim::organism::Organism;
use evo_sim::{PlayerId, WILD};

use super::{ConnId, Game, Outbox, Target};
use crate::protocol::*;

/// Side of a [`StateIndex`] grid cell, in tiles.
const STATE_CELL: f32 = 16.0;
/// `Slot::cell` of an organism that isn't indexed (dead).
const NO_CELL: u32 = u32::MAX;

/// What the index keeps of an organism: enough to pick the ones inside a
/// view window without reading the organism itself.
#[derive(Clone, Copy)]
struct Slot {
    owner: PlayerId,
    pos: Vec2,
    cell: u32,
}

/// The world sorted for building state messages: living organisms by owner
/// and by grid cell (indices into `World::organisms`, ascending within each
/// list), each one's owner and position, open offers by player, and what
/// every state shares (clock, weather, leaderboard). Building one is a pass
/// over the organisms; each state then costs only what it contains, so a
/// push to N connections is O(organisms + output) instead of
/// O(N × organisms). A sub-step updates it in place ([`StateCache`]).
struct StateIndex {
    by_owner: BTreeMap<PlayerId, Vec<u32>>,
    cols: usize,
    rows: usize,
    /// Organisms per cell, ascending.
    cells: Vec<Vec<u32>>,
    /// Per organism index.
    slots: Vec<Slot>,
    /// Possessed organisms when the index was built.
    steering: Vec<u32>,
    /// Open offers a player made or received, by id.
    offers: BTreeMap<PlayerId, Vec<u64>>,
    /// Renown held as the highest bid on auctions, per bidder.
    bids: BTreeMap<PlayerId, u64>,
    clock: ClockView,
    weather: WeatherView,
    board: Board,
}

/// The full leaderboard with each player's rank, for cutting it per state.
struct Board {
    full: Leaderboard,
    /// (player, rank), by player; empty when the board is sent whole.
    ranks: Vec<(PlayerId, usize)>,
    /// `Game::names_changed` when it was made.
    names: u64,
}

impl Board {
    fn new(g: &Game) -> Self {
        let full = g.leaderboard();
        let mut ranks = Vec::new();
        if full.rows.len() > g.cfg.state_leaderboard_rows {
            ranks = full.rows.iter().enumerate().map(|(k, r)| (r.player, k)).collect();
            ranks.sort_unstable();
        }
        Board { full, ranks, names: g.names_changed }
    }

    fn rank(&self, player: PlayerId) -> Option<usize> {
        self.ranks.binary_search_by_key(&player, |&(p, _)| p).ok().map(|k| self.ranks[k].1)
    }
}

impl StateIndex {
    fn new(g: &Game) -> Self {
        let mut idx = StateIndex {
            by_owner: BTreeMap::new(),
            cols: 1,
            rows: 1,
            cells: Vec::new(),
            slots: Vec::new(),
            steering: Vec::new(),
            offers: BTreeMap::new(),
            bids: BTreeMap::new(),
            clock: g.clock(),
            weather: g.weather(),
            board: Board::new(g),
        };
        idx.index_organisms(g);
        idx.refresh(g);
        idx
    }

    fn cell_of(&self, p: Vec2) -> u32 {
        let cx = ((p.x / STATE_CELL).floor().max(0.0) as usize).min(self.cols - 1);
        let cy = ((p.y / STATE_CELL).floor().max(0.0) as usize).min(self.rows - 1);
        (cy * self.cols + cx) as u32
    }

    /// Index the world from scratch, keeping the allocations.
    fn rebuild(&mut self, g: &Game) {
        self.index_organisms(g);
        self.refresh(g);
        self.board = Board::new(g);
    }

    fn index_organisms(&mut self, g: &Game) {
        let w = &g.world;
        self.cols = (w.map.width as f32 / STATE_CELL).ceil().max(1.0) as usize;
        self.rows = (w.map.height as f32 / STATE_CELL).ceil().max(1.0) as usize;
        self.cells.resize_with(self.cols * self.rows, Vec::new);
        self.cells.truncate(self.cols * self.rows);
        self.cells.iter_mut().for_each(Vec::clear);
        self.by_owner.values_mut().for_each(Vec::clear);
        self.slots.clear();
        self.slots.reserve(w.organisms.len());
        self.steering.clear();
        // One pass over the organisms (each a few cache lines).
        for (i, o) in w.organisms.iter().enumerate() {
            if !o.alive {
                self.slots.push(Slot { owner: o.owner, pos: o.pos, cell: NO_CELL });
                continue;
            }
            if o.owner != WILD {
                self.by_owner.entry(o.owner).or_default().push(i as u32);
            }
            if o.possessed_by.is_some() {
                self.steering.push(i as u32);
            }
            let k = self.cell_of(o.pos);
            self.cells[k as usize].push(i as u32);
            self.slots.push(Slot { owner: o.owner, pos: o.pos, cell: k });
        }
        self.by_owner.retain(|_, v| !v.is_empty());
    }

    /// What doesn't depend on organisms: offers, bids, clock and weather.
    fn refresh(&mut self, g: &Game) {
        self.offers.clear();
        for o in g.offers.values().filter(|o| o.status.is_open()) {
            self.offers.entry(o.from).or_default().push(o.id);
            if o.to != o.from {
                self.offers.entry(o.to).or_default().push(o.id);
            }
        }
        self.bids.clear();
        for b in g.economy.listings.values().filter_map(|l| l.auction.as_ref()?.high.as_ref()) {
            *self.bids.entry(b.bidder).or_default() += b.amount;
        }
        self.clock = g.clock();
        self.weather = g.weather();
    }

    /// A player's living organisms, in organism order (none for the wild).
    fn owned(&self, player: PlayerId) -> &[u32] {
        self.by_owner.get(&player).map_or(&[], Vec::as_slice)
    }

    /// Organism `i` changed: follow it to its new position. False if it
    /// died or changed hands, which the index can't follow (it is rebuilt).
    fn update(&mut self, i: u32, o: &Organism) -> bool {
        let s = self.slots[i as usize];
        if s.cell == NO_CELL || !o.alive || o.owner != s.owner {
            return s.cell == NO_CELL && !o.alive;
        }
        if o.pos == s.pos {
            return true;
        }
        let k = self.cell_of(o.pos);
        if k != s.cell {
            let old = &mut self.cells[s.cell as usize];
            if let Ok(at) = old.binary_search(&i) {
                old.remove(at);
            }
            let new = &mut self.cells[k as usize];
            let at = new.binary_search(&i).unwrap_or_else(|at| at);
            new.insert(at, i);
        }
        self.slots[i as usize] = Slot { owner: s.owner, pos: o.pos, cell: k };
        true
    }

    /// The foreign organisms (not `player`'s) inside the window, as
    /// (distance from its center, organism index): the nearest
    /// `max_nearby`, in organism order unless there were more (then nearest
    /// first, ties in organism order), as a scan of the world gives them.
    fn nearby(&self, v: &ViewWindow, player: PlayerId, max_nearby: usize) -> Vec<(f32, u32)> {
        if !(v.x.is_finite() && v.y.is_finite() && v.radius.is_finite()) {
            return Vec::new();
        }
        let span = |lo: f32, hi: f32, n: usize| {
            let a = ((lo / STATE_CELL).floor().max(0.0) as usize).min(n - 1);
            let b = ((hi / STATE_CELL).floor().max(0.0) as usize).min(n - 1);
            a..=b
        };
        let center = Vec2::new(v.x, v.y);
        let mut out = Vec::new();
        for cy in span(v.y - v.radius, v.y + v.radius, self.rows) {
            for cx in span(v.x - v.radius, v.x + v.radius, self.cols) {
                for &i in &self.cells[cy * self.cols + cx] {
                    let s = &self.slots[i as usize];
                    if s.owner != player && (s.pos.x - v.x).abs() <= v.radius && (s.pos.y - v.y).abs() <= v.radius {
                        out.push((s.pos.dist(center), i));
                    }
                }
            }
        }
        out.sort_unstable_by_key(|&(_, i)| i);
        if out.len() > max_nearby {
            out.sort_by(|a, b| a.0.total_cmp(&b.0));
            out.truncate(max_nearby);
        }
        out
    }
}

/// Views of organisms by organism index, valid for one stamp (a tick's
/// index); an entry is dropped when its organism changes.
struct ViewCache<T> {
    stamp: u64,
    entries: Vec<(u64, Option<T>)>,
}

impl<T: Clone> ViewCache<T> {
    fn new() -> Self {
        ViewCache { stamp: 0, entries: Vec::new() }
    }

    /// Forget every entry (in O(1)).
    fn reset(&mut self) {
        self.stamp += 1;
    }

    fn forget(&mut self, i: u32) {
        if let Some(e) = self.entries.get_mut(i as usize) {
            e.0 = 0;
        }
    }

    fn get(&mut self, i: u32, make: impl FnOnce() -> T) -> T {
        let i = i as usize;
        if i >= self.entries.len() {
            self.entries.resize_with(i + 1, || (0, None));
        }
        let e = &mut self.entries[i];
        match &e.1 {
            Some(v) if e.0 == self.stamp => v.clone(),
            _ => {
                let v = make();
                *e = (self.stamp, Some(v.clone()));
                v
            }
        }
    }
}

/// What changed since the cache was last brought up to date, as commands
/// reported it ([`Game::note_command`]).
#[derive(Default)]
struct Dirty {
    /// Organisms a command may have changed.
    orgs: Vec<u64>,
    /// Organisms a command possessed, ordered or released: from now on
    /// moved by sub-steps.
    steered: Vec<u64>,
    /// Players a group order went to (every organism of theirs).
    owners: Vec<PlayerId>,
}

/// The state index and organism views kept from one state build to the
/// next within a tick (see the module docs).
pub(super) struct StateCache {
    /// The world tick the index belongs to; `None` once dropped.
    tick: Option<u64>,
    /// Kept after the tick for its allocations.
    idx: Option<StateIndex>,
    /// Organisms sub-steps may move: possessed at the build or since.
    steering: BTreeSet<u32>,
    near: ViewCache<NearbyOrganism>,
    own: OwnViews,
    dirty: Dirty,
}

impl Default for StateCache {
    fn default() -> Self {
        StateCache {
            tick: None,
            idx: None,
            steering: BTreeSet::new(),
            near: ViewCache::new(),
            own: HashMap::new(),
            dirty: Dirty::default(),
        }
    }
}

impl StateCache {
    /// Drop the index: the next state build starts from scratch.
    pub(super) fn invalidate(&mut self) {
        self.tick = None;
    }

    fn valid(&self) -> bool {
        self.tick.is_some()
    }

    /// A command may have changed organism `id`.
    pub(super) fn touch(&mut self, id: u64) {
        if self.valid() {
            self.dirty.orgs.push(id);
        }
    }

    /// A command possessed, ordered or released organism `id`.
    pub(super) fn steer(&mut self, id: u64) {
        if self.valid() {
            self.dirty.steered.push(id);
        }
    }

    /// A command may have changed every organism of `player`.
    pub(super) fn touch_owner(&mut self, player: PlayerId) {
        if self.valid() {
            self.dirty.owners.push(player);
        }
    }

    /// Index the world from scratch for a new build.
    fn rebuild(&mut self, g: &Game) {
        match &mut self.idx {
            Some(idx) => idx.rebuild(g),
            None => self.idx = Some(StateIndex::new(g)),
        }
        let idx = self.idx.as_ref().expect("just built");
        self.steering = idx.steering.iter().copied().collect();
        self.near.reset();
        self.own.clear();
        self.dirty = Dirty::default();
        self.tick = Some(g.world.tick());
    }

    /// Make the cache current for a sub-step: bring forward the dirty
    /// organisms if it is still this tick's, else build it again.
    fn prepare(&mut self, g: &Game) {
        let w = &g.world;
        let current = self.valid()
            && self.tick == Some(w.tick())
            && self.idx.as_ref().is_some_and(|idx| idx.slots.len() == w.organisms.len());
        if !current || !self.bring_forward(g) {
            self.rebuild(g);
        }
    }

    /// Follow the dirty organisms; false if the index can't (rebuild it).
    fn bring_forward(&mut self, g: &Game) -> bool {
        let w = &g.world;
        let Some(idx) = self.idx.as_mut() else { return false };
        let dirty = std::mem::take(&mut self.dirty);
        let index = |id: u64| w.organism_index(id).map(|i| i as u32);
        self.steering.extend(dirty.steered.iter().filter_map(|&id| index(id)));
        let mut changed: Vec<u32> = self.steering.iter().copied().collect();
        changed.extend(dirty.orgs.iter().filter_map(|&id| index(id)));
        for p in dirty.owners {
            changed.extend_from_slice(idx.owned(p));
            self.own.remove(&p);
        }
        changed.sort_unstable();
        changed.dedup();
        for &i in &changed {
            if !idx.update(i, &w.organisms[i as usize]) {
                return false;
            }
            self.near.forget(i);
            let owner = idx.slots[i as usize].owner;
            if let Some(list) = self.own.get_mut(&owner) {
                if let Ok(k) = idx.owned(owner).binary_search(&i) {
                    list[k] = None;
                }
            }
        }
        idx.refresh(g);
        if idx.board.names != g.names_changed {
            idx.board = Board::new(g);
        }
        true
    }
}

impl Game {
    /// What a command may change in the states: called before it is applied
    /// (a directed mating also reports its pair where it runs). Organisms a
    /// command names, its player's organisms after a group order; reads and
    /// view changes change no organism (views are read for every state).
    pub(super) fn note_command(&mut self, player: PlayerId, msg: &ClientMsg) {
        let c = &mut self.state_cache;
        if !c.valid() {
            return;
        }
        match msg {
            ClientMsg::Possess { organism }
            | ClientMsg::OrderMove { organism, .. }
            | ClientMsg::Release { organism } => c.steer(*organism),
            ClientMsg::Migrate { .. }
            | ClientMsg::ClearMigration { .. }
            | ClientMsg::ClaimTerritory { .. }
            | ClientMsg::AbandonTerritory { .. }
            | ClientMsg::Rally { .. }
            | ClientMsg::CancelRally { .. }
            | ClientMsg::SplitGroup { .. }
            | ClientMsg::MergeGroups { .. } => c.touch_owner(player),
            ClientMsg::RequestMating { mine, partner, .. } => {
                c.touch(*mine);
                c.touch(*partner);
            }
            ClientMsg::CreateListing { organism, .. } | ClientMsg::CreateAuction { organism, .. } => c.touch(*organism),
            ClientMsg::BuyListing { mine, .. } | ClientMsg::Bid { mine, .. } => c.touch(*mine),
            // Offers, listings, balances and escrow are read for every
            // state; a mating they run reports its pair itself.
            ClientMsg::RespondMating { .. }
            | ClientMsg::CompleteMating { .. }
            | ClientMsg::CancelMating { .. }
            | ClientMsg::WithdrawListing { .. } => {}
            ClientMsg::SetView { .. }
            | ClientMsg::ClearView
            | ClientMsg::PreviewMating { .. }
            | ClientMsg::BrowseMarket(_)
            | ClientMsg::Ledger { .. }
            | ClientMsg::Resync
            | ClientMsg::WatchSeason { .. } => {}
        }
    }

    /// One of the player's own organisms as sent in a state; a possessed one
    /// also carries its walking speed for the client's prediction (its
    /// `order` comes from [`OwnOrganism::from_org`]).
    pub(super) fn own_view(&self, o: &Organism, tpd: u32) -> OwnOrganism {
        let mut v = OwnOrganism::from_org(o, tpd);
        if o.possessed_by.is_some() {
            // An order sprints with energy to spare (`World::decide`).
            v.speed = Some(self.world.move_speed(o, o.needs.energy > 40.0));
        }
        v
    }

    /// The state message for one connection: the player's organisms in full,
    /// and other organisms inside the view window only. Builds a
    /// [`StateIndex`] for this one message; [`Game::states`] shares one
    /// between every connection.
    pub fn state_for(&self, conn: ConnId) -> Option<StateView> {
        self.conns.get(&conn)?;
        self.state_with(conn, &StateIndex::new(self), None)
    }

    /// [`Game::state_for`] from a prebuilt index: the cost is the size of
    /// the message (own organisms, the view window's cells, the player's
    /// offers), not the size of the world. With `cache`, organism views
    /// come from it (made and kept there when missing).
    fn state_with(&self, conn: ConnId, idx: &StateIndex, cache: Option<Caches<'_>>) -> Option<StateView> {
        let c = self.conns.get(&conn)?;
        let player = c.player;
        let tpd = self.world.config.ticks_per_day;
        let orgs = &self.world.organisms;
        let mine = idx.owned(player);
        let (near, own_views) = cache.map_or((None, None), |c| (Some(c.near), c.own));
        let own_view = |i: u32| self.own_view(&orgs[i as usize], tpd);
        let own = match own_views {
            Some(views) => {
                let list = views.entry(player).or_insert_with(|| vec![None; mine.len()]);
                mine.iter().zip(list.iter_mut()).map(|(&i, v)| v.get_or_insert_with(|| own_view(i)).clone()).collect()
            }
            None => mine.iter().map(|&i| own_view(i)).collect(),
        };
        let view = c.view.unwrap_or_else(|| {
            // Centered on the possessed organism (the first, in organism
            // order), else on the lineage.
            let possessed = mine.iter().map(|&i| &orgs[i as usize]).find(|o| o.possessed_by == Some(player));
            let center = possessed.map(|o| o.pos).unwrap_or_else(|| self.lineage_center_of(player, mine));
            ViewWindow { x: center.x, y: center.y, radius: self.cfg.default_view_radius }
        });
        let picked = idx.nearby(&view, player, self.cfg.max_nearby);
        let near_view = |i: u32| NearbyOrganism::from_org(&orgs[i as usize]);
        let nearby: Vec<NearbyOrganism> = match near {
            Some(near) => picked.into_iter().map(|(_, i)| near.get(i, || near_view(i))).collect(),
            None => picked.into_iter().map(|(_, i)| near_view(i)).collect(),
        };
        let ids = idx.offers.get(&player).map_or(&[][..], Vec::as_slice);
        let offers = ids.iter().map(|&id| self.offer_view(id)).collect();
        let escrow = ids.iter().map(|id| &self.offers[id]).filter(|o| o.from == player).map(|o| o.escrow).sum::<u64>()
            + idx.bids.get(&player).copied().unwrap_or(0);
        let leaderboard = self.state_leaderboard(&idx.board, player, &nearby);
        Some(StateView {
            clock: idx.clock.clone(),
            weather: idx.weather.clone(),
            player,
            lineage: self.lineage_of(player),
            slots_left: self.world.mating_slots_left(player),
            population: self.owner_population(player),
            migration: self.world.migration_order(player, 0).map(|p| [p.x, p.y]),
            groups: self
                .world
                .groups_of(player, mine.iter().map(|&i| &orgs[i as usize]))
                .iter()
                .map(GroupView::from)
                .collect(),
            view,
            organisms: own,
            nearby,
            leaderboard,
            offers,
            balance: self.balance(player),
            escrow,
        })
    }

    /// The leaderboard as sent in a state: the whole board when it has at
    /// most `state_leaderboard_rows` rows, else the top rows plus the
    /// player's own and those of the owners of the organisms in view (whose
    /// names the client shows), in rank order. `GET /api/leaderboard` has
    /// every row.
    fn state_leaderboard(&self, board: &Board, player: PlayerId, nearby: &[NearbyOrganism]) -> Leaderboard {
        let full = &board.full;
        let top = self.cfg.state_leaderboard_rows;
        if full.rows.len() <= top {
            return full.clone();
        }
        let mut owners: Vec<PlayerId> = nearby.iter().map(|n| n.owner).collect();
        owners.push(player);
        // Neighbors come in organism order, so owners in runs.
        owners.dedup();
        owners.sort_unstable();
        owners.dedup();
        let mut extra: Vec<usize> = owners.into_iter().filter_map(|p| board.rank(p)).filter(|&r| r >= top).collect();
        extra.sort_unstable();
        let mut rows = Vec::with_capacity(top + extra.len());
        rows.extend_from_slice(&full.rows[..top]);
        rows.extend(extra.into_iter().map(|r| full.rows[r].clone()));
        Leaderboard { day: full.day, breeding_adults: full.breeding_adults, rows }
    }

    /// A state message for every connection. Indexes the world afresh (a
    /// push follows a tick) and keeps the index for the sub-steps.
    pub fn states(&mut self) -> Outbox {
        if self.conns.is_empty() {
            return Vec::new();
        }
        let mut cache = std::mem::take(&mut self.state_cache);
        cache.rebuild(self);
        let StateCache { idx, near, own, steering, .. } = &mut cache;
        let idx = idx.as_ref().expect("just built");
        // Own views are kept for the players who steer: they get the next
        // sub-steps' states.
        let steer: BTreeSet<PlayerId> = steering.iter().map(|&i| idx.slots[i as usize].owner).collect();
        let out = self.build(idx, Some((near, own, &steer)), |_| true);
        self.state_cache = cache;
        out
    }

    /// States for the connections of the players `to` takes, from `idx` and
    /// (with `cache`) the views kept there, own views only for the players
    /// in its set.
    fn build(
        &self,
        idx: &StateIndex,
        mut cache: Option<(&mut ViewCache<NearbyOrganism>, &mut OwnViews, &BTreeSet<PlayerId>)>,
        to: impl Fn(PlayerId) -> bool,
    ) -> Outbox {
        self.conns
            .iter()
            .filter(|(_, c)| to(c.player))
            .filter_map(|(&c, conn)| {
                let caches = cache
                    .as_mut()
                    .map(|(near, own, keep)| Caches { near, own: keep.contains(&conn.player).then_some(&mut **own) });
                let s = self.state_with(c, idx, caches)?;
                Some((Target::Conn(c), ServerMsg::State(Box::new(s))))
            })
            .collect()
    }

    /// One possession sub-step between ticks (`World::substep`): possessed
    /// organisms following an order move a fraction of a tick's way, and the
    /// connections of the players who possess one that moved get a fresh
    /// state, so steering feels responsive at the slow world clock. Everyone
    /// else hears nothing until the next state push.
    pub fn substep(&mut self) -> Outbox {
        let moved = self.world.substep();
        if moved.is_empty() {
            return Vec::new();
        }
        let players: BTreeSet<PlayerId> =
            moved.iter().filter_map(|&id| self.world.organism(id).and_then(|o| o.possessed_by)).collect();
        if !self.conns.values().any(|c| players.contains(&c.player)) {
            return Vec::new();
        }
        let mut cache = std::mem::take(&mut self.state_cache);
        cache.prepare(self);
        let StateCache { idx, near, own, .. } = &mut cache;
        let idx = idx.as_ref().expect("prepared");
        let out = self.build(idx, Some((near, own, &players)), |p| players.contains(&p));
        self.state_cache = cache;
        out
    }
}

/// The organism views a state build reads and fills.
struct Caches<'a> {
    near: &'a mut ViewCache<NearbyOrganism>,
    own: Option<&'a mut OwnViews>,
}

/// Own organism views of players who steer, aligned with
/// `StateIndex::owned` (`None`: to be made again).
type OwnViews = HashMap<PlayerId, Vec<Option<OwnOrganism>>>;

#[cfg(test)]
mod tests;
