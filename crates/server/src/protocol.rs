//! JSON wire types shared by the HTTP API and the WebSocket protocol.
//!
//! Every WebSocket message is a JSON object with a `type` field in
//! `snake_case`. Client messages may carry an `id`, echoed in the matching
//! [`ServerMsg::Result`] so a client can pair replies with requests.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::appearance::Appearance;
use crate::store::BodyPlanMode;
use crate::vision::VisionView;
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
    Possess { organism: OrgId },
    /// Walk a possessed organism toward a point (tile units).
    OrderMove { organism: OrgId, x: f32, y: f32 },
    /// Order a possessed organism to attack `target` (another player's or a
    /// wild organism within sight): it pursues it and strikes in reach by
    /// the usual strike rules, and its pack-mates nearby join in. Refused on
    /// your own organisms (`own_target`), out of sight (`target_out_of_sight`), too
    /// heavy (`too_big`, with `mass` and `limit` in `detail`), in its burrow
    /// (`sheltered`) or of a lineage founded within the attack grace
    /// (`newcomer_grace`, with `days_left`). The target's owner is alerted.
    /// A move order or a release ends it.
    PossessAttack { organism: OrgId, target: OrgId },
    /// Return a possessed organism to its genetic behavior.
    Release { organism: OrgId },
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
    PreviewMating { mine: OrgId, partner: OrgId },
    /// Offer a directed mating of your `mine` with any suitable animal of
    /// another player's `lineage`: its owner picks one when accepting
    /// (`respond_mating` with `partner`), or the server picks the nearest
    /// available one (adult, of the other sex, not pregnant, not promised,
    /// fertile with `mine`). Spares guessing which of a rival's females are
    /// free (requests to a pregnant one are refused `partner_unavailable`).
    /// Otherwise like `request_mating` (price in escrow, the same limits).
    /// Data: `{offer}`; the offer's `partner` is 0 and `lineage` set until
    /// accepted.
    OfferToLineage {
        mine: OrgId,
        lineage: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        price: Option<u64>,
    },
    /// The partner organism's owner accepts or declines an offer. For an
    /// offer to the lineage (`offer_to_lineage`), `partner` picks the animal
    /// (default: the nearest available one; `no_partner_available` if none,
    /// and the offer stays open).
    RespondMating {
        request: u64,
        accept: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        partner: Option<OrgId>,
    },
    /// Retry an accepted agreement now (it also completes on its own once the
    /// two organisms meet).
    CompleteMating { request: u64 },
    /// Either side withdraws an open offer or agreement.
    CancelMating { request: u64 },
    /// Group command: your adults drift toward a point. Without `group`,
    /// every group of yours; with it, that group only. Replaces the
    /// territory of the groups it reaches.
    Migrate {
        x: f32,
        y: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<u8>,
    },
    /// Drop the migration order of `group`, or of every group without it.
    ClearMigration {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<u8>,
    },
    /// Group command: `group` (default 0, the main group) holds the circle
    /// of `radius` tiles around `x, y`: its members roam and forage inside
    /// it and chase strangers out. The radius is kept between
    /// `evo_sim::world::groups::TERRITORY_MIN_RADIUS` and what the group's
    /// adults can hold (its `max_radius` in the state). Replaces the group's
    /// migration order. A group may claim once every
    /// `evo_sim::world::groups::CLAIM_COOLDOWN_DAYS` (`claim_cooldown`).
    /// Data: the territory as claimed ([`TerritoryView`]).
    ClaimTerritory {
        #[serde(default)]
        group: u8,
        x: f32,
        y: f32,
        radius: f32,
    },
    /// Group command: `group` (default 0) gives up its territory.
    AbandonTerritory {
        #[serde(default)]
        group: u8,
    },
    /// Group command: `group` (default 0) gathers at `x, y` and holds near
    /// it (a circle sized by the group, `evo_sim::world::World::rally_radius`)
    /// for `evo_sim::world::groups::RALLY_DAYS`, unless cancelled or replaced
    /// first. Replaces the group's migration order, territory or earlier
    /// rally. Data: the rally as given ([`RallyView`]).
    Rally {
        #[serde(default)]
        group: u8,
        x: f32,
        y: f32,
    },
    /// Group command: `group` (default 0) stops answering its rally.
    CancelRally {
        #[serde(default)]
        group: u8,
    },
    /// Group command: divide `group` (default 0) in two. With `organisms`
    /// (yours, in that group, at most [`MAX_SPLIT_ORGANISMS`]), those form
    /// the new group; without, the half of the group on the far side of its
    /// widest spread. The new group starts with no orders. Data:
    /// `{group, moved, kept}`.
    SplitGroup {
        #[serde(default)]
        group: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        organisms: Option<Vec<OrgId>>,
    },
    /// Retire your living lineage, `lineage` (which must be it, so a late
    /// click can't retire the next one), to start over: every animal,
    /// cohort and egg of it leaves the world at once (removed, not killed,
    /// and not turned wild), its orders end, and you may found again at
    /// once. Renown, ledger, records and the old lineage's history stay.
    /// Data: `{removed}`.
    RetireLineage { lineage: u32 },
    /// Group command: every member of group `from` joins group `into`
    /// (default 0); `from`'s orders end. Data: `{moved}`.
    MergeGroups {
        from: u8,
        #[serde(default)]
        into: u8,
    },
    /// Pin the view window (interest area) to a point.
    SetView { x: f32, y: f32, radius: f32 },
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
    WithdrawListing { listing: u64 },
    /// Buy one mating with a listed organism for your adult `mine`: an
    /// already accepted agreement, the fee held in escrow until the two meet,
    /// your seasonal slot spent when it runs. Data: `{offer}`. Not for
    /// auctions (`auction_listing`: bid instead).
    BuyListing { listing: u64, mine: OrgId },
    /// Auction one mating with one of your adults: the highest bid at or
    /// above `reserve` when it closes wins. It closes after `ticks` ticks if
    /// given, else after `days` simulated days (default 1, at most 30).
    /// Data: `{listing}` ([`ListingView`]).
    CreateAuction {
        organism: OrgId,
        reserve: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        days: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ticks: Option<u64>,
    },
    /// Bid `amount` renown on an auction for your adult `mine`. The amount
    /// is held in escrow (raising your own highest bid holds only the
    /// difference); you are refunded at once when outbid. Data: `{listing}`.
    Bid { listing: u64, amount: u64, mine: OrgId },
    /// One page of the open listings, each with the compatibility of your
    /// `mine` with the listed organism if given. Data: [`MarketView`]. With
    /// no fields: the newest [`MARKET_PAGE`] listings of every kind.
    BrowseMarket(MarketQuery),
    /// Who may send you mating requests: `block` or `unblock` a player
    /// (their requests are refused with `requests_refused`), turn
    /// `auto_decline` on to refuse every request (listings and auctions are
    /// unaffected: they are your own offer). With no fields, just read the
    /// settings. Data: [`MatingPrefs`].
    MatingPrefs {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block: Option<PlayerId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unblock: Option<PlayerId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auto_decline: Option<bool>,
    },
    /// Your balance and recent ledger entries (newest first, at most `limit`,
    /// default 50). Data: [`LedgerView`].
    Ledger {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
    },
    /// The renown shop's prices and your standing in it. Data: [`ShopView`].
    RenownShop,
    /// Buy one extra directed-mating slot for the rest of the weather
    /// season (`economy::slot_price`: 30 renown, doubling with each one
    /// bought, at most `economy::MAX_BOUGHT_SLOTS`). The renown is spent
    /// (destroyed). Data: [`ShopView`] after the purchase.
    BuyMatingSlot,
    /// Reinforcements for a small lineage: `economy::REINFORCE_SIZE` adults,
    /// copies of your lineage's living adults, join it near one of them
    /// (`economy::reinforce_price`: 150 renown, doubling with each purchase
    /// in a ranked season). Only while at most `economy::REINFORCE_MAX_SHARE`
    /// of the soft lineage cap is alive (`reinforce_blocked` in the shop
    /// says why not). Data: `{added: [ids], shop: ShopView}`.
    Reinforce,
    /// Buy or change your nameplate: a `color` from the shop's palette and a
    /// `title` (1–24 characters) shown with your name (cosmetic; 500 renown
    /// the first time, 100 per change). Data: `{nameplate, shop}`.
    SetNameplate { color: String, title: String },
    /// Name a species your lineage founded (cosmetic; 300 renown): a
    /// species whose founder is you, still alive. Data: `{species, name}`.
    NameSpecies { species: u32, name: String },
    /// Every player's nameplate. Data: `{nameplates: {player: {color, title}}}`.
    Nameplates,
    /// Which lineages yours can breed with: the best body-plan fertility
    /// between a sample of your adults and each other lineage's, and the
    /// body-plan gene that keeps you apart. Data: [`CompatibilityView`].
    Compatibility,
    /// Protocol 2 only: the client lost track of the delta states; the
    /// connection's next state is a keyframe. Handled by the connection (no
    /// `result`), never by the world.
    Resync,
    /// Receive the ranked season under way as `season` messages (rows
    /// `0..limit` of each category, default 5, at most
    /// [`crate::league::MAX_ROWS`], plus your own row): one at once, then
    /// one whenever the standings change (at most about once a second).
    /// `on: false` stops them. Every watcher pages one shared copy of the
    /// standings (`crate::standings`); answered by the world thread between
    /// ticks, never queued behind world changes.
    WatchSeason {
        on: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
    },
}

impl ClientMsg {
    pub fn name(&self) -> &'static str {
        match self {
            ClientMsg::Possess { .. } => "possess",
            ClientMsg::OrderMove { .. } => "order_move",
            ClientMsg::PossessAttack { .. } => "possess_attack",
            ClientMsg::Release { .. } => "release",
            ClientMsg::RequestMating { .. } => "request_mating",
            ClientMsg::PreviewMating { .. } => "preview_mating",
            ClientMsg::RespondMating { .. } => "respond_mating",
            ClientMsg::OfferToLineage { .. } => "offer_to_lineage",
            ClientMsg::CompleteMating { .. } => "complete_mating",
            ClientMsg::CancelMating { .. } => "cancel_mating",
            ClientMsg::Migrate { .. } => "migrate",
            ClientMsg::ClearMigration { .. } => "clear_migration",
            ClientMsg::ClaimTerritory { .. } => "claim_territory",
            ClientMsg::AbandonTerritory { .. } => "abandon_territory",
            ClientMsg::Rally { .. } => "rally",
            ClientMsg::CancelRally { .. } => "cancel_rally",
            ClientMsg::SplitGroup { .. } => "split_group",
            ClientMsg::MergeGroups { .. } => "merge_groups",
            ClientMsg::RetireLineage { .. } => "retire_lineage",
            ClientMsg::SetView { .. } => "set_view",
            ClientMsg::ClearView => "clear_view",
            ClientMsg::CreateListing { .. } => "create_listing",
            ClientMsg::WithdrawListing { .. } => "withdraw_listing",
            ClientMsg::BuyListing { .. } => "buy_listing",
            ClientMsg::CreateAuction { .. } => "create_auction",
            ClientMsg::Bid { .. } => "bid",
            ClientMsg::BrowseMarket { .. } => "browse_market",
            ClientMsg::Ledger { .. } => "ledger",
            ClientMsg::MatingPrefs { .. } => "mating_prefs",
            ClientMsg::RenownShop => "renown_shop",
            ClientMsg::BuyMatingSlot => "buy_mating_slot",
            ClientMsg::Reinforce => "reinforce",
            ClientMsg::Compatibility => "compatibility",
            ClientMsg::SetNameplate { .. } => "set_nameplate",
            ClientMsg::NameSpecies { .. } => "name_species",
            ClientMsg::Nameplates => "nameplates",
            ClientMsg::Resync => "resync",
            ClientMsg::WatchSeason { .. } => "watch_season",
        }
    }
}

/// Every command's wire name (`type`), as [`ClientMsg::name`] gives them.
pub const COMMAND_NAMES: &[&str] = &[
    "possess",
    "order_move",
    "possess_attack",
    "release",
    "request_mating",
    "preview_mating",
    "respond_mating",
    "complete_mating",
    "cancel_mating",
    "migrate",
    "clear_migration",
    "claim_territory",
    "abandon_territory",
    "rally",
    "cancel_rally",
    "split_group",
    "retire_lineage",
    "merge_groups",
    "set_view",
    "clear_view",
    "create_listing",
    "withdraw_listing",
    "buy_listing",
    "create_auction",
    "bid",
    "browse_market",
    "mating_prefs",
    "ledger",
    "renown_shop",
    "buy_mating_slot",
    "reinforce",
    "compatibility",
    "resync",
    "watch_season",
    "offer_to_lineage",
    "retire_lineage",
    "renown_shop",
    "buy_mating_slot",
    "reinforce",
    "compatibility",
    "set_nameplate",
    "name_species",
    "nameplates",
];

/// The known command named `t`, as a static name (for replies to messages
/// that didn't parse: never the client's own text).
pub fn command_name(t: &str) -> Option<&'static str> {
    COMMAND_NAMES.iter().copied().find(|&n| n == t)
}

impl ClientMsg {
    /// Group commands that change a group (territory, splits, merges): they
    /// have their own, tighter rate limit (`Limits::group_commands`).
    pub fn is_group_change(&self) -> bool {
        matches!(
            self,
            ClientMsg::ClaimTerritory { .. }
                | ClientMsg::AbandonTerritory { .. }
                | ClientMsg::SplitGroup { .. }
                | ClientMsg::MergeGroups { .. }
        )
    }
}

/// Most organisms a `split_group` may name.
pub const MAX_SPLIT_ORGANISMS: usize = 2000;

/// Listings in a `browse_market` page unless asked otherwise.
pub const MARKET_PAGE: usize = 20;
/// The largest `browse_market` page.
pub const MAX_MARKET_PAGE: usize = 50;

/// Order of a `browse_market` page.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarketSort {
    /// Newest listing first.
    #[default]
    Newest,
    /// Cheapest first (price: the fee, or an auction's highest bid or
    /// reserve), then newest.
    PriceAsc,
    PriceDesc,
    /// Closing soonest first (auctions' deadlines, listings' expiry; no
    /// expiry last).
    Ending,
    /// With `mine`: compatible first by expected fertility, then
    /// incompatible ones. Without `mine`, newest first.
    Fertility,
}

/// Which listings a `browse_market` page holds.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListingKind {
    Fixed,
    Auction,
}

/// `stud` (male) or `dam` (female).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListingRole {
    Stud,
    Dam,
}

/// The fields of `browse_market`, all optional.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct MarketQuery {
    /// One of your organisms: every listing in the page carries its
    /// compatibility with it (`preview`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mine: Option<OrgId>,
    /// Listings per page: default [`MARKET_PAGE`], at most
    /// [`MAX_MARKET_PAGE`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// The previous page's `next_cursor`: the page after it, in the same
    /// order and with the same filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<MarketSort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ListingKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<ListingRole>,
    /// Highest price (fee, or an auction's current bid or reserve).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_price: Option<u64>,
    /// Only organisms of this lineage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lineage: Option<u32>,
    /// Only organisms whose lineage belongs to this species (`GET
    /// /api/species`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub species: Option<u32>,
    /// With `mine`: only listings it can conceive with.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compatible: bool,
    /// Leave your own listings out of the page.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hide_own: bool,
    /// Also return all your own listings (`own`), whatever the filters.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub with_own: bool,
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
        /// client asked for it with `/ws?proto=2`, `wire::PROTO_VISION` for
        /// `proto=3` or higher; absent for protocol 1).
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
    /// The ranked season under way, to a connection that sent
    /// `watch_season` (same body as `GET /api/seasons/current`).
    Season(Box<crate::league::CurrentSeason>),
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
    /// A group order ended on its own: a migration arrived, a group was
    /// left with no members.
    Group,
    /// Another player ordered an attack on one of your organisms.
    Attack,
}

impl AlertKind {
    pub const ALL: [AlertKind; 10] = [
        AlertKind::DailySummary,
        AlertKind::Birth,
        AlertKind::Death,
        AlertKind::Milestone,
        AlertKind::Need,
        AlertKind::World,
        AlertKind::Achievement,
        AlertKind::Market,
        AlertKind::Group,
        AlertKind::Attack,
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
            AlertKind::Group => "group",
            AlertKind::Attack => "attack",
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
        // World events (catastrophes, era ends) too: they must not be missed.
        vec![
            AlertKind::Death,
            AlertKind::Need,
            AlertKind::World,
            AlertKind::Market,
            AlertKind::Group,
            AlertKind::Attack,
        ]
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

/// What a push notification is about (`crate::webpush`). Each kind can be
/// switched off on its own ([`PushPrefs`]); kinds that come from an alert
/// also follow the alert preferences: an alert kind hidden from the feed is
/// never pushed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PushKind {
    /// Another player asks to mate their organism with yours.
    Mating,
    /// A possessed organism or a founder died (`AlertKind::Death`).
    Death,
    /// The marketplace: outbid, an auction won or sold, a bid received, a
    /// listing bought, a sale paid, a listing closed (`AlertKind::Market`;
    /// escrow payments, refunds after an offer and bonuses are not pushed).
    Market,
    /// A new species arose from your lineage (`AlertKind::Achievement`).
    Species,
    /// A ranked season or an era ended, with your places (`AlertKind::World`).
    Season,
    /// A catastrophe began (`AlertKind::World`).
    Catastrophe,
}

impl PushKind {
    /// In order of importance (a coalesced notification leads with the
    /// most important).
    pub const ALL: [PushKind; 6] = [
        PushKind::Mating,
        PushKind::Death,
        PushKind::Market,
        PushKind::Species,
        PushKind::Season,
        PushKind::Catastrophe,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PushKind::Mating => "mating",
            PushKind::Death => "death",
            PushKind::Market => "market",
            PushKind::Species => "species",
            PushKind::Season => "season",
            PushKind::Catastrophe => "catastrophe",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The alert kind whose preferences also apply (none for mating offers:
    /// they are not alerts and always show).
    pub fn alert_kind(self) -> Option<AlertKind> {
        match self {
            PushKind::Mating => None,
            PushKind::Death => Some(AlertKind::Death),
            PushKind::Market => Some(AlertKind::Market),
            PushKind::Species => Some(AlertKind::Achievement),
            PushKind::Season | PushKind::Catastrophe => Some(AlertKind::World),
        }
    }

    /// The client tab a notification of this kind opens.
    pub fn tab(self) -> &'static str {
        match self {
            PushKind::Mating => "matings",
            PushKind::Death => "genetics",
            PushKind::Market => "market",
            PushKind::Species | PushKind::Catastrophe => "world",
            PushKind::Season => "seasons",
        }
    }
}

/// `GET` / `POST /api/push/prefs`: the push kinds a player switched off.
/// Every kind is on by default (subscribing a device is the opt-in); stored
/// per account (`Store`), global across worlds.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushPrefs {
    #[serde(default)]
    pub off: Vec<PushKind>,
}

impl PushPrefs {
    /// Sorted, without duplicates.
    pub fn normalized(mut self) -> Self {
        self.off.sort_unstable();
        self.off.dedup();
        self
    }

    pub fn wants(&self, kind: PushKind) -> bool {
        !self.off.contains(&kind)
    }
}

/// The push service a native app's device token belongs to
/// (`POST /api/push/native/subscribe`): Firebase Cloud Messaging for the
/// Android app, Apple Push Notification service for the iOS app.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NativeService {
    Fcm,
    Apns,
}

impl NativeService {
    pub const ALL: [NativeService; 2] = [NativeService::Fcm, NativeService::Apns];

    pub fn name(self) -> &'static str {
        match self {
            NativeService::Fcm => "fcm",
            NativeService::Apns => "apns",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
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
            NotPossessed(_) => "not_possessed",
            OffMap => "off_map",
            Unreachable => "unreachable",
            NoConsent => "no_consent",
            NotMature(_) => "not_mature",
            SameSex => "same_sex",
            AlreadyPregnant => "already_pregnant",
            TooFar { .. } => "too_far",
            NoSlotsLeft => "no_slots_left",
            PossessedByOther(_) => "possessed_by_other",
            NoSuchGroup(_) => "no_such_group",
            GroupLimit => "group_limit",
            GroupTooSmall(_) => "group_too_small",
            NotInGroup(_) => "not_in_group",
            SameGroup(_) => "same_group",
            NoTerritory(_) => "no_territory",
            ClaimCooldown { .. } => "claim_cooldown",
            NoRally(_) => "no_rally",
            MostlyWater { .. } => "mostly_water",
            OwnTarget(_) => "own_target",
            OutOfSight(_) => "target_out_of_sight",
            TooBig { .. } => "too_big",
            Sheltered(_) => "sheltered",
            NewcomerGrace { .. } => "newcomer_grace",
            OwnGrace { .. } => "own_grace",
        };
        let info = ErrorInfo::new(code, e.to_string());
        match e {
            ClaimCooldown { ticks_left } => info.with_detail(serde_json::json!({ "ticks_left": ticks_left })),
            MostlyWater { share } => info.with_detail(serde_json::json!({ "water_share": share })),
            TooBig { mass, limit } => info.with_detail(serde_json::json!({ "mass": mass, "limit": limit })),
            NewcomerGrace { days_left } | OwnGrace { days_left } => {
                info.with_detail(serde_json::json!({ "days_left": days_left }))
            }
            _ => info,
        }
    }
}

/// One finished mating of `GET /api/matings/history`: a closed offer (with
/// its outcome or error), or a mating within the player's own lineage (no
/// offer). Newest first; kept in memory, so it survives reloads but not a
/// server restart.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MatingRecord {
    pub tick: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offer: Option<OfferView>,
    /// The player's organism and the partner (for an offer, as in the offer).
    pub mine: OrgId,
    pub partner: OrgId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<MatingOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
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
    /// While open: the tick a pending request expires unanswered, or an
    /// accepted agreement lapses if the two haven't met (wall-clock
    /// lifetimes, `GameConfig::offer_ticks` and `purchase_deadline_ticks`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_tick: Option<u64>,
    /// An offer to any animal of this lineage (`offer_to_lineage`): `partner`
    /// is 0 until its owner accepts and an animal is picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<u32>,
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
    /// Static names (`action_name`, `sex_name`): no allocation per
    /// organism per state.
    pub action: Cow<'static, str>,
    pub sex: Cow<'static, str>,
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
    /// The group it belongs to (absent: 0, the main group).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub group: u8,
    /// Facing, one of 32 directions (`vision::heading`; absent from older
    /// servers). Own organisms only: nobody else's facing is sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<u8>,
    /// The eyes' fixed geometry for the vision cones (`vision::VisionView`;
    /// absent from older servers). Own organisms only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<VisionView>,
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// A territory a group holds.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct TerritoryView {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub claimed_tick: u64,
}

impl From<evo_sim::world::groups::Territory> for TerritoryView {
    fn from(t: evo_sim::world::groups::Territory) -> Self {
        Self { x: t.center.x, y: t.center.y, radius: t.radius, claimed_tick: t.claimed_tick }
    }
}

/// A rally a group is answering.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct RallyView {
    pub x: f32,
    pub y: f32,
    /// Members farther than this walk to the point; inside it they hold.
    pub radius: f32,
    /// Tick at which the order lapses.
    pub expires_tick: u64,
}

impl From<evo_sim::world::groups::Rally> for RallyView {
    fn from(r: evo_sim::world::groups::Rally) -> Self {
        Self { x: r.center.x, y: r.center.y, radius: r.radius, expires_tick: r.expires_tick }
    }
}

/// A player's mating-request settings ([`ClientMsg::MatingPrefs`]).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MatingPrefs {
    /// Players whose requests are refused.
    pub blocked: Vec<PlayerId>,
    /// Refuse every request.
    pub auto_decline: bool,
}

/// One of the player's groups (`World::groups`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GroupView {
    /// 0 is the main group; `split_group` makes the others.
    pub id: u8,
    /// Living organisms in it (agents; cohort members of abstract regions
    /// belong to no group).
    pub members: usize,
    pub adults: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<[f32; 2]>,
    /// The way the migration is expected to go, from the group's middle to
    /// the target (around water and thin ice), for drawing: at most
    /// `commands::ROUTE_POINTS` points; absent without a migration (or
    /// briefly, after a restart, until it is planned).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub route: Vec<[f32; 2]>,
    /// The tick the migration is ended if the group hasn't gathered at the
    /// target by then (absent: no migration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_deadline_tick: Option<u64>,
    /// Where the group settles after arriving from a migration: members
    /// outside the circle drift back into it until `expires_tick` (absent:
    /// not settling). Not a confinement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settle: Option<RallyView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub territory: Option<TerritoryView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rally: Option<RallyView>,
    /// Largest territory radius the group could claim now.
    pub max_radius: f32,
    /// First tick at which it may claim a territory again (absent: now).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub claim_ready_tick: u64,
    /// Mean obedience gene of its members, to hundredths (0 with none): how
    /// strongly the group heeds its orders against its instincts.
    #[serde(default)]
    pub obedience: f32,
    /// Its territory is the player's home range: the one shielded while
    /// they are offline (`game::offline`; absent: no).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub home_range: bool,
    /// On the home range: minutes of offline protection the player has
    /// left (at most 12 hours of protection in any 24, however often they
    /// reconnect; `game::offline`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection_left_min: Option<u32>,
}

impl From<&evo_sim::world::groups::GroupInfo> for GroupView {
    fn from(g: &evo_sim::world::groups::GroupInfo) -> Self {
        Self {
            id: g.id,
            members: g.members,
            adults: g.adults,
            migration: g.migration.map(|p| [p.x, p.y]),
            route: Vec::new(),
            migration_deadline_tick: g.migration_deadline_tick,
            settle: g.settle.map(|s| RallyView {
                x: s.center.x,
                y: s.center.y,
                radius: s.radius,
                expires_tick: s.until_tick,
            }),
            territory: g.territory.map(TerritoryView::from),
            rally: g.rally.map(RallyView::from),
            max_radius: g.max_radius,
            claim_ready_tick: g.claim_ready_tick,
            obedience: g.obedience,
            home_range: false,
            protection_left_min: None,
        }
    }
}

/// Someone else's organism inside the view window: only what can be seen.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NearbyOrganism {
    pub id: OrgId,
    pub owner: PlayerId,
    pub lineage: u32,
    pub x: f32,
    pub y: f32,
    /// Static names (`action_name`, `sex_name`): no allocation per
    /// organism per state.
    pub action: Cow<'static, str>,
    pub sex: Cow<'static, str>,
    pub matured: bool,
    /// Visible body plan, for the client's sprite.
    pub look: Appearance,
    /// `Some(true)` when its lineage is a newcomer under its grace (no other
    /// player's animal may attack it, nor it theirs); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protected: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LeaderRow {
    pub player: PlayerId,
    /// Shared: every state's leaderboard holds the same rows.
    pub name: Arc<str>,
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
    /// The main group's migration order (each group's is in `groups`).
    pub migration: Option<[f32; 2]>,
    /// The player's groups by id: the main group always, others while they
    /// have members or orders.
    #[serde(default)]
    pub groups: Vec<GroupView>,
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
    /// Your lineage's newcomer grace while it lasts: other players' animals
    /// can't attack it, and it can't attack theirs. Absent once it ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace: Option<GraceView>,
}

/// The newcomer grace left (`StateView::grace`). It lasts a number of game
/// days and at least a number of real minutes, whichever is longer.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct GraceView {
    /// Game days left, to a tenth.
    pub days_left: f32,
    /// The tick it ends on.
    pub until_tick: u64,
    /// Real seconds left at the server's clock, rounded up to tens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_seconds_left: Option<u64>,
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
    /// Whole days until winter; 0 during winter.
    #[serde(default)]
    pub days_to_winter: u32,
    /// Days in a year (four seasons of a quarter each; offers expire when the season changes).
    #[serde(default)]
    pub days_per_year: u32,
    /// Ticks in a game day.
    #[serde(default)]
    pub ticks_per_day: u32,
    /// A lineage founded now starts provisioned for winter (fed, fat
    /// reserves full: `GameConfig::winter_provision`).
    #[serde(default)]
    pub winter_provision: bool,
    /// Most living organisms (young included) one player may have: at it,
    /// births pause (`WorldConfig::lineage_cap`, scaled to the map).
    #[serde(default)]
    pub lineage_cap: usize,
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
    /// Most plant food each tile holds, 0..255 for none..the map's richest
    /// tile (absent from older servers). With the dynamic `veg` share it gives
    /// the food on a tile now.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub veg_max: String,
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
    /// Ripe fruit, seeds and nuts (`Map::fruit`), 0..255 for 0..8 food units
    /// (a full forest crop is about 6–9). Left out while no tile bears any
    /// (no fruit layer, or out of season), so older clients and payloads
    /// stay as they were.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fruit: String,
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
    /// How the legacy share comes about, from the last daily snapshot:
    /// genes, not heads (see [`ScoreBreakdown`]). Absent before the first
    /// daily snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<ScoreBreakdown>,
    /// The soft lineage cap and how crowded the lineage is now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crowding: Option<CrowdingView>,
    /// An egg-laying lineage's clutches waiting to hatch (absent for live
    /// bearers): what "pregnant" means for them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eggs: Option<EggsView>,
    /// Legacy share per recorded day, oldest first.
    pub legacy: Vec<LegacyPoint>,
    /// Recent alerts, newest last.
    pub alerts: Vec<AlertView>,
}

/// Whose genes make up a legacy share (`lineage_stats.score`). The score
/// counts alleles in breeding adults wherever they live, so a lineage's
/// share is not its head count: animals that carry a wild species' or
/// another player's alleles (a design that hybridised) score less than
/// their number, and alleles of yours in other lineages score for you.
/// Every share here is of all alleles in the world's breeding adults, as
/// the legacy share is (morphology alleles included when they count).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScoreBreakdown {
    /// The daily snapshot this is from.
    pub day: u64,
    /// The legacy share: your alleles wherever they live.
    pub share: f64,
    /// Your lineage's share of all alleles, whoever's they are: its head
    /// count among breeding adults (genomes are the same size).
    pub head_share: f64,
    /// Your alleles in your own lineage (the part of `share` at home).
    pub home_share: f64,
    /// Your alleles outside your lineage: in rivals' animals and the wild.
    pub exported_share: f64,
    /// The part of `exported_share` in other players' lineages: what the
    /// royalty pays for (alleles in the wild came from no trade).
    #[serde(default)]
    pub traded_share: f64,
    /// Others' alleles your lineage carries (`head_share - home_share`):
    /// what hybridising cost your share.
    pub foreign_share: f64,
    /// Whose alleles your lineage's breeding adults carry, largest first;
    /// `share` here is of your lineage's alleles (sums to 1).
    pub carried: Vec<GeneSource>,
    /// Where your alleles live, largest first; `share` here is of your
    /// alleles (sums to 1).
    pub placed: Vec<GeneHost>,
    /// Renown a day the royalty pays at this `traded_share`.
    pub royalty: u64,
}

/// Whose alleles: a player (0, the wild ecosystem: wild alleles carry no
/// species) and their share.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GeneSource {
    pub player: PlayerId,
    pub name: String,
    pub share: f64,
}

/// Where alleles live: a lineage (a wild species or a player's) and the
/// share there.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GeneHost {
    pub lineage: u32,
    pub lineage_name: String,
    pub owner: PlayerId,
    pub owner_name: String,
    pub share: f64,
}

/// `lineage_stats.eggs`: an egg-laying lineage's clutches.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EggsView {
    pub clutches: u32,
    pub eggs: u32,
    /// Clutches with a parent incubating and guarding them.
    pub brooded: u32,
}

/// The soft lineage cap (`lineage_stats.crowding`): births slow as the
/// crowding count nears `soft_cap` (half pace at it) and stop only at
/// `hard_cap`; past the soft cap members get hungrier too.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CrowdingView {
    /// Breeding slows from here (`WorldConfig::lineage_cap`, scaled to the
    /// map). With crowding off this is the hard cap.
    pub soft_cap: usize,
    /// No births past this many living members (juveniles count).
    pub hard_cap: usize,
    /// Adults plus juveniles at `juvenile_weight`.
    pub count: f32,
    pub juvenile_weight: f32,
    /// Litters are multiplied by this (1: no crowding).
    pub litter: f32,
    /// Hunger is multiplied by this (1 below the soft cap).
    pub hunger: f32,
}

/// `renown_shop` (and after a purchase): what renown buys.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ShopView {
    pub balance: u64,
    /// Directed-mating slots left this weather season, bought ones included.
    pub slots_left: u32,
    /// Extra slots bought this weather season.
    pub slots_bought: u32,
    /// Price of the next one (absent at the limit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_price: Option<u64>,
    /// Adults one reinforcement adds.
    pub reinforce_size: u32,
    /// Price of the next reinforcement.
    pub reinforce_price: u64,
    /// Most living members a lineage may have to be reinforced.
    pub reinforce_max_alive: usize,
    /// Why a reinforcement isn't possible now (absent when it is).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reinforce_blocked: Option<String>,
    /// Kept (destroyed) from every stud fee or price paid out, percent.
    pub market_fee_percent: u64,
    /// Royalty: renown a day per percentage point of all alleles that are
    /// yours and live in other players' lineages.
    pub royalty_per_point: f64,
    /// Reinforcements bought this ranked season, and the most allowed.
    #[serde(default)]
    pub reinforcements_bought: u32,
    #[serde(default)]
    pub reinforcements_max: u32,
    /// Cosmetic sinks: the price of a nameplate now (first or change), its
    /// palette, your nameplate, and the price of naming a species.
    #[serde(default)]
    pub nameplate_price: u64,
    #[serde(default)]
    pub nameplate_colors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nameplate: Option<crate::economy::Nameplate>,
    #[serde(default)]
    pub species_name_price: u64,
}

/// `compatibility`: which lineages yours can breed with.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CompatibilityView {
    /// Your lineage (absent: none, or no living adult to compare).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<u32>,
    /// Every other lineage with living adults, most fertile first.
    pub rows: Vec<CompatibilityRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CompatibilityRow {
    pub lineage: u32,
    pub lineage_name: String,
    /// 0: the wild ecosystem.
    pub owner: PlayerId,
    pub owner_name: String,
    /// Best body-plan fertility between a sample of your adults and theirs,
    /// 0 (sterile) to 1. A mating's fertility is also limited by the
    /// whole-genome distance (`preview_mating`).
    pub fertility: f32,
    pub compatible: bool,
    /// The body-plan gene with the widest gap in the best pair (`carnivory`,
    /// `endothermy`, `birth_mode`, `body_size`, or `morphology` for eye or
    /// leg counts), and the gap; sterile at 0.30.
    pub limiting: String,
    pub gap: f32,
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
    /// Daily breeder's royalty: renown for alleles living outside the
    /// player's lineage (`economy::royalty`).
    Royalty,
    /// Renown spent in the shop (negative): a mating slot, reinforcements.
    Purchase,
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
            LedgerKind::Royalty => "royalty",
            LedgerKind::Purchase => "purchase",
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
            LedgerKind::Royalty,
            LedgerKind::Purchase,
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
    /// Whether your lineage can breed with the listed organism at all: its
    /// best body-plan fertility with a sample of your adults is above 0
    /// (absent with no living adult of yours). A diet, warmth, birth-mode or
    /// size gap of 0.30 or more is sterile (`compatibility` says which).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatible: Option<bool>,
    /// That best body-plan fertility, 0..1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_plan_fertility: Option<f32>,
    /// `fixed` or `auction` (listings from before auctions read as fixed).
    #[serde(default = "fixed_kind")]
    pub kind: ListingKind,
    /// An auction's state (`expires_tick` is its deadline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auction: Option<AuctionView>,
}

fn fixed_kind() -> ListingKind {
    ListingKind::Fixed
}

/// What a bidder sees of an auction.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AuctionView {
    pub reserve: u64,
    /// Closes (and settles) at this tick.
    pub deadline_tick: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_bid: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_bidder: Option<PlayerId>,
    pub bids: u32,
    /// The smallest bid accepted now.
    pub min_bid: u64,
}

/// Data of `browse_market`: one page of listings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MarketView {
    pub balance: u64,
    pub escrow: u64,
    pub slots_left: u32,
    pub listings: Vec<ListingView>,
    /// Listings matching the filters, on every page.
    #[serde(default)]
    pub total: usize,
    /// Pass back as `cursor` for the next page; none on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// All the caller's own listings (`with_own`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<Vec<ListingView>>,
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
        Action::Patrol => "patrol",
        Action::Defend => "defend",
        Action::Rally => "rally",
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
            action: Cow::Borrowed(action_name(o.action)),
            sex: Cow::Borrowed(sex_name(o.sex)),
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
            group: o.group,
            heading: crate::vision::heading(o),
            // `directional` is set by the game, which knows the balance.
            vision: Some(VisionView::of(o, 0.0)),
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
            action: Cow::Borrowed(action_name(o.action)),
            sex: Cow::Borrowed(sex_name(o.sex)),
            matured: o.matured,
            look: Appearance::of(o),
            protected: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command the parser knows, read from its own "unknown variant"
    /// error, has a name in `COMMAND_NAMES` (for `bad_message` replies).
    #[test]
    fn every_command_has_a_name() {
        let err = serde_json::from_str::<ClientMsg>(r#"{"type":"no_such_command"}"#).unwrap_err().to_string();
        let list = err.split("expected one of").nth(1).expect("a variant list");
        // "`possess`, `order_move`, … `watch_season` at line 1 column 25"
        let names: std::collections::BTreeSet<&str> = list.split('`').skip(1).step_by(2).collect();
        let known: std::collections::BTreeSet<&str> = COMMAND_NAMES.iter().copied().collect();
        assert_eq!(names, known, "{err}");
        // And the web client has a label for each (`client/src/errors.ts`).
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/src/errors.ts");
        let ts = std::fs::read_to_string(path).unwrap();
        for n in COMMAND_NAMES {
            assert!(ts.contains(&format!(" {n}: '")), "errors.ts has no label for {n}");
        }
    }

    #[test]
    fn client_messages_parse() {
        let m: ClientEnvelope =
            serde_json::from_str(r#"{"id":3,"type":"order_move","organism":7,"x":1.5,"y":2}"#).unwrap();
        assert_eq!(m.id, Some(3));
        assert_eq!(m.msg, ClientMsg::OrderMove { organism: 7, x: 1.5, y: 2.0 });
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"clear_migration"}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::ClearMigration { group: None });
        assert_eq!(serde_json::to_string(&m).unwrap(), r#"{"type":"clear_migration"}"#);
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"request_mating","mine":1,"partner":2}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::RequestMating { mine: 1, partner: 2, price: None });
        assert!(serde_json::from_str::<ClientEnvelope>(r#"{"type":"teleport"}"#).is_err());
    }

    #[test]
    fn group_messages_parse() {
        let parse = |s: &str| serde_json::from_str::<ClientEnvelope>(s).map(|e| e.msg);
        // The old migrate message still reads (every group).
        assert_eq!(
            parse(r#"{"type":"migrate","x":3,"y":4}"#).unwrap(),
            ClientMsg::Migrate { x: 3.0, y: 4.0, group: None }
        );
        assert_eq!(
            parse(r#"{"type":"migrate","x":3,"y":4,"group":1}"#).unwrap(),
            ClientMsg::Migrate { x: 3.0, y: 4.0, group: Some(1) }
        );
        assert_eq!(
            parse(r#"{"type":"clear_migration","group":2}"#).unwrap(),
            ClientMsg::ClearMigration { group: Some(2) }
        );
        let claim = parse(r#"{"type":"claim_territory","x":10,"y":12.5,"radius":6}"#).unwrap();
        assert_eq!(claim, ClientMsg::ClaimTerritory { group: 0, x: 10.0, y: 12.5, radius: 6.0 });
        assert_eq!(claim.name(), "claim_territory");
        assert!(claim.is_group_change());
        assert_eq!(
            parse(r#"{"type":"claim_territory","group":1,"x":1,"y":2,"radius":3}"#).unwrap(),
            ClientMsg::ClaimTerritory { group: 1, x: 1.0, y: 2.0, radius: 3.0 }
        );
        assert!(parse(r#"{"type":"claim_territory","x":1,"y":2}"#).is_err(), "radius is required");
        assert!(parse(r#"{"type":"claim_territory","group":300,"x":1,"y":2,"radius":3}"#).is_err());
        assert_eq!(parse(r#"{"type":"abandon_territory"}"#).unwrap(), ClientMsg::AbandonTerritory { group: 0 });
        assert_eq!(parse(r#"{"type":"split_group"}"#).unwrap(), ClientMsg::SplitGroup { group: 0, organisms: None });
        assert_eq!(
            parse(r#"{"type":"split_group","group":1,"organisms":[4,5]}"#).unwrap(),
            ClientMsg::SplitGroup { group: 1, organisms: Some(vec![4, 5]) }
        );
        assert_eq!(parse(r#"{"type":"merge_groups","from":2}"#).unwrap(), ClientMsg::MergeGroups { from: 2, into: 0 });
        assert!(parse(r#"{"type":"merge_groups"}"#).is_err(), "from is required");
        assert!(!ClientMsg::Migrate { x: 0.0, y: 0.0, group: None }.is_group_change());
        let rally = parse(r#"{"type":"rally","group":2,"x":5,"y":6.5}"#).unwrap();
        assert_eq!(rally, ClientMsg::Rally { group: 2, x: 5.0, y: 6.5 });
        assert_eq!(rally.name(), "rally");
        assert!(!rally.is_group_change(), "an order, like a migration");
        assert_eq!(parse(r#"{"type":"rally","x":1,"y":2}"#).unwrap(), ClientMsg::Rally { group: 0, x: 1.0, y: 2.0 });
        assert!(parse(r#"{"type":"rally","x":1}"#).is_err(), "y is required");
        assert_eq!(parse(r#"{"type":"cancel_rally"}"#).unwrap(), ClientMsg::CancelRally { group: 0 });
    }

    #[test]
    fn group_errors_have_codes() {
        use evo_sim::world::commands::CommandError;
        let code = |e: CommandError| ErrorInfo::from(&e);
        assert_eq!(code(CommandError::NoSuchGroup(3)).code, "no_such_group");
        assert_eq!(code(CommandError::GroupLimit).code, "group_limit");
        assert_eq!(code(CommandError::GroupTooSmall(0)).code, "group_too_small");
        assert_eq!(code(CommandError::NotInGroup(9)).code, "not_in_group");
        assert_eq!(code(CommandError::SameGroup(1)).code, "same_group");
        assert_eq!(code(CommandError::NoTerritory(1)).code, "no_territory");
        assert_eq!(code(CommandError::NoRally(1)).code, "no_rally");
        let c = code(CommandError::ClaimCooldown { ticks_left: 7 });
        assert_eq!(c.code, "claim_cooldown");
        assert_eq!(c.detail, Some(serde_json::json!({ "ticks_left": 7 })));
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
        assert_eq!(m.msg, ClientMsg::BrowseMarket(MarketQuery::default()));
        assert_eq!(serde_json::to_string(&m).unwrap(), r#"{"type":"browse_market"}"#);
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"browse_market","mine":3}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::BrowseMarket(MarketQuery { mine: Some(3), ..Default::default() }));
        let m: ClientEnvelope = serde_json::from_str(
            r#"{"type":"browse_market","limit":5,"cursor":"0.12","sort":"price_asc","kind":"auction","role":"dam",
                "max_price":30,"lineage":2,"species":4,"compatible":true,"hide_own":true,"with_own":true}"#,
        )
        .unwrap();
        let q = MarketQuery {
            mine: None,
            limit: Some(5),
            cursor: Some("0.12".into()),
            sort: Some(MarketSort::PriceAsc),
            kind: Some(ListingKind::Auction),
            role: Some(ListingRole::Dam),
            max_price: Some(30),
            lineage: Some(2),
            species: Some(4),
            compatible: true,
            hide_own: true,
            with_own: true,
        };
        assert_eq!(m.msg, ClientMsg::BrowseMarket(q));
        assert!(serde_json::from_str::<ClientEnvelope>(r#"{"type":"browse_market","sort":"random"}"#).is_err());
        let m: ClientEnvelope =
            serde_json::from_str(r#"{"type":"create_auction","organism":9,"reserve":20,"ticks":96}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::CreateAuction { organism: 9, reserve: 20, days: None, ticks: Some(96) });
        assert_eq!(m.msg.name(), "create_auction");
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"bid","listing":4,"amount":25,"mine":7}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::Bid { listing: 4, amount: 25, mine: 7 });
        assert!(serde_json::from_str::<ClientEnvelope>(r#"{"type":"bid","listing":4,"amount":25}"#).is_err());
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
    /// Mean expressed thermoregulation and birth mode genes of its
    /// representatives, and their usual leg and eye counts: with `carnivory`
    /// and `body_size`, what decides who it can interbreed with
    /// (`mating::body_plan_gap`). Absent from older servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endothermy: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth_mode: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leg_count: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eye_count: Option<u8>,
}

/// `GET /api/lineage/tree`: what the caller's lineage looked like over
/// time and the species it split into (the family tree view). Built from
/// the daily history and the species tracker (`game::family`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FamilyTree {
    pub player: PlayerId,
    pub lineage: Option<u32>,
    pub name: Option<String>,
    pub day: u64,
    /// Living members now (0: the lineage died out).
    pub alive: usize,
    /// Oldest first: every recorded day with gene means for the lineage,
    /// then today's living members.
    pub timeline: Vec<FamilySample>,
    /// Species the lineage's members belong or belonged to, with their
    /// ancestors, oldest first.
    pub species: Vec<FamilySpecies>,
}

/// The lineage on one day.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FamilySample {
    pub day: u64,
    pub alive: usize,
    /// Mean expressed value of every gene, by wire name (3 decimals).
    pub genes: BTreeMap<String, f32>,
    /// The typical body plan (absent in records from before it was kept).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub morph: Option<BodyPlanMode>,
}

/// One species on the family tree.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FamilySpecies {
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
    /// Estimated breeding adults at the last detection.
    pub adults: f64,
    /// Of them, the caller's lineage's (0: an ancestor only).
    pub mine: f64,
    /// Mean expressed genes of its representative members (3 decimals).
    pub genes: BTreeMap<String, f32>,
    /// Counts and shapes from its representatives; placement, snout, horns,
    /// wing shape and pattern from the lineage's record nearest its last
    /// sighting (the species tracker keeps only the core body plan).
    pub morph: BodyPlanMode,
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
