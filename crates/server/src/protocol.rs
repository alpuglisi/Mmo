//! JSON wire types shared by the HTTP API and the WebSocket protocol.
//!
//! Every WebSocket message is a JSON object with a `type` field in
//! `snake_case`. Client messages may carry an `id`, echoed in the matching
//! [`ServerMsg::Result`] so a client can pair replies with requests.

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
    /// Return a possessed organism to its genetic behavior.
    Release { organism: OrgId },
    /// Propose a directed mating between your organism `mine` and `partner`.
    /// Pairs within your own lineage run at once; another player's organism
    /// needs that player's consent first. `price` is recorded with the
    /// agreement (no economy yet).
    RequestMating {
        mine: OrgId,
        partner: OrgId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        price: Option<u64>,
    },
    /// The partner organism's owner accepts or declines an offer.
    RespondMating { request: u64, accept: bool },
    /// Retry an accepted agreement now (it also completes on its own once the
    /// two organisms meet).
    CompleteMating { request: u64 },
    /// Either side withdraws an open offer or agreement.
    CancelMating { request: u64 },
    /// Group command: your adults drift toward a point.
    Migrate { x: f32, y: f32 },
    ClearMigration,
    /// Pin the view window (interest area) to a point.
    SetView { x: f32, y: f32, radius: f32 },
    /// Let the view window follow your possessed organism or your lineage.
    ClearView,
}

impl ClientMsg {
    pub fn name(&self) -> &'static str {
        match self {
            ClientMsg::Possess { .. } => "possess",
            ClientMsg::OrderMove { .. } => "order_move",
            ClientMsg::Release { .. } => "release",
            ClientMsg::RequestMating { .. } => "request_mating",
            ClientMsg::RespondMating { .. } => "respond_mating",
            ClientMsg::CompleteMating { .. } => "complete_mating",
            ClientMsg::CancelMating { .. } => "cancel_mating",
            ClientMsg::Migrate { .. } => "migrate",
            ClientMsg::ClearMigration => "clear_migration",
            ClientMsg::SetView { .. } => "set_view",
            ClientMsg::ClearView => "clear_view",
        }
    }
}

/// A server → client WebSocket message.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome { player: PlayerId, name: String, lineage: Option<u32> },
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
    MatingRequest { offer: OfferView },
    /// A directed-mating offer changed status (sent to both sides).
    MatingUpdate {
        offer: OfferView,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<MatingOutcome>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorInfo>,
    },
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
}

impl ErrorInfo {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
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
        Self { mismatch: o.mismatch, cost: o.cost, failure_chance: o.failure_chance, fertility: o.fertility, conceived: o.conceived }
    }
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
    /// The mating was attempted and refused by the rules (not mature, …).
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
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClockView {
    pub tick: u64,
    pub day: u64,
    pub year: u64,
    pub hour: u32,
    pub season: String,
    pub daytime: bool,
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
    pub migration: Option<[f32; 2]>,
    pub view: ViewWindow,
    pub organisms: Vec<OwnOrganism>,
    pub nearby: Vec<NearbyOrganism>,
    pub leaderboard: Leaderboard,
    pub offers: Vec<OfferView>,
}

/// World summary for `GET /api/world`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorldSummary {
    pub clock: ClockView,
    pub weather: WeatherView,
    pub width: usize,
    pub height: usize,
    pub organisms: usize,
    pub lineages: usize,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_messages_parse() {
        let m: ClientEnvelope = serde_json::from_str(r#"{"id":3,"type":"order_move","organism":7,"x":1.5,"y":2}"#).unwrap();
        assert_eq!(m.id, Some(3));
        assert_eq!(m.msg, ClientMsg::OrderMove { organism: 7, x: 1.5, y: 2.0 });
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"clear_migration"}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::ClearMigration);
        let m: ClientEnvelope = serde_json::from_str(r#"{"type":"request_mating","mine":1,"partner":2}"#).unwrap();
        assert_eq!(m.msg, ClientMsg::RequestMating { mine: 1, partner: 2, price: None });
        assert!(serde_json::from_str::<ClientEnvelope>(r#"{"type":"teleport"}"#).is_err());
    }

    #[test]
    fn server_messages_are_tagged() {
        let s = serde_json::to_value(ServerMsg::ok(Some(1), "possess", None)).unwrap();
        assert_eq!(s["type"], "result");
        assert_eq!(s["ok"], true);
        assert_eq!(s["id"], 1);
    }
}
