//! The world and its tick loop.
//!
//! One tick:
//! 1. On the hour: environment (temperature, snow, ice, regrowth), weather,
//!    clutches.
//! 2. On the day: remove the dead, recompute group traits, take a legacy
//!    snapshot, reset mating slots at a season change.
//! 3. Rebuild the spatial index.
//! 4. **Decide** (parallel, read-only): every organism scores its actions by
//!    need urgency × behavior genes and picks one.
//! 5. **Apply** (sequential, shuffled order): movement with terrain checks,
//!    eating, predation, mating, needs, death and birth.
//!
//! All randomness comes from per-(tick, organism) streams, so a run is
//! reproducible from its seed regardless of thread count.

mod apply;
pub mod commands;
mod decide;
mod environment;
mod spatial;

use std::collections::{BTreeMap, HashMap};

use rand::seq::SliceRandom;
use rand::Rng;
use rayon::prelude::*;

use crate::config::WorldConfig;
use crate::design::{DesignError, FounderDesign};
use crate::genome::{Genome, PlayerId};
use crate::group::GroupField;
use crate::legacy::LegacySnapshot;
use crate::math::Vec2;
use crate::organism::{DeathCause, OrgId, Organism, Sex};
use crate::rng::{rng_for, stream};
use crate::terrain::{traverse, Map, Season, Traverse, Weather};

use spatial::SpatialGrid;

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
}

pub struct World {
    pub config: WorldConfig,
    pub map: Map,
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
    migrate_orders: BTreeMap<PlayerId, Vec2>,
    slots_used: BTreeMap<PlayerId, u32>,
    clutch_at: HashMap<(usize, usize), usize>,
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
            new_wander: None,
            new_forage: None,
        }
    }
}

impl World {
    pub fn new(config: WorldConfig) -> Self {
        assert!(config.ticks_per_day >= 24 && config.ticks_per_day.is_multiple_of(24), "ticks_per_day must be a multiple of 24");
        let map = Map::generate(config.seed, config.width, config.height);
        let group = GroupField::new(config.width, config.height, 16);
        let grid = SpatialGrid::new(config.width, config.height, 4.0);
        World {
            map,
            weather: Weather { wind: 0.2 },
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
            migrate_orders: BTreeMap::new(),
            slots_used: BTreeMap::new(),
            clutch_at: HashMap::new(),
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

    pub fn organism(&self, id: OrgId) -> Option<&Organism> {
        self.index.get(&id).map(|&i| &self.organisms[i]).filter(|o| o.alive)
    }

    /// Living members per lineage, indexed like `lineages`.
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
        let probe = Organism::new(0, owner, design.founder_genome(owner, &mut rng), Sex::Female, Vec2::ZERO);
        let group_size = if owner == crate::WILD { 12 } else { design.founders.max(1) };
        let spread = 3.0 + 10.0 * (1.0 - probe.pheno[crate::Gene::GroupTolerance]);
        let mut ids = Vec::with_capacity(design.founders as usize);
        let mut home = Vec2::ZERO;
        let lineage = self.lineages.len() as u32;
        self.lineages.push(Lineage { name: design.name.clone(), owner, founders: design.founders });
        for k in 0..design.founders {
            if k % group_size == 0 {
                home = self.pick_home(&probe, &mut rng);
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
    /// near water, and away from organisms already placed.
    fn pick_home(&self, probe: &Organism, rng: &mut impl Rng) -> Vec2 {
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
            let water = if self.map.find_water(p, 6).is_some() { 1.0 } else { -2.0 };
            let carn = probe.traits.carnivory;
            // Competitors share a diet; prey are plant eaters nearby.
            let (mut crowd, mut prey) = (0.0f32, 0.0f32);
            for o in self.organisms.iter().filter(|o| o.pos.dist(p) < 12.0) {
                if (o.traits.carnivory - carn).abs() < 0.3 {
                    crowd += 1.0;
                }
                if o.traits.carnivory < 0.4 {
                    prey += 1.0;
                }
            }
            let productivity = (t.base_temp / 12.0).clamp(0.2, 1.0);
            let food = t.veg_max / 40.0 * productivity * (1.0 - carn) + carn * 0.04 * prey.min(60.0);
            let score = water + food + comfort - 0.05 * crowd + rng.gen_range(0.0..0.5);
            if score > best.0 {
                best = (score, p);
            }
        }
        best.1
    }

    fn passable_for(&self, o: &Organism, p: Vec2) -> bool {
        matches!(traverse(&o.traits.body, self.map.tile_at(p), &self.weather, false), Traverse::Passable { .. })
    }

    fn insert(&mut self, mut o: Organism) -> OrgId {
        let id = self.next_id;
        self.next_id += 1;
        o.id = id;
        *self.owner_counts.entry(o.owner).or_default() += 1;
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
    }

    /// Advance one tick.
    pub fn step(&mut self) {
        let tpd = self.config.ticks_per_day as u64;
        if self.tick.is_multiple_of(self.config.ticks_per_hour()) {
            self.hourly();
        }
        if self.tick.is_multiple_of(tpd) {
            self.daily();
        }

        self.grid.rebuild(&self.organisms);
        let n = self.organisms.len();
        let this = &*self;
        let intents: Vec<Intent> = (0..n).into_par_iter().map(|i| this.decide(i)).collect();

        let mut rng = rng_for(self.config.seed, stream::APPLY, self.tick, 0);
        let mut order: Vec<usize> = (0..n).collect();
        order.shuffle(&mut rng);
        for i in order {
            self.apply(i, &intents[i], &mut rng);
        }
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
}
