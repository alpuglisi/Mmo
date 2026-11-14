//! Game state around the [`World`]: player names, founded lineages,
//! directed-mating offers, connected views. Pure logic with no channels or
//! I/O, so command routing and the consent flow are unit-testable; the world
//! thread ([`crate::runner`]) owns a `Game` and moves its output to sockets.
//!
//! Every method that changes the world is called by the world thread
//! between ticks, in the order requests arrived.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use evo_sim::chronicle::{AchievementKind, Catastrophe, CatastropheKind};
use evo_sim::design::DesignError;
use evo_sim::math::Vec2;
use evo_sim::organism::{DeathCause, Sex};
use evo_sim::presets::{self, REFERENCE_SIZE};
use evo_sim::terrain::Season;
use evo_sim::world::commands::{CommandError, DirectedMating, MatingPreview, MEETING_DISTANCE};
use evo_sim::world::groups::MAX_GROUPS;
use evo_sim::{FounderDesign, Gene, PlayerId, World, WorldConfig, WILD};
use serde::{Deserialize, Serialize};

use crate::design::gene_key;
use crate::economy::{self, Bid, Economy, Insufficient, Listing};
use crate::league::{self, CurrentSeason, SeasonRecord, SeasonTracker};
use crate::protocol::*;
mod offline;
pub use offline::{OfflineTracker, Spell, DEFAULT_PROTECTION_HOURS, DEFAULT_PROTECTION_TICKS};

use crate::store::{AuctionRecord, BalanceRecord, DailyRecord, LedgerRecord, LegacyEntry, LineageEntry, TradeRecord};

/// Daily records kept in memory for the genetics dashboard.
pub const HISTORY_DAYS: usize = 365;
/// Alerts kept per player.
pub const MAX_ALERTS: usize = 50;
/// Thirst or hunger at which a possessed organism raises a `need` alert.
pub const NEED_ALERT: f32 = 75.0;
const POPULATION_MILESTONES: [usize; 7] = [25, 50, 100, 250, 500, 1000, 1500];
const SHARE_MILESTONES: [f64; 5] = [0.01, 0.05, 0.1, 0.25, 0.5];
/// Layout version of [`SavedGame`]: 2 added the economy (balances, ledger,
/// listings, escrow on offers; older saved games restore with the balances
/// kept by the store instead), 3 the ranked season under way and the last
/// finished one (older saved games start tracking at their next day), 4
/// auctions (with their highest bids) among the listings (versions 2 and 3
/// read as having none), 5 the same state saved smaller (the JSON deflated,
/// the world snapshot's tiles apart: `crate::snapshot`; version 4 saved
/// games still restore).
pub const SAVED_GAME_VERSION: u32 = 5;
/// Ledger entries returned by the `ledger` command unless asked otherwise.
const LEDGER_DEFAULT: usize = 50;
/// Genes shown for a listed organism.
const LISTING_TRAITS: [Gene; 14] = [
    Gene::BodySize,
    Gene::LegLength,
    Gene::Muscle,
    Gene::Fur,
    Gene::Scales,
    Gene::Armor,
    Gene::Wings,
    Gene::Swimming,
    Gene::Climbing,
    Gene::Burrowing,
    Gene::Camouflage,
    Gene::Coloration,
    Gene::Toxins,
    Gene::Carnivory,
];

pub type ConnId = u64;

/// Where an outgoing message goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// One WebSocket connection.
    Conn(ConnId),
    /// Every connection of a player (events such as a mating request).
    Player(PlayerId),
}

pub type Outbox = Vec<(Target, ServerMsg)>;

/// Something worth a push notification to a player (`crate::webpush`),
/// drained by the world thread with [`Game::take_notices`].
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub player: PlayerId,
    pub kind: PushKind,
    pub text: String,
    pub organism: Option<OrgId>,
}

/// Records for the store, drained by the world thread.
#[derive(Clone, Debug, PartialEq)]
pub enum PersistEvent {
    Daily(DailyRecord),
    Trade(TradeRecord),
    /// A player's renown balance changed.
    Balance(BalanceRecord),
    Ledger(LedgerRecord),
    /// An auction closed (settled, unsold, withdrawn or ended by a death).
    Auction(AuctionRecord),
    /// A ranked season ended (`crate::league`); replaces a stored record of
    /// the same world and season.
    Season(Box<SeasonRecord>),
    /// A saved game (`crate::snapshot`), sent by the world thread.
    Snapshot(SnapshotBlob),
}

/// Saved-game bytes and the terrain they name; `Debug` shows only sizes.
#[derive(Clone, PartialEq)]
pub struct SnapshotBlob {
    pub tick: u64,
    pub day: u64,
    pub bytes: Vec<u8>,
    /// The world's static terrain, which the saved game names by hash and
    /// the store keeps once per world (shared by every save of it).
    pub terrain: std::sync::Arc<TerrainBlob>,
}

impl std::fmt::Debug for SnapshotBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SnapshotBlob {{ tick: {}, {} bytes, terrain {:016x} }}",
            self.tick,
            self.bytes.len(),
            self.terrain.hash
        )
    }
}

/// A world's static terrain (`evo_sim::terrain::Terrain::encode`), stored
/// once per world id and content hash.
#[derive(Clone, PartialEq)]
pub struct TerrainBlob {
    pub world_id: String,
    pub hash: u64,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for TerrainBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TerrainBlob {{ world {}, {:016x}, {} bytes }}", self.world_id, self.hash, self.bytes.len())
    }
}

#[derive(Clone, Debug)]
pub struct GameConfig {
    /// Half-side of the default view window, in tiles.
    pub default_view_radius: f32,
    pub max_view_radius: f32,
    /// Most foreign organisms sent per state message (nearest first).
    pub max_nearby: usize,
    /// Leaderboard rows in a state message: a longer board is cut to its
    /// top rows plus the player's own and those of the owners of the
    /// organisms in view (`GET /api/leaderboard` has every row).
    pub state_leaderboard_rows: usize,
    /// Most open offers one player may have made at once.
    pub max_open_offers: usize,
    /// Length of a ranked season in simulated days (`crate::league`). A
    /// change applies from the next season.
    pub season_days: u64,
    /// Longest offline protection after a logout, in ticks; 0 turns it off
    /// (`game::offline`).
    pub offline_protection_ticks: u64,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            default_view_radius: 16.0,
            max_view_radius: 32.0,
            max_nearby: 256,
            state_leaderboard_rows: 50,
            max_open_offers: 8,
            season_days: league::DEFAULT_SEASON_DAYS,
            offline_protection_ticks: DEFAULT_PROTECTION_TICKS,
        }
    }
}

/// Map size and ecosystem of a new world.
#[derive(Clone, Debug)]
pub struct WorldSettings {
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    /// Found the wild ecosystem (voles, antelope, newts, foxes).
    pub wild: bool,
    /// Short years make seasons (and slot resets) come quickly in tests.
    pub days_per_year: u32,
    /// Level of detail (`WorldConfig::lod`): full agents only around the
    /// view windows of connected players and possessed organisms; elsewhere,
    /// offline players' lineages included, statistical cohorts.
    pub lod: bool,
    /// Random catastrophes (`ScoringConfig::catastrophes`; never in the
    /// first year). On by default in the game.
    pub catastrophes: bool,
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            seed: 1,
            width: 128,
            height: 128,
            wild: true,
            days_per_year: WorldConfig::default().days_per_year,
            lod: false,
            catastrophes: true,
        }
    }
}

/// A world with the wild ecosystem scaled to its area (as
/// `presets::standard_world` does) and no player lineages.
pub fn build_world(s: &WorldSettings) -> World {
    let config = world_config(s);
    let area = (s.width * s.height) as f32 / (REFERENCE_SIZE * REFERENCE_SIZE) as f32;
    let mut world = World::new(config);
    if s.wild {
        for (owner, design) in presets::wild_designs() {
            let n = ((design.founders as f32 * area).round() as u32).max(4);
            world.found_lineage_unchecked(owner, &design.founders(n));
        }
    }
    world
}

/// The configuration [`build_world`] uses (also checked against a restored
/// snapshot's).
pub fn world_config(s: &WorldSettings) -> WorldConfig {
    let mut config = WorldConfig {
        seed: s.seed,
        width: s.width,
        height: s.height,
        days_per_year: s.days_per_year.max(4),
        ..Default::default()
    };
    let area = (s.width * s.height) as f32 / (REFERENCE_SIZE * REFERENCE_SIZE) as f32;
    config.wild_cap = ((config.wild_cap as f32 * area) as usize).max(50);
    config.lineage_cap = ((config.lineage_cap as f32 * area.max(0.5)) as usize).max(50);
    config.lod.enabled = s.lod;
    config.scoring.catastrophes = s.catastrophes;
    config
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoundOk {
    pub lineage: u32,
    pub founders: Vec<OrgId>,
    pub cost: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FoundError {
    AlreadyFounded { lineage: u32 },
    Design(DesignError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Offer {
    id: u64,
    from: PlayerId,
    to: PlayerId,
    mine: OrgId,
    partner: OrgId,
    price: Option<u64>,
    status: OfferStatus,
    created_tick: u64,
    /// Bought from this marketplace listing (consent given by the listing).
    #[serde(default)]
    listing: Option<u64>,
    /// Renown taken from the requester, paid to the partner's owner when the
    /// mating runs, refunded on any other ending.
    #[serde(default)]
    escrow: u64,
}

struct Conn {
    player: PlayerId,
    view: Option<ViewWindow>,
}

/// Per-player progress for milestone alerts.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Progress {
    population: usize,
    generation: u32,
    share: f64,
    extinct: bool,
}

/// Why an organism is watched for its death.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Watch {
    Founder,
    Possessed,
}

pub struct Game {
    pub world: World,
    cfg: GameConfig,
    /// Identifies this world (made when the map is generated, kept in saved
    /// games) in the daily records, so a restarted world's records continue
    /// its own history.
    world_id: String,
    names: BTreeMap<PlayerId, String>,
    lineage_of: BTreeMap<PlayerId, u32>,
    offers: BTreeMap<u64, Offer>,
    next_offer: u64,
    conns: BTreeMap<ConnId, Conn>,
    last_legacy_day: Option<u64>,
    last_season: Season,
    persist: Vec<PersistEvent>,
    /// What is worth a push notification since the last
    /// [`Game::take_notices`] (`crate::webpush`).
    notices: Vec<Notice>,
    /// The daily records since the server started, oldest first.
    history: VecDeque<DailyRecord>,
    /// Cumulative birth and death counters at the last day boundary.
    prev_born: BTreeMap<u32, u64>,
    prev_deaths: BTreeMap<(u32, DeathCause), u64>,
    alerts: BTreeMap<PlayerId, VecDeque<AlertView>>,
    /// Organisms whose death is announced to their owner.
    watched: BTreeMap<OrgId, (PlayerId, Watch)>,
    progress: BTreeMap<PlayerId, Progress>,
    /// Last day a `need` alert was raised per organism.
    need_alerted: BTreeMap<OrgId, u64>,
    /// Players whose view windows are the world's level-of-detail focus.
    focused: BTreeSet<PlayerId>,
    /// Chronicle entries already announced: achievements, eras, and per
    /// catastrophe whether its start and end were.
    seen_achievements: usize,
    seen_eras: usize,
    seen_catastrophes: Vec<(bool, bool)>,
    /// Renown balances, ledger and marketplace listings.
    economy: Economy,
    /// Whether the economy came from a saved game or the store (else it
    /// starts empty).
    economy_restored: bool,
    /// The ranked season under way.
    season: SeasonTracker,
    /// The last finished ranked season, kept so a restart can write it
    /// again if the store missed it.
    last_season_record: Option<SeasonRecord>,
    /// Offline players' protection spells (`game::offline`).
    offline: OfflineTracker,
}

/// The game-side state saved with each world snapshot (`crate::snapshot`):
/// what isn't in the `World` and can't be derived from it. Connections,
/// views and the in-memory daily history are not saved (players log in
/// again; the history is reloaded from the store).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedGame {
    /// [`SAVED_GAME_VERSION`] when written; 0 in saved games from before
    /// versions (layout 1).
    version: u32,
    /// Empty in saved games from before world ids (a new one is made).
    world_id: String,
    lineage_of: BTreeMap<PlayerId, u32>,
    /// The main groups' migration orders, for reading only: the world
    /// snapshot holds every group's orders (since world snapshot version 10;
    /// earlier saves don't restore).
    migrations: BTreeMap<PlayerId, Vec2>,
    offers: BTreeMap<u64, Offer>,
    next_offer: u64,
    prev_born: BTreeMap<u32, u64>,
    /// `prev_deaths` as a list (JSON maps need string keys).
    prev_deaths: Vec<((u32, DeathCause), u64)>,
    alerts: BTreeMap<PlayerId, VecDeque<AlertView>>,
    watched: BTreeMap<OrgId, (PlayerId, Watch)>,
    progress: BTreeMap<PlayerId, Progress>,
    need_alerted: BTreeMap<OrgId, u64>,
    focused: BTreeSet<PlayerId>,
    seen_achievements: usize,
    seen_eras: usize,
    seen_catastrophes: Vec<(bool, bool)>,
    /// Version 2 on.
    economy: Option<Economy>,
    /// Version 3 on: the ranked season under way and the last one finished.
    season: Option<SeasonTracker>,
    last_season_record: Option<SeasonRecord>,
    /// Offline protection spells (absent in older saves: every player with
    /// a lineage starts one at the restore).
    offline: Option<OfflineTracker>,
}

impl SavedGame {
    /// The world that wrote it (empty before world ids), which keys its
    /// stored terrain.
    pub fn world_id(&self) -> &str {
        &self.world_id
    }
}

/// Refuse a price above [`economy::MAX_PRICE`].
fn check_price(price: u64) -> Result<(), ErrorInfo> {
    if price > economy::MAX_PRICE {
        return Err(ErrorInfo::new("bad_price", format!("prices go up to {} renown", economy::MAX_PRICE)));
    }
    Ok(())
}

/// How a fresh-start refund names the offer of a discarded world (the
/// trailing comma keeps offer #1 of a world from matching #10, or a world id
/// from matching a longer one).
fn discarded_mark(world: &str, offer: u64) -> String {
    format!("offer #{offer} of world {world},")
}

/// How a fresh-start refund names an auction of a discarded world.
fn discarded_bid_mark(world: &str, listing: u64) -> String {
    format!("auction #{listing} of world {world},")
}

/// A `browse_market` cursor: the sort key and id of the last listing of a
/// page, `"<key>.<id>"`.
fn cursor_text(key: (u64, u64)) -> String {
    format!("{}.{}", key.0, u64::MAX - key.1)
}

fn parse_cursor(c: &str) -> Result<(u64, u64), ErrorInfo> {
    let bad = || ErrorInfo::new("bad_cursor", "cursor must be a next_cursor from browse_market");
    let (a, b) = c.split_once('.').ok_or_else(bad)?;
    let (a, b): (u64, u64) = (a.parse().map_err(|_| bad())?, b.parse().map_err(|_| bad())?);
    Ok((a, u64::MAX - b))
}

/// Where a listing goes in a `browse_market` order: ascending by (key,
/// newest first). `preview` is the buyer's compatibility, for `fertility`.
fn market_key(sort: MarketSort, l: &Listing, preview: Option<&MatingPreviewView>, mine: bool) -> (u64, u64) {
    let newest = u64::MAX - l.id;
    let key = match sort {
        MarketSort::Newest => 0,
        MarketSort::PriceAsc => l.price(),
        MarketSort::PriceDesc => u64::MAX - l.price(),
        MarketSort::Ending => l.expires_tick.unwrap_or(u64::MAX),
        MarketSort::Fertility if !mine => 0,
        MarketSort::Fertility => match preview {
            Some(p) if p.compatible => 1_000_000 - (p.fertility.clamp(0.0, 1.0) * 1e6) as u64,
            Some(_) => 2_000_000,
            None => 3_000_000,
        },
    };
    (key, newest)
}

fn finite(v: &[f32]) -> Result<(), ErrorInfo> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(ErrorInfo::new("bad_coordinates", "coordinates must be finite numbers"))
    }
}

impl Game {
    pub fn new(world: World, cfg: GameConfig) -> Self {
        let last_season = world.season();
        let last_legacy_day = world.latest_legacy().map(|l| l.day);
        // The first season starts with the next daily snapshot.
        let first_day = last_legacy_day.map_or(world.day(), |d| d + 1);
        let season = SeasonTracker::new(1, first_day, cfg.season_days);
        Self {
            world,
            cfg,
            world_id: format!("{:016x}", rand::random::<u64>()),
            names: BTreeMap::new(),
            lineage_of: BTreeMap::new(),
            offers: BTreeMap::new(),
            next_offer: 1,
            conns: BTreeMap::new(),
            last_legacy_day,
            last_season,
            persist: Vec::new(),
            notices: Vec::new(),
            history: VecDeque::new(),
            prev_born: BTreeMap::new(),
            prev_deaths: BTreeMap::new(),
            alerts: BTreeMap::new(),
            watched: BTreeMap::new(),
            progress: BTreeMap::new(),
            need_alerted: BTreeMap::new(),
            focused: BTreeSet::new(),
            seen_achievements: 0,
            seen_eras: 0,
            seen_catastrophes: Vec::new(),
            economy: Economy::new(),
            economy_restored: false,
            season,
            last_season_record: None,
            offline: OfflineTracker::default(),
        }
    }

    /// The game-side state to save with a world snapshot.
    pub fn saved(&self) -> SavedGame {
        SavedGame {
            version: SAVED_GAME_VERSION,
            world_id: self.world_id.clone(),
            lineage_of: self.lineage_of.clone(),
            migrations: (self.lineage_of.keys())
                .filter_map(|&p| self.world.migration_order(p, 0).map(|t| (p, t)))
                .collect(),
            offers: self.offers.clone(),
            next_offer: self.next_offer,
            prev_born: self.prev_born.clone(),
            prev_deaths: self.prev_deaths.iter().map(|(&k, &v)| (k, v)).collect(),
            alerts: self.alerts.clone(),
            watched: self.watched.clone(),
            progress: self.progress.clone(),
            need_alerted: self.need_alerted.clone(),
            focused: self.focused.clone(),
            seen_achievements: self.seen_achievements,
            seen_eras: self.seen_eras,
            seen_catastrophes: self.seen_catastrophes.clone(),
            economy: Some(self.economy.clone()),
            season: Some(self.season.clone()),
            last_season_record: self.last_season_record.clone(),
            offline: Some(self.offline.clone()),
        }
    }

    /// A game continuing a restored world: the saved game-side state, and
    /// the daily records of this world's past (oldest first) for the
    /// dashboard. Records after the world's last legacy day (written after
    /// the snapshot) are dropped, and a day number going backwards (a
    /// replayed day, or an earlier world's records) drops the records it
    /// replaces.
    pub fn resume(world: World, cfg: GameConfig, saved: SavedGame, history: Vec<DailyRecord>) -> Self {
        let mut g = Game::new(world, cfg);
        let last_day = g.world.latest_legacy().map(|l| l.day);
        let mut kept: VecDeque<DailyRecord> = VecDeque::new();
        for r in history.into_iter().filter(|r| last_day.is_some_and(|d| r.day <= d)) {
            while kept.back().is_some_and(|k| k.day >= r.day) {
                kept.pop_back();
            }
            kept.push_back(r);
        }
        while kept.len() > HISTORY_DAYS {
            kept.pop_front();
        }
        g.history = kept;
        if !saved.world_id.is_empty() {
            g.world_id = saved.world_id;
        }
        g.lineage_of = saved.lineage_of;
        // A player's lineage is in the world even if the saved table lost it.
        for (k, l) in g.world.lineages.iter().enumerate() {
            if l.owner != WILD {
                g.lineage_of.entry(l.owner).or_insert(k as u32);
            }
        }
        g.offers = saved.offers;
        g.next_offer = saved.next_offer.max(g.offers.keys().max().map_or(1, |k| k + 1));
        g.prev_born = saved.prev_born;
        g.prev_deaths = saved.prev_deaths.into_iter().collect();
        g.alerts = saved.alerts;
        g.watched = saved.watched;
        g.progress = saved.progress;
        g.need_alerted = saved.need_alerted;
        g.focused = saved.focused;
        g.seen_achievements = saved.seen_achievements;
        g.seen_eras = saved.seen_eras;
        g.seen_catastrophes = saved.seen_catastrophes;
        match saved.economy {
            Some(e) if saved.version >= 2 => {
                g.economy = e;
                g.economy_restored = true;
            }
            // An older layout: no escrow was taken on its offers.
            _ => {
                for o in g.offers.values_mut() {
                    o.escrow = 0;
                }
            }
        }
        // A saved game from before ranked seasons starts season 1 at the
        // next day (made by `Game::new`).
        if let Some(t) = saved.season.filter(|t| t.season > 0) {
            g.season = t;
        }
        // Written again in case the store missed it (a replace, never a
        // second record).
        if let Some(r) = saved.last_season_record {
            g.persist.push(PersistEvent::Season(Box::new(r.clone())));
            g.last_season_record = Some(r);
        }
        // Nobody is connected: drop the level-of-detail focus of views, and
        // every lineage is unattended.
        g.sync_focus();
        g.offline_resume(saved.offline);
        g
    }

    /// Whether the economy came from the saved game (or was seeded).
    pub fn economy_restored(&self) -> bool {
        self.economy_restored
    }

    /// Start the economy from what the store kept (balances and recent
    /// ledger entries, oldest first): a new world, or a saved game from
    /// before the economy. Players with a stored balance keep it and don't
    /// get the founding grant again.
    pub fn seed_economy(&mut self, balances: &[(PlayerId, u64)], ledger: Vec<LedgerEntry>) {
        self.economy = Economy::from_store(balances, ledger);
        self.economy_restored = true;
    }

    /// Refund, in this new world, the escrow still held on open agreements
    /// of a discarded one (`--fresh`): the newest saved game `old` of the
    /// previous world, and recent ledger records from the store. Call after
    /// [`Game::seed_economy`] with the stored balances, which already had the
    /// escrow taken out.
    ///
    /// An agreement counts as held when it is open in the saved game with
    /// escrow, or when the store has an `escrow` entry of the old world
    /// written after the saved game (its sequence number is at least the
    /// saved economy's next one). It is skipped when the store has a `sale`
    /// or `refund` of the old world written after the saved game (it settled
    /// before the server stopped), or a refund made by an earlier fresh start
    /// (recognised by its text, so starting fresh twice from the same saved
    /// game refunds once). Refund entries name the old offer in their text
    /// and carry no offer id (ids restart in the new world). Returns the
    /// refunds as (player, amount).
    pub fn refund_discarded(&mut self, old: &SavedGame, ledger: &[LedgerRecord]) -> Vec<(PlayerId, u64)> {
        let Some(economy) = old.economy.as_ref().filter(|_| old.version >= 2 && !old.world_id.is_empty()) else {
            // Older layouts took no escrow.
            return Vec::new();
        };
        let world = old.world_id.as_str();
        let after = economy.next_seq;
        let mut held: BTreeMap<u64, (PlayerId, u64)> = old
            .offers
            .values()
            .filter(|o| o.status.is_open() && o.escrow > 0)
            .map(|o| (o.id, (o.from, o.escrow)))
            .collect();
        let later = |r: &&LedgerRecord| r.world_id == world && r.entry.seq >= after;
        for r in ledger.iter().filter(later) {
            if let (LedgerKind::Escrow, Some(id), true) = (r.entry.kind, r.entry.offer, r.entry.delta < 0) {
                held.entry(id).or_insert((r.entry.player, r.entry.delta.unsigned_abs()));
            }
        }
        for r in ledger.iter().filter(later) {
            if let (LedgerKind::Sale | LedgerKind::Refund, Some(id)) = (r.entry.kind, r.entry.offer) {
                held.remove(&id);
            }
        }
        held.retain(|&id, _| {
            let mark = discarded_mark(world, id);
            !ledger.iter().any(|r| r.entry.kind == LedgerKind::Refund && r.entry.text.contains(&mark))
        });
        // Auction bids: the highest bids in the saved game, then the bids
        // (escrow with a listing and no offer) and their refunds written
        // after it, per bidder and auction.
        let mut bids: BTreeMap<(u64, PlayerId), i128> = BTreeMap::new();
        for l in economy.listings.values() {
            if let Some(b) = l.auction.as_ref().and_then(|a| a.high.as_ref()) {
                *bids.entry((l.id, b.bidder)).or_default() += i128::from(b.amount);
            }
        }
        for r in ledger.iter().filter(later) {
            let e = &r.entry;
            if let (Some(l), None, LedgerKind::Escrow | LedgerKind::Refund) = (e.listing, e.offer, e.kind) {
                *bids.entry((l, e.player)).or_default() -= i128::from(e.delta);
            }
        }
        bids.retain(|&(l, _), &mut amount| {
            let mark = discarded_bid_mark(world, l);
            amount > 0 && !ledger.iter().any(|r| r.entry.kind == LedgerKind::Refund && r.entry.text.contains(&mark))
        });
        let mut refunds = Vec::new();
        for (id, (player, amount)) in held {
            let Ok(amount_i) = i64::try_from(amount) else { continue };
            let text =
                format!("Refund: {amount} renown held on {} discarded by a fresh start", discarded_mark(world, id));
            if self.post(player, amount_i, LedgerKind::Refund, text, None, None).is_ok() {
                refunds.push((player, amount));
            }
        }
        for ((listing, player), amount) in bids {
            let (Ok(amount), Ok(amount_i)) = (u64::try_from(amount), i64::try_from(amount)) else { continue };
            let mark = discarded_bid_mark(world, listing);
            let text = format!("Refund: {amount} renown bid on {mark} discarded by a fresh start");
            if self.post(player, amount_i, LedgerKind::Refund, text, None, None).is_ok() {
                refunds.push((player, amount));
            }
        }
        refunds
    }

    /// A player's renown balance.
    pub fn balance(&self, player: PlayerId) -> u64 {
        self.economy.balance(player)
    }

    /// Renown of `player`'s held in escrow on open agreements and as the
    /// highest bid on auctions.
    pub fn escrow(&self, player: PlayerId) -> u64 {
        self.offers.values().filter(|o| o.from == player && o.status.is_open()).map(|o| o.escrow).sum::<u64>()
            + self.economy.bids_held(player)
    }

    /// Recent ledger entries of a player, newest first.
    pub fn ledger(&self, player: PlayerId, limit: usize) -> Vec<LedgerEntry> {
        self.economy.entries(player, limit)
    }

    /// Change a balance, record it in the ledger and queue both for the
    /// store. A debit larger than the balance changes nothing.
    fn post(
        &mut self,
        player: PlayerId,
        delta: i64,
        kind: LedgerKind,
        text: String,
        offer: Option<u64>,
        listing: Option<u64>,
    ) -> Result<LedgerEntry, Insufficient> {
        let (tick, day) = (self.world.tick(), self.world.day());
        let entry = self.economy.post(player, delta, kind, text, offer, listing, tick, day)?;
        self.persist.push(PersistEvent::Ledger(LedgerRecord { world_id: self.world_id.clone(), entry: entry.clone() }));
        self.persist.push(PersistEvent::Balance(BalanceRecord { player, balance: entry.balance, day }));
        Ok(entry)
    }

    pub fn set_name(&mut self, player: PlayerId, name: &str) {
        self.names.insert(player, name.to_string());
    }

    fn name(&self, player: PlayerId) -> String {
        if player == WILD {
            return "wild".into();
        }
        self.names.get(&player).cloned().unwrap_or_else(|| format!("player {player}"))
    }

    pub fn lineage_of(&self, player: PlayerId) -> Option<u32> {
        self.lineage_of.get(&player).copied()
    }

    /// Records for the store since the last call.
    pub fn take_persist(&mut self) -> Vec<PersistEvent> {
        std::mem::take(&mut self.persist)
    }

    /// What is worth a push notification since the last call.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    /// Found the player's lineage (one per player per world).
    pub fn found(&mut self, player: PlayerId, design: &FounderDesign) -> Result<FoundOk, FoundError> {
        if let Some(&lineage) = self.lineage_of.get(&player) {
            return Err(FoundError::AlreadyFounded { lineage });
        }
        let lineage = self.world.lineages.len() as u32;
        let founders = self.world.found_lineage(player, design).map_err(FoundError::Design)?;
        self.lineage_of.insert(player, lineage);
        for &id in &founders {
            self.watched.insert(id, (player, Watch::Founder));
        }
        if self.economy.granted.insert(player) {
            let text = format!("Founding grant for {}", design.name);
            let _ = self.post(player, economy::FOUNDING_GRANT as i64, LedgerKind::Grant, text, None, None);
        }
        Ok(FoundOk { lineage, founders, cost: design.point_cost() })
    }

    pub fn connect(&mut self, conn: ConnId, player: PlayerId, name: &str) -> Outbox {
        self.set_name(player, name);
        let first = !self.conns.values().any(|c| c.player == player);
        self.conns.insert(conn, Conn { player, view: None });
        let mut out = vec![(
            Target::Conn(conn),
            ServerMsg::Welcome { player, name: name.into(), lineage: self.lineage_of(player), proto: None },
        )];
        if first {
            self.offline_login(player, &mut out);
        }
        if let Some(s) = self.state_for(conn) {
            out.push((Target::Conn(conn), ServerMsg::State(Box::new(s))));
        }
        out
    }

    pub fn disconnect(&mut self, conn: ConnId) {
        if let Some(c) = self.conns.remove(&conn) {
            if !self.conns.values().any(|o| o.player == c.player) {
                self.offline_logout(c.player);
            }
        }
    }

    pub fn conn_player(&self, conn: ConnId) -> Option<PlayerId> {
        self.conns.get(&conn).map(|c| c.player)
    }

    /// Apply one client command and say what to send back.
    pub fn command(&mut self, conn: ConnId, player: PlayerId, id: Option<u64>, msg: ClientMsg) -> Outbox {
        let name = msg.name();
        let mut out = Outbox::new();
        let result = self.apply(conn, player, msg, &mut out);
        let reply = match result {
            Ok(data) => ServerMsg::ok(id, name, data),
            Err(e) => ServerMsg::err(id, name, e),
        };
        out.insert(0, (Target::Conn(conn), reply));
        out
    }

    fn apply(
        &mut self,
        conn: ConnId,
        player: PlayerId,
        msg: ClientMsg,
        out: &mut Outbox,
    ) -> Result<Option<serde_json::Value>, ErrorInfo> {
        let e = |err: CommandError| ErrorInfo::from(&err);
        match msg {
            ClientMsg::Possess { organism } => self.world.possess(player, organism).map(|_| None).map_err(e),
            ClientMsg::OrderMove { organism, x, y } => {
                finite(&[x, y])?;
                self.world.order_move(player, organism, Vec2::new(x, y)).map_err(e)?;
                Ok(None)
            }
            ClientMsg::Release { organism } => self.world.release(player, organism).map(|_| None).map_err(e),
            ClientMsg::Migrate { x, y, group } => {
                finite(&[x, y])?;
                let target = self.world.map.clamp(Vec2::new(x, y));
                match group {
                    None => self.world.command_migrate(player, target),
                    Some(g) => {
                        self.world.command_migrate_group(player, g, target).map_err(e)?;
                    }
                }
                Ok(Some(serde_json::json!({ "x": target.x, "y": target.y })))
            }
            ClientMsg::ClearMigration { group } => {
                match group {
                    None => self.world.clear_migration(player),
                    Some(g) if g < MAX_GROUPS => self.world.clear_migration_group(player, g),
                    Some(g) => return Err(e(CommandError::NoSuchGroup(g))),
                }
                Ok(None)
            }
            ClientMsg::ClaimTerritory { group, x, y, radius } => {
                finite(&[x, y, radius])?;
                let t = self.world.claim_territory(player, group, Vec2::new(x, y), radius).map_err(e)?;
                Ok(Some(serde_json::to_value(TerritoryView::from(t)).unwrap()))
            }
            ClientMsg::AbandonTerritory { group } => {
                self.world.abandon_territory(player, group).map_err(e)?;
                Ok(None)
            }
            ClientMsg::SplitGroup { group, organisms } => {
                if organisms.as_ref().is_some_and(|o| o.len() > MAX_SPLIT_ORGANISMS) {
                    return Err(ErrorInfo::new(
                        "too_many_organisms",
                        format!("a split names at most {MAX_SPLIT_ORGANISMS} organisms"),
                    ));
                }
                let out = self.world.split_group(player, group, organisms.as_deref()).map_err(e)?;
                Ok(Some(serde_json::json!({ "group": out.group, "moved": out.moved, "kept": out.kept })))
            }
            ClientMsg::MergeGroups { from, into } => {
                let moved = self.world.merge_groups(player, from, into).map_err(e)?;
                Ok(Some(serde_json::json!({ "moved": moved })))
            }
            ClientMsg::SetView { x, y, radius } => {
                finite(&[x, y, radius])?;
                let p = self.world.map.clamp(Vec2::new(x, y));
                let radius = radius.clamp(1.0, self.cfg.max_view_radius);
                let view = ViewWindow { x: p.x, y: p.y, radius };
                if let Some(c) = self.conns.get_mut(&conn) {
                    c.view = Some(view);
                }
                Ok(Some(serde_json::to_value(view).unwrap()))
            }
            ClientMsg::ClearView => {
                if let Some(c) = self.conns.get_mut(&conn) {
                    c.view = None;
                }
                Ok(None)
            }
            ClientMsg::RequestMating { mine, partner, price } => self.request_mating(player, mine, partner, price, out),
            ClientMsg::PreviewMating { mine, partner } => {
                Ok(Some(serde_json::to_value(self.preview_mating(player, mine, partner)?).unwrap()))
            }
            ClientMsg::RespondMating { request, accept } => self.respond_mating(player, request, accept, out),
            ClientMsg::CompleteMating { request } => {
                let offer = self.offer_for(request, player)?;
                if offer.status != OfferStatus::Accepted {
                    return Err(ErrorInfo::new("offer_closed", format!("offer {request} is {:?}", offer.status)));
                }
                match self.execute(request, out) {
                    Err(err) => Err(err),
                    Ok(o) => Ok(Some(serde_json::to_value(o).unwrap())),
                }
            }
            ClientMsg::CancelMating { request } => {
                let offer = self.offer_for(request, player)?;
                if !offer.status.is_open() {
                    return Err(ErrorInfo::new("offer_closed", format!("offer {request} is {:?}", offer.status)));
                }
                self.close(request, OfferStatus::Cancelled, None, None, out);
                Ok(None)
            }
            ClientMsg::CreateListing { organism, fee, expires_days } => {
                let l = self.create_listing(player, organism, fee, expires_days)?;
                Ok(Some(serde_json::json!({ "listing": self.listing_view(&l, None) })))
            }
            ClientMsg::WithdrawListing { listing } => {
                let l = match self.economy.listings.get(&listing) {
                    Some(l) if l.owner == player => l.clone(),
                    _ => return Err(ErrorInfo::new("unknown_listing", format!("no listing {listing} of yours"))),
                };
                if l.auction.as_ref().is_some_and(|a| a.high.is_some()) {
                    return Err(ErrorInfo::new("auction_has_bids", "an auction with a bid can't be withdrawn"));
                }
                self.economy.listings.remove(&listing);
                if l.auction.is_some() {
                    self.record_auction(&l, "withdrawn", None);
                }
                Ok(None)
            }
            ClientMsg::BuyListing { listing, mine } => self.buy_listing(player, listing, mine, out),
            ClientMsg::CreateAuction { organism, reserve, days, ticks } => {
                let l = self.create_auction(player, organism, reserve, days, ticks)?;
                Ok(Some(serde_json::json!({ "listing": self.listing_view(&l, None) })))
            }
            ClientMsg::Bid { listing, amount, mine } => self.bid(player, listing, amount, mine, out),
            ClientMsg::BrowseMarket(q) => Ok(Some(serde_json::to_value(self.market(player, &q)?).unwrap())),
            ClientMsg::Ledger { limit } => {
                let view = LedgerView {
                    balance: self.balance(player),
                    escrow: self.escrow(player),
                    entries: self.ledger(player, limit.unwrap_or(LEDGER_DEFAULT).min(economy::LEDGER_KEPT)),
                };
                Ok(Some(serde_json::to_value(view).unwrap()))
            }
            // The connection answers it (`http.rs`); nothing to do here.
            ClientMsg::Resync => Ok(None),
        }
    }

    fn offer_for(&self, request: u64, player: PlayerId) -> Result<&Offer, ErrorInfo> {
        self.offers
            .get(&request)
            .filter(|o| o.from == player || o.to == player)
            .ok_or_else(|| ErrorInfo::new("unknown_offer", format!("no offer {request} involving you")))
    }

    fn request_mating(
        &mut self,
        player: PlayerId,
        mine: OrgId,
        partner: OrgId,
        price: Option<u64>,
        out: &mut Outbox,
    ) -> Result<Option<serde_json::Value>, ErrorInfo> {
        let e = |err: CommandError| ErrorInfo::from(&err);
        if let Some(p) = price {
            check_price(p)?;
        }
        let a = self.world.organism(mine).ok_or(e(CommandError::NotFound(mine)))?;
        if a.owner != player {
            return Err(e(CommandError::NotOwned(mine)));
        }
        let b = self.world.organism(partner).ok_or(e(CommandError::NotFound(partner)))?;
        if b.owner == player {
            // Within the player's own organisms: no consent needed (and no
            // price). A pair that can't conceive would only use up a slot.
            if let Some(problem) = self.world.mating_preview(mine, partner).ok().as_ref().and_then(incompatibility) {
                return Err(problem);
            }
            let req = DirectedMating { player, a: mine, b: partner, partner_consent: false };
            let outcome = self.world.directed_mating(req).map_err(e)?;
            return Ok(Some(serde_json::to_value(MatingOutcome::from(outcome)).unwrap()));
        }
        let to = self.check_cross_pair(player, mine, partner)?;
        let price = price.filter(|&p| p > 0);
        if let Some(p) = price {
            self.check_funds(player, p)?;
        }
        let id = self.open_offer(player, to, mine, partner, price, OfferStatus::Pending, None)?;
        // Holds the requester's own female; the partner's waits for her
        // owner's acceptance (see `hold_consented_females`).
        self.hold_consented_females();
        let view = self.offer_view(id);
        let price = view.price.map_or(String::new(), |p| format!(" for {p} renown"));
        let text = format!("{} asks to mate their #{mine} with your #{partner}{price}.", view.from_name);
        self.notice(to, PushKind::Mating, text, Some(partner));
        out.push((Target::Player(to), ServerMsg::MatingRequest { offer: view.clone() }));
        Ok(Some(serde_json::json!({ "offer": view })))
    }

    /// Refuse a price or fee the player can't cover.
    fn check_funds(&self, player: PlayerId, price: u64) -> Result<(), ErrorInfo> {
        check_price(price)?;
        let balance = self.balance(player);
        if balance < price {
            return Err(ErrorInfo::new("insufficient_funds", format!("{price} renown needed, you have {balance}"))
                .with_detail(serde_json::json!({ "balance": balance, "needed": price })));
        }
        Ok(())
    }

    /// Record a new offer and take its price into escrow (checked with
    /// [`Game::check_funds`] beforehand). If the escrow can't be taken after
    /// all, no offer is made: an agreement never runs on an unpaid price.
    #[allow(clippy::too_many_arguments)]
    fn open_offer(
        &mut self,
        from: PlayerId,
        to: PlayerId,
        mine: OrgId,
        partner: OrgId,
        price: Option<u64>,
        status: OfferStatus,
        listing: Option<u64>,
    ) -> Result<u64, ErrorInfo> {
        let id = self.next_offer;
        self.next_offer += 1;
        let offer =
            Offer { id, from, to, mine, partner, price, status, created_tick: self.world.tick(), listing, escrow: 0 };
        self.offers.insert(id, offer);
        if let Some(p) = price.filter(|&p| p > 0) {
            let what = match listing {
                Some(l) => {
                    format!("Stud fee for listing #{l} (#{mine} × {}'s #{partner}) held in escrow", self.name(to))
                }
                None => format!("Price of offer #{id} (#{mine} × {}'s #{partner}) held in escrow", self.name(to)),
            };
            let debit = i64::try_from(p).ok().map(|p| -p);
            match debit.map(|d| self.post(from, d, LedgerKind::Escrow, what, Some(id), listing)) {
                Some(Ok(_)) => self.offers.get_mut(&id).unwrap().escrow = p,
                _ => {
                    self.offers.remove(&id);
                    let balance = self.balance(from);
                    return Err(ErrorInfo::new("insufficient_funds", format!("{p} renown needed, you have {balance}"))
                        .with_detail(serde_json::json!({ "balance": balance, "needed": p })));
                }
            }
        }
        Ok(id)
    }

    /// The checks before asking another player's consent (or buying a
    /// listing): ownership, maturity, sex, pregnancy, compatibility, slots
    /// and the open-offer limit. Returns the partner's owner.
    fn check_cross_pair(&self, player: PlayerId, mine: OrgId, partner: OrgId) -> Result<PlayerId, ErrorInfo> {
        let e = |err: CommandError| ErrorInfo::from(&err);
        let a = self.world.organism(mine).ok_or(e(CommandError::NotFound(mine)))?;
        if a.owner != player {
            return Err(e(CommandError::NotOwned(mine)));
        }
        let b = self.world.organism(partner).ok_or(e(CommandError::NotFound(partner)))?;
        let to = b.owner;
        // A pair that can't conceive would only use up a slot (and, between
        // players, the partner owner's time): refuse it with the reason.
        let incompatible = || self.world.mating_preview(mine, partner).ok().as_ref().and_then(incompatibility);
        if to == player {
            return Err(ErrorInfo::new("own_organism", "both organisms are yours: no consent or fee needed"));
        }
        if to == WILD {
            return Err(ErrorInfo::new("no_consent", "wild organisms have no owner to consent"));
        }
        // Refuse what can't work before bothering the other player (the
        // rules are checked again when the mating runs).
        for o in [a, b] {
            if !o.matured {
                return Err(e(CommandError::NotMature(o.id)));
            }
        }
        if a.sex == b.sex {
            return Err(e(CommandError::SameSex));
        }
        if [a, b].iter().any(|o| o.sex == evo_sim::organism::Sex::Female && o.pregnancy.is_some()) {
            return Err(e(CommandError::AlreadyPregnant));
        }
        if let Some(problem) = incompatible() {
            return Err(problem);
        }
        if self.world.mating_slots_left(player) == 0 {
            return Err(e(CommandError::NoSlotsLeft));
        }
        let open = self.offers.values().filter(|o| o.from == player && o.status.is_open()).count();
        if open >= self.cfg.max_open_offers {
            return Err(ErrorInfo::new("too_many_offers", format!("at most {} open offers", self.cfg.max_open_offers)));
        }
        Ok(to)
    }

    /// The checks before putting an organism on the marketplace (either
    /// kind): ownership, maturity, one listing per organism, the per-player
    /// limit, the price.
    fn check_listable(&self, player: PlayerId, organism: OrgId, price: u64) -> Result<(), ErrorInfo> {
        let e = |err: CommandError| ErrorInfo::from(&err);
        let o = self.world.organism(organism).ok_or(e(CommandError::NotFound(organism)))?;
        if o.owner != player {
            return Err(e(CommandError::NotOwned(organism)));
        }
        if !o.matured {
            return Err(e(CommandError::NotMature(organism)));
        }
        if price > economy::MAX_PRICE {
            return Err(ErrorInfo::new("bad_price", format!("prices go up to {} renown", economy::MAX_PRICE)));
        }
        if let Some(l) = self.economy.listing_of(organism) {
            return Err(ErrorInfo::new("already_listed", format!("#{organism} is already listing #{}", l.id)));
        }
        if self.economy.listings_of(player) >= economy::MAX_LISTINGS {
            return Err(ErrorInfo::new(
                "too_many_listings",
                format!("at most {} listings at once", economy::MAX_LISTINGS),
            ));
        }
        Ok(())
    }

    /// Put one of the player's adults on the marketplace.
    fn create_listing(
        &mut self,
        player: PlayerId,
        organism: OrgId,
        fee: u64,
        expires_days: Option<u32>,
    ) -> Result<Listing, ErrorInfo> {
        // A pregnant dam may be listed: buyers wait until she has given
        // birth, and the listing's hold keeps her from conceiving again.
        if let Some(d) = expires_days {
            if d == 0 || d > economy::MAX_LISTING_DAYS {
                return Err(ErrorInfo::new(
                    "bad_expiry",
                    format!("a listing lasts 1 to {} days", economy::MAX_LISTING_DAYS),
                ));
            }
        }
        self.check_listable(player, organism, fee)?;
        let tick = self.world.tick();
        let expires = expires_days.map(|d| tick + d as u64 * self.world.config.ticks_per_day as u64);
        let l = self.economy.add_listing(player, organism, fee, tick, expires);
        self.hold_consented_females();
        Ok(l)
    }

    /// Auction one mating with one of the player's adults, closing after
    /// `ticks` ticks or `days` days (default 1).
    fn create_auction(
        &mut self,
        player: PlayerId,
        organism: OrgId,
        reserve: u64,
        days: Option<u32>,
        ticks: Option<u64>,
    ) -> Result<Listing, ErrorInfo> {
        let tpd = self.world.config.ticks_per_day as u64;
        let longest = economy::MAX_AUCTION_DAYS as u64 * tpd;
        let duration = match (days, ticks) {
            (Some(_), Some(_)) => None,
            (None, Some(t)) => Some(t),
            (d, None) => Some(d.unwrap_or(1) as u64 * tpd),
        };
        let Some(duration) = duration.filter(|&t| t >= 1 && t <= longest) else {
            return Err(ErrorInfo::new(
                "bad_duration",
                format!(
                    "an auction lasts 1 to {} days (days) or 1 to {longest} ticks (ticks), not both",
                    economy::MAX_AUCTION_DAYS
                ),
            ));
        };
        self.check_listable(player, organism, reserve)?;
        // The winner's agreement must be able to run: a pregnant dam can't
        // be auctioned (the auction's hold keeps a listed dam from
        // conceiving meanwhile).
        if self.world.organism(organism).is_some_and(|o| o.sex == Sex::Female && o.pregnancy.is_some()) {
            return Err(ErrorInfo::from(&CommandError::AlreadyPregnant));
        }
        if self.offers.values().any(|o| o.status.is_open() && (o.mine == organism || o.partner == organism)) {
            return Err(ErrorInfo::new(
                "organism_busy",
                format!("#{organism} is already promised in an open agreement"),
            ));
        }
        let tick = self.world.tick();
        let l = self.economy.add_auction(player, organism, reserve, tick, tick + duration);
        self.hold_consented_females();
        Ok(l)
    }

    /// Bid on an auction for the player's adult `mine`: the amount into
    /// escrow (the difference when raising their own highest bid), the
    /// previous highest bidder refunded.
    fn bid(
        &mut self,
        player: PlayerId,
        listing: u64,
        amount: u64,
        mine: OrgId,
        out: &mut Outbox,
    ) -> Result<Option<serde_json::Value>, ErrorInfo> {
        let l = self
            .economy
            .listings
            .get(&listing)
            .cloned()
            .ok_or_else(|| ErrorInfo::new("unknown_listing", format!("no open listing {listing}")))?;
        let Some(auction) = l.auction.clone() else {
            return Err(ErrorInfo::new("not_auction", format!("listing {listing} has a fixed fee: buy it instead")));
        };
        if l.owner == player {
            return Err(ErrorInfo::new("own_listing", "you can't bid on your own auction"));
        }
        if l.expires_tick.is_some_and(|t| self.world.tick() >= t) {
            return Err(ErrorInfo::new("auction_closed", format!("auction {listing} has closed")));
        }
        check_price(amount)?;
        let min = auction.min_bid();
        if amount < min {
            return Err(ErrorInfo::new("bid_too_low", format!("the lowest bid now is {min} renown"))
                .with_detail(serde_json::json!({ "min_bid": min })));
        }
        let to = self.check_cross_pair(player, mine, l.organism)?;
        if to != l.owner {
            return Err(ErrorInfo::new("unknown_listing", format!("listing {listing} is no longer valid")));
        }
        if self.offers.values().any(|o| o.status.is_open() && (o.mine == mine || o.partner == mine)) {
            return Err(ErrorInfo::new("organism_busy", format!("#{mine} is already promised in an open agreement")));
        }
        let prev = auction.high.clone();
        let held = prev.as_ref().filter(|b| b.bidder == player).map_or(0, |b| b.amount);
        let due = amount - held;
        self.check_funds(player, due)?;
        let text = if held > 0 {
            format!(
                "Raised bid on auction #{listing} (#{}) from {held} to {amount}: {due} more held in escrow",
                l.organism
            )
        } else {
            format!("Bid of {amount} on auction #{listing} (#{} of {}) held in escrow", l.organism, self.name(l.owner))
        };
        let debit = i64::try_from(due).map_err(|_| ErrorInfo::new("bad_price", "bid too large"))?;
        if self.post(player, -debit, LedgerKind::Escrow, text, None, Some(listing)).is_err() {
            let balance = self.balance(player);
            return Err(ErrorInfo::new("insufficient_funds", format!("{due} renown needed, you have {balance}"))
                .with_detail(serde_json::json!({ "balance": balance, "needed": due })));
        }
        if let Some(p) = prev.as_ref().filter(|b| b.bidder != player) {
            let text = format!("Refund: outbid on auction #{listing} (#{}), {} renown back", l.organism, p.amount);
            let _ = self.post(p.bidder, p.amount as i64, LedgerKind::Refund, text, None, Some(listing));
            let alert = format!(
                "Outbid on auction #{listing} (#{}): {} bid {amount}; your {} renown is refunded.",
                l.organism,
                self.name(player),
                p.amount
            );
            self.alert(p.bidder, AlertKind::Market, alert, Some(l.organism), out);
        }
        let tick = self.world.tick();
        if let Some(a) = self.economy.listings.get_mut(&listing).and_then(|x| x.auction.as_mut()) {
            a.high = Some(Bid { bidder: player, amount, mine, tick });
            a.bids += 1;
        }
        self.hold_consented_females();
        let role = self.world.organism(l.organism).map_or("organism", |o| role_name(o.sex));
        let text = format!(
            "{} bid {amount} renown on your auction #{listing} of your {role} #{}.",
            self.name(player),
            l.organism
        );
        self.alert(l.owner, AlertKind::Market, text, Some(l.organism), out);
        let l = self.economy.listings[&listing].clone();
        Ok(Some(serde_json::json!({ "listing": self.listing_view(&l, None) })))
    }

    /// Close an auction at its deadline: the highest bid becomes an
    /// accepted agreement with the bid as its escrowed price; with no bid
    /// (or a winner whose organism can no longer mate with the lot) the lot
    /// returns to the seller and any bid is refunded.
    fn settle_auction(&mut self, l: &Listing, out: &mut Outbox) {
        let high = l.auction.as_ref().and_then(|a| a.high.clone());
        let Some(b) = high else {
            self.record_auction(l, "unsold", None);
            let text = format!("Auction #{} of #{} closed without a bid: the lot is yours again.", l.id, l.organism);
            self.alert(l.owner, AlertKind::Market, text, Some(l.organism), out);
            return;
        };
        // Settlement skips the open-offer limit (the bid was the winner's
        // commitment) but the pair must still be able to mate.
        let problem = self.check_settlement(&b, l).err();
        let released = format!("Winning bid on auction #{} released from the auction", l.id);
        let _ = self.post(b.bidder, b.amount as i64, LedgerKind::Refund, released, None, Some(l.id));
        let offer = match problem {
            None => self
                .open_offer(b.bidder, l.owner, b.mine, l.organism, Some(b.amount), OfferStatus::Accepted, Some(l.id))
                .ok(),
            Some(_) => None,
        };
        let Some(id) = offer else {
            let why = problem.map_or_else(|| "the escrow could not be taken".to_string(), |p| p.message);
            self.record_auction(l, "failed", None);
            let text = format!("Auction #{} won with {} renown but it can't run ({why}): refunded.", l.id, b.amount);
            self.alert(b.bidder, AlertKind::Market, text, Some(b.mine), out);
            let text = format!("Auction #{} of #{} closed: the winning bid can't run ({why}).", l.id, l.organism);
            self.alert(l.owner, AlertKind::Market, text, Some(l.organism), out);
            return;
        };
        self.record_auction(l, "sold", Some(id));
        self.hold_consented_females();
        let view = self.offer_view(id);
        let text = format!(
            "Won auction #{} with {} renown: #{} will mate with {}'s #{} (held in escrow until they meet).",
            l.id,
            b.amount,
            b.mine,
            self.name(l.owner),
            l.organism
        );
        self.alert(b.bidder, AlertKind::Market, text, Some(b.mine), out);
        let text = format!(
            "Auction #{} of #{} sold to {} for {} renown (held in escrow until #{} meets it).",
            l.id,
            l.organism,
            self.name(b.bidder),
            b.amount,
            b.mine
        );
        self.alert(l.owner, AlertKind::Market, text, Some(l.organism), out);
        for p in [b.bidder, l.owner] {
            out.push((Target::Player(p), ServerMsg::MatingUpdate { offer: view.clone(), outcome: None, error: None }));
        }
        let _ = self.execute(id, out);
    }

    /// Whether an auction's winning bid can still become an agreement.
    fn check_settlement(&self, b: &Bid, l: &Listing) -> Result<(), ErrorInfo> {
        let e = |err: CommandError| ErrorInfo::from(&err);
        let a = self.world.organism(b.mine).filter(|o| o.owner == b.bidder).ok_or_else(|| self.gone(b.mine))?;
        let lot =
            self.world.organism(l.organism).filter(|o| o.owner == l.owner).ok_or_else(|| self.gone(l.organism))?;
        if !a.matured {
            return Err(e(CommandError::NotMature(a.id)));
        }
        if a.sex == lot.sex {
            return Err(e(CommandError::SameSex));
        }
        if [a, lot].iter().any(|o| o.sex == Sex::Female && o.pregnancy.is_some()) {
            return Err(e(CommandError::AlreadyPregnant));
        }
        if let Some(problem) = self.world.mating_preview(b.mine, l.organism).ok().as_ref().and_then(incompatibility) {
            return Err(problem);
        }
        if self.offers.values().any(|o| o.status.is_open() && (o.mine == b.mine || o.partner == b.mine)) {
            return Err(ErrorInfo::new("organism_busy", format!("#{} is already promised", b.mine)));
        }
        Ok(())
    }

    /// Queue an auction's end for the store.
    fn record_auction(&mut self, l: &Listing, outcome: &str, offer: Option<u64>) {
        let Some(a) = &l.auction else { return };
        let won = (outcome == "sold").then_some(a.high.as_ref()).flatten();
        self.persist.push(PersistEvent::Auction(AuctionRecord {
            world_id: self.world_id.clone(),
            listing: l.id,
            seller: l.owner,
            organism: l.organism,
            reserve: a.reserve,
            deadline_tick: l.expires_tick.unwrap_or_default(),
            bids: a.bids,
            high_bid: a.high.as_ref().map(|b| b.amount),
            winner: won.map(|b| b.bidder),
            winner_organism: won.map(|b| b.mine),
            outcome: outcome.to_string(),
            offer,
            day: self.world.day(),
        }));
    }

    /// Buy one mating with a listed organism: an accepted agreement at once
    /// (the listing is the owner's consent), the fee in escrow.
    fn buy_listing(
        &mut self,
        player: PlayerId,
        listing: u64,
        mine: OrgId,
        out: &mut Outbox,
    ) -> Result<Option<serde_json::Value>, ErrorInfo> {
        let l = self
            .economy
            .listings
            .get(&listing)
            .cloned()
            .ok_or_else(|| ErrorInfo::new("unknown_listing", format!("no open listing {listing}")))?;
        if l.owner == player {
            return Err(ErrorInfo::new("own_listing", "that listing is yours"));
        }
        if l.auction.is_some() {
            return Err(ErrorInfo::new(
                "auction_listing",
                format!("listing {listing} is an auction: bid on it instead"),
            ));
        }
        let to = self.check_cross_pair(player, mine, l.organism)?;
        if to != l.owner {
            return Err(ErrorInfo::new("unknown_listing", format!("listing {listing} is no longer valid")));
        }
        if self.listing_busy(&l) {
            return Err(ErrorInfo::new(
                "listing_busy",
                format!("#{} is already promised in an open agreement", l.organism),
            ));
        }
        if self.offers.values().any(|o| o.status.is_open() && (o.mine == mine || o.partner == mine)) {
            return Err(ErrorInfo::new("organism_busy", format!("#{mine} is already promised in an open agreement")));
        }
        self.check_funds(player, l.fee)?;
        let id = self.open_offer(player, to, mine, l.organism, Some(l.fee), OfferStatus::Accepted, Some(listing))?;
        if let Some(x) = self.economy.listings.get_mut(&listing) {
            x.sales += 1;
        }
        self.hold_consented_females();
        let view = self.offer_view(id);
        let role = self.world.organism(l.organism).map_or("organism", |o| role_name(o.sex));
        let text = format!(
            "{} bought a mating with your {role} #{} for {} renown (held in escrow until #{mine} meets it).",
            self.name(player),
            l.organism,
            l.fee
        );
        self.alert(to, AlertKind::Market, text, Some(l.organism), out);
        for p in [player, to] {
            out.push((Target::Player(p), ServerMsg::MatingUpdate { offer: view.clone(), outcome: None, error: None }));
        }
        // Try at once; apart, the agreement waits for the two to meet.
        let _ = self.execute(id, out);
        Ok(Some(serde_json::json!({ "offer": view })))
    }

    /// A listed dam already promised in an open agreement takes no more
    /// buyers until it closes (a stud may serve several).
    fn listing_busy(&self, l: &Listing) -> bool {
        self.world.organism(l.organism).is_some_and(|o| o.sex == Sex::Female)
            && self.offers.values().any(|o| o.status.is_open() && (o.partner == l.organism || o.mine == l.organism))
    }

    /// The compatibility of a buyer's organism with a listed one (none for
    /// the buyer's own listing).
    fn listing_preview(&self, player: PlayerId, mine: OrgId, l: &Listing) -> Option<MatingPreviewView> {
        (player != l.owner).then(|| self.preview_mating(player, mine, l.organism).ok()).flatten()
    }

    fn listing_view(&self, l: &Listing, preview: Option<MatingPreviewView>) -> Option<ListingView> {
        let o = self.world.organism(l.organism)?;
        let tpd = self.world.config.ticks_per_day;
        let auction = l.auction.as_ref().map(|a| AuctionView {
            reserve: a.reserve,
            deadline_tick: l.expires_tick.unwrap_or_default(),
            high_bid: a.high.as_ref().map(|b| b.amount),
            high_bidder: a.high.as_ref().map(|b| b.bidder),
            bids: a.bids,
            min_bid: a.min_bid(),
        });
        Some(ListingView {
            id: l.id,
            owner: l.owner,
            owner_name: self.name(l.owner),
            organism: l.organism,
            role: role_name(o.sex).into(),
            sex: sex_name(o.sex).into(),
            fee: l.fee,
            created_tick: l.created_tick,
            expires_tick: l.expires_tick,
            sales: l.sales,
            busy: self.listing_busy(l),
            pregnant: o.pregnancy.is_some(),
            lineage: o.lineage,
            lineage_name: self.world.lineages.get(o.lineage as usize).map(|x| x.name.clone()).unwrap_or_default(),
            generation: o.generation,
            mother: o.mother,
            father: o.father,
            offspring_raised: o.offspring_raised,
            age_days: o.age_days(tpd),
            health: o.health,
            x: o.pos.x,
            y: o.pos.y,
            traits: LISTING_TRAITS.iter().map(|&g| TraitView { gene: gene_key(g), value: o.pheno[g] }).collect(),
            preview,
            kind: if l.auction.is_some() { ListingKind::Auction } else { ListingKind::Fixed },
            auction,
        })
    }

    /// One page of the open listings matching `q`, in its order, with
    /// compatibility against `q.mine`.
    pub fn market(&self, player: PlayerId, q: &MarketQuery) -> Result<MarketView, ErrorInfo> {
        let limit = q.limit.unwrap_or(MARKET_PAGE).clamp(1, MAX_MARKET_PAGE);
        let sort = q.sort.unwrap_or_default();
        let after = q.cursor.as_deref().map(parse_cursor).transpose()?;
        let species: Option<BTreeSet<u32>> = q.species.map(|id| {
            self.world.chronicle.species.get(id).map(|s| s.lineages.iter().map(|l| l.0).collect()).unwrap_or_default()
        });
        // Previews of every candidate only when the filter or the order
        // needs them; otherwise only for the page.
        let early = q.mine.is_some() && (q.compatible || sort == MarketSort::Fertility);
        let mut rows: Vec<((u64, u64), &Listing, Option<MatingPreviewView>)> = Vec::new();
        for l in self.economy.listings.values() {
            let Some(o) = self.world.organism(l.organism) else { continue };
            let role = if o.sex == Sex::Male { ListingRole::Stud } else { ListingRole::Dam };
            let kind = if l.auction.is_some() { ListingKind::Auction } else { ListingKind::Fixed };
            if (q.hide_own && l.owner == player)
                || q.kind.is_some_and(|k| k != kind)
                || q.role.is_some_and(|r| r != role)
                || q.max_price.is_some_and(|m| l.price() > m)
                || q.lineage.is_some_and(|x| o.lineage != x)
                || species.as_ref().is_some_and(|s| !s.contains(&o.lineage))
            {
                continue;
            }
            let preview = q.mine.filter(|_| early).and_then(|m| self.listing_preview(player, m, l));
            if q.compatible && q.mine.is_some() && !preview.as_ref().is_some_and(|p| p.compatible) {
                continue;
            }
            rows.push((market_key(sort, l, preview.as_ref(), q.mine.is_some()), l, preview));
        }
        rows.sort_unstable_by_key(|r| r.0);
        let total = rows.len();
        let start = after.map_or(0, |c| rows.partition_point(|r| r.0 <= c));
        let page = &rows[start..];
        let next_cursor = (page.len() > limit).then(|| cursor_text(page[limit - 1].0));
        let view = |l: &Listing, p: Option<MatingPreviewView>| {
            let p = if early { p } else { q.mine.and_then(|m| self.listing_preview(player, m, l)) };
            self.listing_view(l, p)
        };
        let listings = page.iter().take(limit).filter_map(|(_, l, p)| view(l, p.clone())).collect();
        let own = q.with_own.then(|| {
            self.economy
                .listings
                .values()
                .rev()
                .filter(|l| l.owner == player)
                .filter_map(|l| self.listing_view(l, None))
                .collect()
        });
        Ok(MarketView {
            balance: self.balance(player),
            escrow: self.escrow(player),
            slots_left: self.world.mating_slots_left(player),
            listings,
            total,
            next_cursor,
            own,
        })
    }

    /// Close listings whose organism died and listings past their expiry,
    /// telling the owner; settle auctions at their deadline.
    fn maintain_listings(&mut self, out: &mut Outbox) {
        let tick = self.world.tick();
        let closed: Vec<(Listing, bool)> = self
            .economy
            .listings
            .values()
            .filter_map(|l| {
                let dead = self.world.organism(l.organism).is_none_or(|o| o.owner != l.owner);
                let expired = l.expires_tick.is_some_and(|t| tick >= t);
                (dead || expired).then(|| (l.clone(), dead))
            })
            .collect();
        for (l, dead) in closed {
            let (id, owner, organism) = (l.id, l.owner, l.organism);
            self.economy.listings.remove(&id);
            if l.auction.is_some() && !dead {
                self.settle_auction(&l, out);
                continue;
            }
            if let Some(a) = &l.auction {
                if let Some(b) = &a.high {
                    let text =
                        format!("Refund: auction #{id} ended ({organism} is no longer alive), {} back", b.amount);
                    let _ = self.post(b.bidder, b.amount as i64, LedgerKind::Refund, text, None, Some(id));
                    let text = format!(
                        "Auction #{id} ended: #{organism} is no longer alive; your {} renown is refunded.",
                        b.amount
                    );
                    self.alert(b.bidder, AlertKind::Market, text, Some(organism), out);
                }
                self.record_auction(&l, "ended", None);
            }
            let text = if dead {
                format!("Listing #{id} closed: #{organism} is no longer alive.")
            } else {
                format!("Listing #{id} of #{organism} expired.")
            };
            self.alert(owner, AlertKind::Market, text, Some(organism), out);
        }
    }

    /// Daily renown from the day's legacy record.
    fn pay_income(&mut self, record: &DailyRecord) {
        let players: Vec<(PlayerId, u32)> = self.lineage_of.iter().map(|(&p, &l)| (p, l)).collect();
        for (p, lineage) in players {
            let alive = record.lineages.iter().any(|e| e.lineage == lineage && e.alive > 0);
            let (share, weighted) =
                record.legacy.iter().find(|e| e.player == p).map_or((0.0, 0.0), |e| (e.share, e.weighted_share));
            let income = economy::daily_income(share, weighted, alive);
            if income > 0 {
                let text = format!(
                    "Day {} income: legacy share {:.1}%, weighted {:.1}%",
                    record.day + 1,
                    share * 100.0,
                    weighted * 100.0
                );
                let _ = self.post(p, income as i64, LedgerKind::Income, text, None, None);
            }
        }
    }

    fn respond_mating(
        &mut self,
        player: PlayerId,
        request: u64,
        accept: bool,
        out: &mut Outbox,
    ) -> Result<Option<serde_json::Value>, ErrorInfo> {
        let offer = self
            .offers
            .get(&request)
            .filter(|o| o.to == player)
            .ok_or_else(|| ErrorInfo::new("unknown_offer", format!("no offer {request} made to you")))?;
        if offer.status != OfferStatus::Pending {
            return Err(ErrorInfo::new("offer_closed", format!("offer {request} is {:?}", offer.status)));
        }
        if !accept {
            self.close(request, OfferStatus::Declined, None, None, out);
            return Ok(None);
        }
        self.offers.get_mut(&request).unwrap().status = OfferStatus::Accepted;
        self.hold_consented_females();
        // Try at once; if the organisms are apart, the agreement stands until
        // they meet (checked every tick) or the season ends.
        // The responder's answer succeeded either way; the mating's own
        // result goes to both sides as a `mating_update`.
        Ok(self.execute(request, out).ok().map(|o| serde_json::to_value(o).unwrap()))
    }

    /// Run an accepted agreement. Notifies both sides of the result; a
    /// `too_far` failure leaves it accepted.
    fn execute(&mut self, request: u64, out: &mut Outbox) -> Result<MatingOutcome, ErrorInfo> {
        let o = self.offers[&request].clone();
        let partner_ok = self.world.organism(o.partner).is_some_and(|b| b.owner == o.to);
        let gone = [o.mine, o.partner].into_iter().find(|&id| self.world.organism(id).is_none());
        let result = if let Some(id) = gone {
            let info = self.gone(id);
            self.close(request, OfferStatus::Failed, None, Some(info.clone()), out);
            return Err(info);
        } else if partner_ok {
            self.world.directed_mating(DirectedMating {
                player: o.from,
                a: o.mine,
                b: o.partner,
                partner_consent: true,
            })
        } else {
            Err(CommandError::NotOwned(o.partner))
        };
        match result {
            Ok(outcome) => {
                let m = MatingOutcome::from(outcome);
                self.close(request, OfferStatus::Completed, Some(m), None, out);
                Ok(m)
            }
            Err(err @ CommandError::TooFar { .. }) => {
                let info = ErrorInfo::from(&err);
                let view = self.offer_view(request);
                for p in [o.from, o.to] {
                    out.push((
                        Target::Player(p),
                        ServerMsg::MatingUpdate { offer: view.clone(), outcome: None, error: Some(info.clone()) },
                    ));
                }
                Err(info)
            }
            Err(err) => {
                let info = ErrorInfo::from(&err);
                self.close(request, OfferStatus::Failed, None, Some(info.clone()), out);
                Err(info)
            }
        }
    }

    /// What a request for `mine` × `partner` would give; see
    /// [`ClientMsg::PreviewMating`].
    fn preview_mating(&self, player: PlayerId, mine: OrgId, partner: OrgId) -> Result<MatingPreviewView, ErrorInfo> {
        let e = |err: CommandError| ErrorInfo::from(&err);
        let a = self.world.organism(mine).ok_or(e(CommandError::NotFound(mine)))?;
        if a.owner != player {
            return Err(e(CommandError::NotOwned(mine)));
        }
        let p = self.world.mating_preview(mine, partner).map_err(e)?;
        Ok(MatingPreviewView {
            mine,
            partner,
            compatible: p.fertility > 0.0,
            fertility: p.fertility,
            body_plan_fertility: p.body_plan_fertility,
            distance_fertility: p.distance_fertility,
            body_plan_gaps: body_plan_gaps(&p),
            mismatch: p.mismatch,
            cost: p.cost,
            failure_chance: p.failure_chance,
            distance: p.distance,
            meeting_distance: MEETING_DISTANCE,
            problem: incompatibility(&p),
        })
    }

    /// Keep the females promised in mating agreements from breeding on their
    /// own until the agreement closes: otherwise one tends to be pregnant by
    /// the time her partner arrives, and it fails. A female is held only once
    /// her owner has consented: the requester's own female from the request
    /// on (asking is consent for one's own organism), the partner's female
    /// only once her owner has *accepted*. A merely pending request carries
    /// no consent from the partner's owner, and holding on it would let
    /// anyone freeze another player's female with requests that owner
    /// ignores. (Pairs within one lineage need no consent and mate at once,
    /// so they never wait.) A dam listed on the marketplace is held while
    /// listed: the listing is her owner's consent to a buyer's male, and
    /// otherwise she would rarely be free when one comes. Runs on request,
    /// on acceptance, on listing and every tick.
    fn hold_consented_females(&mut self) {
        let until = self.world.tick() + 2;
        // The highest bidder's female too: the bid is her owner's consent,
        // and she must be free when the auction settles.
        let listed = self.economy.listings.values().flat_map(|l| {
            let bidder = l.auction.as_ref().and_then(|a| a.high.as_ref()).map(|b| b.mine);
            std::iter::once(l.organism).chain(bidder)
        });
        let held: Vec<OrgId> = self
            .offers
            .values()
            .filter(|o| o.status.is_open())
            .flat_map(|o| {
                let partner = (o.status == OfferStatus::Accepted).then_some(o.partner);
                std::iter::once(o.mine).chain(partner)
            })
            .chain(listed)
            .filter(|&id| self.world.organism(id).is_some_and(|x| x.sex == Sex::Female))
            .collect();
        for id in held {
            self.world.hold_breeding(id, until);
        }
    }

    /// Error for an offer's organism that is no longer alive: how it died
    /// when the world still has the record.
    fn gone(&self, id: OrgId) -> ErrorInfo {
        let cause = self.world.organisms.iter().find(|o| o.id == id && !o.alive).and_then(|o| o.death);
        let how = match cause {
            Some(DeathCause::Predation) => "was eaten".to_string(),
            Some(DeathCause::Starvation) => "starved".to_string(),
            Some(DeathCause::OldAge) => "died of old age".to_string(),
            Some(c) => format!("died ({})", c.label()),
            None => "died".to_string(),
        };
        ErrorInfo::new("died", format!("organism {id} {how} before the mating"))
    }

    fn close(
        &mut self,
        request: u64,
        status: OfferStatus,
        outcome: Option<MatingOutcome>,
        error: Option<ErrorInfo>,
        out: &mut Outbox,
    ) {
        let o = self.offers.get_mut(&request).unwrap();
        o.status = status;
        let o = o.clone();
        let view = self.offer_view(request);
        for p in [o.from, o.to] {
            out.push((
                Target::Player(p),
                ServerMsg::MatingUpdate { offer: view.clone(), outcome, error: error.clone() },
            ));
        }
        let settlement = self.settle(&o, status, out);
        self.persist.push(PersistEvent::Trade(TradeRecord {
            offer: o.id,
            from_player: o.from,
            to_player: o.to,
            mine: o.mine,
            partner: o.partner,
            price: o.price,
            status: serde_json::to_value(status).unwrap().as_str().unwrap_or_default().to_string(),
            conceived: outcome.map(|m| m.conceived),
            day: self.world.day(),
            listing: o.listing,
            settlement: settlement.map(String::from),
        }));
        // Closed offers stay only until reported; keep the table small.
        self.offers.remove(&request);
    }

    /// Settle a closing offer's escrow: paid to the partner's owner if the
    /// mating ran, refunded to the requester otherwise.
    fn settle(&mut self, o: &Offer, status: OfferStatus, out: &mut Outbox) -> Option<&'static str> {
        if o.escrow == 0 {
            return None;
        }
        let amount = o.escrow;
        let (from, to) = (self.name(o.from), self.name(o.to));
        if status == OfferStatus::Completed {
            let what = match o.listing {
                Some(l) => format!("listing #{l}"),
                None => format!("offer #{}", o.id),
            };
            let text = format!("Sale: {amount} renown from {from} for {what} (your #{} with #{})", o.partner, o.mine);
            if self.post(o.to, amount as i64, LedgerKind::Sale, text.clone(), Some(o.id), o.listing).is_ok() {
                self.season.add_sale(o.to, amount);
            }
            self.alert(o.to, AlertKind::Market, format!("{text}."), Some(o.partner), out);
            let paid =
                format!("Paid {amount} renown to {to} from escrow: the mating of #{} with #{} ran.", o.mine, o.partner);
            // The payer asked for it: no push.
            self.alert_push(o.from, AlertKind::Market, None, paid, Some(o.mine), out);
            Some("paid")
        } else {
            let how = serde_json::to_value(status).unwrap().as_str().unwrap_or_default().to_string();
            let text = format!("Refund: {amount} renown back from offer #{} with {to} ({how})", o.id);
            let _ = self.post(o.from, amount as i64, LedgerKind::Refund, text.clone(), Some(o.id), o.listing);
            // The offer's own update says why; the refund is no news.
            self.alert_push(o.from, AlertKind::Market, None, format!("{text}."), Some(o.mine), out);
            Some("refunded")
        }
    }

    fn offer_view(&self, id: u64) -> OfferView {
        let o = &self.offers[&id];
        OfferView {
            id: o.id,
            from_player: o.from,
            from_name: self.name(o.from),
            to_player: o.to,
            to_name: self.name(o.to),
            mine: o.mine,
            partner: o.partner,
            price: o.price,
            status: o.status,
            created_tick: o.created_tick,
            listing: o.listing,
            escrow: o.escrow,
        }
    }

    /// Bookkeeping after `World::step`: complete agreements whose organisms
    /// have met, expire offers at a season change, record a new legacy day.
    pub fn after_tick(&mut self) -> Outbox {
        let mut out = Outbox::new();
        let open: Vec<u64> = self.offers.values().filter(|o| o.status.is_open()).map(|o| o.id).collect();
        for id in open {
            let o = &self.offers[&id];
            let (accepted, mine, partner) = (o.status == OfferStatus::Accepted, o.mine, o.partner);
            match (self.world.organism(mine), self.world.organism(partner)) {
                (Some(a), Some(b)) if accepted && a.pos.dist(b.pos) <= MEETING_DISTANCE => {
                    let _ = self.execute(id, &mut out);
                }
                (Some(_), Some(_)) => {}
                _ => {
                    // A pending offer fails too: there is nothing left to accept.
                    let missing = if self.world.organism(mine).is_none() { mine } else { partner };
                    let info = self.gone(missing);
                    self.close(id, OfferStatus::Failed, None, Some(info), &mut out);
                }
            }
        }
        self.hold_consented_females();
        self.maintain_listings(&mut out);
        let season = self.world.season();
        if season != self.last_season {
            self.last_season = season;
            let open: Vec<u64> = self.offers.keys().copied().collect();
            for id in open {
                self.close(id, OfferStatus::Expired, None, None, &mut out);
            }
        }
        self.watch_possessed(&mut out);
        self.announce_deaths(&mut out);
        self.offline_after_tick();
        if self.world.tick().is_multiple_of(self.world.config.ticks_per_hour()) {
            self.sync_focus();
        }
        let legacy_day = self.world.latest_legacy().map(|l| l.day);
        if legacy_day != self.last_legacy_day {
            self.last_legacy_day = legacy_day;
            let record = self.daily_record();
            self.season_day(&record, &mut out);
            self.pay_income(&record);
            self.daily_alerts(&record, &mut out);
            self.chronicle_alerts(&mut out);
            self.history.push_back(record.clone());
            if self.history.len() > HISTORY_DAYS {
                self.history.pop_front();
            }
            self.persist.push(PersistEvent::Daily(record));
        }
        out
    }

    /// Count a daily record towards the ranked season, ending the season
    /// first if the record belongs to the next one: its standings are frozen
    /// into a record for the store and every player hears where they placed.
    fn season_day(&mut self, record: &DailyRecord, out: &mut Outbox) {
        if self.season.is_over_by(record.day) {
            let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let finished = self.season.finish(&self.world_id, self.world.tick(), unix, |p| self.name(p));
            let mut players: BTreeSet<PlayerId> = self.lineage_of.keys().copied().collect();
            players.extend(finished.standings.iter().flat_map(|s| s.rows.iter().map(|r| r.player)));
            for p in players {
                let text = match finished.player_season(p) {
                    Some(mine) => {
                        let places: Vec<String> = mine
                            .placements
                            .iter()
                            .map(|x| format!("#{} of {} in {}", x.rank, x.of, x.category.name()))
                            .collect();
                        format!("Season {} has ended: you placed {}.", finished.head.season, places.join(", "))
                    }
                    None => format!("Season {} has ended; you were not ranked.", finished.head.season),
                };
                self.alert_push(p, AlertKind::World, Some(PushKind::Season), text, None, out);
            }
            self.season = self.season.next(record.day, self.cfg.season_days);
            self.persist.push(PersistEvent::Season(Box::new(finished.clone())));
            self.last_season_record = Some(finished);
        }
        self.season.record_day(record);
    }

    /// This world's id (daily records, ledger, seasons).
    pub fn world_id(&self) -> &str {
        &self.world_id
    }

    /// The ranked season under way: rows `offset..offset + limit` of each
    /// category, plus `player`'s row.
    pub fn current_season(&self, offset: usize, limit: usize, player: Option<PlayerId>) -> CurrentSeason {
        let t = &self.season;
        let standings = t.standings(|p| self.name(p));
        let next = self.last_legacy_day.map_or(t.first_day, |d| d + 1).max(t.first_day);
        CurrentSeason {
            world_id: self.world_id.clone(),
            season: t.season,
            first_day: t.first_day,
            last_day: t.end_day.saturating_sub(1),
            length: t.length(),
            days: t.days,
            days_left: t.end_day.saturating_sub(next),
            offset,
            limit,
            standings: league::page(&standings, offset, limit, player),
        }
    }

    /// The last finished ranked season (since this world started).
    pub fn last_season(&self) -> Option<&SeasonRecord> {
        self.last_season_record.as_ref()
    }

    /// Alert a player; deaths and market alerts are also worth a push.
    fn alert(&mut self, player: PlayerId, kind: AlertKind, text: String, organism: Option<OrgId>, out: &mut Outbox) {
        let push = match kind {
            AlertKind::Death => Some(PushKind::Death),
            AlertKind::Market => Some(PushKind::Market),
            _ => None,
        };
        self.alert_push(player, kind, push, text, organism, out);
    }

    /// Record a push notice (`crate::webpush`) for a player.
    fn notice(&mut self, player: PlayerId, kind: PushKind, text: String, organism: Option<OrgId>) {
        if player != WILD {
            self.notices.push(Notice { player, kind, text, organism });
        }
    }

    /// Alert a player, and record a push notice of kind `push` with it.
    fn alert_push(
        &mut self,
        player: PlayerId,
        kind: AlertKind,
        push: Option<PushKind>,
        text: String,
        organism: Option<OrgId>,
        out: &mut Outbox,
    ) {
        if let Some(p) = push {
            self.notice(player, p, text.clone(), organism);
        }
        let alert = AlertView { tick: self.world.tick(), day: self.world.day(), kind, text, organism };
        let list = self.alerts.entry(player).or_default();
        list.push_back(alert.clone());
        if list.len() > MAX_ALERTS {
            list.pop_front();
        }
        out.push((Target::Player(player), ServerMsg::Alert { alert }));
    }

    /// Recent alerts of a player, oldest first.
    pub fn alerts(&self, player: PlayerId) -> Vec<AlertView> {
        self.alerts.get(&player).map(|l| l.iter().cloned().collect()).unwrap_or_default()
    }

    /// Watch possessed organisms for their death, and tell the owner once a
    /// day when one is parched or starving. The sim looks after it: a
    /// possessed organism breaks off its move order to drink, eat or flee
    /// when the need is severe (`Balance::possess_urgency`) and drops the
    /// order once it arrives.
    fn watch_possessed(&mut self, out: &mut Outbox) {
        let day = self.world.day();
        let urgency = self.world.config.balance.possess_urgency;
        let mut needy = Vec::new();
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner != WILD) {
            let Some(p) = o.possessed_by else { continue };
            self.watched.entry(o.id).or_insert((p, Watch::Possessed));
            let need = o.needs.thirst.max(o.needs.hunger);
            if need >= NEED_ALERT && self.need_alerted.get(&o.id) != Some(&day) {
                needy.push((p, o.id, o.needs.thirst, o.needs.hunger, o.order.is_some()));
            }
        }
        for (p, id, thirst, hunger, walking) in needy {
            self.need_alerted.insert(id, day);
            let (what, act) = if thirst >= hunger {
                (format!("parched (thirst {thirst:.0})"), "drink")
            } else {
                (format!("starving (hunger {hunger:.0})"), "eat")
            };
            let tail = if walking {
                format!("it keeps to your order until the need reaches {urgency:.0}, then stops to {act}; walk it to water or food, or release it")
            } else {
                "it is foraging on its own until you give it an order".to_string()
            };
            self.alert(p, AlertKind::Need, format!("Possessed #{id} is {what}: {tail}."), Some(id), out);
        }
        // Forget released organisms that aren't founders (the dead are
        // announced next).
        let world = &self.world;
        self.watched
            .retain(|id, (_, w)| *w == Watch::Founder || world.organism(*id).is_none_or(|o| o.possessed_by.is_some()));
    }

    /// With level of detail on, make every connected player's view windows
    /// the world's focus for that player (regions around them run as full
    /// agents) and clear the focus of players with no connection: their
    /// lineages go on as cohorts wherever no one is watching. Runs on the
    /// hour, just before the world updates its regions.
    fn sync_focus(&mut self) {
        if !self.world.config.lod.enabled {
            return;
        }
        let mut points: BTreeMap<PlayerId, Vec<Vec2>> = BTreeMap::new();
        for c in self.conns.values() {
            let v = self.view_of(c);
            points.entry(c.player).or_default().push(Vec2::new(v.x, v.y));
        }
        for p in std::mem::take(&mut self.focused) {
            if !points.contains_key(&p) {
                self.world.set_focus(p, Vec::new());
            }
        }
        for (p, pts) in points {
            self.world.set_focus(p, pts);
            self.focused.insert(p);
        }
    }

    /// Living members of a player's lineages: agents and, with level of
    /// detail on, cohort members of abstract regions.
    fn owner_population(&self, player: PlayerId) -> usize {
        let cohorts: usize = if self.world.config.lod.enabled {
            self.world
                .abstract_counts()
                .iter()
                .zip(&self.world.lineages)
                .filter(|(_, l)| l.owner == player)
                .map(|(c, _)| c.alive)
                .sum()
        } else {
            0
        };
        self.world.owner_count(player) + cohorts
    }

    /// Where a player's lineage lives: the mean position of its agents, or,
    /// with none (all of it abstract), of its cohorts by region; the map's
    /// center for a player with neither.
    fn lineage_center(&self, player: PlayerId) -> Vec2 {
        let own: Vec<u32> = (self.world.organisms.iter().enumerate())
            .filter(|(_, o)| o.alive && o.owner == player)
            .map(|(i, _)| i as u32)
            .collect();
        self.lineage_center_of(player, &own)
    }

    /// [`Game::lineage_center`] given the player's living organisms (indices
    /// in organism order).
    fn lineage_center_of(&self, player: PlayerId, own: &[u32]) -> Vec2 {
        let (mut sum, mut n) = (Vec2::ZERO, 0.0f32);
        for &i in own {
            sum += self.world.organisms[i as usize].pos;
            n += 1.0;
        }
        if n == 0.0 {
            let m = &self.world.map;
            let chunk = evo_sim::terrain::CHUNK;
            for (r, c) in self.world.cohorts().filter(|(_, c)| c.owner == player) {
                let k = c.count() as f32;
                let center = Vec2::new(
                    ((r % m.chunks_x) * chunk) as f32 + chunk as f32 / 2.0,
                    ((r / m.chunks_x) * chunk) as f32 + chunk as f32 / 2.0,
                );
                sum += m.clamp(center) * k;
                n += k;
            }
        }
        if n > 0.0 {
            sum * (1.0 / n)
        } else {
            Vec2::new(self.world.map.width as f32 / 2.0, self.world.map.height as f32 / 2.0)
        }
    }

    /// A connection's view window: the pinned one (`set_view`), else
    /// centered on the player's possessed organism, else on its lineage.
    fn view_of(&self, c: &Conn) -> ViewWindow {
        c.view.unwrap_or_else(|| {
            let possessed = self
                .world
                .organisms
                .iter()
                .find(|o| o.alive && o.possessed_by == Some(c.player) && o.owner == c.player);
            let center = possessed.map(|o| o.pos).unwrap_or_else(|| self.lineage_center(c.player));
            ViewWindow { x: center.x, y: center.y, radius: self.cfg.default_view_radius }
        })
    }

    fn announce_deaths(&mut self, out: &mut Outbox) {
        let dead: Vec<(OrgId, PlayerId, Watch)> = self
            .watched
            .iter()
            .filter(|(id, _)| self.world.organism(**id).is_none())
            .map(|(&id, &(p, w))| (id, p, w))
            .collect();
        for (id, player, watch) in dead {
            self.watched.remove(&id);
            self.need_alerted.remove(&id);
            let cause = self.world.organisms.iter().find(|o| o.id == id).and_then(|o| o.death);
            let how = match cause {
                Some(DeathCause::Predation) => "was eaten".to_string(),
                Some(DeathCause::OldAge) => "died of old age".to_string(),
                Some(DeathCause::Starvation) => "starved".to_string(),
                Some(DeathCause::Drowning) => "drowned".to_string(),
                Some(c) => format!("died ({})", c.label()),
                None => "died".to_string(),
            };
            let who = match watch {
                Watch::Founder => "Founder",
                Watch::Possessed => "Possessed",
            };
            self.alert(player, AlertKind::Death, format!("{who} #{id} {how}."), Some(id), out);
        }
    }

    /// Per player: the day's births and deaths, and milestones.
    fn daily_alerts(&mut self, record: &DailyRecord, out: &mut Outbox) {
        let players: Vec<(PlayerId, u32)> = self.lineage_of.iter().map(|(&p, &l)| (p, l)).collect();
        let mut max_gen: BTreeMap<u32, u32> = BTreeMap::new();
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner != WILD) {
            let g = max_gen.entry(o.lineage).or_default();
            *g = (*g).max(o.generation);
        }
        for (p, lineage) in players {
            let Some(entry) = record.lineages.iter().find(|e| e.lineage == lineage) else { continue };
            let died: u64 = entry.deaths.values().sum();
            if entry.born > 0 || died > 0 {
                let mut causes: Vec<(&String, &u64)> = entry.deaths.iter().collect();
                causes.sort_by(|a, b| b.1.cmp(a.1));
                let causes: Vec<String> = causes.iter().map(|(c, n)| format!("{n} {}", c.replace('_', " "))).collect();
                let text = format!(
                    "Day {}: {} born, {} died{} · {} alive",
                    record.day + 1,
                    entry.born,
                    died,
                    if causes.is_empty() { String::new() } else { format!(" ({})", causes.join(", ")) },
                    entry.alive
                );
                self.alert(p, AlertKind::DailySummary, text, None, out);
            }
            let alive = entry.alive;
            let share = record.legacy.iter().find(|e| e.player == p).map_or(0.0, |e| e.share);
            let generation = max_gen.get(&lineage).copied().unwrap_or(0);
            let prog = self.progress.entry(p).or_default();
            let mut texts = Vec::new();
            if let Some(&m) = POPULATION_MILESTONES.iter().rev().find(|&&m| alive >= m && prog.population < m) {
                texts.push((AlertKind::Milestone, format!("Your lineage reached {m} living organisms.")));
            }
            prog.population = prog.population.max(alive);
            if generation > prog.generation {
                let text = if prog.generation == 0 && generation == 1 {
                    "The first offspring of your lineage were born (generation 1).".to_string()
                } else {
                    format!("Generation {generation} reached.")
                };
                texts.push((AlertKind::Birth, text));
                prog.generation = generation;
            }
            if let Some(&m) = SHARE_MILESTONES.iter().rev().find(|&&m| share >= m && prog.share < m) {
                texts.push((AlertKind::Milestone, format!("Your legacy share passed {:.0}%.", m * 100.0)));
            }
            prog.share = prog.share.max(share);
            if alive == 0 && !prog.extinct {
                prog.extinct = true;
                texts.push((AlertKind::Milestone, "Your lineage has no living organisms left.".to_string()));
            }
            for (kind, text) in texts {
                self.alert(p, kind, text, None, out);
            }
        }
    }

    /// Announce what the chronicle recorded since the last day: catastrophes
    /// beginning and ending (to every player with a lineage), achievements
    /// (to their player) and finished eras (to every player, with their rank).
    fn chronicle_alerts(&mut self, out: &mut Outbox) {
        let players: Vec<PlayerId> = self.lineage_of.keys().copied().collect();
        let ch = &self.world.chronicle;
        // (Player, alert kind, push kind, text.)
        let mut alerts: Vec<(PlayerId, AlertKind, Option<PushKind>, String)> = Vec::new();
        self.seen_catastrophes.resize(ch.catastrophes.len(), (false, false));
        for (c, seen) in ch.catastrophes.iter().zip(self.seen_catastrophes.iter_mut()) {
            let what = catastrophe_text(c.kind);
            if c.started && !seen.0 {
                seen.0 = true;
                for &p in &players {
                    alerts.push((
                        p,
                        AlertKind::World,
                        Some(PushKind::Catastrophe),
                        format!("Catastrophe: {what} has begun (until about day {}).", c.end_day + 1),
                    ));
                }
            }
            if c.ended && !seen.1 {
                seen.1 = true;
                let lost = c.population_before.saturating_sub(c.population_after.unwrap_or(0));
                for &p in &players {
                    alerts.push((
                        p,
                        AlertKind::World,
                        None,
                        format!("The {what} is over: {lost} fewer organisms in the world than at its start."),
                    ));
                }
            }
        }
        let mut bonuses: Vec<(PlayerId, u64, String)> = Vec::new();
        for a in &ch.achievements[self.seen_achievements.min(ch.achievements.len())..] {
            let text = achievement_text(&self.world, &a.kind);
            let mut push = None;
            match a.kind {
                AchievementKind::SpeciesFounded { .. } => {
                    self.season.add_species(a.player);
                    push = Some(PushKind::Species);
                }
                AchievementKind::SurvivedCatastrophe { .. } => self.season.add_survival(a.player),
                _ => {}
            }
            let bonus = economy::achievement_bonus(a.kind.label());
            if bonus > 0 && a.player != WILD {
                bonuses.push((a.player, bonus, text.clone()));
            }
            alerts.push((a.player, AlertKind::Achievement, push, text));
        }
        self.seen_achievements = ch.achievements.len();
        for e in &ch.eras[self.seen_eras.min(ch.eras.len())..] {
            for &p in &players {
                let rank = e.ranking.iter().filter(|r| r.player != WILD).position(|r| r.player == p);
                let text = match rank {
                    Some(r) => format!(
                        "Era {} has ended: you finished #{} with {:.1}% of breeding alleles.",
                        e.era + 1,
                        r + 1,
                        e.ranking.iter().find(|x| x.player == p).map_or(0.0, |x| x.share * 100.0)
                    ),
                    None => format!("Era {} has ended; your alleles were not among the breeding adults.", e.era + 1),
                };
                alerts.push((p, AlertKind::World, Some(PushKind::Season), text));
            }
        }
        self.seen_eras = ch.eras.len();
        for (p, kind, push, text) in alerts {
            self.alert_push(p, kind, push, text, None, out);
        }
        for (p, bonus, text) in bonuses {
            let what = text.split(':').next().unwrap_or("Achievement").to_string();
            if self
                .post(p, bonus as i64, LedgerKind::Achievement, format!("{what}: achievement bonus"), None, None)
                .is_ok()
            {
                // The achievement itself is the news.
                let text = format!("+{bonus} renown for an achievement ({what}).");
                self.alert_push(p, AlertKind::Market, None, text, None, out);
            }
        }
    }

    /// Species, eras, catastrophes and achievements (`World::chronicle`).
    pub fn chronicle(&self) -> ChronicleView {
        let w = &self.world;
        let ch = &w.chronicle;
        let t = &ch.species;
        let lineage_name = |l: u32| w.lineages.get(l as usize).map(|x| (x.name.clone(), x.owner)).unwrap_or_default();
        let mut species: Vec<SpeciesView> = t
            .species
            .iter()
            .map(|s| SpeciesView {
                id: s.id,
                name: s.name.clone(),
                origin: s.origin.label().into(),
                parent: s.parent,
                founded_day: s.founded_day,
                extinct_day: s.extinct_day,
                merged_into: s.merged_into,
                founder: s.founder,
                founder_name: s.founder.map(|p| self.name(p)),
                dominant: s.dominant,
                dominant_name: self.name(s.dominant),
                dominant_share: s.dominant_share,
                adults: s.adults,
                peak_adults: s.peak_adults,
                lineages: s
                    .lineages
                    .iter()
                    .map(|&(l, adults)| {
                        let (name, owner) = lineage_name(l);
                        SpeciesLineage { lineage: l, name, owner, adults }
                    })
                    .collect(),
                carnivory: s.carnivory,
                body_size: s.body_size,
            })
            .collect();
        species.sort_by(|a, b| {
            a.extinct_day
                .is_some()
                .cmp(&b.extinct_day.is_some())
                .then(b.adults.total_cmp(&a.adults))
                .then(a.id.cmp(&b.id))
        });
        let day = w.day();
        let era_days = w.config.era_days();
        let era = w.era();
        let eras = EraList {
            era,
            era_days,
            first_day: era * era_days,
            last_day: (era + 1) * era_days - 1,
            days_left: (era + 1) * era_days - day,
            eras: ch
                .eras
                .iter()
                .rev()
                .map(|e| EraBoard {
                    era: e.era,
                    first_day: e.first_day,
                    last_day: e.last_day,
                    breeding_adults: e.breeding_adults,
                    species_alive: e.species_alive,
                    catastrophes: e.catastrophes.clone(),
                    rows: e
                        .ranking
                        .iter()
                        .map(|r| EraRow {
                            player: r.player,
                            name: self.name(r.player),
                            share: r.share,
                            weighted_share: r.weighted_share,
                        })
                        .collect(),
                    niches: e
                        .niches
                        .iter()
                        .map(|n| NicheView {
                            niche: n.niche.clone(),
                            player: n.player,
                            name: self.name(n.player),
                            share: n.share,
                            adults: n.adults,
                        })
                        .collect(),
                })
                .collect(),
        };
        let catastrophes = CatastropheList {
            enabled: w.config.scoring.catastrophes,
            active: ch.active_catastrophe().map(catastrophe_view),
            history: ch.catastrophes.iter().rev().filter(|c| c.started).map(catastrophe_view).collect(),
        };
        // Tallies for every player with a lineage or an achievement.
        let mut tallies: BTreeMap<PlayerId, AchievementTally> = BTreeMap::new();
        let new = |p: PlayerId| AchievementTally { player: p, name: self.name(p), ..Default::default() };
        for &p in self.lineage_of.keys() {
            tallies.entry(p).or_insert_with(|| new(p));
        }
        for (l, life) in ch.lineages.iter().enumerate() {
            let owner = w.lineages[l].owner;
            if owner != WILD {
                let e = tallies.entry(owner).or_insert_with(|| new(owner));
                e.longevity_days = e.longevity_days.max(life.days(day));
            }
        }
        for a in &ch.achievements {
            let e = tallies.entry(a.player).or_insert_with(|| new(a.player));
            match a.kind {
                AchievementKind::SpeciesFounded { .. } => e.species_founded += 1,
                AchievementKind::NicheDominance { .. } => e.niche_dominance += 1,
                AchievementKind::SurvivedCatastrophe { .. } => e.catastrophes_survived += 1,
                AchievementKind::LineageLongevity { .. } => {}
            }
        }
        let mut players: Vec<AchievementTally> = tallies.into_values().collect();
        players.sort_by(|a, b| {
            (b.species_founded + b.niche_dominance + b.catastrophes_survived)
                .cmp(&(a.species_founded + a.niche_dominance + a.catastrophes_survived))
                .then(b.longevity_days.cmp(&a.longevity_days))
                .then(a.player.cmp(&b.player))
        });
        let recent = ch
            .achievements
            .iter()
            .rev()
            .take(200)
            .map(|a| AchievementView {
                day: a.day,
                player: a.player,
                name: self.name(a.player),
                kind: a.kind.label().into(),
                text: achievement_text(w, &a.kind),
            })
            .collect();
        ChronicleView {
            species: SpeciesList {
                day,
                detected_day: t.last_detection,
                samples: t.last_samples,
                alive: t.alive().count(),
                species,
                founded: t
                    .founded
                    .iter()
                    .map(|(&p, &count)| FoundedCount { player: p, name: self.name(p), count })
                    .collect(),
            },
            eras,
            catastrophes,
            achievements: AchievementList { players, recent },
        }
    }

    /// Births and deaths per lineage since the last day boundary.
    fn vitals_since_boundary(&self) -> (BTreeMap<u32, u64>, BTreeMap<u32, BTreeMap<String, u64>>) {
        let c = &self.world.counters;
        let born =
            c.born.iter().map(|(&l, &n)| (l, n.saturating_sub(self.prev_born.get(&l).copied().unwrap_or(0)))).collect();
        let mut deaths: BTreeMap<u32, BTreeMap<String, u64>> = BTreeMap::new();
        for (&(l, cause), &n) in &c.deaths_by_lineage {
            let d = n.saturating_sub(self.prev_deaths.get(&(l, cause)).copied().unwrap_or(0));
            if d > 0 {
                *deaths.entry(l).or_default().entry(death_name(cause).to_string()).or_default() += d;
            }
        }
        (born, deaths)
    }

    /// Sums of every gene's expressed value per player lineage, with counts.
    fn gene_sums(&self) -> BTreeMap<u32, (usize, [f64; Gene::COUNT])> {
        let mut sums: BTreeMap<u32, (usize, [f64; Gene::COUNT])> = BTreeMap::new();
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner != WILD) {
            let e = sums.entry(o.lineage).or_insert((0, [0.0; Gene::COUNT]));
            e.0 += 1;
            for (k, &g) in Gene::ALL.iter().enumerate() {
                e.1[k] += o.pheno[g] as f64;
            }
        }
        sums
    }

    fn daily_record(&mut self) -> DailyRecord {
        let board = self.leaderboard();
        let counts = self.world.population_counts();
        let (born, mut deaths) = self.vitals_since_boundary();
        self.prev_born = self.world.counters.born.clone();
        self.prev_deaths = self.world.counters.deaths_by_lineage.clone();
        let sums = self.gene_sums();
        let gene_means = |k: u32| -> BTreeMap<String, f32> {
            match sums.get(&k) {
                Some((n, s)) if *n > 0 => {
                    Gene::ALL.iter().zip(s).map(|(&g, v)| (gene_key(g), (v / *n as f64) as f32)).collect()
                }
                _ => BTreeMap::new(),
            }
        };
        DailyRecord {
            world_id: self.world_id.clone(),
            day: board.day.unwrap_or(self.world.day()),
            year: self.world.year(),
            season: season_name(self.world.season()).into(),
            breeding_adults: board.breeding_adults,
            legacy: board
                .rows
                .iter()
                .map(|r| LegacyEntry { player: r.player, share: r.share, weighted_share: r.weighted_share })
                .collect(),
            lineages: self
                .world
                .lineages
                .iter()
                .zip(counts)
                .enumerate()
                .map(|(k, (l, c))| {
                    let k = k as u32;
                    LineageEntry {
                        lineage: k,
                        name: l.name.clone(),
                        owner: l.owner,
                        alive: c.alive,
                        adults: c.adults,
                        born: born.get(&k).copied().unwrap_or(0),
                        deaths: deaths.remove(&k).unwrap_or_default(),
                        genes: if l.owner == WILD { BTreeMap::new() } else { gene_means(k) },
                    }
                })
                .collect(),
        }
    }

    /// The daily records since the server started, oldest first.
    pub fn history(&self) -> impl Iterator<Item = &DailyRecord> {
        self.history.iter()
    }

    /// The genetics dashboard of a player's lineage.
    pub fn lineage_stats(&self, player: PlayerId) -> LineageStats {
        let lineage = self.lineage_of(player);
        let day = self.world.day();
        let tpd = self.world.config.ticks_per_day;
        let members: Vec<&evo_sim::organism::Organism> = match lineage {
            Some(l) => self.world.organisms.iter().filter(|o| o.alive && o.lineage == l).collect(),
            None => Vec::new(),
        };
        let adults = members.iter().filter(|o| o.matured).count();
        let cohorts = match lineage {
            Some(l) if self.world.config.lod.enabled => {
                self.world.abstract_counts().get(l as usize).copied().unwrap_or_default()
            }
            _ => Default::default(),
        };
        // Age bands: up to six equal bands (whole days) up to the oldest.
        let oldest = members.iter().map(|o| o.age_days(tpd)).fold(0.0f32, f32::max);
        let width = (oldest / 6.0).ceil().max(1.0);
        let bands = if members.is_empty() { 0 } else { ((oldest / width) as usize + 1).min(6) };
        let mut age_bands: Vec<AgeBand> = (0..bands)
            .map(|k| AgeBand { from_days: k as f32 * width, to_days: (k + 1) as f32 * width, female: 0, male: 0 })
            .collect();
        for o in &members {
            let k = ((o.age_days(tpd) / width) as usize).min(bands - 1);
            match o.sex {
                Sex::Female => age_bands[k].female += 1,
                Sex::Male => age_bands[k].male += 1,
            }
        }
        // Per recorded day: vitals, gene means, legacy share.
        let mut vitals: Vec<DayVitals> = Vec::new();
        let mut history_days = Vec::new();
        let mut gene_hist: Vec<Vec<f32>> = vec![Vec::new(); Gene::COUNT];
        let mut legacy = Vec::new();
        for r in &self.history {
            let alive: usize = r.lineages.iter().filter(|e| e.owner == player).map(|e| e.alive).sum();
            if let Some(e) = r.legacy.iter().find(|e| e.player == player) {
                legacy.push(LegacyPoint { day: r.day, share: e.share, weighted_share: e.weighted_share, alive });
            }
            let Some(e) = lineage.and_then(|l| r.lineages.iter().find(|e| e.lineage == l)) else { continue };
            vitals.push(DayVitals { day: r.day, births: e.born, deaths: e.deaths.clone() });
            if !e.genes.is_empty() {
                history_days.push(r.day);
                for (k, &g) in Gene::ALL.iter().enumerate() {
                    gene_hist[k].push(e.genes.get(&gene_key(g)).copied().unwrap_or(f32::NAN));
                }
            }
        }
        if let Some(l) = lineage {
            let (born, mut deaths) = self.vitals_since_boundary();
            vitals.push(DayVitals {
                day,
                births: born.get(&l).copied().unwrap_or(0),
                deaths: deaths.remove(&l).unwrap_or_default(),
            });
        }
        let keep = vitals.len().saturating_sub(30);
        vitals.drain(..keep);
        let n = members.len().max(1) as f64;
        let mut genes: Vec<GeneStat> = Gene::ALL
            .iter()
            .enumerate()
            .map(|(k, &g)| {
                let mean = members.iter().map(|o| o.pheno[g] as f64).sum::<f64>() / n;
                let var = members.iter().map(|o| (o.pheno[g] as f64 - mean).powi(2)).sum::<f64>() / n;
                let history = std::mem::take(&mut gene_hist[k]);
                let founding_mean = history.first().copied().filter(|v| v.is_finite()).unwrap_or(mean as f32);
                let bins = crate::genetics::histogram(members.iter().map(|o| o.pheno[g]));
                GeneStat { gene: gene_key(g), mean: mean as f32, sd: var.sqrt() as f32, founding_mean, history, bins }
            })
            .collect();
        genes.sort_by(|a, b| {
            (b.mean - b.founding_mean).abs().total_cmp(&(a.mean - a.founding_mean).abs()).then(a.gene.cmp(&b.gene))
        });
        let origins = crate::genetics::allele_origins(members.iter().map(|o| &*o.genome), |p| {
            let lineage = self.lineage_of(p).and_then(|l| self.world.lineages.get(l as usize)).map(|l| l.name.clone());
            (self.name(p), lineage)
        });
        LineageStats {
            player,
            lineage,
            name: lineage.and_then(|l| self.world.lineages.get(l as usize)).map(|l| l.name.clone()),
            day,
            alive: members.len() + cohorts.alive,
            agents: members.len(),
            adults: adults + cohorts.adults,
            juveniles: members.len() + cohorts.alive - adults - cohorts.adults,
            pregnant: members.iter().filter(|o| o.pregnancy.is_some()).count(),
            max_generation: members.iter().map(|o| o.generation).max().unwrap_or(0),
            age_bands,
            vitals,
            history_days,
            genes,
            origins,
            legacy,
            alerts: self.alerts(player),
        }
    }

    /// Legacy shares from the latest daily snapshot, best first. Every player
    /// with a lineage appears, plus the wild ecosystem.
    pub fn leaderboard(&self) -> Leaderboard {
        let snap = self.world.latest_legacy();
        let mut players: Vec<PlayerId> = self.lineage_of.keys().copied().collect();
        if let Some(s) = snap {
            players.extend(s.alleles.keys().copied());
        }
        players.sort_unstable();
        players.dedup();
        let mut rows: Vec<LeaderRow> = players
            .into_iter()
            .map(|p| LeaderRow {
                player: p,
                name: self.name(p).into(),
                share: snap.map_or(0.0, |s| s.share(p)),
                weighted_share: snap.map_or(0.0, |s| s.weighted_share(p)),
                alive: self.owner_population(p),
            })
            .collect();
        rows.sort_by(|a, b| b.share.total_cmp(&a.share).then(a.player.cmp(&b.player)));
        Leaderboard { day: snap.map(|s| s.day), breeding_adults: snap.map_or(0, |s| s.breeding_adults), rows }
    }

    pub fn clock(&self) -> ClockView {
        let w = &self.world;
        ClockView {
            tick: w.tick(),
            day: w.day(),
            year: w.year(),
            hour: w.hour(),
            season: season_name(w.season()).into(),
            daytime: w.is_daytime(),
            era: w.era(),
            era_days: w.config.era_days(),
        }
    }

    pub fn weather(&self) -> WeatherView {
        let w = &self.world;
        WeatherView {
            mean_wind: w.weather.wind,
            storm: w.weather.storm(),
            drought: w.map.drought,
            flood: w.map.flood,
            catastrophe: w.chronicle.active_catastrophe().map(catastrophe_view),
        }
    }

    pub fn summary(&self) -> WorldSummary {
        WorldSummary {
            clock: self.clock(),
            weather: self.weather(),
            width: self.world.map.width,
            height: self.world.map.height,
            organisms: self.world.population(),
            agents: self.world.alive_count(),
            lineages: self.world.lineages.len(),
        }
    }

    /// One of the player's own organisms as sent in a state; a possessed one
    /// also carries its walking speed for the client's prediction (its
    /// `order` comes from [`OwnOrganism::from_org`]).
    fn own_view(&self, o: &evo_sim::organism::Organism, tpd: u32) -> OwnOrganism {
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
        self.state_with(conn, &StateIndex::new(self))
    }

    /// [`Game::state_for`] from a prebuilt index: the cost is the size of
    /// the message (own organisms, the view window's cells, the player's
    /// offers), not the size of the world.
    fn state_with(&self, conn: ConnId, idx: &StateIndex) -> Option<StateView> {
        let c = self.conns.get(&conn)?;
        let player = c.player;
        let tpd = self.world.config.ticks_per_day;
        let orgs = &self.world.organisms;
        let mine = idx.owned(player);
        let own: Vec<OwnOrganism> = mine.iter().map(|&i| self.own_view(&orgs[i as usize], tpd)).collect();
        let view = c.view.unwrap_or_else(|| {
            // Centered on the possessed organism (the first, in organism
            // order), else on the lineage.
            let possessed = mine.iter().map(|&i| &orgs[i as usize]).find(|o| o.possessed_by == Some(player));
            let center = possessed.map(|o| o.pos).unwrap_or_else(|| self.lineage_center_of(player, mine));
            ViewWindow { x: center.x, y: center.y, radius: self.cfg.default_view_radius }
        });
        let center = Vec2::new(view.x, view.y);
        // Candidates from the grid cells the window touches, back in
        // organism order so ties and truncation match a scan of the world.
        let mut cand = idx.in_window(&view);
        cand.sort_unstable();
        let mut nearby: Vec<(f32, u32)> = cand
            .into_iter()
            .filter(|&i| {
                let o = &orgs[i as usize];
                o.owner != player && (o.pos.x - view.x).abs() <= view.radius && (o.pos.y - view.y).abs() <= view.radius
            })
            .map(|i| (orgs[i as usize].pos.dist(center), i))
            .collect();
        if nearby.len() > self.cfg.max_nearby {
            nearby.sort_by(|a, b| a.0.total_cmp(&b.0));
            nearby.truncate(self.cfg.max_nearby);
        }
        let nearby: Vec<NearbyOrganism> =
            nearby.into_iter().map(|(_, i)| NearbyOrganism::from_org(&orgs[i as usize])).collect();
        let ids = idx.offers.get(&player).map_or(&[][..], Vec::as_slice);
        let offers = ids.iter().map(|&id| self.offer_view(id)).collect();
        let escrow = ids.iter().map(|id| &self.offers[id]).filter(|o| o.from == player).map(|o| o.escrow).sum::<u64>()
            + idx.bids.get(&player).copied().unwrap_or(0);
        let leaderboard = self.state_leaderboard(&idx.leaderboard, player, &nearby);
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
    fn state_leaderboard(&self, full: &Leaderboard, player: PlayerId, nearby: &[NearbyOrganism]) -> Leaderboard {
        let top = self.cfg.state_leaderboard_rows;
        if full.rows.len() <= top {
            return full.clone();
        }
        let mut owners: Vec<PlayerId> = nearby.iter().map(|n| n.owner).collect();
        owners.sort_unstable();
        owners.dedup();
        let rows = full
            .rows
            .iter()
            .enumerate()
            .filter(|(rank, r)| *rank < top || r.player == player || owners.binary_search(&r.player).is_ok())
            .map(|(_, r)| r.clone())
            .collect();
        Leaderboard { day: full.day, breeding_adults: full.breeding_adults, rows }
    }

    /// A state message for every connection.
    pub fn states(&self) -> Outbox {
        if self.conns.is_empty() {
            return Vec::new();
        }
        let idx = StateIndex::new(self);
        self.conns
            .keys()
            .filter_map(|&c| self.state_with(c, &idx).map(|s| (Target::Conn(c), ServerMsg::State(Box::new(s)))))
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
        let idx = StateIndex::new(self);
        self.conns
            .iter()
            .filter(|(_, c)| players.contains(&c.player))
            .filter_map(|(&c, _)| self.state_with(c, &idx).map(|s| (Target::Conn(c), ServerMsg::State(Box::new(s)))))
            .collect()
    }
}

/// Side of a [`StateIndex`] grid cell, in tiles.
const STATE_CELL: f32 = 16.0;

/// The world sorted for building state messages, made once per push: living
/// organisms by owner and by grid cell (indices into `World::organisms`, in
/// organism order within each list), open offers by player, and what every
/// state shares (clock, weather, leaderboard). Building one is a pass over
/// the organisms; each state then costs only what it contains, so a push to
/// N connections is O(organisms + output) instead of O(N × organisms).
struct StateIndex {
    by_owner: BTreeMap<PlayerId, Vec<u32>>,
    cols: usize,
    rows: usize,
    /// Cell `k` holds `items[starts[k]..starts[k + 1]]`.
    starts: Vec<u32>,
    items: Vec<u32>,
    /// Open offers a player made or received, by id.
    offers: BTreeMap<PlayerId, Vec<u64>>,
    /// Renown held as the highest bid on auctions, per bidder.
    bids: BTreeMap<PlayerId, u64>,
    clock: ClockView,
    weather: WeatherView,
    leaderboard: Leaderboard,
}

impl StateIndex {
    fn new(g: &Game) -> Self {
        let w = &g.world;
        let cols = (w.map.width as f32 / STATE_CELL).ceil().max(1.0) as usize;
        let rows = (w.map.height as f32 / STATE_CELL).ceil().max(1.0) as usize;
        let cell_of = |p: Vec2| {
            let cx = ((p.x / STATE_CELL).floor().max(0.0) as usize).min(cols - 1);
            let cy = ((p.y / STATE_CELL).floor().max(0.0) as usize).min(rows - 1);
            cy * cols + cx
        };
        let mut by_owner: BTreeMap<PlayerId, Vec<u32>> = BTreeMap::new();
        let mut starts = vec![0u32; cols * rows + 1];
        // One pass over the organisms (each a few cache lines), noting each
        // living one's cell; the counting sort then reads only the cells.
        let mut cell = vec![u32::MAX; w.organisms.len()];
        for (i, o) in w.organisms.iter().enumerate() {
            if !o.alive {
                continue;
            }
            if o.owner != WILD {
                by_owner.entry(o.owner).or_default().push(i as u32);
            }
            let k = cell_of(o.pos);
            cell[i] = k as u32;
            starts[k + 1] += 1;
        }
        for k in 1..starts.len() {
            starts[k] += starts[k - 1];
        }
        let mut fill = starts.clone();
        let mut items = vec![0u32; starts[cols * rows] as usize];
        for (i, &k) in cell.iter().enumerate() {
            if k != u32::MAX {
                items[fill[k as usize] as usize] = i as u32;
                fill[k as usize] += 1;
            }
        }
        let mut offers: BTreeMap<PlayerId, Vec<u64>> = BTreeMap::new();
        for o in g.offers.values().filter(|o| o.status.is_open()) {
            offers.entry(o.from).or_default().push(o.id);
            if o.to != o.from {
                offers.entry(o.to).or_default().push(o.id);
            }
        }
        let mut bids: BTreeMap<PlayerId, u64> = BTreeMap::new();
        for b in g.economy.listings.values().filter_map(|l| l.auction.as_ref()?.high.as_ref()) {
            *bids.entry(b.bidder).or_default() += b.amount;
        }
        StateIndex {
            by_owner,
            cols,
            rows,
            starts,
            items,
            offers,
            bids,
            clock: g.clock(),
            weather: g.weather(),
            leaderboard: g.leaderboard(),
        }
    }

    /// A player's living organisms, in organism order (none for the wild).
    fn owned(&self, player: PlayerId) -> &[u32] {
        self.by_owner.get(&player).map_or(&[], Vec::as_slice)
    }

    /// Every living organism in a grid cell the window touches (a superset
    /// of those inside it), in no particular order.
    fn in_window(&self, v: &ViewWindow) -> Vec<u32> {
        if !(v.x.is_finite() && v.y.is_finite() && v.radius.is_finite()) {
            return Vec::new();
        }
        let span = |lo: f32, hi: f32, n: usize| {
            let a = ((lo / STATE_CELL).floor().max(0.0) as usize).min(n - 1);
            let b = ((hi / STATE_CELL).floor().max(0.0) as usize).min(n - 1);
            a..=b
        };
        let mut out = Vec::new();
        for cy in span(v.y - v.radius, v.y + v.radius, self.rows) {
            for cx in span(v.x - v.radius, v.x + v.radius, self.cols) {
                let k = cy * self.cols + cx;
                out.extend_from_slice(&self.items[self.starts[k] as usize..self.starts[k + 1] as usize]);
            }
        }
        out
    }
}

pub fn catastrophe_view(c: &Catastrophe) -> CatastropheView {
    CatastropheView {
        id: c.id,
        kind: c.kind.label().into(),
        severity: c.severity,
        start_day: c.start_day,
        end_day: c.end_day,
        active: c.is_active(),
        population_before: c.population_before,
        population_after: c.population_after,
        players_before: c.players_before.clone(),
        survivors: c.survivors.clone(),
    }
}

fn catastrophe_text(k: CatastropheKind) -> &'static str {
    match k {
        CatastropheKind::SevereWinter => "a severe winter",
        CatastropheKind::Megadrought => "a megadrought",
        CatastropheKind::Epidemic => "an epidemic",
        CatastropheKind::FloodYear => "a flood year",
    }
}

fn achievement_text(w: &World, k: &AchievementKind) -> String {
    match k {
        AchievementKind::SpeciesFounded { species } => {
            let name = w.chronicle.species.get(*species).map_or_else(|| format!("#{species}"), |s| s.name.clone());
            format!("Species founded: {name} split off as a species of its own, carrying mostly your alleles.")
        }
        AchievementKind::LineageLongevity { lineage, years } => {
            let name = w.lineages.get(*lineage as usize).map_or("Your lineage", |l| l.name.as_str());
            format!("Lineage longevity: {name} has lived {years} year{}.", if *years == 1 { "" } else { "s" })
        }
        AchievementKind::NicheDominance { era, niche, share } => {
            let (axis, what) = niche.split_once(':').unwrap_or(("niche", niche));
            let label = if axis == "diet" { format!("{what}s") } else { format!("the {what}") };
            format!(
                "Niche dominance: era {} ended with {:.0}% of breeding alleles among {label} yours.",
                era + 1,
                share * 100.0
            )
        }
        AchievementKind::SurvivedCatastrophe { catastrophe } => {
            let what = w
                .chronicle
                .catastrophes
                .get(*catastrophe as usize)
                .map_or("a catastrophe", |c| catastrophe_text(c.kind));
            format!("Survivor: your lineage came through {what}.")
        }
    }
}

fn role_name(s: Sex) -> &'static str {
    match s {
        Sex::Male => "stud",
        Sex::Female => "dam",
    }
}

fn body_plan_gaps(p: &MatingPreview) -> Vec<BodyPlanGap> {
    p.body_plan_gaps.iter().map(|&(g, gap)| BodyPlanGap { gene: gene_key(g), gap }).collect()
}

/// Why a pair can't conceive, if it can't: different body plans (naming the
/// genes) or too distant genomes.
fn incompatibility(p: &MatingPreview) -> Option<ErrorInfo> {
    if p.body_plan_fertility <= 0.0 {
        let gaps = body_plan_gaps(p);
        let sterile: Vec<String> = p
            .body_plan_gaps
            .iter()
            .filter(|&&(_, gap)| gap >= evo_sim::mating::BODY_PLAN_STERILE)
            .map(|&(g, gap)| format!("{} ({gap:.2} apart)", g.label()))
            .collect();
        let msg = format!("different body plans: {} differ too much to conceive", sterile.join(", "));
        return Some(ErrorInfo::new("incompatible_body_plans", msg).with_detail(serde_json::json!({ "genes": gaps })));
    }
    if p.distance_fertility <= 0.0 {
        return Some(ErrorInfo::new(
            "genetically_incompatible",
            "the two genomes are too different to conceive (outbreeding depression)",
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_sim::{BirthType, Thermo};

    fn design(name: &str) -> FounderDesign {
        FounderDesign::new(name, Thermo::Warm, BirthType::Live, 2).founders(6)
    }

    /// Two players on a small world with no wild ecosystem.
    fn game() -> (Game, Vec<OrgId>, Vec<OrgId>) {
        let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
        let mut g = Game::new(world, GameConfig::default());
        g.set_name(1, "ann");
        g.set_name(2, "bob");
        let a = g.found(1, &design("A")).unwrap().founders;
        let b = g.found(2, &design("B")).unwrap().founders;
        g.connect(10, 1, "ann");
        g.connect(20, 2, "bob");
        (g, a, b)
    }

    fn place_together(g: &mut Game, a: OrgId, b: OrgId) {
        let pos = g.world.organism(a).unwrap().pos;
        g.world.organisms.iter_mut().find(|o| o.id == b).unwrap().pos = pos;
    }

    fn result(out: &Outbox) -> (bool, Option<String>) {
        match &out[0].1 {
            ServerMsg::Result { ok, error, .. } => (*ok, error.as_ref().map(|e| e.code.clone())),
            m => panic!("expected a result first, got {m:?}"),
        }
    }

    fn female(g: &Game, ids: &[OrgId]) -> OrgId {
        *ids.iter().find(|&&i| g.world.organism(i).unwrap().sex == evo_sim::organism::Sex::Female).unwrap()
    }

    fn male(g: &Game, ids: &[OrgId]) -> OrgId {
        *ids.iter().find(|&&i| g.world.organism(i).unwrap().sex == evo_sim::organism::Sex::Male).unwrap()
    }

    #[test]
    fn one_lineage_per_player() {
        let (mut g, _, _) = game();
        assert!(matches!(g.found(1, &design("again")), Err(FoundError::AlreadyFounded { .. })));
        let over = design("big")
            .with(evo_sim::Gene::Wings, 1.0)
            .with(evo_sim::Gene::Armor, 1.0)
            .with(evo_sim::Gene::Toxins, 1.0)
            .founders(300);
        assert!(matches!(g.found(3, &over), Err(FoundError::Design(DesignError::OverBudget { .. }))));
        assert_eq!(g.lineage_of(3), None);
    }

    #[test]
    fn possession_routes_to_the_world() {
        let (mut g, a, b) = game();
        let out = g.command(10, 1, Some(1), ClientMsg::Possess { organism: a[0] });
        assert_eq!(result(&out), (true, None));
        assert_eq!(out[0].0, Target::Conn(10));
        // Someone else's organism.
        let out = g.command(10, 1, Some(2), ClientMsg::Possess { organism: b[0] });
        assert_eq!(result(&out), (false, Some("not_owned".into())));
        // Bob can't steer Ann's possessed organism.
        let out = g.command(20, 2, None, ClientMsg::OrderMove { organism: a[0], x: 1.0, y: 1.0 });
        assert_eq!(result(&out).1.as_deref(), Some("not_owned"));
        let out = g.command(10, 1, None, ClientMsg::OrderMove { organism: a[0], x: f32::NAN, y: 1.0 });
        assert_eq!(result(&out).1.as_deref(), Some("bad_coordinates"));
        let start = g.world.organism(a[0]).unwrap().pos;
        let target = g.world.map.clamp(start + Vec2::new(4.0, 0.0));
        let out = g.command(10, 1, None, ClientMsg::OrderMove { organism: a[0], x: target.x, y: target.y });
        assert_eq!(result(&out), (true, None));
        for _ in 0..3 {
            g.world.step();
        }
        let o = g.world.organism(a[0]).unwrap();
        assert_eq!(o.action, evo_sim::organism::Action::Ordered);
        // The possessed organism centers the view.
        let s = g.state_for(10).unwrap();
        assert!((s.view.x - o.pos.x).abs() < 1e-3);
        assert!(s.organisms.iter().any(|x| x.id == a[0] && x.possessed));
        // A new order moves it at the next sub-step, and only its owner's
        // connection gets a state for it.
        let p0 = o.pos;
        let target = g.world.map.clamp(p0 + Vec2::new(0.0, if p0.y < 30.0 { 6.0 } else { -6.0 }));
        let out = g.command(10, 1, None, ClientMsg::OrderMove { organism: a[0], x: target.x, y: target.y });
        assert_eq!(result(&out), (true, None));
        let out = g.substep();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, Target::Conn(10));
        let ServerMsg::State(s) = &out[0].1 else { panic!("want a state, got {:?}", out[0].1) };
        let seen = s.organisms.iter().find(|x| x.id == a[0]).unwrap();
        assert!(((seen.x - target.x).powi(2) + (seen.y - target.y).powi(2)).sqrt() < p0.dist(target));
        // What the client needs to predict the walk: the order and the speed.
        assert_eq!(seen.order, Some([target.x, target.y]));
        let o = g.world.organism(a[0]).unwrap();
        assert_eq!(seen.speed, Some(g.world.move_speed(o, o.needs.energy > 40.0)));
        assert!(seen.speed.unwrap() > 0.0);
        assert!(s.organisms.iter().filter(|x| !x.possessed).all(|x| x.order.is_none() && x.speed.is_none()));
        let out = g.command(10, 1, None, ClientMsg::Release { organism: a[0] });
        assert_eq!(result(&out), (true, None));
        let s = g.state_for(10).unwrap();
        let seen = s.organisms.iter().find(|x| x.id == a[0]).unwrap();
        assert_eq!((seen.order, seen.speed), (None, None));
        // Nothing possessed: a sub-step sends nothing.
        assert!(g.substep().is_empty());
        let out = g.command(10, 1, None, ClientMsg::Migrate { x: 5.0, y: 5.0, group: None });
        assert_eq!(result(&out), (true, None));
        let out = g.command(10, 1, None, ClientMsg::ClearMigration { group: None });
        assert_eq!(result(&out), (true, None));
    }

    fn reply_data(out: &Outbox) -> serde_json::Value {
        match &out[0].1 {
            ServerMsg::Result { ok: true, data: Some(d), .. } => d.clone(),
            m => panic!("expected data, got {m:?}"),
        }
    }

    /// Split, per-group orders, territory and merge through the protocol,
    /// with what the state shows of them.
    #[test]
    fn group_commands_split_claim_and_merge() {
        let (mut g, a, b) = game();
        let s = g.state_for(10).unwrap();
        assert_eq!(s.groups.len(), 1);
        assert_eq!((s.groups[0].id, s.groups[0].members), (0, a.len()));
        assert!(s.organisms.iter().all(|o| o.group == 0));
        // Split ann's founders: half go to group 1.
        let out = g.command(10, 1, Some(4), ClientMsg::SplitGroup { group: 0, organisms: None });
        let d = reply_data(&out);
        assert_eq!((d["group"].as_u64(), d["moved"].as_u64()), (Some(1), Some(a.len() as u64 / 2)));
        // Group 1 holds a territory; the main group migrates.
        let out = g.command(10, 1, None, ClientMsg::ClaimTerritory { group: 1, x: 20.0, y: 22.0, radius: 500.0 });
        let t: TerritoryView = serde_json::from_value(reply_data(&out)).unwrap();
        assert_eq!((t.x, t.y), (20.0, 22.0));
        let out = g.command(10, 1, None, ClientMsg::Migrate { x: 50.0, y: 50.0, group: Some(0) });
        assert_eq!(result(&out), (true, None));
        let s = g.state_for(10).unwrap();
        assert_eq!(s.groups.iter().map(|x| x.id).collect::<Vec<_>>(), vec![0, 1]);
        let (g0, g1) = (&s.groups[0], &s.groups[1]);
        assert_eq!((g0.members + g1.members, g1.members), (a.len(), a.len() / 2));
        assert_eq!((g0.migration, g0.territory), (Some([50.0, 50.0]), None));
        assert_eq!((g1.migration, g1.territory.map(|t| t.radius)), (None, Some(g1.max_radius)));
        assert!(g1.claim_ready_tick > 0);
        assert_eq!(s.migration, Some([50.0, 50.0]), "the main group's order");
        assert_eq!(s.organisms.iter().filter(|o| o.group == 1).count(), a.len() / 2);
        // Bob sees none of it.
        let sb = g.state_for(20).unwrap();
        assert_eq!(sb.groups.len(), 1);
        assert!(sb.groups[0].territory.is_none() && sb.groups[0].migration.is_none());

        // Errors: their codes, nothing changed.
        let code = |g: &mut Game, conn, player, m| result(&g.command(conn, player, None, m)).1;
        let claim = ClientMsg::ClaimTerritory { group: 1, x: 20.0, y: 20.0, radius: 5.0 };
        assert_eq!(code(&mut g, 10, 1, claim).as_deref(), Some("claim_cooldown"));
        let nan = ClientMsg::ClaimTerritory { group: 0, x: f32::NAN, y: 20.0, radius: 5.0 };
        assert_eq!(code(&mut g, 10, 1, nan).as_deref(), Some("bad_coordinates"));
        let inf = ClientMsg::ClaimTerritory { group: 0, x: 1.0, y: 20.0, radius: f32::INFINITY };
        assert_eq!(code(&mut g, 10, 1, inf).as_deref(), Some("bad_coordinates"));
        let none = ClientMsg::ClaimTerritory { group: 3, x: 1.0, y: 1.0, radius: 5.0 };
        assert_eq!(code(&mut g, 10, 1, none).as_deref(), Some("no_such_group"));
        assert_eq!(code(&mut g, 10, 1, ClientMsg::AbandonTerritory { group: 0 }).as_deref(), Some("no_territory"));
        let theirs = ClientMsg::SplitGroup { group: 0, organisms: Some(vec![b[0]]) };
        assert_eq!(code(&mut g, 10, 1, theirs).as_deref(), Some("not_owned"));
        let many = ClientMsg::SplitGroup { group: 0, organisms: Some(vec![a[0]; MAX_SPLIT_ORGANISMS + 1]) };
        assert_eq!(code(&mut g, 10, 1, many).as_deref(), Some("too_many_organisms"));
        let in_one = g.world.organisms.iter().find(|o| o.owner == 1 && o.group == 1).unwrap().id;
        let wrong = ClientMsg::SplitGroup { group: 0, organisms: Some(vec![in_one]) };
        assert_eq!(code(&mut g, 10, 1, wrong).as_deref(), Some("not_in_group"));
        assert_eq!(code(&mut g, 10, 1, ClientMsg::MergeGroups { from: 1, into: 1 }).as_deref(), Some("same_group"));
        let far = ClientMsg::Migrate { x: 1.0, y: 1.0, group: Some(9) };
        assert_eq!(code(&mut g, 10, 1, far).as_deref(), Some("no_such_group"));
        assert_eq!(code(&mut g, 10, 1, ClientMsg::ClearMigration { group: Some(9) }).as_deref(), Some("no_such_group"));
        assert_eq!(g.world.territory(1, 1).map(|t| (t.center.x, t.center.y)), Some((20.0, 22.0)));
        // Bob can't touch ann's groups: his own group 1 doesn't exist.
        assert_eq!(code(&mut g, 20, 2, ClientMsg::AbandonTerritory { group: 1 }).as_deref(), Some("no_territory"));
        assert_eq!(code(&mut g, 20, 2, ClientMsg::MergeGroups { from: 1, into: 0 }).as_deref(), Some("no_such_group"));

        // Abandon, merge back: one group again.
        assert_eq!(result(&g.command(10, 1, None, ClientMsg::AbandonTerritory { group: 1 })), (true, None));
        let out = g.command(10, 1, None, ClientMsg::MergeGroups { from: 1, into: 0 });
        assert_eq!(reply_data(&out)["moved"].as_u64(), Some(a.len() as u64 / 2));
        let s = g.state_for(10).unwrap();
        assert_eq!(s.groups.len(), 1);
        assert!(s.organisms.iter().all(|o| o.group == 0));
        // A whole-lineage clear reaches every group.
        assert_eq!(result(&g.command(10, 1, None, ClientMsg::ClearMigration { group: None })), (true, None));
        assert_eq!(g.state_for(10).unwrap().migration, None);
    }

    #[test]
    fn state_hides_the_far_world() {
        let (mut g, a, b) = game();
        let s = g.state_for(10).unwrap();
        assert_eq!(s.organisms.len(), a.len());
        assert!(s.nearby.iter().all(|n| n.owner != 1));
        // Pin the view on Bob's founders: they appear; then far away: none.
        let p = g.world.organism(b[0]).unwrap().pos;
        g.command(10, 1, None, ClientMsg::SetView { x: p.x, y: p.y, radius: 2.0 });
        let s = g.state_for(10).unwrap();
        assert!(s.nearby.iter().any(|n| n.id == b[0]));
        assert!(s.nearby.iter().all(|n| (n.x - p.x).abs() <= 2.0 && (n.y - p.y).abs() <= 2.0));
        assert_eq!(g.states().len(), 2);
    }

    #[test]
    fn consent_flow_completes_when_organisms_meet() {
        let (mut g, a, b) = game();
        let (fa, mb) = (female(&g, &a), male(&g, &b));
        // Organisms apart: request, accept, agreement waits.
        let out = g.command(10, 1, Some(5), ClientMsg::RequestMating { mine: fa, partner: mb, price: Some(30) });
        assert_eq!(result(&out), (true, None));
        let offer = match &out[1] {
            (Target::Player(2), ServerMsg::MatingRequest { offer }) => offer.clone(),
            m => panic!("partner owner not asked: {m:?}"),
        };
        assert_eq!(offer.price, Some(30));
        assert_eq!(offer.status, OfferStatus::Pending);
        // Only the partner owner can answer.
        let out = g.command(10, 1, None, ClientMsg::RespondMating { request: offer.id, accept: true });
        assert_eq!(result(&out).1.as_deref(), Some("unknown_offer"));
        let slots = g.world.mating_slots_left(1);
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: offer.id, accept: true });
        assert_eq!(result(&out), (true, None));
        let too_far =
            out.iter().any(|(_, m)| matches!(m, ServerMsg::MatingUpdate { error: Some(e), .. } if e.code == "too_far"));
        assert!(too_far, "{out:?}");
        assert_eq!(g.state_for(10).unwrap().offers[0].status, OfferStatus::Accepted);
        let out = g.command(10, 1, None, ClientMsg::CompleteMating { request: offer.id });
        assert_eq!(result(&out).1.as_deref(), Some("too_far"));
        // They meet: the next tick's bookkeeping runs the mating.
        place_together(&mut g, fa, mb);
        let out = g.after_tick();
        let done = out.iter().find_map(|(t, m)| match m {
            ServerMsg::MatingUpdate { offer, outcome: Some(o), .. } if *t == Target::Player(1) => {
                Some((offer.status, *o))
            }
            _ => None,
        });
        let (status, outcome) = done.expect("completion reported to the requester");
        assert_eq!(status, OfferStatus::Completed);
        assert!(outcome.fertility > 0.0);
        assert!(out.iter().any(|(t, m)| *t == Target::Player(2) && matches!(m, ServerMsg::MatingUpdate { .. })));
        assert_eq!(g.world.mating_slots_left(1), slots - 1);
        assert!(g.state_for(10).unwrap().offers.is_empty());
        let trades = g.take_persist();
        assert!(trades
            .iter()
            .any(|t| matches!(t, PersistEvent::Trade(r) if r.price == Some(30) && r.status == "completed")));
    }

    #[test]
    fn consent_flow_decline_cancel_and_wild() {
        let (mut g, a, b) = game();
        let (fa, mb) = (female(&g, &a), male(&g, &b));
        place_together(&mut g, fa, mb);
        // Decline.
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: None });
        let id = match &out[1].1 {
            ServerMsg::MatingRequest { offer } => offer.id,
            m => panic!("{m:?}"),
        };
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: false });
        assert_eq!(result(&out), (true, None));
        assert!(out.iter().any(|(t, m)| *t == Target::Player(1)
            && matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Declined)));
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        assert_eq!(result(&out).1.as_deref(), Some("unknown_offer"));
        // Cancel by the requester.
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: None });
        let id = match &out[1].1 {
            ServerMsg::MatingRequest { offer } => offer.id,
            m => panic!("{m:?}"),
        };
        let out = g.command(10, 1, None, ClientMsg::CancelMating { request: id });
        assert_eq!(result(&out), (true, None));
        // Accept with the organisms together: runs at once.
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: None });
        let id = match &out[1].1 {
            ServerMsg::MatingRequest { offer } => offer.id,
            m => panic!("{m:?}"),
        };
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        assert!(
            out.iter().any(
                |(_, m)| matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Completed)
            ),
            "{out:?}"
        );
        // Own pair: runs at once, no consent round.
        let (f2, m2) = (a[2], a[1]);
        place_together(&mut g, f2, m2);
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: m2, partner: f2, price: None });
        assert_eq!(result(&out), (true, None));
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0].1, ServerMsg::Result { data: Some(d), .. } if d.get("conceived").is_some()));
        // Not your organism.
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: b[0], partner: a[0], price: None });
        assert_eq!(result(&out).1.as_deref(), Some("not_owned"));
    }

    #[test]
    fn wild_organisms_cannot_consent_and_seasons_expire_offers() {
        let world = build_world(&WorldSettings {
            seed: 3,
            width: 64,
            height: 64,
            wild: true,
            days_per_year: 8,
            lod: false,
            catastrophes: false,
        });
        let mut g = Game::new(world, GameConfig::default());
        let a = g.found(1, &design("A")).unwrap().founders;
        let b = g.found(2, &design("B")).unwrap().founders;
        g.connect(10, 1, "ann");
        let wild = g.world.organisms.iter().find(|o| o.owner == WILD).unwrap().id;
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: a[0], partner: wild, price: None });
        assert_eq!(result(&out).1.as_deref(), Some("no_consent"));
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: a[0], partner: b[1], price: None });
        assert_eq!(result(&out), (true, None));
        let season = g.world.season();
        let days = g.world.config.days_per_year / 4 + 1;
        let mut expired = false;
        for _ in 0..days * g.world.config.ticks_per_day {
            g.world.step();
            let out = g.after_tick();
            expired |= out.iter().any(
                |(_, m)| matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Expired),
            );
            if g.world.season() != season {
                break;
            }
        }
        assert!(expired);
        let daily = g.take_persist().into_iter().filter(|e| matches!(e, PersistEvent::Daily(_))).count();
        assert!(daily >= 1);
        assert!(g.leaderboard().rows.iter().any(|r| r.player == WILD));
    }

    fn run_days(g: &mut Game, days: u32) -> Outbox {
        let mut out = Outbox::new();
        for _ in 0..days * g.world.config.ticks_per_day {
            g.world.step();
            out.extend(g.after_tick());
        }
        out
    }

    fn alerts_in(out: &Outbox, player: PlayerId) -> Vec<AlertView> {
        out.iter()
            .filter_map(|(t, m)| match m {
                ServerMsg::Alert { alert } if *t == Target::Player(player) => Some(alert.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn lineage_stats_follow_the_daily_history() {
        let (mut g, a, _) = game();
        let s = g.lineage_stats(1);
        assert_eq!((s.lineage, s.alive, s.adults), (Some(0), a.len(), a.len()));
        assert_eq!(s.age_bands.iter().map(|b| b.female + b.male).sum::<usize>(), a.len());
        assert!(s.age_bands.len() <= 6 && s.age_bands.windows(2).all(|w| w[0].to_days == w[1].from_days));
        assert_eq!(s.genes.len(), Gene::COUNT);
        assert!(s.history_days.is_empty() && s.vitals.len() == 1);
        run_days(&mut g, 3);
        let s = g.lineage_stats(1);
        assert_eq!(s.history_days.len(), 3);
        assert_eq!(s.vitals.len(), 4, "three recorded days and today");
        assert!(s.genes.iter().all(|x| x.history.len() == 3 && (0.0..=1.0).contains(&x.mean) && x.sd >= 0.0));
        // Sorted by drift from the founding mean.
        let drift: Vec<f32> = s.genes.iter().map(|x| (x.mean - x.founding_mean).abs()).collect();
        assert!(drift.windows(2).all(|w| w[0] >= w[1]));
        let body = s.genes.iter().find(|x| x.gene == "body_size").unwrap();
        assert_eq!(body.founding_mean, body.history[0], "{body:?}");
        assert_eq!(s.legacy.len(), 3);
        // The history counts births and deaths like the sim's counters.
        let born: u64 = s.vitals.iter().map(|v| v.births).sum();
        assert_eq!(born, g.world.counters.born.get(&0).copied().unwrap_or(0));
        let died: u64 = s.vitals.iter().flat_map(|v| v.deaths.values()).sum();
        let sim_died: u64 =
            g.world.counters.deaths_by_lineage.iter().filter(|((l, _), _)| *l == 0).map(|(_, n)| n).sum();
        assert_eq!(died, sim_died);
        // Records carry gene means for players only; nobody else's lineage.
        let rec = g.history().last().unwrap();
        assert!(rec.lineages.iter().filter(|e| e.owner != WILD).all(|e| e.genes.len() == Gene::COUNT || e.alive == 0));
        let none = g.lineage_stats(9);
        assert_eq!((none.lineage, none.alive, none.genes.len()), (None, 0, Gene::COUNT));
    }

    #[test]
    fn deaths_of_watched_organisms_and_milestones_raise_alerts() {
        let (mut g, a, _) = game();
        // A founder at the end of its strength dies on its next tick.
        g.world.organisms.iter_mut().find(|o| o.id == a[1]).unwrap().health = -1000.0;
        let out = run_days(&mut g, 1);
        let alerts = alerts_in(&out, 1);
        let death = alerts.iter().find(|x| x.kind == AlertKind::Death).expect("founder death announced");
        assert_eq!(death.organism, Some(a[1]));
        assert!(death.text.starts_with(&format!("Founder #{}", a[1])), "{}", death.text);
        assert!(alerts.iter().any(|x| x.kind == AlertKind::DailySummary && x.text.contains("died")), "{alerts:?}");
        assert!(alerts_in(&out, 2).iter().all(|x| x.organism != Some(a[1])), "only the owner hears");
        assert_eq!(g.alerts(1).len(), alerts.len());
        assert!(g.lineage_stats(1).alerts.iter().any(|x| x.kind == AlertKind::Death));
        // The death is worth a push; daily summaries are not.
        let notices = g.take_notices();
        let n = notices.iter().find(|n| n.organism == Some(a[1])).expect("a death notice");
        assert_eq!((n.player, n.kind, &n.text), (1, PushKind::Death, &death.text));
        assert!(notices.iter().all(|n| n.kind == PushKind::Death), "{notices:?}");
        assert!(g.take_notices().is_empty());
    }

    #[test]
    fn possessed_needs_raise_an_alert_and_the_sim_lets_severe_ones_win() {
        let (mut g, a, _) = game();
        let id = a[0];
        // Start it a few tiles from water, on open ground: too far to drink
        // on its first steps, near enough to reach water within the day.
        let m = &g.world.map;
        let dry = (0..m.tiles.len())
            .find(|&k| m.water_dist[k] == 5 && m.ground()[k].slope < 0.3 && m.ground()[k].veg_density < 0.4)
            .expect("open ground five tiles from water");
        let dry = evo_sim::terrain::Map::tile_center(dry % m.width, dry / m.width);
        g.world.organisms.iter_mut().find(|o| o.id == id).unwrap().pos = dry;
        g.command(10, 1, None, ClientMsg::Possess { organism: id });
        let pos = g.world.organism(id).unwrap().pos;
        let far = Vec2::new(if pos.x < 32.0 { 63.0 } else { 0.0 }, pos.y);
        g.command(10, 1, None, ClientMsg::OrderMove { organism: id, x: far.x, y: far.y });
        let set_thirst = |g: &mut Game, t: f32| {
            let o = g.world.organisms.iter_mut().find(|o| o.id == id).unwrap();
            o.needs.thirst = t;
            o.needs.hunger = 0.0;
        };
        // Thirsty, but not yet severely: an alert, and it keeps walking.
        set_thirst(&mut g, 80.0);
        g.world.step();
        let out = g.after_tick();
        let need = alerts_in(&out, 1).into_iter().find(|x| x.kind == AlertKind::Need).expect("need alert");
        assert!(need.text.contains("parched") && need.text.contains("stops to drink"), "{}", need.text);
        let o = g.world.organism(id).unwrap();
        assert_eq!(o.action, evo_sim::organism::Action::Ordered);
        assert!(o.order.is_some() && o.possessed_by == Some(1));
        // Once a day per organism.
        g.world.step();
        assert!(alerts_in(&g.after_tick(), 1).iter().all(|x| x.kind != AlertKind::Need));
        // Severely thirsty: it breaks off to drink, keeping the order for later.
        set_thirst(&mut g, 92.0);
        g.world.step();
        g.after_tick();
        let o = g.world.organism(id).unwrap();
        assert_eq!(o.action, evo_sim::organism::Action::Drink);
        assert!(o.order.is_some());
        let mut least = 100.0f32;
        for _ in 0..g.world.config.ticks_per_day {
            g.world.step();
            g.after_tick();
            least = least.min(g.world.organism(id).expect("still alive").needs.thirst);
        }
        assert!(least < 50.0, "thirst stayed at {least}");
    }

    /// A 128-tile world with level of detail, the wild ecosystem and one
    /// player, connected.
    fn lod_game() -> Game {
        let world = build_world(&WorldSettings {
            seed: 7,
            width: 128,
            height: 128,
            wild: true,
            lod: true,
            ..Default::default()
        });
        let mut g = Game::new(world, GameConfig::default());
        g.found(1, &design("A").founders(12)).unwrap();
        g.connect(10, 1, "ann");
        g
    }

    fn lineage_agents(g: &Game, lineage: u32) -> usize {
        g.world.organisms.iter().filter(|o| o.alive && o.lineage == lineage).count()
    }

    #[test]
    fn level_of_detail_follows_the_views_of_connected_players() {
        let mut g = lod_game();
        let l = g.lineage_of(1).unwrap();
        run_days(&mut g, 2);
        let s = g.state_for(10).unwrap();
        let center = Vec2::new(s.view.x, s.view.y);
        // The player's view runs as agents (its own lineage among them).
        assert!(g.world.region_is_active(g.world.region_of(center)));
        assert!(!s.organisms.is_empty(), "no agents in view at {center:?}");
        // The far corner is cohorts.
        let far = Vec2::new(if center.x < 64.0 { 120.0 } else { 8.0 }, if center.y < 64.0 { 120.0 } else { 8.0 });
        assert!(!g.world.region_is_active(g.world.region_of(far)), "far region {far:?} still active");
        assert!(g.world.abstract_count() > 0);
        // Counts include cohort members everywhere they are reported.
        let summary = g.summary();
        assert_eq!(summary.organisms, summary.agents + g.world.abstract_count());
        let rec = g.history().last().unwrap();
        let recorded: usize = rec.lineages.iter().map(|e| e.alive).sum();
        assert!(recorded > g.world.alive_count(), "daily record counts agents only: {recorded}");
        let check = |g: &Game| {
            let stats = g.lineage_stats(1);
            let cohorts = g.world.abstract_counts()[l as usize].alive;
            assert_eq!(stats.agents, lineage_agents(g, l));
            assert_eq!(stats.alive, stats.agents + cohorts);
            assert_eq!(g.state_for(10).map(|s| s.population), Some(stats.alive));
            assert_eq!(g.leaderboard().rows.iter().find(|r| r.player == 1).map(|r| r.alive), Some(stats.alive));
            stats
        };
        check(&g);

        // Offline, the player's lineage goes on as cohorts.
        g.disconnect(10);
        run_days(&mut g, 2);
        assert_eq!(lineage_agents(&g, l), 0, "an offline lineage stays agents");
        let stats = g.lineage_stats(1);
        assert!(stats.alive > 0 && stats.agents == 0, "{} alive, {} agents", stats.alive, stats.agents);

        // Back online, the view finds the lineage by its cohorts and the
        // region around it expands again within the hour.
        g.connect(10, 1, "ann");
        for _ in 0..2 * g.world.config.ticks_per_hour() {
            g.world.step();
            g.after_tick();
        }
        assert!(lineage_agents(&g, l) > 0, "the lineage did not expand around the returning player");
        let s = check(&g);
        assert!(s.agents > 0);
    }

    #[test]
    fn without_level_of_detail_focus_is_left_alone() {
        let (mut g, _, _) = game();
        run_days(&mut g, 1);
        assert_eq!(g.world.abstract_count(), 0);
        let stats = g.lineage_stats(1);
        assert_eq!(stats.alive, stats.agents);
        assert_eq!(g.summary().organisms, g.summary().agents);
    }

    #[test]
    fn catastrophes_achievements_and_eras_are_announced_and_exposed() {
        // Short years (8 days), one-year eras, a forced severe winter.
        let mut world = build_world(&WorldSettings {
            seed: 3,
            width: 64,
            height: 64,
            wild: true,
            days_per_year: 8,
            lod: false,
            catastrophes: false,
        });
        world.config.scoring.era_years = 1;
        world.config.scoring.species_interval_days = 1;
        let id = world.schedule_catastrophe(CatastropheKind::SevereWinter, 2, Some(2));
        let mut g = Game::new(world, GameConfig::default());
        g.set_name(1, "ann");
        g.found(1, &FounderDesign::new("A", Thermo::Cold, BirthType::Live, 2).with(Gene::BodySize, 0.5).founders(12))
            .unwrap();
        g.connect(10, 1, "ann");
        let out = run_days(&mut g, 3);
        let alerts = alerts_in(&out, 1);
        assert!(
            alerts.iter().any(|a| a.kind == AlertKind::World && a.text.contains("severe winter has begun")),
            "{alerts:?}"
        );
        let begun = g.take_notices();
        assert!(
            begun.iter().any(|n| n.kind == PushKind::Catastrophe && n.text.contains("severe winter has begun")),
            "{begun:?}"
        );
        let state = g.state_for(10).unwrap();
        let c = state.weather.catastrophe.expect("banner while active");
        assert_eq!((c.id, c.kind.as_str(), c.active), (id, "severe_winter", true));
        let out = run_days(&mut g, 6);
        let alerts = alerts_in(&out, 1);
        assert!(alerts.iter().any(|a| a.kind == AlertKind::World && a.text.contains("is over")));
        assert!(
            alerts.iter().any(|a| a.kind == AlertKind::Achievement && a.text.starts_with("Survivor")),
            "{alerts:?}"
        );
        assert!(alerts.iter().any(|a| a.kind == AlertKind::Achievement && a.text.starts_with("Lineage longevity")));
        assert!(
            alerts.iter().any(|a| a.kind == AlertKind::World && a.text.starts_with("Era 1 has ended: you finished #1")),
            "{alerts:?}"
        );
        assert!(g.state_for(10).unwrap().weather.catastrophe.is_none());
        // Pushed: the era's end; not: the catastrophe's end, other achievements.
        let notices = g.take_notices();
        assert!(notices.iter().any(|n| n.kind == PushKind::Season && n.text.starts_with("Era 1 has ended")));
        assert!(!notices.iter().any(|n| n.text.contains("is over") || n.text.starts_with("Survivor")), "{notices:?}");

        let ch = g.chronicle();
        assert_eq!(ch.catastrophes.history.len(), 1);
        assert_eq!(ch.catastrophes.history[0].survivors, vec![1]);
        assert!(ch.catastrophes.active.is_none());
        assert_eq!((ch.eras.era, ch.eras.era_days, ch.eras.eras.len()), (1, 8, 1));
        assert_eq!(ch.eras.eras[0].rows.iter().find(|r| r.player == 1).unwrap().name, "ann");
        assert!(ch.species.alive >= 2 && ch.species.detected_day == Some(8));
        let mine =
            ch.species.species.iter().find(|s| s.lineages.first().is_some_and(|l| l.owner == 1)).expect("own species");
        assert_eq!((mine.dominant, mine.dominant_name.as_str(), mine.origin.as_str()), (1, "ann", "initial"));
        let ann = ch.achievements.players.iter().find(|p| p.player == 1).unwrap();
        assert_eq!(ann.catastrophes_survived, 1);
        assert!(ann.longevity_days >= 8);
        assert!(ch.achievements.recent.iter().all(|a| a.player == 1 && !a.text.is_empty()));
        assert_eq!(g.clock().era, 1);
    }

    /// Request a mating of player 1's male with player 2's female, have the
    /// partner owner accept, leave the agreement waiting a few days, then walk
    /// the male to her (as the client's escort does) until it closes. Returns
    /// the requester's updates.
    #[allow(clippy::type_complexity)]
    fn escorted_mating(
        g: &mut Game,
        a: &[OrgId],
        b: &[OrgId],
    ) -> Option<Vec<(OfferStatus, Option<MatingOutcome>, Option<ErrorInfo>)>> {
        let pick = |g: &Game, ids: &[OrgId], sex| {
            ids.iter()
                .copied()
                .find(|&i| g.world.organism(i).is_some_and(|o| o.matured && o.sex == sex && o.pregnancy.is_none()))
        };
        let (mine, partner) = (pick(g, a, Sex::Male)?, pick(g, b, Sex::Female)?);
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine, partner, price: Some(25) });
        assert_eq!(result(&out), (true, None));
        let id = out
            .iter()
            .find_map(|(_, m)| if let ServerMsg::MatingRequest { offer } = m { Some(offer.id) } else { None })
            .unwrap();
        let updates = |out: &Outbox| -> Vec<_> {
            out.iter()
                .filter_map(|(t, m)| match (t, m) {
                    (Target::Player(1), ServerMsg::MatingUpdate { offer, outcome, error }) => {
                        Some((offer.status, *outcome, error.clone()))
                    }
                    _ => None,
                })
                .collect()
        };
        let mut log = Vec::new();
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        log.extend(updates(&out));
        // Accepted: she must not conceive with one of her own while the
        // agreement waits for the two to meet.
        for _ in 0..4 * g.world.config.ticks_per_day {
            if !g.offers.contains_key(&id) {
                return Some(log);
            }
            g.world.step();
            log.extend(updates(&g.after_tick()));
        }
        g.command(10, 1, None, ClientMsg::Possess { organism: mine });
        for t in 0..20 * g.world.config.ticks_per_day {
            if !g.offers.contains_key(&id) {
                break;
            }
            if t % 10 == 0 {
                if let Some(p) = g.world.organism(partner).map(|o| o.pos) {
                    g.command(10, 1, None, ClientMsg::OrderMove { organism: mine, x: p.x, y: p.y });
                }
            }
            g.world.step();
            log.extend(updates(&g.after_tick()));
        }
        Some(log)
    }

    #[test]
    fn cross_player_agreement_survives_the_wait_and_completes_when_they_meet() {
        // Two players' similar designs in a wild world, as in the e2e test.
        let ann = design("Smoke").with(Gene::BodySize, 0.45).with(Gene::Fur, 0.4).with(Gene::Nose, 0.3).founders(20);
        let bob = design("Bob").founders(20);
        let mut completed = 0;
        for seed in 0..5 {
            let world =
                build_world(&WorldSettings { seed, width: 64, height: 64, days_per_year: 480, ..Default::default() });
            let mut g = Game::new(world, GameConfig::default());
            let a = g.found(1, &ann).unwrap().founders;
            let b = g.found(2, &bob).unwrap().founders;
            g.connect(10, 1, "ann");
            g.connect(20, 2, "bob");
            let log = escorted_mating(&mut g, &a, &b).expect("a pair of founders");
            let last = log.last().unwrap_or_else(|| panic!("seed {seed}: no update"));
            match last {
                (OfferStatus::Completed, Some(o), None) => {
                    assert!(o.fertility > 0.5, "seed {seed}: {o:?}");
                    completed += 1;
                }
                // Predators don't wait for agreements.
                (OfferStatus::Failed, None, Some(e)) if e.code == "died" => {}
                other => panic!("seed {seed}: agreement ended {other:?} (all updates: {log:?})"),
            }
        }
        assert!(completed >= 3, "{completed}/5 completed");
    }

    #[test]
    fn different_body_plans_are_refused_at_request_time_with_the_genes() {
        let (mut g, a, _) = game();
        let newts = FounderDesign::new("C", Thermo::Cold, BirthType::AbandonedEggs, 2).founders(6);
        let c = g.found(3, &newts).unwrap().founders;
        g.connect(30, 3, "cat");
        let ma = male(&g, &a);
        let fc = female(&g, &c);
        let out = g.command(10, 1, None, ClientMsg::PreviewMating { mine: ma, partner: fc });
        let data = match &out[0].1 {
            ServerMsg::Result { ok: true, data: Some(d), .. } => {
                serde_json::from_value::<MatingPreviewView>(d.clone()).unwrap()
            }
            m => panic!("{m:?}"),
        };
        assert!(!data.compatible);
        assert_eq!(data.fertility, 0.0);
        let genes: Vec<&str> = data.body_plan_gaps.iter().map(|g| g.gene.as_str()).collect();
        assert_eq!(genes, ["endothermy", "birth_mode"]);
        assert_eq!(data.problem.as_ref().map(|p| p.code.as_str()), Some("incompatible_body_plans"));
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: ma, partner: fc, price: None });
        let err = match &out[0].1 {
            ServerMsg::Result { ok: false, error: Some(e), .. } => e.clone(),
            m => panic!("{m:?}"),
        };
        assert_eq!(err.code, "incompatible_body_plans");
        assert!(err.message.contains("endothermy"), "{}", err.message);
        assert_eq!(err.detail.unwrap()["genes"][0]["gene"], "endothermy");
        assert_eq!(out.len(), 1, "the other player is not asked");
        assert_eq!(g.world.mating_slots_left(1), g.world.config.mating_slots_per_season);
    }

    #[test]
    fn preview_of_a_compatible_pair() {
        let (mut g, a, b) = game();
        let (fa, mb) = (female(&g, &a), male(&g, &b));
        let out = g.command(10, 1, None, ClientMsg::PreviewMating { mine: fa, partner: mb });
        let data = match &out[0].1 {
            ServerMsg::Result { ok: true, data: Some(d), .. } => {
                serde_json::from_value::<MatingPreviewView>(d.clone()).unwrap()
            }
            m => panic!("{m:?}"),
        };
        assert!(data.compatible && data.problem.is_none());
        assert!(data.fertility > 0.9 && data.body_plan_gaps.is_empty(), "{data:?}");
        assert_eq!(data.cost, 0.0);
        assert_eq!(data.meeting_distance, MEETING_DISTANCE);
        let out = g.command(20, 2, None, ClientMsg::PreviewMating { mine: fa, partner: mb });
        assert_eq!(result(&out).1.as_deref(), Some("not_owned"));
    }

    #[test]
    fn only_an_accepted_offer_holds_the_partner_female() {
        let (mut g, a, b) = game();
        let (ma, fb) = (male(&g, &a), female(&g, &b));
        let ready = |g: &Game| g.world.organism(fb).unwrap().breed_ready_tick;
        g.world.organisms.iter_mut().find(|o| o.id == fb).unwrap().breed_ready_tick = 0;
        // Ann asks for Bob's female; Bob doesn't answer. A request alone
        // must not freeze her: she keeps breeding naturally.
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: ma, partner: fb, price: None });
        assert_eq!(result(&out), (true, None));
        let id = out
            .iter()
            .find_map(|(_, m)| if let ServerMsg::MatingRequest { offer } = m { Some(offer.id) } else { None })
            .unwrap();
        for _ in 0..5 {
            g.after_tick();
        }
        assert_eq!(g.offers[&id].status, OfferStatus::Pending);
        assert_eq!(ready(&g), 0, "a pending, unanswered offer doesn't hold the partner");
        assert!(g.world.organism(fb).unwrap().can_breed(g.world.tick()) || !g.world.organism(fb).unwrap().matured);
        // Bob accepts while the two are apart: from now on she waits for Ann's male.
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        assert_eq!(result(&out), (true, None));
        assert_eq!(g.offers[&id].status, OfferStatus::Accepted);
        assert!(ready(&g) > g.world.tick(), "held from the moment of acceptance");
        for _ in 0..3 {
            g.world.step();
            g.after_tick();
            let f = g.world.organism(fb).unwrap();
            assert!(!f.can_breed(g.world.tick()) && f.pregnancy.is_none(), "an accepted offer holds her");
        }
        // Ann cancels: the hold lapses within a couple of ticks.
        g.command(10, 1, None, ClientMsg::CancelMating { request: id });
        assert!(!g.offers.get(&id).is_some_and(|o| o.status.is_open()));
        let tick = g.world.tick();
        assert!(ready(&g) <= tick + 2);
    }

    fn offer_id(out: &Outbox) -> u64 {
        out.iter()
            .find_map(|(_, m)| match m {
                ServerMsg::MatingRequest { offer } => Some(offer.id),
                ServerMsg::Result { data: Some(d), .. } => d["offer"]["id"].as_u64(),
                _ => None,
            })
            .expect("an offer")
    }

    fn data(out: &Outbox) -> serde_json::Value {
        match &out[0].1 {
            ServerMsg::Result { ok: true, data: Some(d), .. } => d.clone(),
            m => panic!("expected data, got {m:?}"),
        }
    }

    fn ledger_kinds(g: &Game, p: PlayerId) -> Vec<LedgerKind> {
        g.ledger(p, 100).into_iter().rev().map(|e| e.kind).collect()
    }

    fn trades(g: &mut Game) -> Vec<TradeRecord> {
        g.take_persist()
            .into_iter()
            .filter_map(|e| if let PersistEvent::Trade(t) = e { Some(t) } else { None })
            .collect()
    }

    #[test]
    fn founding_grant_daily_income_and_the_ledger() {
        let (mut g, _, _) = game();
        let grant = economy::FOUNDING_GRANT;
        assert_eq!((g.balance(1), g.balance(2), g.balance(3)), (grant, grant, 0));
        let persisted = g.take_persist();
        assert!(persisted.iter().any(|e| matches!(e, PersistEvent::Balance(b) if b.player == 1 && b.balance == grant)));
        assert!(persisted.iter().any(
            |e| matches!(e, PersistEvent::Ledger(r) if r.entry.kind == LedgerKind::Grant && r.world_id == g.world_id)
        ));
        run_days(&mut g, 2);
        // Each day pays the stipend plus one per point of mean legacy share.
        let days: Vec<&DailyRecord> = g.history().collect();
        assert_eq!(days.len(), 2);
        let expected: u64 = days
            .iter()
            .map(|r| {
                let e = r.legacy.iter().find(|e| e.player == 1).unwrap();
                economy::daily_income(e.share, e.weighted_share, true)
            })
            .sum();
        assert!(expected >= 2 * economy::DAILY_STIPEND);
        assert_eq!(g.balance(1), grant + expected);
        assert_eq!(ledger_kinds(&g, 1), vec![LedgerKind::Grant, LedgerKind::Income, LedgerKind::Income]);
        let entries = g.ledger(1, 100);
        assert!(entries.windows(2).all(|w| w[0].seq > w[1].seq), "newest first, increasing sequence numbers");
        assert_eq!(entries[0].balance, g.balance(1));
        assert!(entries[0].text.contains("income"), "{}", entries[0].text);
        // A re-founding after a restart doesn't pay the grant again.
        assert!(g.economy.granted.contains(&1));
        // The ledger command and the state carry it.
        let out = g.command(10, 1, None, ClientMsg::Ledger { limit: Some(2) });
        let v: LedgerView = serde_json::from_value(data(&out)).unwrap();
        assert_eq!((v.balance, v.escrow, v.entries.len()), (g.balance(1), 0, 2));
        assert_eq!(g.state_for(10).unwrap().balance, g.balance(1));
    }

    #[test]
    fn a_directed_price_is_escrowed_and_paid_when_the_mating_runs() {
        let (mut g, a, b) = game();
        let (fa, mb) = (female(&g, &a), male(&g, &b));
        let start = economy::FOUNDING_GRANT;
        // More than Ann has: refused, nothing asked.
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: Some(start + 1) });
        assert_eq!(result(&out).1.as_deref(), Some("insufficient_funds"));
        assert_eq!(out.len(), 1);
        assert_eq!(g.balance(1), start);
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: Some(30) });
        let id = offer_id(&out);
        // The partner's owner is asked: worth a push.
        let asked = Notice {
            player: 2,
            kind: PushKind::Mating,
            text: format!("ann asks to mate their #{fa} with your #{mb} for 30 renown."),
            organism: Some(mb),
        };
        assert_eq!(g.take_notices(), vec![asked]);
        assert_eq!((g.balance(1), g.escrow(1)), (start - 30, 30));
        assert_eq!(g.state_for(10).unwrap().escrow, 30);
        assert_eq!(g.state_for(10).unwrap().offers[0].escrow, 30);
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        assert_eq!(g.escrow(1), 30, "still held while they are apart");
        place_together(&mut g, fa, mb);
        let out = g.after_tick();
        assert!(out.iter().any(
            |(_, m)| matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Completed)
        ));
        assert_eq!((g.balance(1), g.escrow(1), g.balance(2)), (start - 30, 0, start + 30));
        let bob_alert = alerts_in(&out, 2).into_iter().find(|x| x.kind == AlertKind::Market).expect("sale alert");
        assert!(bob_alert.text.starts_with("Sale: 30 renown from ann"), "{}", bob_alert.text);
        assert!(alerts_in(&out, 1).iter().any(|x| x.kind == AlertKind::Market && x.text.starts_with("Paid 30")));
        // The sale is news to the seller; the buyer's payment is not.
        let notices = g.take_notices();
        assert!(notices.iter().any(|n| n.player == 2 && n.kind == PushKind::Market && n.text == bob_alert.text));
        assert!(notices.iter().all(|n| n.player != 1), "{notices:?}");
        assert_eq!(ledger_kinds(&g, 1), vec![LedgerKind::Grant, LedgerKind::Escrow]);
        assert_eq!(ledger_kinds(&g, 2), vec![LedgerKind::Grant, LedgerKind::Sale]);
        let t = trades(&mut g);
        assert_eq!(
            (t[0].price, t[0].settlement.as_deref(), t[0].status.as_str()),
            (Some(30), Some("paid"), "completed")
        );
    }

    #[test]
    fn escrow_is_refunded_on_decline_cancel_death_and_expiry() {
        let world = build_world(&WorldSettings {
            seed: 3,
            width: 64,
            height: 64,
            wild: false,
            days_per_year: 8,
            lod: false,
            catastrophes: false,
        });
        let mut g = Game::new(world, GameConfig::default());
        let a = g.found(1, &design("A")).unwrap().founders;
        let b = g.found(2, &design("B")).unwrap().founders;
        g.connect(10, 1, "ann");
        g.connect(20, 2, "bob");
        let (fa, mb) = (female(&g, &a), male(&g, &b));
        let start = g.balance(1);
        let ask = |g: &mut Game| {
            offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: Some(10) }))
        };
        // Declined.
        let id = ask(&mut g);
        assert_eq!(g.balance(1), start - 10);
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: false });
        assert_eq!(g.balance(1), start);
        assert!(alerts_in(&out, 1)
            .iter()
            .any(|x| x.kind == AlertKind::Market && x.text.contains("Refund: 10") && x.text.contains("declined")));
        // Cancelled by the requester, and by the partner's owner after accepting.
        let id = ask(&mut g);
        g.command(10, 1, None, ClientMsg::CancelMating { request: id });
        assert_eq!(g.balance(1), start);
        let id = ask(&mut g);
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        g.command(20, 2, None, ClientMsg::CancelMating { request: id });
        assert_eq!((g.balance(1), g.balance(2)), (start, start), "bob is paid nothing");
        // Died on the way.
        let id = ask(&mut g);
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        let o = g.world.organisms.iter_mut().find(|o| o.id == mb).unwrap();
        o.alive = false;
        o.death = Some(DeathCause::Predation);
        g.after_tick();
        assert_eq!(g.balance(1), start);
        // Free requests take nothing and settle nothing.
        let (ma, fb) = (male(&g, &a), female(&g, &b));
        let id = offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: ma, partner: fb, price: Some(0) }));
        assert_eq!(g.offers[&id].escrow, 0);
        g.command(10, 1, None, ClientMsg::CancelMating { request: id });
        // Expired at the season change.
        let id = offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: ma, partner: fb, price: Some(10) }));
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        assert_eq!(g.balance(1), start - 10);
        let season = g.world.season();
        let mut ticks = 0;
        while g.world.season() == season {
            g.world.step();
            g.after_tick();
            ticks += 1;
            assert!(ticks < 10 * g.world.config.ticks_per_day, "no season change");
        }
        assert!(!g.offers.contains_key(&id));
        assert_eq!(g.escrow(1), 0);
        let refunds = g.ledger(1, 100).iter().filter(|e| e.kind == LedgerKind::Refund).count();
        assert_eq!(refunds, 5);
        let income: u64 =
            g.ledger(1, 100).iter().filter(|e| e.kind == LedgerKind::Income).map(|e| e.delta as u64).sum();
        assert_eq!(g.balance(1), start + income, "every hold came back");
        let t = trades(&mut g);
        let settled: Vec<_> = t.iter().map(|t| (t.status.as_str(), t.settlement.as_deref())).collect();
        assert_eq!(
            settled,
            vec![
                ("declined", Some("refunded")),
                ("cancelled", Some("refunded")),
                ("cancelled", Some("refunded")),
                ("failed", Some("refunded")),
                ("cancelled", None),
                ("expired", Some("refunded"))
            ]
        );
    }

    #[test]
    fn listing_lifecycle_purchase_and_payment() {
        let (mut g, a, b) = game();
        let (ma, fb) = (male(&g, &a), female(&g, &b));
        let start = economy::FOUNDING_GRANT;
        // Errors first.
        let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: fb, fee: 5, expires_days: None });
        assert_eq!(result(&out).1.as_deref(), Some("not_owned"));
        let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: ma, fee: 5, expires_days: Some(0) });
        assert_eq!(result(&out).1.as_deref(), Some("bad_expiry"));
        let out = g.command(
            10,
            1,
            None,
            ClientMsg::CreateListing { organism: ma, fee: economy::MAX_PRICE + 1, expires_days: None },
        );
        assert_eq!(result(&out).1.as_deref(), Some("bad_price"));
        let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: ma, fee: 12, expires_days: Some(30) });
        let listing: ListingView = serde_json::from_value(data(&out)["listing"].clone()).unwrap();
        assert_eq!(
            (listing.role.as_str(), listing.fee, listing.owner_name.as_str(), listing.sales),
            ("stud", 12, "ann", 0)
        );
        assert_eq!(listing.expires_tick, Some(g.world.tick() + 30 * g.world.config.ticks_per_day as u64));
        assert_eq!(listing.traits.len(), LISTING_TRAITS.len());
        assert!(listing.traits.iter().any(|t| t.gene == "body_size"));
        let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: ma, fee: 1, expires_days: None });
        assert_eq!(result(&out).1.as_deref(), Some("already_listed"));
        // Bob browses with his female: compatibility is shown; Ann sees no preview of her own.
        let out = g.command(20, 2, None, ClientMsg::BrowseMarket(MarketQuery { mine: Some(fb), ..Default::default() }));
        let m: MarketView = serde_json::from_value(data(&out)).unwrap();
        assert_eq!((m.balance, m.listings.len()), (start, 1));
        let p = m.listings[0].preview.as_ref().expect("preview");
        assert!(p.compatible && p.mine == fb && p.partner == ma);
        let out =
            g.command(10, 1, None, ClientMsg::BrowseMarket(MarketQuery { mine: Some(a[0]), ..Default::default() }));
        assert!(serde_json::from_value::<MarketView>(data(&out)).unwrap().listings[0].preview.is_none());
        // Ann can't buy her own; Bob can't buy with a same-sex organism or what he can't pay.
        let out = g.command(10, 1, None, ClientMsg::BuyListing { listing: listing.id, mine: a[0] });
        assert_eq!(result(&out).1.as_deref(), Some("own_listing"));
        let mb = male(&g, &b);
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing: listing.id, mine: mb });
        assert_eq!(result(&out).1.as_deref(), Some("same_sex"));
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing: 99, mine: fb });
        assert_eq!(result(&out).1.as_deref(), Some("unknown_listing"));
        // Buy: an accepted agreement at once, the fee in escrow, Ann told.
        let slots = g.world.mating_slots_left(2);
        let out = g.command(20, 2, Some(7), ClientMsg::BuyListing { listing: listing.id, mine: fb });
        assert_eq!(result(&out), (true, None));
        let id = offer_id(&out);
        assert_eq!(g.offers[&id].status, OfferStatus::Accepted);
        assert_eq!(
            (g.offers[&id].listing, g.offers[&id].escrow, g.offers[&id].from, g.offers[&id].to),
            (Some(listing.id), 12, 2, 1)
        );
        assert_eq!((g.balance(2), g.escrow(2)), (start - 12, 12));
        let sold = alerts_in(&out, 1).into_iter().find(|x| x.kind == AlertKind::Market).expect("seller alert");
        assert!(sold.text.starts_with("bob bought a mating with your stud"), "{}", sold.text);
        assert!(out.iter().any(|(t, m)| *t == Target::Player(1)
            && matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Accepted)));
        assert_eq!(g.economy.listings[&listing.id].sales, 1);
        // The buyer's female waits for the stud (the listing is consent).
        assert!(g.world.organism(fb).unwrap().breed_ready_tick > g.world.tick());
        // A stud may serve several buyers: the listing stays open (but the
        // same buyer's organism can't be promised twice).
        let m: MarketView =
            serde_json::from_value(data(&g.command(20, 2, None, ClientMsg::BrowseMarket(MarketQuery::default()))))
                .unwrap();
        assert!(!m.listings[0].busy);
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing: listing.id, mine: fb });
        assert_eq!(result(&out).1.as_deref(), Some("organism_busy"));
        assert_eq!(g.balance(2), start - 12);
        // They meet: the mating runs on the buyer's slot and Ann is paid.
        place_together(&mut g, ma, fb);
        let out = g.after_tick();
        assert!(out.iter().any(|(_, m)| matches!(m, ServerMsg::MatingUpdate { offer, outcome: Some(_), .. } if offer.status == OfferStatus::Completed)));
        assert_eq!(g.world.mating_slots_left(2), slots - 1);
        assert_eq!(
            g.world.mating_slots_left(1),
            g.world.config.mating_slots_per_season,
            "the lister's slots are untouched"
        );
        assert_eq!((g.balance(1), g.balance(2), g.escrow(2)), (start + 12, start - 12, 0));
        let t = trades(&mut g);
        assert_eq!((t[0].listing, t[0].settlement.as_deref()), (Some(listing.id), Some("paid")));
        // Withdraw: only the owner.
        let out = g.command(20, 2, None, ClientMsg::WithdrawListing { listing: listing.id });
        assert_eq!(result(&out).1.as_deref(), Some("unknown_listing"));
        let out = g.command(10, 1, None, ClientMsg::WithdrawListing { listing: listing.id });
        assert_eq!(result(&out), (true, None));
        assert!(g.market(2, &MarketQuery::default()).unwrap().listings.is_empty());
    }

    #[test]
    fn a_listed_dam_takes_one_buyer_and_listings_close_on_expiry_and_death() {
        let (mut g, a, b) = game();
        let c = g.found(3, &design("C")).unwrap().founders;
        g.connect(30, 3, "cat");
        let (fa, mb, mc) = (female(&g, &a), male(&g, &b), male(&g, &c));
        let l = data(&g.command(10, 1, None, ClientMsg::CreateListing { organism: fa, fee: 0, expires_days: None }))
            ["listing"]["id"]
            .as_u64()
            .unwrap();
        assert_eq!(g.market(2, &MarketQuery::default()).unwrap().listings[0].role, "dam");
        // Listed, she is kept from breeding on her own.
        for _ in 0..3 {
            g.world.step();
            g.after_tick();
            assert!(!g.world.organism(fa).unwrap().can_breed(g.world.tick()), "a listed dam is held");
        }
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing: l, mine: mb });
        assert_eq!(result(&out), (true, None));
        let id = offer_id(&out);
        assert_eq!(g.offers[&id].escrow, 0, "a free listing holds nothing");
        assert!(g.market(3, &MarketQuery::default()).unwrap().listings[0].busy);
        let out = g.command(30, 3, None, ClientMsg::BuyListing { listing: l, mine: mc });
        assert_eq!(result(&out).1.as_deref(), Some("listing_busy"));
        // Bob gives up: she is free for the next buyer.
        g.command(20, 2, None, ClientMsg::CancelMating { request: id });
        let out = g.command(30, 3, None, ClientMsg::BuyListing { listing: l, mine: mc });
        assert_eq!(result(&out), (true, None));
        // Insufficient funds for a listing.
        let ma = male(&g, &a);
        let pricey =
            data(&g.command(10, 1, None, ClientMsg::CreateListing { organism: ma, fee: 1000, expires_days: Some(1) }))
                ["listing"]["id"]
                .as_u64()
                .unwrap();
        let fb = female(&g, &b);
        // (She may have conceived with one of her own meanwhile.)
        g.world.organisms.iter_mut().find(|o| o.id == fb).unwrap().pregnancy = None;
        let before = g.balance(2);
        assert!(before < 1000);
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing: pricey, mine: fb });
        assert_eq!(result(&out).1.as_deref(), Some("insufficient_funds"));
        assert_eq!(g.balance(2), before);
        // A day later the one-day listing has expired.
        let out = run_days(&mut g, 1);
        assert!(!g.economy.listings.contains_key(&pricey));
        assert!(alerts_in(&out, 1).iter().any(|x| x.kind == AlertKind::Market
            && x.text.contains(&format!("Listing #{pricey}"))
            && x.text.contains("expired")));
        // A listed organism that dies closes its listing.
        if g.economy.listings.contains_key(&l) {
            let o = g.world.organisms.iter_mut().find(|o| o.id == fa).unwrap();
            o.alive = false;
            o.death = Some(DeathCause::Predation);
            let out = g.after_tick();
            assert!(!g.economy.listings.contains_key(&l));
            assert!(alerts_in(&out, 1).iter().any(|x| x.text.contains("no longer alive")));
        }
        // Too many listings.
        let ids: Vec<OrgId> =
            g.world.organisms.iter().filter(|o| o.alive && o.owner == 1 && o.matured).map(|o| o.id).collect();
        let mut refused = None;
        for id in ids {
            let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: id, fee: 1, expires_days: None });
            if let (false, Some(code)) = result(&out) {
                if code != "already_listed" {
                    refused = Some(code);
                    break;
                }
            }
        }
        if g.economy.listings_of(1) >= economy::MAX_LISTINGS {
            assert_eq!(refused.as_deref(), Some("too_many_listings"));
        }
    }

    fn browse(g: &mut Game, conn: ConnId, player: PlayerId, q: MarketQuery) -> MarketView {
        serde_json::from_value(data(&g.command(conn, player, None, ClientMsg::BrowseMarket(q)))).unwrap()
    }

    /// Every page of a query, following `next_cursor`.
    fn all_pages(g: &mut Game, conn: ConnId, player: PlayerId, q: MarketQuery) -> (Vec<ListingView>, usize) {
        let mut out: Vec<ListingView> = Vec::new();
        let mut q = q;
        let mut pages = 0;
        loop {
            let m = browse(g, conn, player, q.clone());
            pages += 1;
            assert!(m.listings.len() <= q.limit.unwrap_or(MARKET_PAGE));
            out.extend(m.listings);
            match m.next_cursor {
                Some(c) => q.cursor = Some(c),
                None => {
                    assert_eq!(out.len(), m.total, "the pages hold every match once");
                    return (out, pages);
                }
            }
            assert!(pages < 100);
        }
    }

    /// Four players with every founder listed: 24 listings, fees by order.
    fn market_game() -> (Game, Vec<Vec<OrgId>>) {
        let (mut g, a, b) = game();
        let c = g.found(3, &design("C")).unwrap().founders;
        let d = g.found(4, &design("D")).unwrap().founders;
        g.connect(30, 3, "cat");
        g.connect(40, 4, "dan");
        let all = vec![a, b, c, d];
        let mut fee = 0;
        for (k, ids) in all.iter().enumerate() {
            let p = k as PlayerId + 1;
            for &id in ids {
                fee = (fee * 7 + 3) % 31;
                let out = g.command(
                    p as u64 * 10,
                    p,
                    None,
                    ClientMsg::CreateListing { organism: id, fee, expires_days: None },
                );
                assert_eq!(result(&out), (true, None), "{out:?}");
            }
        }
        (g, all)
    }

    #[test]
    fn browse_market_pages_with_a_stable_cursor_filters_and_sorts() {
        let (mut g, all) = market_game();
        let n = g.economy.listings.len();
        assert_eq!(n, 24);
        // No fields: the newest page of the default size, bounded.
        let m = browse(&mut g, 10, 1, MarketQuery::default());
        assert_eq!((m.listings.len(), m.total), (MARKET_PAGE, n));
        let newest: Vec<u64> = g.economy.listings.keys().rev().copied().collect();
        assert_eq!(m.listings.iter().map(|l| l.id).collect::<Vec<_>>(), newest[..MARKET_PAGE]);
        assert!(m.next_cursor.is_some() && m.own.is_none());
        assert!(m.listings.iter().all(|l| l.kind == ListingKind::Fixed && l.auction.is_none()));
        // Small pages walk every listing once, newest first.
        let (pages, count) = all_pages(&mut g, 10, 1, MarketQuery { limit: Some(5), ..Default::default() });
        assert_eq!((pages.iter().map(|l| l.id).collect::<Vec<_>>(), count), (newest.clone(), 5));
        // The page size is clamped.
        assert_eq!(browse(&mut g, 10, 1, MarketQuery { limit: Some(0), ..Default::default() }).listings.len(), 1);
        let big = browse(&mut g, 10, 1, MarketQuery { limit: Some(10_000), ..Default::default() });
        assert_eq!((big.listings.len(), big.next_cursor), (n.min(MAX_MARKET_PAGE), None));
        // A cursor stays valid while listings come and go: a listing made
        // after the first page (newer) and one withdrawn from the second
        // neither repeat nor shift what follows.
        let first = browse(&mut g, 30, 3, MarketQuery { limit: Some(6), ..Default::default() });
        let gone = newest[8];
        let owner = g.economy.listings[&gone].owner;
        let organism = g.economy.listings[&gone].organism;
        let out = g.command(owner as u64 * 10, owner, None, ClientMsg::WithdrawListing { listing: gone });
        assert_eq!(result(&out), (true, None));
        let relisted = ClientMsg::CreateListing { organism, fee: 1, expires_days: None };
        assert_eq!(result(&g.command(owner as u64 * 10, owner, None, relisted)), (true, None));
        let second = browse(
            &mut g,
            30,
            3,
            MarketQuery { limit: Some(6), cursor: first.next_cursor.clone(), ..Default::default() },
        );
        let expect: Vec<u64> = newest[6..].iter().copied().filter(|&id| id != gone).take(6).collect();
        assert_eq!(second.listings.iter().map(|l| l.id).collect::<Vec<_>>(), expect);
        // A cursor not made by the server is refused.
        for bad in ["", "12", "a.b", "1.2.3", "-1.4"] {
            let q = MarketQuery { cursor: Some(bad.into()), ..Default::default() };
            let out = g.command(10, 1, None, ClientMsg::BrowseMarket(q));
            assert_eq!(result(&out).1.as_deref(), Some("bad_cursor"), "{bad:?}");
        }
        let n = g.economy.listings.len();
        // Own listings: left out of the page with `hide_own`, all of them in
        // `own` with `with_own`.
        let mine_count = g.economy.listings_of(1);
        let m = browse(&mut g, 10, 1, MarketQuery { hide_own: true, with_own: true, ..Default::default() });
        assert_eq!(m.total, n - mine_count);
        assert!(m.listings.iter().all(|l| l.owner != 1));
        let own = m.own.unwrap();
        assert_eq!(own.len(), mine_count);
        assert!(own.iter().all(|l| l.owner == 1));
        // Price order, both ways, across pages.
        let (asc, _) = all_pages(
            &mut g,
            10,
            1,
            MarketQuery { limit: Some(7), sort: Some(MarketSort::PriceAsc), ..Default::default() },
        );
        assert!(asc.windows(2).all(|w| w[0].fee < w[1].fee || (w[0].fee == w[1].fee && w[0].id > w[1].id)));
        let (desc, _) = all_pages(
            &mut g,
            10,
            1,
            MarketQuery { limit: Some(7), sort: Some(MarketSort::PriceDesc), ..Default::default() },
        );
        assert!(desc.windows(2).all(|w| w[0].fee >= w[1].fee));
        assert_eq!(asc.len(), n);
        // Filters: role, price, lineage, species, kind.
        let (dams, _) = all_pages(&mut g, 10, 1, MarketQuery { role: Some(ListingRole::Dam), ..Default::default() });
        assert!(!dams.is_empty() && dams.iter().all(|l| l.role == "dam" && l.sex == "female"));
        let (studs, _) = all_pages(&mut g, 10, 1, MarketQuery { role: Some(ListingRole::Stud), ..Default::default() });
        assert_eq!(dams.len() + studs.len(), n);
        let (cheap, _) = all_pages(&mut g, 10, 1, MarketQuery { max_price: Some(10), ..Default::default() });
        assert_eq!(cheap.len(), g.economy.listings.values().filter(|l| l.fee <= 10).count());
        let bob = g.lineage_of(2).unwrap();
        let (bobs, _) = all_pages(&mut g, 10, 1, MarketQuery { lineage: Some(bob), ..Default::default() });
        assert!(!bobs.is_empty() && bobs.iter().all(|l| l.lineage == bob && l.owner == 2));
        assert_eq!(browse(&mut g, 10, 1, MarketQuery { species: Some(9_999), ..Default::default() }).total, 0);
        let q = MarketQuery { kind: Some(ListingKind::Auction), ..Default::default() };
        assert_eq!(browse(&mut g, 10, 1, q.clone()).total, 0);
        // Compatibility: with `mine`, the page carries previews; `compatible`
        // keeps only pairs that can conceive (never one's own listings), and
        // `fertility` puts the best first.
        let mine = all[0][0];
        let q = MarketQuery { mine: Some(mine), compatible: true, ..Default::default() };
        let (compat, _) = all_pages(&mut g, 10, 1, MarketQuery { limit: Some(4), ..q });
        assert!(!compat.is_empty());
        assert!(compat.iter().all(|l| l.owner != 1 && l.preview.as_ref().is_some_and(|p| p.compatible)));
        let q =
            MarketQuery { mine: Some(mine), sort: Some(MarketSort::Fertility), limit: Some(4), ..Default::default() };
        let (fert, _) = all_pages(&mut g, 10, 1, q);
        assert_eq!(fert.len(), n);
        let f = |l: &ListingView| l.preview.as_ref().filter(|p| p.compatible).map_or(-1.0, |p| p.fertility);
        assert!(fert.windows(2).all(|w| f(&w[0]) >= f(&w[1]) - 1e-6), "best fertility first");
        // The page's previews come with any order.
        let m = browse(&mut g, 10, 1, MarketQuery { mine: Some(mine), hide_own: true, ..Default::default() });
        let sex = sex_name(g.world.organism(mine).unwrap().sex);
        assert!(m.listings.iter().any(|l| l.sex != sex));
        assert!(m.listings.iter().filter(|l| l.sex != sex).all(|l| l.preview.is_some()));
    }

    fn not_pregnant(g: &mut Game, ids: &[OrgId]) {
        for o in g.world.organisms.iter_mut().filter(|o| ids.contains(&o.id)) {
            o.pregnancy = None;
        }
    }

    fn auction_records(g: &mut Game) -> Vec<AuctionRecord> {
        g.take_persist()
            .into_iter()
            .filter_map(|e| if let PersistEvent::Auction(r) = e { Some(r) } else { None })
            .collect()
    }

    fn tick(g: &mut Game) -> Outbox {
        g.world.step();
        g.after_tick()
    }

    /// Keep two organisms apart (so an agreement waits for them to meet).
    fn place_apart(g: &mut Game, a: OrgId, b: OrgId) {
        let pa = g.world.organism(a).unwrap().pos;
        let far = Vec2::new((pa.x + 30.0) % 60.0 + 2.0, (pa.y + 30.0) % 60.0 + 2.0);
        g.world.organisms.iter_mut().find(|o| o.id == b).unwrap().pos = far;
    }

    #[test]
    fn auction_bids_are_escrowed_outbid_bidders_refunded_and_the_winner_settles() {
        let (mut g, a, b) = game();
        let c = g.found(3, &design("C")).unwrap().founders;
        g.connect(30, 3, "cat");
        let (ma, fb, fc) = (male(&g, &a), female(&g, &b), female(&g, &c));
        not_pregnant(&mut g, &[fb, fc]);
        let start = economy::FOUNDING_GRANT;
        let create = |g: &mut Game, reserve, days, ticks| {
            g.command(10, 1, None, ClientMsg::CreateAuction { organism: ma, reserve, days, ticks })
        };
        for (days, ticks) in
            [(Some(1), Some(10)), (Some(0), None), (Some(economy::MAX_AUCTION_DAYS + 1), None), (None, Some(0))]
        {
            assert_eq!(result(&create(&mut g, 10, days, ticks)).1.as_deref(), Some("bad_duration"));
        }
        assert_eq!(result(&create(&mut g, economy::MAX_PRICE + 1, None, Some(5))).1.as_deref(), Some("bad_price"));
        let out =
            g.command(20, 2, None, ClientMsg::CreateAuction { organism: ma, reserve: 1, days: None, ticks: None });
        assert_eq!(result(&out).1.as_deref(), Some("not_owned"));
        let out = create(&mut g, 10, None, Some(6));
        let l: ListingView = serde_json::from_value(data(&out)["listing"].clone()).unwrap();
        let deadline = g.world.tick() + 6;
        assert_eq!(l.kind, ListingKind::Auction);
        let view = l.auction.clone().unwrap();
        assert_eq!(
            (view.reserve, view.deadline_tick, view.bids, view.min_bid, view.high_bid),
            (10, deadline, 0, 10, None)
        );
        assert_eq!(l.expires_tick, Some(deadline));
        // A day-long auction by default, and one listing per organism.
        assert_eq!(result(&create(&mut g, 10, None, None)).1.as_deref(), Some("already_listed"));
        let id = l.id;
        let bid = |g: &mut Game, conn, player, amount, mine| {
            g.command(conn, player, None, ClientMsg::Bid { listing: id, amount, mine })
        };
        // The seller can't bid; an auction can't be bought outright; a
        // fixed listing takes no bids; bids start at the reserve.
        assert_eq!(result(&bid(&mut g, 10, 1, 50, a[0])).1.as_deref(), Some("own_listing"));
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing: id, mine: fb });
        assert_eq!(result(&out).1.as_deref(), Some("auction_listing"));
        let fa = female(&g, &a);
        let fixed =
            data(&g.command(10, 1, None, ClientMsg::CreateListing { organism: fa, fee: 3, expires_days: None }))
                ["listing"]["id"]
                .as_u64()
                .unwrap();
        let mb = male(&g, &b);
        let out = g.command(20, 2, None, ClientMsg::Bid { listing: fixed, amount: 5, mine: mb });
        assert_eq!(result(&out).1.as_deref(), Some("not_auction"));
        let out = bid(&mut g, 20, 2, 9, fb);
        assert_eq!(result(&out).1.as_deref(), Some("bid_too_low"));
        assert_eq!(g.balance(2), start);
        assert_eq!(result(&bid(&mut g, 20, 2, 10, mb)).1.as_deref(), Some("same_sex"));
        assert_eq!(result(&bid(&mut g, 20, 2, start + 1, fb)).1.as_deref(), Some("insufficient_funds"));
        // Bob bids the reserve: held in escrow, Ann told.
        let out = bid(&mut g, 20, 2, 10, fb);
        assert_eq!(result(&out), (true, None));
        let v: ListingView = serde_json::from_value(data(&out)["listing"].clone()).unwrap();
        let av = v.auction.unwrap();
        assert_eq!((av.high_bid, av.high_bidder, av.bids, av.min_bid), (Some(10), Some(2), 1, 11));
        assert_eq!((g.balance(2), g.escrow(2)), (start - 10, 10));
        assert_eq!(g.state_for(20).unwrap().escrow, 10, "the state counts bids as escrow");
        assert!(alerts_in(&out, 1).iter().any(|x| x.text.starts_with("bob bid 10 renown on your auction")));
        // The highest bidder's female is held for the settlement.
        tick(&mut g);
        assert!(!g.world.organism(fb).unwrap().can_breed(g.world.tick()), "the bidder's dam is held");
        // With a bid, the auction can't be withdrawn.
        let out = g.command(10, 1, None, ClientMsg::WithdrawListing { listing: id });
        assert_eq!(result(&out).1.as_deref(), Some("auction_has_bids"));
        // Cat must beat it; then Bob is refunded at once and told.
        assert_eq!(result(&bid(&mut g, 30, 3, 10, fc)).1.as_deref(), Some("bid_too_low"));
        let (b2, b3) = (g.balance(2), g.balance(3));
        let out = bid(&mut g, 30, 3, 11, fc);
        assert_eq!(result(&out), (true, None));
        assert_eq!((g.balance(2), g.escrow(2)), (b2 + 10, 0));
        assert_eq!((g.balance(3), g.escrow(3)), (b3 - 11, 11));
        assert!(alerts_in(&out, 2)
            .iter()
            .any(|x| x.kind == AlertKind::Market && x.text.starts_with("Outbid on auction")));
        let bob: Vec<LedgerKind> = ledger_kinds(&g, 2).into_iter().filter(|&k| k != LedgerKind::Income).collect();
        assert_eq!(bob, vec![LedgerKind::Grant, LedgerKind::Escrow, LedgerKind::Refund]);
        assert!(g.ledger(2, 1)[0].listing == Some(id) && g.ledger(2, 1)[0].offer.is_none());
        // Cat raises her own bid: only the difference is held.
        let out = bid(&mut g, 30, 3, 20, fc);
        assert_eq!(result(&out), (true, None));
        assert_eq!((g.balance(3), g.escrow(3)), (b3 - 20, 20));
        assert_eq!(g.ledger(3, 1)[0].delta, -9);
        // The deadline: Cat's bid becomes an accepted agreement with the bid
        // in escrow on it; the auction is recorded as sold.
        place_apart(&mut g, ma, fc);
        auction_records(&mut g);
        let mut settled = Outbox::new();
        while g.world.tick() < deadline {
            assert!(g.economy.listings.contains_key(&id));
            settled = tick(&mut g);
            place_apart(&mut g, ma, fc);
        }
        assert!(!g.economy.listings.contains_key(&id));
        let offer = g.offers.values().find(|o| o.listing == Some(id)).expect("the winner's agreement").clone();
        assert_eq!(
            (offer.from, offer.to, offer.mine, offer.partner, offer.escrow, offer.status),
            (3, 1, fc, ma, 20, OfferStatus::Accepted)
        );
        // The bid moved from the auction to the agreement: released from
        // the one, held on the other.
        assert_eq!(g.escrow(3), 20);
        let moves: Vec<(LedgerKind, i64, Option<u64>)> =
            g.ledger(3, 100).iter().filter(|e| e.listing == Some(id)).map(|e| (e.kind, e.delta, e.offer)).collect();
        assert_eq!(
            moves,
            vec![
                (LedgerKind::Escrow, -20, Some(offer.id)),
                (LedgerKind::Refund, 20, None),
                (LedgerKind::Escrow, -9, None),
                (LedgerKind::Escrow, -11, None)
            ]
        );
        assert!(alerts_in(&settled, 3).iter().any(|x| x.text.starts_with(&format!("Won auction #{id} with 20"))));
        assert!(alerts_in(&settled, 1).iter().any(|x| x.text.contains("sold to cat for 20")));
        let rec = auction_records(&mut g);
        assert_eq!(rec.len(), 1);
        assert_eq!(
            (rec[0].outcome.as_str(), rec[0].winner, rec[0].high_bid, rec[0].bids, rec[0].offer, rec[0].reserve),
            ("sold", Some(3), Some(20), 3, Some(offer.id), 10)
        );
        assert_eq!(rec[0].world_id, g.world_id);
        // They meet: Ann is paid the winning bid.
        let (b1, b3) = (g.balance(1), g.balance(3));
        place_together(&mut g, ma, fc);
        g.after_tick();
        assert!(!g.offers.contains_key(&offer.id));
        assert_eq!((g.balance(1), g.balance(3), g.escrow(3)), (b1 + 20, b3, 0));
        let t = trades(&mut g);
        assert_eq!((t[0].listing, t[0].price, t[0].settlement.as_deref()), (Some(id), Some(20), Some("paid")));
    }

    #[test]
    fn auctions_without_a_bid_return_to_the_seller_and_deaths_refund_the_bid() {
        let (mut g, a, b) = game();
        let (ma, fa, fb, mb) = (male(&g, &a), female(&g, &a), female(&g, &b), male(&g, &b));
        not_pregnant(&mut g, &[fa, fb]);
        let auction = |g: &mut Game, organism, ticks| {
            g.command(10, 1, None, ClientMsg::CreateAuction { organism, reserve: 5, days: None, ticks: Some(ticks) })
        };
        // No bid: the lot is the seller's again at the deadline.
        let l = data(&auction(&mut g, ma, 2))["listing"]["id"].as_u64().unwrap();
        auction_records(&mut g);
        let mut out = Outbox::new();
        for _ in 0..2 {
            out.extend(tick(&mut g));
        }
        assert!(!g.economy.listings.contains_key(&l));
        assert!(alerts_in(&out, 1).iter().any(|x| x.text.contains("closed without a bid")));
        assert_eq!(auction_records(&mut g).iter().map(|r| r.outcome.as_str()).collect::<Vec<_>>(), vec!["unsold"]);
        assert!(g.ledger(1, 100).iter().all(|e| e.listing.is_none()), "no bid: no renown moved");
        // Withdrawn before any bid.
        let l = data(&auction(&mut g, ma, 50))["listing"]["id"].as_u64().unwrap();
        assert_eq!(result(&g.command(10, 1, None, ClientMsg::WithdrawListing { listing: l })), (true, None));
        assert_eq!(auction_records(&mut g)[0].outcome, "withdrawn");
        // The lot dies with a bid on it: the bidder is refunded.
        let l = data(&auction(&mut g, ma, 50))["listing"]["id"].as_u64().unwrap();
        let b2 = g.balance(2);
        assert_eq!(result(&g.command(20, 2, None, ClientMsg::Bid { listing: l, amount: 7, mine: fb })), (true, None));
        assert_eq!(g.balance(2), b2 - 7);
        let o = g.world.organisms.iter_mut().find(|o| o.id == ma).unwrap();
        o.alive = false;
        o.death = Some(DeathCause::Predation);
        let out = g.after_tick();
        assert!(!g.economy.listings.contains_key(&l));
        assert_eq!((g.balance(2), g.escrow(2)), (b2, 0));
        assert!(alerts_in(&out, 2).iter().any(|x| x.text.contains("refunded")));
        assert_eq!(auction_records(&mut g)[0].outcome, "ended");
        // The winner's organism dies before the deadline: no agreement, the
        // bid refunded, the auction recorded as failed.
        let mb2 = mb;
        let fa2 = a.iter().copied().find(|&id| id != fa && g.world.organism(id).is_some_and(|o| o.sex == Sex::Female));
        let Some(dam) = fa2 else { return };
        g.world.organisms.iter_mut().find(|o| o.id == dam).unwrap().pregnancy = None;
        let l = data(&auction(&mut g, dam, 3))["listing"]["id"].as_u64().unwrap();
        assert_eq!(result(&g.command(20, 2, None, ClientMsg::Bid { listing: l, amount: 9, mine: mb2 })), (true, None));
        let o = g.world.organisms.iter_mut().find(|o| o.id == mb2).unwrap();
        o.alive = false;
        o.death = Some(DeathCause::Starvation);
        let mut out = Outbox::new();
        for _ in 0..3 {
            out.extend(tick(&mut g));
        }
        assert!(!g.economy.listings.contains_key(&l));
        assert!(!g.offers.values().any(|o| o.listing == Some(l)));
        assert_eq!(g.escrow(2), 0);
        let moves: Vec<i64> = g.ledger(2, 100).iter().filter(|e| e.listing == Some(l)).map(|e| e.delta).collect();
        assert_eq!(moves, vec![9, -9], "refunded");
        assert!(alerts_in(&out, 2).iter().any(|x| x.text.contains("can't run") && x.text.contains("refunded")));
        let rec = auction_records(&mut g);
        assert_eq!((rec[0].outcome.as_str(), rec[0].winner, rec[0].high_bid), ("failed", None, Some(9)));
    }

    #[test]
    fn saved_games_keep_open_auctions_and_their_escrowed_bids() {
        let (mut g, a, b) = game();
        let (ma, fb) = (male(&g, &a), female(&g, &b));
        not_pregnant(&mut g, &[fb]);
        let l = data(&g.command(
            10,
            1,
            None,
            ClientMsg::CreateAuction { organism: ma, reserve: 4, days: None, ticks: Some(3) },
        ))["listing"]["id"]
            .as_u64()
            .unwrap();
        assert_eq!(result(&g.command(20, 2, None, ClientMsg::Bid { listing: l, amount: 6, mine: fb })), (true, None));
        let json = serde_json::to_string(&g.saved()).unwrap();
        let saved: SavedGame = serde_json::from_str(&json).unwrap();
        assert_eq!(saved.version, SAVED_GAME_VERSION);
        let world = World::restore(&g.world.snapshot(), Some(g.world.map.terrain().clone())).unwrap();
        let mut r = Game::resume(world, GameConfig::default(), saved, Vec::new());
        assert_eq!(r.economy, g.economy);
        assert_eq!((r.balance(2), r.escrow(2)), (g.balance(2), 6));
        assert_eq!(r.ledger(2, 1)[0].delta, -6);
        let a = r.economy.listings[&l].auction.clone().unwrap();
        assert_eq!((a.reserve, a.bids, a.high.map(|b| (b.bidder, b.amount, b.mine))), (4, 1, Some((2, 6, fb))));
        // It settles in the restored game.
        r.connect(10, 1, "ann");
        r.connect(20, 2, "bob");
        for _ in 0..3 {
            place_apart(&mut r, ma, fb);
            tick(&mut r);
        }
        assert!(!r.economy.listings.contains_key(&l));
        let offer = r.offers.values().find(|o| o.listing == Some(l)).expect("agreement").clone();
        assert_eq!((offer.from, offer.escrow), (2, 6));
        assert_eq!(r.escrow(2), 6);
    }

    #[test]
    fn saved_games_of_layout_2_and_3_still_restore_without_auctions() {
        let (mut g, a, _) = game();
        let (ma, fa) = (male(&g, &a), female(&g, &a));
        not_pregnant(&mut g, &[fa]);
        let fixed = ClientMsg::CreateListing { organism: ma, fee: 7, expires_days: None };
        assert_eq!(result(&g.command(10, 1, None, fixed)), (true, None));
        run_days(&mut g, 1);
        let json: serde_json::Value = serde_json::to_value(g.saved()).unwrap();
        assert_eq!(json["version"], SAVED_GAME_VERSION);
        let season = serde_json::to_value(&g.season).unwrap();
        // Layout 3 (ranked seasons, no auctions) and layout 2 (neither).
        let mut v3 = json.clone();
        v3["version"] = 3.into();
        let mut v2 = v3.clone();
        v2["version"] = 2.into();
        v2.as_object_mut().unwrap().remove("season");
        v2.as_object_mut().unwrap().remove("last_season_record");
        for (layout, v) in [(3, v3), (2, v2)] {
            let saved: SavedGame = serde_json::from_value(v).unwrap();
            let world = World::restore(&g.world.snapshot(), Some(g.world.map.terrain().clone())).unwrap();
            let mut r = Game::resume(world, GameConfig::default(), saved, Vec::new());
            assert!(r.economy_restored(), "layout {layout}");
            assert_eq!(r.economy, g.economy, "layout {layout}");
            assert!(r.economy.listings.values().all(|l| l.auction.is_none()));
            if layout == 3 {
                assert_eq!(serde_json::to_value(&r.season).unwrap(), season, "the season tracker is kept");
            }
            // Auctions work in the restored game.
            not_pregnant(&mut r, &[fa]);
            let out = r.command(
                10,
                1,
                None,
                ClientMsg::CreateAuction { organism: fa, reserve: 2, days: None, ticks: Some(5) },
            );
            assert_eq!(result(&out), (true, None), "layout {layout}");
            assert_eq!(r.saved().version, SAVED_GAME_VERSION);
        }
    }

    #[test]
    fn a_fresh_start_refunds_bids_held_on_auctions_of_the_discarded_world_once() {
        let (mut g, a, b) = game();
        let c = g.found(3, &design("C")).unwrap().founders;
        g.connect(30, 3, "cat");
        let (ma, fa, fb, fc, mb) = (male(&g, &a), female(&g, &a), female(&g, &b), female(&g, &c), male(&g, &b));
        not_pregnant(&mut g, &[fa, fb, fc]);
        let create = |g: &mut Game, organism| {
            data(&g.command(10, 1, None, ClientMsg::CreateAuction { organism, reserve: 4, days: Some(3), ticks: None }))
                ["listing"]["id"]
                .as_u64()
                .unwrap()
        };
        let (one, two) = (create(&mut g, ma), create(&mut g, fa));
        assert!(result(&g.command(20, 2, None, ClientMsg::Bid { listing: one, amount: 10, mine: fb })).0);
        let mut ledger = ledger_records(&mut g);
        let saved: SavedGame = serde_json::from_str(&serde_json::to_string(&g.saved()).unwrap()).unwrap();
        // After the save: Cat outbids Bob (Bob refunded) and raises; Bob
        // bids on the other auction.
        assert!(result(&g.command(30, 3, None, ClientMsg::Bid { listing: one, amount: 12, mine: fc })).0);
        assert!(result(&g.command(30, 3, None, ClientMsg::Bid { listing: one, amount: 15, mine: fc })).0);
        assert!(result(&g.command(20, 2, None, ClientMsg::Bid { listing: two, amount: 5, mine: mb })).0);
        ledger.extend(ledger_records(&mut g));
        let stored = [(1, g.balance(1)), (2, g.balance(2)), (3, g.balance(3))];
        let mut n = Game::new(build_world(&WorldSettings { seed: 9, ..Default::default() }), GameConfig::default());
        n.seed_economy(&stored, Vec::new());
        let mut refunds = n.refund_discarded(&saved, &ledger);
        refunds.sort_unstable();
        assert_eq!(refunds, vec![(2, 5), (3, 15)]);
        assert_eq!((n.balance(2), n.balance(3)), (economy::FOUNDING_GRANT, economy::FOUNDING_GRANT));
        ledger.extend(ledger_records(&mut n));
        let mut again = Game::new(build_world(&WorldSettings { seed: 9, ..Default::default() }), GameConfig::default());
        again.seed_economy(&[(2, n.balance(2)), (3, n.balance(3))], Vec::new());
        assert!(again.refund_discarded(&saved, &ledger).is_empty());
    }

    #[test]
    fn saved_games_keep_the_economy_and_older_ones_fall_back() {
        let (mut g, a, b) = game();
        let (ma, fb) = (male(&g, &a), female(&g, &b));
        let l = data(&g.command(10, 1, None, ClientMsg::CreateListing { organism: ma, fee: 7, expires_days: None }))
            ["listing"]["id"]
            .as_u64()
            .unwrap();
        let id = offer_id(&g.command(20, 2, None, ClientMsg::BuyListing { listing: l, mine: fb }));
        let json = serde_json::to_string(&g.saved()).unwrap();
        let saved: SavedGame = serde_json::from_str(&json).unwrap();
        assert_eq!(saved.version, SAVED_GAME_VERSION);
        let world = World::restore(&g.world.snapshot(), Some(g.world.map.terrain().clone())).unwrap();
        let mut r = Game::resume(world, GameConfig::default(), saved, Vec::new());
        assert!(r.economy_restored());
        assert_eq!((r.balance(1), r.balance(2), r.escrow(2)), (g.balance(1), g.balance(2), 7));
        assert_eq!(r.economy, g.economy);
        assert_eq!(r.ledger(2, 10), g.ledger(2, 10));
        // The escrowed agreement settles in the restored game.
        r.connect(10, 1, "ann");
        r.connect(20, 2, "bob");
        place_together(&mut r, ma, fb);
        r.after_tick();
        assert!(!r.offers.contains_key(&id));
        assert_eq!(r.balance(1), economy::FOUNDING_GRANT + 7);
        // A layout-1 saved game (no version, no economy, offers without
        // escrow): the economy is left for the server to seed from the store.
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        old.as_object_mut().unwrap().remove("version");
        old.as_object_mut().unwrap().remove("economy");
        let saved: SavedGame = serde_json::from_value(old).unwrap();
        let world = World::restore(&g.world.snapshot(), Some(g.world.map.terrain().clone())).unwrap();
        let mut r = Game::resume(world, GameConfig::default(), saved, Vec::new());
        assert!(!r.economy_restored());
        assert_eq!((r.balance(1), r.escrow(2)), (0, 0));
        r.seed_economy(&[(1, 33)], Vec::new());
        assert_eq!(r.balance(1), 33);
        assert!(r.economy_restored());
        // Players with a stored balance don't get the founding grant again;
        // a newcomer does.
        assert!(r.economy.granted.contains(&1) && !r.economy.granted.contains(&4));
        r.found(4, &design("D")).unwrap();
        assert_eq!(r.balance(4), economy::FOUNDING_GRANT);
    }

    #[test]
    fn escrow_settles_once_and_only_the_parties_act_on_an_offer() {
        let (mut g, a, b) = game();
        g.set_name(3, "eve");
        let c = g.found(3, &design("C")).unwrap().founders;
        g.connect(30, 3, "eve");
        let (fa, mb, ma, fb) = (female(&g, &a), male(&g, &b), male(&g, &a), female(&g, &b));
        let start = g.balance(1);
        let code = |out: &Outbox| result(out).1;
        let id = offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: Some(20) }));
        // A stranger can't answer, complete or cancel it; the requester can't
        // accept it for the partner's owner.
        for msg in [
            ClientMsg::RespondMating { request: id, accept: true },
            ClientMsg::CompleteMating { request: id },
            ClientMsg::CancelMating { request: id },
        ] {
            assert_eq!(code(&g.command(30, 3, None, msg)).as_deref(), Some("unknown_offer"));
        }
        let out = g.command(10, 1, None, ClientMsg::RespondMating { request: id, accept: true });
        assert_eq!(code(&out).as_deref(), Some("unknown_offer"));
        // Nor use someone else's organism as "mine".
        let out = g.command(30, 3, None, ClientMsg::RequestMating { mine: fa, partner: c[0], price: None });
        assert_eq!(code(&out).as_deref(), Some("not_owned"));
        // Cancel, then cancel or complete again: refunded exactly once.
        g.command(10, 1, None, ClientMsg::CancelMating { request: id });
        assert_eq!(g.balance(1), start);
        for msg in [ClientMsg::CancelMating { request: id }, ClientMsg::CompleteMating { request: id }] {
            assert_eq!(code(&g.command(10, 1, None, msg)).as_deref(), Some("unknown_offer"));
        }
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        assert_eq!((g.balance(1), g.balance(2)), (start, start));
        assert_eq!(ledger_kinds(&g, 1), vec![LedgerKind::Grant, LedgerKind::Escrow, LedgerKind::Refund]);
        // Completed (paid), then cancelled or declined: nothing moves again.
        let id = offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: Some(20) }));
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        place_together(&mut g, fa, mb);
        g.command(10, 1, None, ClientMsg::CompleteMating { request: id });
        assert_eq!((g.balance(1), g.balance(2)), (start - 20, start + 20));
        g.command(20, 2, None, ClientMsg::CancelMating { request: id });
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: false });
        g.after_tick();
        assert_eq!((g.balance(1), g.balance(2)), (start - 20, start + 20));

        // Prices: above the cap, or more than the balance, are refused and
        // take nothing; a direct escrow the balance can't cover makes no offer.
        let over = economy::MAX_PRICE + 1;
        let out = g.command(20, 2, None, ClientMsg::RequestMating { mine: fb, partner: ma, price: Some(over) });
        assert_eq!(code(&out).as_deref(), Some("bad_price"));
        let out = g.command(20, 2, None, ClientMsg::RequestMating { mine: fb, partner: ma, price: Some(u64::MAX) });
        assert_eq!(code(&out).as_deref(), Some("bad_price"));
        let out = g.command(20, 2, None, ClientMsg::CreateListing { organism: fb, fee: u64::MAX, expires_days: None });
        assert_eq!(code(&out).as_deref(), Some("bad_price"));
        let open = g.offers.len();
        let err = g.open_offer(2, 1, fb, ma, Some(g.balance(2) + 1), OfferStatus::Pending, None).unwrap_err();
        assert_eq!(err.code, "insufficient_funds");
        assert_eq!((g.offers.len(), g.balance(2)), (open, start + 20));
        // Negative or oversized numbers never parse.
        for bad in [
            r#"{"type":"request_mating","mine":1,"partner":2,"price":-5}"#,
            r#"{"type":"request_mating","mine":1,"partner":2,"price":1e30}"#,
            r#"{"type":"request_mating","mine":1,"partner":2,"price":18446744073709551616}"#,
            r#"{"type":"create_listing","organism":1,"fee":-1}"#,
            r#"{"type":"ledger","limit":-1}"#,
        ] {
            assert!(serde_json::from_str::<crate::protocol::ClientEnvelope>(bad).is_err(), "{bad}");
        }

        // Listings: only the owner lists or withdraws; a listed dam takes one
        // buyer at a time; one's own listing can't be bought.
        let out = g.command(30, 3, None, ClientMsg::CreateListing { organism: fb, fee: 5, expires_days: None });
        assert_eq!(code(&out).as_deref(), Some("not_owned"));
        let l = data(&g.command(20, 2, None, ClientMsg::CreateListing { organism: fb, fee: 5, expires_days: None }))
            ["listing"]["id"]
            .as_u64()
            .unwrap();
        let out = g.command(30, 3, None, ClientMsg::WithdrawListing { listing: l });
        assert_eq!(code(&out).as_deref(), Some("unknown_listing"));
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing: l, mine: mb });
        assert_eq!(code(&out).as_deref(), Some("own_listing"));
        let out = g.command(30, 3, None, ClientMsg::BuyListing { listing: l, mine: ma });
        assert_eq!(code(&out).as_deref(), Some("not_owned"), "buying with someone else's organism");
        let (b1, b3) = (g.balance(1), g.balance(3));
        let first = g.command(10, 1, None, ClientMsg::BuyListing { listing: l, mine: ma });
        assert!(result(&first).0, "{first:?}");
        let mc = male(&g, &c);
        let second = g.command(30, 3, None, ClientMsg::BuyListing { listing: l, mine: mc });
        assert!(
            matches!(code(&second).as_deref(), Some("listing_busy" | "already_pregnant")),
            "the dam is promised (or already served): {second:?}"
        );
        assert_eq!(g.balance(3), b3, "a refused purchase takes nothing");
        assert_eq!(g.balance(1), b1 - 5, "the first buyer's fee is held once");
    }

    fn ledger_records(g: &mut Game) -> Vec<LedgerRecord> {
        g.take_persist()
            .into_iter()
            .filter_map(|e| if let PersistEvent::Ledger(r) = e { Some(r) } else { None })
            .collect()
    }

    #[test]
    fn a_fresh_start_refunds_the_escrow_of_the_discarded_world_once() {
        let (mut g, a, b) = game();
        let (fa, mb, ma, fb) = (female(&g, &a), male(&g, &b), male(&g, &a), female(&g, &b));
        let start = g.balance(1);
        let ask = |g: &mut Game, mine, partner, price| {
            offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine, partner, price: Some(price) }))
        };
        let first = ask(&mut g, fa, mb, 10);
        let _second = ask(&mut g, ma, fb, 5);
        let mut ledger = ledger_records(&mut g);
        // The newest saved game holds two open priced offers.
        let saved: SavedGame = serde_json::from_str(&serde_json::to_string(&g.saved()).unwrap()).unwrap();
        // After the save: the first is cancelled (refunded) and a third one
        // is escrowed; the store saw both, the saved game neither.
        g.command(10, 1, None, ClientMsg::CancelMating { request: first });
        let _third = ask(&mut g, fa, mb, 3);
        ledger.extend(ledger_records(&mut g));
        assert_eq!(g.balance(1), start - 5 - 3);
        let stored = [(1, g.balance(1)), (2, g.balance(2))];

        // The new world starts from the stored balances, then refunds what
        // was still held: the second (from the saved game) and the third
        // (from the store), not the first (settled after the save).
        let mut n = Game::new(build_world(&WorldSettings { seed: 9, ..Default::default() }), GameConfig::default());
        n.seed_economy(&stored, Vec::new());
        let refunds = n.refund_discarded(&saved, &ledger);
        assert_eq!(refunds, vec![(1, 5), (1, 3)]);
        assert_eq!((n.balance(1), n.balance(2)), (start, g.balance(2)));
        let new_entries = ledger_records(&mut n);
        assert_eq!(new_entries.len(), 2);
        assert!(new_entries.iter().all(|r| r.entry.kind == LedgerKind::Refund
            && r.entry.offer.is_none()
            && r.world_id == n.world_id
            && r.entry.text.contains(&g.world_id)));
        assert!(new_entries[0].entry.text.starts_with("Refund: 5 renown held on offer #2 of world"));

        // Starting fresh again from the same saved game (the new world never
        // saved) refunds nothing a second time.
        ledger.extend(new_entries);
        let mut again = Game::new(build_world(&WorldSettings { seed: 9, ..Default::default() }), GameConfig::default());
        again.seed_economy(&[(1, n.balance(1))], Vec::new());
        assert!(again.refund_discarded(&saved, &ledger).is_empty());
        assert_eq!(again.balance(1), start);

        // A saved game from before the economy held no escrow.
        let mut old = saved.clone();
        old.version = 1;
        assert!(again.refund_discarded(&old, &[]).is_empty());
    }

    #[test]
    fn accepted_offer_holds_the_female_and_a_death_fails_it_with_the_cause() {
        let (mut g, a, b) = game();
        let (fa, mb) = (female(&g, &a), male(&g, &b));
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: None });
        let id = out
            .iter()
            .find_map(|(_, m)| if let ServerMsg::MatingRequest { offer } = m { Some(offer.id) } else { None })
            .unwrap();
        // Ann's own female is held as soon as Ann asks: that is her consent.
        g.world.step();
        g.after_tick();
        assert_eq!(g.offers[&id].status, OfferStatus::Pending);
        assert!(
            !g.world.organism(fa).unwrap().can_breed(g.world.tick()),
            "the requester's own female is held while pending"
        );
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        g.world.step();
        g.after_tick();
        let tick = g.world.tick();
        assert!(!g.world.organism(fa).unwrap().can_breed(tick), "a promised female doesn't breed on her own");
        // The partner dies while the agreement waits for them to meet.
        let o = g.world.organisms.iter_mut().find(|o| o.id == mb).unwrap();
        o.alive = false;
        o.death = Some(DeathCause::Predation);
        let out = g.after_tick();
        let err = out.iter().find_map(|(t, m)| match m {
            ServerMsg::MatingUpdate { offer, error: Some(e), .. }
                if *t == Target::Player(1) && offer.status == OfferStatus::Failed =>
            {
                Some(e.clone())
            }
            _ => None,
        });
        let err = err.expect("the offer fails");
        assert_eq!(err.code, "died");
        assert!(err.message.contains("was eaten"), "{}", err.message);
        // Once closed, she breeds on her own again.
        g.world.step();
        assert!(
            g.world.organism(fa).unwrap().can_breed(g.world.tick() + 2)
                || g.world.organism(fa).unwrap().pregnancy.is_some()
        );
    }

    /// The state message as it was built before [`StateIndex`]: a scan of
    /// every organism per connection. The indexed build must match it.
    fn state_by_scan(g: &Game, conn: ConnId) -> StateView {
        let c = &g.conns[&conn];
        let player = c.player;
        let tpd = g.world.config.ticks_per_day;
        let alive = || g.world.organisms.iter().filter(|o| o.alive);
        let own = alive()
            .filter(|o| o.owner == player)
            .map(|o| {
                let mut v = OwnOrganism::from_org(o, tpd);
                if o.possessed_by.is_some() {
                    v.speed = Some(g.world.move_speed(o, o.needs.energy > 40.0));
                }
                v
            })
            .collect();
        let view = g.view_of(c);
        let center = Vec2::new(view.x, view.y);
        let mut nearby: Vec<(f32, NearbyOrganism)> = alive()
            .filter(|o| o.owner != player)
            .filter(|o| (o.pos.x - view.x).abs() <= view.radius && (o.pos.y - view.y).abs() <= view.radius)
            .map(|o| (o.pos.dist(center), NearbyOrganism::from_org(o)))
            .collect();
        if nearby.len() > g.cfg.max_nearby {
            nearby.sort_by(|a, b| a.0.total_cmp(&b.0));
            nearby.truncate(g.cfg.max_nearby);
        }
        let offers = (g.offers.values())
            .filter(|o| o.status.is_open() && (o.from == player || o.to == player))
            .map(|o| g.offer_view(o.id))
            .collect();
        StateView {
            clock: g.clock(),
            weather: g.weather(),
            player,
            lineage: g.lineage_of(player),
            slots_left: g.world.mating_slots_left(player),
            population: g.owner_population(player),
            migration: g.world.migration_order(player, 0).map(|p| [p.x, p.y]),
            groups: g.world.groups(player).iter().map(GroupView::from).collect(),
            view,
            organisms: own,
            nearby: nearby.into_iter().map(|(_, n)| n).collect(),
            leaderboard: g.leaderboard(),
            offers,
            balance: g.balance(player),
            escrow: g.escrow(player),
        }
    }

    /// A crowded world: the wild ecosystem and twelve players, each with a
    /// connection, some views pinned (one at the map's edge, one off it),
    /// one possessed organism and an offer.
    fn crowded(cfg: GameConfig) -> Game {
        let world = build_world(&WorldSettings { seed: 3, width: 96, height: 96, wild: true, ..Default::default() });
        let mut g = Game::new(world, cfg);
        for p in 1..=12u32 {
            g.set_name(p, &format!("p{p}"));
            g.found(p, &design(&format!("L{p}"))).unwrap();
            g.connect(p as u64 * 10, p, &format!("p{p}"));
        }
        g.world.run_days(2);
        let _ = g.after_tick();
        let views = [(3u32, 0.0, 0.0, 20.0), (4, 95.9, 95.9, 32.0), (5, 120.0, -10.0, 32.0), (6, 48.0, 48.0, 8.0)];
        for (p, x, y, radius) in views {
            g.command(p as u64 * 10, p, None, ClientMsg::SetView { x, y, radius });
        }
        let first = |g: &Game, p: u32| g.world.organisms.iter().find(|o| o.alive && o.owner == p).unwrap().id;
        let org = first(&g, 7);
        assert!(result(&g.command(70, 7, None, ClientMsg::Possess { organism: org })).0);
        let (m, p) = (first(&g, 8), first(&g, 9));
        g.command(80, 8, None, ClientMsg::RequestMating { mine: m, partner: p, price: None });
        g
    }

    #[test]
    fn indexed_states_match_a_scan_of_the_world() {
        for max_nearby in [256, 5] {
            let cfg = GameConfig { max_nearby, state_leaderboard_rows: usize::MAX, ..Default::default() };
            let mut g = crowded(cfg);
            for round in 0..3 {
                let states = g.states();
                assert_eq!(states.len(), 12);
                for (target, msg) in states {
                    let (Target::Conn(c), ServerMsg::State(s)) = (target, msg) else { panic!("not a state") };
                    assert_eq!(*s, state_by_scan(&g, c), "connection {c}, round {round}, max_nearby {max_nearby}");
                    assert_eq!(g.state_for(c).as_ref(), Some(&*s));
                }
                g.world.step();
                let _ = g.after_tick();
            }
            // Someone sees foreign organisms, and the cap applies.
            let s = g.state_for(60).unwrap();
            assert!(!s.nearby.is_empty() && s.nearby.len() <= max_nearby);
            // The possessor's sub-step states match too.
            let org = g.state_for(70).unwrap().organisms.iter().find(|o| o.possessed).map(|o| o.id).unwrap();
            g.command(70, 7, None, ClientMsg::OrderMove { organism: org, x: 1.0, y: 1.0 });
            // The prediction fields: the order and speed of the possessed one.
            let s = g.state_for(70).unwrap();
            assert_eq!(s, state_by_scan(&g, 70));
            let p = s.organisms.iter().find(|o| o.id == org).unwrap();
            assert_eq!(p.order, Some([1.0, 1.0]));
            assert!(p.speed.is_some_and(|v| v > 0.0));
            assert!(s.organisms.iter().filter(|o| o.id != org).all(|o| o.order.is_none() && o.speed.is_none()));
            for (target, msg) in g.substep() {
                let (Target::Conn(c), ServerMsg::State(s)) = (target, msg) else { panic!("not a state") };
                assert_eq!(c, 70);
                assert_eq!(*s, state_by_scan(&g, c));
            }
        }
    }

    #[test]
    fn a_long_leaderboard_is_cut_to_the_top_rows_plus_mine_and_neighbors() {
        let g = crowded(GameConfig { state_leaderboard_rows: 3, ..Default::default() });
        let full = g.leaderboard();
        assert!(full.rows.len() > 3);
        for &conn in g.conns.keys() {
            let s = g.state_for(conn).unwrap();
            let rows: Vec<PlayerId> = s.leaderboard.rows.iter().map(|r| r.player).collect();
            let mut owners: Vec<PlayerId> = s.nearby.iter().map(|n| n.owner).collect();
            owners.push(s.player);
            // In rank order: a subsequence of the full board.
            let mut it = full.rows.iter().map(|r| r.player);
            assert!(rows.iter().all(|p| it.any(|q| q == *p)), "{rows:?}");
            for (rank, r) in full.rows.iter().enumerate() {
                let wanted = rank < 3 || owners.contains(&r.player);
                assert_eq!(rows.contains(&r.player), wanted, "player {} at rank {rank}", r.player);
            }
            assert_eq!((s.leaderboard.day, s.leaderboard.breeding_adults), (full.day, full.breeding_adults));
        }
        // A board that fits is sent whole.
        let g = crowded(GameConfig::default());
        assert_eq!(g.state_for(10).unwrap().leaderboard, g.leaderboard());
    }

    fn seasons_in(g: &mut Game) -> Vec<SeasonRecord> {
        g.take_persist()
            .into_iter()
            .filter_map(|e| if let PersistEvent::Season(r) = e { Some(*r) } else { None })
            .collect()
    }

    /// A ranked season ends with the first daily record past it: its
    /// standings are frozen once, players hear where they placed, and the
    /// next season starts where it ended.
    #[test]
    fn ranked_seasons_roll_over_once() {
        let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
        let mut g = Game::new(world, GameConfig { season_days: 2, ..Default::default() });
        g.set_name(1, "ann");
        g.set_name(2, "bob");
        let a = g.found(1, &design("A")).unwrap().founders;
        let b = g.found(2, &design("B")).unwrap().founders;
        g.connect(10, 1, "ann");
        // A stud fee paid during the season counts for the market board.
        let (ma, fb) = (male(&g, &a), female(&g, &b));
        let l = data(&g.command(10, 1, None, ClientMsg::CreateListing { organism: ma, fee: 7, expires_days: None }))
            ["listing"]["id"]
            .as_u64()
            .unwrap();
        g.connect(20, 2, "bob");
        offer_id(&g.command(20, 2, None, ClientMsg::BuyListing { listing: l, mine: fb }));
        place_together(&mut g, ma, fb);
        run_days(&mut g, 2);
        assert!(seasons_in(&mut g).is_empty(), "days 0 and 1 are season 1");
        let cur = g.current_season(0, 10, None);
        assert_eq!((cur.season, cur.days, cur.days_left, cur.first_day, cur.last_day), (1, 2, 0, 0, 1));
        g.world.step();
        let out = g.after_tick();
        let done = seasons_in(&mut g);
        assert_eq!(done.len(), 1);
        let r = &done[0];
        assert_eq!((r.head.season, r.head.days, r.head.first_day, r.head.last_day), (1, 2, 0, 1));
        assert_eq!(r.head.world_id, g.world_id);
        let legacy = &r.standings[0];
        assert_eq!(legacy.category, league::Category::Legacy);
        assert_eq!(legacy.rows.len(), 2);
        let market = &r.standings[3];
        assert_eq!(market.rows.iter().map(|p| (p.player, p.value)).collect::<Vec<_>>(), vec![(1, 7.0)]);
        assert!(r.standings[1].rows.iter().all(|p| p.value >= 6.0), "founders count for the lineage board");
        assert!(alerts_in(&out, 1).iter().any(|x| x.text.starts_with("Season 1 has ended: you placed #")));
        let notices = g.take_notices();
        for p in [1, 2] {
            let ended =
                |n: &&Notice| n.player == p && n.kind == PushKind::Season && n.text.starts_with("Season 1 has ended");
            assert!(notices.iter().any(|n| ended(&n)), "{notices:?}");
        }
        assert_eq!(g.last_season(), Some(r));
        // Season 2 started with the record that ended season 1.
        let cur = g.current_season(0, 10, Some(1));
        assert_eq!((cur.season, cur.first_day, cur.last_day, cur.days, cur.days_left), (2, 2, 3, 1, 1));
        assert!(cur.standings[3].rows.is_empty(), "the market starts again");
        run_days(&mut g, 2);
        let done = seasons_in(&mut g);
        assert_eq!(done.iter().map(|r| (r.head.season, r.head.days)).collect::<Vec<_>>(), vec![(2, 2)]);
    }

    /// A world restored from a save before the rollover replays it into the
    /// same record (the store replaces it); one restored from the save after
    /// it writes the finished record again and counts nothing twice.
    #[test]
    fn a_restart_around_a_rollover_neither_loses_nor_double_counts() {
        let cfg = GameConfig { season_days: 2, ..Default::default() };
        let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
        let mut g = Game::new(world, cfg.clone());
        g.set_name(1, "ann");
        g.found(1, &design("A")).unwrap();
        run_days(&mut g, 2);
        g.take_persist();
        let before = (g.world.snapshot(), serde_json::to_string(&g.saved()).unwrap());
        g.world.step();
        g.after_tick();
        let original = seasons_in(&mut g).pop().expect("rolled over");
        let after = (g.world.snapshot(), serde_json::to_string(&g.saved()).unwrap());
        let terrain = Some(g.world.map.terrain().clone());

        // Restored from the save before: the replayed rollover gives the same standings.
        let world = World::restore(&before.0, terrain.clone()).unwrap();
        let mut r = Game::resume(world, cfg.clone(), serde_json::from_str(&before.1).unwrap(), Vec::new());
        r.set_name(1, "ann");
        assert!(seasons_in(&mut r).is_empty(), "nothing had finished yet");
        r.world.step();
        r.after_tick();
        let replayed = seasons_in(&mut r).pop().expect("rolled over again");
        assert_eq!(replayed.key(), original.key());
        assert_eq!(replayed.standings, original.standings);
        assert_eq!(replayed.head.days, original.head.days);

        // Restored from the save after: the record is written again, the
        // next season goes on from its first day.
        let world = World::restore(&after.0, terrain).unwrap();
        let mut r = Game::resume(world, cfg.clone(), serde_json::from_str(&after.1).unwrap(), Vec::new());
        assert_eq!(seasons_in(&mut r), vec![original.clone()]);
        let cur = r.current_season(0, 10, None);
        assert_eq!((cur.season, cur.days), (2, 1));
        run_days(&mut r, 2);
        let next = seasons_in(&mut r);
        assert_eq!(next.iter().map(|x| (x.head.season, x.head.days)).collect::<Vec<_>>(), vec![(2, 2)]);

        // A saved game from before ranked seasons starts season 1 at its next day.
        let mut old: serde_json::Value = serde_json::from_str(&after.1).unwrap();
        old.as_object_mut().unwrap().remove("season");
        old.as_object_mut().unwrap().remove("last_season_record");
        let world = World::restore(&after.0, Some(g.world.map.terrain().clone())).unwrap();
        let r = Game::resume(world, cfg, serde_json::from_value(old).unwrap(), Vec::new());
        let cur = r.current_season(0, 10, None);
        assert_eq!((cur.season, cur.first_day, cur.days), (1, 3, 0));
    }

    /// World-thread cost of possession sub-steps and state pushes with many
    /// steering players, for the README (`cargo test --release -p evo-server
    /// --lib game::tests::substep_cost -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn substep_cost() {
        let players = 500u32;
        let world = build_world(&WorldSettings { seed: 5, width: 512, height: 512, wild: true, ..Default::default() });
        let mut g = Game::new(world, GameConfig::default());
        for p in 1..=players {
            g.set_name(p, &format!("p{p}"));
            g.found(p, &design(&format!("L{p}"))).unwrap();
            g.connect(p as u64, p, &format!("p{p}"));
        }
        g.world.run_days(3);
        let _ = g.after_tick();
        let (mut subs, mut pushes) = (Vec::new(), Vec::new());
        for round in 0..12u32 {
            // Every player steers its first organism somewhere far.
            for p in 1..=players {
                let Some(o) = g.world.organisms.iter().find(|o| o.alive && o.owner == p) else { continue };
                let (id, pos) = (o.id, o.pos);
                g.command(p as u64, p, None, ClientMsg::Possess { organism: id });
                let (x, y) = ((pos.x + 40.0 + round as f32) % 512.0, (pos.y + 70.0) % 512.0);
                g.command(p as u64, p, None, ClientMsg::OrderMove { organism: id, x, y });
            }
            let t = std::time::Instant::now();
            let n = g.states().len();
            pushes.push(t.elapsed().as_secs_f64() * 1e3);
            for _ in 0..3 {
                let t = std::time::Instant::now();
                let m = g.substep().len();
                subs.push(t.elapsed().as_secs_f64() * 1e3);
                assert!(m > players as usize / 2 && n == players as usize, "{m} {n}");
            }
            g.world.step();
            let _ = g.after_tick();
        }
        let median = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        eprintln!(
            "{} organisms, {players} steering players: state push {:.2} ms, sub-step {:.2} ms (medians)",
            g.world.alive_count(),
            median(&mut pushes),
            median(&mut subs)
        );
    }
}
