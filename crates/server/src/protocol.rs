//! JSON wire types shared by the HTTP API and the WebSocket protocol.
//!
//! Every WebSocket message is a JSON object with a `type` field in
//! `snake_case`. Client messages may carry an `id`, echoed in the matching
//! [`ServerMsg::Result`] so a client can pair replies with requests.

use crate::appearance::Appearance;
use evo_sim::organism::{Action, Organism, Sex};
use evo_sim::terrain::Season;
use evo_sim::PlayerId;
use serde::{Deserialize, Serialize};

pub type OrgId = evo_sim::organism::OrgId;

/// A client → server WebSocket message.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ClientEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub msg: ClientMsg,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Take direct control of one of your organisms.
    Possess {
        organism: OrgId,
    },
    /// Walk a possessed organism toward a point (tile units).
    OrderMove {
        organism: OrgId,
        x: f32,
        y: f32,
    },
    /// Return a possessed organism to its genetic behavior.
    Release {
        organism: OrgId,
    },
    /// Propose a directed mating between your organism `mine` and `partner`.
    /// Pairs within your own lineage run at once; another player's organism
    /// needs that player's consent first. A `price` (renown) is taken from
    /// your balance into escrow at once and paid to the partner's owner when
    /// the mating runs, refunded if it doesn't (`insufficient_funds` if you
    /// can't cover it).
    RequestMating {
        mine: OrgId,
        partner: OrgId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        price: Option<u64>,
    },
    /// What a directed mating of your `mine` with `partner` would give (any
    /// living organism of the other sex): expected fertility, override cost
    /// and failure chance, and why the pair can't conceive if it can't. Data:
    /// [`MatingPreviewView`]. Nothing changes and no slot is used.
    PreviewMating {
        mine: OrgId,
        partner: OrgId,
    },
    /// The partner organism's owner accepts or declines an offer.
    RespondMating {
        request: u64,
        accept: bool,
    },
    /// Retry an accepted agreement now (it also completes on its own once the
    /// two organisms meet).
    CompleteMating {
        request: u64,
    },
    /// Either side withdraws an open offer or agreement.
    CancelMating {
        request: u64,
    },
    /// Group command: your adults drift toward a point.
    Migrate {
        x: f32,
        y: f32,
    },
    ClearMigration,
    /// Pin the view window (interest area) to a point.
    SetView {
        x: f32,
        y: f32,
        radius: f32,
    },
    /// Let the view window follow your possessed organism or your lineage.
    ClearView,
    /// Offer one of your adults on the stud marketplace: anyone may buy one
    /// mating with it for `fee` renown (consent given in advance). Expires
    /// after `expires_days` simulated days if set. Data: `{listing}`
    /// ([`ListingView`]).
    CreateListing {
        organism: OrgId,
        fee: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_days: Option<u32>,
    },
    /// Take one of your listings off the marketplace (agreements already
    /// bought from it stand).
    WithdrawListing {
        listing: u64,
    },
    /// Buy one mating with a listed organism for your adult `mine`: an
    /// already accepted agreement, the fee held in escrow until the two meet,
    /// your seasonal slot spent when it runs. Data: `{offer}`.
    BuyListing {
        listing: u64,
        mine: OrgId,
    },
    /// The open listings, newest first, each with the compatibility of your
    /// `mine` with the listed organism if given. Data: [`MarketView`].
    BrowseMarket {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mine: Option<OrgId>,
    },
    /// Your balance and recent ledger entries (newest first, at most `limit`,
    /// default 50). Data: [`LedgerView`].
    Ledger {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
    },
    /// Protocol 2 only: the client lost track of the delta states; the
    /// connection's next state is a keyframe. Handled by the connection (no
    /// `result`), never by the world.
    Resync,
}

impl ClientMsg {
    pub fn name(&self) -> &'static str {
        match self {
            ClientMsg::Possess { .. } => "possess",
            ClientMsg::OrderMove { .. } => "order_move",
            ClientMsg::Release { .. } => "release",
            ClientMsg::RequestMating { .. } => "request_mating",
            ClientMsg::PreviewMating { .. } => "preview_mating",
            ClientMsg::RespondMating { .. } => "respond_mating",
            ClientMsg::CompleteMating { .. } => "complete_mating",
            ClientMsg::CancelMating { .. } => "cancel_mating",
            ClientMsg::Migrate { .. } => "migrate",
            ClientMsg::ClearMigration => "clear_migration",
            ClientMsg::SetView { .. } => "set_view",
            ClientMsg::ClearView => "clear_view",
            ClientMsg::CreateListing { .. } => "create_listing",
            ClientMsg::WithdrawListing { .. } => "withdraw_listing",
            ClientMsg::BuyListing { .. } => "buy_listing",
            ClientMsg::BrowseMarket { .. } => "browse_market",
            ClientMsg::Ledger { .. } => "ledger",
            ClientMsg::Resync => "resync",
        }
    }
}

/// A server → client WebSocket message.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        player: PlayerId,
        name: String,
        lineage: Option<u32>,
        /// The protocol this connection speaks (`wire::PROTO_DELTA` when the
        /// client asked for it with `/ws?proto=2`; absent for protocol 1).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        proto: Option<u32>,
    },
    State(Box<StateView>),
    /// The outcome of one client command.
    Result {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<u64>,
        command: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
    /// Another player asks to mate their organism with yours.
    MatingRequest {
        offer: OfferView,
    },
    /// A directed-mating offer changed status (sent to both sides).
    MatingUpdate {
        offer: OfferView,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<MatingOutcome>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorInfo>,
    },
    /// Something the player should notice: a death of a possessed or
    /// founding organism, a day's births and deaths, a lineage milestone, a
    /// possessed organism in need. Also listed in `GET /api/lineage/stats`.
    Alert {
        alert: AlertView,
    },
    /// The session expired or was logged out; the socket closes next.
    SessionEnded {
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// Births and deaths of the day just ended.
    DailySummary,
    /// The first offspring of the lineage, or a new generation reached.
    Birth,
    /// A possessed organism or a founder died.
    Death,
    /// Population or legacy milestone, or extinction.
    Milestone,
    /// A possessed organism is parched or starving (possession overrides
    /// its own foraging once its standing order is reached).
    Need,
    /// A catastrophe began or ended, or an era closed.
    World,
    /// An achievement (species founded, longevity, niche dominance,
    /// surviving a catastrophe).
    Achievement,
    /// The marketplace and renown: a listing bought, a sale paid, a refund,
    /// a listing closed, an achievement bonus.
    Market,
}

impl AlertKind {
    pub const ALL: [AlertKind; 8] = [
        AlertKind::DailySummary,
        AlertKind::Birth,
        AlertKind::Death,
        AlertKind::Milestone,
        AlertKind::Need,
        AlertKind::World,
        AlertKind::Achievement,
        AlertKind::Market,
    ];

    /// The wire name (`daily_summary`, `birth`, ...).
    pub fn name(self) -> &'static str {
        match self {
            AlertKind::DailySummary => "daily_summary",
            AlertKind::Birth => "birth",
            AlertKind::Death => "death",
            AlertKind::Milestone => "milestone",
            AlertKind::Need => "need",
            AlertKind::World => "world",
            AlertKind::Achievement => "achievement",
            AlertKind::Market => "market",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// `GET` / `POST /api/alerts/prefs`: which alert kinds a player sees in the
/// feed and which pop up as a toast. Stored per player (`Store`); the
/// server still records every alert, and the client applies the
/// preferences, so a kind switched back on shows its recent alerts again.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlertPrefs {
    /// Kinds left out of the feed and the unread badge (never toasted).
    #[serde(default)]
    pub hidden: Vec<AlertKind>,
    /// Kinds that also pop up as a toast when they arrive.
    #[serde(default = "AlertPrefs::default_toast")]
    pub toast: Vec<AlertKind>,
}

impl AlertPrefs {
    /// Toasted by default: what needs attention now.
    pub fn default_toast() -> Vec<AlertKind> {
        vec![AlertKind::Death, AlertKind::Need, AlertKind::Market]
    }

    /// Sorted, without duplicates, and nothing hidden is toasted.
    pub fn normalized(mut self) -> Self {
        self.hidden.sort_unstable();
        self.hidden.dedup();
        self.toast.sort_unstable();
        self.toast.dedup();
        let hidden = &self.hidden;
        self.toast.retain(|k| !hidden.contains(k));
        self
    }

    pub fn shows(&self, kind: AlertKind) -> bool {
        !self.hidden.contains(&kind)
    }

    pub fn toasts(&self, kind: AlertKind) -> bool {
        self.shows(kind) && self.toast.contains(&kind)
    }
}

impl Default for AlertPrefs {
    fn default() -> Self {
        AlertPrefs { hidden: Vec::new(), toast: Self::default_toast() }.normalized()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AlertView {
    pub tick: u64,
    pub day: u64,
    pub kind: AlertKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organism: Option<OrgId>,
}

impl ServerMsg {
    pub fn ok(id: Option<u64>, command: &str, data: Option<serde_json::Value>) -> Self {
        ServerMsg::Result { id, command: command.into(), ok: true, error: None, data }
    }

    pub fn err(id: Option<u64>, command: &str, error: ErrorInfo) -> Self {
        ServerMsg::Result { id, command: command.into(), ok: false, error: Some(error), data: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
    /// Machine-readable specifics for some codes (`incompatible_body_plans`
    /// lists the differing genes as [`BodyPlanGap`]s under `genes`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl ErrorInfo {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), detail: None }
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl From<&evo_sim::world::commands::CommandError> for ErrorInfo {
    fn from(e: &evo_sim::world::commands::CommandError) -> Self {
        use evo_sim::world::commands::CommandError::*;
        let code = match e {
            NotFound(_) => "not_found",
            NotOwned(_) => "not_owned",
            NoConsent => "no_consent",
            NotMature(_) => "not_mature",
            SameSex => "same_sex",
            AlreadyPregnant => "already_pregnant",
            TooFar { .. } => "too_far",
            NoSlotsLeft => "no_slots_left",
            PossessedByOther(_) => "possessed_by_other",
        };
        ErrorInfo::new(code, e.to_string())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct MatingOutcome {
    pub mismatch: f32,
    pub cost: f32,
    pub failure_chance: f32,
    pub fertility: f32,
    pub conceived: bool,
}

impl From<evo_sim::world::commands::DirectedOutcome> for MatingOutcome {
    fn from(o: evo_sim::world::commands::DirectedOutcome) -> Self {
        Self {
            mismatch: o.mismatch,
            cost: o.cost,
            failure_chance: o.failure_chance,
            fertility: o.fertility,
            conceived: o.conceived,
        }
    }
}

/// A body-plan gene (endothermy, birth mode, body size, carnivory) in which
/// two organisms differ enough to lower their fertility.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BodyPlanGap {
    /// Gene key as in `/api/genes` (`"endothermy"`, `"body_size"`, …).
    pub gene: String,
    /// Difference in expressed value (fertility falls to 0 at
    /// `evo_sim::mating::BODY_PLAN_STERILE`).
    pub gap: f32,
}

/// Reply to `preview_mating`: what the directed mating would give now.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MatingPreviewView {
    pub mine: OrgId,
    pub partner: OrgId,
    /// Whether the pair can conceive at all (`fertility > 0`).
    pub compatible: bool,
    /// Fertility the mating would run with: the lower of the two below,
    /// times the mates' infections.
    pub fertility: f32,
    /// 0 for different body plans (thermoregulation, birth mode, body size,
    /// diet).
    pub body_plan_fertility: f32,
    /// Outbreeding depression over the whole genome.
    pub distance_fertility: f32,
    pub body_plan_gaps: Vec<BodyPlanGap>,
    /// Larger of the two preference mismatches (≤ 1: both would choose it).
    pub mismatch: f32,
    pub cost: f32,
    pub failure_chance: f32,
    /// Tiles apart (they must meet, within `meeting_distance`).
    pub distance: f32,
    pub meeting_distance: f32,
    /// Why a request for this pair would be refused for incompatibility
    /// (`incompatible_body_plans` or `genetically_incompatible`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<ErrorInfo>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfferStatus {
    /// Waiting for the partner owner's answer.
    Pending,
    /// Consent given; executes once the two organisms meet.
    Accepted,
    Completed,
    Declined,
    Cancelled,
    /// The agreement could not be carried out: an organism died (`died`)
    /// or the rules refused it when it ran.
    Failed,
    /// The season ended (slots reset) before it happened.
    Expired,
}

impl OfferStatus {
    pub fn is_open(self) -> bool {
        matches!(self, OfferStatus::Pending | OfferStatus::Accepted)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OfferView {
    pub id: u64,
    pub from_player: PlayerId,
    pub from_name: String,
    pub to_player: PlayerId,
    pub to_name: String,
    /// The requester's organism.
    pub mine: OrgId,
    /// The partner owner's organism.
    pub partner: OrgId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<u64>,
    pub status: OfferStatus,
    pub created_tick: u64,
    /// The marketplace listing this agreement was bought from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing: Option<u64>,
    /// Renown held in escrow (the price, from the requester) while open.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub escrow: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WeatherView {
    /// Map-wide mean wind, 0 calm to 1 gale.
    pub mean_wind: f32,
    pub storm: bool,
    /// Share of land in drought, 0..1.
    pub drought: f32,
    /// Share of river tiles in flood, 0..1.
    pub flood: f32,
    /// The catastrophe under way, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catastrophe: Option<CatastropheView>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClockView {
    pub tick: u64,
    pub day: u64,
    pub year: u64,
    pub hour: u32,
    pub season: String,
    pub daytime: bool,
    /// Current era (0-based); eras last `era_days` days.
    pub era: u64,
    pub era_days: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct ViewWindow {
    pub x: f32,
    pub y: f32,
    /// Half the side of the square window, in tiles.
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct NeedsView {
    pub hunger: f32,
    pub thirst: f32,
    pub energy: f32,
    pub cold: f32,
    pub heat: f32,
    pub safety: f32,
    pub breeding: f32,
    pub social: f32,
}

/// One of the player's own organisms: full detail.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OwnOrganism {
    pub id: OrgId,
    pub lineage: u32,
    pub x: f32,
    pub y: f32,
    pub action: String,
    pub sex: String,
    pub age_days: f32,
    pub growth: f32,
    pub matured: bool,
    pub health: f32,
    pub needs: NeedsView,
    pub possessed: bool,
    pub pregnant: bool,
    pub generation: u32,
    /// Visible body plan, for the client's sprite.
    pub look: Appearance,
    /// Possessed only: the move order's target `[x, y]` (tiles) while it has
    /// one, so the client can predict the walk between state messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<[f32; 2]>,
    /// Possessed only: tiles per tick it walks toward an order on open
    /// ground (`World::move_speed`, before the terrain's speed factor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
}

/// Someone else's organism inside the view window: only what can be seen.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NearbyOrganism {
    pub id: OrgId,
    pub owner: PlayerId,
    pub lineage: u32,
    pub x: f32,
    pub y: f32,
    pub action: String,
    pub sex: String,
    pub matured: bool,
    /// Visible body plan, for the client's sprite.
    pub look: Appearance,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LeaderRow {
    pub player: PlayerId,
    pub name: String,
    /// Share of alleles among living breeding adults.
    pub share: f64,
    /// Share weighted by offspring raised to adulthood.
    pub weighted_share: f64,
    /// Living organisms the player owns.
    pub alive: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Leaderboard {
    /// Day of the legacy snapshot (none before the first simulated day ends).
    pub day: Option<u64>,
    pub breeding_adults: usize,
    pub rows: Vec<LeaderRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StateView {
    pub clock: ClockView,
    pub weather: WeatherView,
    pub player: PlayerId,
    pub lineage: Option<u32>,
    pub slots_left: u32,
    /// Living members of the player's lineage. With level of detail on this
    /// includes cohort members of abstract regions, which are not agents and
    /// so are not in `organisms`.
    pub population: usize,
    pub migration: Option<[f32; 2]>,
    pub view: ViewWindow,
    pub organisms: Vec<OwnOrganism>,
    pub nearby: Vec<NearbyOrganism>,
    pub leaderboard: Leaderboard,
    pub offers: Vec<OfferView>,
    /// Renown available to spend.
    #[serde(default)]
    pub balance: u64,
    /// Renown of yours held in escrow on open agreements.
    #[serde(default)]
    pub escrow: u64,
}

/// World summary for `GET /api/world`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorldSummary {
    pub clock: ClockView,
    pub weather: WeatherView,
    pub width: usize,
    pub height: usize,
    /// Every living organism, cohort members of abstract regions included.
    pub organisms: usize,
    /// Organisms simulated as full agents (all of them with level of detail off).
    pub agents: usize,
    pub lineages: usize,
}

/// One world of `GET /api/worlds`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorldListing {
    /// What `?world=` takes.
    pub id: String,
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub day: u64,
    pub year: u64,
    pub season: String,
    /// Living organisms (`WorldSummary::organisms`).
    pub population: usize,
    pub lineages: usize,
    /// Distinct players connected right now.
    pub players_online: usize,
}

/// `GET /api/worlds`: the worlds this server hosts, the default first.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorldList {
    /// The world used when a request names none.
    pub default: String,
    pub worlds: Vec<WorldListing>,
}

/// `GET /api/terrain`: the whole tile grid. Every layer is one byte per tile
/// in row-major order (`y * width + x`), base64-encoded.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TerrainView {
    pub width: usize,
    pub height: usize,
    /// Changes when the fixed layers change (they don't yet after generation).
    pub terrain_version: u64,
    /// Biome index: 0 deep water, 1 shallow water, 2 marsh, 3 grassland,
    /// 4 forest, 5 desert, 6 tundra, 7 mountain.
    pub biome: String,
    /// Elevation, 0..255 for the map's lowest..highest (percentile-normalized).
    pub elevation: String,
    /// River channel strength, 0 none .. 255 main stream.
    pub river: String,
    /// Water depth at normal wetness, centimeters (capped at 255).
    pub base_water: String,
    pub dynamic: TerrainDynamic,
}

/// `GET /api/terrain/dynamic`: the layers that change with weather and
/// seasons (updated by the sim every hour). Same encoding as [`TerrainView`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TerrainDynamic {
    pub tick: u64,
    pub day: u64,
    pub hour: u32,
    pub season: String,
    /// Current water depth, centimeters (capped at 255).
    pub water: String,
    /// Ice cover, 0..255 for 0..1.
    pub ice: String,
    /// Snow, 0..255 for 0..0.5 m (deeper snow saturates).
    pub snow: String,
    /// Plant food as a share of the tile's maximum, 0..255.
    pub veg: String,
    /// Regional soil wetness, 0..255 for 0..1 (0.5 normal; below 0.3 is drought).
    pub wetness: String,
}

/// Count of one sex in an age band.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AgeBand {
    pub from_days: f32,
    pub to_days: f32,
    pub female: usize,
    pub male: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct DayVitals {
    pub day: u64,
    pub births: u64,
    /// Deaths by cause (`starved`, `thirst`, `eaten`, `old_age`, ...).
    pub deaths: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GeneStat {
    /// Wire name of the gene (as in `GET /api/genes`).
    pub gene: String,
    /// Mean expressed value over living members, 0..1.
    pub mean: f32,
    /// Standard deviation over living members.
    pub sd: f32,
    /// Mean at founding (the first daily record, else the current mean).
    pub founding_mean: f32,
    /// Daily means, oldest first, aligned with `LineageStats::history_days`.
    pub history: Vec<f32>,
    /// Distribution over living members: counts per equal-width bin of the
    /// expressed value, [`GENE_BINS`] bins over 0..1 (1.0 falls in the
    /// last). All zero without members.
    #[serde(default)]
    pub bins: Vec<u32>,
}

/// Bins of [`GeneStat::bins`].
pub const GENE_BINS: usize = 10;

/// Where the alleles carried by a lineage's living members came from
/// (`LineageStats::origins`). Every allele carries the id of the player it
/// originated with: founder alleles the designer's, a mutation the owner of
/// the organism it appeared in. Tags pass unchanged through any mating, so
/// alleles of another player's lineage enter yours by cross-lineage
/// matings (directed matings, stud listings, wild encounters).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AlleleOrigin {
    /// The originating player (0: the wild ecosystem).
    pub player: PlayerId,
    pub name: String,
    /// That player's lineage, if they have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<String>,
    /// Alleles carried, gene and hidden-recessive loci of every member.
    pub alleles: u64,
    /// Share of all the alleles the members carry, 0..1.
    pub share: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LegacyPoint {
    pub day: u64,
    pub share: f64,
    pub weighted_share: f64,
    pub alive: usize,
}

/// `GET /api/lineage/stats`: the genetics dashboard of the caller's lineage.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LineageStats {
    pub player: PlayerId,
    pub lineage: Option<u32>,
    pub name: Option<String>,
    pub day: u64,
    /// Living members, cohort members of abstract regions included (level
    /// of detail). `adults` and `juveniles` count them too; the age bands,
    /// pregnancies, generation and gene statistics cover agents only.
    pub alive: usize,
    /// Members simulated as full agents (all of them with level of detail off).
    pub agents: usize,
    pub adults: usize,
    pub juveniles: usize,
    pub pregnant: usize,
    pub max_generation: u32,
    /// Population by age band and sex (equal-width bands up to the oldest).
    pub age_bands: Vec<AgeBand>,
    /// Births and deaths by cause per recent day, oldest first; the last
    /// entry is the current day so far.
    pub vitals: Vec<DayVitals>,
    /// Days of the daily records the gene histories cover, oldest first.
    pub history_days: Vec<u64>,
    /// Every gene, largest drift from the founding mean first.
    pub genes: Vec<GeneStat>,
    /// Whose alleles the living members carry, largest share first.
    #[serde(default)]
    pub origins: Vec<AlleleOrigin>,
    /// Legacy share per recorded day, oldest first.
    pub legacy: Vec<LegacyPoint>,
    /// Recent alerts, newest last.
    pub alerts: Vec<AlertView>,
}

/// One player's standing over a season of the history.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SeasonRow {
    pub player: PlayerId,
    pub name: String,
    pub mean_share: f64,
    pub final_share: f64,
    pub peak_share: f64,
    pub mean_weighted_share: f64,
    /// Most organisms the player had alive on a recorded day of the season.
    pub peak_alive: usize,
    pub days: usize,
}

/// `GET /api/leaderboard/seasons`: legacy standings per season, ranked by
/// mean share over the season's recorded days.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SeasonBoard {
    pub year: u64,
    pub season: String,
    pub first_day: u64,
    pub last_day: u64,
    pub days: usize,
    /// True for the season still being played.
    pub current: bool,
    pub rows: Vec<SeasonRow>,
}

/// Why a balance changed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerKind {
    /// The founding grant.
    Grant,
    /// Daily income from the legacy share.
    Income,
    /// An achievement bonus.
    Achievement,
    /// A price or stud fee moved into escrow (negative).
    Escrow,
    /// Escrow paid out to the partner's owner when the mating ran.
    Sale,
    /// Escrow returned: the agreement ended without a mating.
    Refund,
}

impl LedgerKind {
    pub fn name(self) -> &'static str {
        match self {
            LedgerKind::Grant => "grant",
            LedgerKind::Income => "income",
            LedgerKind::Achievement => "achievement",
            LedgerKind::Escrow => "escrow",
            LedgerKind::Sale => "sale",
            LedgerKind::Refund => "refund",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            LedgerKind::Grant,
            LedgerKind::Income,
            LedgerKind::Achievement,
            LedgerKind::Escrow,
            LedgerKind::Sale,
            LedgerKind::Refund,
        ]
        .into_iter()
        .find(|k| k.name() == s)
    }
}

/// One change of a player's balance.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LedgerEntry {
    /// Unique per world, increasing.
    pub seq: u64,
    pub player: PlayerId,
    pub tick: u64,
    pub day: u64,
    pub delta: i64,
    /// Balance after the change.
    pub balance: u64,
    pub kind: LedgerKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offer: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing: Option<u64>,
}

/// Data of the `ledger` command.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LedgerView {
    pub balance: u64,
    pub escrow: u64,
    /// Newest first.
    pub entries: Vec<LedgerEntry>,
}

/// An expressed gene value of a listed organism.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TraitView {
    /// Gene key as in `/api/genes`.
    pub gene: String,
    pub value: f32,
}

/// A marketplace listing with what a buyer gets to see of the organism.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ListingView {
    pub id: u64,
    pub owner: PlayerId,
    pub owner_name: String,
    pub organism: OrgId,
    /// `stud` (male) or `dam` (female).
    pub role: String,
    pub sex: String,
    pub fee: u64,
    pub created_tick: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_tick: Option<u64>,
    /// Matings bought from it so far.
    pub sales: u32,
    /// A dam already promised in an open agreement (one at a time).
    pub busy: bool,
    pub pregnant: bool,
    pub lineage: u32,
    pub lineage_name: String,
    pub generation: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mother: Option<OrgId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub father: Option<OrgId>,
    pub offspring_raised: u32,
    pub age_days: f32,
    pub health: f32,
    pub x: f32,
    pub y: f32,
    /// Key genes (body size, legs, coverings, locomotion, defenses, diet,
    /// coloration), expressed values.
    pub traits: Vec<TraitView>,
    /// Compatibility with the buyer's chosen organism (`browse_market` with
    /// `mine`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<MatingPreviewView>,
}

/// Data of `browse_market`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MarketView {
    pub balance: u64,
    pub escrow: u64,
    pub slots_left: u32,
    pub listings: Vec<ListingView>,
}

pub fn action_name(a: Action) -> &'static str {
    match a {
        Action::Idle => "idle",
        Action::Graze => "graze",
        Action::Hunt => "hunt",
        Action::Scavenge => "scavenge",
        Action::Drink => "drink",
        Action::Rest => "rest",
        Action::Bask => "bask",
        Action::Flee => "flee",
        Action::SeekMate => "seek_mate",
        Action::Group => "group",
        Action::Wander => "wander",
        Action::Migrate => "migrate",
        Action::FollowParent => "follow_parent",
        Action::Guard => "guard",
        Action::Ordered => "ordered",
        Action::Dormant => "dormant",
        Action::Cache => "cache",
        Action::Sentry => "sentry",
    }
}

pub fn sex_name(s: Sex) -> &'static str {
    match s {
        Sex::Female => "female",
        Sex::Male => "male",
    }
}

pub fn season_name(s: Season) -> &'static str {
    match s {
        Season::Spring => "spring",
        Season::Summer => "summer",
        Season::Autumn => "autumn",
        Season::Winter => "winter",
    }
}

/// Wire name of a death cause (the sim's label in `snake_case`).
pub fn death_name(c: evo_sim::organism::DeathCause) -> &'static str {
    use evo_sim::organism::DeathCause::*;
    match c {
        Starvation => "starved",
        Thirst => "thirst",
        Exposure => "exposure",
        Predation => "eaten",
        Toxin => "toxin",
        Drowning => "drowned",
        OldAge => "old_age",
        Attrition => "attrition",
        Recessive => "recessive",
        Disease => "disease",
        Journey => "journey",
    }
}

impl OwnOrganism {
    pub fn from_org(o: &Organism, ticks_per_day: u32) -> Self {
        let n = &o.needs;
        Self {
            id: o.id,
            lineage: o.lineage,
            x: o.pos.x,
            y: o.pos.y,
            action: action_name(o.action).into(),
            sex: sex_name(o.sex).into(),
            age_days: o.age_days(ticks_per_day),
            growth: o.growth,
            matured: o.matured,
            health: o.health,
            needs: NeedsView {
                hunger: n.hunger,
                thirst: n.thirst,
                energy: n.energy,
                cold: n.cold,
                heat: n.heat,
                safety: n.safety,
                breeding: n.breeding,
                social: n.social,
            },
            possessed: o.possessed_by.is_some(),
            pregnant: o.pregnancy.is_some(),
            generation: o.generation,
            look: Appearance::of(o),
            order: o.possessed_by.and(o.order).map(|t| [t.x, t.y]),
            speed: None,
        }
    }
}

impl NearbyOrganism {
    pub fn from_org(o: &Organism) -> Self {
        Self {
            id: o.id,
            owner: o.owner,
            lineage: o.lineage,
            x: o.pos.x,
            y: o.pos.y,
            action: action_name(o.action).into(),
            sex: sex_name(o.sex).into(),
            matured: o.matured,
            look: Appearance::of(o),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_messages_parse() {
        let m: ClientEnvelope =
            serde_json::from_str(r#"{"id":3,"type":"order_move","organism":7,"x":1.5,"y":2}"#).unwrap();
        assert_eq!(m.id, Some(3));
        assert_eq!(m.msg, ClientMsg::OrderMove { organism: 7, x: 1.5, y: 2.0 });
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"clear_migration"}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::ClearMigration);
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"request_mating","mine":1,"partner":2}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::RequestMating { mine: 1, partner: 2, price: None });
        assert!(serde_json::from_str::<ClientEnvelope>(r#"{"type":"teleport"}"#).is_err());
    }

    #[test]
    fn market_messages_parse() {
        let m: ClientEnvelope =
            serde_json::from_str(r#"{"id":4,"type":"create_listing","organism":9,"fee":12}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::CreateListing { organism: 9, fee: 12, expires_days: None });
        assert_eq!(m.msg.name(), "create_listing");
        let m: ClientEnvelope =
            serde_json::from_str(r#"{"type":"create_listing","organism":9,"fee":0,"expires_days":3}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::CreateListing { organism: 9, fee: 0, expires_days: Some(3) });
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"buy_listing","listing":2,"mine":5}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::BuyListing { listing: 2, mine: 5 });
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"withdraw_listing","listing":2}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::WithdrawListing { listing: 2 });
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"browse_market"}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::BrowseMarket { mine: None });
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"browse_market","mine":3}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::BrowseMarket { mine: Some(3) });
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"ledger","limit":5}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::Ledger { limit: Some(5) });
        // Negative fees and missing fields are refused by the parser.
        assert!(serde_json::from_str::<ClientEnvelope>(r#"{"type":"create_listing","organism":9,"fee":-1}"#).is_err());
        assert!(serde_json::from_str::<ClientEnvelope>(r#"{"type":"buy_listing","listing":2}"#).is_err());
        // Ledger entries as the client parses them.
        let e = LedgerEntry {
            seq: 3,
            player: 1,
            tick: 96,
            day: 2,
            delta: -10,
            balance: 40,
            kind: LedgerKind::Escrow,
            text: "held".into(),
            offer: Some(7),
            listing: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"seq": 3, "player": 1, "tick": 96, "day": 2, "delta": -10, "balance": 40, "kind": "escrow", "text": "held", "offer": 7})
        );
        for k in ["grant", "income", "achievement", "escrow", "sale", "refund"] {
            let kind = LedgerKind::parse(k).unwrap();
            assert_eq!(serde_json::to_value(kind).unwrap(), k);
        }
        let alert = AlertView { tick: 1, day: 0, kind: AlertKind::Market, text: "sold".into(), organism: None };
        assert_eq!(serde_json::to_value(alert).unwrap()["kind"], "market");
    }

    #[test]
    fn alert_kind_names_match_serde_and_prefs_normalize() {
        for k in AlertKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.name());
            assert_eq!(AlertKind::parse(k.name()), Some(k));
        }
        assert_eq!(AlertKind::parse("nope"), None);
        // Missing fields read as the defaults.
        let p: AlertPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p, AlertPrefs::default());
        assert!(p.toasts(AlertKind::Death) && !p.toasts(AlertKind::Birth) && p.shows(AlertKind::Birth));
        let p: AlertPrefs =
            serde_json::from_str(r#"{"hidden":["world","daily_summary","world"],"toast":["world","birth","birth"]}"#)
                .unwrap();
        let p = p.normalized();
        assert_eq!(p.hidden, vec![AlertKind::DailySummary, AlertKind::World]);
        assert_eq!(p.toast, vec![AlertKind::Birth], "a hidden kind is never toasted");
        assert!(!p.shows(AlertKind::World) && !p.toasts(AlertKind::World));
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::json!({"hidden": ["daily_summary", "world"], "toast": ["birth"]})
        );
        assert!(serde_json::from_str::<AlertPrefs>(r#"{"hidden":["bogus"]}"#).is_err());
    }

    #[test]
    fn server_messages_are_tagged() {
        let s = serde_json::to_value(ServerMsg::ok(Some(1), "possess", None)).unwrap();
        assert_eq!(s["type"], "result");
        assert_eq!(s["ok"], true);
        assert_eq!(s["id"], 1);
        // The shapes client/src/protocol.ts parses.
        let alert = AlertView { tick: 96, day: 2, kind: AlertKind::DailySummary, text: "Day 3".into(), organism: None };
        let s = serde_json::to_value(ServerMsg::Alert { alert }).unwrap();
        assert_eq!(
            s,
            serde_json::json!({"type": "alert", "alert": {"tick": 96, "day": 2, "kind": "daily_summary", "text": "Day 3"}})
        );
        let s = serde_json::to_value(ServerMsg::SessionEnded { reason: "expired".into() }).unwrap();
        assert_eq!(s, serde_json::json!({"type": "session_ended", "reason": "expired"}));
        for c in evo_sim::organism::DeathCause::ALL {
            assert_eq!(death_name(c), c.label().replace(' ', "_"));
        }
    }
}

/// A catastrophe (`GET /api/catastrophes`, and the active one in
/// `weather.catastrophe`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CatastropheView {
    pub id: u32,
    /// `severe_winter`, `megadrought`, `epidemic` or `flood_year`.
    pub kind: String,
    pub severity: f32,
    pub start_day: u64,
    /// Planned first day after it while active; the day it ended once over.
    pub end_day: u64,
    pub active: bool,
    pub population_before: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub population_after: Option<usize>,
    /// Players with a living lineage at its start, and those alive at its end.
    pub players_before: Vec<PlayerId>,
    pub survivors: Vec<PlayerId>,
}

/// `GET /api/catastrophes`: catastrophes that have started, newest first.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CatastropheList {
    /// Random catastrophes are on in this world.
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<CatastropheView>,
    pub history: Vec<CatastropheView>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpeciesLineage {
    pub lineage: u32,
    pub name: String,
    pub owner: PlayerId,
    /// Estimated breeding adults of this lineage in the species.
    pub adults: f64,
}

/// One detected species.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpeciesView {
    pub id: u32,
    pub name: String,
    /// `initial`, `founding` or `split`.
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
    pub founded_day: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extinct_day: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<u32>,
    /// Player credited with the speciation (splits only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub founder: Option<PlayerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub founder_name: Option<String>,
    /// Origin tag holding the most alleles in its adults.
    pub dominant: PlayerId,
    pub dominant_name: String,
    pub dominant_share: f64,
    /// Estimated breeding adults at the last detection, and the most ever.
    pub adults: f64,
    pub peak_adults: f64,
    pub lineages: Vec<SpeciesLineage>,
    pub carnivory: f32,
    pub body_size: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FoundedCount {
    pub player: PlayerId,
    pub name: String,
    pub count: u32,
}

/// `GET /api/species`: every species detected, living first by size.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpeciesList {
    pub day: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_day: Option<u64>,
    pub samples: usize,
    pub alive: usize,
    pub species: Vec<SpeciesView>,
    /// Speciation events credited per player.
    pub founded: Vec<FoundedCount>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EraRow {
    pub player: PlayerId,
    pub name: String,
    pub share: f64,
    pub weighted_share: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NicheView {
    /// `diet:herbivore|omnivore|carnivore` or `biome:<name>`.
    pub niche: String,
    pub player: PlayerId,
    pub name: String,
    pub share: f64,
    pub adults: f64,
}

/// The final legacy ranking of a finished era.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EraBoard {
    pub era: u64,
    pub first_day: u64,
    pub last_day: u64,
    pub breeding_adults: usize,
    pub species_alive: usize,
    pub catastrophes: Vec<u32>,
    pub rows: Vec<EraRow>,
    pub niches: Vec<NicheView>,
}

/// `GET /api/eras`: the current era and finished eras' rankings, newest first.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EraList {
    pub era: u64,
    pub era_days: u64,
    pub first_day: u64,
    pub last_day: u64,
    pub days_left: u64,
    pub eras: Vec<EraBoard>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AchievementView {
    pub day: u64,
    pub player: PlayerId,
    pub name: String,
    /// `species_founded`, `lineage_longevity`, `niche_dominance` or
    /// `survived_catastrophe`.
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AchievementTally {
    pub player: PlayerId,
    pub name: String,
    pub species_founded: u32,
    /// Longest lineage life in days (to today, or to extinction).
    pub longevity_days: u64,
    pub niche_dominance: u32,
    pub catastrophes_survived: u32,
}

/// `GET /api/achievements`: per-player tallies and the latest achievements.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AchievementList {
    pub players: Vec<AchievementTally>,
    /// Newest first (at most 200).
    pub recent: Vec<AchievementView>,
}

/// Everything the scoring extras expose, built on the world thread.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChronicleView {
    pub species: SpeciesList,
    pub eras: EraList,
    pub catastrophes: CatastropheList,
    pub achievements: AchievementList,
}
