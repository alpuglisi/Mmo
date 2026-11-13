//! Game state around the [`World`]: player names, founded lineages,
//! directed-mating offers, connected views. Pure logic with no channels or
//! I/O, so command routing and the consent flow are unit-testable; the world
//! thread ([`crate::runner`]) owns a `Game` and moves its output to sockets.
//!
//! Every method that changes the world is called by the world thread
//! between ticks, in the order requests arrived.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use evo_sim::design::DesignError;
use evo_sim::math::Vec2;
use evo_sim::presets::{self, REFERENCE_SIZE};
use evo_sim::organism::{DeathCause, Sex};
use evo_sim::terrain::Season;
use evo_sim::world::commands::{CommandError, DirectedMating, MEETING_DISTANCE};
use evo_sim::{FounderDesign, Gene, PlayerId, World, WorldConfig, WILD};

use crate::design::gene_key;
use crate::protocol::*;
use crate::store::{DailyRecord, LegacyEntry, LineageEntry, TradeRecord};

/// Daily records kept in memory for the genetics dashboard.
pub const HISTORY_DAYS: usize = 365;
/// Alerts kept per player.
pub const MAX_ALERTS: usize = 50;
/// Thirst or hunger at which a possessed organism raises a `need` alert.
pub const NEED_ALERT: f32 = 75.0;
const POPULATION_MILESTONES: [usize; 7] = [25, 50, 100, 250, 500, 1000, 1500];
const SHARE_MILESTONES: [f64; 5] = [0.01, 0.05, 0.1, 0.25, 0.5];

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

/// Records for the store, drained by the world thread.
#[derive(Clone, Debug, PartialEq)]
pub enum PersistEvent {
    Daily(DailyRecord),
    Trade(TradeRecord),
}

#[derive(Clone, Debug)]
pub struct GameConfig {
    /// Half-side of the default view window, in tiles.
    pub default_view_radius: f32,
    pub max_view_radius: f32,
    /// Most foreign organisms sent per state message (nearest first).
    pub max_nearby: usize,
    /// Most open offers one player may have made at once.
    pub max_open_offers: usize,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self { default_view_radius: 16.0, max_view_radius: 32.0, max_nearby: 256, max_open_offers: 8 }
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
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self { seed: 1, width: 128, height: 128, wild: true, days_per_year: WorldConfig::default().days_per_year, lod: false }
    }
}

/// A world with the wild ecosystem scaled to its area (as
/// `presets::standard_world` does) and no player lineages.
pub fn build_world(s: &WorldSettings) -> World {
    let mut config =
        WorldConfig { seed: s.seed, width: s.width, height: s.height, days_per_year: s.days_per_year.max(4), ..Default::default() };
    let area = (s.width * s.height) as f32 / (REFERENCE_SIZE * REFERENCE_SIZE) as f32;
    config.wild_cap = ((config.wild_cap as f32 * area) as usize).max(50);
    config.lineage_cap = ((config.lineage_cap as f32 * area.max(0.5)) as usize).max(50);
    config.lod.enabled = s.lod;
    let mut world = World::new(config);
    if s.wild {
        for (owner, design) in presets::wild_designs() {
            let n = ((design.founders as f32 * area).round() as u32).max(4);
            world.found_lineage_unchecked(owner, &design.founders(n));
        }
    }
    world
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

#[derive(Clone, Debug)]
struct Offer {
    id: u64,
    from: PlayerId,
    to: PlayerId,
    mine: OrgId,
    partner: OrgId,
    price: Option<u64>,
    status: OfferStatus,
    created_tick: u64,
}

struct Conn {
    player: PlayerId,
    view: Option<ViewWindow>,
}

/// Per-player progress for milestone alerts.
#[derive(Default)]
struct Progress {
    population: usize,
    generation: u32,
    share: f64,
    extinct: bool,
}

/// Why an organism is watched for its death.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Watch {
    Founder,
    Possessed,
}

pub struct Game {
    pub world: World,
    cfg: GameConfig,
    names: BTreeMap<PlayerId, String>,
    lineage_of: BTreeMap<PlayerId, u32>,
    /// Standing migration orders (mirrors the world's, which are private).
    migrations: BTreeMap<PlayerId, Vec2>,
    offers: BTreeMap<u64, Offer>,
    next_offer: u64,
    conns: BTreeMap<ConnId, Conn>,
    last_legacy_day: Option<u64>,
    last_season: Season,
    persist: Vec<PersistEvent>,
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
        Self {
            world,
            cfg,
            names: BTreeMap::new(),
            lineage_of: BTreeMap::new(),
            migrations: BTreeMap::new(),
            offers: BTreeMap::new(),
            next_offer: 1,
            conns: BTreeMap::new(),
            last_legacy_day,
            last_season,
            persist: Vec::new(),
            history: VecDeque::new(),
            prev_born: BTreeMap::new(),
            prev_deaths: BTreeMap::new(),
            alerts: BTreeMap::new(),
            watched: BTreeMap::new(),
            progress: BTreeMap::new(),
            need_alerted: BTreeMap::new(),
            focused: BTreeSet::new(),
        }
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
        Ok(FoundOk { lineage, founders, cost: design.point_cost() })
    }

    pub fn connect(&mut self, conn: ConnId, player: PlayerId, name: &str) -> Outbox {
        self.set_name(player, name);
        self.conns.insert(conn, Conn { player, view: None });
        let mut out = vec![(
            Target::Conn(conn),
            ServerMsg::Welcome { player, name: name.into(), lineage: self.lineage_of(player) },
        )];
        if let Some(s) = self.state_for(conn) {
            out.push((Target::Conn(conn), ServerMsg::State(Box::new(s))));
        }
        out
    }

    pub fn disconnect(&mut self, conn: ConnId) {
        self.conns.remove(&conn);
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

    fn apply(&mut self, conn: ConnId, player: PlayerId, msg: ClientMsg, out: &mut Outbox) -> Result<Option<serde_json::Value>, ErrorInfo> {
        let e = |err: CommandError| ErrorInfo::from(&err);
        match msg {
            ClientMsg::Possess { organism } => self.world.possess(player, organism).map(|_| None).map_err(e),
            ClientMsg::OrderMove { organism, x, y } => {
                finite(&[x, y])?;
                self.world.order_move(player, organism, Vec2::new(x, y)).map_err(e)?;
                Ok(None)
            }
            ClientMsg::Release { organism } => self.world.release(player, organism).map(|_| None).map_err(e),
            ClientMsg::Migrate { x, y } => {
                finite(&[x, y])?;
                let target = self.world.map.clamp(Vec2::new(x, y));
                self.world.command_migrate(player, target);
                self.migrations.insert(player, target);
                Ok(Some(serde_json::json!({ "x": target.x, "y": target.y })))
            }
            ClientMsg::ClearMigration => {
                self.world.clear_migration(player);
                self.migrations.remove(&player);
                Ok(None)
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
        }
    }

    fn offer_for(&self, request: u64, player: PlayerId) -> Result<&Offer, ErrorInfo> {
        self.offers
            .get(&request)
            .filter(|o| o.from == player || o.to == player)
            .ok_or_else(|| ErrorInfo::new("unknown_offer", format!("no offer {request} involving you")))
    }

    fn request_mating(&mut self, player: PlayerId, mine: OrgId, partner: OrgId, price: Option<u64>, out: &mut Outbox) -> Result<Option<serde_json::Value>, ErrorInfo> {
        let e = |err: CommandError| ErrorInfo::from(&err);
        let a = self.world.organism(mine).ok_or(e(CommandError::NotFound(mine)))?;
        if a.owner != player {
            return Err(e(CommandError::NotOwned(mine)));
        }
        let b = self.world.organism(partner).ok_or(e(CommandError::NotFound(partner)))?;
        let to = b.owner;
        if to == player {
            // Within the player's own organisms: no consent needed.
            let req = DirectedMating { player, a: mine, b: partner, partner_consent: false };
            let outcome = self.world.directed_mating(req).map_err(e)?;
            return Ok(Some(serde_json::to_value(MatingOutcome::from(outcome)).unwrap()));
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
        if self.world.mating_slots_left(player) == 0 {
            return Err(e(CommandError::NoSlotsLeft));
        }
        let open = self.offers.values().filter(|o| o.from == player && o.status.is_open()).count();
        if open >= self.cfg.max_open_offers {
            return Err(ErrorInfo::new("too_many_offers", format!("at most {} open offers", self.cfg.max_open_offers)));
        }
        let id = self.next_offer;
        self.next_offer += 1;
        let offer = Offer { id, from: player, to, mine, partner, price, status: OfferStatus::Pending, created_tick: self.world.tick() };
        self.offers.insert(id, offer);
        let view = self.offer_view(id);
        out.push((Target::Player(to), ServerMsg::MatingRequest { offer: view.clone() }));
        Ok(Some(serde_json::json!({ "offer": view })))
    }

    fn respond_mating(&mut self, player: PlayerId, request: u64, accept: bool, out: &mut Outbox) -> Result<Option<serde_json::Value>, ErrorInfo> {
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
        let result = if partner_ok {
            self.world.directed_mating(DirectedMating { player: o.from, a: o.mine, b: o.partner, partner_consent: true })
        } else {
            Err(CommandError::NotFound(o.partner))
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
                    out.push((Target::Player(p), ServerMsg::MatingUpdate { offer: view.clone(), outcome: None, error: Some(info.clone()) }));
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

    fn close(&mut self, request: u64, status: OfferStatus, outcome: Option<MatingOutcome>, error: Option<ErrorInfo>, out: &mut Outbox) {
        let o = self.offers.get_mut(&request).unwrap();
        o.status = status;
        let o = o.clone();
        let view = self.offer_view(request);
        for p in [o.from, o.to] {
            out.push((Target::Player(p), ServerMsg::MatingUpdate { offer: view.clone(), outcome, error: error.clone() }));
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
        }));
        // Closed offers stay only until reported; keep the table small.
        self.offers.remove(&request);
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
        }
    }

    /// Bookkeeping after `World::step`: complete agreements whose organisms
    /// have met, expire offers at a season change, record a new legacy day.
    pub fn after_tick(&mut self) -> Outbox {
        let mut out = Outbox::new();
        let accepted: Vec<u64> = self.offers.values().filter(|o| o.status == OfferStatus::Accepted).map(|o| o.id).collect();
        for id in accepted {
            let o = &self.offers[&id];
            match (self.world.organism(o.mine), self.world.organism(o.partner)) {
                (Some(a), Some(b)) if a.pos.dist(b.pos) <= MEETING_DISTANCE => {
                    let _ = self.execute(id, &mut out);
                }
                (Some(_), Some(_)) => {}
                _ => {
                    let missing = if self.world.organism(o.mine).is_none() { o.mine } else { o.partner };
                    let info = ErrorInfo::from(&CommandError::NotFound(missing));
                    self.close(id, OfferStatus::Failed, None, Some(info), &mut out);
                }
            }
        }
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
        if self.world.tick().is_multiple_of(self.world.config.ticks_per_hour()) {
            self.sync_focus();
        }
        let legacy_day = self.world.latest_legacy().map(|l| l.day);
        if legacy_day != self.last_legacy_day {
            self.last_legacy_day = legacy_day;
            let record = self.daily_record();
            self.daily_alerts(&record, &mut out);
            self.history.push_back(record.clone());
            if self.history.len() > HISTORY_DAYS {
                self.history.pop_front();
            }
            self.persist.push(PersistEvent::Daily(record));
        }
        out
    }

    fn alert(&mut self, player: PlayerId, kind: AlertKind, text: String, organism: Option<OrgId>, out: &mut Outbox) {
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
        self.watched.retain(|id, (_, w)| *w == Watch::Founder || world.organism(*id).is_none_or(|o| o.possessed_by.is_some()));
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
            self.world.abstract_counts().iter().zip(&self.world.lineages).filter(|(_, l)| l.owner == player).map(|(c, _)| c.alive).sum()
        } else {
            0
        };
        self.world.owner_count(player) + cohorts
    }

    /// Where a player's lineage lives: the mean position of its agents, or,
    /// with none (all of it abstract), of its cohorts by region; the map's
    /// center for a player with neither.
    fn lineage_center(&self, player: PlayerId) -> Vec2 {
        let (mut sum, mut n) = (Vec2::ZERO, 0.0f32);
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner == player) {
            sum += o.pos;
            n += 1.0;
        }
        if n == 0.0 {
            let m = &self.world.map;
            let chunk = evo_sim::terrain::CHUNK;
            for (r, c) in self.world.cohorts().filter(|(_, c)| c.owner == player) {
                let k = c.count() as f32;
                let center = Vec2::new(((r % m.chunks_x) * chunk) as f32 + chunk as f32 / 2.0, ((r / m.chunks_x) * chunk) as f32 + chunk as f32 / 2.0);
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
            let possessed = self.world.organisms.iter().find(|o| o.alive && o.possessed_by == Some(c.player) && o.owner == c.player);
            let center = possessed.map(|o| o.pos).unwrap_or_else(|| self.lineage_center(c.player));
            ViewWindow { x: center.x, y: center.y, radius: self.cfg.default_view_radius }
        })
    }

    fn announce_deaths(&mut self, out: &mut Outbox) {
        let dead: Vec<(OrgId, PlayerId, Watch)> =
            self.watched.iter().filter(|(id, _)| self.world.organism(**id).is_none()).map(|(&id, &(p, w))| (id, p, w)).collect();
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

    /// Births and deaths per lineage since the last day boundary.
    fn vitals_since_boundary(&self) -> (BTreeMap<u32, u64>, BTreeMap<u32, BTreeMap<String, u64>>) {
        let c = &self.world.counters;
        let born = c.born.iter().map(|(&l, &n)| (l, n.saturating_sub(self.prev_born.get(&l).copied().unwrap_or(0)))).collect();
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
                Some((n, s)) if *n > 0 => Gene::ALL.iter().zip(s).map(|(&g, v)| (gene_key(g), (v / *n as f64) as f32)).collect(),
                _ => BTreeMap::new(),
            }
        };
        DailyRecord {
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
            Some(l) if self.world.config.lod.enabled => self.world.abstract_counts().get(l as usize).copied().unwrap_or_default(),
            _ => Default::default(),
        };
        // Age bands: up to six equal bands (whole days) up to the oldest.
        let oldest = members.iter().map(|o| o.age_days(tpd)).fold(0.0f32, f32::max);
        let width = (oldest / 6.0).ceil().max(1.0);
        let bands = if members.is_empty() { 0 } else { ((oldest / width) as usize + 1).min(6) };
        let mut age_bands: Vec<AgeBand> =
            (0..bands).map(|k| AgeBand { from_days: k as f32 * width, to_days: (k + 1) as f32 * width, female: 0, male: 0 }).collect();
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
            vitals.push(DayVitals { day, births: born.get(&l).copied().unwrap_or(0), deaths: deaths.remove(&l).unwrap_or_default() });
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
                GeneStat { gene: gene_key(g), mean: mean as f32, sd: var.sqrt() as f32, founding_mean, history }
            })
            .collect();
        genes.sort_by(|a, b| (b.mean - b.founding_mean).abs().total_cmp(&(a.mean - a.founding_mean).abs()).then(a.gene.cmp(&b.gene)));
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
                name: self.name(p),
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
        }
    }

    pub fn weather(&self) -> WeatherView {
        let w = &self.world;
        WeatherView { mean_wind: w.weather.wind, storm: w.weather.storm(), drought: w.map.drought, flood: w.map.flood }
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

    /// The state message for one connection: the player's organisms in full,
    /// and other organisms inside the view window only.
    pub fn state_for(&self, conn: ConnId) -> Option<StateView> {
        let c = self.conns.get(&conn)?;
        let player = c.player;
        let tpd = self.world.config.ticks_per_day;
        let own: Vec<OwnOrganism> =
            self.world.organisms.iter().filter(|o| o.alive && o.owner == player).map(|o| OwnOrganism::from_org(o, tpd)).collect();
        let view = self.view_of(c);
        let center = Vec2::new(view.x, view.y);
        let mut nearby: Vec<(f32, NearbyOrganism)> = self
            .world
            .organisms
            .iter()
            .filter(|o| o.alive && o.owner != player)
            .filter(|o| (o.pos.x - view.x).abs() <= view.radius && (o.pos.y - view.y).abs() <= view.radius)
            .map(|o| (o.pos.dist(center), NearbyOrganism::from_org(o)))
            .collect();
        if nearby.len() > self.cfg.max_nearby {
            nearby.sort_by(|a, b| a.0.total_cmp(&b.0));
            nearby.truncate(self.cfg.max_nearby);
        }
        let offers = self
            .offers
            .values()
            .filter(|o| o.status.is_open() && (o.from == player || o.to == player))
            .map(|o| self.offer_view(o.id))
            .collect();
        Some(StateView {
            clock: self.clock(),
            weather: self.weather(),
            player,
            lineage: self.lineage_of(player),
            slots_left: self.world.mating_slots_left(player),
            population: self.owner_population(player),
            migration: self.migrations.get(&player).map(|p| [p.x, p.y]),
            view,
            organisms: own,
            nearby: nearby.into_iter().map(|(_, n)| n).collect(),
            leaderboard: self.leaderboard(),
            offers,
        })
    }

    /// A state message for every connection.
    pub fn states(&self) -> Outbox {
        self.conns
            .keys()
            .filter_map(|&c| self.state_for(c).map(|s| (Target::Conn(c), ServerMsg::State(Box::new(s)))))
            .collect()
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
        let over = design("big").with(evo_sim::Gene::Wings, 1.0).with(evo_sim::Gene::Armor, 1.0).with(evo_sim::Gene::Toxins, 1.0).founders(300);
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
        let out = g.command(10, 1, None, ClientMsg::Release { organism: a[0] });
        assert_eq!(result(&out), (true, None));
        let out = g.command(10, 1, None, ClientMsg::Migrate { x: 5.0, y: 5.0 });
        assert_eq!(result(&out), (true, None));
        let out = g.command(10, 1, None, ClientMsg::ClearMigration);
        assert_eq!(result(&out), (true, None));
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
        let too_far = out.iter().any(|(_, m)| matches!(m, ServerMsg::MatingUpdate { error: Some(e), .. } if e.code == "too_far"));
        assert!(too_far, "{out:?}");
        assert_eq!(g.state_for(10).unwrap().offers[0].status, OfferStatus::Accepted);
        let out = g.command(10, 1, None, ClientMsg::CompleteMating { request: offer.id });
        assert_eq!(result(&out).1.as_deref(), Some("too_far"));
        // They meet: the next tick's bookkeeping runs the mating.
        place_together(&mut g, fa, mb);
        let out = g.after_tick();
        let done = out.iter().find_map(|(t, m)| match m {
            ServerMsg::MatingUpdate { offer, outcome: Some(o), .. } if *t == Target::Player(1) => Some((offer.status, *o)),
            _ => None,
        });
        let (status, outcome) = done.expect("completion reported to the requester");
        assert_eq!(status, OfferStatus::Completed);
        assert!(outcome.fertility > 0.0);
        assert!(out.iter().any(|(t, m)| *t == Target::Player(2) && matches!(m, ServerMsg::MatingUpdate { .. })));
        assert_eq!(g.world.mating_slots_left(1), slots - 1);
        assert!(g.state_for(10).unwrap().offers.is_empty());
        let trades = g.take_persist();
        assert!(trades.iter().any(|t| matches!(t, PersistEvent::Trade(r) if r.price == Some(30) && r.status == "completed")));
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
        assert!(out.iter().any(|(t, m)| *t == Target::Player(1) && matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Declined)));
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
        assert!(out.iter().any(|(_, m)| matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Completed)), "{out:?}");
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
        let world = build_world(&WorldSettings { seed: 3, width: 64, height: 64, wild: true, days_per_year: 8, lod: false });
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
            expired |= out.iter().any(|(_, m)| matches!(m, ServerMsg::MatingUpdate { offer, .. } if offer.status == OfferStatus::Expired));
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
        let sim_died: u64 = g.world.counters.deaths_by_lineage.iter().filter(|((l, _), _)| *l == 0).map(|(_, n)| n).sum();
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
    }

    #[test]
    fn possessed_needs_raise_an_alert_and_the_sim_lets_severe_ones_win() {
        let (mut g, a, _) = game();
        let id = a[0];
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
        let world = build_world(&WorldSettings { seed: 7, width: 128, height: 128, wild: true, lod: true, ..Default::default() });
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
}
