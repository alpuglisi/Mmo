//! The world and its tick loop.
//!
//! One tick:
//! 0. With level of detail on (`lod`), on the hour: expand regions that came
//!    into focus; on the day: collapse regions out of focus into cohorts and
//!    advance every cohort one day.
//! 1. On the hour: regional weather, clutches. Every tick: a share of the
//!    environment chunks (temperature, snow, ice, water levels, regrowth),
//!    so each chunk updates once per simulated hour.
//! 2. On the day: remove the dead, recompute group traits, take a legacy
//!    snapshot, reset mating slots at a season change.
//! 3. Rebuild the spatial index.
//! 4. **Decide** (parallel, read-only): every organism scores its actions by
//!    need urgency × behavior genes and picks one.
//! 5. **Apply** (shuffled order): movement (steering, with A* routes
//!    around blocked terrain; see `pathfinding`), eating, predation, mating,
//!    needs, death and birth. Movement and needs run on all cores, the
//!    interactions sequentially (`par_apply`; `parallel_apply = false` for
//!    the fully sequential apply).
//!
//! All randomness comes from per-(tick, organism) streams, so a run is
//! reproducible from its seed regardless of thread count.

mod apply;
pub mod commands;
mod decide;
mod density;
mod disease;
mod environment;
pub mod lod;
mod par_apply;
#[cfg(test)]
mod group_tests;
#[cfg(test)]
mod path_tests;
mod seasonal;
mod spatial;

use std::collections::{BTreeMap, HashMap};

use rand::seq::SliceRandom;
use rand::Rng;
use rayon::prelude::*;

use crate::config::WorldConfig;
use crate::design::{DesignError, FounderDesign};
use crate::genome::{Genome, PlayerId};
use crate::group::{GroupField, GroupTrait};
use crate::legacy::LegacySnapshot;
use crate::math::Vec2;
use crate::organism::{DeathCause, OrgId, Organism, Sex};
use crate::pathfinding::PathScratch;
use crate::rng::{rng_for, stream};
use crate::terrain::{traverse, Map, Season, Traverse, Weather};

use density::SpeciesDensity;
pub use lod::{Cohort, LodStats, PoolEntry};
use spatial::SpatialGrid;

/// Founding: longest walk (tiles) from a home to a spot where it can drink
/// (water within 6 tiles, as drinking reaches the next tile).
const WATER_STEPS: usize = 5;
/// Founding: radius in tiles of a hunter's prey count.
const PREY_RANGE: f32 = 16.0;
/// Founding: prey this close to a hunter's home counts against it, so hunters
/// don't start on top of a prey group.
const PREY_ON_TOP: f32 = 8.0;
/// Founding: share of a prey lineage's founders a hunter's home may count.
const PREY_SHARE: f32 = 0.15;

/// Eggs waiting to hatch.
#[derive(Clone, Debug)]
pub struct Clutch {
    pub pos: Vec2,
    pub owner: PlayerId,
    pub mother: OrgId,
    pub father: OrgId,
    pub generation: u32,
    pub lineage: u32,
    pub eggs: Vec<Genome>,
    pub hatch_tick: u64,
    /// Incubated by the mother; `None` for abandoned eggs.
    pub guardian: Option<OrgId>,
    /// Eggs of warm-blooded parents need more heat.
    pub warm_blooded: bool,
    /// Accumulated chilling; the clutch dies at 1.
    pub chill: f32,
    pub newborn_growth: f32,
    /// Mass of one egg in kg (food value when raided).
    pub egg_mass: f32,
}

/// A founding population and its descendants.
#[derive(Clone, Debug, PartialEq)]
pub struct Lineage {
    pub name: String,
    pub owner: PlayerId,
    pub founders: u32,
}

/// Living members of one lineage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineageCount {
    pub alive: usize,
    pub adults: usize,
}

/// Cumulative event counters.
#[derive(Clone, Debug, Default)]
pub struct Counters {
    pub births: u64,
    pub hatched: u64,
    pub eggs_lost: u64,
    pub deaths: BTreeMap<DeathCause, u64>,
    pub deaths_by_lineage: BTreeMap<(u32, DeathCause), u64>,
    /// Deaths before reaching breeding age, per lineage.
    pub juvenile_deaths: BTreeMap<u32, u64>,
    /// Newborns and hatchlings per lineage.
    pub born: BTreeMap<u32, u64>,
    pub matings: u64,
    /// Conceptions between parents of different founding lineages.
    pub cross_lineage_matings: u64,
    /// Offspring (newborns or eggs) of cross-lineage matings, by
    /// (mother's lineage, father's lineage).
    pub hybrids: BTreeMap<(u32, u32), u64>,
    pub directed_matings: u64,
    pub kills: u64,
    pub failed_hunts: u64,
    /// New mating-transmitted infections per lineage (emerged or caught).
    pub infections: BTreeMap<u32, u64>,
    /// Path searches run (`World::move_toward`), and how many found no route.
    pub path_searches: u64,
    pub path_failures: u64,
}

pub struct World {
    pub config: WorldConfig,
    pub map: Map,
    /// Map-wide mean weather, for reports. Movement rules use the local
    /// weather, `map.weather_at(pos)`.
    pub weather: Weather,
    pub organisms: Vec<Organism>,
    pub clutches: Vec<Clutch>,
    pub group: GroupField,
    pub counters: Counters,
    pub legacy: Vec<LegacySnapshot>,
    pub lineages: Vec<Lineage>,
    tick: u64,
    next_id: OrgId,
    index: HashMap<OrgId, usize>,
    grid: SpatialGrid,
    owner_counts: BTreeMap<PlayerId, usize>,
    /// Living organisms per lineage, indexed like `lineages`.
    lineage_alive: Vec<usize>,
    density: SpeciesDensity,
    migrate_orders: BTreeMap<PlayerId, Vec2>,
    slots_used: BTreeMap<PlayerId, u32>,
    clutch_at: HashMap<(usize, usize), usize>,
    path_scratch: PathScratch,
    /// Seasonal part of today's temperature (`Map::seasonal_offset`).
    season_offset: f32,
    /// Tile → organism index of every food cache (`seasonal::rebuild_caches`).
    cache_at: HashMap<(usize, usize), usize>,
    /// Shared caches by (group region, lineage), and tile → key for raids
    /// (`seasonal`, `GroupTrait::SharedCaches`).
    shared_caches: BTreeMap<(usize, u32), seasonal::SharedCache>,
    shared_cache_at: HashMap<(usize, usize), (usize, u32)>,
    /// Winter-dormant communal-den carriers per den cell (`seasonal`).
    den_cells: HashMap<(usize, usize), u16>,
    /// Level of detail: focus points and the cohorts of abstract regions.
    lod: lod::Lod,
    /// Wall-clock time spent in each phase of `step` (for `bench`).
    pub phase_times: PhaseTimes,
}

/// Cumulative wall-clock seconds per phase of `World::step`. Timing never
/// feeds back into the simulation.
#[derive(Clone, Copy, Debug, Default)]
pub struct PhaseTimes {
    /// Level of detail (`lod_update`), hourly bookkeeping and environment chunks.
    pub environment: f64,
    pub daily: f64,
    pub grid: f64,
    pub decide: f64,
    pub apply: f64,
    /// Part of `apply` spent in the parallel apply's sequential sweeps (act
    /// and finish, `par_apply`).
    pub apply_serial: f64,
    /// Organism-turns taken whole and in order because of a chase
    /// (`World::engaged`), and all organism-turns applied (`par_apply`).
    pub engaged: u64,
    pub applied: u64,
}

/// The decision for one organism this tick.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Intent {
    pub action: crate::organism::Action,
    pub target: Option<Vec2>,
    pub target_org: Option<usize>,
    pub sprint: bool,
    pub kin_nearby: u16,
    pub threat: f32,
    /// The sentry warning this organism gives its group (`Organism::alert`).
    pub alert: Option<(Vec2, f32)>,
    pub new_wander: Option<Vec2>,
    /// `Some(None)` forgets the remembered food patch.
    pub new_forage: Option<Option<Vec2>>,
}

impl Intent {
    fn idle() -> Self {
        Intent {
            action: crate::organism::Action::Idle,
            target: None,
            target_org: None,
            sprint: false,
            kin_nearby: 0,
            threat: 0.0,
            alert: None,
            new_wander: None,
            new_forage: None,
        }
    }
}

impl World {
    pub fn new(config: WorldConfig) -> Self {
        assert!(config.ticks_per_day >= 24 && config.ticks_per_day.is_multiple_of(24), "ticks_per_day must be a multiple of 24");
        let map = Map::generate(config.seed, config.width, config.height);
        let weather = map.mean_weather();
        let group = GroupField::new(config.width, config.height, 16);
        let grid = SpatialGrid::new(config.width, config.height, 4.0);
        let lod = lod::Lod::new(map.cells.len());
        World {
            map,
            weather,
            organisms: Vec::new(),
            clutches: Vec::new(),
            group,
            counters: Counters::default(),
            legacy: Vec::new(),
            lineages: Vec::new(),
            tick: 0,
            next_id: 1,
            index: HashMap::new(),
            grid,
            owner_counts: BTreeMap::new(),
            lineage_alive: Vec::new(),
            density: SpeciesDensity::new(config.width, config.height),
            migrate_orders: BTreeMap::new(),
            slots_used: BTreeMap::new(),
            clutch_at: HashMap::new(),
            path_scratch: PathScratch::default(),
            season_offset: Map::seasonal_offset(0, config.days_per_year),
            cache_at: HashMap::new(),
            shared_caches: BTreeMap::new(),
            shared_cache_at: HashMap::new(),
            den_cells: HashMap::new(),
            lod,
            phase_times: PhaseTimes::default(),
            config,
        }
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn day(&self) -> u64 {
        self.tick / self.config.ticks_per_day as u64
    }

    pub fn day_of_year(&self) -> u32 {
        (self.day() % self.config.days_per_year as u64) as u32
    }

    pub fn year(&self) -> u64 {
        self.day() / self.config.days_per_year as u64
    }

    pub fn hour(&self) -> u32 {
        ((self.tick % self.config.ticks_per_day as u64) / self.config.ticks_per_hour()) as u32
    }

    pub fn season(&self) -> Season {
        Season::from_day(self.day_of_year(), self.config.days_per_year)
    }

    pub fn is_daytime(&self) -> bool {
        (6..19).contains(&self.hour())
    }

    pub fn alive_count(&self) -> usize {
        self.owner_counts.values().sum()
    }

    pub fn owner_count(&self, owner: PlayerId) -> usize {
        self.owner_counts.get(&owner).copied().unwrap_or(0)
    }

    /// The hard cap that applies to `lineage` and the count it is measured
    /// against. Each wild species gets a share of `wild_cap` in proportion to
    /// its founding population (the designed food pyramid: many voles, few
    /// foxes), so one booming species can't take the others' room. Players
    /// are capped per owner at `lineage_cap`.
    pub fn cap_usage(&self, lineage: u32) -> (usize, usize) {
        let Some(l) = self.lineages.get(lineage as usize) else {
            return (0, 0);
        };
        if l.owner == crate::WILD {
            let wild_founders: u64 = self.lineages.iter().filter(|w| w.owner == crate::WILD).map(|w| w.founders as u64).sum();
            let cap = self.config.wild_cap as u64 * l.founders as u64 / wild_founders.max(1);
            let alive = self.lineage_alive.get(lineage as usize).copied().unwrap_or(0);
            (alive + self.lod.lineage_count(lineage).alive, cap as usize)
        } else {
            (self.owner_count(l.owner) + self.lod.owner_count(l.owner), self.config.cap_for(l.owner))
        }
    }

    /// Births `lineage` can still add before reaching its cap.
    /// Cohort members of abstract regions (`lod`) count like agents.
    pub fn cap_room(&self, lineage: u32) -> usize {
        let (count, cap) = self.cap_usage(lineage);
        cap.saturating_sub(count)
    }

    pub fn organism(&self, id: OrgId) -> Option<&Organism> {
        self.index.get(&id).map(|&i| &self.organisms[i]).filter(|o| o.alive)
    }

    /// Living agents per lineage, indexed like `lineages` (cohort members of
    /// abstract regions are in `population_counts`).
    pub fn lineage_counts(&self) -> Vec<LineageCount> {
        let mut counts = vec![LineageCount::default(); self.lineages.len()];
        for o in self.organisms.iter().filter(|o| o.alive) {
            let c = &mut counts[o.lineage as usize];
            c.alive += 1;
            if o.matured {
                c.adults += 1;
            }
        }
        counts
    }

    pub fn latest_legacy(&self) -> Option<&LegacySnapshot> {
        self.legacy.last()
    }

    /// Spawn a validated founding population for `owner`.
    pub fn found_lineage(&mut self, owner: PlayerId, design: &FounderDesign) -> Result<Vec<OrgId>, DesignError> {
        design.validate()?;
        Ok(self.found_lineage_unchecked(owner, design))
    }

    /// Spawn a founding population without the point-budget check (wild
    /// ecosystem, tests). Player lineages start as one group; the wild
    /// ecosystem is scattered in small groups across the map.
    pub fn found_lineage_unchecked(&mut self, owner: PlayerId, design: &FounderDesign) -> Vec<OrgId> {
        let mut rng = rng_for(self.config.seed, stream::FOUNDING, owner as u64, self.next_id);
        let mut probe = Organism::new(0, owner, design.founder_genome(owner, &mut rng), Sex::Female, Vec2::ZERO);
        probe.growth = 1.0;
        let group_size = if owner == crate::WILD { 12 } else { design.founders.max(1) };
        let spread = 3.0 + 10.0 * (1.0 - probe.pheno[crate::Gene::GroupTolerance]);
        let mut ids = Vec::with_capacity(design.founders as usize);
        let mut home = Vec2::ZERO;
        let lineage = self.lineages.len() as u32;
        self.lineages.push(Lineage { name: design.name.clone(), owner, founders: design.founders });
        for k in 0..design.founders {
            if k % group_size == 0 {
                home = self.pick_home(&probe, lineage, &mut rng);
            }
            let genome = design.founder_genome(owner, &mut rng);
            let sex = if k % 2 == 0 { Sex::Female } else { Sex::Male };
            let mut pos = home;
            for _ in 0..20 {
                let off = Vec2::new(rng.gen_range(-spread..spread), rng.gen_range(-spread..spread));
                let cand = self.map.clamp(home + off);
                if self.passable_for(&probe, cand) {
                    pos = cand;
                    break;
                }
            }
            let mut o = Organism::new(self.next_id, owner, genome, sex, pos);
            o.lineage = lineage;
            // Founders start as adults of mixed ages.
            o.growth = 1.0;
            o.matured = true;
            o.age_ticks = ((o.traits.maturity_days + rng.gen_range(0.0..o.traits.lifespan_days * 0.3))
                * self.config.ticks_per_day as f32) as u32;
            o.needs.breeding = rng.gen_range(0.0..40.0);
            ids.push(self.insert(o));
        }
        ids
    }

    /// A start location the design can live on: passable, comfortable, edible,
    /// near water it can walk to without passing its predators, away from
    /// organisms already placed, and out of reach of anything that could eat
    /// it. A hunter's prey only counts if it can reach it, and prey so close
    /// that the hunters would start on top of it counts against the spot.
    fn pick_home(&self, probe: &Organism, lineage: u32, rng: &mut impl Rng) -> Vec2 {
        let buffer = self.config.balance.home_buffer.max(0.0);
        let region = self.passable_regions(probe);
        let region_at = |p: Vec2| {
            let (x, y) = self.map.tile_coords(p);
            region[self.map.idx(x, y)]
        };
        let carn = probe.traits.carnivory;
        let tax = self.config.balance.generalist_tax;
        let (plant_eff, meat_eff) = (probe.traits.plant_efficiency(tax), probe.traits.meat_efficiency(tax));
        let hunter = carn > 0.45;
        // Prey it can take alone: a pack placed by big prey it can only
        // take together eats through that prey lineage before it settles.
        let max_prey = decide::max_prey_mass(probe, 0.0);
        let others = || self.organisms.iter().filter(move |o| o.alive && o.lineage != lineage);
        // Predators that could eat the founders, packs that only take them
        // together included, weighted by their odds in a fight (as prey judge
        // threats when deciding).
        let resistance = probe.resistance();
        let threats: Vec<(Vec2, f32)> = others()
            .filter_map(|o| {
                let coop = if GroupTrait::CooperativeHunting.is_carrier(&o.pheno) { 1.0 } else { 0.0 };
                let can_take = o.traits.carnivory > 0.45 && probe.mass() < decide::max_prey_mass(o, coop);
                (decide::can_threaten(o, probe) || can_take).then(|| {
                    let power = o.power() * (1.0 + coop);
                    (o.pos, (2.0 * power / (power + resistance + 1e-3)).min(1.0))
                })
            })
            .collect();
        let threat_pos: Vec<Vec2> = threats.iter().map(|t| t.0).collect();
        // How far founders scatter from home (`found_lineage_unchecked`),
        // plus a margin.
        let scatter = 3.0 + 10.0 * (1.0 - probe.pheno[crate::Gene::GroupTolerance]) + 8.0;
        // Reachable prey: land hunters can't take newts in deep water.
        let prey: Vec<(Vec2, u32, u32)> = if hunter {
            others()
                .filter(|o| o.traits.carnivory < 0.4 && o.mass() < max_prey && !decide::can_threaten(o, probe))
                .filter(|o| self.passable_for(probe, o.pos))
                .map(|o| (o.pos, o.lineage, region_at(o.pos)))
                .collect()
        } else {
            Vec::new()
        };
        let mut per_lineage: BTreeMap<u32, f32> = BTreeMap::new();
        let mut best = (f32::MIN, Map::tile_center(self.map.width / 2, self.map.height / 2));
        for _ in 0..200 {
            let p = Vec2::new(rng.gen_range(0.0..self.map.width as f32), rng.gen_range(0.0..self.map.height as f32));
            if !self.passable_for(probe, p) {
                continue;
            }
            let t = self.map.tile_at(p);
            let comfort = if probe.traits.warm {
                -(2.0 - t.base_temp).max(0.0) * 0.1
            } else {
                -(18.0 - t.base_temp).max(0.0) * 0.1
            };
            // A short walk to drinking water that avoids the threats.
            let water = if self.water_reachable(probe, p, &threat_pos) { 1.0 } else { -2.0 };
            // Competitors share a diet: an omnivore competes with grazers and
            // hunters alike (diet overlap `1 - |carnivory difference|` above
            // 0.45), specialists only with their own kind.
            let crowd = self.organisms.iter().filter(|o| o.pos.dist(p) < 12.0 && (o.traits.carnivory - carn).abs() < 0.55).count() as f32;
            let danger: f32 = threats.iter().map(|&(q, w)| w * (1.0 - q.dist(p) / buffer.max(1e-3)).max(0.0)).sum();
            // A threat within the founders' scatter (plus a margin) would start
            // among them: weigh it heavily, not just as a slice of `danger`.
            let close: f32 = threats.iter().filter(|t| t.0.dist(p) < scatter).map(|t| t.1).sum();
            // Reachable prey nearby. Each prey lineage counts up to a share of
            // its founders and counts against the spot beyond twice that, so
            // hunters don't settle on most of one prey lineage; prey right on
            // the spot counts against it too.
            let home_region = region_at(p);
            per_lineage.clear();
            let mut on_top = 0.0f32;
            for &(q, l, r) in &prey {
                let d = q.dist(p);
                if d < PREY_ON_TOP {
                    on_top += 1.0;
                } else if d < PREY_RANGE && r == home_region {
                    *per_lineage.entry(l).or_default() += 1.0;
                }
            }
            let prey_count: f32 = per_lineage
                .iter()
                .map(|(&l, &n)| {
                    let share = (PREY_SHARE * self.lineages[l as usize].founders as f32).max(3.0);
                    n.min(share) - (n - 2.0 * share).max(0.0)
                })
                .sum();
            let productivity = (t.base_temp / 12.0).clamp(0.2, 1.0);
            let food = t.veg_max / 40.0 * productivity * plant_eff + meat_eff * 0.04 * prey_count.min(60.0);
            let score = water + food + comfort - 0.05 * crowd - (0.3 * danger).min(4.0) - (0.1 * carn * on_top).min(2.0) - 2.0 * close
                + rng.gen_range(0.0..0.5);
            if score > best.0 {
                best = (score, p);
            }
        }
        best.1
    }

    /// Connected regions of tiles the organism can walk (8-neighbor), as a
    /// label per tile; `u32::MAX` for impassable tiles.
    fn passable_regions(&self, o: &Organism) -> Vec<u32> {
        let (w, h) = (self.map.width, self.map.height);
        let open: Vec<bool> = (0..w * h).map(|i| self.passable_for(o, Map::tile_center(i % w, i / w))).collect();
        let mut label = vec![u32::MAX; w * h];
        let mut next = 0;
        let mut stack = Vec::new();
        for start in 0..w * h {
            if !open[start] || label[start] != u32::MAX {
                continue;
            }
            label[start] = next;
            stack.push(start);
            while let Some(i) = stack.pop() {
                let (x, y) = (i % w, i / w);
                for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        let j = ny * w + nx;
                        if open[j] && label[j] == u32::MAX {
                            label[j] = next;
                            stack.push(j);
                        }
                    }
                }
            }
            next += 1;
        }
        label
    }

    /// A spot where it can drink within `WATER_STEPS` steps (8-neighbor) of
    /// `p`, walking only over passable tiles that keep clear of `threats`.
    fn water_reachable(&self, o: &Organism, p: Vec2, threats: &[Vec2]) -> bool {
        const STEPS: usize = WATER_STEPS;
        const CLEARANCE: f32 = 5.0;
        let side = 2 * STEPS + 1;
        let (cx, cy) = self.map.tile_coords(p);
        let mut seen = vec![false; side * side];
        let mut frontier = vec![(cx, cy)];
        seen[STEPS * side + STEPS] = true;
        for step in 0..=STEPS {
            let mut next = Vec::new();
            for &(x, y) in &frontier {
                let c = Map::tile_center(x, y);
                if self.map.can_drink_at(c) {
                    return true;
                }
                if step == STEPS {
                    continue;
                }
                for ny in y.saturating_sub(1)..=(y + 1).min(self.map.height - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(self.map.width - 1) {
                        let (lx, ly) = (nx + STEPS - cx, ny + STEPS - cy);
                        let k = ly * side + lx;
                        if seen[k] {
                            continue;
                        }
                        seen[k] = true;
                        let q = Map::tile_center(nx, ny);
                        if self.passable_for(o, q) && threats.iter().all(|t| t.dist(q) >= CLEARANCE) {
                            next.push((nx, ny));
                        }
                    }
                }
            }
            frontier = next;
        }
        false
    }

    fn passable_for(&self, o: &Organism, p: Vec2) -> bool {
        matches!(traverse(&o.traits.body, self.map.tile_at(p), &self.map.weather_at(p), false), Traverse::Passable { .. })
    }

    fn insert(&mut self, mut o: Organism) -> OrgId {
        let id = self.next_id;
        self.next_id += 1;
        o.id = id;
        *self.owner_counts.entry(o.owner).or_default() += 1;
        self.count_lineage(o.lineage, 1);
        self.index.insert(id, self.organisms.len());
        self.organisms.push(o);
        id
    }

    fn kill(&mut self, i: usize, cause: DeathCause) {
        let o = &mut self.organisms[i];
        if !o.alive {
            return;
        }
        o.alive = false;
        o.death = Some(cause);
        o.possessed_by = None;
        let (owner, lineage, juvenile) = (o.owner, o.lineage, !o.matured);
        *self.counters.deaths.entry(cause).or_default() += 1;
        *self.counters.deaths_by_lineage.entry((lineage, cause)).or_default() += 1;
        if juvenile {
            *self.counters.juvenile_deaths.entry(lineage).or_default() += 1;
        }
        if let Some(c) = self.owner_counts.get_mut(&owner) {
            *c = c.saturating_sub(1);
        }
        self.count_lineage(lineage, -1);
    }

    fn count_lineage(&mut self, lineage: u32, delta: isize) {
        let l = lineage as usize;
        if self.lineage_alive.len() <= l {
            self.lineage_alive.resize(l + 1, 0);
        }
        self.lineage_alive[l] = self.lineage_alive[l].saturating_add_signed(delta);
    }

    /// Advance one tick.
    pub fn step(&mut self) {
        let tpd = self.config.ticks_per_day as u64;
        let mut clock = std::time::Instant::now();
        let mut lap = || {
            let now = std::time::Instant::now();
            let s = (now - clock).as_secs_f64();
            clock = now;
            s
        };
        if self.config.lod.enabled && self.tick.is_multiple_of(self.config.ticks_per_hour()) {
            self.lod_update();
        }
        if self.tick.is_multiple_of(self.config.ticks_per_hour()) {
            self.hourly();
        }
        self.environment_slice();
        self.phase_times.environment += lap();
        if self.tick.is_multiple_of(tpd) {
            self.daily();
        }
        self.phase_times.daily += lap();

        self.settle_orders();
        self.grid.rebuild(&self.organisms);
        self.phase_times.grid += lap();
        let n = self.organisms.len();
        let this = &*self;
        let intents: Vec<Intent> = (0..n).into_par_iter().map(|i| this.decide(i)).collect();
        self.phase_times.decide += lap();

        let mut rng = rng_for(self.config.seed, stream::APPLY, self.tick, 0);
        let mut order: Vec<usize> = (0..n).collect();
        order.shuffle(&mut rng);
        if self.config.parallel_apply {
            self.par_apply(&intents, &order, &mut rng);
        } else {
            for i in order {
                self.apply(i, &intents[i], &mut rng);
            }
        }
        self.phase_times.apply += lap();
        self.tick += 1;
    }

    pub fn run_days(&mut self, days: u32) {
        for _ in 0..days * self.config.ticks_per_day {
            self.step();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets;

    fn small_world(seed: u64) -> World {
        let mut w = World::new(WorldConfig { seed, width: 64, height: 64, ..Default::default() });
        for (owner, design) in presets::wild_designs() {
            let n = (design.founders / 3).max(4);
            let d = design.founders(n);
            w.found_lineage_unchecked(owner, &d);
        }
        for (k, design) in presets::player_designs().into_iter().enumerate() {
            w.found_lineage(k as PlayerId + 1, &design).unwrap();
        }
        w
    }

    #[test]
    fn runs_and_keeps_life_going() {
        let mut w = small_world(3);
        let start = w.alive_count();
        w.run_days(6);
        assert!(w.alive_count() > start / 4, "population collapsed: {} -> {}", start, w.alive_count());
        assert!(!w.legacy.is_empty());
        let s = w.latest_legacy().unwrap();
        let total: f64 = s.alleles.values().sum();
        assert!((total - s.total_alleles).abs() < 1e-6);
    }

    #[test]
    fn same_seed_same_world() {
        let mut a = small_world(11);
        let mut b = small_world(11);
        a.run_days(2);
        b.run_days(2);
        assert_eq!(a.alive_count(), b.alive_count());
        let pa: Vec<_> = a.organisms.iter().map(|o| (o.id, o.pos)).collect();
        let pb: Vec<_> = b.organisms.iter().map(|o| (o.id, o.pos)).collect();
        assert_eq!(pa, pb);
    }

    #[test]
    fn over_budget_design_is_rejected() {
        let mut w = World::new(WorldConfig { width: 32, height: 32, ..Default::default() });
        let mut d = FounderDesign::new("god", crate::Thermo::Warm, crate::BirthType::Live, 12);
        for &g in crate::Gene::ALL {
            if crate::design::trait_cost(g) > 0.0 {
                d = d.with(g, 1.0);
            }
        }
        assert!(w.found_lineage(1, &d).is_err());
    }

    #[test]
    fn founders_start_clear_of_their_predators() {
        // Player groups keep away from bigger predators founded before them.
        for seed in 1..=4 {
            let w = presets::standard_world(WorldConfig { seed, width: 96, height: 96, ..Default::default() }, 1);
            for (l, lineage) in w.lineages.iter().enumerate().filter(|(_, l)| l.owner != crate::WILD) {
                let l = l as u32;
                let nearest = w
                    .organisms
                    .iter()
                    .filter(|f| f.lineage == l)
                    .flat_map(|f| {
                        w.organisms
                            .iter()
                            .filter(move |o| o.lineage < l && decide::can_threaten(o, f) && o.mass() > f.mass())
                            .map(move |o| o.pos.dist(f.pos))
                    })
                    .fold(f32::MAX, f32::min);
                assert!(nearest > 8.0, "seed {seed}: {} founded {nearest:.1} tiles from a predator", lineage.name);
            }
        }
    }
}
