//! Game state around the [`World`]: player names, founded lineages,
//! directed-mating offers, connected views. Pure logic with no channels or
//! I/O, so command routing and the consent flow are unit-testable; the world
//! thread ([`crate::runner`]) owns a `Game` and moves its output to sockets.
//!
//! Every method that changes the world is called by the world thread
//! between ticks, in the order requests arrived.

use std::collections::BTreeMap;

use evo_sim::design::DesignError;
use evo_sim::math::Vec2;
use evo_sim::presets::{self, REFERENCE_SIZE};
use evo_sim::terrain::Season;
use evo_sim::world::commands::{CommandError, DirectedMating, MEETING_DISTANCE};
use evo_sim::{FounderDesign, PlayerId, World, WorldConfig, WILD};

use crate::protocol::*;
use crate::store::{DailyRecord, LegacyEntry, LineageEntry, TradeRecord};

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
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self { seed: 1, width: 128, height: 128, wild: true, days_per_year: WorldConfig::default().days_per_year }
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
        let legacy_day = self.world.latest_legacy().map(|l| l.day);
        if legacy_day != self.last_legacy_day {
            self.last_legacy_day = legacy_day;
            self.persist.push(PersistEvent::Daily(self.daily_record()));
        }
        out
    }

    fn daily_record(&self) -> DailyRecord {
        let board = self.leaderboard();
        let counts = self.world.lineage_counts();
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
                .map(|(k, (l, c))| LineageEntry { lineage: k as u32, name: l.name.clone(), owner: l.owner, alive: c.alive, adults: c.adults })
                .collect(),
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
                alive: self.world.owner_count(p),
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
            organisms: self.world.alive_count(),
            lineages: self.world.lineages.len(),
        }
    }

    /// The state message for one connection: the player's organisms in full,
    /// and other organisms inside the view window only.
    pub fn state_for(&self, conn: ConnId) -> Option<StateView> {
        let c = self.conns.get(&conn)?;
        let player = c.player;
        let tpd = self.world.config.ticks_per_day;
        let mut own = Vec::new();
        let (mut sum, mut possessed) = (Vec2::ZERO, None);
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner == player) {
            sum += o.pos;
            if o.possessed_by == Some(player) && possessed.is_none() {
                possessed = Some(o.pos);
            }
            own.push(OwnOrganism::from_org(o, tpd));
        }
        let view = c.view.unwrap_or_else(|| {
            let center = possessed.unwrap_or_else(|| {
                if own.is_empty() {
                    Vec2::new(self.world.map.width as f32 / 2.0, self.world.map.height as f32 / 2.0)
                } else {
                    sum * (1.0 / own.len() as f32)
                }
            });
            ViewWindow { x: center.x, y: center.y, radius: self.cfg.default_view_radius }
        });
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
        let world = build_world(&WorldSettings { seed: 3, width: 64, height: 64, wild: true, days_per_year: 8 });
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
}
