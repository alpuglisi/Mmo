//! Game state around the [`World`]: player names, founded lineages,
//! directed-mating offers, connected views. Pure logic with no channels or
//! I/O, so command routing and the consent flow are unit-testable; the world
//! thread ([`crate::runner`]) owns a `Game` and moves its output to sockets.
//!
//! Every method that changes the world is called by the world thread
//! between ticks, in the order requests arrived.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use evo_sim::chronicle::{AchievementKind, Catastrophe, CatastropheKind};
use evo_sim::design::{DesignError, DESIGN_BUDGET};
use evo_sim::math::Vec2;
use evo_sim::organism::{DeathCause, Dormancy, Organism, Sex};
use evo_sim::presets::{self, REFERENCE_SIZE};
use evo_sim::terrain::Season;
use evo_sim::world::commands::{CommandError, DirectedMating, MatingPreview, MEETING_DISTANCE};
use evo_sim::world::groups::{ClaimOutcome, FAR_CLAIM_TILES, MAX_GROUPS};
use evo_sim::{FounderDesign, Gene, PlayerId, World, WorldConfig, WILD};
use serde::{Deserialize, Serialize};

use crate::design::gene_key;
use crate::economy::{self, Bid, Economy, Insufficient, Listing};
use crate::league::{self, CurrentSeason, SeasonRecord, SeasonTracker};
use crate::protocol::*;
mod eras;
mod family;
mod market;
mod offline;
#[cfg(test)]
mod orders_tests;
mod shop;
mod spectate;
mod states;

pub use offline::{
    home_group, HomeClaim, OfflineTracker, Spell, DEFAULT_HOME_RANGE_FADE_TICKS, DEFAULT_HOME_RANGE_HOURS,
    DEFAULT_HOME_RANGE_ONSET_TICKS, DEFAULT_HOME_RANGE_TICKS, DEFAULT_PROTECTION_HOURS, DEFAULT_PROTECTION_TICKS,
    HOME_RANGE_ONSET_MINUTES,
};
pub use spectate::{Spectate, SpectateLineage, PLAYER_CELL, SPECTATE_CELL};

use crate::store::{AuctionRecord, BalanceRecord, DailyRecord, LedgerRecord, LegacyEntry, LineageEntry, TradeRecord};

/// Daily records kept in memory for the genetics dashboard (thinned with
/// age as the store thins them: [`crate::history`]).
pub const HISTORY_DAYS: usize = crate::history::WINDOW_DAYS as usize;
/// Alerts kept per player.
pub const MAX_ALERTS: usize = 50;
/// Thirst or hunger at which a possessed organism raises a `need` alert.
pub const NEED_ALERT: f32 = 75.0;
/// A need alert is raised again only once the need has fallen this far below `NEED_ALERT`.
const NEED_EASED: f32 = 25.0;
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
    /// Ticks after a logout during which the home range shields at full
    /// strength; 0 turns home ranges off (`game::offline`).
    pub home_range_ticks: u64,
    /// Ticks offline before the home range applies.
    pub home_range_onset_ticks: u64,
    /// Ticks over which the home range then fades to nothing.
    pub home_range_fade_ticks: u64,
    /// Milliseconds a tick stands for in wall-clock time (the runner's
    /// tick; 1000 when it runs as fast as it can), to show the protection
    /// left in minutes.
    pub tick_ms: f64,
    /// A player's founders start fed with full fat reserves when founded
    /// within `WINTER_PROVISION_DAYS` before winter (or during it). Off with
    /// `--no-winter-provision`. Only new foundings change; a restored world
    /// runs as before until someone founds.
    pub winter_provision: bool,
    /// Ticks an accepted agreement (a bought listing, a won auction, an
    /// accepted request) waits for the requester's organism to meet the
    /// partner before it lapses: the dam is freed and the requester
    /// forfeits [`LAPSE_FEE_PERCENT`] of the price (or a mating slot when
    /// nothing was paid). [`PURCHASE_DEADLINE_MINUTES`] of wall-clock time
    /// (30 minutes at the default 1 s tick; `main.rs` scales it by the tick).
    pub purchase_deadline_ticks: u64,
    /// Ticks a pending request waits for an answer before it expires
    /// (refunded): [`OFFER_MINUTES`] of wall-clock time. Offers used to all
    /// expire at the next weather-season change (2.4 minutes into a season
    /// at a 100 ms tick), accepted agreements too.
    pub offer_ticks: u64,
    /// The sender is warned this many ticks before a pending request
    /// expires ([`OFFER_WARN_MINUTES`]).
    pub offer_warn_ticks: u64,
    /// The window of the per-target request limit ([`REQUESTS_PER_TARGET`]):
    /// [`REQUEST_WINDOW_MINUTES`] of wall-clock time (an hour at the default
    /// 1 s tick). Only requests still unanswered or declined count:
    /// accepted, expired, cancelled and failed ones don't.
    pub request_window_ticks: u64,
    /// Shortest auction, in ticks: [`MIN_AUCTION_MINUTES`] of wall-clock
    /// time (a 1-tick auction closed before anyone could bid).
    pub min_auction_ticks: u64,
    /// Shortest a rally stands, in minutes of wall-clock time (at
    /// `tick_ms`): the sim's `RALLY_DAYS` pass in seconds at a fast tick.
    pub rally_min_minutes: f64,
    /// Shortest the newcomer grace lasts, in minutes of wall-clock time (at
    /// `tick_ms`): `Balance::attack_grace` days pass in under two minutes at
    /// a fast tick.
    pub grace_min_minutes: f64,
    /// Longest the newcomer grace lasts in game days, real-time minimum
    /// included: at a fast tick 30 minutes would be hundreds of days.
    pub grace_max_days: f64,
    /// A player hears at most once in this many minutes of wall-clock time
    /// that one attacker lineage is hunting their animals.
    pub attack_alert_minutes: f64,
    /// Shortest wait between a group's territory claims (abandoning in
    /// between changes nothing), in minutes of wall-clock time: the sim's
    /// half day passes in seconds at a fast tick.
    pub claim_cooldown_min_minutes: f64,
    /// Ticks without a connection before a player's offline protection
    /// starts ([`OFFLINE_IDLE_MINUTES`]; 0: at once). Playtest 3: an API
    /// player making short connections was "away" all the time, protected
    /// while giving orders, and told "While you were away…" each time.
    pub offline_idle_ticks: u64,
}

/// Fewer drinking-water tiles than this in and around a territory or
/// rally circle draw a warning in the reply ([`Game::circle_report`]).
pub const LOW_WATER_TILES: usize = 3;
/// A circle whose standing plants would feed its group for fewer days than
/// this draws a warning.
pub const STANDING_WARN_DAYS: f32 = 10.0;
/// A territory or rally circle farther than this from its group's middle
/// draws a warning (playtest 3: a claim 85 tiles off).
pub const FAR_CIRCLE_TILES: f32 = 30.0;
/// A territory whose standing plants would feed its group for fewer days
/// than this is grazed bare (the daily check's alert).
pub const BARE_DAYS: f32 = 3.0;
/// A group whose median member is farther than this many home radii from
/// the home its migration gave it has drifted (an alert).
pub const DRIFT_FACTOR: f32 = 2.0;
/// A migration route crossing at least this many tiles of open water draws
/// a warning.
pub const ROUTE_WATER_TILES: usize = 3;
/// A migration given while at least this share of the group is dormant
/// draws a warning.
pub const DORMANT_WARN_SHARE: f32 = 0.3;

/// Whether a "species founded" achievement of `player` for `species` on
/// `day` earns its renown bonus (and a season count), recording it if so.
/// Not for a re-detection: a lineage whose earlier species of this player
/// merged back and was detected again doesn't pay twice (it drifted back and
/// forth across the threshold, no speciation). And at most once a year per
/// lineage (`dpy` days). Both used to pay every time (+100 renown each).
fn species_bonus_due(
    t: &evo_sim::species::SpeciesTracker,
    paid: &mut BTreeMap<u32, u64>,
    species: u32,
    player: PlayerId,
    day: u64,
    dpy: u64,
) -> bool {
    let Some(lineage) = t.get(species).and_then(|s| s.main_lineage()) else {
        return false;
    };
    let cooling = paid.get(&lineage).is_some_and(|&d| day < d + dpy);
    if species_redetected(t, species, player) || cooling {
        return false;
    }
    paid.insert(lineage, day);
    true
}

/// Whether `species`, credited to `player`, is a re-detection: an earlier
/// species of that player from the same lineage merged back, and this one
/// is it drifting across the threshold again, not a speciation. Worlds from
/// snapshot version 42 re-use the merged species' identity instead (and
/// confirm splits first), so this only catches older worlds' records; such
/// achievements earn nothing and raise no alert, and the counts skip them.
fn species_redetected(t: &evo_sim::species::SpeciesTracker, species: u32, player: PlayerId) -> bool {
    let Some(lineage) = t.get(species).and_then(|s| s.main_lineage()) else {
        return false;
    };
    t.species.iter().any(|o| {
        o.id < species && o.founder == Some(player) && o.merged_into.is_some() && o.main_lineage() == Some(lineage)
    })
}

/// Whether a species belongs in the species list: alive with adults, or
/// extinct after a real life. Left out: living species with no adults of
/// their own (merging into another, or missed by the last sample) and
/// species that merged back within a season of appearing (the numbered
/// copies of a species flapping across the threshold in older worlds).
fn species_listed(s: &evo_sim::species::Species, season_days: u64) -> bool {
    match (s.extinct_day, s.merged_into) {
        (None, _) => s.adults > 0.0,
        (Some(d), Some(_)) => d.saturating_sub(s.founded_day) >= season_days,
        (Some(_), None) => s.peak_adults > 0.0,
    }
}

/// Share of an animal's alleles that must be one other player's for a
/// retiring lineage to hand it to them (`Game::retire_sparing`): a quarter,
/// a grandchild of that player's stud.
pub const RETIRE_TRANSFER_SHARE: f64 = 0.25;

/// Eggs lost between two counts of a lineage, by cause (none listed at 0).
fn egg_losses(now: &evo_sim::world::EggCounts, before: &evo_sim::world::EggCounts) -> BTreeMap<String, u64> {
    [
        ("chilled", now.chilled.saturating_sub(before.chilled)),
        ("overheated", now.overheated.saturating_sub(before.overheated)),
        ("raided", now.raided.saturating_sub(before.raided)),
    ]
    .into_iter()
    .filter(|&(_, n)| n > 0)
    .map(|(c, n)| (c.to_string(), n))
    .collect()
}

/// An egg loss cause in a sentence. `abandoned`: the lineage leaves its
/// eggs by design (abandoned eggs), so a chilled clutch is expected, not a
/// parent's failure.
fn egg_loss_why(cause: &str, abandoned: bool) -> &'static str {
    match cause {
        "chilled" if abandoned => "chilled: abandoned eggs lie exposed, by design",
        "chilled" => "chilled: no parent was brooding them (it strayed too far from the nest, or died)",
        "overheated" => "overheated",
        _ => "eaten by egg raiders",
    }
}

/// Most mating requests one player may send another within
/// `GameConfig::request_window_ticks` (refused with `too_many_requests`).
pub const REQUESTS_PER_TARGET: usize = 3;
/// Most players one player may block.
pub const MAX_BLOCKED: usize = 200;

/// Tiles a view may reach beyond what the player's animals perceive
/// ([`Game::view_reach`]). 16 in playtest 3 still listed hundreds of
/// rivals' animals 3–5 times as far as any of the player's perceived.
pub const VIEW_MARGIN: f32 = 8.0;

/// Tiles beyond what the player's animals perceive within which they may
/// still name another animal in a request or a preview
/// ([`Game::check_partner_visible`]): wider than [`VIEW_MARGIN`] (what
/// shows), as a herd's members spread.
pub const NAME_MARGIN: f32 = 16.0;

/// Share of a bought agreement's fee the buyer forfeits to the seller when
/// it lapses (at least 1 renown when anything was paid).
pub const LAPSE_FEE_PERCENT: u64 = 25;
/// The least a forfeit takes when anything was paid (capped at what was
/// paid): playtest 4 found 10% (1 renown on a cheap listing) no deterrent.
pub const LAPSE_FEE_MIN: u64 = 5;
/// Longest an accepted agreement holds its pair, in simulated days,
/// whatever its wall-clock deadline (30 real minutes were about 375 game
/// days at a fast tick).
pub const HOLD_MAX_DAYS: u64 = 30;
/// Least fee (or auction reserve) for listing a dam: buying one holds her
/// from breeding, so a free listing gave rivals a free dam-lock.
pub const LISTING_MIN_FEE: u64 = 5;
/// Minutes an accepted agreement waits for the two to meet
/// (`GameConfig::purchase_deadline_ticks`).
pub const PURCHASE_DEADLINE_MINUTES: f64 = 30.0;
/// Minutes a pending request waits for an answer (`GameConfig::offer_ticks`).
pub const OFFER_MINUTES: f64 = 10.0;
/// Minutes before a pending request expires that its sender is warned.
pub const OFFER_WARN_MINUTES: f64 = 2.0;
/// Minutes of the per-target request window (`GameConfig::request_window_ticks`).
pub const REQUEST_WINDOW_MINUTES: f64 = 60.0;

/// Wall-clock minutes a player must be gone (no connection) before their
/// offline protection starts (`GameConfig::offline_idle_ticks`): a player
/// giving orders over short connections is playing, not away.
pub const OFFLINE_IDLE_MINUTES: f64 = 10.0;
/// Shortest auction in minutes (`GameConfig::min_auction_ticks`).
pub const MIN_AUCTION_MINUTES: f64 = 5.0;

/// A lineage is starving (`Game::lineage_warnings`) when this share of it
/// died of hunger or thirst in a day…
pub const STARVING_SHARE: f64 = 0.05;
/// …and at least this many did.
pub const STARVING_MIN: u64 = 5;
/// Near the hard cap from this share of it (alerted once per approach)…
pub const NEAR_CAP_SHARE: f64 = 0.9;
/// …and alerted again only after dropping below this share.
pub const NEAR_CAP_REARM: f64 = 0.85;
/// Litters at or above this multiplier are "full size" in the wording.
pub const FULL_LITTER: f32 = 0.98;
/// Animals in a territory at this hunger or thirst count as starving there.
pub const TERRITORY_NEED: f32 = 85.0;
/// Territories are starving when this share of the animals in them are.
pub const TERRITORY_STARVING_SHARE: f64 = 0.25;
/// A warning still in force is said again after this many days.
pub const WARNING_REPEAT_DAYS: u64 = 3;

/// Real minutes a possession outlives its player's last session (a reload
/// or a dropped connection keeps it); then the animal is released, so it
/// doesn't stand possessed and starve while nobody steers it (playtest 3).
pub const POSSESSION_GRACE_MINUTES: f64 = 1.0;

/// `minutes` of wall-clock time in ticks of `tick_ms` (at least 1).
pub fn minutes_to_ticks(minutes: f64, tick_ms: f64) -> u64 {
    ((minutes * 60_000.0 / tick_ms.max(1.0)).round() as u64).max(1)
}

/// Days over which a new lineage's legacy share comes to count in full
/// toward the ranked season's legacy score: on day `d` after its founding,
/// `d / FOUNDING_RAMP_DAYS` of it. A fresh founding has a large share at
/// once (its founders are all adults, many pregnant), so without the ramp
/// letting a lineage die and founding again would pay.
pub const FOUNDING_RAMP_DAYS: u64 = 10;

/// How close to winter a founding has to be for the founders to start
/// provisioned (`GameConfig::winter_provision`).
pub const WINTER_PROVISION_DAYS: u32 = 30;

/// Finished matings kept per player (`Game::mating_history`).
pub const MATING_LOG_LEN: usize = 40;

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
            home_range_ticks: DEFAULT_HOME_RANGE_TICKS,
            home_range_onset_ticks: DEFAULT_HOME_RANGE_ONSET_TICKS,
            home_range_fade_ticks: DEFAULT_HOME_RANGE_FADE_TICKS,
            tick_ms: 1000.0,
            winter_provision: true,
            purchase_deadline_ticks: 30 * 60,
            offer_ticks: 10 * 60,
            offer_warn_ticks: 2 * 60,
            request_window_ticks: 3600,
            min_auction_ticks: 5 * 60,
            rally_min_minutes: 5.0,
            grace_min_minutes: 30.0,
            grace_max_days: 60.0,
            attack_alert_minutes: 5.0,
            claim_cooldown_min_minutes: 2.0,
            offline_idle_ticks: 600,
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
    /// Points the founded design cost (with the founders it got).
    pub cost: f32,
    /// Founders the design asked for (more than `founders` when re-founding
    /// soon after a lineage ended).
    pub requested: u32,
    /// Why fewer were founded, if they were.
    pub reduced: Option<String>,
    /// Whether the new lineage has a newcomer grace, and why (or why not).
    pub grace: Option<String>,
}

/// A re-founding soon after the player's last lineage ended
/// ([`Game::refound_terms`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefoundTerms {
    /// The first simulated day a founding gets the full design again.
    pub until_day: u64,
    /// About how many wall-clock minutes until then.
    pub minutes_left: u64,
    /// Said to the player.
    pub reason: String,
}

impl RefoundTerms {
    /// Founders a re-founding of `design` gets inside the window: founders
    /// cost double points, so as many of its founders as fit the budget at
    /// that price ([`RefoundTerms::cost`]), and at least half (at least 2).
    /// A design with points to spare gets more than half (playtest 5: the
    /// half rule charged the full points for half the founders).
    pub fn founders(&self, design: &FounderDesign) -> u32 {
        let n = design.founders;
        let half = (n / 2).max(2).min(n);
        (half..=n).rev().find(|&k| Self::cost(design, k) <= DESIGN_BUDGET + 1e-3).unwrap_or(half)
    }

    /// Points `design` with `founders` founders costs inside the window:
    /// its cost with the founders' points counted twice.
    pub fn cost(design: &FounderDesign, founders: u32) -> f32 {
        design.clone().founders(founders).point_cost() + evo_sim::design::founder_cost(founders)
    }
}

/// Wall-clock minutes a re-founding counts as soon after a lineage ended,
/// at the least ([`Game::refound_window_days`]).
pub const REFOUND_MINUTES: f64 = 30.0;

#[derive(Clone, Debug, PartialEq)]
pub enum FoundError {
    AlreadyFounded {
        lineage: u32,
    },
    Design(DesignError),
    /// The lineage name looks like another player's (`names::impersonates`)
    /// or fails the name filter: why.
    BadName(String),
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
    /// The market fee kept from the payout (`economy::market_fee`, fixed
    /// when the escrow was taken; 0 for offers saved before the fee).
    #[serde(default)]
    fee: u64,
    /// When it was accepted (a purchase: when bought). Its meeting deadline
    /// counts from here ([`GameConfig::purchase_deadline_ticks`]); absent in
    /// offers saved before the deadline counted from acceptance (then from
    /// `created_tick`).
    #[serde(default)]
    accepted_tick: Option<u64>,
    /// The sender was warned that this pending offer is about to expire.
    #[serde(default)]
    warned: bool,
    /// An offer to any animal of this lineage (`offer_to_lineage`):
    /// `partner` is 0 until accepted.
    #[serde(default)]
    lineage: Option<u32>,
    /// Where an accepted agreement's pair meets (`World::meeting_point`,
    /// fixed at acceptance; absent in older saves: computed again).
    #[serde(default)]
    meet: Option<(f32, f32)>,
    /// The requester's animal died after acceptance: how (until they pick
    /// a substitute with `substitute_mating`, within the deadline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lost: Option<String>,
    /// That loss was outside the requester's control (eaten, disease, a
    /// harmful recessive, attrition, the winter-sleep toll, a migration):
    /// a lapse or cancel then forfeits nothing.
    #[serde(default)]
    excused: bool,
    /// The partner's animal slept while the requester's was awake: a lapse
    /// forfeits nothing (the requester mustn't pay for the other side's
    /// sleep).
    #[serde(default)]
    partner_slept: bool,
    /// Ticks the deadline has waited for a sleeper (at most
    /// [`HOLD_MAX_DAYS`] of them).
    #[serde(default)]
    paused: u64,
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

/// A migration's drawn route and the target it was planned for.
type PlannedRoute = (Vec2, Vec<[f32; 2]>);

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
    /// Egg counts per lineage at the last day boundary (egg losses of the day).
    prev_eggs: BTreeMap<u32, evo_sim::world::EggCounts>,
    alerts: BTreeMap<PlayerId, VecDeque<AlertView>>,
    /// Each player's recent finished matings, newest first (memory only:
    /// `GET /api/matings/history`).
    mating_log: BTreeMap<PlayerId, VecDeque<MatingRecord>>,
    /// Organisms whose death is announced to their owner.
    watched: BTreeMap<OrgId, (PlayerId, Watch)>,
    progress: BTreeMap<PlayerId, Progress>,
    /// Organisms whose spell of need was alerted (the day it was), until it eases.
    need_alerted: BTreeMap<OrgId, u64>,
    /// The tick an alerted need eased below the threshold: the spell ends
    /// once it stays eased a game day (playtest 4: a hunter's hunger swung
    /// across the threshold with each small kill, and the alert came every
    /// few seconds). Not saved.
    need_eased: BTreeMap<OrgId, u64>,
    /// Possessed organisms told they bedded down (dormant), until they wake. Not saved.
    sleep_alerted: BTreeSet<OrgId>,
    /// Watched organisms' deaths not yet announced: grouped per player into
    /// one alert per game hour (`announce_deaths`). Not saved.
    pending_deaths: BTreeMap<PlayerId, Vec<(OrgId, Watch, Why)>>,
    /// Each player lineage's founding: the day, the founders and their mean
    /// genes and body plan (the family tree's first stage). Saved.
    founded: BTreeMap<u32, crate::protocol::FamilySample>,
    /// Players whose view windows are the world's level-of-detail focus.
    focused: BTreeSet<PlayerId>,
    /// Chronicle entries already announced: achievements, eras, and per
    /// catastrophe whether its start and end were.
    seen_achievements: usize,
    seen_eras: usize,
    /// Era standings by mean share (`eras::EraScores`).
    /// Survival credits given (player, catastrophe): once each, in the
    /// season the catastrophe mostly fell in (`credit_survival`).
    survival_credited: BTreeSet<(PlayerId, u32)>,
    era_scores: eras::EraScores,
    seen_catastrophes: Vec<(bool, bool)>,
    /// A drought warning is out (`DROUGHT_WARN`); cleared below
    /// `DROUGHT_CLEAR`. Not saved: a restart in a drought warns again.
    drought_warned: bool,
    /// Players who founded since the last tick: told of a drought under way
    /// (they missed its warning). Not saved.
    joined: Vec<PlayerId>,
    /// Alerts for players who just founded (their grace explained), sent
    /// with the next tick's alerts. Not saved.
    founding_notes: Vec<(PlayerId, String)>,
    /// Players whose last session closed, and the tick it did: their
    /// possession ends after [`POSSESSION_GRACE_MINUTES`]. Not saved (a
    /// restart drops every session; the release then follows the grace).
    left: BTreeMap<PlayerId, u64>,
    /// The animal each player possesses, as last seen alive and possessed:
    /// the sim clears `possessed_by` as an animal dies, so this says whether
    /// it died possessed. Not saved (possession ends with the sessions).
    possessing: BTreeMap<PlayerId, OrgId>,
    /// Lineage warnings given (player, kind) and the day: repeated every
    /// [`WARNING_REPEAT_DAYS`] while they last, forgotten when they end.
    /// Not saved.
    warned: BTreeMap<(PlayerId, String), u64>,
    /// Attack orders given (attacker → its player and target), to tell the
    /// player how each ended. Not saved.
    attack_watch: BTreeMap<OrgId, (PlayerId, OrgId)>,
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
    /// Who may send each player mating requests (`ClientMsg::MatingPrefs`).
    mating_prefs: BTreeMap<PlayerId, MatingPrefs>,
    /// Day each lineage last earned the species-founded bonus
    /// ([`species_bonus_due`]).
    species_bonus: BTreeMap<u32, u64>,
    /// Ticks of each player's recent requests to each other player, for the
    /// per-target limit ([`REQUESTS_PER_TARGET`]); not saved.
    request_log: BTreeMap<(PlayerId, PlayerId), VecDeque<(u64, u64)>>,
    /// Name changes ([`Game::set_name`]), for [`Game::live_season_key`].
    names_changed: u64,
    /// The players whose name changed, with the `names_changed` count after
    /// each change, since the state index was last built (the kept
    /// leaderboard patches their rows instead of being made again).
    renamed: Vec<(u64, PlayerId)>,
    /// The state index and organism views kept between the state builds
    /// of a tick (`game::states`).
    state_cache: states::StateCache,
    /// The tick of the last full [`Game::hold_consented_females`]: until
    /// the next tick, a command need only hold what it adds
    /// ([`Game::hold_added`]).
    held_tick: Option<u64>,
    /// The marketplace listings indexed by every page order
    /// (`game::market`).
    market: market::MarketIndex,
    /// Each migration order's route as drawn by clients
    /// (`World::migration_route`), keyed by (player, group) with the
    /// target it was planned for; planned when first missing (an order
    /// just given, or after a restart) in [`Game::plan_routes`].
    routes: BTreeMap<(PlayerId, u8), PlannedRoute>,
    /// Day each player's current lineage was founded, for the founding
    /// ramp of the season's legacy score ([`FOUNDING_RAMP_DAYS`]); absent
    /// for lineages founded before it was kept (counted in full).
    founded_day: BTreeMap<PlayerId, u64>,
    /// Territory conditions already announced by the daily check, by
    /// (player, group, kind: 0 dry, 1 grazed bare); not saved.
    territory_alerted: BTreeSet<(PlayerId, u8, u8)>,
    /// Groups already told they drifted from their home; not saved.
    drift_alerted: BTreeSet<(PlayerId, u8)>,
    /// How each group's last order ended on its own (arrived, stalled,
    /// claimed on arrival), shown on its card until its next order: alerts
    /// scroll away (playtest 4: a migration "silently did nothing"). Not
    /// saved.
    outcomes: BTreeMap<(PlayerId, u8), String>,
    /// Alleles in breeding adults by (host lineage, origin), taken with
    /// each daily record (`World::legacy_hosts`): the royalty and the score
    /// breakdown read it. Not saved; recomputed on a restore.
    hosts: evo_sim::world::HostTally,
    /// Messages made outside a command or tick (a founding over HTTP),
    /// sent with the next tick's (`Game::after_tick`).
    pending_out: Outbox,
    /// Tick of the last attack alert by (attacker lineage, victim): at most
    /// one per [`GameConfig::attack_alert_minutes`]. Not saved.
    attack_alerts: BTreeMap<(u32, PlayerId), u64>,
    /// Players whose lineage was under its newcomer grace at the last tick,
    /// to announce its end ([`Game::announce_grace_end`]). Not saved
    /// (rebuilt on a restore).
    graced: BTreeSet<PlayerId>,
    /// Players whose last connection closed, with the tick: their offline
    /// spell starts after `GameConfig::offline_idle_ticks`. Not saved (a
    /// restart starts every unconnected player's spell).
    leaving: BTreeMap<PlayerId, u64>,
    /// Tests: take the plain, slower paths these shortcuts replace (full
    /// hold scans, a fresh index per new connection) to compare against.
    #[cfg(test)]
    reference: bool,
}

/// (requester, target) of the per-target request log.
type PlayerPair = (PlayerId, PlayerId);
/// (tick, offer) of each logged request, oldest first.
type RequestTicks = VecDeque<(u64, u64)>;

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
    /// `prev_eggs` as a list (saves from before it: none, so the first day
    /// after the restore counts every loss since founding once).
    #[serde(default)]
    prev_eggs: Vec<(u32, evo_sim::world::EggCounts)>,
    alerts: BTreeMap<PlayerId, VecDeque<AlertView>>,
    watched: BTreeMap<OrgId, (PlayerId, Watch)>,
    progress: BTreeMap<PlayerId, Progress>,
    need_alerted: BTreeMap<OrgId, u64>,
    focused: BTreeSet<PlayerId>,
    seen_achievements: usize,
    seen_eras: usize,
    /// Era standings by mean share (absent in older saves).
    /// Survival credits given (absent in older saves).
    survival_credited: BTreeSet<(PlayerId, u32)>,
    era_scores: eras::EraScores,
    seen_catastrophes: Vec<(bool, bool)>,
    /// Version 2 on.
    economy: Option<Economy>,
    /// Version 3 on: the ranked season under way and the last one finished.
    season: Option<SeasonTracker>,
    last_season_record: Option<SeasonRecord>,
    /// Offline protection spells (absent in older saves: every player with
    /// a lineage starts one at the restore).
    offline: Option<OfflineTracker>,
    /// Players' mating-request settings (absent in older saves: none).
    mating_prefs: BTreeMap<PlayerId, MatingPrefs>,
    /// Day each lineage last earned the species-founded bonus (absent in
    /// older saves: none).
    species_bonus: BTreeMap<u32, u64>,
    /// Each player lineage's founding as it happened (absent in older saves:
    /// the family tree then starts at the first recorded day).
    founded: BTreeMap<u32, crate::protocol::FamilySample>,
    /// Each player's recent finished matings (`Game::mating_history`):
    /// the Mates tab's history survives a restart (absent in older saves).
    mating_log: BTreeMap<PlayerId, VecDeque<MatingRecord>>,
    /// The day each player's lineage was founded (the
    /// founding ramp; absent in older saves, which count every lineage in
    /// full).
    founded_day: BTreeMap<PlayerId, u64>,
    /// When players ended their newcomer grace (`World::end_grace`; absent
    /// in older saves: none).
    grace_waived: BTreeMap<PlayerId, u64>,
    /// The per-target request log, as a list (absent in older saves: empty).
    request_log: Vec<(PlayerPair, RequestTicks)>,
    /// The last attack alert ticks, as a list (absent in older saves: none).
    attack_alerts: Vec<((u32, PlayerId), u64)>,
}

impl SavedGame {
    /// The world that wrote it (empty before world ids), which keys its
    /// stored terrain.
    pub fn world_id(&self) -> &str {
        &self.world_id
    }
}

/// Refuse a price above [`economy::MAX_PRICE`].
#[cfg(test)]
pub(crate) fn check_price_for_tests(price: u64) -> Result<(), ErrorInfo> {
    check_price(price)
}

fn check_price(price: u64) -> Result<(), ErrorInfo> {
    if price > economy::MAX_PRICE {
        return Err(ErrorInfo::new("bad_price", format!("prices go up to {} renown", economy::MAX_PRICE)));
    }
    Ok(())
}

/// The organisms a command names as the player's own (possess, orders,
/// splits, matings' `mine`, listings), for [`Game::hide_unseen`].
fn named_organisms(msg: &ClientMsg) -> Vec<OrgId> {
    match msg {
        ClientMsg::Possess { organism }
        | ClientMsg::OrderMove { organism, .. }
        | ClientMsg::PossessAttack { organism, .. }
        | ClientMsg::Release { organism }
        | ClientMsg::CreateListing { organism, .. }
        | ClientMsg::CreateAuction { organism, .. } => vec![*organism],
        ClientMsg::SplitGroup { organisms, .. } => organisms.clone().unwrap_or_default(),
        ClientMsg::RequestMating { mine, .. }
        | ClientMsg::PreviewMating { mine, .. }
        | ClientMsg::OfferToLineage { mine, .. }
        | ClientMsg::BuyListing { mine, .. }
        | ClientMsg::Bid { mine, .. } => vec![*mine],
        _ => Vec::new(),
    }
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

/// A listing a market query matched: its sort key, and its preview when
/// the query needed every one.
type MarketRow<'a> = ((u64, u64), &'a Listing, Option<Box<MatingPreviewView>>);

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

/// Tiles in the cells listings report positions in ([`coarse`]).
const LISTING_CELL: f32 = 16.0;

/// A coordinate as the center of its [`LISTING_CELL`] cell.
fn coarse(v: f32) -> f32 {
    (v / LISTING_CELL).floor() * LISTING_CELL + LISTING_CELL / 2.0
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
        let economy = Economy::new();
        let market = market::MarketIndex::build(&economy, &world);
        let hosts = world.legacy_hosts();
        let pending_out = Outbox::new();
        let mut g = Self {
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
            prev_eggs: BTreeMap::new(),
            alerts: BTreeMap::new(),
            mating_log: BTreeMap::new(),
            watched: BTreeMap::new(),
            progress: BTreeMap::new(),
            need_alerted: BTreeMap::new(),
            need_eased: BTreeMap::new(),
            sleep_alerted: BTreeSet::new(),
            pending_deaths: BTreeMap::new(),
            founded: BTreeMap::new(),
            focused: BTreeSet::new(),
            seen_achievements: 0,
            seen_eras: 0,
            era_scores: eras::EraScores::default(),
            survival_credited: BTreeSet::new(),
            seen_catastrophes: Vec::new(),
            drought_warned: false,
            joined: Vec::new(),
            founding_notes: Vec::new(),
            left: BTreeMap::new(),
            possessing: BTreeMap::new(),
            warned: BTreeMap::new(),
            attack_watch: BTreeMap::new(),
            economy,
            economy_restored: false,
            season,
            last_season_record: None,
            offline: OfflineTracker::default(),
            mating_prefs: BTreeMap::new(),
            species_bonus: BTreeMap::new(),
            request_log: BTreeMap::new(),
            names_changed: 0,
            renamed: Vec::new(),
            state_cache: states::StateCache::default(),
            held_tick: None,
            market,
            routes: BTreeMap::new(),
            founded_day: BTreeMap::new(),
            territory_alerted: BTreeSet::new(),
            drift_alerted: BTreeSet::new(),
            outcomes: BTreeMap::new(),
            hosts,
            pending_out,
            attack_alerts: BTreeMap::new(),
            graced: BTreeSet::new(),
            leaving: BTreeMap::new(),
            #[cfg(test)]
            reference: false,
        };
        let minutes = |m: f64| (m * 60_000.0 / g.cfg.tick_ms.max(1.0)).round() as u64;
        let (rally, claim) = (minutes(g.cfg.rally_min_minutes), minutes(g.cfg.claim_cooldown_min_minutes));
        g.world.set_rally_min_ticks(rally);
        g.world.set_claim_cooldown_min_ticks(claim);
        let cap = (g.cfg.grace_max_days * g.world.config.ticks_per_day as f64).round() as u64;
        g.world.set_attack_grace_min_ticks(minutes(g.cfg.grace_min_minutes).min(cap));
        g
    }

    /// The player's newcomer grace left (`StateView::grace`), if any.
    pub(crate) fn grace_view(&self, player: PlayerId) -> Option<GraceView> {
        let days = self.world.lineage_grace_left(self.lineage_of(player)?)?;
        let tpd = self.world.config.ticks_per_day.max(1) as f64;
        let ticks = (days as f64 * tpd).round() as u64;
        let seconds = (ticks as f64 * self.cfg.tick_ms / 1000.0 / 10.0).ceil() as u64 * 10;
        Some(GraceView {
            days_left: (days * 10.0).ceil() / 10.0,
            until_tick: self.world.tick() + ticks,
            real_seconds_left: Some(seconds),
        })
    }

    /// Names chosen before the shared filter (`crate::names`), cleaned on a
    /// restore: players' lineage names, species names, nameplate titles.
    /// Bidi overrides, zero-width characters and markup are dropped; a name
    /// left empty or passing for staff gets a plain one.
    fn sanitize_names(&mut self) {
        use crate::names::{sanitize, Kind};
        let max = crate::design::MAX_NAME_LEN;
        for (l, lin) in self.world.lineages.iter_mut().enumerate() {
            if lin.owner != WILD {
                lin.name = sanitize(&lin.name, max, Kind::Lineage, &format!("Lineage {l}"));
            }
        }
        for s in self.world.chronicle.species.species.iter_mut() {
            s.name = sanitize(&s.name, max, Kind::Species, &format!("Species {}", s.id));
        }
        for plate in self.economy.nameplates.values_mut() {
            plate.title = sanitize(&plate.title, economy::MAX_TITLE_LEN, Kind::Title, "Breeder");
        }
    }

    /// Players whose current lineage is under its newcomer grace.
    fn graced_players(&self) -> BTreeSet<PlayerId> {
        self.lineage_of.iter().filter(|(_, &l)| self.world.lineage_grace_ticks(l).is_some()).map(|(&p, _)| p).collect()
    }

    /// Tell each player whose grace just ended (ran out, or they ended it).
    fn announce_grace_end(&mut self, out: &mut Outbox) {
        let now = self.graced_players();
        let ended: Vec<PlayerId> = self.graced.difference(&now).copied().collect();
        self.graced = now;
        for p in ended {
            let text = "Your newcomer grace has ended: other players' animals can now attack yours, and yours theirs"
                .to_string();
            self.alert(p, AlertKind::Attack, text, None, out);
        }
    }

    /// A command error, with the real time left added to a grace refusal
    /// (the days alone mislead at a fast clock).
    fn grace_error(&self, err: &CommandError) -> ErrorInfo {
        let info = ErrorInfo::from(err);
        let days = match err {
            CommandError::NewcomerGrace { days_left } | CommandError::OwnGrace { days_left } => *days_left,
            _ => return info,
        };
        let secs = self.real_seconds((days as f64 * self.world.config.ticks_per_day as f64).round() as u64);
        let minutes = secs.div_ceil(60);
        let message = format!("{} (about {minutes} real minute{})", info.message, if minutes == 1 { "" } else { "s" });
        ErrorInfo { message, ..info }.with_detail(serde_json::json!({ "days_left": days, "real_seconds_left": secs }))
    }

    /// An attack target out of reach, with how far it is now: the player's
    /// view lags the world, and in a chase the gap can open between the
    /// state they saw and the order landing (playtest 4).
    fn attack_out_of_sight(&self, organism: OrgId, target: OrgId, err: &CommandError) -> ErrorInfo {
        let info = ErrorInfo::from(err);
        let (Some(a), Some(t)) = (self.world.organism(organism), self.world.organism(target)) else {
            return info;
        };
        let dist = a.pos.dist(t.pos);
        let reach = evo_sim::world::commands::ORDER_KEPT * self.world.attack_range();
        let message = format!(
            "#{target} is {dist:.0} tiles from your #{organism} now (orders reach {reach:.0}); it moved since your view updated, so get closer first"
        );
        ErrorInfo { message, ..info }.with_detail(serde_json::json!({ "distance": dist, "reach": reach }))
    }

    /// Real seconds `ticks` take at the server's clock.
    fn real_seconds(&self, ticks: u64) -> u64 {
        (ticks as f64 * self.cfg.tick_ms / 1000.0).ceil() as u64
    }

    /// Mark the neighbours whose lineage is under its newcomer grace
    /// (`NearbyOrganism::protected`).
    pub(crate) fn mark_protected(&self, nearby: &mut [NearbyOrganism]) {
        for n in nearby {
            if n.owner != WILD && self.world.lineage_grace_left(n.lineage).is_some() {
                n.protected = Some(true);
            }
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
            prev_eggs: self.prev_eggs.iter().map(|(&k, &v)| (k, v)).collect(),
            alerts: self.alerts.clone(),
            watched: self.watched.clone(),
            progress: self.progress.clone(),
            need_alerted: self.need_alerted.clone(),
            focused: self.focused.clone(),
            seen_achievements: self.seen_achievements,
            seen_eras: self.seen_eras,
            era_scores: self.era_scores.clone(),
            survival_credited: self.survival_credited.clone(),
            seen_catastrophes: self.seen_catastrophes.clone(),
            economy: Some(self.economy.clone()),
            season: Some(self.season.clone()),
            last_season_record: self.last_season_record.clone(),
            offline: Some(self.offline.clone()),
            mating_prefs: self.mating_prefs.clone(),
            species_bonus: self.species_bonus.clone(),
            founded: self.founded.clone(),
            mating_log: self.mating_log.clone(),
            founded_day: self.founded_day.clone(),
            grace_waived: self.world.grace_waived().clone(),
            request_log: self.request_log.iter().map(|(k, v)| (*k, v.clone())).collect(),
            attack_alerts: self.attack_alerts.iter().map(|(k, v)| (*k, *v)).collect(),
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
        crate::history::compact_window(&mut kept);
        g.history = kept;
        if !saved.world_id.is_empty() {
            g.world_id = saved.world_id;
        }
        g.lineage_of = saved.lineage_of;
        g.founded_day = saved.founded_day;
        // A player's lineage is in the world even if the saved table lost it.
        for (k, l) in g.world.lineages.iter().enumerate() {
            if l.owner != WILD {
                g.lineage_of.entry(l.owner).or_insert(k as u32);
            }
        }
        g.offers = saved.offers;
        g.next_offer = saved.next_offer.max(g.offers.keys().max().map_or(1, |k| k + 1));
        // Accepted agreements walk their pair together again (the world
        // keeps no rendezvous in its snapshot).
        let accepted: Vec<u64> =
            g.offers.values().filter(|o| o.status == OfferStatus::Accepted && o.partner != 0).map(|o| o.id).collect();
        for id in accepted {
            g.start_rendezvous(id);
        }
        g.prev_born = saved.prev_born;
        g.prev_deaths = saved.prev_deaths.into_iter().collect();
        g.prev_eggs = saved.prev_eggs.into_iter().collect();
        if g.prev_eggs.is_empty() {
            // Saves from before egg losses were reported: count from now.
            g.prev_eggs = g.world.counters.eggs.clone();
        }
        g.alerts = saved.alerts;
        g.watched = saved.watched;
        g.progress = saved.progress;
        g.need_alerted = saved.need_alerted;
        g.focused = saved.focused;
        g.seen_achievements = saved.seen_achievements;
        g.seen_eras = saved.seen_eras;
        g.era_scores = saved.era_scores;
        g.survival_credited = saved.survival_credited;
        g.seen_catastrophes = saved.seen_catastrophes;
        match saved.economy {
            Some(e) if saved.version >= 2 => {
                g.economy = e;
                g.market.rebuild(&g.economy, &g.world);
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
        g.mating_prefs = saved.mating_prefs;
        g.species_bonus = saved.species_bonus;
        g.offline_resume(saved.offline);
        g.founded = saved.founded;
        g.world.set_grace_waived(saved.grace_waived);
        g.sanitize_names();
        g.request_log = saved.request_log.into_iter().collect();
        g.attack_alerts = saved.attack_alerts.into_iter().collect();
        g.graced = g.graced_players();
        g.mating_log = saved.mating_log;
        // Every session ended with the restart: possessions follow the grace.
        let now = g.world.tick();
        for o in g.world.organisms.iter().filter(|o| o.alive) {
            if let Some(p) = o.possessed_by {
                g.left.insert(p, now);
            }
        }
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
        self.market.rebuild(&self.economy, &self.world);
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
        let bids = if self.market.in_sync(&self.economy) {
            self.market.bids_held(player)
        } else {
            self.economy.bids_held(player)
        };
        self.offers.values().filter(|o| o.from == player && o.status.is_open()).map(|o| o.escrow).sum::<u64>() + bids
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
        self.post_on(player, delta, kind, text, offer, listing, self.world.day())
    }

    /// [`Game::post`] dated `day` (from 0): daily income and royalties are
    /// paid as a day starts, for the day just ended, and are dated so.
    #[allow(clippy::too_many_arguments)]
    fn post_on(
        &mut self,
        player: PlayerId,
        delta: i64,
        kind: LedgerKind,
        text: String,
        offer: Option<u64>,
        listing: Option<u64>,
        day: u64,
    ) -> Result<LedgerEntry, Insufficient> {
        let tick = self.world.tick();
        let entry = self.economy.post(player, delta, kind, text, offer, listing, tick, day)?;
        self.persist.push(PersistEvent::Ledger(LedgerRecord { world_id: self.world_id.clone(), entry: entry.clone() }));
        self.persist.push(PersistEvent::Balance(BalanceRecord { player, balance: entry.balance, day }));
        Ok(entry)
    }

    pub fn set_name(&mut self, player: PlayerId, name: &str) {
        if self.names.get(&player).map(String::as_str) != Some(name) {
            self.names.insert(player, name.to_string());
            self.names_changed += 1;
            self.renamed.push((self.names_changed, player));
        }
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

    /// Whether everything the player founded has died out: no living
    /// organisms (agents or level-of-detail cohorts) and no eggs waiting to
    /// hatch. A player who never founded is not extinct.
    pub fn is_extinct(&self, player: PlayerId) -> bool {
        self.lineage_of.contains_key(&player)
            && self.owner_population(player) == 0
            && !self.world.clutches.iter().any(|c| c.owner == player)
    }

    /// Remember a finished mating for the player's history (newest first, capped).
    fn log_mating(&mut self, player: PlayerId, record: MatingRecord) {
        let log = self.mating_log.entry(player).or_default();
        log.push_front(record);
        log.truncate(MATING_LOG_LEN);
    }

    /// The player's recent finished matings, newest first.
    pub fn mating_history(&self, player: PlayerId) -> Vec<MatingRecord> {
        self.mating_log.get(&player).map(|l| l.iter().cloned().collect()).unwrap_or_default()
    }

    /// An egg-laying lineage's clutches; `None` for live bearers (or a lineage with nobody left to tell).
    fn eggs_view(&self, lineage: u32) -> Option<EggsView> {
        let w = &self.world;
        let lays = w.organisms.iter().find(|o| o.alive && o.lineage == lineage).map(|o| o.traits.birth.lays_eggs());
        let mine = w.clutches.iter().filter(|c| c.lineage == lineage);
        let mut v = EggsView {
            lost: egg_losses(&w.counters.eggs.get(&lineage).copied().unwrap_or_default(), &Default::default()),
            ..Default::default()
        };
        for c in mine {
            v.clutches += 1;
            v.eggs += c.eggs.len() as u32;
            if c.guardian.is_some_and(|g| w.organism(g).is_some_and(|o| o.alive)) {
                v.brooded += 1;
            }
        }
        (lays == Some(true) || v.clutches > 0 || !v.lost.is_empty()).then_some(v)
    }

    /// Whether founders founded now start provisioned for winter.
    pub fn winter_provision_now(&self) -> bool {
        self.cfg.winter_provision && self.world.days_until_winter() <= WINTER_PROVISION_DAYS
    }

    /// Test hook (`EVO_E2E_HOOKS`): every organism the player owns dies at
    /// once (and their eggs), as if a catastrophe struck; the number killed.
    pub fn e2e_wipe_out(&mut self, player: PlayerId) -> usize {
        let n = self.world.kill_all_of(player, evo_sim::organism::DeathCause::Predation);
        self.state_cache.invalidate();
        n
    }

    /// Found the player's lineage: one living lineage per player per world.
    /// A player whose lineage has died out may found a new one at once; their
    /// renown, ledger, records and seen alerts stay, the old lineage stays in
    /// the history as extinct, and the dead lineage's group orders go.
    pub fn found(&mut self, player: PlayerId, design: &FounderDesign) -> Result<FoundOk, FoundError> {
        design.validate().map_err(FoundError::Design)?;
        // The design's name is the lineage's (and its founding species'):
        // the shared name filter, and not another player's name.
        let name = crate::names::check(&design.name, crate::design::MAX_NAME_LEN, crate::names::Kind::Lineage)
            .map_err(FoundError::BadName)?;
        if self.impersonates(player, &name, crate::names::Kind::Lineage) {
            return Err(FoundError::BadName("it looks like another player's name".into()));
        }
        // Re-founding soon after the player's last lineage ended (retired or
        // died out) prices founders double ([`Game::refound_terms`]):
        // ending a lineage and founding again was a free re-roll (fresh, fed
        // founders at a new spot; playtests 3 and 4).
        if let Some(t) = self.refound_terms(player) {
            let n = t.founders(design);
            if self.lineage_of(player).is_none_or(|_| self.is_extinct(player)) {
                let fewer = design.clone().founders(n);
                let mut ok = self.found_at_any_price(player, &fewer)?;
                ok.cost = RefoundTerms::cost(design, n);
                ok.requested = design.founders;
                ok.reduced = Some(t.reason.clone());
                let text = format!("Re-founded: {n} of {} founders ({}).", design.founders, t.reason);
                // Live: delivered with the next tick's messages.
                let mut out = Outbox::new();
                self.alert(player, AlertKind::Milestone, text, None, &mut out);
                self.pending_out.extend(out);
                return Ok(ok);
            }
        }
        self.found_at_any_price(player, design)
    }

    /// Simulated days after a lineage ends (retired or died out) during which
    /// a re-founding prices founders double: a ranked season, and at least
    /// [`REFOUND_MINUTES`] of wall-clock time at this tick (a 60-day season
    /// passes in 5 minutes at a 100 ms tick).
    pub fn refound_window_days(&self) -> u64 {
        let tpd = self.world.config.ticks_per_day.max(1) as u64;
        let real = minutes_to_ticks(REFOUND_MINUTES, self.cfg.tick_ms).div_ceil(tpd);
        self.cfg.season_days.max(real).max(1)
    }

    /// How `player`'s lineage ended, once it has: `retired` or `died_out`
    /// (the design screen words a re-founding by it, in any browser).
    pub fn lineage_ended(&self, player: PlayerId) -> Option<String> {
        if !self.lineage_of.contains_key(&player) || !self.is_extinct(player) {
            return None;
        }
        let founded = self.founded_day.get(&player).copied().unwrap_or(0);
        Some(match self.economy.retired_day.get(&player) {
            Some(&d) if d >= founded && !self.economy.died_out.contains(&player) => "retired".into(),
            _ => "died_out".into(),
        })
    }

    /// The terms of `player`'s next founding if it would be a re-founding
    /// within [`Game::refound_window_days`] of their last lineage ending
    /// (retired, or died out), else None (a first founding, or long after).
    pub fn refound_terms(&self, player: PlayerId) -> Option<RefoundTerms> {
        if !self.lineage_of.contains_key(&player) || !self.is_extinct(player) {
            return None;
        }
        let founded = self.founded_day.get(&player).copied().unwrap_or(0);
        let today = self.world.day();
        // The end recorded for this lineage (retire, or the daily check
        // that saw it die out); a lineage that died since that check ended
        // today.
        let (ended, how) = match self.economy.retired_day.get(&player) {
            Some(&d) if d >= founded => {
                (d, if self.economy.died_out.contains(&player) { "died out" } else { "retired" })
            }
            _ => (today, "died out"),
        };
        let window = self.refound_window_days();
        (today < ended + window).then(|| {
            let days_left = ended + window - today;
            let minutes_left =
                (days_left as f64 * self.world.config.ticks_per_day as f64 * self.cfg.tick_ms / 60_000.0).ceil();
            RefoundTerms {
                until_day: ended + window,
                minutes_left: minutes_left as u64,
                reason: format!(
                    "your last lineage {how} on day {}: re-founding within {window} days (about {} real minutes), founders cost double points (you get as many as the budget then buys, at least half)",
                    ended + 1,
                    (window as f64 * self.world.config.ticks_per_day as f64 * self.cfg.tick_ms / 60_000.0).round()
                ),
            }
        })
    }

    /// The founders a re-founding of `design` gets now inside the window
    /// ([`Game::refound_terms`], [`RefoundTerms::founders`]), None otherwise.
    pub fn refound_founders(&self, player: PlayerId, design: &FounderDesign) -> Option<u32> {
        self.refound_terms(player).map(|t| t.founders(design))
    }

    /// [`Game::found`] without the point-budget check, for offline tools
    /// that replay older prices (`examples/competition.rs` founds playtest
    /// 1's 130-founder swarms); the server never calls it.
    #[doc(hidden)]
    pub fn found_at_any_price(&mut self, player: PlayerId, design: &FounderDesign) -> Result<FoundOk, FoundError> {
        if let Some(&lineage) = self.lineage_of.get(&player) {
            if !self.is_extinct(player) {
                return Err(FoundError::AlreadyFounded { lineage });
            }
        }
        let refounding = self.lineage_of.contains_key(&player);
        let lineage = self.world.lineages.len() as u32;
        let founders = self.world.found_lineage_unchecked(player, design);
        if self.winter_provision_now() {
            self.world.provision_for_winter(&founders);
        }
        if refounding {
            self.world.clear_group_orders(player);
            // Milestones count for the new lineage (the legacy share's best stays).
            if let Some(prog) = self.progress.get_mut(&player) {
                prog.population = 0;
                prog.generation = 0;
                prog.extinct = false;
            }
        }
        // New organisms: the next state build indexes the world again.
        self.state_cache.invalidate();
        self.lineage_of.insert(player, lineage);
        self.founded.insert(lineage, self.founding_sample(&founders));
        self.founded_day.insert(player, self.world.day());
        self.joined.push(player);
        for &id in &founders {
            self.watched.insert(id, (player, Watch::Founder));
        }
        if self.economy.granted.insert(player) {
            let text = format!("Founding grant for {}", design.name);
            let _ = self.post(player, economy::FOUNDING_GRANT as i64, LedgerKind::Grant, text, None, None);
        }
        if self.world.lineage_grace_ticks(lineage).is_some() {
            self.graced.insert(player);
        }
        let requested = design.founders;
        let grace = self.grace_at_founding(player, lineage);
        if let Some(text) = &grace {
            self.founding_notes.push((player, text.clone()));
        }
        Ok(FoundOk { lineage, founders, cost: design.point_cost(), requested, reduced: None, grace })
    }

    /// The partner organism of an open offer (0 for an offer to a lineage
    /// not yet accepted), for tools that escort it.
    #[doc(hidden)]
    pub fn offer_partner(&self, id: u64) -> Option<OrgId> {
        self.offers.get(&id).map(|o| o.partner).filter(|&p| p != 0)
    }

    /// Retire the player's living lineage (`lineage` must name it, so a
    /// late click can't retire the next one): every animal, cohort and egg
    /// leaves the world at once (removed, not killed and not turned wild:
    /// `World::retire_player`), its group orders and possession end, and
    /// the player may found again at once ([`Game::found`]). Their renown,
    /// ledger, records and the old lineage's history stay. Marketplace
    /// offers and listings of the removed animals close on the next tick as
    /// for any animal gone. The number removed.
    pub fn retire(&mut self, player: PlayerId, lineage: u32) -> Result<usize, ErrorInfo> {
        self.retire_sparing(player, lineage, &mut Outbox::new()).map(|r| r.0)
    }

    /// [`Game::retire`], telling who received animals: an animal whose
    /// alleles are at least [`RETIRE_TRANSFER_SHARE`] another player's (the
    /// most of any one, who has a living lineage) joins that player's
    /// lineage instead of leaving the world (`World::retire_player_sparing`),
    /// so retiring no longer deletes rivals' alleles. Returns (removed,
    /// transferred).
    fn retire_sparing(
        &mut self,
        player: PlayerId,
        lineage: u32,
        out: &mut Outbox,
    ) -> Result<(usize, usize), ErrorInfo> {
        let current = self.lineage_of(player);
        if current != Some(lineage) || self.is_extinct(player) {
            return Err(ErrorInfo::new(
                "no_lineage",
                match current {
                    Some(l) if l != lineage => format!("lineage {lineage} is not your living lineage (yours is {l})"),
                    _ => "you have no living lineage to retire".to_string(),
                },
            ));
        }
        let homes: BTreeMap<PlayerId, u32> = self
            .lineage_of
            .iter()
            .filter(|&(&p, _)| p != player && !self.is_extinct(p))
            .map(|(&p, &l)| (p, l))
            .collect();
        let before: BTreeMap<PlayerId, usize> = homes.keys().map(|&p| (p, self.world.owner_count(p))).collect();
        let (n, moved) = self.world.retire_player_sparing(player, &homes, RETIRE_TRANSFER_SHARE);
        self.economy.retired_day.insert(player, self.world.day());
        self.economy.died_out.remove(&player);
        for (&p, &b) in &before {
            let got = self.world.owner_count(p).saturating_sub(b);
            if got > 0 {
                let (animals, carry) = if got == 1 { ("animal", "carries") } else { ("animals", "carry") };
                let text = format!(
                    "{} retired their lineage: {got} of its {animals}, which {carry} at least {:.0}% of your genes, joined yours.",
                    self.name(player),
                    RETIRE_TRANSFER_SHARE * 100.0
                );
                self.alert(p, AlertKind::Milestone, text, None, out);
            }
        }
        // Not deaths: no alerts for founders or the possessed.
        self.watched.retain(|_, (p, _)| *p != player);
        self.routes.retain(|&(p, _), _| p != player);
        if let Some(prog) = self.progress.get_mut(&player) {
            prog.extinct = true;
        }
        // Organisms left the world outside a tick: the next state build
        // indexes it again, the listings are checked again, holds are redone.
        self.state_cache.invalidate();
        self.market.validated = None;
        self.held_tick = None;
        Ok((n, moved))
    }

    pub fn connect(&mut self, conn: ConnId, player: PlayerId, name: &str) -> Outbox {
        self.set_name(player, name);
        let first = !self.conns.values().any(|c| c.player == player);
        self.conns.insert(conn, Conn { player, view: None });
        self.left.remove(&player);
        let mut out = vec![(
            Target::Conn(conn),
            ServerMsg::Welcome { player, name: name.into(), lineage: self.lineage_of(player), proto: None },
        )];
        // Back within the idle time: never away, no spell to end.
        if first && self.leaving.remove(&player).is_none() {
            self.offline_login(player, &mut out);
        }
        if let Some(s) = self.state_now(conn) {
            out.push((Target::Conn(conn), ServerMsg::State(Box::new(s))));
        }
        out
    }

    pub fn disconnect(&mut self, conn: ConnId) {
        if let Some(c) = self.conns.remove(&conn) {
            if !self.conns.values().any(|o| o.player == c.player) {
                if self.cfg.offline_idle_ticks == 0 {
                    self.offline_logout(c.player);
                } else {
                    // Away only once gone a while (`offline_idle_ticks`).
                    self.leaving.insert(c.player, self.world.tick());
                }
                self.left.insert(c.player, self.world.tick());
            }
        }
    }

    /// Start the offline spells of players gone (no connection) for
    /// `offline_idle_ticks`.
    fn start_idle_spells(&mut self) {
        let (tick, idle) = (self.world.tick(), self.cfg.offline_idle_ticks);
        let due: Vec<PlayerId> = self.leaving.iter().filter(|&(_, &t)| tick >= t + idle).map(|(&p, _)| p).collect();
        for p in due {
            self.leaving.remove(&p);
            self.offline_logout(p);
        }
    }

    pub fn conn_player(&self, conn: ConnId) -> Option<PlayerId> {
        self.conns.get(&conn).map(|c| c.player)
    }

    /// Apply one client command and say what to send back.
    pub fn command(&mut self, conn: ConnId, player: PlayerId, id: Option<u64>, msg: ClientMsg) -> Outbox {
        let name = msg.name();
        let mut out = Outbox::new();
        self.note_command(player, &msg);
        let named = named_organisms(&msg);
        let result = self.apply(conn, player, msg, &mut out).map_err(|e| self.hide_unseen(player, e, &named));
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
            ClientMsg::Possess { organism } => {
                // One at a time: say which one possessing this let go.
                let released: Vec<OrgId> = self
                    .world
                    .organisms
                    .iter()
                    .filter(|o| o.alive && o.possessed_by == Some(player) && o.id != organism)
                    .map(|o| o.id)
                    .collect();
                self.world.possess(player, organism).map_err(e)?;
                self.possessing.insert(player, organism);
                Ok(released.first().map(|&r| {
                    serde_json::json!({
                        "organism": organism,
                        "released": r,
                        "notice": format!("Released #{r}: you possess one animal at a time."),
                    })
                }))
            }
            ClientMsg::OrderMove { organism, x, y } => {
                finite(&[x, y])?;
                let to = self.world.order_move(player, organism, Vec2::new(x, y)).map_err(e)?;
                Ok(Some(target_data(Vec2::new(x, y), to, "it can't get there; it goes as near as it can")))
            }
            ClientMsg::PossessAttack { organism, target } => {
                // Repeating the standing order changes nothing: no new alert.
                let repeat = self.world.attack_target(organism) == Some(target);
                self.world.possess_attack(player, organism, target).map_err(|err| match err {
                    CommandError::OutOfSight(_) => self.attack_out_of_sight(organism, target, &err),
                    _ => self.grace_error(&err),
                })?;
                self.attack_watch.insert(organism, (player, target));
                if !repeat {
                    self.alert_attacked(player, organism, target, out);
                }
                let mut data = serde_json::json!({ "organism": organism, "target": target });
                if let Some(w) = self.world.attack_warning(organism, target) {
                    data["warning"] = w.into();
                }
                Ok(Some(data))
            }
            ClientMsg::Release { organism } => self.world.release(player, organism).map(|_| None).map_err(e),
            ClientMsg::Migrate { x, y, group } => {
                finite(&[x, y])?;
                // No group: the main group, like a rally or a territory.
                let group = group.unwrap_or(0);
                let asked = Vec2::new(x, y);
                let before = self.orders_of(player, group);
                let to = self.world.command_migrate_group(player, group, asked).map_err(e)?;
                self.order_given(player, group);
                self.plan_route(player, group);
                let mut data =
                    target_data(asked, to, "that is water or impassable; the group goes to the nearest land");
                data["group"] = group.into();
                if let Some((_, route)) = self.routes.get(&(player, group)) {
                    data["route"] = serde_json::to_value(route).unwrap_or_default();
                }
                let mut warnings = self.travel_warnings(player, group, true);
                // What waits at the destination: the home circle it will keep.
                let members = self.world.groups(player).iter().find(|g| g.id == group).map_or(0, |g| g.members);
                let mut dest = serde_json::json!({});
                let there = self.circle_report(player, group, to, World::home_radius(members), &mut dest);
                warnings.extend(there.into_iter().map(|w| format!("At the destination: {w}")));
                dest.as_object_mut().map(|o| o.remove("distance"));
                data["destination"] = dest;
                put_warnings(&mut data, warnings);
                self.brood_report(player, group, &mut data);
                note_replaced(&mut data, &before, "migration");
                Ok(Some(data))
            }
            ClientMsg::EndGrace => {
                self.world.end_grace(player).map_err(e)?;
                self.announce_grace_end(out);
                Ok(None)
            }
            ClientMsg::RetireLineage { lineage } => {
                let (n, moved) = self.retire_sparing(player, lineage, out)?;
                Ok(Some(serde_json::json!({ "removed": n, "transferred": moved })))
            }
            ClientMsg::ClearMigration { group } => {
                match group {
                    None => self.world.clear_migration(player),
                    // A group that exists, or one whose order outlived it.
                    Some(g)
                        if g < MAX_GROUPS
                            && (g == 0
                                || self.world.group_has_members(player, g)
                                || self.world.migration_order(player, g).is_some()) =>
                    {
                        self.world.clear_migration_group(player, g)
                    }
                    Some(g) => return Err(e(CommandError::NoSuchGroup(g))),
                }
                Ok(None)
            }
            ClientMsg::ClaimTerritory { group, x, y, radius } => {
                finite(&[x, y, radius])?;
                let before = self.orders_of(player, group);
                let outcome = match self.world.claim_or_migrate(player, group, Vec2::new(x, y), radius) {
                    Ok(o) => o,
                    Err(CommandError::ClaimCooldown { ticks_left }) => return Err(self.cooldown_error(ticks_left)),
                    Err(err) => return Err(e(err)),
                };
                self.order_given(player, group);
                let mut data = match outcome {
                    ClaimOutcome::Claimed(t) => {
                        let mut data = serde_json::to_value(TerritoryView::from(t)).unwrap();
                        if (t.radius - radius).abs() > 0.01 {
                            data["note"] = if t.radius > radius {
                                format!("radius raised to the smallest territory, {:.0} tiles", t.radius)
                            } else {
                                format!(
                                    "radius cut to the group's limit, {:.1} tiles (it grows with the adults)",
                                    t.radius
                                )
                            }
                            .into();
                        }
                        let warnings = self.circle_report(player, group, t.center, t.radius, &mut data);
                        put_warnings(&mut data, warnings);
                        data
                    }
                    ClaimOutcome::Migrating { target, radius } => {
                        self.plan_route(player, group);
                        let mut data = serde_json::json!({
                            "x": x, "y": y, "radius": radius, "pending": true,
                            "migrating_to": { "x": target.x, "y": target.y },
                            "note": format!(
                                "the circle is more than {FAR_CLAIM_TILES:.0} tiles from the group: it migrates there \
                                 first and claims the territory when it arrives"
                            ),
                        });
                        if let Some((_, route)) = self.routes.get(&(player, group)) {
                            data["route"] = serde_json::to_value(route).unwrap_or_default();
                        }
                        let mut warnings = self.travel_warnings(player, group, true);
                        warnings.extend(self.circle_report(player, group, Vec2::new(x, y), radius.max(3.0), &mut data));
                        put_warnings(&mut data, warnings);
                        data
                    }
                };
                self.brood_report(player, group, &mut data);
                note_replaced(&mut data, &before, "territory");
                Ok(Some(data))
            }
            ClientMsg::AbandonTerritory { group } => {
                let t = self.world.abandon_territory(player, group).map_err(e)?;
                self.territory_alerted.retain(|&(p, g, _)| (p, g) != (player, group));
                let mut data = serde_json::to_value(TerritoryView::from(t)).unwrap();
                if self.world.settle_circle(player, group).is_some() {
                    data["note"] = "the group goes back to its home ground".into();
                }
                Ok(Some(data))
            }
            ClientMsg::Rally { group, x, y } => {
                finite(&[x, y])?;
                let before = self.orders_of(player, group);
                let r = self.world.rally_group(player, group, Vec2::new(x, y)).map_err(e)?;
                self.order_given(player, group);
                let mut data = serde_json::to_value(RallyView::from(r)).unwrap();
                let mut warnings = self.travel_warnings(player, group, false);
                warnings.extend(self.circle_report(player, group, r.center, r.radius, &mut data));
                put_warnings(&mut data, warnings);
                self.brood_report(player, group, &mut data);
                note_replaced(&mut data, &before, "rally");
                Ok(Some(data))
            }
            ClientMsg::CancelRally { group } => {
                self.world.cancel_rally(player, group).map_err(e)?;
                let back = self.world.settle_circle(player, group).is_some();
                Ok(Some(serde_json::json!({
                    "note": if back { "the group goes back to its home ground" } else { "the group roams freely" }
                })))
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
                // A radius of 0 or less pinned a 1-tile view (playtest 5).
                if radius <= 0.0 {
                    return Err(ErrorInfo::new("bad_coordinates", "a view radius must be positive"));
                }
                let p = self.world.map.clamp(Vec2::new(x, y));
                // Only where the player has organisms (or their lineage's
                // center), and no wider than they perceive there plus a
                // margin: a view anywhere scouted every player's animals.
                let reach = self.view_reach(player, p);
                if reach < 1.0 {
                    return Err(ErrorInfo::new("out_of_sight", "you can only look where your organisms are"));
                }
                let radius = radius.clamp(1.0, self.cfg.max_view_radius.min(reach));
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
            ClientMsg::RespondMating { request, accept, partner } => {
                self.respond_mating(player, request, accept, partner, out)
            }
            ClientMsg::OfferToLineage { mine, lineage, price } => {
                self.offer_to_lineage(player, mine, lineage, price, out)
            }
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
            ClientMsg::SubstituteMating { request, mine } => self.substitute_mating(player, request, mine, out),
            ClientMsg::CancelMating { request } => {
                let offer = self.offer_for(request, player)?;
                if !offer.status.is_open() {
                    return Err(ErrorInfo::new("offer_closed", format!("offer {request} is {:?}", offer.status)));
                }
                // An accepted agreement held the partner's dam: the requester
                // backing out pays for the hold as a lapse would (else
                // buying a rival's dam and cancelling before the deadline
                // locked her for free). The partner's owner backing out
                // costs the requester nothing.
                let held = offer.status == OfferStatus::Accepted && offer.from == player && offer.to != player;
                let info = held.then(|| {
                    let forfeit = self.forfeit_hold(request, "the buyer cancelled");
                    ErrorInfo::new("cancelled", format!("the agreement was cancelled after acceptance; {forfeit}"))
                });
                let data = info.as_ref().map(|i| serde_json::json!({ "forfeit": i.message }));
                self.close(request, OfferStatus::Cancelled, None, info, out);
                Ok(data)
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
                self.market.remove(&self.economy, listing);
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
            ClientMsg::MatingPrefs { block, unblock, auto_decline, blocked } => {
                // Only a real player can be blocked: not the wild (0), not a
                // made-up id, not oneself. Every one named is checked and
                // the bad ones are said (they used to be dropped quietly).
                let named: Vec<PlayerId> = blocked.iter().flatten().chain(&block).copied().collect();
                if named.contains(&player) {
                    return Err(ErrorInfo::new("self_block", "you can't block yourself"));
                }
                let unknown: Vec<PlayerId> = named
                    .iter()
                    .copied()
                    .filter(|&p| {
                        p == WILD || !(self.names.contains_key(&p) || self.world.lineages.iter().any(|l| l.owner == p))
                    })
                    .collect();
                if !unknown.is_empty() {
                    let list = unknown.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
                    return Err(ErrorInfo::new("no_such_player", format!("there is no player {list}"))
                        .with_detail(serde_json::json!({ "player": unknown[0], "players": unknown })));
                }
                if blocked.as_ref().is_some_and(|b| b.len() > MAX_BLOCKED) {
                    return Err(ErrorInfo::new(
                        "too_many_blocked",
                        format!("you can block up to {MAX_BLOCKED} players"),
                    ));
                }
                let prefs = self.mating_prefs.entry(player).or_default();
                if let Some(mut list) = blocked {
                    list.sort_unstable();
                    list.dedup();
                    prefs.blocked = list;
                }
                if let Some(p) = unblock {
                    prefs.blocked.retain(|&b| b != p);
                }
                if let Some(p) = block.filter(|&p| !prefs.blocked.contains(&p)) {
                    if prefs.blocked.len() >= MAX_BLOCKED {
                        return Err(ErrorInfo::new(
                            "too_many_blocked",
                            format!("you can block up to {MAX_BLOCKED} players"),
                        ));
                    }
                    prefs.blocked.push(p);
                    prefs.blocked.sort_unstable();
                }
                if let Some(on) = auto_decline {
                    prefs.auto_decline = on;
                }
                Ok(Some(serde_json::to_value(prefs.clone()).unwrap()))
            }
            ClientMsg::Ledger { limit } => {
                let view = LedgerView {
                    balance: self.balance(player),
                    escrow: self.escrow(player),
                    entries: self.ledger(player, limit.unwrap_or(LEDGER_DEFAULT).min(economy::LEDGER_KEPT)),
                };
                Ok(Some(serde_json::to_value(view).unwrap()))
            }
            ClientMsg::RenownShop => Ok(Some(serde_json::to_value(self.shop_view(player)).unwrap())),
            ClientMsg::BuyMatingSlot => Ok(Some(serde_json::to_value(self.buy_mating_slot(player)?).unwrap())),
            ClientMsg::Reinforce => {
                let (ids, shop, site) = self.reinforce(player)?;
                Ok(Some(serde_json::json!({ "added": ids, "shop": shop, "at": site.at, "group": site.group })))
            }
            ClientMsg::Compatibility => Ok(Some(serde_json::to_value(self.compatibility(player)).unwrap())),
            ClientMsg::Provision => {
                let (fed, shop) = self.provision(player)?;
                Ok(Some(serde_json::json!({ "fed": fed, "shop": shop })))
            }
            ClientMsg::SetNameplate { color, title } => {
                let (plate, shop) = self.set_nameplate(player, &color, &title)?;
                Ok(Some(serde_json::json!({ "nameplate": plate, "shop": shop })))
            }
            ClientMsg::NameSpecies { species, name } => {
                let name = self.name_species(player, species, &name)?;
                Ok(Some(serde_json::json!({ "species": species, "name": name })))
            }
            ClientMsg::Nameplates => Ok(Some(self.nameplates())),
            // The connection answers it (`http.rs`); nothing to do here.
            // Answered by the world thread (`crate::runner`) before it gets here.
            ClientMsg::Resync | ClientMsg::WatchSeason { .. } => Ok(None),
        }
    }

    /// `World::command_migrate` (the main group and every group with a
    /// member), finding the player's groups from the state index when it is
    /// current instead of a scan of every organism.
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
        self.check_partner_visible(player, partner)?;
        let b = self.world.organism(partner).ok_or(e(CommandError::NotFound(partner)))?;
        self.check_auction_lock(mine)?;
        self.check_auction_lock(partner)?;
        if b.owner == player {
            // Maturity first, then sex and compatibility (playtest 5).
            for o in [a, b] {
                if !o.matured {
                    return Err(e(CommandError::NotMature(o.id)));
                }
            }
            // Within the player's own organisms: no consent needed (and no
            // price). A pair that can't conceive would only use up a slot.
            if let Some(problem) = self.world.mating_preview(mine, partner).ok().as_ref().and_then(incompatibility) {
                return Err(problem);
            }
            let req = DirectedMating { player, a: mine, b: partner, partner_consent: false };
            self.state_cache.touch(mine);
            self.state_cache.touch(partner);
            let outcome = MatingOutcome::from(self.world.directed_mating(req).map_err(e)?);
            let tick = self.world.tick();
            self.log_mating(
                player,
                MatingRecord { tick, offer: None, mine, partner, outcome: Some(outcome), error: None },
            );
            return Ok(Some(serde_json::to_value(outcome).unwrap()));
        }
        let to = self.check_cross_pair(player, mine, partner)?;
        self.check_request_allowed(player, to)?;
        let price = price.filter(|&p| p > 0);
        if let Some(p) = price {
            self.check_funds(player, p)?;
        }
        let id = self.open_offer(player, to, mine, partner, price, OfferStatus::Pending, None)?;
        self.request_log.entry((player, to)).or_default().push_back((self.world.tick(), id));
        // Holds the requester's own female; the partner's waits for her
        // owner's acceptance (see `hold_consented_females`).
        self.hold_added(Some(id), None);
        let view = self.offer_view(id);
        let price = view.price.map_or(String::new(), |p| format!(" for {p} renown"));
        let text = format!("{} asks to mate their #{mine} with your #{partner}{price}.", view.from_name);
        self.notice(to, PushKind::Mating, text, Some(partner));
        out.push((Target::Player(to), ServerMsg::MatingRequest { offer: view.clone() }));
        Ok(Some(serde_json::json!({ "offer": view })))
    }

    /// Refuse a request `to` doesn't take: they blocked `from` or decline
    /// every request (`requests_refused`), or `from` already sent them
    /// [`REQUESTS_PER_TARGET`] within the window (`too_many_requests`).
    fn check_request_allowed(&mut self, from: PlayerId, to: PlayerId) -> Result<(), ErrorInfo> {
        if self.mating_prefs.get(&to).is_some_and(|p| p.auto_decline || p.blocked.contains(&from)) {
            return Err(ErrorInfo::new(
                "requests_refused",
                format!("{} isn't taking mating requests from you", self.name(to)),
            ));
        }
        let (tick, window) = (self.world.tick(), self.cfg.request_window_ticks);
        let log = self.request_log.entry((from, to)).or_default();
        while log.front().is_some_and(|&(t, _)| t + window <= tick) {
            log.pop_front();
        }
        if log.len() >= REQUESTS_PER_TARGET {
            let wait = log.front().map_or(0, |&(t, _)| t + window - tick);
            return Err(ErrorInfo::new(
                "too_many_requests",
                format!("you have sent {} {REQUESTS_PER_TARGET} requests recently; try again later", self.name(to)),
            )
            .with_detail(serde_json::json!({ "ticks_left": wait })));
        }
        Ok(())
    }

    /// `offer_to_lineage`: a request to any suitable animal of `lineage`
    /// (another player's living lineage), picked when its owner accepts.
    fn offer_to_lineage(
        &mut self,
        player: PlayerId,
        mine: OrgId,
        lineage: u32,
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
        if !a.matured {
            return Err(e(CommandError::NotMature(mine)));
        }
        if a.sex == Sex::Female && a.pregnancy.is_some() {
            return Err(e(CommandError::AlreadyPregnant));
        }
        let to = match self.world.lineages.get(lineage as usize) {
            Some(l) if l.owner != WILD && l.owner != player && self.lineage_of(l.owner) == Some(lineage) => l.owner,
            Some(l) if l.owner == player => {
                return Err(ErrorInfo::new("own_organism", "that is your own lineage: no consent or fee needed"))
            }
            _ => {
                return Err(ErrorInfo::new(
                    "unknown_lineage",
                    format!("lineage {lineage} isn't another player's living lineage"),
                ))
            }
        };
        self.check_auction_lock(mine)?;
        if self.world.mating_slots_left(player) == 0 {
            return Err(e(CommandError::NoSlotsLeft));
        }
        let open = self.offers.values().filter(|o| o.from == player && o.status.is_open()).count();
        if open >= self.cfg.max_open_offers {
            return Err(ErrorInfo::new("too_many_offers", format!("at most {} open offers", self.cfg.max_open_offers)));
        }
        self.check_request_allowed(player, to)?;
        let price = price.filter(|&p| p > 0);
        if let Some(p) = price {
            self.check_funds(player, p)?;
        }
        let id = self.open_offer(player, to, mine, 0, price, OfferStatus::Pending, None)?;
        self.offers.get_mut(&id).unwrap().lineage = Some(lineage);
        self.request_log.entry((player, to)).or_default().push_back((self.world.tick(), id));
        self.hold_added(Some(id), None);
        let view = self.offer_view(id);
        let price = view.price.map_or(String::new(), |p| format!(" for {p} renown"));
        let name = self.world.lineages[lineage as usize].name.clone();
        let text =
            format!("{} asks to mate their #{mine} with one of your {name}{price}: you pick which.", view.from_name);
        self.notice(to, PushKind::Mating, text, None);
        out.push((Target::Player(to), ServerMsg::MatingRequest { offer: view.clone() }));
        Ok(Some(serde_json::json!({ "offer": view })))
    }

    /// The partner for an offer to the lineage being accepted: `pick` if it
    /// is suitable, else the nearest suitable animal of the lineage to the
    /// sender's organism (adult, other sex, not pregnant, not promised or
    /// auctioned, fertile with it).
    fn pick_partner(&self, request: u64, pick: Option<OrgId>) -> Result<OrgId, ErrorInfo> {
        let o = &self.offers[&request];
        let a = self.world.organism(o.mine).ok_or_else(|| self.gone(o.mine))?;
        let busy = |id: OrgId| {
            self.offers.values().any(|x| x.id != o.id && x.status.is_open() && (x.mine == id || x.partner == id))
                || self.check_auction_lock(id).is_err()
        };
        let suits = |b: &Organism| {
            b.alive
                && b.owner == o.to
                && b.matured
                && b.sex != a.sex
                && !(b.sex == Sex::Female && b.pregnancy.is_some())
                && !busy(b.id)
                && self.world.mating_preview(o.mine, b.id).ok().as_ref().and_then(incompatibility).is_none()
        };
        if let Some(p) = pick {
            let b = self
                .world
                .organism(p)
                .filter(|b| b.owner == o.to)
                .ok_or_else(|| ErrorInfo::new("not_owned", format!("#{p} isn't one of your living animals")))?;
            if suits(b) {
                return Ok(p);
            }
            // The animal is the accepter's own: say why.
            let why = if !b.matured {
                "it isn't grown"
            } else if b.sex == a.sex {
                "it is the same sex as theirs"
            } else if b.sex == Sex::Female && b.pregnancy.is_some() {
                "it is pregnant"
            } else if busy(b.id) {
                "it is promised in another agreement or auction"
            } else {
                "the two can't conceive (body plans or genomes too far apart)"
            };
            return Err(ErrorInfo::new("partner_unavailable", format!("#{p} can't take this mating: {why}")));
        }
        let lineage = o.lineage;
        self.world
            .organisms
            .iter()
            .filter(|b| lineage.is_none_or(|l| b.lineage == l) && suits(b))
            .min_by(|x, y| x.pos.dist(a.pos).total_cmp(&y.pos.dist(a.pos)).then(x.id.cmp(&y.id)))
            .map(|b| b.id)
            .ok_or_else(|| {
                ErrorInfo::new(
                    "no_partner_available",
                    "none of your animals can take this mating now (the offer stays open)",
                )
            })
    }

    /// Refuse a promise or agreement for a dam in an open auction: she is
    /// locked until the auction closes or is withdrawn, so the winner's
    /// agreement can run (a stud may serve several).
    fn check_auction_lock(&self, organism: OrgId) -> Result<(), ErrorInfo> {
        let dam = self.world.organism(organism).is_some_and(|o| o.sex == Sex::Female);
        match self.economy.listing_of(organism).filter(|l| dam && l.auction.is_some()) {
            Some(l) => Err(ErrorInfo::new(
                "in_auction",
                format!("#{organism} is up for auction (#{}) until it closes or is withdrawn", l.id),
            )
            .with_detail(serde_json::json!({ "listing": l.id }))),
            None => Ok(()),
        }
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
        let offer = Offer {
            id,
            from,
            to,
            mine,
            partner,
            price,
            status,
            created_tick: self.world.tick(),
            listing,
            escrow: 0,
            fee: 0,
            accepted_tick: (status == OfferStatus::Accepted).then(|| self.world.tick()),
            warned: false,
            lineage: None,
            meet: None,
            lost: None,
            excused: false,
            partner_slept: false,
            paused: 0,
        };
        self.offers.insert(id, offer);
        if status == OfferStatus::Accepted {
            // The pair walks to meet (`World::set_rendezvous`).
            self.start_rendezvous(id);
        }
        if let Some(p) = price.filter(|&p| p > 0) {
            let what = match listing {
                Some(l) => {
                    format!("Stud fee for listing #{l} (#{mine} × {}'s #{partner}) held in escrow", self.name(to))
                }
                None if partner == 0 => {
                    format!("Price of offer #{id} (#{mine} × one of {}'s animals) held in escrow", self.name(to))
                }
                None => format!("Price of offer #{id} (#{mine} × {}'s #{partner}) held in escrow", self.name(to)),
            };
            let debit = i64::try_from(p).ok().map(|p| -p);
            match debit.map(|d| self.post(from, d, LedgerKind::Escrow, what, Some(id), listing)) {
                Some(Ok(_)) => {
                    let o = self.offers.get_mut(&id).unwrap();
                    o.escrow = p;
                    o.fee = economy::market_fee(p);
                }
                _ => {
                    self.offers.remove(&id);
                    let balance = self.balance(from);
                    return Err(ErrorInfo::new("insufficient_funds", format!("{p} renown needed, you have {balance}"))
                        .with_detail(serde_json::json!({ "balance": balance, "needed": p })));
                }
            }
        }
        // Its organisms stay agents from the next level-of-detail update.
        self.sync_focus();
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
        if a.sex == Sex::Female && a.pregnancy.is_some() {
            return Err(e(CommandError::AlreadyPregnant));
        }
        // Another player's female: her pregnancy is hidden state, so a
        // generic refusal (`already_pregnant` told a rival about it).
        if b.sex == Sex::Female && b.pregnancy.is_some() {
            return Err(ErrorInfo::new(
                "partner_unavailable",
                format!(
                    "#{partner} can't mate now (pregnant, nursing or not ready: its owner's own business); \
                     offer to the lineage instead and its owner picks a free animal"
                ),
            ));
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

    /// A listing (fee, or auction reserve) is at least [`LISTING_MIN_FEE`]:
    /// buying holds the animal (a dam from breeding, a stud from other
    /// buyers) until the two meet. (Playtest 5: a 1-renown stud purchase
    /// locked the stud.)
    fn check_listing_price(&self, price: u64) -> Result<(), ErrorInfo> {
        if price < LISTING_MIN_FEE {
            return Err(ErrorInfo::new(
                "bad_price",
                format!("a listing is at least {LISTING_MIN_FEE} renown (a buyer holds the animal until the two meet)"),
            )
            .with_detail(serde_json::json!({ "min": LISTING_MIN_FEE })));
        }
        Ok(())
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
        let synced = self.market.in_sync(&self.economy);
        let listed =
            if synced { self.market.listing_of(organism) } else { self.economy.listing_of(organism).map(|l| l.id) };
        if let Some(id) = listed {
            return Err(ErrorInfo::new("already_listed", format!("#{organism} is already listing #{id}")));
        }
        let count = if synced { self.market.listings_of(player) } else { self.economy.listings_of(player) };
        if count >= economy::MAX_LISTINGS {
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
        self.check_listing_price(fee)?;
        // A dam already promised in an open agreement can't take a buyer
        // (playtest 3: a promised dam got a fixed listing).
        let dam = self.world.organism(organism).is_some_and(|o| o.sex == Sex::Female);
        if dam && self.offers.values().any(|o| o.status.is_open() && (o.mine == organism || o.partner == organism)) {
            return Err(ErrorInfo::new(
                "organism_busy",
                format!("#{organism} is already promised in an open agreement"),
            ));
        }
        let tick = self.world.tick();
        let expires = expires_days.map(|d| tick + d as u64 * self.world.config.ticks_per_day as u64);
        let l = self.economy.add_listing(player, organism, fee, tick, expires);
        self.listing_added(&l);
        self.hold_added(None, Some(l.id));
        self.sync_focus();
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
                    "an auction lasts 1 to {} days (days) or 1 to {longest} ticks (ticks), not both; \
                     at least {} real minutes whatever is asked",
                    economy::MAX_AUCTION_DAYS,
                    (self.cfg.min_auction_ticks as f64 * self.cfg.tick_ms / 60_000.0).round()
                ),
            ));
        };
        // At least `min_auction_ticks` of wall-clock time, whatever was asked
        // (a 1-tick auction closed before anyone could bid).
        let duration = duration.max(self.cfg.min_auction_ticks);
        self.check_listable(player, organism, reserve)?;
        self.check_listing_price(reserve)?;
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
        self.listing_added(&l);
        self.hold_added(None, Some(l.id));
        self.sync_focus();
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
        self.check_auction_lock(mine)?;
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
        if let Some(x) = self.economy.listings.get(&listing) {
            self.market.update(&self.economy, x);
        }
        self.hold_added(None, Some(listing));
        self.sync_focus();
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
        self.hold_added(Some(id), None);
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
        // `execute` tells both sides (apart: the agreement's update; else
        // the outcome), once (playtest 5: every purchase update came twice).
        let _ = (view, self.execute(id, out));
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
        self.check_auction_lock(mine)?;
        if self.listing_busy(&l) {
            return Err(ErrorInfo::new(
                "listing_busy",
                format!("#{} is already promised in an open agreement", l.organism),
            ));
        }
        if self.offers.values().any(|o| o.status.is_open() && (o.mine == mine || o.partner == mine)) {
            return Err(ErrorInfo::new("organism_busy", format!("#{mine} is already promised in an open agreement")));
        }
        // A listed stud walks to one meeting at a time: the next buyer waits
        // until it is done.
        if self.in_agreement(l.organism, 0) {
            return Err(ErrorInfo::new(
                "listing_busy",
                format!("#{} is walking to another agreed mating: try again once it is done", l.organism),
            ));
        }
        self.check_funds(player, l.fee)?;
        let id = self.open_offer(player, to, mine, l.organism, Some(l.fee), OfferStatus::Accepted, Some(listing))?;
        if let Some(x) = self.economy.listings.get_mut(&listing) {
            x.sales += 1;
        }
        self.hold_added(Some(id), Some(listing));
        let view = self.offer_view(id);
        let role = self.world.organism(l.organism).map_or("organism", |o| role_name(o.sex));
        let text = format!(
            "{} bought a mating with your {role} #{} for {} renown (held in escrow until #{mine} meets it).",
            self.name(player),
            l.organism,
            l.fee
        );
        self.alert(to, AlertKind::Market, text, Some(l.organism), out);
        // Try at once; apart, the agreement waits for the two to meet.
        // `execute` sends both sides the update, once.
        let _ = self.execute(id, out);
        Ok(Some(serde_json::json!({ "offer": view })))
    }

    /// A listed animal that can't take a buyer now: in an accepted
    /// agreement (walking to its meeting), or a pregnant dam (buyers wait
    /// until she has given birth). A pending request naming it doesn't
    /// count (playtest 5: a free request blocked a listed dam for its 10
    /// minutes).
    fn listing_busy(&self, l: &Listing) -> bool {
        self.world.organism(l.organism).is_some_and(|o| o.sex == Sex::Female && o.pregnancy.is_some())
            || self.in_agreement(l.organism, 0)
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
            // The region (16 tiles) it is in, not its live position: a
            // predator player could otherwise hunt listed animals down.
            x: coarse(o.pos.x),
            y: coarse(o.pos.y),
            traits: LISTING_TRAITS.iter().map(|&g| TraitView { gene: gene_key(g), value: o.pheno[g] }).collect(),
            preview,
            compatible: None,
            body_plan_fertility: None,
            kind: if l.auction.is_some() { ListingKind::Auction } else { ListingKind::Fixed },
            auction,
        })
    }

    /// One page of the open listings matching `q`, in its order, with
    /// compatibility against `q.mine`, and each listing's body-plan
    /// compatibility with the player's lineage.
    pub fn market(&self, player: PlayerId, q: &MarketQuery) -> Result<MarketView, ErrorInfo> {
        let mut page = self.market_page(player, q)?;
        self.annotate_compatibility(player, &mut page.listings);
        if let Some(own) = page.own.as_mut() {
            self.annotate_compatibility(player, own);
        }
        Ok(page)
    }

    fn market_page(&self, player: PlayerId, q: &MarketQuery) -> Result<MarketView, ErrorInfo> {
        let sort = q.sort.unwrap_or_default();
        let early = q.mine.is_some() && (q.compatible || sort == MarketSort::Fertility);
        // The index counts by kind and role only; other filters (and
        // orders that need every preview) take the scan.
        let indexed = !early
            && q.max_price.is_none()
            && q.lineage.is_none()
            && q.species.is_none()
            && self.market.usable(&self.economy, self.world.tick());
        if !indexed {
            return self.market_scan(player, q);
        }
        let page = self.market_indexed(player, q);
        #[cfg(test)]
        {
            market::INDEXED_PAGES.with(|n| n.set(n.get() + 1));
            if market::CHECK_PAGES.with(|c| c.get()) {
                assert!(self.market.matches(&self.economy, &self.world), "market index out of date");
                assert_eq!(page, self.market_scan(player, q), "indexed market page differs from the scan");
            }
        }
        page
    }

    /// Whether listing `l` (of organism `o`) passes `q`'s filters other than
    /// `compatible`.
    fn listing_matches(
        &self,
        player: PlayerId,
        q: &MarketQuery,
        l: &Listing,
        o: &Organism,
        species: Option<&BTreeSet<u32>>,
    ) -> bool {
        let role = if o.sex == Sex::Male { ListingRole::Stud } else { ListingRole::Dam };
        let kind = if l.auction.is_some() { ListingKind::Auction } else { ListingKind::Fixed };
        !((q.hide_own && l.owner == player)
            || q.kind.is_some_and(|k| k != kind)
            || q.role.is_some_and(|r| r != role)
            || q.max_price.is_some_and(|m| l.price() > m)
            || q.lineage.is_some_and(|x| o.lineage != x)
            || species.is_some_and(|s| !s.contains(&o.lineage)))
    }

    /// [`Game::market`] from the index (`game::market`): a walk of the
    /// order from the cursor until a page and one more match, the total
    /// from the counts. Only for queries without `max_price`, `lineage`,
    /// `species` or every-candidate previews.
    fn market_indexed(&self, player: PlayerId, q: &MarketQuery) -> Result<MarketView, ErrorInfo> {
        let limit = q.limit.unwrap_or(MARKET_PAGE).clamp(1, MAX_MARKET_PAGE);
        let sort = q.sort.unwrap_or_default();
        let after = q.cursor.as_deref().map(parse_cursor).transpose()?;
        let mut page: Vec<(market::Key, &Listing)> = Vec::with_capacity(limit + 1);
        for (key, id) in self.market.walk(sort, after) {
            let Some(l) = self.economy.listings.get(&id) else { continue };
            let Some(o) = self.world.organism(l.organism) else { continue };
            if self.listing_matches(player, q, l, o, None) {
                page.push((key, l));
                if page.len() > limit {
                    break;
                }
            }
        }
        let next_cursor = (page.len() > limit).then(|| cursor_text(page[limit - 1].0));
        let listings = page
            .iter()
            .take(limit)
            .filter_map(|&(_, l)| self.listing_view(l, q.mine.and_then(|m| self.listing_preview(player, m, l))))
            .collect();
        let own = q.with_own.then(|| {
            (self.market.owned_by(player).rev())
                .filter_map(|id| self.economy.listings.get(&id))
                .filter_map(|l| self.listing_view(l, None))
                .collect()
        });
        Ok(MarketView {
            balance: self.balance(player),
            escrow: self.escrow(player),
            slots_left: self.world.mating_slots_left(player),
            listings,
            total: self.market.count(player, q.kind, q.role, q.hide_own),
            next_cursor,
            own,
        })
    }

    /// [`Game::market`] by a pass over every listing.
    fn market_scan(&self, player: PlayerId, q: &MarketQuery) -> Result<MarketView, ErrorInfo> {
        let limit = q.limit.unwrap_or(MARKET_PAGE).clamp(1, MAX_MARKET_PAGE);
        let sort = q.sort.unwrap_or_default();
        let after = q.cursor.as_deref().map(parse_cursor).transpose()?;
        let species: Option<BTreeSet<u32>> = q.species.map(|id| {
            self.world.chronicle.species.get(id).map(|s| s.lineages.iter().map(|l| l.0).collect()).unwrap_or_default()
        });
        // Previews of every candidate only when the filter or the order
        // needs them; otherwise only for the page.
        let early = q.mine.is_some() && (q.compatible || sort == MarketSort::Fertility);
        // (Previews boxed: most queries have none, and a row is moved
        // around while the page is picked.)
        let mut rows: Vec<MarketRow<'_>> = Vec::with_capacity(self.economy.listings.len());
        for l in self.economy.listings.values() {
            let Some(o) = self.world.organism(l.organism) else { continue };
            if !self.listing_matches(player, q, l, o, species.as_ref()) {
                continue;
            }
            let preview = q.mine.filter(|_| early).and_then(|m| self.listing_preview(player, m, l));
            if q.compatible && q.mine.is_some() && !preview.as_ref().is_some_and(|p| p.compatible) {
                continue;
            }
            rows.push((market_key(sort, l, preview.as_ref(), q.mine.is_some()), l, preview.map(Box::new)));
        }
        let total = rows.len();
        // The page is the `limit` smallest keys after the cursor (keys are
        // unique), plus one to tell whether there is a next page: select
        // those and sort only them.
        if let Some(c) = after {
            rows.retain(|r| r.0 > c);
        }
        if rows.len() > limit + 1 {
            rows.select_nth_unstable_by_key(limit, |r| r.0);
            rows.truncate(limit + 1);
        }
        rows.sort_unstable_by_key(|r| r.0);
        let page = &rows[..];
        let next_cursor = (page.len() > limit).then(|| cursor_text(page[limit - 1].0));
        let view = |(_, l, p): &MarketRow<'_>| {
            let p = if early { p.as_deref().cloned() } else { q.mine.and_then(|m| self.listing_preview(player, m, l)) };
            self.listing_view(l, p)
        };
        let listings = page.iter().take(limit).filter_map(view).collect();
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
            self.market.remove(&self.economy, id);
            if l.auction.is_some() && !dead {
                self.settle_auction(&l, out);
                continue;
            }
            if let Some(a) = &l.auction {
                if let Some(b) = &a.high {
                    let why = self.why_unlisted(organism);
                    let text = format!("Refund: auction #{id} ended ({organism} {why}), {} back", b.amount);
                    let _ = self.post(b.bidder, b.amount as i64, LedgerKind::Refund, text, None, Some(id));
                    let text = format!("Auction #{id} ended: #{organism} {why}; your {} renown is refunded.", b.amount);
                    self.alert(b.bidder, AlertKind::Market, text, Some(organism), out);
                }
                self.record_auction(&l, "ended", None);
            }
            let text = if dead {
                format!("Listing #{id} closed: #{organism} {}.", self.why_unlisted(organism))
            } else {
                format!("Listing #{id} of #{organism} expired.")
            };
            self.alert(owner, AlertKind::Market, text, Some(organism), out);
        }
        // Every listing left has a living organism of its owner's until the
        // next tick: the index's counts are exact.
        self.market.validated = Some(tick);
    }

    /// A listing was just created: index it.
    fn listing_added(&mut self, l: &Listing) {
        let sex = self.world.organism(l.organism).map(|o| o.sex);
        self.market.insert(&self.economy, l, sex);
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
                    // The day just ended, counted from 1 (a record is taken
                    // as the next day starts: `record.day` is that day's index).
                    record.day.max(1),
                    share * 100.0,
                    weighted * 100.0
                );
                let for_day = record.day.saturating_sub(1);
                let _ = self.post_on(p, income as i64, LedgerKind::Income, text, None, None, for_day);
            }
        }
    }

    fn respond_mating(
        &mut self,
        player: PlayerId,
        request: u64,
        accept: bool,
        pick: Option<OrgId>,
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
        let (mine, partner) = (offer.mine, offer.partner);
        let offer_lineage_open = partner == 0;
        if !accept {
            self.close(request, OfferStatus::Declined, None, None, out);
            return Ok(None);
        }
        // An offer to the lineage: the owner's pick, or the nearest
        // available animal.
        let partner = if partner == 0 {
            let p = self.pick_partner(request, pick)?;
            self.offers.get_mut(&request).unwrap().partner = p;
            p
        } else {
            partner
        };
        // An animal walks to one meeting at a time (playtest 4: one male held
        // several agreements and could reach none of them).
        for id in [mine, partner] {
            if self.in_agreement(id, request) {
                if offer_lineage_open {
                    self.offers.get_mut(&request).unwrap().partner = 0;
                }
                return Err(ErrorInfo::new(
                    "organism_busy",
                    format!("#{id} is already walking to another agreed mating: accept again once it is done"),
                ));
            }
        }
        // The accepter's own female can't take it while pregnant: say so
        // now (her state is theirs to know) instead of failing later with
        // "already pregnant".
        if self.world.organism(partner).is_some_and(|b| b.sex == Sex::Female && b.pregnancy.is_some()) {
            if offer_lineage_open {
                self.offers.get_mut(&request).unwrap().partner = 0;
            }
            return Err(ErrorInfo::new(
                "partner_unavailable",
                format!("your #{partner} is pregnant: it can't take this mating now"),
            ));
        }
        self.check_auction_lock(mine)?;
        self.check_auction_lock(partner)?;
        let tick = self.world.tick();
        let o = self.offers.get_mut(&request).unwrap();
        o.status = OfferStatus::Accepted;
        o.accepted_tick = Some(tick);
        let (from, to) = (o.from, o.to);
        // An accepted request is no spam (`check_request_allowed`).
        if let Some(log) = self.request_log.get_mut(&(from, to)) {
            log.retain(|&(_, id)| id != request);
        }
        // The pair walks to meet (`World::set_rendezvous`).
        let _ = (mine, partner);
        self.start_rendezvous(request);
        self.hold_added(Some(request), None);
        // Try at once; if the organisms are apart, the agreement stands until
        // they meet (checked every tick) or its deadline
        // (`GameConfig::purchase_deadline_ticks`) passes.
        // The responder's answer succeeded either way; the mating's own
        // result goes to both sides as a `mating_update`.
        let outcome = self.execute(request, out).ok();
        // The offer as it stands (accepted and walking to meet, or completed).
        let offer = self.offers.contains_key(&request).then(|| self.offer_view(request));
        Ok(Some(serde_json::json!({ "offer": offer, "outcome": outcome })))
    }

    /// Run an accepted agreement. Notifies both sides of the result; a
    /// `too_far` failure leaves it accepted.
    fn execute(&mut self, request: u64, out: &mut Outbox) -> Result<MatingOutcome, ErrorInfo> {
        let o = self.offers[&request].clone();
        let partner_ok = self.world.organism(o.partner).is_some_and(|b| b.owner == o.to);
        let gone = [o.mine, o.partner].into_iter().find(|&id| self.world.organism(id).is_none());
        let result = if let Some(id) = gone {
            let info = self.gone(id);
            // The requester's animal died after acceptance: the agreement
            // waits for another (`buyer_lost`), it doesn't fail refunded.
            if o.status == OfferStatus::Accepted && id == o.mine && self.world.organism(o.partner).is_some() {
                return Err(ErrorInfo::new(
                    "lost",
                    format!(
                        "#{} {}: send another animal (`substitute_mating`) or cancel",
                        o.mine,
                        o.lost.as_deref().unwrap_or("died")
                    ),
                ));
            }
            self.close(request, OfferStatus::Failed, None, Some(info.clone()), out);
            return Err(info);
        } else if partner_ok {
            self.state_cache.touch(o.mine);
            self.state_cache.touch(o.partner);
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
                // Not a failure: the agreement stands and the pair walks to
                // meet (`World::set_rendezvous`); the update carries where
                // they are (`OfferView::meet`), no error.
                let info = ErrorInfo::from(&err);
                let view = self.offer_view(request);
                for p in [o.from, o.to] {
                    out.push((
                        Target::Player(p),
                        ServerMsg::MatingUpdate { offer: view.clone(), outcome: None, error: None },
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

    /// `not_owned` for a rival's animal the player can't see reads as
    /// `not_found`, like an id that doesn't exist (playtest 5: possess and
    /// split told living rival ids from dead ones anywhere on the map).
    fn hide_unseen(&self, player: PlayerId, e: ErrorInfo, named: &[OrgId]) -> ErrorInfo {
        if e.code != "not_owned" {
            return e;
        }
        let hidden = named.iter().copied().find(|&id| {
            self.world.organism(id).is_some_and(|o| {
                o.owner != player && self.view_reach(player, o.pos) + (NAME_MARGIN - VIEW_MARGIN) < 0.0
            })
        });
        match hidden {
            Some(id) => ErrorInfo::from(&CommandError::NotFound(id)),
            None => e,
        }
    }

    /// A partner a player may name in a request or a preview: one of
    /// theirs, one in their sight, one on the market, or one in an open
    /// offer of theirs. Else `out_of_sight`, also for an id that doesn't
    /// exist, so neither command tells a rival's animal's existence, owner,
    /// sex or maturity anywhere on the map.
    fn check_partner_visible(&self, player: PlayerId, partner: OrgId) -> Result<(), ErrorInfo> {
        let seen = self
            .world
            .organism(partner)
            .is_some_and(|b| b.owner == player || self.view_reach(player, b.pos) + (NAME_MARGIN - VIEW_MARGIN) >= 0.0);
        let public = || {
            self.world.organism(partner).is_some()
                && (self.economy.listings.values().any(|l| l.organism == partner)
                    || self.offers.values().any(|o| {
                        o.status.is_open()
                            && (o.from == player || o.to == player)
                            && (o.mine == partner || o.partner == partner)
                    }))
        };
        if seen || public() {
            return Ok(());
        }
        Err(ErrorInfo::new(
            "out_of_sight",
            "you can only name an animal you can see, one on the market or one in your offers",
        )
        .with_detail(serde_json::json!({ "organism": partner })))
    }

    /// What a request for `mine` × `partner` would give; see
    /// [`ClientMsg::PreviewMating`].
    fn preview_mating(&self, player: PlayerId, mine: OrgId, partner: OrgId) -> Result<MatingPreviewView, ErrorInfo> {
        let e = |err: CommandError| ErrorInfo::from(&err);
        let a = self.world.organism(mine).ok_or(e(CommandError::NotFound(mine)))?;
        if a.owner != player {
            return Err(e(CommandError::NotOwned(mine)));
        }
        let partner_seen =
            self.world.organism(partner).is_some_and(|b| b.owner == player || self.in_sight(player, b.pos));
        self.check_partner_visible(player, partner)?;
        // Maturity before sex (a juvenile read `same_sex`: playtest 5).
        for id in [mine, partner] {
            if self.world.organism(id).is_some_and(|o| !o.matured) {
                return Err(e(CommandError::NotMature(id)));
            }
        }
        let p = self.world.mating_preview(mine, partner).map_err(e)?;
        Ok(MatingPreviewView {
            mine,
            partner,
            compatible: p.fertility > 0.0,
            fertility: p.fertility,
            body_plan_fertility: p.body_plan_fertility,
            distance_fertility: p.distance_fertility,
            // Exact gene differences only for a partner in plain sight (one
            // just inside the naming margin got its exact genes: playtest 4).
            body_plan_gaps: body_plan_gaps(&p)
                .into_iter()
                .map(|g| if partner_seen { g } else { BodyPlanGap { gap: (g.gap * 10.0).round() / 10.0, ..g } })
                .collect(),
            mismatch: p.mismatch,
            cost: p.cost,
            failure_chance: p.failure_chance,
            // Exact only for a partner the player can see: distances to
            // hidden organisms would locate them (rounded up to 16 tiles).
            distance: if partner_seen { p.distance } else { (p.distance / LISTING_CELL).ceil() * LISTING_CELL },
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
        self.held_tick = Some(self.world.tick());
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

    /// [`Game::hold_consented_females`] after a command that opened or
    /// changed `offer` and/or opened or changed `listing` and nothing else:
    /// every other female was held at this tick already, with the same
    /// deadline (a hold only ever extends), so only those two can add one.
    /// The whole scan when no full hold ran this tick.
    fn hold_added(&mut self, offer: Option<u64>, listing: Option<u64>) {
        #[cfg(test)]
        if self.reference {
            return self.hold_consented_females();
        }
        if self.held_tick != Some(self.world.tick()) {
            return self.hold_consented_females();
        }
        let until = self.world.tick() + 2;
        let mut ids: Vec<OrgId> = Vec::new();
        if let Some(o) = offer.and_then(|id| self.offers.get(&id)).filter(|o| o.status.is_open()) {
            ids.push(o.mine);
            if o.status == OfferStatus::Accepted {
                ids.push(o.partner);
            }
        }
        if let Some(l) = listing.and_then(|id| self.economy.listings.get(&id)) {
            ids.push(l.organism);
            ids.extend(l.auction.as_ref().and_then(|a| a.high.as_ref()).map(|b| b.mine));
        }
        for id in ids {
            if self.world.organism(id).is_some_and(|x| x.sex == Sex::Female) {
                self.world.hold_breeding(id, until);
            }
        }
    }

    /// A bought listing or won auction whose buyer never brought their
    /// organism to the lot within [`GameConfig::purchase_deadline_ticks`]:
    /// it lapses (the dam is freed), the seller keeps [`LAPSE_FEE_PERCENT`]
    /// of the fee and the rest is refunded; with nothing paid, the buyer
    /// forfeits a mating slot instead. Buying a rival's dam and never coming
    /// used to hold her until the season ended.
    fn lapse(&mut self, id: u64, out: &mut Outbox) {
        let o = self.offers[&id].clone();
        let forfeit = self.forfeit_hold(id, "the buyer never came");
        let what = match &o.lost {
            Some(how) => format!("#{} {how} and no other animal came to #{} within the deadline", o.mine, o.partner),
            None => format!("#{} never came to #{} within the deadline", o.mine, o.partner),
        };
        let info = ErrorInfo::new("lapsed", format!("{what}: the agreement lapsed; {forfeit}"));
        self.close(id, OfferStatus::Expired, None, Some(info), out);
    }

    /// What the requester of accepted agreement `id` pays for having held
    /// the partner's dam and not mating: [`LAPSE_FEE_PERCENT`] of the
    /// escrowed price (at least [`LAPSE_FEE_MIN`], at most all of it) to the
    /// partner's owner, or a mating slot
    /// when nothing was paid. Taken from the escrow before it closes, so the
    /// rest is refunded. Says what was forfeit.
    fn forfeit_hold(&mut self, id: u64, why: &str) -> String {
        let o = self.offers[&id].clone();
        if o.excused {
            return format!(
                "nothing is forfeit: #{} was lost through no fault of {} ({})",
                o.mine,
                self.name(o.from),
                o.lost.as_deref().unwrap_or("died")
            );
        }
        if o.partner_slept {
            return format!("nothing is forfeit: {}'s #{} slept through part of it", self.name(o.to), o.partner);
        }
        let fee = if o.escrow > 0 { (o.escrow * LAPSE_FEE_PERCENT / 100).max(LAPSE_FEE_MIN).min(o.escrow) } else { 0 };
        if fee > 0 {
            self.offers.get_mut(&id).unwrap().escrow -= fee;
            let what = o.listing.map_or(format!("offer #{id}"), |l| format!("listing #{l}"));
            let text = format!("Hold fee: {fee} renown from {} for {what} ({why})", self.name(o.from));
            let _ = self.post(o.to, fee as i64, LedgerKind::Sale, text, Some(id), o.listing);
            format!("{fee} renown of the fee is forfeit to {}", self.name(o.to))
        } else if self.world.forfeit_mating_slot(o.from) {
            "a mating slot is forfeit".to_string()
        } else {
            "nothing was forfeit".to_string()
        }
    }

    /// How an offer's partner animal reads in texts: `#id`, or for an offer
    /// to a lineage not yet accepted "one of their <lineage>".
    fn partner_text(&self, o: &Offer) -> String {
        match (o.partner, o.lineage) {
            (0, Some(l)) => {
                let name = self.world.lineages.get(l as usize).map_or("animals".to_string(), |x| x.name.clone());
                format!("one of {}'s {name}", self.name(o.to))
            }
            (p, _) => format!("{}'s #{p}", self.name(o.to)),
        }
    }

    /// Tell the sender a pending request expires soon (once).
    fn warn_expiry(&mut self, id: u64, ticks_left: u64, out: &mut Outbox) {
        let o = self.offers.get_mut(&id).unwrap();
        o.warned = true;
        let o = o.clone();
        let minutes = (ticks_left as f64 * self.cfg.tick_ms / 60_000.0).ceil().max(1.0);
        let text = format!(
            "Your request #{} (#{} × {}) expires in about {minutes:.0} minute{} without an answer.",
            o.id,
            o.mine,
            self.partner_text(&o),
            if minutes == 1.0 { "" } else { "s" }
        );
        self.alert_push(o.from, AlertKind::Market, Some(PushKind::Market), text, Some(o.mine), out);
    }

    /// Why listed organism `id` left its owner's hands: joined another
    /// player's lineage, died (how), or left the world with a retired
    /// lineage (playtest 5: retired animals read "no longer alive").
    fn why_unlisted(&self, id: OrgId) -> String {
        if let Some(o) = self.world.organism(id) {
            return format!("joined {}'s lineage", self.name(o.owner));
        }
        if self.world.organisms.iter().any(|o| o.id == id && !o.alive) {
            return self.gone(id).message.replace(&format!("organism {id} "), "").replace(" before the mating", "");
        }
        "left the world with its retired lineage".to_string()
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
        if self.world.rendezvous_of(o.mine) == Some(o.partner) {
            self.world.clear_rendezvous(o.mine);
        }
        let view = self.offer_view(request);
        let tick = self.world.tick();
        // A pregnancy is told only to the female's owner: the other party
        // reads a generic `partner_unavailable` (as `request_mating` says).
        let pregnant_owners: Vec<PlayerId> = [o.mine, o.partner]
            .iter()
            .filter_map(|&id| self.world.organism(id))
            .filter(|x| x.sex == Sex::Female && x.pregnancy.is_some())
            .map(|x| x.owner)
            .collect();
        let error_for = |p: PlayerId| match &error {
            Some(e) if e.code == "already_pregnant" && !pregnant_owners.contains(&p) => {
                Some(ErrorInfo::new("partner_unavailable", "the partner can't take this mating now"))
            }
            e => e.clone(),
        };
        for p in [o.from, o.to] {
            let error = error_for(p);
            out.push((
                Target::Player(p),
                ServerMsg::MatingUpdate { offer: view.clone(), outcome, error: error.clone() },
            ));
            let (mine, partner) = if p == o.from { (o.mine, o.partner) } else { (o.partner, o.mine) };
            let record = MatingRecord { tick, offer: Some(view.clone()), mine, partner, outcome, error };
            self.log_mating(p, record);
        }
        // Only requests the target declined (or hasn't answered yet) count
        // against the sender's per-target limit (`check_request_allowed`):
        // accepted, completed, expired, cancelled or failed ones don't, so
        // the limiter guards against spam without capping trade.
        if status != OfferStatus::Declined {
            if let Some(log) = self.request_log.get_mut(&(o.from, o.to)) {
                log.retain(|&(_, id)| id != o.id);
            }
        }
        let settlement = self.settle(&o, status, out);
        if status == OfferStatus::Completed && o.from != o.to {
            // The buyer hears the mating ran (a push: they may have left
            // the escort to the game).
            let how = match outcome {
                Some(m) if m.conceived => "conceived",
                Some(_) => "did not conceive",
                None => "ran",
            };
            let paid = if o.escrow > 0 { format!("; {} renown paid from escrow", o.escrow) } else { String::new() };
            let text = format!("Mating done: your #{} with {}'s #{} {how}{paid}.", o.mine, self.name(o.to), o.partner);
            self.alert_push(o.from, AlertKind::Market, Some(PushKind::Mating), text, Some(o.mine), out);
        }
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
            // The market fee is destroyed: only the rest is paid out.
            let fee = o.fee.min(amount);
            let net = amount - fee;
            let fee_note = if fee > 0 { format!(", less a {fee} renown market fee") } else { String::new() };
            let text =
                format!("Sale: {net} renown from {from} for {what}{fee_note} (your #{} with #{})", o.partner, o.mine);
            if self.post(o.to, net as i64, LedgerKind::Sale, text.clone(), Some(o.id), o.listing).is_ok() {
                self.season.add_sale(o.to, o.from, net);
            }
            self.alert(o.to, AlertKind::Market, format!("{text}."), Some(o.partner), out);
            // The payer hears it with the outcome (`Game::close`).
            let _ = to;
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
            expires_tick: o.status.is_open().then(|| self.offer_deadline(o)),
            lineage: o.lineage.filter(|_| o.partner == 0),
            meet: (o.status == OfferStatus::Accepted).then(|| self.meet_view(o)).flatten(),
            lost: o.lost.clone().filter(|_| o.status == OfferStatus::Accepted),
        }
    }

    /// Accepted agreement `id`'s requester animal is dead: the first time,
    /// say how and that another may be sent; at the deadline it lapses
    /// (forfeiting the hold fee unless the loss was outside the
    /// requester's control). Playtest 5: every purchase failed with a full
    /// refund when the buyer's animal died, which also held a dam for free.
    fn buyer_lost(&mut self, id: u64, due: bool, out: &mut Outbox) {
        let o = self.offers[&id].clone();
        if o.lost.is_none() {
            let cause = self.world.organisms.iter().find(|x| x.id == o.mine && !x.alive).and_then(|x| x.death);
            let how = self
                .gone(o.mine)
                .message
                .replace(&format!("organism {} ", o.mine), "")
                .replace(" before the mating", "");
            let excused = matches!(
                cause,
                Some(
                    DeathCause::Predation
                        | DeathCause::Disease
                        | DeathCause::Recessive
                        | DeathCause::Attrition
                        | DeathCause::WinterSleep
                        | DeathCause::Journey
                )
            );
            let x = self.offers.get_mut(&id).unwrap();
            x.lost = Some(how.clone());
            x.excused = excused;
            x.meet = None;
            self.world.clear_rendezvous(o.partner);
            let minutes =
                (self.offer_deadline(&o).saturating_sub(self.world.tick()) as f64 * self.cfg.tick_ms / 60_000.0).ceil();
            let fee = if excused {
                "nothing is forfeit if none comes"
            } else {
                "else the hold fee is forfeit at the deadline"
            };
            let text = format!(
                "Your #{} {how} on the way to {}: send another animal for agreement #{id} within about {minutes:.0} minutes, or cancel ({fee}).",
                o.mine,
                self.partner_text(&o)
            );
            self.alert_push(o.from, AlertKind::Market, Some(PushKind::Market), text, None, out);
            let text = format!(
                "{}'s #{} {how} on the way to your #{}: they may send another animal before the deadline.",
                self.name(o.from),
                o.mine,
                o.partner
            );
            self.alert(o.to, AlertKind::Market, text, Some(o.partner), out);
            let view = self.offer_view(id);
            for p in [o.from, o.to] {
                out.push((
                    Target::Player(p),
                    ServerMsg::MatingUpdate { offer: view.clone(), outcome: None, error: None },
                ));
            }
        }
        if due {
            self.lapse(id, out);
        }
    }

    /// `substitute_mating`: the requester of an accepted agreement whose
    /// animal died sends `mine` instead (the same checks as a purchase).
    fn substitute_mating(
        &mut self,
        player: PlayerId,
        request: u64,
        mine: OrgId,
        out: &mut Outbox,
    ) -> Result<Option<serde_json::Value>, ErrorInfo> {
        let o = self.offer_for(request, player)?;
        if o.from != player || o.status != OfferStatus::Accepted {
            return Err(ErrorInfo::new(
                "not_substitutable",
                "only the requester of an accepted agreement can send another animal",
            ));
        }
        if self.world.organism(o.mine).is_some() {
            return Err(ErrorInfo::new(
                "not_substitutable",
                format!("#{} is still alive: it is the one going", o.mine),
            ));
        }
        match self.check_cross_pair(player, mine, o.partner) {
            Ok(_) => {}
            Err(e) if e.code == "too_many_offers" => {}
            Err(e) => return Err(e),
        }
        self.check_auction_lock(mine)?;
        if self.offers.values().any(|x| x.id != request && x.status.is_open() && (x.mine == mine || x.partner == mine))
        {
            return Err(ErrorInfo::new("organism_busy", format!("#{mine} is already promised in an open agreement")));
        }
        let x = self.offers.get_mut(&request).unwrap();
        let old = x.mine;
        x.mine = mine;
        x.lost = None;
        x.excused = false;
        x.meet = None;
        self.start_rendezvous(request);
        self.sync_focus();
        let o = self.offers[&request].clone();
        let text = format!("{} sends #{mine} instead of #{old} to your #{}.", self.name(player), o.partner);
        self.alert(o.to, AlertKind::Market, text, Some(o.partner), out);
        let view = self.offer_view(request);
        let _ = self.execute(request, out);
        Ok(Some(serde_json::json!({ "offer": view })))
    }

    /// Walk an accepted agreement's pair to its meeting point, fixing the
    /// point the first time (`World::meeting_point`: dry land near their
    /// midpoint, so the walk doesn't end in a lake).
    fn start_rendezvous(&mut self, id: u64) {
        let Some(o) = self.offers.get(&id) else { return };
        let (a, b) = (o.mine, o.partner);
        let meet = match o.meet {
            Some((x, y)) => Some(Vec2::new(x, y)),
            None => self.world.meeting_point(a, b),
        };
        if let Some(m) = meet {
            self.offers.get_mut(&id).unwrap().meet = Some((m.x, m.y));
            self.world.set_rendezvous(a, b, m);
        }
    }

    /// Whether `id` is already in an accepted agreement other than `except`
    /// (an animal walks to one meeting at a time).
    fn in_agreement(&self, id: OrgId, except: u64) -> bool {
        self.offers
            .values()
            .any(|o| o.id != except && o.status == OfferStatus::Accepted && (o.mine == id || o.partner == id))
    }

    /// Where an accepted agreement's two animals are, coarsely (8-tile
    /// cells), for both sides' escort: they walk to meet on their own
    /// (`World::set_rendezvous`), and a player may possess theirs to hurry.
    fn meet_view(&self, o: &Offer) -> Option<MeetView> {
        let (a, b) = (self.world.organism(o.mine)?, self.world.organism(o.partner)?);
        let coarse = |v: f32| (v / 8.0).floor() * 8.0 + 4.0;
        let asleep = |x: &Organism| matches!(x.dormancy, Dormancy::Winter | Dormancy::Summer);
        Some(MeetView {
            mine_x: coarse(a.pos.x),
            mine_y: coarse(a.pos.y),
            partner_x: coarse(b.pos.x),
            partner_y: coarse(b.pos.y),
            distance: a.pos.dist(b.pos).round(),
            meet_x: o.meet.map(|m| coarse(m.0)),
            meet_y: o.meet.map(|m| coarse(m.1)),
            asleep: asleep(a) || asleep(b),
            possessed: a.possessed_by.is_some() || b.possessed_by.is_some(),
        })
    }

    /// The tick an open offer expires (pending: no answer) or lapses
    /// (accepted: the two never met).
    fn offer_deadline(&self, o: &Offer) -> u64 {
        if o.status == OfferStatus::Accepted {
            // At most `HOLD_MAX_DAYS` of simulated time, whatever the clock.
            let cap = HOLD_MAX_DAYS * self.world.config.ticks_per_day as u64;
            o.accepted_tick.unwrap_or(o.created_tick).saturating_add(self.cfg.purchase_deadline_ticks.min(cap))
        } else {
            o.created_tick.saturating_add(self.cfg.offer_ticks)
        }
    }

    /// Bookkeeping after `World::step`: complete agreements whose organisms
    /// have met, expire offers at a season change, record a new legacy day.
    pub fn after_tick(&mut self) -> Outbox {
        let mut out = std::mem::take(&mut self.pending_out);
        let open: Vec<u64> = self.offers.values().filter(|o| o.status.is_open()).map(|o| o.id).collect();
        let tick = self.world.tick();
        for id in open {
            let o = &self.offers[&id];
            let (accepted, mine, partner) = (o.status == OfferStatus::Accepted, o.mine, o.partner);
            // Either asleep (hibernating, brumating, estivating): the
            // meeting waits, and so does its deadline (for at most
            // `HOLD_MAX_DAYS`). The partner sleeping while the requester's
            // animal is awake excuses a lapse.
            let asleep = |id: OrgId| {
                self.world.organism(id).is_some_and(|x| matches!(x.dormancy, Dormancy::Winter | Dormancy::Summer))
            };
            let (sa, sb) = (asleep(mine), asleep(partner));
            let pause_cap = HOLD_MAX_DAYS * self.world.config.ticks_per_day as u64;
            if accepted && (sa || sb) {
                let o = self.offers.get_mut(&id).unwrap();
                if o.paused < pause_cap {
                    o.paused += 1;
                    o.accepted_tick = Some(o.accepted_tick.unwrap_or(o.created_tick).saturating_add(1));
                }
                if sb && !sa {
                    o.partner_slept = true;
                }
            }
            let o = &self.offers[&id];
            let deadline = self.offer_deadline(o);
            let due = tick >= deadline;
            let warn = !accepted && !o.warned && tick.saturating_add(self.cfg.offer_warn_ticks) >= deadline;
            // An offer to the lineage has no partner yet: only its sender's
            // organism must live (and the lineage).
            let open_lineage = !accepted && partner == 0;
            let partner_alive = if open_lineage {
                o.lineage.is_some_and(|l| self.world.organisms.iter().any(|x| x.alive && x.lineage == l))
            } else {
                self.world.organism(partner).is_some()
            };
            let (pa, pb) = (self.world.organism(mine), if open_lineage { None } else { self.world.organism(partner) });
            match (pa, pb.or(if partner_alive && open_lineage { pa } else { None })) {
                (Some(a), Some(b)) if accepted && a.pos.dist(b.pos) <= MEETING_DISTANCE => {
                    let _ = self.execute(id, &mut out);
                }
                (Some(_), Some(_)) if accepted && due => self.lapse(id, &mut out),
                (Some(_), Some(_)) if due => {
                    let minutes = self.cfg.offer_ticks as f64 * self.cfg.tick_ms / 60_000.0;
                    let info = ErrorInfo::new(
                        "expired",
                        format!("no answer within {minutes:.0} minutes: the request expired"),
                    );
                    self.close(id, OfferStatus::Expired, None, Some(info), &mut out);
                }
                (Some(_), Some(_)) if warn => self.warn_expiry(id, deadline - tick, &mut out),
                (Some(_), Some(_)) => {}
                // The requester's animal died on the way: they may send
                // another within the deadline (`substitute_mating`).
                (None, Some(_)) if accepted => self.buyer_lost(id, due, &mut out),
                _ if open_lineage && self.world.organism(mine).is_some() => {
                    let info = ErrorInfo::new("lineage_gone", "the lineage has no living animals left");
                    self.close(id, OfferStatus::Failed, None, Some(info), &mut out);
                }
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
        // Offers no longer expire at a weather-season change (they have
        // wall-clock lifetimes); the season is still tracked for alerts.
        self.last_season = self.world.season();
        self.watch_possessed(&mut out);
        self.announce_deaths(&mut out);
        self.tell_joiners(&mut out);
        self.release_the_absent();
        self.announce_attack_results(&mut out);
        self.announce_group_events(&mut out);
        self.plan_routes();
        self.start_idle_spells();
        self.offline_after_tick();
        self.announce_grace_end(&mut out);
        if self.world.tick().is_multiple_of(self.world.config.ticks_per_hour()) {
            self.sync_focus();
        }
        let legacy_day = self.world.latest_legacy().map(|l| l.day);
        if legacy_day != self.last_legacy_day {
            self.last_legacy_day = legacy_day;
            let record = self.daily_record();
            // Before a rollover: a catastrophe ending at the boundary counts
            // for the season it fell in, not the next one.
            self.credit_survival(record.day);
            self.season_day(&record, &mut out);
            let ramped = self.ramped(&record);
            self.era_scores.add(&ramped, self.world.config.era_days());
            self.refresh_hosts();
            self.pay_income(&record);
            self.pay_royalties(record.day);
            self.daily_alerts(&record, &mut out);
            self.daily_group_checks(&record, &mut out);
            self.chronicle_alerts(&mut out);
            self.history.push_back(record.clone());
            self.warning_alerts(record.day, &mut out);
            if self.history.len() > HISTORY_DAYS {
                self.history.pop_front();
            }
            crate::history::compact_window(&mut self.history);
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
        self.season.record_day(&self.ramped(record));
    }

    /// Credit the ranked season with survived catastrophes (playtest 5,
    /// M12), before the season can roll over: each survivor's credit from
    /// the sim's achievements (only a lineage founded before it began), and
    /// at a rollover, a catastrophe still under way that mostly fell in the
    /// ending season is credited there to the players alive through it so
    /// far (its later achievement doesn't credit the next season again).
    fn credit_survival(&mut self, record_day: u64) {
        let ch = &self.world.chronicle;
        let founded_before = |p: PlayerId, start: u64| self.founded_day.get(&p).is_none_or(|&f| f < start);
        let mut credits: Vec<(PlayerId, u32)> = Vec::new();
        for a in &ch.achievements[self.seen_achievements.min(ch.achievements.len())..] {
            if let AchievementKind::SurvivedCatastrophe { catastrophe } = a.kind {
                let start = ch.catastrophes.iter().find(|c| c.id == catastrophe).map_or(0, |c| c.start_day);
                if a.player != WILD && founded_before(a.player, start) {
                    credits.push((a.player, catastrophe));
                }
            }
        }
        if self.season.is_over_by(record_day) {
            if let Some(c) = ch.active_catastrophe() {
                let (s0, s1) = (self.season.first_day, self.season.end_day);
                let total = c.end_day.saturating_sub(c.start_day).max(1);
                let inside = c.end_day.min(s1).saturating_sub(c.start_day.max(s0));
                if 2 * inside > total {
                    let counts = self.world.population_counts();
                    for (&p, &l) in &self.lineage_of {
                        let alive = counts.get(l as usize).is_some_and(|x| x.alive > 0);
                        if alive && founded_before(p, c.start_day) {
                            credits.push((p, c.id));
                        }
                    }
                }
            }
        }
        for (p, id) in credits {
            if self.survival_credited.insert((p, id)) {
                self.season.add_survival(p, id);
            }
        }
    }

    /// `record` with each player's legacy shares scaled by their lineage's
    /// founding ramp ([`FOUNDING_RAMP_DAYS`]), as the ranked season counts
    /// them.
    fn ramped(&self, record: &DailyRecord) -> DailyRecord {
        let mut r = record.clone();
        for e in &mut r.legacy {
            let f = founding_ramp(self.founded_day.get(&e.player).copied(), record.day);
            e.share *= f;
            e.weighted_share *= f;
        }
        r
    }

    /// This world's id (daily records, ledger, seasons).
    pub fn world_id(&self) -> &str {
        &self.world_id
    }

    /// The ranked season under way: rows `offset..offset + limit` of each
    /// category, plus `player`'s row.
    pub fn current_season(&self, offset: usize, limit: usize, player: Option<PlayerId>) -> CurrentSeason {
        self.live_season().page(offset, limit, player)
    }

    /// What the ranked season's standings depend on: when it is unchanged,
    /// so is [`Self::live_season`] (`crate::standings`). Cheap.
    pub fn live_season_key(&self) -> league::LiveKey {
        let t = &self.season;
        let next = self.last_legacy_day.map_or(t.first_day, |d| d + 1).max(t.first_day);
        league::LiveKey {
            season: t.season,
            generation: t.generation(),
            days_left: t.end_day.saturating_sub(next),
            names: self.names_changed,
        }
    }

    /// The ranked season under way with its full standings (computed).
    pub fn live_season(&self) -> league::LiveSeason {
        let standings = self.season.standings(|p| self.name(p));
        league::LiveSeason::new(self.live_season_key(), &self.world_id, &self.season, standings)
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

    /// Tell the owner of `target` that `player` ordered `attacker` to attack
    /// it (`ClientMsg::PossessAttack`); nobody to tell for a wild one.
    fn alert_attacked(&mut self, player: PlayerId, attacker: OrgId, target: OrgId, out: &mut Outbox) {
        let w = &self.world;
        let (Some(a), Some(t)) = (w.organism(attacker), w.organism(target)) else { return };
        if t.owner == WILD || t.owner == player {
            return;
        }
        let kind = |l: u32| w.lineages.get(l as usize).map_or("animals", |x| x.name.as_str()).to_string();
        let (owner, theirs, mine) = (t.owner, kind(t.lineage), kind(a.lineage));
        // One alert per attacker lineage and victim in a while.
        let window = (self.cfg.attack_alert_minutes * 60_000.0 / self.cfg.tick_ms.max(1.0)).round() as u64;
        let now = w.tick();
        let key = (a.lineage, owner);
        if self.attack_alerts.get(&key).is_some_and(|&t| now.saturating_sub(t) < window) {
            return;
        }
        self.attack_alerts.retain(|_, t| now.saturating_sub(*t) < window);
        self.attack_alerts.insert(key, now);
        let text = format!("#{target} of your {theirs} is being hunted by a player's {mine} (#{attacker}).");
        self.alert(owner, AlertKind::Attack, text, Some(target), out);
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
        self.alert_on(player, kind, text, organism, self.world.day(), out);
    }

    /// An alert stamped with `day` (from 0) rather than today: a daily
    /// summary, taken as the next day starts, is about the day just ended,
    /// and the feed shows that day beside its "Day N" text.
    fn alert_on(
        &mut self,
        player: PlayerId,
        kind: AlertKind,
        text: String,
        organism: Option<OrgId>,
        day: u64,
        out: &mut Outbox,
    ) {
        let alert = AlertView { tick: self.world.tick(), day, kind, text, organism };
        let list = self.alerts.entry(player).or_default();
        list.push_back(alert.clone());
        if list.len() > MAX_ALERTS {
            // Routine alerts go first: a group order's outcome used to be
            // pushed out by daily summaries within days (playtest 5).
            let routine = list.iter().position(|a| a.kind == AlertKind::DailySummary);
            match routine {
                Some(k) => {
                    list.remove(k);
                }
                None => {
                    list.pop_front();
                }
            }
        }
        out.push((Target::Player(player), ServerMsg::Alert { alert }));
    }

    /// Recent alerts of a player, oldest first.
    pub fn alerts(&self, player: PlayerId) -> Vec<AlertView> {
        self.alerts.get(&player).map(|l| l.iter().cloned().collect()).unwrap_or_default()
    }

    /// Watch possessed organisms for their death, and tell the owner once a
    /// spell when one is parched or starving. The sim looks after it: a
    /// possessed organism breaks off its move order to drink, eat or flee
    /// when the need is severe (`Balance::possess_urgency`) and drops the
    /// order once it arrives.
    fn watch_possessed(&mut self, out: &mut Outbox) {
        let day = self.world.day();
        let urgency = self.world.config.balance.possess_urgency;
        let (tick, tpd) = (self.world.tick(), self.world.config.ticks_per_day as u64);
        let mut needy = Vec::new();
        let mut sleepers = Vec::new();
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner != WILD) {
            let Some(p) = o.possessed_by else { continue };
            self.watched.entry(o.id).or_insert((p, Watch::Possessed));
            // Bedded down with its kind (`Balance::possessed_sleep`): said
            // once per sleep.
            if o.dormancy.is_deep() {
                if self.sleep_alerted.insert(o.id) {
                    sleepers.push((p, o.id, o.dormancy));
                }
                continue;
            }
            self.sleep_alerted.remove(&o.id);
            // Once per spell of need (playtest 3: the same alert came on two
            // days running): again only after it has stayed well below for
            // a game day (playtest 4: hunger swings with each kill).
            let need = o.needs.thirst.max(o.needs.hunger);
            if need < NEED_ALERT - NEED_EASED {
                if self.need_alerted.contains_key(&o.id) {
                    let since = *self.need_eased.entry(o.id).or_insert(tick);
                    if tick >= since + tpd {
                        self.need_alerted.remove(&o.id);
                        self.need_eased.remove(&o.id);
                    }
                }
            } else if self.need_alerted.contains_key(&o.id) {
                self.need_eased.remove(&o.id);
            } else if need >= NEED_ALERT {
                // Already broken off to drink or eat (the sim's own rule:
                // urgency, or water near with `Balance::drink_priority`).
                let tending = matches!(o.action, evo_sim::organism::Action::Drink | evo_sim::organism::Action::Graze);
                needy.push((p, o.id, o.needs.thirst, o.needs.hunger, o.order.is_some(), tending));
            }
        }
        for (p, id, thirst, hunger, walking, tending) in needy {
            self.need_alerted.insert(id, day);
            let (what, act) = if thirst >= hunger {
                (format!("parched (thirst {thirst:.0})"), "drink")
            } else {
                (format!("starving (hunger {hunger:.0})"), "eat")
            };
            let tail = if walking && (tending || thirst.max(hunger) >= urgency) {
                format!("it has broken off your order to {act} and goes back to it afterwards")
            } else if walking {
                format!("it keeps to your order until the need reaches {urgency:.0}, then stops to {act}; walk it to water or food, or release it")
            } else {
                "it is foraging on its own until you give it an order".to_string()
            };
            self.alert(p, AlertKind::Need, format!("Possessed #{id} is {what}: {tail}."), Some(id), out);
        }
        for (p, id, how) in sleepers {
            let what = if how == evo_sim::organism::Dormancy::Summer {
                "to sleep out the summer heat"
            } else {
                "for the winter"
            };
            let text = format!(
                "Possessed #{id} has bedded down {what} with its kind: it sleeps where it is and takes no orders until it wakes. Release it or possess another."
            );
            self.alert(p, AlertKind::Need, text, Some(id), out);
        }
        let world = &self.world;
        self.sleep_alerted.retain(|id| world.organism(*id).is_some_and(|o| o.possessed_by.is_some()));
        self.need_eased.retain(|id, _| world.organism(*id).is_some_and(|o| o.alive));
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
        // Organisms in open agreements, on the market and offered as an
        // auction's highest bid stay agents: in a cohort they are gone to
        // the game, which would fail the agreement or close the listing as
        // if they had died.
        let offers = self.offers.values().filter(|o| o.status.is_open()).flat_map(|o| [o.mine, o.partner]);
        let listed = self.economy.listings.values().flat_map(|l| {
            std::iter::once(l.organism).chain(l.auction.as_ref().and_then(|a| a.high.as_ref()).map(|b| b.mine))
        });
        let traded: BTreeSet<OrgId> = offers.chain(listed).collect();
        for id in traded {
            if let Some(o) = self.world.organism(id).filter(|o| o.owner != WILD) {
                points.entry(o.owner).or_default().push(o.pos);
            }
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
    /// Whether `player` may look at point `p`: within the largest view
    /// radius of one of their living organisms, or of their lineage's center
    /// (the default view, which also finds a lineage living as cohorts).
    pub fn in_sight(&self, player: PlayerId, p: Vec2) -> bool {
        self.view_reach(player, p) >= 0.0
    }

    /// How far around `p` the player may look: the most, over their living
    /// organisms, of that organism's sight plus [`VIEW_MARGIN`] less its
    /// distance to `p` (along the farther axis), or [`VIEW_MARGIN`] less the
    /// distance from their lineage's center. Negative where they can't see.
    /// A view's window must fit in it, so a lineage spread over the map
    /// can't map most of it with a few wide views.
    pub fn view_reach(&self, player: PlayerId, p: Vec2) -> f32 {
        let cheb = |q: Vec2| (q.x - p.x).abs().max((q.y - p.y).abs());
        let mut best = f32::NEG_INFINITY;
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner == player) {
            best = best.max(self.sight(o) + VIEW_MARGIN - cheb(o.pos));
        }
        if self.lineage_of(player).is_some() {
            best = best.max(VIEW_MARGIN - cheb(self.lineage_center(player)));
        }
        best
    }

    /// How far an organism perceives, as the sim's `decide` reckons it
    /// unalarmed (`Balance::perception_range`, capped at `max_perception`).
    fn sight(&self, o: &evo_sim::organism::Organism) -> f32 {
        let c = &self.world.config;
        (2.0 + c.balance.perception_range * o.traits.stats.perception).min(c.max_perception)
    }

    /// The map image (`GET /api/map.png`) as `player` may see it: the
    /// terrain, their organisms, and others only within their sight
    /// ([`Game::in_sight`]), at `scale` pixels per tile.
    pub fn map_rgb(&self, player: PlayerId, scale: usize) -> (usize, usize, Vec<u8>) {
        let (w, h) = (self.world.map.width, self.world.map.height);
        // Tiles in sight: each own organism's square of view
        // ([`Game::view_reach`]).
        let mut seen = vec![false; w * h];
        let mut mark = |p: Vec2, r: f32| {
            let r = r.max(0.0).floor() as i64;
            let (cx, cy) = (p.x as i64, p.y as i64);
            for y in (cy - r).max(0)..=(cy + r).min(h as i64 - 1) {
                for x in (cx - r).max(0)..=(cx + r).min(w as i64 - 1) {
                    seen[y as usize * w + x as usize] = true;
                }
            }
        };
        // The widest sight per tile (organisms crowd a few tiles).
        let mut tiles: BTreeMap<(i64, i64), (Vec2, f32)> = BTreeMap::new();
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner == player) {
            let r = self.sight(o) + VIEW_MARGIN;
            let e = tiles.entry((o.pos.x as i64, o.pos.y as i64)).or_insert((o.pos, r));
            if r > e.1 {
                *e = (o.pos, r);
            }
        }
        for (p, r) in tiles.into_values() {
            mark(p, r);
        }
        if self.lineage_of(player).is_some() {
            mark(self.lineage_center(player), VIEW_MARGIN);
        }
        let visible = |o: &evo_sim::organism::Organism| {
            o.owner == player || {
                let (x, y) = ((o.pos.x as usize).min(w - 1), (o.pos.y as usize).min(h - 1));
                seen[y * w + x]
            }
        };
        evo_sim::render::render_rgb_visible(&self.world, scale, visible)
    }

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

    /// Deaths of founders and possessed organisms, told to their owner with
    /// the cause. Deaths are gathered per player and announced once a game
    /// hour: one death as "Founder #12 starved.", several as one alert
    /// ("5 founders died: 3 eaten, 2 starved (#12, #14, …).") rather than one
    /// alert each.
    fn announce_deaths(&mut self, out: &mut Outbox) {
        // Possessions that ended with the animal alive (released, another
        // possessed, retired) are forgotten; a dead one's waits for its alert.
        let w = &self.world;
        self.possessing.retain(|&p, id| w.organism(*id).is_none_or(|o| o.possessed_by == Some(p)));
        let dead: Vec<(OrgId, PlayerId, Watch)> = self
            .watched
            .iter()
            .filter(|(id, _)| self.world.organism(**id).is_none())
            .map(|(&id, &(p, w))| (id, p, w))
            .collect();
        for (id, player, watch) in dead {
            self.watched.remove(&id);
            self.need_alerted.remove(&id);
            let Some(o) = self.world.organisms.iter().find(|o| o.id == id) else {
                // Gone without a body: with level of detail on it went into a
                // statistical cohort far from every player, alive; not a death.
                if !self.world.config.lod.enabled {
                    self.pending_deaths.entry(player).or_default().push((id, watch, Why::Of(None)));
                }
                continue;
            };
            // Possessed when it died (not merely once: possession ends with the
            // player's sessions, and a released animal is an ordinary one).
            // The sim clears `possessed_by` as it kills: `possessing` remembers.
            if self.possessing.get(&player) == Some(&id) {
                self.possessing.remove(&player);
                // Said at once: the player was steering it (its bar just vanished).
                let text = format!(
                    "#{id}, the animal you were possessing, {}. Possess another from the map or the Lineage tab.",
                    death_words(why_died(o))
                );
                self.alert(player, AlertKind::Death, text, Some(id), out);
                continue;
            }
            self.pending_deaths.entry(player).or_default().push((id, watch, why_died(o)));
        }
        if !self.world.tick().is_multiple_of(self.world.config.ticks_per_hour()) || self.pending_deaths.is_empty() {
            return;
        }
        for (player, deaths) in std::mem::take(&mut self.pending_deaths) {
            let (text, organism) = death_alert_text(&deaths);
            self.alert(player, AlertKind::Death, text, organism, out);
        }
    }

    /// Lineage-wide warnings for `player` now (playtest 3, finding 19): a
    /// hunger or thirst crash in the last recorded day, the lineage near the
    /// hard cap, animals starving inside the territories they hold.
    /// Session A's alerts cover each territory running dry; these are the
    /// lineage's.
    pub fn lineage_warnings(&self, player: PlayerId) -> Vec<LineageWarning> {
        let Some(lineage) = self.lineage_of(player) else { return Vec::new() };
        let mut out = Vec::new();
        let w = &self.world;
        if let Some((r, e)) =
            self.history.back().and_then(|r| r.lineages.iter().find(|e| e.lineage == lineage).map(|e| (r, e)))
        {
            let n = |c: &str| e.deaths.get(c).copied().unwrap_or(0);
            let (starved, thirst) = (n("starved"), n("thirst"));
            let died = starved + thirst;
            let base = e.alive as u64 + e.deaths.values().sum::<u64>();
            if died >= STARVING_MIN && base > 0 && died as f64 >= STARVING_SHARE * base as f64 {
                let litter = self.crowding_view(player).map_or(1.0, |c| c.litter);
                let why = if litter >= FULL_LITTER {
                    "Litters are still full size, so the population cap isn't the cause: their ground can't feed them."
                        .to_string()
                } else {
                    format!(
                        "The soft cap has litters at ×{litter:.2} of full size, yet the lineage is still more than its \
                         ground can feed."
                    )
                };
                out.push(LineageWarning {
                    kind: "starving".into(),
                    text: format!(
                        "Your lineage is going hungry: on day {} {starved} starved and {thirst} died of thirst ({:.0}% of it). \
                         {why} Move groups to better ground (the Food layer shows it), split big herds, keep them near water.",
                        r.day.max(1),
                        100.0 * died as f64 / base as f64
                    ),
                });
            }
        }
        let alive = w.population_counts().get(lineage as usize).map_or(0, |c| c.alive);
        let hard = w.player_ceiling();
        if hard > 0 && alive as f64 >= NEAR_CAP_SHARE * hard as f64 {
            out.push(LineageWarning {
                kind: "near_cap".into(),
                text: format!(
                    "Near the hard cap: {alive} of {hard} living organisms (young count too). At {hard} no more young are born; \
                     a bigger herd no longer adds to your share, healthy breeding adults do."
                ),
            });
        }
        // Animals inside the territories their groups hold, and how many of them are starving.
        let (mut inside, mut starving) = (0usize, 0usize);
        let mut groups: BTreeSet<u8> = BTreeSet::new();
        for o in w.organisms.iter().filter(|o| o.alive && o.owner == player) {
            if w.territory(player, o.group).is_some_and(|t| t.contains(o.pos)) {
                inside += 1;
                if o.needs.hunger >= TERRITORY_NEED || o.needs.thirst >= TERRITORY_NEED {
                    starving += 1;
                    groups.insert(o.group);
                }
            }
        }
        if starving >= STARVING_MIN as usize && starving as f64 >= TERRITORY_STARVING_SHARE * inside as f64 {
            let names: Vec<String> = groups.iter().map(|&g| group_name(g)).collect();
            out.push(LineageWarning {
                kind: "territory_starving".into(),
                text: format!(
                    "{starving} of the {inside} animals holding territories ({}) are starving or parched: they obey the claim \
                     and stay inside while the ground runs out. Move them, or abandon the claim.",
                    names.join(", ")
                ),
            });
        }
        out
    }

    /// Say each lineage warning as an alert when it starts, and again every
    /// [`WARNING_REPEAT_DAYS`] while it lasts.
    fn warning_alerts(&mut self, day: u64, out: &mut Outbox) {
        let players: Vec<PlayerId> = self.lineage_of.keys().copied().collect();
        for p in players {
            let now = self.lineage_warnings(p);
            // Near the cap is said once per approach: forgotten only below the re-arm share.
            let near_cap_armed = self.lineage_of(p).is_some_and(|l| {
                let alive = self.world.population_counts().get(l as usize).map_or(0, |c| c.alive);
                alive as f64 >= NEAR_CAP_REARM * self.world.player_ceiling() as f64
            });
            self.warned
                .retain(|(q, k), _| *q != p || now.iter().any(|w| &w.kind == k) || (k == "near_cap" && near_cap_armed));
            for w in now {
                let key = (p, w.kind.clone());
                let repeat = w.kind != "near_cap";
                if self.warned.get(&key).is_some_and(|&d| !repeat || day < d + WARNING_REPEAT_DAYS) {
                    continue;
                }
                self.warned.insert(key, day);
                self.alert(p, AlertKind::Need, w.text, None, out);
            }
        }
    }

    /// End the possession of players gone longer than the grace
    /// ([`POSSESSION_GRACE_MINUTES`]): the animal returns to its instincts.
    fn release_the_absent(&mut self) {
        if self.left.is_empty() {
            return;
        }
        let grace = minutes_to_ticks(POSSESSION_GRACE_MINUTES, self.cfg.tick_ms);
        let now = self.world.tick();
        let gone: Vec<PlayerId> =
            self.left.iter().filter(|&(_, &t)| now.saturating_sub(t) >= grace).map(|(&p, _)| p).collect();
        for p in gone {
            self.left.remove(&p);
            let held: Vec<OrgId> =
                self.world.organisms.iter().filter(|o| o.alive && o.possessed_by == Some(p)).map(|o| o.id).collect();
            for id in held {
                let _ = self.world.release(p, id);
            }
        }
    }

    /// Whether `player`'s new lineage `lineage` has a newcomer grace, and why
    /// (playtest 5: a re-founding inherited an expired grace unexplained).
    /// The rule: a grace runs from the owner's first founding in an unbroken
    /// run of foundings each soon after the last, so founding again soon
    /// continues the old grace (even one already over) rather than starting
    /// a new one. None on worlds without a grace.
    fn grace_at_founding(&self, player: PlayerId, lineage: u32) -> Option<String> {
        let w = &self.world;
        if w.config.balance.attack_grace <= 0.0 {
            return None;
        }
        let tpd = w.config.ticks_per_day.max(1) as f64;
        let founded = w.tick();
        let start = w.lineage_grace_start(lineage)?;
        let soon = (2.0 * w.grace_length() as f64 / tpd).round();
        let days = |t: u64| t as f64 / tpd;
        let real = |t: u64| {
            let min = t as f64 * self.cfg.tick_ms / 60_000.0;
            if min < 1.5 {
                "about a minute".to_string()
            } else {
                format!("about {min:.0} minutes")
            }
        };
        let text = match w.lineage_grace_ticks(lineage) {
            Some(left) if start >= founded => format!(
                "Newcomer grace: {:.0} days ({} of real time). Other players can't order attacks on your animals, \
                 nor yours on theirs, until it ends.",
                days(left),
                real(left)
            ),
            Some(left) => format!(
                "Newcomer grace: {:.1} days left ({}). It continues the grace of your earlier lineage (from day {}): \
                 founding again within {soon:.0} days of your last founding carries a grace on instead of starting a new one.",
                days(left),
                real(left),
                start / w.config.ticks_per_day.max(1) as u64 + 1
            ),
            None if start == 0 => return None,
            None if w.grace_waived().get(&player).is_some_and(|&t| t >= start) => {
                "No newcomer grace: you ended it, and founding again soon doesn't bring it back. Other players' animals \
                 can attack yours from now on."
                    .to_string()
            }
            None => format!(
                "No newcomer grace: it continues the grace of your earlier lineage (from day {}), which has run out. \
                 Founding again within {soon:.0} days of your last founding carries a grace on rather than starting \
                 a fresh one; other players' animals can attack yours now.",
                start / w.config.ticks_per_day.max(1) as u64 + 1
            ),
        };
        Some(text)
    }

    /// Tell players who just founded about a drought or catastrophe under
    /// way, whose warnings went out before they joined.
    fn tell_joiners(&mut self, out: &mut Outbox) {
        if self.joined.is_empty() {
            return;
        }
        let drought = self.drought_warned.then(|| drought_text(self.world.map.drought));
        let catastrophe =
            self.world.chronicle.active_catastrophe().map(|c| {
                format!("Catastrophe under way: {} (through about day {}).", catastrophe_text(c.kind), c.end_day)
            });
        for (p, text) in std::mem::take(&mut self.founding_notes) {
            self.alert(p, AlertKind::Attack, text, None, out);
        }
        for p in std::mem::take(&mut self.joined) {
            for text in drought.iter().chain(catastrophe.iter()) {
                self.alert(p, AlertKind::World, text.clone(), None, out);
            }
        }
    }

    /// Tell players how their attack orders ended: the target killed, dead
    /// of something else, gone to ground or out of sight, or the attacker
    /// dead. Orders the player replaced or released end silently.
    fn announce_attack_results(&mut self, out: &mut Outbox) {
        if self.attack_watch.is_empty() {
            return;
        }
        let ended: Vec<(OrgId, PlayerId, OrgId)> = self
            .attack_watch
            .iter()
            .filter(|&(&a, &(_, t))| self.world.attack_target(a) != Some(t))
            .map(|(&a, &(p, t))| (a, p, t))
            .collect();
        for (a, p, t) in ended {
            self.attack_watch.remove(&a);
            if let Some(text) = attack_result_text(&self.world, a, t) {
                self.alert(p, AlertKind::Attack, text, Some(a), out);
            }
        }
    }

    /// Tell players about group orders that ended on their own: a
    /// migration that arrived or timed out, a group left with no members.
    fn announce_group_events(&mut self, out: &mut Outbox) {
        use evo_sim::world::commands::GroupEvent;
        for ev in self.world.take_group_events() {
            match ev {
                GroupEvent::Arrived { player, group, target } => {
                    self.routes.remove(&(player, group));
                    self.drift_alerted.remove(&(player, group));
                    let text = format!(
                        "{} reached its migration target ({:.0}, {:.0}); the order is done, and the group keeps it as \
                         its home ground until its next order (claim a territory to hold it firmly).",
                        group_name(group),
                        target.x,
                        target.y
                    );
                    self.outcomes.insert((player, group), format!("reached ({:.0}, {:.0})", target.x, target.y));
                    self.alert(player, AlertKind::Group, text, None, out);
                }
                GroupEvent::ClaimedOnArrival { player, group, territory: t } => {
                    self.routes.remove(&(player, group));
                    self.drift_alerted.remove(&(player, group));
                    let text = format!(
                        "{} arrived and now holds its territory of radius {:.1} at ({:.0}, {:.0}).",
                        group_name(group),
                        t.radius,
                        t.center.x,
                        t.center.y
                    );
                    self.outcomes
                        .insert((player, group), format!("arrived and claimed ({:.0}, {:.0})", t.center.x, t.center.y));
                    self.alert(player, AlertKind::Group, text, None, out);
                }
                GroupEvent::Emptied { player, group } => {
                    self.routes.remove(&(player, group));
                    self.outcomes.remove(&(player, group));
                    if group != 0 {
                        let text =
                            format!("{} has no members left; its orders and territory are gone.", group_name(group));
                        self.alert(player, AlertKind::Group, text, None, out);
                    }
                }
                GroupEvent::TimedOut {
                    player,
                    group,
                    target,
                    median,
                    dormant_share,
                    basking_share,
                    needy_share,
                    never_moved,
                    pending_claim,
                } => {
                    self.routes.remove(&(player, group));
                    let (why, short) = timeout_reason(dormant_share, basking_share, needy_share, never_moved);
                    let claim = if pending_claim { " The territory it was to claim there is dropped too." } else { "" };
                    let text = format!(
                        "{} didn't gather at its migration target ({:.0}, {:.0}) (half the group is still more than \
                         {:.0} tiles away); the order has ended: {why}.{claim}",
                        group_name(group),
                        target.x,
                        target.y,
                        median
                    );
                    self.outcomes.insert(
                        (player, group),
                        format!("migration to ({:.0}, {:.0}) ended: {short}", target.x, target.y),
                    );
                    self.alert(player, AlertKind::Group, text, None, out);
                }
            }
        }
    }

    /// Add what a territory or rally circle offers to its command's reply
    /// (`water_share`, `drinkable_tiles`, `food_supports`, `distance` from
    /// the group), with `warning`s: little to drink, much of it water, its
    /// centre in water, too little food for the group, or far from it.
    fn circle_report(
        &self,
        player: PlayerId,
        group: u8,
        center: Vec2,
        radius: f32,
        data: &mut serde_json::Value,
    ) -> Vec<String> {
        let mut warnings = Vec::new();
        let w = self.world.water_check(player, group, center, radius);
        data["water_share"] = ((w.water_share * 100.0).round() / 100.0).into();
        data["drinkable_tiles"] = w.drinkable.into();
        // In a drought what water there is dries up: ask for more of it, in
        // proportion to the group (playtest 4: 12 tiles passed in a 0.83
        // drought for hundreds of animals).
        let members = self.world.groups(player).iter().find(|g| g.id == group).map_or(0, |g| g.members);
        let dryness = self.world.map.dryness_at(center);
        data["dryness"] = ((dryness * 100.0).round() / 100.0).into();
        let wanted = if dryness > 0.5 { (members / 8).max(LOW_WATER_TILES) } else { LOW_WATER_TILES };
        if w.drinkable < wanted && w.drinkable >= LOW_WATER_TILES {
            warnings.push(format!(
                "Only {} tiles of drinking water for {members} animals in a drought ({:.0}% dry here): it may not last.",
                w.drinkable,
                dryness * 100.0
            ));
        } else if w.drinkable < LOW_WATER_TILES {
            warnings.push(
                "Little or no drinking water in or near this circle: the group will leave it to drink, and may die \
                 of thirst in a drought."
                    .to_string(),
            );
        } else if w.water_share > 0.25 {
            warnings.push(format!("{:.0}% of this circle is water the group can't stand in.", w.water_share * 100.0));
        }
        if self.world.water_check(player, group, center, 0.5).water_share > 0.5 {
            warnings.push("Its centre is in water.".to_string());
        }
        if let Some(f) = self.world.food_check(player, group, center, radius) {
            data["food_supports"] = f.supports.into();
            data["standing_days"] = ((f.standing_days * 10.0).round() / 10.0).into();
            if f.supports < f.members {
                warnings.push(format!(
                    "At this season's growth the plants here feed about {} of the group's {} animals; expect hunger.",
                    f.supports, f.members
                ));
            }
            if f.standing_days < STANDING_WARN_DAYS {
                warnings.push(format!(
                    "The plants standing here would feed the whole group for {}.",
                    days_text(f.standing_days)
                ));
            }
        }
        if let Some(p) = self.world.prey_check(player, group, center, radius) {
            data["prey_nearby"] = p.prey.into();
            if p.prey * 2 < p.members {
                warnings
                    .push(format!("Only {} prey animals in or near this circle for {} hunters.", p.prey, p.members));
            }
        }
        if let Some(m) = self.world.group_middle(player, group) {
            let d = m.dist(center);
            data["distance"] = ((d * 10.0).round() / 10.0).into();
            if d > FAR_CIRCLE_TILES {
                warnings.push(format!("It is {d:.0} tiles from the group, which has to walk there first."));
            }
        }
        warnings
    }

    /// Warnings for a group about to travel: much of it dormant, and (with
    /// `route`) its planned way over open water. Shared by migrate, far
    /// claims and rallies.
    fn travel_warnings(&self, player: PlayerId, group: u8, route: bool) -> Vec<String> {
        let mut warnings = Vec::new();
        if route {
            if let Some((_, way)) = self.routes.get(&(player, group)) {
                let way: Vec<Vec2> = way.iter().map(|p| Vec2::new(p[0], p[1])).collect();
                let wet = self.world.route_water_tiles(player, group, &way);
                if wet >= ROUTE_WATER_TILES {
                    warnings.push(format!(
                        "The route crosses about {wet} tiles of open water: weak swimmers will be held up or drown."
                    ));
                }
            }
        }
        let dormant = self.world.dormant_share(player, group);
        if dormant >= DORMANT_WARN_SHARE {
            warnings.push(format!(
                "{:.0}% of the group is dormant (hibernating or estivating) and won't walk until it wakes; the \
                 order may run out before then.",
                dormant * 100.0
            ));
        }
        warnings
    }

    /// A group's standing orders, by name: what a new order replaces.
    fn orders_of(&self, player: PlayerId, group: u8) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.world.migration_order(player, group).is_some() {
            v.push("migration");
        }
        if self.world.territory(player, group).is_some() {
            v.push("territory");
        }
        if self.world.rally(player, group).is_some() {
            v.push("rally");
        }
        v
    }

    /// A new order for a group: the last order's outcome is no longer news.
    fn order_given(&mut self, player: PlayerId, group: u8) {
        self.outcomes.remove(&(player, group));
    }

    /// `claim_cooldown`, the wait said in minutes of real time and game days.
    fn cooldown_error(&self, ticks_left: u64) -> ErrorInfo {
        let minutes = ticks_left as f64 * self.cfg.tick_ms / 60_000.0;
        let days = ticks_left as f64 / self.world.config.ticks_per_day.max(1) as f64;
        let wait = if minutes < 1.0 {
            "under a minute".to_string()
        } else if minutes < 10.0 {
            format!("about {:.1} min", minutes)
        } else {
            format!("about {:.0} min", minutes)
        };
        ErrorInfo::new(
            "claim_cooldown",
            format!(
                "This group (or the group it was split from) claimed a territory recently; it can claim again in \
                 {wait} ({days:.1} game days). Abandoning a territory cuts the wait to a quarter, not to nothing."
            ),
        )
        .with_detail(serde_json::json!({ "ticks_left": ticks_left, "minutes_left": minutes, "days_left": days }))
    }

    /// Once a day: tell owners when a territory has gone dry or been grazed
    /// bare (with the day's thirst and hunger deaths), and when a group has
    /// drifted from the home its last migration gave it. Each is said once,
    /// and again only after it has recovered.
    fn daily_group_checks(&mut self, record: &DailyRecord, out: &mut Outbox) {
        let players: Vec<(PlayerId, u32)> = self.lineage_of.iter().map(|(&p, &l)| (p, l)).collect();
        for (p, lineage) in players {
            let deaths = record.lineages.iter().find(|e| e.lineage == lineage).map(|e| e.deaths.clone());
            let died = |cause: &str| deaths.as_ref().and_then(|d| d.get(cause)).copied().unwrap_or(0);
            let (thirst, starved) = (died("thirst"), died("starved"));
            let groups = self.world.groups(p);
            for g in &groups {
                let key = (p, g.id);
                let name = group_name(g.id);
                let mut texts = Vec::new();
                // The ground the group holds: its territory, else the home its
                // last migration gave it while no other order takes it away.
                let held = match (g.territory, g.settle) {
                    (Some(t), _) => Some((t.center, t.radius, "territory")),
                    (None, Some(h)) if g.rally.is_none() && g.migration.is_none() => {
                        Some((h.center, h.radius, "home ground"))
                    }
                    _ => None,
                };
                match held {
                    Some((center, radius, what)) if g.members > 0 => {
                        let (x, y) = (center.x, center.y);
                        let fix = if what == "territory" {
                            "abandon it or move the group"
                        } else {
                            "migrate the group to better ground"
                        };
                        let dry = self.world.water_check(p, g.id, center, radius).drinkable == 0;
                        if self.condition_begins((p, g.id, 0), dry) {
                            let tail = if thirst > 0 {
                                format!(" {thirst} of your animals died of thirst yesterday.")
                            } else {
                                String::new()
                            };
                            texts.push(format!(
                                "{name}'s {what} at ({x:.0}, {y:.0}) has no drinking water left: {fix}.{tail}"
                            ));
                        }
                        let food = self.world.food_check(p, g.id, center, radius);
                        let bare = food.is_some_and(|f| f.standing_days < BARE_DAYS);
                        if self.condition_begins((p, g.id, 1), bare) {
                            let days = days_text(food.map_or(0.0, |f| f.standing_days));
                            let tail = if starved > 0 {
                                format!(" {starved} of your animals starved yesterday.")
                            } else {
                                String::new()
                            };
                            texts.push(format!(
                                "{name}'s {what} at ({x:.0}, {y:.0}) is grazed bare (food for {days}): {fix}.{tail}"
                            ));
                        }
                        // Outgrown: the herd has grown past what the ground feeds,
                        // before anyone starves (playtest 5: claimed at 475, held
                        // 1,941, 55% starved in 10 days).
                        let outgrown = !bare && food.is_some_and(|f| f.supports < f.members);
                        if self.condition_begins((p, g.id, 2), outgrown) {
                            let f = food.expect("checked");
                            texts.push(format!(
                                "{name} has outgrown its {what} at ({x:.0}, {y:.0}): it feeds about {} of the group's {} \
                                 animals now. Split the group, move it, or claim more ground before they go hungry.",
                                f.supports, f.members
                            ));
                        }
                        let prey = self.world.prey_check(p, g.id, center, radius);
                        let short = prey.is_some_and(|q| q.prey * 2 < q.members);
                        if self.condition_begins((p, g.id, 3), short) {
                            let q = prey.expect("checked");
                            texts.push(format!(
                                "Only {} prey animals in or near {name}'s {what} at ({x:.0}, {y:.0}) for {} hunters: \
                                 move the pack to where prey is.",
                                q.prey, q.members
                            ));
                        }
                    }
                    _ => {
                        for kind in 0..4 {
                            self.territory_alerted.remove(&(p, g.id, kind));
                        }
                    }
                }
                // Drifted from home, only while nothing else holds the group
                // elsewhere (a rally or territory there is the player's doing).
                let drifted = g
                    .settle
                    .filter(|_| g.territory.is_none() && g.rally.is_none() && g.migration.is_none())
                    .and_then(|s| {
                        let m = self.world.group_median_dist(p, g.id, s.center)?;
                        (m > DRIFT_FACTOR * s.radius).then_some((s, m))
                    });
                match drifted {
                    Some((s, m)) => {
                        if self.drift_alerted.insert(key) {
                            texts.push(format!(
                                "{name} has drifted from its home at ({:.0}, {:.0}): half the group is more than {m:.0} \
                                 tiles away. Migrate or rally it back, or claim a territory to hold it.",
                                s.center.x, s.center.y
                            ));
                        }
                    }
                    None => {
                        self.drift_alerted.remove(&key);
                    }
                }
                for text in texts {
                    self.alert(p, AlertKind::Group, text, None, out);
                }
            }
            let live: BTreeSet<u8> = groups.iter().map(|g| g.id).collect();
            self.territory_alerted.retain(|&(q, g, _)| q != p || live.contains(&g));
            self.drift_alerted.retain(|&(q, g)| q != p || live.contains(&g));
        }
    }

    /// Track a daily territory condition: true when it has just begun (say
    /// it now); a condition that has passed is forgotten.
    fn condition_begins(&mut self, key: (PlayerId, u8, u8), bad: bool) -> bool {
        if bad {
            self.territory_alerted.insert(key)
        } else {
            self.territory_alerted.remove(&key);
            false
        }
    }

    /// What a group order does to the group's clutches (playtest 5: orders
    /// pulled brooding parents off their nests with no word, 20–66 eggs
    /// chilled a day). `brooding`: members sitting on a clutch. With
    /// `Balance::brood_care` they stay on their nests until the eggs hatch
    /// and then follow; without it the clutches are left to chill.
    fn brood_report(&self, player: PlayerId, group: u8, data: &mut serde_json::Value) {
        let brooding = self
            .world
            .organisms
            .iter()
            .filter(|o| o.alive && o.owner == player && o.group == group && o.nest.is_some())
            .count();
        if brooding == 0 {
            return;
        }
        data["brooding"] = brooding.into();
        let s = if brooding == 1 { "" } else { "s" };
        data["brooding_note"] = if self.world.config.balance.brood_care > 0.0 {
            format!(
                "{brooding} brooding parent{s} stay{} on {} nest{s} until the eggs hatch, then follow.",
                if brooding == 1 { "s" } else { "" },
                if brooding == 1 { "its" } else { "their" }
            )
        } else {
            format!("{brooding} clutch{} will be left behind and may chill.", if brooding == 1 { "" } else { "es" })
        }
        .into();
    }

    /// Plan the route of one group's migration order for clients.
    fn plan_route(&mut self, player: PlayerId, group: u8) {
        let Some(target) = self.world.migration_order(player, group) else {
            self.routes.remove(&(player, group));
            return;
        };
        let route = self.world.migration_route(player, group).iter().map(|p| [p.x, p.y]).collect();
        self.routes.insert((player, group), (target, route));
    }

    /// Keep the drawn routes in step with the orders: drop those whose order
    /// ended or changed, plan a few missing ones (after a restart) per tick.
    fn plan_routes(&mut self) {
        let world = &self.world;
        self.routes.retain(|&(p, g), (t, _)| world.migration_order(p, g) == Some(*t));
        let missing: Vec<(PlayerId, u8)> = (self.lineage_of.keys())
            .flat_map(|&p| (0..MAX_GROUPS).map(move |g| (p, g)))
            .filter(|&(p, g)| world.migration_order(p, g).is_some() && !self.routes.contains_key(&(p, g)))
            .take(2)
            .collect();
        for (p, g) in missing {
            self.plan_route(p, g);
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
        let mut ended_now: Vec<PlayerId> = Vec::new();
        for (p, lineage) in players {
            let Some(entry) = record.lineages.iter().find(|e| e.lineage == lineage) else { continue };
            let died: u64 = entry.deaths.values().sum();
            let eggs_lost: u64 = entry.eggs_lost.values().sum();
            if entry.born > 0 || died > 0 || eggs_lost > 0 {
                let mut causes: Vec<(&String, &u64)> = entry.deaths.iter().collect();
                causes.sort_by(|a, b| b.1.cmp(a.1));
                // The day's counts come from the sim's causes ("exposure" only
                // in records from before cold and heat were told apart).
                let word = |c: &str| {
                    if c == "exposure" {
                        "cold or heat".to_string()
                    } else {
                        c.replace('_', " ")
                    }
                };
                let causes: Vec<String> = causes.iter().map(|(c, n)| format!("{n} {}", word(c))).collect();
                let eggs = if eggs_lost > 0 {
                    // Most living members abandon their eggs: say so.
                    let (abandon, layers) = self
                        .world
                        .organisms
                        .iter()
                        .filter(|o| o.alive && o.lineage == lineage && o.traits.birth.lays_eggs())
                        .fold((0usize, 0usize), |(a, n), o| {
                            (a + usize::from(o.traits.birth == evo_sim::design::BirthType::AbandonedEggs), n + 1)
                        });
                    let abandoned = layers > 0 && abandon * 2 > layers;
                    let why: Vec<String> =
                        entry.eggs_lost.iter().map(|(c, n)| format!("{n} {}", egg_loss_why(c, abandoned))).collect();
                    format!(" · {eggs_lost} egg{} lost ({})", if eggs_lost == 1 { "" } else { "s" }, why.join(", "))
                } else {
                    String::new()
                };
                let text = format!(
                    "Day {}: {} born, {} died{}{eggs} · {} alive",
                    // The day the births and deaths happened (the one just
                    // ended), counted from 1, as the vitals chart labels it.
                    record.day.max(1),
                    entry.born,
                    died,
                    if causes.is_empty() { String::new() } else { format!(" ({})", causes.join(", ")) },
                    entry.alive
                );
                // Stamped with the day it is about (the feed shows "day N" beside "Day N: …").
                self.alert_on(p, AlertKind::DailySummary, text, None, record.day.saturating_sub(1), out);
            }
            let alive = entry.alive;
            let share = record.legacy.iter().find(|e| e.player == p).map_or(0.0, |e| e.share);
            let generation = max_gen.get(&lineage).copied().unwrap_or(0);
            // The first record after a founding sets the baseline: the founders
            // alone are no milestone (playtest 2: "reached 50" on founding day).
            let fresh = self.founded_day.get(&p).is_some_and(|&f| record.day <= f + 1);
            let prog = self.progress.entry(p).or_default();
            if fresh {
                prog.population = prog.population.max(alive);
                prog.share = prog.share.max(share);
            }
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
                // A re-founding soon after costs founders (`refound_terms`).
                ended_now.push(p);
            }
            for (kind, text) in texts {
                self.alert(p, kind, text, None, out);
            }
        }
        for p in ended_now {
            self.economy.retired_day.insert(p, record.day);
            self.economy.died_out.insert(p);
        }
    }

    /// Announce what the chronicle recorded since the last day: catastrophes
    /// beginning and ending (to every player with a lineage), achievements
    /// (to their player) and finished eras (to every player, with their rank).
    fn chronicle_alerts(&mut self, out: &mut Outbox) {
        let players: Vec<PlayerId> = self.lineage_of.keys().copied().collect();
        let dpy = self.world.config.days_per_year.max(1) as u64;
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
                        // The same days the World tab lists (counted from 1).
                        format!("Catastrophe: {what} has begun (days {}–{}).", c.start_day + 1, c.end_day),
                    ));
                }
            }
            if c.ended && !seen.1 {
                seen.1 = true;
                let change = population_change_text(c.population_before, c.population_after.unwrap_or(0));
                for &p in &players {
                    alerts.push((
                        p,
                        AlertKind::World,
                        None,
                        format!("{} is over: {change}.", catastrophe_name(c.kind)),
                    ));
                }
            }
        }
        // Droughts: a warning when much of the land dries out, and when it eases.
        let drought = self.world.map.drought;
        // A megadrought has its own catastrophe alert: no second one for it
        // (a drought still on after it ends is warned of then).
        let megadrought = ch.active_catastrophe().is_some_and(|c| c.kind == CatastropheKind::Megadrought);
        let warn = if megadrought {
            None
        } else if !self.drought_warned && drought >= DROUGHT_WARN {
            self.drought_warned = true;
            Some((Some(PushKind::Catastrophe), drought_text(drought)))
        } else if self.drought_warned && drought < DROUGHT_CLEAR {
            self.drought_warned = false;
            Some((None, format!("The drought has eased: {:.0}% of the land is dry now.", drought * 100.0)))
        } else {
            None
        };
        if let Some((push, text)) = warn {
            for &p in &players {
                alerts.push((p, AlertKind::World, push, text.clone()));
            }
        }
        let mut bonuses: Vec<(PlayerId, u64, String)> = Vec::new();
        for a in &ch.achievements[self.seen_achievements.min(ch.achievements.len())..] {
            let text = achievement_text(&self.world, &a.kind);
            let mut push = None;
            let mut paid = true;
            match a.kind {
                AchievementKind::SpeciesFounded { species } => {
                    if species_redetected(&ch.species, species, a.player) {
                        // No speciation: no bonus, count or alert.
                        continue;
                    }
                    paid = species_bonus_due(&ch.species, &mut self.species_bonus, species, a.player, a.day, dpy);
                    if paid {
                        self.season.add_species(a.player);
                    }
                    push = Some(PushKind::Species);
                }
                AchievementKind::SurvivedCatastrophe { catastrophe } => {
                    // Only a lineage founded before the catastrophe began
                    // survived it: re-founding during one used to earn the
                    // bonus and the season's survival count.
                    let start = ch.catastrophes.iter().find(|c| c.id == catastrophe).map(|c| c.start_day);
                    let founded = self.founded_day.get(&a.player).copied();
                    paid = match (start, founded) {
                        (Some(s), Some(f)) => f < s,
                        _ => true,
                    };
                    // The season's credit was given before the rollover (`credit_survival`).
                }
                _ => {}
            }
            let bonus = economy::achievement_bonus(a.kind.label());
            if bonus > 0 && a.player != WILD && paid {
                bonuses.push((a.player, bonus, text.clone()));
            }
            alerts.push((a.player, AlertKind::Achievement, push, text));
        }
        self.seen_achievements = ch.achievements.len();
        let closed: Vec<(u64, Vec<EraRowData>)> = ch.eras[self.seen_eras.min(ch.eras.len())..]
            .iter()
            .map(|e| (e.era, e.ranking.iter().map(|r| (r.player, r.share, r.weighted_share)).collect()))
            .collect();
        for (era, ranking) in closed {
            let means = self.era_scores.close(era).cloned();
            let order = era_order(&ranking, means.as_ref());
            for &p in &players {
                let rank = order.iter().filter(|r| r.0 != WILD).position(|r| r.0 == p);
                let last = ranking.iter().find(|x| x.0 == p).map_or(0.0, |x| x.1 * 100.0);
                let text = match (rank, &means) {
                    (Some(r), Some(m)) => format!(
                        "Era {} has ended: you finished #{} with a mean {:.1}% of breeding alleles over the era ({last:.1}% at its end).",
                        era + 1,
                        r + 1,
                        m.get(&p).copied().unwrap_or(0.0) * 100.0
                    ),
                    (Some(r), None) => format!(
                        "Era {} has ended: you finished #{} with {last:.1}% of breeding alleles.",
                        era + 1,
                        r + 1
                    ),
                    (None, _) => format!("Era {} has ended; your alleles were not among the breeding adults.", era + 1),
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
        let season_days = (w.config.days_per_year / 4).max(1) as u64;
        let coarse = |dominant: PlayerId, v: f32| if dominant == WILD { v } else { (v * 4.0).round() / 4.0 };
        let mut species: Vec<SpeciesView> = t
            .species
            .iter()
            .filter(|s| species_listed(s, season_days))
            .map(|s| SpeciesView {
                id: s.id,
                name: s.name.clone(),
                origin: s.origin.label().into(),
                parent: s.parent,
                founded_day: s.founded_day,
                extinct_day: s.extinct_day,
                merged_into: s.merged_into,
                merged_into_name: s.merged_into.and_then(|m| ch.species.get(m)).map(|m| m.name.clone()),
                founder: s.founder,
                founder_name: s.founder.map(|p| self.name(p)),
                namer: self.species_namer(s),
                named: self.economy.named_species.contains_key(&s.id),
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
                        let owner_name = self.name(owner);
                        SpeciesLineage { lineage: l, name, owner, owner_name, adults }
                    })
                    .collect(),
                // A player's species: quarters, as the leaderboard history
                // gives rivals' genes (exact means were a design recipe).
                carnivory: coarse(s.dominant, s.carnivory),
                body_size: coarse(s.dominant, s.body_size),
                endothermy: family::rep_mean(s.reps(), Gene::Endothermy).map(|v| coarse(s.dominant, v)),
                birth_mode: family::rep_mean(s.reps(), Gene::BirthMode).map(|v| coarse(s.dominant, v)),
                leg_count: family::rep_count(s.reps(), evo_sim::morph::MorphLocus::LegCount),
                eye_count: family::rep_count(s.reps(), evo_sim::morph::MorphLocus::EyeCount),
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
                    rows: {
                        let ranking: Vec<(PlayerId, f64, f64)> =
                            e.ranking.iter().map(|r| (r.player, r.share, r.weighted_share)).collect();
                        let means = self.era_scores.means(e.era);
                        era_order(&ranking, means)
                            .into_iter()
                            .map(|(player, share, weighted_share)| EraRow {
                                player,
                                name: self.name(player),
                                share,
                                weighted_share,
                                mean_share: means.map(|m| m.get(&player).copied().unwrap_or(0.0)),
                            })
                            .collect()
                    },
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
        let real = |a: &&evo_sim::chronicle::Achievement| match a.kind {
            AchievementKind::SpeciesFounded { species } => !species_redetected(t, species, a.player),
            _ => true,
        };
        for a in ch.achievements.iter().filter(real) {
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
            .filter(real)
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
                alive: species.iter().filter(|s| s.extinct_day.is_none()).count(),
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

    /// Eggs lost per lineage since the last day boundary, by cause.
    fn eggs_lost_since_boundary(&self) -> BTreeMap<u32, BTreeMap<String, u64>> {
        let mut out = BTreeMap::new();
        for (&l, now) in &self.world.counters.eggs {
            let before = self.prev_eggs.get(&l).copied().unwrap_or_default();
            let lost = egg_losses(now, &before);
            if !lost.is_empty() {
                out.insert(l, lost);
            }
        }
        out
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
        let mut eggs_lost = self.eggs_lost_since_boundary();
        self.prev_born = self.world.counters.born.clone();
        self.prev_deaths = self.world.counters.deaths_by_lineage.clone();
        self.prev_eggs = self.world.counters.eggs.clone();
        let sums = self.gene_sums();
        // Player lineages' body plans, for the family tree.
        let mut plans: BTreeMap<u32, Vec<&evo_sim::morph::Morphology>> = BTreeMap::new();
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner != WILD) {
            plans.entry(o.lineage).or_default().push(&o.morph);
        }
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
                        eggs_lost: eggs_lost.remove(&k).unwrap_or_default(),
                        genes: if l.owner == WILD { BTreeMap::new() } else { gene_means(k) },
                        morph: if l.owner == WILD {
                            None
                        } else {
                            plans.get(&k).and_then(|p| family::body_plan_mode(p.iter().copied()))
                        },
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
            // A record is taken as the next day starts: its births and deaths
            // happened the day before (the chart and the daily summary agree).
            // The world's first record (day 0) covers no day.
            if r.day > 0 {
                vitals.push(DayVitals {
                    day: r.day - 1,
                    births: e.born,
                    deaths: e.deaths.clone(),
                    eggs_lost: e.eggs_lost.clone(),
                });
            }
            if !e.genes.is_empty() {
                history_days.push(r.day);
                for (k, &g) in Gene::ALL.iter().enumerate() {
                    gene_hist[k].push(e.genes.get(&gene_key(g)).copied().unwrap_or(f32::NAN));
                }
            }
        }
        if let Some(l) = lineage {
            let (born, mut deaths) = self.vitals_since_boundary();
            let (births, deaths) = (born.get(&l).copied().unwrap_or(0), deaths.remove(&l).unwrap_or_default());
            let eggs_lost = self.eggs_lost_since_boundary().remove(&l).unwrap_or_default();
            // Today so far: since the last day boundary, so the day it began
            // (the clock can show the next day for a tick before the record).
            let day = self.world.latest_legacy().map_or(day, |l| l.day);
            match vitals.last_mut().filter(|v| v.day >= day) {
                Some(v) => {
                    v.births += births;
                    for (cause, n) in deaths {
                        *v.deaths.entry(cause).or_default() += n;
                    }
                    for (cause, n) in eggs_lost {
                        *v.eggs_lost.entry(cause).or_default() += n;
                    }
                }
                None => vitals.push(DayVitals { day, births, deaths, eggs_lost }),
            }
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
            score: self.score_breakdown(player),
            crowding: self.crowding_view(player),
            ramp: self.founded_day.get(&player).map(|&f| crate::protocol::RampView {
                founded_day: f,
                weight: founding_ramp(Some(f), day),
                days: FOUNDING_RAMP_DAYS,
            }),
            grace_days_left: lineage.and_then(|l| self.world.lineage_grace_left(l)),
            warnings: self.lineage_warnings(player),
            eggs: lineage.and_then(|l| self.eggs_view(l)),
            ended: self.lineage_ended(player),
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
            days_to_winter: self.world.days_until_winter(),
            days_per_year: self.world.config.days_per_year,
            ticks_per_day: self.world.config.ticks_per_day,
            winter_provision: self.winter_provision_now(),
            refound_window_days: self.refound_window_days(),
            refound_window_minutes: (self.refound_window_days() as f64
                * self.world.config.ticks_per_day as f64
                * self.cfg.tick_ms
                / 60_000.0)
                .round() as u64,
            lineage_cap: self.world.config.lineage_cap,
            lineage_hard_cap: self.world.player_ceiling(),
            protected: self
                .lineage_of
                .iter()
                .filter_map(|(&player, &lineage)| {
                    let days_left = self.world.lineage_grace_left(lineage)?;
                    Some(crate::protocol::ProtectedLineage { player, lineage, days_left })
                })
                .collect(),
        }
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

/// Why an organism died, as alerts tell it: the sim's cause, with exposure
/// split into cold, heat and a winter sleep it never woke from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    Of(Option<DeathCause>),
    Cold,
    Heat,
    WinterSleep,
}

/// Why `o` died: exposure told apart by the state it died in (dormant, or
/// further from its comfort range on the cold or the hot side).
fn why_died(o: &Organism) -> Why {
    match o.death {
        Some(DeathCause::Exposure) if o.dormancy != evo_sim::organism::Dormancy::Awake => Why::WinterSleep,
        // Snapshots before version 52 counted heat as exposure: told apart by its needs.
        Some(DeathCause::Exposure) if o.needs.heat > o.needs.cold => Why::Heat,
        Some(DeathCause::Exposure) => Why::Cold,
        Some(DeathCause::Heat) => Why::Heat,
        cause => Why::Of(cause),
    }
}

/// How a death is told: "was eaten", "starved", "died of thirst", …
fn death_words(why: Why) -> &'static str {
    match why {
        Why::Cold => "died of cold",
        Why::Heat => "died of heat",
        Why::WinterSleep => "died in its winter sleep",
        Why::Of(Some(DeathCause::Predation)) => "was eaten",
        Why::Of(Some(DeathCause::OldAge)) => "died of old age",
        Why::Of(Some(DeathCause::Starvation)) => "starved",
        Why::Of(Some(DeathCause::Thirst)) => "died of thirst",
        Why::Of(Some(DeathCause::Exposure)) => "died of cold or heat",
        Why::Of(Some(DeathCause::Toxin)) => "was poisoned",
        Why::Of(Some(DeathCause::Drowning)) => "drowned",
        Why::Of(Some(DeathCause::Attrition)) => "died of attrition (illness or an accident)",
        Why::Of(Some(DeathCause::Recessive)) => "died of an inherited defect",
        Why::Of(Some(DeathCause::Disease)) => "died of disease",
        Why::Of(Some(DeathCause::Journey)) => "died on a journey",
        Why::Of(Some(DeathCause::WinterSleep)) => "died in its winter sleep",
        Why::Of(Some(DeathCause::Heat)) => "died of heat",
        Why::Of(None) => "died",
    }
}

/// A cause as a count's noun phrase: "3 eaten", "2 of thirst".
fn death_count_words(why: Why, n: usize) -> String {
    let what = match why {
        Why::Cold => "of cold",
        Why::Heat => "of heat",
        Why::WinterSleep => "in their winter sleep",
        Why::Of(Some(DeathCause::Predation)) => "eaten",
        Why::Of(Some(DeathCause::OldAge)) => "of old age",
        Why::Of(Some(DeathCause::Starvation)) => "starved",
        Why::Of(Some(DeathCause::Thirst)) => "of thirst",
        Why::Of(Some(DeathCause::Exposure)) => "of cold or heat",
        Why::Of(Some(DeathCause::Toxin)) => "poisoned",
        Why::Of(Some(DeathCause::Drowning)) => "drowned",
        Why::Of(Some(DeathCause::Attrition)) => "of attrition",
        Why::Of(Some(DeathCause::Recessive)) => "of an inherited defect",
        Why::Of(Some(DeathCause::Disease)) => "of disease",
        Why::Of(Some(DeathCause::Journey)) => "on a journey",
        Why::Of(Some(DeathCause::WinterSleep)) => "in their winter sleep",
        Why::Of(Some(DeathCause::Heat)) => "of heat",
        Why::Of(None) => "of an unrecorded cause",
    };
    format!("{n} {what}")
}

/// One alert for a player's deaths in the last hour, and the organism it
/// is about when there is just one.
fn death_alert_text(deaths: &[(OrgId, Watch, Why)]) -> (String, Option<OrgId>) {
    let role = |w: Watch| match w {
        Watch::Founder => "Founder",
        // Watched since it was possessed; possessed no more when it died
        // (that death is told at once: `announce_deaths`).
        Watch::Possessed => "Once-possessed",
    };
    if let [(id, watch, cause)] = deaths {
        return (format!("{} #{id} {}.", role(*watch), death_words(*cause)), Some(*id));
    }
    let founders = deaths.iter().all(|d| d.1 == Watch::Founder);
    let mut by_cause: Vec<(Why, usize)> = Vec::new();
    for d in deaths {
        match by_cause.iter_mut().find(|(c, _)| *c == d.2) {
            Some(e) => e.1 += 1,
            None => by_cause.push((d.2, 1)),
        }
    }
    by_cause.sort_by_key(|b| std::cmp::Reverse(b.1));
    let causes: Vec<String> = by_cause.iter().map(|&(c, n)| death_count_words(c, n)).collect();
    let ids: Vec<String> = deaths.iter().take(6).map(|d| format!("#{}", d.0)).collect();
    let more = if deaths.len() > 6 { ", …" } else { "" };
    let who = if founders { "founders" } else { "watched organisms" };
    (format!("{} {who} died: {} ({}{more}).", deaths.len(), causes.join(", "), ids.join(", ")), None)
}

/// One era row: player, final share, final weighted share.
type EraRowData = (PlayerId, f64, f64);

/// An era's standings, best first: by mean day score when the era was
/// scored day by day (`eras::EraScores`), else by the final share (older
/// eras). Each row: player, final share, final weighted share.
fn era_order(ranking: &[EraRowData], means: Option<&BTreeMap<PlayerId, f64>>) -> Vec<EraRowData> {
    let mut rows = ranking.to_vec();
    if let Some(m) = means {
        // Players with a mean but no alleles at the end still rank (they held a share for the era).
        for &p in m.keys() {
            if !rows.iter().any(|r| r.0 == p) {
                rows.push((p, 0.0, 0.0));
            }
        }
        let mean = |p: PlayerId| m.get(&p).copied().unwrap_or(0.0);
        rows.sort_by(|a, b| mean(b.0).total_cmp(&mean(a.0)).then(b.1.total_cmp(&a.1)).then(a.0.cmp(&b.0)));
    }
    rows
}

/// How an attack order of `attacker` on `target` ended, in words, or None
/// when the player ended it (released, or gave another order).
fn attack_result_text(w: &World, attacker: OrgId, target: OrgId) -> Option<String> {
    let find = |id: OrgId| w.organisms.iter().find(|o| o.id == id);
    let a = find(attacker);
    let t = find(target);
    if let Some(a) = a.filter(|a| !a.alive) {
        return Some(format!(
            "Attack over: your #{attacker} {} before it reached #{target}.",
            death_words(why_died(a))
        ));
    }
    let Some(a) = a else {
        return Some(format!("Attack over: #{attacker} is gone."));
    };
    a.possessed_by?; // released: the player ended it
    match t {
        Some(t) if !t.alive => Some(match t.death {
            Some(DeathCause::Predation) if a.pos.dist(t.pos) <= 3.0 => {
                format!("Attack over: your #{attacker} killed #{target}.")
            }
            _ => format!("Attack over: #{target} {} before your #{attacker} could kill it.", death_words(why_died(t))),
        }),
        Some(t) if t.sheltered => {
            Some(format!("Attack over: #{target} went to ground (a burrow or shelter) and escaped your #{attacker}."))
        }
        Some(t) if a.pos.dist(t.pos) > evo_sim::world::commands::ORDER_KEPT * w.attack_range() => Some(format!(
            "Attack over: #{target} escaped; your #{attacker} lost sight of it ({:.0} tiles away).",
            a.pos.dist(t.pos)
        )),
        Some(_) => None, // another order replaced it
        None => Some(format!("Attack over: your #{attacker} lost track of #{target} (it died or left the area).")),
    }
}

/// A drought warning from this share of land in drought (the playtests'
/// thirst die-offs began around 47%; an ordinary summer is about 35%).
pub const DROUGHT_WARN: f32 = 0.45;
/// The warning clears below this share.
pub const DROUGHT_CLEAR: f32 = 0.3;

/// The drought warning's text.
fn drought_text(drought: f32) -> String {
    format!(
        "Drought: {:.0}% of the land is dry. Animals far from lakes and rivers go thirsty and plants thin out; \
         keep herds near water (territories and rallies away from water are risky).",
        drought * 100.0
    )
}

/// How the world's population changed over a catastrophe, in words.
fn population_change_text(before: usize, after: usize) -> String {
    match after.cmp(&before) {
        std::cmp::Ordering::Less => format!("{} fewer organisms in the world than at its start", before - after),
        std::cmp::Ordering::Equal => "the world's population is where it was at its start".to_string(),
        std::cmp::Ordering::Greater => format!("{} more organisms in the world than at its start", after - before),
    }
}

/// The catastrophe's name to start a sentence ("The severe winter").
fn catastrophe_name(k: CatastropheKind) -> &'static str {
    match k {
        CatastropheKind::SevereWinter => "The severe winter",
        CatastropheKind::Megadrought => "The megadrought",
        CatastropheKind::Epidemic => "The epidemic",
        CatastropheKind::FloodYear => "The flood year",
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
            // "among herbivores" for a diet, "in the forest" for a biome.
            let label = if axis == "diet" { format!("among {what}s") } else { format!("in the {what}") };
            format!(
                "Niche dominance: when era {} ended, {:.0}% of the breeding alleles {label} were yours.",
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
            // Factual, not a cheer: it can come after heavy losses (playtest 3: after a 70% loss).
            format!("Survivor: your lineage is still alive after {what}, whatever it lost.")
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
            .map(|&(g, gap)| format!("{} ({gap:.2} apart)", g.display_name_lower()))
            .collect();
        // No gene past the sterile gap: the eye or leg counts are
        // (`morph::morph_fertility_gap`), which the gene list doesn't show.
        let msg = if sterile.is_empty() {
            "different body plans: their eye or leg counts differ too much to conceive".to_string()
        } else {
            format!("different body plans: {} differ too much to conceive", sterile.join(", "))
        };
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

/// A command's target as used: `{x, y}`, plus `{moved: true, note}` when
/// the world moved it from `asked` (water, a cliff, out of reach).
fn target_data(asked: Vec2, used: Vec2, note: &str) -> serde_json::Value {
    let mut v = serde_json::json!({ "x": used.x, "y": used.y });
    // (Snapping to a tile's centre moves a target by under a tile: no news.)
    if asked.dist(used) > 1.0 {
        v["moved"] = true.into();
        v["note"] = note.into();
    }
    v
}

/// Why a migration ended without arriving, from the group's state then:
/// the alert's sentence and a short form for the group's card.
fn timeout_reason(
    dormant_share: f32,
    basking_share: f32,
    needy_share: f32,
    never_moved: bool,
) -> (String, &'static str) {
    if dormant_share >= 0.5 {
        (
            format!(
                "{:.0}% of the group is dormant (hibernating or estivating) and can't walk; migrate again once it wakes",
                dormant_share * 100.0
            ),
            "dormant",
        )
    } else if basking_share >= 0.5 {
        (
            format!(
                "{:.0}% of the group was basking or resting (cold-blooded animals warm up before they travel); \
                 migrate again in warmer weather, or to a nearer point",
                basking_share * 100.0
            ),
            "too cold to travel",
        )
    } else if needy_share >= 0.5 {
        (
            format!(
                "{:.0}% of the group was too hungry or thirsty to travel (it stops to eat and drink first); \
                 migrate to a nearer point with food and water, or once it is fed",
                needy_share * 100.0
            ),
            "too hungry or thirsty to travel",
        )
    } else if never_moved {
        (
            "it never got going: the way may be blocked (water it can't cross, cliffs) or the group too busy with \
             hunger and thirst. Try a nearer point on its side of the water"
                .to_string(),
            "never got going",
        )
    } else {
        ("it stopped getting closer. Rally it, or migrate again to a nearer point".to_string(), "stalled")
    }
}

/// Say in a command's reply which of the group's standing orders it
/// replaced (`replaced`, and a sentence in `note`): a rally or claim used
/// to cancel a migration without a word.
fn note_replaced(data: &mut serde_json::Value, before: &[&'static str], new: &str) {
    let _ = new;
    let gone: Vec<&str> = before.to_vec();
    if gone.is_empty() {
        return;
    }
    data["replaced"] = gone.clone().into();
    let text = format!("this replaces the group's {}", gone.join(" and "));
    data["note"] = match data["note"].as_str() {
        Some(n) => format!("{n}; {text}").into(),
        None => text.into(),
    };
}

/// A number of days in words: "less than a day", "about 1 day", "about 5 days".
fn days_text(days: f32) -> String {
    if days < 1.0 {
        "less than a day".to_string()
    } else if days < 1.5 {
        "about 1 day".to_string()
    } else {
        format!("about {days:.0} days")
    }
}

/// Put a command's warnings in its reply: `warnings` (a list) and
/// `warning` (them joined), when there are any.
fn put_warnings(data: &mut serde_json::Value, warnings: Vec<String>) {
    if !warnings.is_empty() {
        data["warning"] = warnings.join(" ").into();
        data["warnings"] = warnings.into();
    }
}

/// How a group is named to its player, as the client names it: "Group A
/// (main)", "Group B"...
fn group_name(group: u8) -> String {
    let letter = (b'A' + group.min(25)) as char;
    if group == 0 {
        format!("Group {letter} (main)")
    } else {
        format!("Group {letter}")
    }
}

/// The share of a lineage founded on `founded` counted toward the season
/// on `day`: ramping from 0 to 1 over [`FOUNDING_RAMP_DAYS`] (1 if the
/// founding day isn't known).
pub fn founding_ramp(founded: Option<u64>, day: u64) -> f64 {
    match founded {
        Some(f) => (day.saturating_sub(f) as f64 / FOUNDING_RAMP_DAYS as f64).min(1.0),
        None => 1.0,
    }
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

    #[test]
    fn death_alerts_name_the_cause_and_group() {
        let (t, id) = death_alert_text(&[(5, Watch::Founder, Why::Of(Some(DeathCause::Starvation)))]);
        assert_eq!((t.as_str(), id), ("Founder #5 starved.", Some(5)));
        let many = [
            (1, Watch::Founder, Why::Of(Some(DeathCause::Predation))),
            (2, Watch::Founder, Why::Of(Some(DeathCause::Thirst))),
            (3, Watch::Founder, Why::Of(Some(DeathCause::Predation))),
        ];
        let (t, id) = death_alert_text(&many);
        assert_eq!(t, "3 founders died: 2 eaten, 1 of thirst (#1, #2, #3).");
        assert_eq!(id, None);
        let mixed = [(1, Watch::Founder, Why::Of(None)), (2, Watch::Possessed, Why::Of(None))];
        assert_eq!(death_alert_text(&mixed).0, "2 watched organisms died: 2 of an unrecorded cause (#1, #2).");
        // Exposure says which; a once-possessed animal isn't "possessed".
        assert_eq!(death_alert_text(&[(7, Watch::Possessed, Why::Heat)]).0, "Once-possessed #7 died of heat.");
        let three =
            [(1, Watch::Founder, Why::Cold), (2, Watch::Founder, Why::Cold), (3, Watch::Founder, Why::WinterSleep)];
        assert_eq!(death_alert_text(&three).0, "3 founders died: 2 of cold, 1 in their winter sleep (#1, #2, #3).");
    }

    /// Bring `b` within sight of `a` (5 tiles off, not yet met): a request
    /// or a preview names only an animal in sight.
    fn in_sight_of(g: &mut Game, a: OrgId, b: OrgId) {
        let pos = g.world.organism(a).unwrap().pos;
        let to = g.world.map.clamp(pos + Vec2::new(5.0, 0.0));
        let to = if to.dist(pos) < 4.0 { g.world.map.clamp(pos - Vec2::new(5.0, 0.0)) } else { to };
        g.world.organisms.iter_mut().find(|o| o.id == b).unwrap().pos = to;
    }

    fn place_together(g: &mut Game, a: OrgId, b: OrgId) {
        let pos = g.world.organism(a).unwrap().pos;
        g.world.organisms.iter_mut().find(|o| o.id == b).unwrap().pos = pos;
    }

    fn err_message(out: &Outbox) -> String {
        match &out[0].1 {
            ServerMsg::Result { error: Some(e), .. } => e.message.clone(),
            _ => String::new(),
        }
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

    /// Kill every organism the player owns and drop their eggs, then run a
    /// day (the dead leave the world at the day boundary).
    fn wipe_out(g: &mut Game, player: PlayerId) {
        assert!(g.e2e_wipe_out(player) > 0);
        for _ in 0..g.world.config.ticks_per_day {
            g.world.step();
        }
    }

    #[test]
    fn lineage_stats_tell_egg_layers_their_clutches() {
        let (mut g, _, _) = game();
        assert!(g.lineage_stats(1).eggs.is_none(), "a live-bearing lineage has no clutches to show");
        g.set_name(3, "cat");
        g.found(3, &FounderDesign::new("Layers", Thermo::Warm, BirthType::IncubatedEggs, 4).founders(6)).unwrap();
        let e = g.lineage_stats(3).eggs.expect("an egg-laying lineage shows its clutches");
        assert_eq!(e, EggsView::default());
    }

    /// Playtest 3: a winter founding's clutches vanished with no word. Lost
    /// eggs show in the lineage stats, the day's vitals and the daily summary.
    #[test]
    fn lost_eggs_are_reported() {
        let (mut g, _, _) = game();
        g.set_name(3, "cat");
        let ok =
            g.found(3, &FounderDesign::new("Layers", Thermo::Warm, BirthType::IncubatedEggs, 4).founders(6)).unwrap();
        // Past the world's first record (day 0, which covers no day).
        run_days(&mut g, 1);
        let counts = g.world.counters.eggs.entry(ok.lineage).or_default();
        (counts.laid, counts.chilled, counts.raided) = (9, 5, 1);
        let lost = g.lineage_stats(3).eggs.unwrap().lost;
        assert_eq!(lost, [("chilled".to_string(), 5), ("raided".to_string(), 1)].into_iter().collect());
        let out = run_days(&mut g, 1);
        let summary = alerts_in(&out, 3)
            .into_iter()
            .find(|x| x.kind == AlertKind::DailySummary)
            .expect("a day with lost eggs is summarised");
        assert!(
            summary.text.contains("6 eggs lost (5 chilled: no parent was brooding them (it strayed too far from the nest, or died), 1 eaten by egg raiders)"),
            "{}",
            summary.text
        );
        let vitals: u64 = g.lineage_stats(3).vitals.iter().flat_map(|v| v.eggs_lost.values()).sum();
        assert_eq!(vitals, 6);
        // Counted once: the next day reports none.
        let out = run_days(&mut g, 1);
        assert!(alerts_in(&out, 3).iter().all(|x| !x.text.contains("eggs lost")));
    }

    /// A lineage that abandons its eggs is told they lie exposed by design,
    /// not that no parent was warming them (playtest 4).
    #[test]
    fn chilled_abandoned_eggs_are_worded_as_by_design() {
        let (mut g, _, _) = game();
        g.set_name(3, "cat");
        let ok =
            g.found(3, &FounderDesign::new("Frogs", Thermo::Cold, BirthType::AbandonedEggs, 8).founders(6)).unwrap();
        run_days(&mut g, 1);
        let counts = g.world.counters.eggs.entry(ok.lineage).or_default();
        (counts.laid, counts.chilled) = (9, 4);
        let out = run_days(&mut g, 1);
        let summary = alerts_in(&out, 3).into_iter().find(|x| x.kind == AlertKind::DailySummary).expect("summary");
        assert!(summary.text.contains("4 chilled: abandoned eggs lie exposed, by design"), "{}", summary.text);
    }

    #[test]
    fn refounding_only_after_extinction() {
        let (mut g, _, _) = game();
        let first = g.lineage_of(1).unwrap();
        // Refused while the lineage lives.
        assert!(!g.is_extinct(1));
        assert!(matches!(g.found(1, &design("again")), Err(FoundError::AlreadyFounded { .. })));
        // A player who never founded is not "extinct".
        assert!(!g.is_extinct(99));
        let renown = g.balance(1);
        wipe_out(&mut g, 1);
        assert_eq!(g.owner_population(1), 0);
        assert!(g.is_extinct(1));
        assert!(!g.is_extinct(2), "bob is untouched");
        assert_eq!(g.lineage_stats(1).ended.as_deref(), Some("died_out"));
        assert_eq!(g.lineage_stats(2).ended, None);
        let ok = g.found(1, &design("Second try")).expect("refound after extinction");
        assert_eq!(g.lineage_stats(1).ended, None, "a living lineage has not ended");
        assert_ne!(ok.lineage, first);
        assert_eq!(g.lineage_of(1), Some(ok.lineage));
        assert!(!ok.founders.is_empty());
        assert_eq!(g.owner_population(1), ok.founders.len());
        assert_eq!(g.world.lineages[first as usize].owner, 1, "the old lineage stays in the history");
        assert_eq!(g.balance(1), renown, "renown kept, no second founding grant");
        // The new lineage is alive: a further lineage is refused again.
        assert!(
            matches!(g.found(1, &design("third")), Err(FoundError::AlreadyFounded { lineage }) if lineage == ok.lineage)
        );
    }

    #[test]
    fn founders_start_provisioned_just_before_winter() {
        let world = build_world(&WorldSettings {
            seed: 7,
            width: 64,
            height: 64,
            wild: false,
            days_per_year: 120,
            ..Default::default()
        });
        let mut g = Game::new(world, GameConfig::default());
        // Spring: founded as before (no fat, ordinary hunger).
        assert!(!g.winter_provision_now());
        let spring = g.found(1, &design("Spring")).unwrap().founders;
        assert!(spring.iter().all(|&id| g.world.organism(id).unwrap().fat == 0.0));
        // Ten days before winter: fed, fat reserves full.
        while g.world.days_until_winter() > 10 {
            g.world.step();
        }
        assert!(g.winter_provision_now());
        assert_eq!(g.summary().days_to_winter, 10);
        assert!(g.summary().winter_provision);
        let late = g.found(2, &design("Late")).unwrap().founders;
        for &id in &late {
            let o = g.world.organism(id).unwrap();
            assert_eq!(o.needs.hunger, 0.0);
            assert!(o.fat > 0.0, "founder {id} has no fat");
        }
        // Switched off: founded as before.
        g.cfg.winter_provision = false;
        assert!(!g.winter_provision_now());
        let off = g.found(3, &design("Off")).unwrap().founders;
        assert!(off.iter().all(|&id| g.world.organism(id).unwrap().fat == 0.0));
    }

    /// Playtest 4: possessing a second animal released the first silently;
    /// a possessed animal that bedded down said nothing; one possessed when
    /// it died was called "Once-possessed".
    #[test]
    fn possession_says_what_it_released_slept_and_died() {
        let (mut g, a, _) = game();
        g.command(10, 1, None, ClientMsg::Possess { organism: a[0] });
        let out = g.command(10, 1, None, ClientMsg::Possess { organism: a[1] });
        let ServerMsg::Result { ok: true, data: Some(d), .. } = &out[0].1 else { panic!("{:?}", out[0].1) };
        assert_eq!(d["released"], serde_json::json!(a[0]));
        assert!(d["notice"].as_str().unwrap().contains(&format!("Released #{}", a[0])));
        // Bedded down: said once, not every tick of the sleep.
        // (Set between ticks: in this spring world it would wake at once.)
        g.world.organisms.iter_mut().find(|o| o.id == a[1]).unwrap().dormancy = evo_sim::organism::Dormancy::Winter;
        let said = |out: &Outbox| alerts_in(out, 1).iter().filter(|x| x.text.contains("bedded down")).count();
        let first = g.after_tick();
        assert_eq!((said(&first), said(&g.after_tick())), (1, 0));
        // Killed while possessed: the death says so.
        let i = g.world.organisms.iter().position(|o| o.id == a[1]).unwrap();
        g.world.organisms[i].health = -1.0;
        g.world.organisms[i].dormancy = evo_sim::organism::Dormancy::Awake;
        let mut text = String::new();
        for _ in 0..g.world.config.ticks_per_hour() + 1 {
            g.world.step();
            for x in alerts_in(&g.after_tick(), 1) {
                if x.kind == AlertKind::Death {
                    text = x.text;
                }
            }
            if !text.is_empty() {
                break;
            }
        }
        assert!(text.contains("the animal you were possessing") && !text.contains("Once-possessed"), "{text}");
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

    fn result_error(out: &Outbox) -> Option<ErrorInfo> {
        match &out[0].1 {
            ServerMsg::Result { error, .. } => error.clone(),
            _ => None,
        }
    }

    fn reply_data(out: &Outbox) -> serde_json::Value {
        match &out[0].1 {
            ServerMsg::Result { ok: true, data: Some(d), .. } => d.clone(),
            m => panic!("expected data, got {m:?}"),
        }
    }

    /// Playtest 5 (M10): a migration's reply says what becomes of the
    /// group's brooding parents.
    #[test]
    fn migrate_reply_tells_of_the_brooding_parents() {
        let (mut g, a, _) = game();
        let out = g.command(10, 1, None, ClientMsg::Migrate { x: 50.0, y: 50.0, group: Some(0) });
        assert!(reply_data(&out).get("brooding_note").is_none());
        for &id in &a[..2] {
            let k = g.world.organism_index(id).unwrap();
            g.world.organisms[k].nest = Some(g.world.organisms[k].pos);
        }
        let out = g.command(10, 1, None, ClientMsg::Migrate { x: 40.0, y: 40.0, group: Some(0) });
        let d = reply_data(&out);
        assert_eq!(d["brooding"].as_u64(), Some(2));
        assert!(d["brooding_note"].as_str().unwrap().starts_with("2 brooding parents stay"), "{d}");
        g.world.config.balance.brood_care = 0.0;
        let out = g.command(10, 1, None, ClientMsg::Migrate { x: 45.0, y: 45.0, group: Some(0) });
        assert_eq!(reply_data(&out)["brooding_note"].as_str(), Some("2 clutches will be left behind and may chill."));
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
        // (Near the group: a claim farther than FAR_CLAIM_TILES migrates first.)
        let (cx, cy) = g.world.group_middle(1, 1).map(|m| (m.x.round(), m.y.round())).unwrap();
        let out = g.command(10, 1, None, ClientMsg::ClaimTerritory { group: 1, x: cx, y: cy, radius: 500.0 });
        let t: TerritoryView = serde_json::from_value(reply_data(&out)).unwrap();
        assert_eq!((t.x, t.y), (cx, cy));
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
        // Group ids in range but without members don't exist either.
        let empty = ClientMsg::Migrate { x: 20.0, y: 20.0, group: Some(5) };
        assert_eq!(code(&mut g, 10, 1, empty).as_deref(), Some("no_such_group"));
        assert_eq!(code(&mut g, 10, 1, ClientMsg::ClearMigration { group: Some(5) }).as_deref(), Some("no_such_group"));
        assert_eq!(g.world.territory(1, 1).map(|t| (t.center.x, t.center.y)), Some((cx, cy)));
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

    /// A rally through the protocol: validated like the other group orders,
    /// shown in the state, replaced by a migration, cancelled.
    #[test]
    fn rally_gathers_a_group_until_cancelled_or_replaced() {
        let (mut g, a, _) = game();
        let code = |g: &mut Game, conn, player, m| result(&g.command(conn, player, None, m)).1;
        let out = g.command(10, 1, None, ClientMsg::Rally { group: 0, x: 30.0, y: 31.0 });
        let r: RallyView = serde_json::from_value(reply_data(&out)).unwrap();
        assert_eq!((r.x, r.y, r.radius), (30.0, 31.0, World::rally_radius(a.len())));
        assert!(r.expires_tick > g.world.tick());
        let s = g.state_for(10).unwrap();
        assert_eq!(s.groups[0].rally, Some(r));
        // Bob sees none of it.
        assert_eq!(g.state_for(20).unwrap().groups[0].rally, None);
        let nan = ClientMsg::Rally { group: 0, x: f32::NAN, y: 1.0 };
        assert_eq!(code(&mut g, 10, 1, nan).as_deref(), Some("bad_coordinates"));
        let empty = ClientMsg::Rally { group: 2, x: 1.0, y: 1.0 };
        assert_eq!(code(&mut g, 10, 1, empty).as_deref(), Some("no_such_group"));
        let far = ClientMsg::Rally { group: 9, x: 1.0, y: 1.0 };
        assert_eq!(code(&mut g, 10, 1, far).as_deref(), Some("no_such_group"));
        assert_eq!(code(&mut g, 20, 2, ClientMsg::CancelRally { group: 0 }).as_deref(), Some("no_rally"));
        // A migration replaces it; a new rally replaces the migration.
        g.command(10, 1, None, ClientMsg::Migrate { x: 50.0, y: 50.0, group: Some(0) });
        let s = g.state_for(10).unwrap();
        assert_eq!((s.groups[0].rally, s.groups[0].migration), (None, Some([50.0, 50.0])));
        g.command(10, 1, None, ClientMsg::Rally { group: 0, x: 30.0, y: 31.0 });
        assert_eq!(g.state_for(10).unwrap().groups[0].migration, None);
        assert_eq!(result(&g.command(10, 1, None, ClientMsg::CancelRally { group: 0 })), (true, None));
        assert_eq!(g.state_for(10).unwrap().groups[0].rally, None);
        assert_eq!(code(&mut g, 10, 1, ClientMsg::CancelRally { group: 0 }).as_deref(), Some("no_rally"));
    }

    #[test]
    fn state_hides_the_far_world() {
        let (mut g, a, b) = game();
        let s = g.state_for(10).unwrap();
        assert_eq!(s.organisms.len(), a.len());
        assert!(s.nearby.iter().all(|n| n.owner != 1));
        // Pin the view on Bob's founders: they appear; then far away: none.
        // (A view reaches only near one's own animals: one of Ann's is there.)
        in_sight_of(&mut g, b[0], a[0]);
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
        let out = g.command(10, 1, None, ClientMsg::RespondMating { request: offer.id, accept: true, partner: None });
        assert_eq!(result(&out).1.as_deref(), Some("unknown_offer"));
        let slots = g.world.mating_slots_left(1);
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: offer.id, accept: true, partner: None });
        assert_eq!(result(&out), (true, None));
        // Too far is no error for either side: the update carries where the
        // pair is, and they walk to meet.
        let walking = out.iter().any(|(_, m)| {
            matches!(m, ServerMsg::MatingUpdate { error: None, offer, .. } if offer.meet.is_some_and(|x| x.distance > 1.5))
        });
        assert!(walking, "{out:?}");
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
    fn mating_history_keeps_closed_offers_and_own_lineage_matings() {
        let (mut g, a, b) = game();
        assert!(g.mating_history(1).is_empty());
        // Within ann's own lineage: recorded for her, no offer.
        let (fa, ma) = (female(&g, &a), male(&g, &a));
        place_together(&mut g, fa, ma);
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: ma, price: None });
        assert_eq!(result(&out), (true, None));
        let h = g.mating_history(1);
        assert_eq!(h.len(), 1);
        assert!(h[0].offer.is_none() && h[0].outcome.is_some());
        assert_eq!((h[0].mine, h[0].partner), (fa, ma));
        assert!(g.mating_history(2).is_empty());
        // An offer bob declines: recorded for both, each from their own side.
        let mb = male(&g, &b);
        let fa2 = *a
            .iter()
            .find(|&&i| i != fa && g.world.organism(i).unwrap().sex == evo_sim::organism::Sex::Female)
            .unwrap();
        place_together(&mut g, fa2, mb);
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: fa2, partner: mb, price: None });
        let id = match &out[1].1 {
            ServerMsg::MatingRequest { offer } => offer.id,
            m => panic!("{m:?}"),
        };
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: false, partner: None });
        let (h1, h2) = (g.mating_history(1), g.mating_history(2));
        assert_eq!(h1.len(), 2, "newest first, the own-lineage mating after");
        let declined =
            |r: &MatingRecord| r.offer.as_ref().is_some_and(|o| o.id == id && o.status == OfferStatus::Declined);
        assert!(declined(&h1[0]) && h1[1].offer.is_none());
        assert!(declined(&h2[0]));
        assert_eq!((h1[0].mine, h1[0].partner), (fa2, mb));
        assert_eq!((h2[0].mine, h2[0].partner), (mb, fa2));
        // It survives a restart from the saved game.
        let r = Game::resume(g.world.clone(), GameConfig::default(), g.saved(), Vec::new());
        assert_eq!((r.mating_history(1), r.mating_history(2)), (g.mating_history(1), g.mating_history(2)));
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
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: false, partner: None });
        assert_eq!(result(&out), (true, None));
        assert!(out.iter().any(|(t, m)| *t == Target::Player(1)
            && matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Declined)));
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
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
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
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
        // Many requests from Ann to Bob in a row: no per-target limit here.
        let mut g = Game::new(world, GameConfig { request_window_ticks: 0, ..GameConfig::default() });
        let a = g.found(1, &design("A")).unwrap().founders;
        let b = g.found(2, &design("B")).unwrap().founders;
        g.connect(10, 1, "ann");
        let wild = g.world.organisms.iter().find(|o| o.owner == WILD).unwrap().id;
        in_sight_of(&mut g, a[0], wild);
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: a[0], partner: wild, price: None });
        assert_eq!(result(&out).1.as_deref(), Some("no_consent"));
        let partner =
            *b.iter().find(|&&x| g.world.organism(x).unwrap().sex != g.world.organism(a[0]).unwrap().sex).unwrap();
        let out = g.command(10, 1, None, ClientMsg::RequestMating { mine: a[0], partner, price: None });
        assert_eq!(result(&out), (true, None));
        let id = offer_id(&out);
        // A weather-season change no longer expires it (offers have
        // wall-clock lifetimes: 10 minutes pending, 30 accepted).
        let season = g.world.season();
        let mut ticks = 0;
        while g.world.season() == season {
            g.world.step();
            g.after_tick();
            ticks += 1;
            assert!(ticks < 3 * g.world.config.ticks_per_day, "no season change");
        }
        assert!(g.offers.contains_key(&id), "a season change expired the offer");
        let view = g.offer_view(id);
        assert_eq!(view.expires_tick, Some(g.offers[&id].created_tick + g.cfg.offer_ticks));
        // Unanswered past its lifetime: the sender is warned, then it expires.
        g.cfg.offer_ticks = g.world.tick() - g.offers[&id].created_tick + 5;
        g.cfg.offer_warn_ticks = 3;
        let (mut warned, mut expired) = (false, false);
        for _ in 0..8 {
            g.world.step();
            let out = g.after_tick();
            warned |= alerts_in(&out, 1).iter().any(|x| x.text.contains("expires in about"));
            expired |= out.iter().any(|(_, m)| {
                matches!(m, ServerMsg::MatingUpdate { offer, error: Some(e), .. }
                    if offer.status == OfferStatus::Expired && e.code == "expired")
            });
        }
        assert!(warned && expired, "warned {warned} expired {expired}");
        assert!(!g.offers.contains_key(&id));
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
        // The world's first record covers no day: days 1 and 2 (from 1) and today.
        assert_eq!(s.vitals.len(), 3, "two recorded days and today");
        // Each day once, in order, today last.
        let days = |s: &LineageStats| s.vitals.iter().map(|v| v.day).collect::<Vec<_>>();
        assert!(s.vitals.windows(2).all(|w| w[0].day < w[1].day), "{:?}", days(&s));
        assert_eq!(s.vitals.last().unwrap().day, g.world.latest_legacy().unwrap().day);
        // A record carrying today's day (as at the boundary) isn't repeated
        // by today's entry.
        let mut r = g.history.back().unwrap().clone();
        r.day = g.world.day();
        g.history.push_back(r);
        let t = g.lineage_stats(1);
        assert!(t.vitals.windows(2).all(|w| w[0].day < w[1].day), "{:?}", days(&t));
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
        // The urgency rule alone: with `drink_priority` a possessed animal
        // this near water breaks off already at thirst 60.
        g.world.config.balance.possess_drink = 0.0;
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
        // Once per spell of need: not again the next day while it lasts.
        g.world.step();
        assert!(alerts_in(&g.after_tick(), 1).iter().all(|x| x.kind != AlertKind::Need));
        let day = g.world.day();
        g.need_alerted.insert(id, day.saturating_sub(1));
        g.world.step();
        assert!(
            alerts_in(&g.after_tick(), 1).iter().all(|x| x.kind != AlertKind::Need),
            "a second alert for the same spell"
        );
        // Eased a moment (a kill, a sip), then back up: the same spell
        // (playtest 4: hunger swinging across the threshold alerted every
        // few seconds).
        set_thirst(&mut g, 20.0);
        g.world.step();
        g.after_tick();
        set_thirst(&mut g, 80.0);
        g.world.step();
        assert!(alerts_in(&g.after_tick(), 1).iter().all(|x| x.kind != AlertKind::Need), "an alert for a brief ease");
        // Eased for a game day, then thirsty again: a new spell, a new alert.
        for _ in 0..=g.world.config.ticks_per_day {
            set_thirst(&mut g, 20.0);
            g.world.step();
            g.after_tick();
        }
        set_thirst(&mut g, 80.0);
        g.world.step();
        assert!(alerts_in(&g.after_tick(), 1).iter().any(|x| x.kind == AlertKind::Need), "no alert for a new spell");
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

    /// With level of detail on, organisms in open agreements and on the
    /// market stay agents while nobody watches them: folded into a cohort
    /// they were "gone", so the agreement failed as if they had died, the
    /// listing closed as "no longer alive" and an auction's bids were
    /// refunded.
    #[test]
    fn level_of_detail_keeps_traded_organisms_as_agents() {
        let world = build_world(&WorldSettings {
            seed: 7,
            width: 128,
            height: 128,
            wild: true,
            lod: true,
            ..Default::default()
        });
        let mut g = Game::new(world, GameConfig::default());
        let a = g.found(1, &design("A").founders(12)).unwrap().founders;
        let b = g.found(2, &design("B").founders(12)).unwrap().founders;
        let (dam, stud) = (female(&g, &a), male(&g, &b));
        let listed = male(&g, &a);
        assert!(
            result(&g.command(
                10,
                1,
                Some(1),
                ClientMsg::CreateListing { organism: listed, fee: 5, expires_days: None }
            ))
            .0
        );
        let home = g.world.organism(stud).unwrap().pos;
        in_sight_of(&mut g, dam, stud);
        assert!(
            result(&g.command(20, 2, Some(2), ClientMsg::RequestMating { mine: stud, partner: dam, price: None })).0
        );
        g.world.organisms.iter_mut().find(|o| o.id == stud).unwrap().pos = home;
        // Nobody is connected: every region may fold into cohorts.
        run_days(&mut g, 2);
        assert!(g.world.abstract_count() > 0, "the test needs cohorts");
        for id in [dam, stud, listed] {
            assert!(g.world.organism(id).is_some(), "#{id} was folded into a cohort");
        }
        assert_eq!(g.offers.values().filter(|o| o.status.is_open()).count(), 1);
        assert!(g.economy.listing_of(listed).is_some(), "the listing closed");
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
    fn droughts_are_announced_once_and_when_they_ease() {
        let (mut g, _, _) = game();
        let mut out = Outbox::new();
        g.world.map.drought = 0.5;
        g.chronicle_alerts(&mut out);
        g.chronicle_alerts(&mut out); // still dry: no second warning
        let a = alerts_in(&out, 1);
        assert_eq!(a.iter().filter(|a| a.text.starts_with("Drought: 50% of the land is dry")).count(), 1, "{a:?}");
        assert!(a.iter().all(|a| a.kind == AlertKind::World));
        let mut out = Outbox::new();
        g.world.map.drought = 0.35; // between the thresholds: nothing
        g.chronicle_alerts(&mut out);
        g.world.map.drought = 0.2;
        g.chronicle_alerts(&mut out);
        let a = alerts_in(&out, 2);
        assert_eq!(a.len(), 1, "{a:?}");
        assert!(a[0].text.starts_with("The drought has eased: 20%"), "{a:?}");
        // A player who founds during a drought is told of it.
        g.world.map.drought = 0.5;
        g.chronicle_alerts(&mut Outbox::new());
        g.set_name(3, "cy");
        g.found(3, &design("C")).unwrap();
        let mut out = Outbox::new();
        g.tell_joiners(&mut out);
        let a = alerts_in(&out, 3);
        assert!(a.iter().any(|a| a.text.starts_with("Drought: 50%")), "{a:?}");
        g.tell_joiners(&mut out); // once
        assert_eq!(alerts_in(&out, 3).len(), a.len());
        // During a megadrought only its catastrophe alert goes out.
        g.world.map.drought = 0.1;
        g.chronicle_alerts(&mut Outbox::new()); // eased
        let id = g.world.schedule_catastrophe(CatastropheKind::Megadrought, 0, Some(100)) as usize;
        g.world.chronicle.catastrophes[id].started = true;
        g.seen_catastrophes.resize(id + 1, (true, false));
        g.world.map.drought = 0.9;
        let mut out = Outbox::new();
        g.chronicle_alerts(&mut out);
        assert!(alerts_in(&out, 1).iter().all(|a| !a.text.starts_with("Drought:")), "{:?}", alerts_in(&out, 1));
        assert_eq!(population_change_text(10, 10), "the world's population is where it was at its start");
        assert_eq!(population_change_text(10, 13), "3 more organisms in the world than at its start");
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
        // A player's species gives its body-plan genes in quarters only.
        let quarter = |x: f32| ((x * 4.0).round() - x * 4.0).abs() < 1e-5;
        let genes = [Some(mine.carnivory), Some(mine.body_size), mine.endothermy, mine.birth_mode];
        assert!(genes.iter().flatten().all(|&x| quarter(x)), "{genes:?}");
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
        in_sight_of(g, mine, partner);
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
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
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
        // Warm × cold alone is partly fertile (`Balance::thermo_hybrid`):
        // the abandoned eggs make the pair sterile.
        assert!(err.message.contains("birth mode") && !err.message.contains("warm"), "{}", err.message);
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
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
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
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        assert_eq!(g.escrow(1), 30, "still held while they are apart");
        place_together(&mut g, fa, mb);
        let out = g.after_tick();
        assert!(out.iter().any(
            |(_, m)| matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Completed)
        ));
        // Bob is paid the price less the 10% market fee, which is destroyed.
        assert_eq!((g.balance(1), g.escrow(1), g.balance(2)), (start - 30, 0, start + 27));
        let bob_alert = alerts_in(&out, 2).into_iter().find(|x| x.kind == AlertKind::Market).expect("sale alert");
        assert!(bob_alert.text.starts_with("Sale: 27 renown from ann"), "{}", bob_alert.text);
        assert!(bob_alert.text.contains("less a 3 renown market fee"), "{}", bob_alert.text);
        let ann_alert = alerts_in(&out, 1).into_iter().find(|x| x.text.starts_with("Mating done")).expect("buyer told");
        assert!(ann_alert.text.contains("30 renown paid from escrow"), "{}", ann_alert.text);
        // The sale is news to the seller, and the mating's outcome to the
        // buyer (who may have left the escort to the game).
        let notices = g.take_notices();
        assert!(notices.iter().any(|n| n.player == 2 && n.kind == PushKind::Market && n.text == bob_alert.text));
        assert!(notices.iter().any(|n| n.player == 1 && n.kind == PushKind::Mating && n.text == ann_alert.text));
        assert_eq!(ledger_kinds(&g, 1), vec![LedgerKind::Grant, LedgerKind::Escrow]);
        assert_eq!(ledger_kinds(&g, 2), vec![LedgerKind::Grant, LedgerKind::Sale]);
        let t = trades(&mut g);
        assert_eq!(
            (t[0].price, t[0].settlement.as_deref(), t[0].status.as_str()),
            (Some(30), Some("paid"), "completed")
        );
    }

    /// Alleles of a player living in a rival's lineage: the score breakdown
    /// shows where they live and whose genes each lineage carries, and the
    /// royalty pays for them.
    #[test]
    fn royalty_and_score_breakdown_follow_genes_into_rivals_lineages() {
        let (mut g, a, b) = game();
        // Two of Bob's animals carry Ann's genome (as if sired by her studs
        // for generations); they stay Bob's.
        let ann = g.world.organism(a[0]).unwrap().genome.clone();
        for &id in &b[..2] {
            g.world.organisms.iter_mut().find(|o| o.id == id).unwrap().genome = ann.clone();
        }
        g.refresh_hosts();
        let s1 = g.score_breakdown(1).unwrap();
        let s2 = g.score_breakdown(2).unwrap();
        // 6 + 6 founders, all breeding adults: Ann's alleles in 8, Bob's in 4.
        let close = |x: f64, y: f64| (x - y).abs() < 1e-9;
        assert!(close(s1.share, 8.0 / 12.0) && close(s1.head_share, 0.5) && close(s1.exported_share, 2.0 / 12.0));
        assert!(close(s1.home_share + s1.exported_share, s1.share));
        assert!(close(s2.share, 4.0 / 12.0) && close(s2.head_share, 0.5) && close(s2.foreign_share, 2.0 / 12.0));
        assert_eq!(s2.carried.iter().map(|c| c.player).collect::<Vec<_>>(), vec![2, 1]);
        assert!(close(s2.carried[1].share, 1.0 / 3.0));
        assert_eq!(s1.placed.len(), 2);
        assert!(close(s1.placed[1].share, 0.25) && s1.placed[1].owner == 2);
        assert_eq!(s1.royalty, economy::royalty(2.0 / 12.0, true));
        assert_eq!(s2.royalty, 0);
        // The royalty is paid with the daily income.
        let before = g.balance(1);
        g.pay_royalties(0);
        assert_eq!(g.balance(1), before + s1.royalty);
        assert_eq!(ledger_kinds(&g, 1).last(), Some(&LedgerKind::Royalty));
        assert_eq!(ledger_kinds(&g, 2), vec![LedgerKind::Grant]);
        // And it comes with the lineage statistics, with the crowding.
        let stats = g.lineage_stats(2);
        assert!(stats.score.is_some());
        let c = stats.crowding.unwrap();
        let cap = g.world.config.lineage_cap;
        let ceiling = (cap as f32 * g.world.config.balance.crowding_ceiling).round() as usize;
        assert_eq!((c.soft_cap, c.hard_cap), (cap, ceiling));
        assert!(c.litter > 0.99 && c.hunger == 1.0);
    }

    /// The renown shop: mating slots at doubling prices up to a limit, and
    /// reinforcements for a small lineage; renown spent is gone.
    #[test]
    fn the_renown_shop_sells_slots_and_reinforcements() {
        let (mut g, a, _) = game();
        let _ = g.post(1, 2000, LedgerKind::Income, "test".into(), None, None);
        let slots = g.world.mating_slots_left(1);
        let shop: ShopView = serde_json::from_value(data(&g.command(10, 1, None, ClientMsg::RenownShop))).unwrap();
        assert_eq!((shop.slot_price, shop.reinforce_price, shop.reinforce_blocked), (Some(30), 150, None));
        let mut spent = 0;
        for k in 0..economy::MAX_BOUGHT_SLOTS {
            let out = g.command(10, 1, None, ClientMsg::BuyMatingSlot);
            let v: ShopView = serde_json::from_value(data(&out)).unwrap();
            spent += economy::SLOT_BASE_PRICE << k;
            assert_eq!(
                (v.slots_left, v.slots_bought, v.balance),
                (slots + k + 1, k + 1, economy::FOUNDING_GRANT + 2000 - spent)
            );
        }
        let out = g.command(10, 1, None, ClientMsg::BuyMatingSlot);
        assert_eq!(result(&out).1.as_deref(), Some("limit_reached"));
        assert_eq!(ledger_kinds(&g, 1).last(), Some(&LedgerKind::Purchase));
        // Reinforcements: ten adults copied from the lineage's own.
        let alive = |g: &Game| g.world.organisms.iter().filter(|o| o.alive && o.owner == 1).count();
        let n = alive(&g);
        let out = g.command(10, 1, None, ClientMsg::Reinforce);
        let d = data(&out);
        assert_eq!(d["added"].as_array().unwrap().len(), economy::REINFORCE_SIZE as usize);
        assert_eq!(d["shop"]["reinforce_price"], 300);
        // The reply says where they landed, beside how many of the lineage.
        assert!(d["at"][0].as_f64().is_some() && d["at"][1].as_f64().is_some(), "{d}");
        assert!(d["group"].as_u64().is_some_and(|n| n > 0), "{d}");
        assert_eq!(alive(&g), n + economy::REINFORCE_SIZE as usize);
        let lineage = g.lineage_of(1).unwrap();
        assert!(g.world.organisms.iter().filter(|o| o.owner == 1).all(|o| o.lineage == lineage));
        // Not for a lineage past a fifth of the soft cap.
        g.world.config.lineage_cap = 50;
        let out = g.command(10, 1, None, ClientMsg::Reinforce);
        assert_eq!(result(&out).1.as_deref(), Some("too_large"));
        let shop: ShopView = serde_json::from_value(data(&g.command(10, 1, None, ClientMsg::RenownShop))).unwrap();
        assert!(shop.reinforce_blocked.unwrap().contains("at most 10"));
        // Bought slots end with the weather season.
        let _ = a;
        let season = g.world.season();
        while g.world.season() == season {
            g.world.step();
        }
        assert_eq!(g.world.bonus_slots(1), 0);
        // Broke: refused, nothing spent.
        let (mut g, _, _) = game();
        for _ in 0..2 {
            g.command(10, 1, None, ClientMsg::BuyMatingSlot);
        }
        assert_eq!(g.balance(1), economy::FOUNDING_GRANT - 30);
        assert_eq!(result(&g.command(10, 1, None, ClientMsg::Reinforce)).1.as_deref(), Some("insufficient_funds"));
    }

    /// Playtest 3: 0 of 21 cross-player matings completed, the pairs 54–110
    /// tiles apart. An accepted, paid agreement now brings its two animals
    /// together on its own (a rendezvous), the dam is held from natural
    /// mating meanwhile, both sides see where the pair is, and it completes
    /// and pays.
    #[test]
    fn a_paid_mating_between_animals_far_apart_completes() {
        let (mut g, a, b) = game();
        let (fa, mb) = (female(&g, &a), male(&g, &b));
        not_pregnant(&mut g, &[fa]);
        // Put Bob's stud across the map from Ann's dam, on land.
        let far = g.world.organisms.iter().filter(|o| o.alive).map(|o| o.pos).max_by(|x, y| {
            let fpos = g.world.organism(fa).unwrap().pos;
            x.dist(fpos).total_cmp(&y.dist(fpos))
        });
        g.world.organisms.iter_mut().find(|o| o.id == mb).unwrap().pos = far.unwrap();
        let start = g.world.organism(fa).unwrap().pos.dist(g.world.organism(mb).unwrap().pos);
        assert!(start > 20.0, "{start}");
        let id = offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: Some(20) }));
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        // Too far is not an error for the accepter: the agreement stands.
        assert_eq!(result(&out), (true, None));
        assert_eq!(g.world.rendezvous_of(fa), Some(mb));
        let meet = g.offer_view(id).meet.expect("escort info");
        assert!(meet.distance > 20.0);
        let (bal1, bal2) = (g.balance(1), g.balance(2));
        let mut done = false;
        for _ in 0..30 * g.world.config.ticks_per_day {
            g.world.step();
            let out = g.after_tick();
            if out.iter().any(|(_, m)| matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.id == id && offer.status == OfferStatus::Completed)) {
                done = true;
                break;
            }
            assert!(g.offers.contains_key(&id), "the agreement ended without the mating");
            // Held from natural mating while promised.
            assert!(g
                .world
                .organism(fa)
                .is_none_or(|o| o.pregnancy.is_none() || o.pregnancy.as_ref().unwrap().father_id == mb));
        }
        assert!(done, "the pair never met (started {start:.0} tiles apart)");
        let _ = (bal1, bal2);
        let t = trades(&mut g);
        let paid = t.iter().find(|t| t.offer == id).expect("trade recorded");
        assert_eq!((paid.status.as_str(), paid.settlement.as_deref()), ("completed", Some("paid")));
        assert!(g.ledger(2, 100).iter().any(|e| e.kind == LedgerKind::Sale && e.delta == 18), "paid less the fee");
        assert_eq!(g.world.rendezvous_of(fa), None, "the rendezvous ends with the agreement");
    }

    /// One accepted agreement per animal (playtest 4: one male held several
    /// and reached none); an agreement whose pair sleeps waits, deadline
    /// included; the pair meets on dry land.
    #[test]
    fn one_agreement_per_animal_and_sleep_pauses_the_deadline() {
        let (mut g, a, b) = game();
        let fem: Vec<OrgId> = b.iter().copied().filter(|&x| g.world.organism(x).unwrap().sex == Sex::Female).collect();
        not_pregnant(&mut g, &fem);
        let ma = male(&g, &a);
        // Apart, so the first agreement stays open while they walk.
        let far = g.world.map.clamp(g.world.organism(ma).unwrap().pos + Vec2::new(12.0, 0.0));
        g.world.organisms.iter_mut().find(|o| o.id == fem[0]).unwrap().pos = far;
        let id1 =
            offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: ma, partner: fem[0], price: None }));
        let id2 =
            offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: ma, partner: fem[1], price: None }));
        g.command(20, 2, None, ClientMsg::RespondMating { request: id1, accept: true, partner: None });
        assert_eq!(g.offers.get(&id1).map(|o| o.status), Some(OfferStatus::Accepted));
        {
            let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id2, accept: true, partner: None });
            assert_eq!(result(&out).1.as_deref(), Some("organism_busy"));
            assert_eq!(g.offers[&id2].status, OfferStatus::Pending);
            let m = g.offers[&id1].meet.expect("a meeting point");
            assert_eq!(g.world.map.tile_at(Vec2::new(m.0, m.1)).water_depth, 0.0, "on dry land");
            // Both asleep: the deadline moves on with the clock.
            for id in [ma, fem[0]] {
                g.world.organisms.iter_mut().find(|o| o.id == id).unwrap().dormancy = Dormancy::Winter;
            }
            let before = g.offer_deadline(&g.offers[&id1].clone());
            g.after_tick();
            g.after_tick();
            assert_eq!(g.offer_deadline(&g.offers[&id1].clone()), before + 2);
            assert!(g.offer_view(id1).meet.unwrap().asleep);
        }
    }

    /// A pregnant dam can't be accepted into an agreement (her owner is
    /// told why), shows as busy on the market, and a promised dam can't get
    /// a fixed listing.
    #[test]
    fn pregnant_and_promised_dams_are_busy() {
        let (mut g, a, b) = game();
        let (ma, fb) = (male(&g, &a), female(&g, &b));
        not_pregnant(&mut g, &[fb]);
        let id = offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: ma, partner: fb, price: None }));
        let father = g.world.organism(ma).unwrap().genome.clone();
        g.world.organisms.iter_mut().find(|o| o.id == fb).unwrap().pregnancy = Some(evo_sim::organism::Pregnancy {
            father,
            father_id: 0,
            father_lineage: 0,
            fertility: 1.0,
            due_tick: u64::MAX,
        });
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        assert_eq!(result(&out).1.as_deref(), Some("partner_unavailable"));
        assert!(err_message(&out).contains("pregnant"));
        assert_eq!(g.offers[&id].status, OfferStatus::Pending);
        // Listed while pregnant: busy.
        // Promised in an open offer: no fixed listing.
        let out = g.command(20, 2, None, ClientMsg::CreateListing { organism: fb, fee: 5, expires_days: None });
        assert_eq!(result(&out).1.as_deref(), Some("organism_busy"));
        g.command(10, 1, None, ClientMsg::CancelMating { request: id });
        let l = data(&g.command(20, 2, None, ClientMsg::CreateListing { organism: fb, fee: 5, expires_days: None }));
        assert_eq!(l["listing"]["busy"], true, "a pregnant dam is busy: {l}");
    }

    /// Reinforcements: at most two a ranked season per player, counted
    /// across a retire and re-found (the price doesn't reset either).
    #[test]
    fn reinforcements_are_capped_per_season_across_a_refound() {
        let (mut g, _, _) = game();
        let _ = g.post(1, 5000, LedgerKind::Income, "test".into(), None, None);
        for _ in 0..economy::MAX_REINFORCEMENTS_PER_SEASON {
            assert_eq!(result(&g.command(10, 1, None, ClientMsg::Reinforce)), (true, None));
        }
        assert_eq!(result(&g.command(10, 1, None, ClientMsg::Reinforce)).1.as_deref(), Some("limit_reached"));
        let l = g.lineage_of(1).unwrap();
        g.retire(1, l).unwrap();
        g.found(1, &design("A2")).unwrap();
        let shop: ShopView = serde_json::from_value(data(&g.command(10, 1, None, ClientMsg::RenownShop))).unwrap();
        assert_eq!(shop.reinforce_price, economy::reinforce_price(economy::MAX_REINFORCEMENTS_PER_SEASON));
        assert!(shop.reinforce_blocked.unwrap().contains("retired lineages count"));
        assert_eq!(result(&g.command(10, 1, None, ClientMsg::Reinforce)).1.as_deref(), Some("limit_reached"));
    }

    /// The reinforcement cap and price count purchases within a ranked
    /// season's length (rolling), not since the season began: a season
    /// boundary doesn't reset them (playtest 3: +40 adults in 2 minutes).
    #[test]
    fn reinforcements_count_in_a_rolling_window() {
        let (mut g, _, _) = game();
        g.cfg.season_days = 4;
        let _ = g.post(1, 5000, LedgerKind::Income, "test".into(), None, None);
        for _ in 0..economy::MAX_REINFORCEMENTS_PER_SEASON {
            assert_eq!(result(&g.command(10, 1, None, ClientMsg::Reinforce)), (true, None));
        }
        run_days(&mut g, 2);
        let out = g.command(10, 1, None, ClientMsg::Reinforce);
        assert_eq!(result(&out).1.as_deref(), Some("limit_reached"));
        assert!(format!("{out:?}").contains("the next is possible on day"), "{out:?}");
        let shop: ShopView = serde_json::from_value(data(&g.command(10, 1, None, ClientMsg::RenownShop))).unwrap();
        assert_eq!(shop.reinforce_price, economy::reinforce_price(2), "the price doesn't reset either");
        assert!(shop.reinforce_next_day.is_some() && shop.reinforce_next_minutes.is_some());
        run_days(&mut g, 3);
        let shop: ShopView = serde_json::from_value(data(&g.command(10, 1, None, ClientMsg::RenownShop))).unwrap();
        assert_eq!((shop.reinforce_price, shop.reinforcements_bought), (economy::REINFORCE_BASE_PRICE, 0));
        // (The lineage may have grown past the size limit; only the window is checked here.)
    }

    /// A provision feeds and waters a lineage's neediest adults, priced per
    /// adult fed (more for a large lineage), at most 3 within a season's
    /// length.
    #[test]
    fn a_provision_feeds_a_small_lineage() {
        let (mut g, _, _) = game();
        let _ = g.post(1, 5000, LedgerKind::Income, "test".into(), None, None);
        let l = g.lineage_of(1).unwrap();
        for o in g.world.organisms.iter_mut().filter(|o| o.lineage == l) {
            o.matured = true;
            o.needs.hunger = 80.0;
            o.needs.thirst = 70.0;
        }
        let before = g.balance(1);
        let d = data(&g.command(10, 1, None, ClientMsg::Provision));
        let fed = d["fed"].as_u64().unwrap() as usize;
        assert!(fed > 0 && fed <= economy::PROVISION_SIZE);
        assert_eq!(g.balance(1), before - economy::PROVISION_PER_ANIMAL * fed as u64, "priced per adult fed");
        let full = g.world.organisms.iter().filter(|o| o.lineage == l && o.needs.hunger <= 30.0 + 1e-3).count();
        assert_eq!(full, fed, "the fed are 50 less hungry");
        assert!(g.world.organisms.iter().filter(|o| o.lineage == l).all(|o| o.needs.thirst <= 70.0));
        for _ in 1..economy::MAX_PROVISIONS_PER_SEASON {
            assert_eq!(result(&g.command(10, 1, None, ClientMsg::Provision)), (true, None));
        }
        assert_eq!(result(&g.command(10, 1, None, ClientMsg::Provision)).1.as_deref(), Some("limit_reached"));
        let shop: ShopView = serde_json::from_value(data(&g.command(10, 1, None, ClientMsg::RenownShop))).unwrap();
        assert_eq!((shop.provisions_bought, shop.provisions_max), (3, 3));
        assert!(shop.provision_blocked.is_some());
        // A large lineage pays more per adult, and feeds more of them.
        g.economy.provision_days.clear();
        g.world.config.lineage_cap = 5;
        let shop: ShopView = serde_json::from_value(data(&g.command(10, 1, None, ClientMsg::RenownShop))).unwrap();
        let before = g.balance(1);
        let d = data(&g.command(10, 1, None, ClientMsg::Provision));
        let fed = d["fed"].as_u64().unwrap();
        assert_eq!(fed as usize, shop.provision_size);
        assert_eq!(g.balance(1), before - economy::PROVISION_PER_ANIMAL_LARGE * fed);
        assert_eq!(shop.provision_price, economy::PROVISION_PER_ANIMAL_LARGE * fed);
    }

    /// Any animal is listed or auctioned for at least [`LISTING_MIN_FEE`] (a
    /// free listing held it for nothing); a hold lasts at
    /// most [`HOLD_MAX_DAYS`] simulated days, whatever the clock.
    #[test]
    fn dam_listings_have_a_floor_and_holds_a_cap() {
        let (mut g, a, _) = game();
        let (fa, ma) = (female(&g, &a), male(&g, &a));
        not_pregnant(&mut g, &[fa]);
        let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: fa, fee: 0, expires_days: None });
        assert_eq!(result(&out).1.as_deref(), Some("bad_price"));
        let out =
            g.command(10, 1, None, ClientMsg::CreateAuction { organism: fa, reserve: 4, days: Some(2), ticks: None });
        assert_eq!(result(&out).1.as_deref(), Some("bad_price"));
        let out =
            g.command(10, 1, None, ClientMsg::CreateListing { organism: fa, fee: LISTING_MIN_FEE, expires_days: None });
        assert_eq!(result(&out), (true, None));
        let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: ma, fee: 1, expires_days: None });
        assert_eq!(result(&out).1.as_deref(), Some("bad_price"), "a stud too (playtest 5: a 1-renown stud-lock)");
        g.cfg.purchase_deadline_ticks = u64::MAX / 4;
        let o = Offer {
            id: 1,
            from: 2,
            to: 1,
            mine: 0,
            partner: 0,
            price: None,
            status: OfferStatus::Accepted,
            created_tick: 50,
            listing: None,
            escrow: 0,
            fee: 0,
            accepted_tick: Some(100),
            warned: false,
            lineage: None,
            meet: None,
            lost: None,
            excused: false,
            partner_slept: false,
            paused: 0,
        };
        let tpd = g.world.config.ticks_per_day as u64;
        assert_eq!(g.offer_deadline(&o), 100 + HOLD_MAX_DAYS * tpd);
    }

    /// Re-founding within a ranked season's length of retiring prices
    /// founders double: a design that fills the budget gets about half its
    /// founders, one with points to spare gets all it asks for, and the
    /// cost reported is what was founded; after the window, the full design.
    #[test]
    fn refounding_soon_after_retiring_costs_founders() {
        let (mut g, _, _) = game();
        g.cfg.season_days = 4;
        // A slow clock (a minute a tick): the real-time floor (30 minutes)
        // is 30 ticks, under a day.
        g.cfg.tick_ms = 60_000.0;
        assert_eq!(g.refound_window_days(), 4);
        let l = g.lineage_of(1).unwrap();
        g.retire(1, l).unwrap();
        let full = (2..200).take_while(|&n| design("A2").founders(n).point_cost() <= DESIGN_BUDGET).last().unwrap();
        let ok = g.found(1, &design("A2").founders(full)).unwrap();
        let n = ok.founders.len() as u32;
        assert!(n >= full / 2 && n < full, "{n} of {full}");
        assert_eq!(ok.requested, full);
        assert!(ok.reduced.as_deref().unwrap().contains("retired"), "{:?}", ok.reduced);
        assert!(ok.cost <= DESIGN_BUDGET + 1e-3 && ok.cost > DESIGN_BUDGET - 5.0, "{}", ok.cost);
        // The alert goes out live with the next tick.
        g.world.step();
        let out = g.after_tick();
        let said = format!("Re-founded: {n} of {full} founders");
        assert!(alerts_in(&out, 1).iter().any(|a| a.text.starts_with(&said)), "{out:?}");
        let l = g.lineage_of(1).unwrap();
        g.retire(1, l).unwrap();
        run_days(&mut g, 5);
        let ok = g.found(1, &design("A3").founders(10)).unwrap();
        assert_eq!(ok.founders.len(), 10, "long after the retire: the full design");
        assert_eq!(g.refound_founders(2, &design("B")), None, "Bob's lineage lives");
        // Dying out counts too (not only retiring); a cheap design gets all
        // its founders, charged double.
        wipe_out(&mut g, 2);
        run_days(&mut g, 1);
        let t = g.refound_terms(2).expect("re-founding after dying out");
        assert!(t.reason.contains("died out"), "{}", t.reason);
        let cheap = design("B2").founders(10);
        let ok = g.found(2, &cheap).unwrap();
        assert_eq!(ok.founders.len(), 10);
        assert!((ok.cost - cheap.point_cost() - evo_sim::design::founder_cost(10)).abs() < 1e-4);
    }

    /// The window is at least 30 real minutes: a 4-day season at 1 s a tick
    /// lasts 3 minutes, so the window stretches to about 38 days.
    #[test]
    fn the_refound_window_has_a_real_time_floor() {
        let (mut g, _, _) = game();
        g.cfg.season_days = 4;
        g.cfg.tick_ms = 1000.0;
        let tpd = g.world.config.ticks_per_day as u64;
        assert_eq!(g.refound_window_days(), (30 * 60u64).div_ceil(tpd));
    }

    /// Retiring hands animals mostly of a rival's genes to the rival, who is
    /// told; the rest leave the world.
    #[test]
    fn retiring_hands_rivals_their_animals() {
        let (mut g, a, b) = game();
        let bob = g.world.organism(b[0]).unwrap().genome.clone();
        for &id in &a[..2] {
            g.world.organisms.iter_mut().find(|o| o.id == id).unwrap().genome = bob.clone();
        }
        let before = g.world.owner_count(2);
        let l = g.lineage_of(1).unwrap();
        let out = g.command(10, 1, None, ClientMsg::RetireLineage { lineage: l });
        let d = data(&out);
        assert_eq!((d["removed"].as_u64(), d["transferred"].as_u64()), (Some(a.len() as u64 - 2), Some(2)));
        assert_eq!(g.world.owner_count(2), before + 2);
        assert_eq!(g.world.owner_count(1), 0);
        assert_eq!(g.lineage_stats(1).ended.as_deref(), Some("retired"));
        let bl = g.lineage_of(2).unwrap();
        assert!(a[..2].iter().all(|&id| g.world.organism(id).is_some_and(|o| o.owner == 2 && o.lineage == bl)));
        assert!(alerts_in(&out, 2).iter().any(|x| x.text.contains("joined yours")));
    }

    /// An offer to a lineage: the owner picks a free animal, or the server
    /// the nearest one; a pregnant female is never picked.
    #[test]
    fn offers_to_a_lineage_let_the_owner_pick_a_free_animal() {
        let (mut g, a, b) = game();
        let mine = male(&g, &a);
        let bl = g.lineage_of(2).unwrap();
        let females: Vec<OrgId> =
            b.iter().copied().filter(|&x| g.world.organism(x).unwrap().sex == Sex::Female).collect();
        not_pregnant(&mut g, &females);
        // The first female is pregnant (and nearest): she isn't picked.
        let pos = g.world.organism(mine).unwrap().pos;
        let preg = females[0];
        let fa = g.world.organism(a[0]).unwrap().genome.clone();
        {
            let o = g.world.organisms.iter_mut().find(|o| o.id == preg).unwrap();
            o.pos = pos;
            o.pregnancy = Some(evo_sim::organism::Pregnancy {
                father: fa,
                father_id: 0,
                father_lineage: 0,
                fertility: 1.0,
                due_tick: u64::MAX,
            });
        }
        let out = g.command(10, 1, None, ClientMsg::OfferToLineage { mine, lineage: bl, price: Some(10) });
        let id = offer_id(&out);
        let view = &data(&out)["offer"];
        assert_eq!((view["partner"].as_u64(), view["lineage"].as_u64()), (Some(0), Some(bl as u64)));
        assert_eq!(g.escrow(1), 10);
        // Picking the pregnant one is refused; the offer stays open.
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: Some(preg) });
        assert_eq!(result(&out).1.as_deref(), Some("partner_unavailable"));
        assert_eq!(g.offers[&id].status, OfferStatus::Pending);
        // It survives ticks while pending (no partner yet).
        g.world.step();
        g.after_tick();
        assert!(g.offers.contains_key(&id));
        // Accepting without a pick: the nearest free female.
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        let picked = g.offers.get(&id).map(|o| o.partner);
        let partner = picked.unwrap_or_else(|| *females.iter().find(|&&f| f != preg).unwrap());
        assert!(picked.is_none_or(|p| p != preg && p != 0 && females.contains(&p)), "{picked:?}");
        let _ = partner;
        // Not to your own lineage, nor a wild one.
        let al = g.lineage_of(1).unwrap();
        let out = g.command(10, 1, None, ClientMsg::OfferToLineage { mine, lineage: al, price: None });
        assert_eq!(result(&out).1.as_deref(), Some("own_organism"));
        // With nobody free, accepting says so and the offer stays.
        let (mut g, a, b) = game();
        let mine = male(&g, &a);
        let bl = g.lineage_of(2).unwrap();
        let id = offer_id(&g.command(10, 1, None, ClientMsg::OfferToLineage { mine, lineage: bl, price: None }));
        let fa = g.world.organism(a[0]).unwrap().genome.clone();
        for o in g.world.organisms.iter_mut().filter(|o| b.contains(&o.id) && o.sex == Sex::Female) {
            o.pregnancy = Some(evo_sim::organism::Pregnancy {
                father: fa.clone(),
                father_id: 0,
                father_lineage: 0,
                fertility: 1.0,
                due_tick: u64::MAX,
            });
        }
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        assert_eq!(result(&out).1.as_deref(), Some("no_partner_available"));
        assert_eq!(g.offers[&id].status, OfferStatus::Pending);
    }

    /// Cosmetic sinks: a nameplate (500, then 100 a change) and naming a
    /// species your lineage founded (300).
    #[test]
    fn nameplates_and_species_names_are_cosmetic_sinks() {
        let (mut g, _, _) = game();
        let _ = g.post(1, 1000, LedgerKind::Income, "test".into(), None, None);
        let start = g.balance(1);
        let out = g.command(10, 1, None, ClientMsg::SetNameplate { color: "plaid".into(), title: "x".into() });
        assert_eq!(result(&out).1.as_deref(), Some("bad_color"));
        let out = g.command(10, 1, None, ClientMsg::SetNameplate { color: "amber".into(), title: " ".into() });
        assert_eq!(result(&out).1.as_deref(), Some("bad_title"));
        // No bidi overrides, zero-width or other non-ASCII characters, no
        // markup, nothing passing for the server or another player.
        for bad in [
            "rtl\u{202e}evil",
            "zero\u{200b}width",
            "Ѕerver",
            "<b>Hi</b>",
            "a &lt; b",
            "Server Admin",
            "S3rv3r ADM1N",
            "Bob",
            "b0b",
            "two  spaces",
            "--",
            "Game Master",
            "GM",
            "Mod",
            "Owner",
            "Support",
            "Sysop",
            "wild",
            "bob (alt)",
        ] {
            let out = g.command(10, 1, None, ClientMsg::SetNameplate { color: "amber".into(), title: bad.into() });
            assert_eq!(result(&out).1.as_deref(), Some("bad_title"), "{bad:?}");
        }
        assert_eq!(g.balance(1), start, "nothing spent on a refused title");
        g.command(10, 1, None, ClientMsg::SetNameplate { color: "amber".into(), title: "Herd queen".into() });
        assert_eq!(g.balance(1), start - economy::NAMEPLATE_PRICE);
        g.command(10, 1, None, ClientMsg::SetNameplate { color: "jet".into(), title: "Herd king".into() });
        assert_eq!(g.balance(1), start - economy::NAMEPLATE_PRICE - economy::NAMEPLATE_CHANGE_PRICE);
        let d = data(&g.command(20, 2, None, ClientMsg::Nameplates));
        assert_eq!(d["nameplates"]["1"]["title"], "Herd king");
        // A species Ann founded (made by hand here): she can name it, Bob can't.
        run_days(&mut g, 10);
        let id = g.world.chronicle.species.species.len() as u32;
        let mut sp = g.world.chronicle.species.species.first().cloned().expect("a species");
        sp.id = id;
        sp.founder = Some(1);
        sp.extinct_day = None;
        sp.merged_into = None;
        let (la, lb) = (g.lineage_of(1).unwrap(), g.lineage_of(2).unwrap());
        sp.lineages = vec![(la, 30.0), (lb, 10.0)];
        g.world.chronicle.species.species.push(sp.clone());
        let out = g.command(20, 2, None, ClientMsg::NameSpecies { species: id, name: "Bobs".into() });
        assert_eq!(result(&out).1.as_deref(), Some("not_yours"));
        // A wild species most of whose adults are Bob's lineage: his to name.
        let mut wild = sp;
        wild.id = id + 1;
        wild.founder = None;
        wild.lineages = vec![(lb, 60.0), (0, 40.0)];
        g.world.chronicle.species.species.push(wild);
        let _ = g.post(2, 1000, LedgerKind::Income, "test".into(), None, None);
        let out = g.command(20, 2, None, ClientMsg::NameSpecies { species: id + 1, name: "Bobs".into() });
        assert_eq!(result(&out), (true, None));
        let before = g.balance(1);
        for bad in ["\u{202e}srezarg", "<i>Grazers</i>", "Official grazers", "bob"] {
            let out = g.command(10, 1, None, ClientMsg::NameSpecies { species: id, name: bad.into() });
            assert_eq!(result(&out).1.as_deref(), Some("bad_name"), "{bad:?}");
        }
        // A look-alike of another species' name is taken.
        let other = g.world.chronicle.species.species[0].name.clone();
        let look_alike = other.replace('o', "0").replace('l', "1");
        if look_alike != other && display_text_ok(&look_alike) {
            let out = g.command(10, 1, None, ClientMsg::NameSpecies { species: id, name: look_alike });
            assert_eq!(result(&out).1.as_deref(), Some("name_taken"));
        }
        // Its own name again is refused, not charged.
        let own = g.world.chronicle.species.get(id).unwrap().name.clone();
        let out = g.command(10, 1, None, ClientMsg::NameSpecies { species: id, name: own });
        assert_eq!(result(&out).1.as_deref(), Some("same_name"));
        assert_eq!(g.balance(1), before);
        let out = g.command(10, 1, None, ClientMsg::NameSpecies { species: id, name: "Queen's grazers".into() });
        assert_eq!(result(&out), (true, None));
        assert_eq!(g.balance(1), before - economy::SPECIES_NAME_PRICE);
        assert_eq!(g.world.chronicle.species.get(id).unwrap().name, "Queen's grazers");
        // Named once: shown as named, and not named (or charged) again.
        assert!(g.chronicle().species.species.iter().any(|v| v.id == id && v.named));
        let out = g.command(10, 1, None, ClientMsg::NameSpecies { species: id, name: "Queen's herd".into() });
        assert_eq!(result(&out).1.as_deref(), Some("already_named"));
        assert_eq!(g.balance(1), before - economy::SPECIES_NAME_PRICE);
    }

    /// Playtest 4: lineage names were unfiltered ("Server Admin" with a
    /// bidi override, public everywhere and the founding species' name).
    /// One filter now for every chosen name, and old ones are cleaned.
    #[test]
    fn lineage_names_go_through_the_name_filter_and_old_ones_are_cleaned() {
        let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
        let mut g = Game::new(world, GameConfig::default());
        g.set_name(1, "Herdmaster4");
        g.set_name(2, "bob");
        for bad in ["Server Admin", "rtl\u{202e}nimdA", "<b>Herd</b>", "Herdrnaster4"] {
            let r = g.found(2, &design(bad));
            assert!(matches!(r, Err(FoundError::BadName(_))), "{bad:?}: {:?}", r.map(|o| o.lineage));
        }
        let ok = g.found(2, &design("Bob's grazers")).unwrap();
        // A name saved before the filter is cleaned on a restore.
        g.world.lineages[ok.lineage as usize].name = "Grazers\u{202e} of\u{200b} Admin".into();
        let r = Game::resume(g.world.clone(), GameConfig::default(), g.saved(), Vec::new());
        assert_eq!(r.world.lineages[ok.lineage as usize].name, format!("Lineage {}", ok.lineage));
        g.world.lineages[ok.lineage as usize].name = "Grazers\u{202e} of\u{200b} the north".into();
        let r = Game::resume(g.world.clone(), GameConfig::default(), g.saved(), Vec::new());
        assert_eq!(r.world.lineages[ok.lineage as usize].name, "Grazers of the north");
    }

    /// A juvenile reads `not_mature`, not `same_sex` (playtest 5), in a
    /// preview and in a request within one's own lineage.
    #[test]
    fn maturity_is_checked_before_sex() {
        let (mut g, a, _) = game();
        let (m1, m2) = {
            let males: Vec<OrgId> =
                a.iter().copied().filter(|&id| g.world.organism(id).unwrap().sex == Sex::Male).collect();
            (males[0], males[1])
        };
        let i = g.world.organism_index(m2).unwrap();
        (g.world.organisms[i].matured, g.world.organisms[i].growth) = (false, 0.3);
        assert_eq!(g.preview_mating(1, m1, m2).unwrap_err().code, "not_mature");
        let out = g.command(10, 1, Some(1), ClientMsg::RequestMating { mine: m1, partner: m2, price: None });
        assert_eq!(result(&out).1.as_deref(), Some("not_mature"));
    }

    fn display_text_ok(t: &str) -> bool {
        crate::names::check(t, crate::design::MAX_NAME_LEN, crate::names::Kind::Species).is_ok()
    }

    /// The species a player's founding started can be named by that player
    /// (playtest 3: `not_yours` for one's own founding species).
    #[test]
    fn a_founding_species_can_be_named_by_its_owner() {
        let (mut g, _, _) = game();
        let _ = g.post(1, 1000, LedgerKind::Income, "test".into(), None, None);
        let _ = g.post(2, 1000, LedgerKind::Income, "test".into(), None, None);
        run_days(&mut g, 10);
        let ch = g.chronicle();
        let own = ch
            .species
            .species
            .iter()
            .find(|s| s.origin != "split" && s.lineages.first().is_some_and(|l| l.owner == 1))
            .expect("the species player 1's founding started");
        assert_eq!(own.namer, Some(1));
        let id = own.id;
        let out = g.command(20, 2, None, ClientMsg::NameSpecies { species: id, name: "Not mine".into() });
        assert_eq!(result(&out).1.as_deref(), Some("not_yours"));
        let out = g.command(10, 1, None, ClientMsg::NameSpecies { species: id, name: "Ann's runners".into() });
        assert_eq!(result(&out), (true, None));
        assert_eq!(g.chronicle().species.species.iter().find(|s| s.id == id).unwrap().name, "Ann's runners");
    }

    /// An initial species that holds a wild lineage can't be named by a
    /// player, even one whose lineage leads it (playtest 4: the leader
    /// renamed the Steppe antelope for everyone).
    #[test]
    fn initial_wild_species_cannot_be_renamed() {
        let world = build_world(&WorldSettings { seed: 5, width: 64, height: 64, ..Default::default() });
        let mut g = Game::new(world, GameConfig::default());
        g.set_name(1, "ann");
        let mine = g.found(1, &design("A")).unwrap().lineage;
        run_days(&mut g, 2);
        let wild = (0..g.world.lineages.len() as u32).find(|&l| g.world.lineages[l as usize].owner == WILD).unwrap();
        let mut sp = g.world.chronicle.species.species.first().cloned().expect("a species");
        (sp.origin, sp.founder, sp.extinct_day, sp.merged_into) =
            (evo_sim::species::SpeciesOrigin::Initial, None, None, None);
        sp.lineages = vec![(mine, 60.0), (wild, 40.0)];
        assert_eq!(g.species_namer(&sp), None);
        sp.lineages = vec![(mine, 60.0)];
        sp.origin = evo_sim::species::SpeciesOrigin::Founding;
        assert_eq!(g.species_namer(&sp), Some(1));
        // Once named, the namer stays whoever comes to lead it.
        g.economy.named_species.insert(sp.id, 1);
        sp.lineages = vec![(wild, 90.0), (mine, 10.0)];
        assert_eq!(g.species_namer(&sp), Some(1));
    }

    /// An auction lasts at least the wall-clock floor, whatever was asked.
    #[test]
    fn auctions_last_at_least_the_floor() {
        let (mut g, a, _) = game();
        let ma = male(&g, &a);
        let out =
            g.command(10, 1, None, ClientMsg::CreateAuction { organism: ma, reserve: 5, days: None, ticks: Some(1) });
        let l = &data(&out)["listing"];
        let deadline = l["expires_tick"].as_u64().unwrap();
        assert_eq!(deadline - g.world.tick(), g.cfg.min_auction_ticks);
    }

    /// Which lineages can breed with whom: same body plans are fertile, a
    /// diet 0.30 apart is sterile, in the list and on every listing.
    #[test]
    fn compatibility_shows_the_diet_split() {
        let (mut g, a, _) = game();
        let hunter = FounderDesign::new("C", Thermo::Warm, BirthType::Live, 2).founders(6).with(Gene::Carnivory, 0.85);
        let c = g.found(3, &hunter).unwrap().founders;
        g.set_name(3, "cat");
        let v: CompatibilityView =
            serde_json::from_value(data(&g.command(10, 1, None, ClientMsg::Compatibility))).unwrap();
        assert_eq!(v.lineage, g.lineage_of(1));
        assert_eq!(v.rows.len(), 2);
        assert!(v.rows[0].compatible && v.rows[0].owner == 2 && v.rows[0].fertility == 1.0);
        let cat = &v.rows[1];
        assert!(!cat.compatible && cat.owner == 3 && cat.limiting == "carnivory" && cat.gap > 0.6, "{cat:?}");
        // On listings: Cat's stud is sterile with Ann's lineage, Bob's isn't.
        let _ = a;
        let mc = male(&g, &c);
        g.command(30, 3, None, ClientMsg::CreateListing { organism: mc, fee: 5, expires_days: None });
        let m = g.market(1, &MarketQuery::default()).unwrap();
        assert_eq!((m.listings[0].compatible, m.listings[0].body_plan_fertility), (Some(false), Some(0.0)));
        let m = g.market(3, &MarketQuery { with_own: true, ..Default::default() }).unwrap();
        assert_eq!(m.own.unwrap()[0].compatible, Some(true));
    }

    /// The design-time check samples the design's founders against each
    /// lineage's adults: a near design gives a fertility range, a far one
    /// is sterile with no drift risk, an egg layer next to live bearers is
    /// partly fertile (`Balance::egg_hybrid`), and players are never wild.
    #[test]
    fn design_compatibility_samples_individuals() {
        let (g, _, _) = game();
        let same = FounderDesign::new("S", Thermo::Warm, BirthType::Live, 2).founders(6);
        let v = g.design_compatibility(9, &same);
        assert_eq!(v.sample, 16);
        assert_eq!(v.name_warning, None);
        // A name like a rival's ("B0b" for bob) is flagged before Found.
        let alike = FounderDesign::new("B0b", Thermo::Warm, BirthType::Live, 2).founders(6);
        assert!(g.design_compatibility(1, &alike).name_warning.is_some());
        assert!(v.rows.iter().all(|r| !r.wild && r.owner != WILD), "{:?}", v.rows);
        assert!(
            v.rows.iter().all(|r| r.compatible && r.fertile_share >= 0.9 && r.drift_risk == "high"),
            "{:?}",
            v.rows
        );
        assert!(v.rows.iter().all(|r| r.fertility_low <= r.fertility_high && r.fertility_high <= r.best_fertility));
        let eggs = FounderDesign::new("E", Thermo::Warm, BirthType::IncubatedEggs, 2).founders(6);
        let e = g.design_compatibility(9, &eggs);
        // Other players' lineages: tenths, and no limiting gene (varying one
        // gene against exact gaps solved their body plans).
        assert!(e.rows.iter().all(|r| r.limiting == "body_plan" && r.fertility < 0.6), "{:?}", e.rows);
        let tenths = |x: f32| ((x * 10.0).round() - x * 10.0).abs() < 1e-4;
        assert!(e.rows.iter().all(|r| tenths(r.gap) && tenths(r.fertility)), "{:?}", e.rows);
        // One's own lineage is shown exactly.
        let mine = g.design_compatibility(1, &eggs);
        let row = |o: PlayerId| mine.rows.iter().find(|r| r.owner == o).unwrap();
        assert_eq!(row(1).limiting, "birth_mode");
        assert_eq!(row(2).limiting, "body_plan");
        assert!(e.rows.iter().any(|r| r.fertile_share > 0.0), "{:?}", e.rows);
        let far = FounderDesign::new("F", Thermo::Cold, BirthType::AbandonedEggs, 2).founders(6);
        let f = g.design_compatibility(9, &far);
        assert!(f.rows.iter().all(|r| r.fertility == 0.0 && r.drift_risk == "none"), "{:?}", f.rows);
        assert_eq!((f.fertile_with, f.at_risk), (0, 0));
    }

    /// The design check warns a warm-blooded design that abandons its eggs
    /// (playtest 5: 252 chilled, none hatched), and no other.
    #[test]
    fn warm_abandoned_eggs_are_warned() {
        let (g, _, _) = game();
        let warm = FounderDesign::new("W", Thermo::Warm, BirthType::AbandonedEggs, 4).founders(6);
        let w = g.design_compatibility(9, &warm).egg_warning.expect("warned");
        assert!(w.contains("24 °C") && w.contains("incubated"), "{w}");
        for (t, b) in [(Thermo::Cold, BirthType::AbandonedEggs), (Thermo::Warm, BirthType::IncubatedEggs)] {
            let d = FounderDesign::new("O", t, b, 4).founders(6);
            assert_eq!(g.design_compatibility(9, &d).egg_warning, None);
        }
    }

    /// Warm and cold lineages of one plan are partly fertile
    /// (`Balance::thermo_hybrid`, playtest 5: cold blood walled the market
    /// off), the playtest 5 flier with a plain warm grazer too; off, sterile.
    #[test]
    fn warm_and_cold_lineages_are_partly_fertile() {
        let (mut g, _, _) = game();
        let cold = FounderDesign::new("C", Thermo::Cold, BirthType::Live, 2).founders(6);
        let flier = FounderDesign::new("Fl", Thermo::Cold, BirthType::Live, 3)
            .with(Gene::BodySize, 0.45)
            .with(Gene::Wings, 0.8)
            .with(Gene::Toxins, 0.8)
            .with(Gene::Coloration, 0.8)
            .with(Gene::Brumation, 0.6)
            .with(Gene::Scales, 0.4)
            .founders(10);
        for d in [&cold, &flier] {
            let v = g.design_compatibility(9, d);
            eprintln!(
                "{} vs warm live: {:?}",
                d.name,
                v.rows.iter().map(|r| (r.fertility, r.fertile_share, r.limiting.clone())).collect::<Vec<_>>()
            );
            assert!(v.rows.iter().all(|r| r.fertility > 0.0 && r.fertility < 0.6), "{:?}", v.rows);
        }
        g.world.config.balance.thermo_hybrid = 0.0;
        let v = g.design_compatibility(9, &cold);
        assert!(v.rows.iter().all(|r| r.fertility == 0.0), "{:?}", v.rows);
    }

    /// Two foundings of one design are always mutually fertile: founder
    /// variation never opens a sterile gap within a design, the client's
    /// egg-laying starter included (every pair of 16 founders × 16 adults).
    #[test]
    fn two_foundings_of_one_design_are_mutually_fertile() {
        use evo_sim::presets;
        let starter = FounderDesign::new("Starter", Thermo::Warm, BirthType::IncubatedEggs, 3)
            .with(Gene::BodySize, 0.5)
            .with(Gene::GroupTolerance, 0.9)
            .with(Gene::Boldness, 0.3)
            .with(Gene::Fur, 0.6)
            .with(Gene::FatStores, 0.6)
            .with(Gene::Hibernation, 0.5)
            .founders(20);
        let smoke = starter.clone().with(Gene::BodySize, 0.45).with(Gene::Fur, 0.4).with(Gene::Nose, 0.3);
        let mut designs = vec![starter, smoke, presets::migrant_design()];
        designs.extend(presets::player_designs());
        for (k, d) in designs.iter().enumerate() {
            for seed in [3, 11] {
                let world =
                    build_world(&WorldSettings { seed, width: 64, height: 64, wild: false, ..Default::default() });
                let mut g = Game::new(world, GameConfig::default());
                let first = g.found_at_any_price(1, d).unwrap().lineage;
                let v = g.design_compatibility(2, d);
                let row = v.rows.iter().find(|r| r.lineage == first).expect("the first founding");
                assert!(
                    row.fertile_share >= 1.0 && row.compatible && row.fertility > 0.5,
                    "design {k} ({}), seed {seed}: {row:?}",
                    d.name
                );
            }
        }
    }

    /// Playtest 3: a design that can't climb died of thirst under steep
    /// banks. The design check says how much shore it can reach and warns
    /// when it is under half.
    #[test]
    fn design_check_warns_when_banks_are_too_steep() {
        let (mut g, _, _) = game();
        let walker = FounderDesign::new("W", Thermo::Warm, BirthType::Live, 2).with(Gene::Climbing, 0.0).founders(6);
        let climber = walker.clone().with(Gene::Climbing, 0.6);
        let v = g.design_compatibility(9, &walker);
        assert!(v.shore_access.is_some_and(|s| s > 0.5) && v.water_warning.is_none(), "{v:?}");
        // Every bank steep.
        for g in &mut g.world.map.edit_terrain().ground {
            g.slope = 0.8;
        }
        let v = g.design_compatibility(9, &walker);
        assert_eq!(v.shore_access, Some(0.0));
        assert!(v.water_warning.as_deref().is_some_and(|w| w.contains("climbing")), "{v:?}");
        let c = g.design_compatibility(9, &climber);
        assert_eq!((c.shore_access, c.water_warning), (Some(1.0), None));
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
        // Many requests from Ann to Bob in a row: no per-target limit here.
        let mut g = Game::new(world, GameConfig { request_window_ticks: 0, ..GameConfig::default() });
        let a = g.found(1, &design("A")).unwrap().founders;
        let b = g.found(2, &design("B")).unwrap().founders;
        g.connect(10, 1, "ann");
        g.connect(20, 2, "bob");
        let (fa, mb) = (female(&g, &a), male(&g, &b));
        in_sight_of(&mut g, fa, mb);
        let start = g.balance(1);
        let ask = |g: &mut Game| {
            offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: Some(10) }))
        };
        // Declined.
        let id = ask(&mut g);
        assert_eq!(g.balance(1), start - 10);
        let out = g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: false, partner: None });
        assert_eq!(g.balance(1), start);
        assert!(alerts_in(&out, 1)
            .iter()
            .any(|x| x.kind == AlertKind::Market && x.text.contains("Refund: 10") && x.text.contains("declined")));
        // Cancelled by the requester, and by the partner's owner after accepting.
        let id = ask(&mut g);
        g.command(10, 1, None, ClientMsg::CancelMating { request: id });
        assert_eq!(g.balance(1), start);
        let id = ask(&mut g);
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        g.command(20, 2, None, ClientMsg::CancelMating { request: id });
        assert_eq!((g.balance(1), g.balance(2)), (start, start), "bob is paid nothing");
        // Died on the way.
        let id = ask(&mut g);
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        let o = g.world.organisms.iter_mut().find(|o| o.id == mb).unwrap();
        o.alive = false;
        o.death = Some(DeathCause::Predation);
        g.after_tick();
        assert_eq!(g.balance(1), start);
        // Free requests take nothing and settle nothing.
        let (ma, fb) = (male(&g, &a), female(&g, &b));
        let home = g.world.organism(fb).unwrap().pos;
        in_sight_of(&mut g, ma, fb);
        let id = offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: ma, partner: fb, price: Some(0) }));
        assert_eq!(g.offers[&id].escrow, 0);
        g.command(10, 1, None, ClientMsg::CancelMating { request: id });
        // An accepted agreement whose two never meet lapses at its
        // deadline (the requester forfeits 25%, at least 5 renown).
        let id = offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: ma, partner: fb, price: Some(10) }));
        // Back out of reach: they mustn't meet before the deadline.
        g.world.organisms.iter_mut().find(|o| o.id == fb).unwrap().pos = home;
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        assert_eq!(g.balance(1), start - 10);
        g.cfg.purchase_deadline_ticks = 3;
        for _ in 0..4 {
            g.world.step();
            g.after_tick();
        }
        assert!(!g.offers.contains_key(&id));
        assert_eq!(g.escrow(1), 0);
        let refunds = g.ledger(1, 100).iter().filter(|e| e.kind == LedgerKind::Refund).count();
        assert_eq!(refunds, 5);
        let income: u64 =
            g.ledger(1, 100).iter().filter(|e| e.kind == LedgerKind::Income).map(|e| e.delta as u64).sum();
        assert_eq!(g.balance(1), start + income - 5, "every hold came back but the lapse fee");
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
        let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: ma, fee: 5, expires_days: None });
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
        // The listing stays open, but the stud walks to one meeting at a
        // time: busy until this one is done.
        let m: MarketView =
            serde_json::from_value(data(&g.command(20, 2, None, ClientMsg::BrowseMarket(MarketQuery::default()))))
                .unwrap();
        assert!(m.listings[0].busy);
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing: listing.id, mine: fb });
        assert_eq!(result(&out).1.as_deref(), Some("listing_busy"));
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
        // Less the market fee (10% of 12, rounded down).
        assert_eq!((g.balance(1), g.balance(2), g.escrow(2)), (start + 11, start - 12, 0));
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
        let l = data(&g.command(10, 1, None, ClientMsg::CreateListing { organism: fa, fee: 5, expires_days: None }))
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
        assert_eq!(g.offers[&id].escrow, 5, "the fee is held");
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
            assert!(alerts_in(&out, 1).iter().any(|x| x.text.contains("was eaten")));
        }
        // Too many listings.
        let ids: Vec<OrgId> =
            g.world.organisms.iter().filter(|o| o.alive && o.owner == 1 && o.matured).map(|o| o.id).collect();
        let mut refused = None;
        for id in ids {
            let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: id, fee: 5, expires_days: None });
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
                fee = (fee * 7 + 3) % 31 + 5;
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

    /// The indexed market (`game::market`) against the scan: after a tick
    /// (the index is then usable), random pages in every order, with and
    /// without filters, cursors and previews, while listings, auctions and
    /// bids come and go. `Game::market` compares both on every page.
    #[test]
    fn indexed_market_pages_match_the_scan() {
        use rand::{Rng, SeedableRng};
        let (mut g, all) = market_game();
        g.world.step();
        let _ = g.after_tick();
        assert!(g.market.usable(&g.economy, g.world.tick()));
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let sorts = [
            MarketSort::Newest,
            MarketSort::PriceAsc,
            MarketSort::PriceDesc,
            MarketSort::Ending,
            MarketSort::Fertility,
        ];
        let before = market::INDEXED_PAGES.with(|n| n.get());
        let mut scanned_too = 0;
        for round in 0..300 {
            let p = rng.gen_range(1..=4u32);
            let conn = p as u64 * 10;
            let mine: Vec<OrgId> =
                all[p as usize - 1].iter().copied().filter(|&id| g.world.organism(id).is_some()).collect();
            match rng.gen_range(0..6) {
                // Withdraw one of one's listings (not an auction with a bid).
                0 => {
                    let ids: Vec<u64> = g.market.owned_by(p).collect();
                    if let Some(&id) = ids.get(rng.gen_range(0..ids.len().max(1))) {
                        let _ = g.command(conn, p, None, ClientMsg::WithdrawListing { listing: id });
                    }
                }
                // List or auction an organism.
                1 => {
                    if let Some(&id) = mine.get(rng.gen_range(0..mine.len().max(1))) {
                        let msg = if rng.gen_bool(0.5) {
                            ClientMsg::CreateListing { organism: id, fee: rng.gen_range(0..30), expires_days: None }
                        } else {
                            ClientMsg::CreateAuction {
                                organism: id,
                                reserve: rng.gen_range(0..30),
                                days: None,
                                ticks: Some(50),
                            }
                        };
                        let _ = g.command(conn, p, None, msg);
                    }
                }
                // Bid on someone's auction.
                2 => {
                    let auctions: Vec<(u64, u64)> = g
                        .economy
                        .listings
                        .values()
                        .filter(|l| l.owner != p)
                        .filter_map(|l| Some((l.id, l.auction.as_ref()?.min_bid())))
                        .collect();
                    if let (Some(&(l, min)), Some(&m)) =
                        (auctions.get(rng.gen_range(0..auctions.len().max(1))), mine.first())
                    {
                        let _ = g.command(conn, p, None, ClientMsg::Bid { listing: l, amount: min, mine: m });
                    }
                }
                _ => {}
            }
            let q = MarketQuery {
                mine: if rng.gen_bool(0.5) { mine.first().copied() } else { None },
                limit: Some(rng.gen_range(1..9)),
                sort: Some(sorts[rng.gen_range(0..sorts.len())]),
                kind: [None, Some(ListingKind::Fixed), Some(ListingKind::Auction)][rng.gen_range(0..3)],
                role: [None, Some(ListingRole::Stud), Some(ListingRole::Dam)][rng.gen_range(0..3)],
                hide_own: rng.gen_bool(0.5),
                with_own: rng.gen_bool(0.3),
                ..Default::default()
            };
            scanned_too += usize::from(q.mine.is_some() && q.sort == Some(MarketSort::Fertility));
            // Every page, following the cursor (each compared with the scan).
            let (pages, _) = all_pages(&mut g, conn, p, q);
            assert!(pages.len() <= g.economy.listings.len());
            if round % 50 == 49 {
                g.world.step();
                let _ = g.after_tick();
            }
        }
        let indexed = market::INDEXED_PAGES.with(|n| n.get()) - before;
        assert!(indexed > 300 && scanned_too > 20, "{indexed} indexed, {scanned_too} queries scanned");
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
        let relisted = ClientMsg::CreateListing { organism, fee: 5, expires_days: None };
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
        // Auctions of a few ticks (the server's floor is minutes).
        g.cfg.min_auction_ticks = 1;
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
            g.command(20, 2, None, ClientMsg::CreateAuction { organism: ma, reserve: 5, days: None, ticks: None });
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
            data(&g.command(10, 1, None, ClientMsg::CreateListing { organism: fa, fee: 5, expires_days: None }))
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
        assert_eq!((g.balance(1), g.balance(3), g.escrow(3)), (b1 + 18, b3, 0), "less the market fee");
        let t = trades(&mut g);
        assert_eq!((t[0].listing, t[0].price, t[0].settlement.as_deref()), (Some(id), Some(20), Some("paid")));
    }

    #[test]
    fn auctions_without_a_bid_return_to_the_seller_and_deaths_refund_the_bid() {
        let (mut g, a, b) = game();
        // Auctions of a few ticks (the server's floor is minutes).
        g.cfg.min_auction_ticks = 1;
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
        // Auctions of a few ticks (the server's floor is minutes).
        g.cfg.min_auction_ticks = 1;
        let (ma, fb) = (male(&g, &a), female(&g, &b));
        not_pregnant(&mut g, &[fb]);
        let l = data(&g.command(
            10,
            1,
            None,
            ClientMsg::CreateAuction { organism: ma, reserve: 5, days: None, ticks: Some(3) },
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
        assert_eq!((a.reserve, a.bids, a.high.map(|b| (b.bidder, b.amount, b.mine))), (5, 1, Some((2, 6, fb))));
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
                ClientMsg::CreateAuction { organism: fa, reserve: 5, days: None, ticks: Some(5) },
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
            data(&g.command(10, 1, None, ClientMsg::CreateAuction { organism, reserve: 5, days: Some(3), ticks: None }))
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
        assert_eq!(r.balance(1), economy::FOUNDING_GRANT + 6, "less the market fee (at least 1)");
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

    /// A dam in an open auction is locked: no request, acceptance, purchase,
    /// bid or own directed mating may promise her until the auction closes
    /// or is withdrawn (before, her owner could promise her elsewhere and the
    /// winner's agreement then failed).
    #[test]
    fn a_dam_in_an_open_auction_is_locked() {
        let (mut g, a, b) = game();
        let dam = female(&g, &a);
        let own_male = male(&g, &a);
        let stud = male(&g, &b);
        place_together(&mut g, dam, own_male);
        let out = g.command(
            10,
            1,
            Some(1),
            ClientMsg::CreateAuction { organism: dam, reserve: 5, days: Some(2), ticks: None },
        );
        assert!(result(&out).0, "{out:?}");
        let listing = g.economy.listing_of(dam).unwrap().id;
        let locked = |out: &Outbox| result(out) == (false, Some("in_auction".to_string()));
        // Another player asks for her; her owner promises her to someone else
        // or mates her directly.
        assert!(locked(&g.command(20, 2, Some(2), ClientMsg::RequestMating { mine: stud, partner: dam, price: None })));
        assert!(locked(&g.command(10, 1, Some(3), ClientMsg::RequestMating { mine: dam, partner: stud, price: None })));
        assert!(locked(&g.command(
            10,
            1,
            Some(4),
            ClientMsg::RequestMating { mine: dam, partner: own_male, price: None }
        )));
        assert!(g.world.organism(dam).unwrap().pregnancy.is_none());
        // Bidding on the auction itself is still open.
        g.economy.balances.insert(2, 100);
        let out = g.command(20, 2, Some(5), ClientMsg::Bid { listing, amount: 5, mine: stud });
        assert!(result(&out).0, "{out:?}");
        // A stud in an auction isn't locked: he may serve several.
        let out = g.command(
            20,
            2,
            Some(6),
            ClientMsg::CreateAuction { organism: stud, reserve: 5, days: Some(2), ticks: None },
        );
        assert!(!locked(&out));
    }

    /// A bought listing whose buyer never brings their organism lapses at
    /// the deadline: the dam is freed, the seller keeps a quarter of the fee
    /// and the rest is refunded. (Free dam listings are refused: a buyer
    /// would hold her for nothing.) (Buying a rival's dam and never coming held her until
    /// the season ended.)
    #[test]
    fn a_bought_listing_lapses_when_the_buyer_never_comes() {
        // A dam is listed for at least [`LISTING_MIN_FEE`]; the hold fee is a
        // quarter of it, at least [`LAPSE_FEE_MIN`] (so all of a cheap one).
        for fee in [20u64, LISTING_MIN_FEE] {
            let (mut g, a, b) = game();
            g.cfg.purchase_deadline_ticks = 5;
            let (dam, stud) = (female(&g, &a), male(&g, &b));
            let out = g.command(10, 1, Some(1), ClientMsg::CreateListing { organism: dam, fee, expires_days: None });
            assert!(result(&out).0, "{out:?}");
            let listing = g.economy.listing_of(dam).unwrap().id;
            g.economy.balances.insert(1, 0);
            g.economy.balances.insert(2, 100);
            let slots = g.world.mating_slots_left(2);
            let out = g.command(20, 2, Some(2), ClientMsg::BuyListing { listing, mine: stud });
            assert!(result(&out).0, "{out:?}");
            let id = *g.offers.keys().next().unwrap();
            assert_eq!(g.offers[&id].status, OfferStatus::Accepted, "apart, it waits");
            // Keep them apart until the deadline.
            let mut lapsed = Vec::new();
            for _ in 0..8 {
                g.world.step();
                lapsed.extend(g.after_tick());
            }
            assert!(!g.offers.contains_key(&id), "the purchase never lapsed (fee {fee})");
            let told = lapsed
                .iter()
                .any(|(_, m)| matches!(m, ServerMsg::MatingUpdate { error: Some(e), .. } if e.code == "lapsed"));
            assert!(told, "the parties hear why");
            let entry = |p: PlayerId, kind: LedgerKind| {
                g.ledger(p, 50).into_iter().find(|e| e.kind == kind && e.offer == Some(id)).map(|e| e.delta)
            };
            assert_eq!(entry(1, LedgerKind::Sale), Some(5), "a quarter of the fee (at least 5) to the seller");
            assert_eq!(entry(2, LedgerKind::Refund), (fee > 5).then_some(fee as i64 - 5), "the rest refunded");
            assert_eq!(g.world.mating_slots_left(2), slots);
            // The dam is free to breed again once the listing is gone.
            g.command(10, 1, Some(3), ClientMsg::WithdrawListing { listing });
            for _ in 0..3 {
                g.world.step();
                g.after_tick();
            }
            let t = g.world.tick();
            assert!(
                g.world.organism(dam).unwrap().breed_ready_tick <= t + 1
                    || g.world.organism(dam).unwrap().pregnancy.is_some()
            );
        }
    }

    /// A buyer whose animal dies on the way may send another before the
    /// deadline; if none comes, the hold fee is forfeit unless the loss was
    /// outside their control (eaten). Playtest 5: the buyer's animal dying
    /// refunded everything, a free dam-lock, and failed every purchase.
    #[test]
    fn a_buyer_whose_animal_dies_may_send_another_or_pays_the_hold() {
        for (cause, excused) in [(DeathCause::Thirst, false), (DeathCause::Predation, true)] {
            let (mut g, a, b) = game();
            g.cfg.purchase_deadline_ticks = 30;
            let dam = female(&g, &a);
            let studs: Vec<OrgId> =
                b.iter().copied().filter(|&i| g.world.organism(i).unwrap().sex == Sex::Male).take(2).collect();
            not_pregnant(&mut g, &[dam]);
            let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: dam, fee: 20, expires_days: None });
            let listing = data(&out)["listing"]["id"].as_u64().unwrap();
            g.economy.balances.insert(2, 100);
            // Far apart: they mustn't meet in the test.
            g.world.organisms.iter_mut().find(|o| o.id == dam).unwrap().pos = Vec2::new(4.0, 4.0);
            for &s in &studs {
                g.world.organisms.iter_mut().find(|o| o.id == s).unwrap().pos = Vec2::new(60.0, 60.0);
            }
            let out = g.command(20, 2, None, ClientMsg::BuyListing { listing, mine: studs[0] });
            assert!(result(&out).0, "{out:?}");
            let id = *g.offers.keys().next().unwrap();
            let die = |g: &mut Game, s: OrgId| {
                let o = g.world.organisms.iter_mut().find(|o| o.id == s).unwrap();
                o.alive = false;
                o.death = Some(cause);
            };
            die(&mut g, studs[0]);
            g.world.step();
            let out = g.after_tick();
            assert!(g.offers.contains_key(&id), "the agreement waits for another animal");
            assert!(g.offers[&id].lost.is_some());
            assert!(alerts_in(&out, 2).iter().any(|x| x.text.contains("send another animal")), "{out:?}");
            assert!(g.offer_view(id).lost.is_some());
            // Bob sends his second male.
            let out = g.command(20, 2, None, ClientMsg::SubstituteMating { request: id, mine: studs[1] });
            assert!(result(&out).0, "{out:?}");
            assert_eq!((g.offers[&id].mine, g.offers[&id].lost.as_deref()), (studs[1], None));
            let out = g.command(20, 2, None, ClientMsg::SubstituteMating { request: id, mine: studs[1] });
            assert_eq!(result(&out).1.as_deref(), Some("not_substitutable"), "only for a lost animal");
            // It dies too, and nobody else comes: the agreement lapses.
            die(&mut g, studs[1]);
            for _ in 0..40 {
                g.world.step();
                g.after_tick();
            }
            assert!(!g.offers.contains_key(&id));
            let entry = |p: PlayerId, kind: LedgerKind| {
                g.ledger(p, 50).into_iter().find(|e| e.kind == kind && e.offer == Some(id)).map(|e| e.delta)
            };
            if excused {
                assert_eq!(
                    (entry(1, LedgerKind::Sale), entry(2, LedgerKind::Refund)),
                    (None, Some(20)),
                    "eaten: no fault"
                );
            } else {
                assert_eq!(
                    (entry(1, LedgerKind::Sale), entry(2, LedgerKind::Refund)),
                    (Some(5), Some(15)),
                    "{cause:?}"
                );
            }
        }
    }

    /// A pending request naming a listed animal doesn't make it busy (a
    /// free request blocked a listed dam for its 10 minutes); the deadline
    /// waits while either animal sleeps, and a lapse doesn't charge the
    /// buyer for the seller's animal sleeping.
    #[test]
    fn pending_requests_dont_block_listings_and_one_sleeper_pauses_the_deadline() {
        let (mut g, a, b) = game();
        let (dam, stud) = (female(&g, &a), male(&g, &b));
        not_pregnant(&mut g, &[dam]);
        let out = g.command(10, 1, None, ClientMsg::CreateListing { organism: dam, fee: 20, expires_days: None });
        let listing = data(&out)["listing"]["id"].as_u64().unwrap();
        in_sight_of(&mut g, stud, dam);
        let out = g.command(20, 2, None, ClientMsg::RequestMating { mine: stud, partner: dam, price: Some(0) });
        assert!(result(&out).0, "{out:?}");
        let l = g.economy.listings[&listing].clone();
        assert!(!g.listing_busy(&l), "a pending request holds nothing");
        for id in g.offers.keys().copied().collect::<Vec<_>>() {
            g.command(20, 2, None, ClientMsg::CancelMating { request: id });
        }
        // A purchase, the seller's dam asleep and the buyer's stud awake.
        g.cfg.purchase_deadline_ticks = 5;
        g.economy.balances.insert(2, 100);
        g.world.organisms.iter_mut().find(|o| o.id == dam).unwrap().pos = Vec2::new(4.0, 4.0);
        g.world.organisms.iter_mut().find(|o| o.id == stud).unwrap().pos = Vec2::new(60.0, 60.0);
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing, mine: stud });
        assert!(result(&out).0, "{out:?}");
        let id = *g.offers.keys().next().unwrap();
        let before = g.offer_deadline(&g.offers[&id]);
        for _ in 0..3 {
            g.world.step();
            g.world.organisms.iter_mut().find(|o| o.id == dam).unwrap().dormancy = Dormancy::Winter;
            g.after_tick();
        }
        assert!(g.offer_deadline(&g.offers[&id]) >= before + 3, "the deadline waited");
        assert!(g.offers[&id].partner_slept);
        g.world.organisms.iter_mut().find(|o| o.id == dam).unwrap().dormancy = Dormancy::Awake;
        for _ in 0..12 {
            g.world.organisms.iter_mut().find(|o| o.id == dam).unwrap().pos = Vec2::new(4.0, 4.0);
            g.world.organisms.iter_mut().find(|o| o.id == stud).unwrap().pos = Vec2::new(60.0, 60.0);
            g.world.step();
            g.after_tick();
        }
        assert!(!g.offers.contains_key(&id), "it lapsed");
        let refund =
            g.ledger(2, 50).into_iter().find(|e| e.kind == LedgerKind::Refund && e.offer == Some(id)).map(|e| e.delta);
        assert_eq!(refund, Some(20), "the buyer pays nothing for the seller's dam sleeping");
    }

    /// Cancelling a bought dam before the deadline isn't a free hold: the
    /// buyer forfeits a quarter of the fee; the seller cancelling
    /// refunds the buyer in full.
    #[test]
    fn a_buyer_cancelling_an_accepted_purchase_pays_for_the_hold() {
        for (fee, by_buyer) in [(20u64, true), (20, false)] {
            let (mut g, a, b) = game();
            let (dam, stud) = (female(&g, &a), male(&g, &b));
            g.command(10, 1, Some(1), ClientMsg::CreateListing { organism: dam, fee, expires_days: None });
            let listing = g.economy.listing_of(dam).unwrap().id;
            g.economy.balances.insert(1, 0);
            g.economy.balances.insert(2, 100);
            let slots = g.world.mating_slots_left(2);
            let out = g.command(20, 2, Some(2), ClientMsg::BuyListing { listing, mine: stud });
            assert!(result(&out).0, "{out:?}");
            let id = *g.offers.keys().next().unwrap();
            assert_eq!(g.offers[&id].status, OfferStatus::Accepted);
            let out = if by_buyer {
                g.command(20, 2, Some(3), ClientMsg::CancelMating { request: id })
            } else {
                g.command(10, 1, Some(3), ClientMsg::CancelMating { request: id })
            };
            assert!(result(&out).0, "{out:?}");
            let entry = |g: &Game, p: PlayerId, kind: LedgerKind| {
                g.ledger(p, 50).into_iter().find(|e| e.kind == kind && e.offer == Some(id)).map(|e| e.delta)
            };
            match (fee, by_buyer) {
                (20, true) => {
                    assert_eq!(entry(&g, 1, LedgerKind::Sale), Some(5), "a quarter to the seller");
                    assert_eq!(entry(&g, 2, LedgerKind::Refund), Some(15));
                    assert_eq!(g.world.mating_slots_left(2), slots);
                }
                _ => {
                    assert_eq!(entry(&g, 1, LedgerKind::Sale), None);
                    assert_eq!(entry(&g, 2, LedgerKind::Refund), Some(20), "the seller backing out refunds all");
                }
            }
        }
    }

    /// Request spam: a player can block another or decline every request,
    /// and one player may send another at most [`REQUESTS_PER_TARGET`]
    /// requests in the window. A request to a juvenile says `not_mature`.
    #[test]
    fn mating_requests_can_be_blocked_declined_and_are_limited_per_target() {
        let (mut g, a, b) = game();
        let (dam, stud) = (female(&g, &a), male(&g, &b));
        let code = |out: &Outbox| result(out).1;
        let ask = |g: &mut Game, id| {
            g.command(20, 2, Some(id), ClientMsg::RequestMating { mine: stud, partner: dam, price: None })
        };
        let cancel_all = |g: &mut Game| {
            for id in g.offers.keys().copied().collect::<Vec<_>>() {
                g.command(20, 2, None, ClientMsg::CancelMating { request: id });
            }
        };
        // Ann blocks Bob.
        let out = g.command(
            10,
            1,
            Some(1),
            ClientMsg::MatingPrefs { block: Some(2), unblock: None, auto_decline: None, blocked: None },
        );
        assert_eq!(reply_data(&out)["blocked"], serde_json::json!([2]));
        assert_eq!(code(&ask(&mut g, 2)), Some("requests_refused".into()));
        // Not the wild, nor a player who doesn't exist.
        for p in [WILD, 999] {
            let out = g.command(
                10,
                1,
                Some(1),
                ClientMsg::MatingPrefs { block: Some(p), unblock: None, auto_decline: None, blocked: None },
            );
            assert_eq!(code(&out), Some("no_such_player".into()), "{p}");
        }
        // Nor oneself; a whole list names its bad entries and changes nothing.
        let me = ClientMsg::MatingPrefs { block: Some(1), unblock: None, auto_decline: None, blocked: None };
        assert_eq!(code(&g.command(10, 1, Some(1), me)), Some("self_block".into()));
        let list =
            ClientMsg::MatingPrefs { block: None, unblock: None, auto_decline: None, blocked: Some(vec![0, 999, 2]) };
        let out = g.command(10, 1, Some(1), list);
        assert_eq!(code(&out), Some("no_such_player".into()));
        assert!(result_error(&out).unwrap().message.contains("0, 999"), "{out:?}");
        let list = ClientMsg::MatingPrefs { block: None, unblock: None, auto_decline: None, blocked: Some(vec![2, 2]) };
        assert_eq!(reply_data(&g.command(10, 1, Some(1), list))["blocked"], serde_json::json!([2]));
        assert_eq!(g.mating_prefs[&1].blocked, vec![2]);
        g.command(
            10,
            1,
            Some(3),
            ClientMsg::MatingPrefs { block: None, unblock: Some(2), auto_decline: Some(true), blocked: None },
        );
        assert_eq!(code(&ask(&mut g, 4)), Some("requests_refused".into()), "auto-decline");
        let out = g.command(
            10,
            1,
            Some(5),
            ClientMsg::MatingPrefs { block: None, unblock: None, auto_decline: Some(false), blocked: None },
        );
        assert_eq!(reply_data(&out), serde_json::json!({ "blocked": [], "auto_decline": false }));
        // Three open requests; the fourth is refused.
        for k in 0..REQUESTS_PER_TARGET as u64 {
            assert_eq!(code(&ask(&mut g, 10 + k)), None, "request {k}");
        }
        assert_eq!(code(&ask(&mut g, 20)), Some("too_many_requests".into()));
        // A restart doesn't reset the limit (playtest 3), nor the attack
        // alerts' clocks.
        g.attack_alerts.insert((0, 1), g.world.tick());
        let saved: SavedGame = serde_json::from_str(&serde_json::to_string(&g.saved()).unwrap()).unwrap();
        let mut r = Game::resume(g.world.clone(), g.cfg.clone(), saved, Vec::new());
        r.connect(20, 2, "bob");
        assert_eq!(code(&ask(&mut r, 20)), Some("too_many_requests".into()));
        assert_eq!(r.attack_alerts, g.attack_alerts);
        // Requests that came to nothing (cancelled, expired) or were accepted
        // don't count: trading isn't capped by the limiter.
        cancel_all(&mut g);
        assert_eq!(code(&ask(&mut g, 19)), None);
        cancel_all(&mut g);
        for k in 0..REQUESTS_PER_TARGET as u64 {
            assert_eq!(code(&ask(&mut g, 30 + k)), None, "request {k}");
        }
        assert_eq!(code(&ask(&mut g, 33)), Some("too_many_requests".into()));
        // The window passes.
        g.cfg.request_window_ticks = 1;
        g.world.step();
        g.after_tick();
        assert_eq!(code(&ask(&mut g, 21)), None);
        cancel_all(&mut g);
        // The settings survive a restart.
        g.command(
            10,
            1,
            Some(6),
            ClientMsg::MatingPrefs { block: Some(2), unblock: None, auto_decline: None, blocked: None },
        );
        let mut r = Game::resume(g.world.clone(), GameConfig::default(), g.saved(), Vec::new());
        r.connect(20, 2, "bob");
        let out = r.command(20, 2, Some(7), ClientMsg::RequestMating { mine: stud, partner: dam, price: None });
        assert_eq!(code(&out), Some("requests_refused".into()));
        // A juvenile partner: not mature.
        let (mut g, a, b) = game();
        let (dam, stud) = (female(&g, &a), male(&g, &b));
        let i = g.world.organism_index(dam).unwrap();
        (g.world.organisms[i].matured, g.world.organisms[i].growth) = (false, 0.3);
        let out = g.command(20, 2, Some(8), ClientMsg::RequestMating { mine: stud, partner: dam, price: None });
        assert_eq!(code(&out), Some("not_mature".into()));
        let own = male(&g, &a);
        let out = g.command(10, 1, Some(9), ClientMsg::RequestMating { mine: own, partner: dam, price: None });
        assert_eq!(code(&out), Some("not_mature".into()), "own pair");
    }

    /// Info leaks: the map image shows a player only what they may see,
    /// `set_view` only looks where their organisms are, listings give the
    /// region rather than the live position, and a rival's pregnancy is
    /// refused generically.
    #[test]
    fn rivals_hidden_state_and_positions_do_not_leak() {
        let (mut g, a, b) = game();
        // Ann's lineage in one corner, Bob's in the other.
        for o in g.world.organisms.iter_mut().filter(|o| o.alive) {
            o.pos = if o.owner == 1 { Vec2::new(4.5, 4.5) } else { Vec2::new(59.5, 59.5) };
        }
        let bob_color = evo_sim::render::owner_color(2, g.world.organism(b[0]).unwrap().traits.carnivory);
        let count = |rgb: &[u8]| rgb.chunks(3).filter(|c| c == &bob_color).count();
        let (_, _, ann_map) = g.map_rgb(1, 1);
        let (_, _, bob_map) = g.map_rgb(2, 1);
        assert_eq!(count(&ann_map), 0, "Ann's map shows Bob's animals");
        assert!(count(&bob_map) > 0, "Bob sees his own");
        // set_view: near one's own organisms, not across the map.
        let far = g.command(10, 1, Some(1), ClientMsg::SetView { x: 59.0, y: 59.0, radius: 16.0 });
        assert_eq!(result(&far).1, Some("out_of_sight".into()));
        let near = g.command(10, 1, Some(2), ClientMsg::SetView { x: 10.0, y: 10.0, radius: 16.0 });
        assert!(result(&near).0, "{near:?}");
        for r in [0.0, -5.0] {
            let bad = g.command(10, 1, Some(2), ClientMsg::SetView { x: 10.0, y: 10.0, radius: r });
            assert_eq!(result(&bad).1, Some("bad_coordinates".into()), "radius {r}");
        }
        // A rival's animal out of sight reads as not found (not "not yours"),
        // as an id that doesn't exist: no alive/dead oracle from afar.
        let far = b[0];
        assert_eq!(
            result(&g.command(10, 1, Some(2), ClientMsg::Possess { organism: far })).1,
            Some("not_found".into())
        );
        let split = ClientMsg::SplitGroup { group: 0, organisms: Some(vec![far]) };
        assert_eq!(result(&g.command(10, 1, Some(2), split)).1, Some("not_found".into()));
        let i = g.world.organism_index(far).unwrap();
        let back = g.world.organisms[i].pos;
        g.world.organisms[i].pos = Vec2::new(6.5, 4.5);
        assert_eq!(
            result(&g.command(10, 1, Some(2), ClientMsg::Possess { organism: far })).1,
            Some("not_owned".into())
        );
        g.world.organisms[i].pos = back;
        // And no wider than Ann's animals perceive plus the margin: a wide
        // view is narrowed, one beyond reach refused (32 tiles away was fine
        // before, with a 32-tile radius on top).
        let sight =
            (g.world.organisms.iter().filter(|o| o.alive && o.owner == 1)).map(|o| g.sight(o)).fold(0.0, f32::max);
        assert!(sight < 10.0, "{sight}");
        let wide = g.command(10, 1, Some(2), ClientMsg::SetView { x: 10.0, y: 10.0, radius: 32.0 });
        let r = reply_data(&wide)["radius"].as_f64().unwrap() as f32;
        assert!(r + 5.5 <= sight + VIEW_MARGIN + 1e-3 && r >= 1.0, "radius {r}, sight {sight}");
        let edge = g.command(10, 1, Some(2), ClientMsg::SetView { x: 4.5 + 30.0, y: 4.5, radius: 32.0 });
        assert_eq!(result(&edge).1, Some("out_of_sight".into()));
        assert!(!g.in_sight(1, Vec2::new(4.5 + sight + VIEW_MARGIN + 1.0, 4.5)));
        // A listing gives the 16-tile region, not the live position.
        let dam = female(&g, &a);
        let i = g.world.organism_index(dam).unwrap();
        g.world.organisms[i].pos = Vec2::new(3.3, 13.7);
        let out = g.command(10, 1, Some(3), ClientMsg::CreateListing { organism: dam, fee: 5, expires_days: None });
        let l = &reply_data(&out)["listing"];
        assert_eq!((l["x"].as_f64(), l["y"].as_f64()), (Some(8.0), Some(8.0)));
        // A preview needs the partner in sight, on the market or in an
        // offer; the distance to one out of sight is rounded up to 16 tiles.
        let stud = male(&g, &b);
        assert_eq!(g.preview_mating(1, dam, stud).unwrap_err().code, "out_of_sight");
        let out = g.command(20, 2, Some(9), ClientMsg::CreateListing { organism: stud, fee: 5, expires_days: None });
        assert!(result(&out).0, "{out:?}");
        let p = g.preview_mating(1, dam, stud).unwrap();
        assert_eq!(p.distance % 16.0, 0.0, "{}", p.distance);
        let tenths = |x: f32| ((x * 10.0).round() - x * 10.0).abs() < 1e-4;
        assert!(p.body_plan_gaps.iter().all(|g| tenths(g.gap)), "{:?}", p.body_plan_gaps);
        let listing = reply_data(&out)["listing"]["id"].as_u64().unwrap();
        g.command(20, 2, Some(10), ClientMsg::WithdrawListing { listing });
        assert_eq!(g.preview_mating(1, dam, stud).unwrap_err().code, "out_of_sight");
        // Once in sight, exact.
        let i = g.world.organism_index(stud).unwrap();
        g.world.organisms[i].pos = Vec2::new(6.5, 6.5);
        assert!(g.preview_mating(1, dam, stud).is_ok());
        g.world.organisms[i].pos = Vec2::new(59.5, 59.5);
        // A rival's pregnant female: a generic refusal; one's own: the reason.
        let bob_dam = female(&g, &b);
        let ann_male = male(&g, &a);
        place_together(&mut g, bob_dam, stud);
        for k in 0..4 {
            if g.world.organism(bob_dam).unwrap().pregnancy.is_none() {
                g.command(20, 2, Some(40 + k), ClientMsg::RequestMating { mine: stud, partner: bob_dam, price: None });
            }
        }
        assert!(g.world.organism(bob_dam).unwrap().pregnancy.is_some(), "the test needs her pregnant");
        // Out of sight, Ann can't even name her.
        let out = g.command(10, 1, Some(4), ClientMsg::RequestMating { mine: ann_male, partner: bob_dam, price: None });
        assert_eq!(result(&out).1, Some("out_of_sight".into()));
        let out = g.command(10, 1, Some(4), ClientMsg::RequestMating { mine: ann_male, partner: 999_999, price: None });
        assert_eq!(result(&out).1, Some("out_of_sight".into()), "nor tell whether an id exists");
        assert_eq!(g.preview_mating(1, ann_male, 999_999).unwrap_err().code, "out_of_sight");
        in_sight_of(&mut g, bob_dam, ann_male);
        let out = g.command(10, 1, Some(4), ClientMsg::RequestMating { mine: ann_male, partner: bob_dam, price: None });
        assert_eq!(result(&out).1, Some("partner_unavailable".into()));
        // An open offer that fails on her pregnancy tells only Bob why.
        let id = g.open_offer(1, 2, ann_male, bob_dam, None, OfferStatus::Pending, None).unwrap();
        let mut out = Outbox::new();
        g.close(id, OfferStatus::Failed, None, Some(ErrorInfo::from(&CommandError::AlreadyPregnant)), &mut out);
        let told = |p: PlayerId| {
            out.iter()
                .find_map(|(t, m)| match (t, m) {
                    (Target::Player(q), ServerMsg::MatingUpdate { error: Some(e), .. }) if *q == p => {
                        Some(e.code.clone())
                    }
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!((told(1).as_str(), told(2).as_str()), ("partner_unavailable", "already_pregnant"));
        let ann_stud_for_bob = male(&g, &a);
        let out = g.command(
            20,
            2,
            Some(5),
            ClientMsg::RequestMating { mine: bob_dam, partner: ann_stud_for_bob, price: None },
        );
        assert_eq!(result(&out).1, Some("already_pregnant".into()), "her owner hears why");
    }

    /// "Species founded" can't be farmed: a lineage split off, merged back
    /// and split off again pays once; and a lineage pays at most once a year.
    #[test]
    fn species_founded_bonus_is_not_paid_twice_for_one_lineage() {
        use evo_sim::species::{expr_of, DetectParams, Sample, SpeciesEvent, SpeciesTracker};
        let samples = |size: f32, n: usize, lineage: u32, owner: PlayerId, seed: u64| {
            let d = FounderDesign::new("x", Thermo::Warm, BirthType::Live, 4).with(Gene::BodySize, size);
            let mut rng = evo_sim::rng::rng_for(seed, 0, 0, 0);
            (0..n)
                .map(|_| {
                    let g = d.founder_genome(owner, &mut rng);
                    let mut origins = BTreeMap::new();
                    g.tally_origins(1.0, &mut origins);
                    Sample { expr: expr_of(&g), weight: 1.0, lineage, owner, origins }
                })
                .collect::<Vec<_>>()
        };
        let mut cfg = evo_sim::WorldConfig::default();
        cfg.scoring.species_extinct_after = 1;
        // The tracker of worlds before snapshot version 42 (no hysteresis):
        // what this guards against.
        cfg.scoring.species_confirm = 0;
        cfg.scoring.species_merge_confirm = 0;
        cfg.scoring.species_revive_days = 0;
        // Owners clustered together, so a player's lineage can bridge to a
        // wild one (`Balance::species_credit` off).
        cfg.balance.species_credit = 0.0;
        let p = DetectParams::from_config(&cfg);
        let mut t = SpeciesTracker::default();
        let name = |l: u32| format!("L{l}");
        // Wild kind at body size 0.3 (lineage 0), the player's (lineage 1)
        // either bridged to it (one species) or drifted apart at 0.6.
        let joined = |seed| {
            let mut v = samples(0.3, 30, 0, WILD, seed);
            v.extend(samples(0.45, 10, 1, 1, seed + 1));
            v.extend(samples(0.6, 20, 1, 1, seed + 2));
            v
        };
        let apart = |seed| {
            let mut v = samples(0.3, 30, 0, WILD, seed);
            v.extend(samples(0.6, 20, 1, 1, seed + 2));
            v
        };
        let founded = |ev: Vec<SpeciesEvent>| {
            ev.into_iter().find_map(|e| match e {
                SpeciesEvent::Appeared { species, founder: Some(1), .. } => Some(species),
                _ => None,
            })
        };
        let mut paid = BTreeMap::new();
        t.detect(0, &joined(1), &p, name);
        let first = founded(t.detect(5, &apart(10), &p, name)).expect("a split credited to player 1");
        assert!(species_bonus_due(&t, &mut paid, first, 1, 5, 120), "the first speciation pays");
        t.detect(10, &joined(20), &p, name);
        assert!(t.species[first as usize].merged_into.is_some(), "merged back");
        // Long after the cooldown, the same lineage splits off again.
        let again = founded(t.detect(500, &apart(30), &p, name)).expect("detected again");
        assert_ne!(again, first);
        assert!(!species_bonus_due(&t, &mut paid, again, 1, 500, 120), "a re-detection doesn't pay");
        // ...nor raise an alert or count; the first one does, and the
        // flicker copy is left out of the species list.
        assert!(species_redetected(&t, again, 1) && !species_redetected(&t, first, 1));
        assert!(!species_listed(&t.species[first as usize], 30), "merged back within a season");
        assert!(species_listed(&t.species[again as usize], 30));
        assert!(species_listed(&t.species[0], 30));
        // The cooldown alone: a different species id of the lineage, within a year.
        let mut paid = BTreeMap::new();
        assert!(species_bonus_due(&t, &mut paid, first, 1, 5, 120));
        let mut fresh = SpeciesTracker::default();
        fresh.detect(0, &joined(1), &p, name);
        let s2 = founded(fresh.detect(50, &apart(10), &p, name)).unwrap();
        assert!(!species_bonus_due(&fresh, &mut paid, s2, 1, 50, 120), "within the year");
        assert!(species_bonus_due(&fresh, &mut paid, s2, 1, 200, 120), "after it");
    }

    /// One day convention: `day` fields count from 0, player-facing text
    /// from 1. Income is paid as a day starts, for the day just ended, so its
    /// text names that day (the client shows the paying day, `day` + 1).
    #[test]
    fn ledger_texts_name_their_entry_day() {
        let (mut g, _, _) = game();
        run_days(&mut g, 3);
        let incomes: Vec<LedgerEntry> = g.ledger(1, 50).into_iter().filter(|e| e.kind == LedgerKind::Income).collect();
        assert!(!incomes.is_empty());
        for e in incomes {
            // Dated the day it is for (paid as the next one starts): the text names the entry's day.
            assert!(e.text.starts_with(&format!("Day {} income", e.day + 1)), "{} vs day {}", e.text, e.day);
        }
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
            ClientMsg::RespondMating { request: id, accept: true, partner: None },
            ClientMsg::CompleteMating { request: id },
            ClientMsg::CancelMating { request: id },
        ] {
            assert_eq!(code(&g.command(30, 3, None, msg)).as_deref(), Some("unknown_offer"));
        }
        let out = g.command(10, 1, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        assert_eq!(code(&out).as_deref(), Some("unknown_offer"));
        // Nor use someone else's organism as "mine" (out of sight it reads
        // as not found, as an unknown id would: `Game::hide_unseen`).
        let out = g.command(30, 3, None, ClientMsg::RequestMating { mine: fa, partner: c[0], price: None });
        let seen = g.view_reach(3, g.world.organism(fa).unwrap().pos) + (NAME_MARGIN - VIEW_MARGIN) >= 0.0;
        assert_eq!(code(&out).as_deref(), Some(if seen { "not_owned" } else { "not_found" }));
        // Cancel, then cancel or complete again: refunded exactly once.
        g.command(10, 1, None, ClientMsg::CancelMating { request: id });
        assert_eq!(g.balance(1), start);
        for msg in [ClientMsg::CancelMating { request: id }, ClientMsg::CompleteMating { request: id }] {
            assert_eq!(code(&g.command(10, 1, None, msg)).as_deref(), Some("unknown_offer"));
        }
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        assert_eq!((g.balance(1), g.balance(2)), (start, start));
        assert_eq!(ledger_kinds(&g, 1), vec![LedgerKind::Grant, LedgerKind::Escrow, LedgerKind::Refund]);
        // Completed (paid), then cancelled or declined: nothing moves again.
        let id = offer_id(&g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: Some(20) }));
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
        place_together(&mut g, fa, mb);
        g.command(10, 1, None, ClientMsg::CompleteMating { request: id });
        assert_eq!((g.balance(1), g.balance(2)), (start - 20, start + 18));
        g.command(20, 2, None, ClientMsg::CancelMating { request: id });
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: false, partner: None });
        g.after_tick();
        assert_eq!((g.balance(1), g.balance(2)), (start - 20, start + 18));

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
        assert_eq!((g.offers.len(), g.balance(2)), (open, start + 18));
        // Negative numbers never parse; oversized prices parse saturated, so
        // the range check answers `bad_price` (playtest 4).
        for big in [
            r#"{"type":"request_mating","mine":1,"partner":2,"price":1e30}"#,
            r#"{"type":"request_mating","mine":1,"partner":2,"price":18446744073709551616}"#,
        ] {
            let m = serde_json::from_str::<crate::protocol::ClientEnvelope>(big).unwrap();
            assert!(matches!(m.msg, ClientMsg::RequestMating { price: Some(u64::MAX), .. }), "{big}");
        }
        for bad in [
            r#"{"type":"request_mating","mine":1,"partner":2,"price":-5}"#,
            r#"{"type":"create_listing","organism":1,"fee":-1}"#,
            r#"{"type":"ledger","limit":-1}"#,
        ] {
            assert!(serde_json::from_str::<crate::protocol::ClientEnvelope>(bad).is_err(), "{bad}");
        }

        // Listings: only the owner lists or withdraws; a listed dam takes one
        // buyer at a time; one's own listing can't be bought.
        let out = g.command(30, 3, None, ClientMsg::CreateListing { organism: fb, fee: 5, expires_days: None });
        let seen = g.view_reach(3, g.world.organism(fb).unwrap().pos) + (NAME_MARGIN - VIEW_MARGIN) >= 0.0;
        assert_eq!(code(&out).as_deref(), Some(if seen { "not_owned" } else { "not_found" }));
        let l = data(&g.command(20, 2, None, ClientMsg::CreateListing { organism: fb, fee: 5, expires_days: None }))
            ["listing"]["id"]
            .as_u64()
            .unwrap();
        let out = g.command(30, 3, None, ClientMsg::WithdrawListing { listing: l });
        assert_eq!(code(&out).as_deref(), Some("unknown_listing"));
        let out = g.command(20, 2, None, ClientMsg::BuyListing { listing: l, mine: mb });
        assert_eq!(code(&out).as_deref(), Some("own_listing"));
        let out = g.command(30, 3, None, ClientMsg::BuyListing { listing: l, mine: ma });
        // (Out of sight it reads as not found: `Game::hide_unseen`.)
        assert!(
            matches!(code(&out).as_deref(), Some("not_owned" | "not_found")),
            "buying with someone else's organism"
        );
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
        g.command(20, 2, None, ClientMsg::RespondMating { request: id, accept: true, partner: None });
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
                if let Some(vision) = &mut v.vision {
                    vision.directional = g.world.config.balance.directional_vision > 0.0;
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
            // Those within OWN_NEAR of one of the player's own first, then nearest the center.
            let own: Vec<Vec2> = alive().filter(|o| o.owner == player).map(|o| o.pos).collect();
            let near = |n: &NearbyOrganism| own.iter().any(|q| q.dist(Vec2::new(n.x, n.y)) <= states::OWN_NEAR);
            nearby.sort_by(|a, b| near(&b.1).cmp(&near(&a.1)).then(a.0.total_cmp(&b.0)));
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
            groups: g.group_views_for(player, &g.world.groups(player)),
            view,
            organisms: own,
            nearby: {
                let mut v: Vec<NearbyOrganism> = nearby.into_iter().map(|(_, n)| n).collect();
                g.mark_protected(&mut v);
                v
            },
            leaderboard: g.leaderboard(),
            offers,
            balance: g.balance(player),
            escrow: g.escrow(player),
            grace: g.grace_view(player),
            poisonous: (0..g.world.lineages.len() as u32).filter(|&l| g.world.looks_poisonous_to(player, l)).collect(),
        }
    }

    /// A crowded world: the wild ecosystem and twelve players, each with a
    /// connection, some views pinned (one at the map's edge, one off it),
    /// one possessed organism and an offer.
    fn crowded(cfg: GameConfig) -> Game {
        // Seed 4: one where player 6's view in the middle of the map holds
        // foreign organisms (seed 3 lost them with dry founding sites,
        // version 44).
        let world = build_world(&WorldSettings { seed: 4, width: 96, height: 96, wild: true, ..Default::default() });
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
        // One sale isn't a stud market (it needs 2 sales to 2 different buyers).
        assert!(market.rows.is_empty(), "{:?}", market.rows);
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

    /// World-thread cost of `browse_market` with many listings, for
    /// docs/load-test.md (`cargo test --release -p evo-server --lib
    /// game::tests::market_cost -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn market_cost() {
        let players = std::env::var("EVO_MARKET_PLAYERS").ok().and_then(|p| p.parse().ok()).unwrap_or(2000u32);
        let world = build_world(&WorldSettings { seed: 5, width: 512, height: 512, wild: false, ..Default::default() });
        let mut g = Game::new(world, GameConfig::default());
        for p in 1..=players {
            g.set_name(p, &format!("p{p}"));
            let ids = g.found(p, &design(&format!("L{p}"))).unwrap().founders;
            for &id in ids.iter().take(2) {
                let _ = g.command(
                    p as u64,
                    p,
                    None,
                    ClientMsg::CreateListing { organism: id, fee: p as u64 % 20, expires_days: Some(10) },
                );
            }
        }
        // A tick validates the market index (`EVO_MARKET_SCAN` measures the
        // scan instead).
        if std::env::var("EVO_MARKET_SCAN").is_err() {
            g.world.step();
            let _ = g.after_tick();
        }
        market::CHECK_PAGES.with(|c| c.set(false));
        let listings = g.economy.listings.len();
        let mut times = Vec::new();
        for round in 0..400u32 {
            let p = 1 + round * 7 % players;
            let mine = g.world.organisms.iter().find(|o| o.alive && o.owner == p).map(|o| o.id);
            let q: MarketQuery = serde_json::from_value(serde_json::json!({"mine": mine, "hide_own": true})).unwrap();
            let t = std::time::Instant::now();
            let v = g.market(p, &q).unwrap();
            times.push(t.elapsed().as_secs_f64() * 1e6);
            assert_eq!(v.listings.len(), 20);
        }
        times.sort_by(f64::total_cmp);
        eprintln!("{listings} listings: browse_market {:.0} µs (median), p90 {:.0} µs", times[200], times[360]);
    }

    /// World-thread cost of a connection after a restart (every lineage
    /// unattended), for docs/load-test.md (`cargo test --release -p
    /// evo-server --lib game::tests::connect_cost -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn connect_cost() {
        let players = std::env::var("EVO_CONNECT_PLAYERS").ok().and_then(|p| p.parse().ok()).unwrap_or(1000u32);
        let world = build_world(&WorldSettings { seed: 5, width: 512, height: 512, wild: true, ..Default::default() });
        let mut g = Game::new(world, GameConfig::default());
        for p in 1..=players {
            g.set_name(p, &format!("p{p}"));
            g.found(p, &design(&format!("L{p}"))).unwrap();
        }
        g.world.run_days(1);
        let _ = g.after_tick();
        let saved = g.saved();
        let world = g.world.clone();
        let mut g = Game::resume(world, GameConfig::default(), saved, Vec::new());
        // As the runner runs it.
        g.defer_offline();
        let t = std::time::Instant::now();
        for p in 1..=players {
            let _ = g.connect(p as u64, p, &format!("p{p}"));
        }
        let per = t.elapsed().as_secs_f64() * 1e6 / players as f64;
        eprintln!("{} organisms, {players} players reconnecting: {per:.0} µs per connect", g.world.alive_count());
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
        // A denser world with `EVO_SUBSTEP_DAYS` (default 3: 8.3k organisms).
        let days = std::env::var("EVO_SUBSTEP_DAYS").ok().and_then(|d| d.parse().ok()).unwrap_or(3);
        g.world.run_days(days);
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
