//! Level of detail ("Scale and performance" in the design doc): full agents
//! near active players, statistical populations everywhere else.
//!
//! **Regions** are the 16-tile environment chunks (`terrain::CHUNK`). A region
//! is *active* while any part of it lies within `LodConfig::active_radius`
//! tiles of a focus point (`World::set_focus`) or a possessed organism, and
//! *abstract* once it is more than `active_radius + hysteresis` away.
//!
//! **Collapse.** At the daily update every agent standing in an abstract
//! region (and every clutch there) is folded into its region's [`Cohort`]
//! for its lineage: a count per birth-day bucket ([`BIN_DAYS`]) and sex, a
//! pool of genomes (every member's exact genome up to `pool_size` members,
//! a uniform sample above that), and mean hunger, health and fat. Agents
//! that wander into an abstract region are absorbed the same way.
//!
//! **Abstract dynamics**, once per day per region: food from the region's
//! own vegetation (eaten off its tiles, so it regrows as usual), predation
//! by predator cohorts hunting their own and the 8 surrounding abstract
//! regions (Holling type II; prey by mass, pack hunting, toxicity, stealth,
//! burrows, dens and mothers guarding young; hunters live on plants between
//! kills but breed on meat), fat reserves and dormancy, starvation, thirst where
//! no water is in reach, old age, attrition and larval competition as for
//! agents, births from the traits (litter, gestation, recovery, season,
//! condition, density) with genomes recombined from the pool (mutations
//! tagged with the owner, as for agents), and migration to neighboring
//! regions (into an active one as agents). Deaths and births go into the
//! world's counters like any other.
//!
//! **Expand.** When a region turns active its cohorts become agents again:
//! ages drawn within each bucket, positions on tiles the body can use,
//! genomes from the pool (exact while the pool holds every member, else a
//! recombinant of two pool genomes: alleles and origin tags are conserved in
//! expectation), dependent young placed next to a mother.
//!
//! Legacy scoring and population caps count cohorts like agents
//! (`World::lod_legacy`, `World::cap_usage`). Group traits and disease state
//! are agent-only: infections are forgotten on collapse.

use std::collections::{BTreeMap, HashMap};

use rand::Rng;

use super::{decide, LineageCount, World};
use crate::design::BirthType;
use crate::group::GroupTrait;
use crate::genome::{Gene, Genome, MutationParams, PlayerId};
use crate::legacy::LegacySnapshot;
use crate::math::Vec2;
use crate::organism::{DeathCause, OrgId, Organism, Sex};
use crate::rng::{normal, rng_for, stream, SimRng};
use crate::terrain::{Map, Season, CHUNK};

/// Width in days of a cohort's age buckets.
pub const BIN_DAYS: f64 = 5.0;
/// Tiles from water within which a cohort member can drink.
const WATER_REACH: u16 = 6;
/// Plant-digesting efficiency (1 - carnivory) from which a cohort grazes.
/// As for agents (`World::eat_plants`): even carnivores graze a little when
/// hungry, and plant food is plentiful, so it carries them between kills.
const PLANT_EATER: f32 = 0.05;
/// Mean activity multiplier on hunger (agents rest at 0.8, walk at 1.2).
const ACTIVITY: f32 = 0.9;
/// Tiles within which an organism counts kin (larval competition).
const KIN_AREA: f32 = 25.0;
/// Carnivory above which a cohort hunts (as agents do).
const HUNTER: f32 = 0.45;
/// Days between refreshes of a cohort's representative traits.
const REP_REFRESH_DAYS: u64 = 5;

/// One genome in a cohort's pool.
#[derive(Clone, Debug, PartialEq)]
pub struct PoolEntry {
    pub genome: Genome,
    pub generation: u32,
}

/// Mean traits of a cohort, derived from its genome pool.
#[derive(Clone, Debug)]
struct Rep {
    /// An adult organism from the first pool genome: body for passability,
    /// reachable browse and prey size.
    probe: Organism,
    mass: f32,
    metabolism: f32,
    insulation: f32,
    carnivory: f32,
    litter: f32,
    gestation: f32,
    recovery: f32,
    maturity: f32,
    lifespan: f32,
    newborn_growth: f32,
    pace: f32,
    group_tolerance: f32,
    toxins: f32,
    stealth: f32,
    burrow: f32,
    wanderlust: f32,
    fat_stores: f32,
    caching: f32,
    winter: f32,
    estivation: f32,
}

impl Rep {
    fn compute(pool: &[PoolEntry]) -> Rep {
        let mut probe = Organism::new(0, 0, pool[0].genome.clone(), Sex::Female, Vec2::ZERO);
        probe.growth = 1.0;
        probe.matured = true;
        let sample = &pool[..pool.len().min(16)];
        let n = sample.len() as f32;
        let mut r = Rep {
            probe,
            mass: 0.0,
            metabolism: 0.0,
            insulation: 0.0,
            carnivory: 0.0,
            litter: 0.0,
            gestation: 0.0,
            recovery: 0.0,
            maturity: 0.0,
            lifespan: 0.0,
            newborn_growth: 0.0,
            pace: 0.0,
            group_tolerance: 0.0,
            toxins: 0.0,
            stealth: 0.0,
            burrow: 0.0,
            wanderlust: 0.0,
            fat_stores: 0.0,
            caching: 0.0,
            winter: 0.0,
            estivation: 0.0,
        };
        for e in sample {
            let p = e.genome.phenotype();
            let t = crate::stats::derive(&p);
            r.mass += t.body.mass / n;
            r.metabolism += t.stats.metabolism / n;
            r.insulation += t.stats.insulation / n;
            r.carnivory += t.carnivory / n;
            r.litter += t.litter as f32 / n;
            r.gestation += t.gestation_days / n;
            r.recovery += t.recovery_days / n;
            r.maturity += t.maturity_days / n;
            r.lifespan += t.lifespan_days / n;
            r.newborn_growth += t.newborn_growth / n;
            r.pace += t.pace_of_life(p[Gene::BodySize]) / n;
            r.group_tolerance += p[Gene::GroupTolerance] / n;
            r.toxins += p[Gene::Toxins] / n;
            r.stealth += t.stats.stealth / n;
            r.burrow += t.body.burrow / n;
            r.wanderlust += p[Gene::Wanderlust] / n;
            r.fat_stores += p[Gene::FatStores] / n;
            r.caching += t.seasonal.caching / n;
            r.winter += t.seasonal.winter.map_or(0.0, |w| w.1) / n;
            r.estivation += t.seasonal.estivation / n;
        }
        r
    }

    fn birth(&self) -> BirthType {
        self.probe.traits.birth
    }

    fn warm(&self) -> bool {
        self.probe.traits.warm
    }

    fn food_per_hunger(&self) -> f32 {
        0.04 * self.mass.powf(0.75)
    }

    fn growth_at(&self, age_days: f64) -> f32 {
        let progress = (age_days as f32 / self.maturity.max(1e-3)).clamp(0.0, 1.0);
        self.newborn_growth + (1.0 - self.newborn_growth) * progress
    }
}

/// A member taken out of a cohort.
struct Member {
    sex: Sex,
    birth_day: f64,
    entry: Option<PoolEntry>,
}

/// The members of one lineage in one abstract region, as statistics.
#[derive(Clone, Debug)]
pub struct Cohort {
    pub lineage: u32,
    pub owner: PlayerId,
    /// Members by birth-day bucket (`floor(birth day / BIN_DAYS)`), as
    /// `[females, males]`.
    pub ages: BTreeMap<i64, [u32; 2]>,
    /// Genome pool: `min(members, pool_size)` entries.
    pub pool: Vec<PoolEntry>,
    pub hunger: f32,
    pub health: f32,
    pub fat: f32,
    /// Offspring raised to adulthood by the members (legacy weighting).
    pub raised: f64,
    count: u32,
    rep: Option<Box<Rep>>,
    rep_day: u64,
}

fn bucket_of(birth_day: f64) -> i64 {
    (birth_day / BIN_DAYS).floor() as i64
}

/// Age in days of the middle of bucket `b` on `today`.
fn bucket_age(b: i64, today: f64) -> f64 {
    (today - (b as f64 + 0.5) * BIN_DAYS).max(0.0)
}

fn sex_index(s: Sex) -> usize {
    match s {
        Sex::Female => 0,
        Sex::Male => 1,
    }
}

impl Cohort {
    fn new(lineage: u32, owner: PlayerId) -> Self {
        Cohort {
            lineage,
            owner,
            ages: BTreeMap::new(),
            pool: Vec::new(),
            hunger: 0.0,
            health: 100.0,
            fat: 0.0,
            raised: 0.0,
            count: 0,
            rep: None,
            rep_day: 0,
        }
    }

    /// Living members.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Days to maturity of the cohort's (mean) phenotype.
    pub fn maturity_days(&self) -> f32 {
        self.rep.as_ref().map_or(0.0, |r| r.maturity)
    }

    /// Members old enough to breed on `today` (days), as `[females, males]`.
    pub fn adults(&self, today: f64) -> [u32; 2] {
        let m = self.maturity_days() as f64;
        let mut out = [0; 2];
        for (&b, c) in &self.ages {
            if bucket_age(b, today) >= m {
                out[0] += c[0];
                out[1] += c[1];
            }
        }
        out
    }

    fn ensure_rep(&mut self, day: u64, refresh: bool) {
        if self.pool.is_empty() {
            return;
        }
        if self.rep.is_none() || (refresh && day >= self.rep_day + REP_REFRESH_DAYS) {
            self.rep = Some(Box::new(Rep::compute(&self.pool)));
            self.rep_day = day;
        }
    }

    /// Add a member. Its genome joins the pool while the pool has room, and
    /// otherwise replaces a random entry with chance `pool_size / members`,
    /// so the pool stays a uniform sample.
    fn add(&mut self, sex: Sex, birth_day: f64, entry: PoolEntry, pool_size: usize, rng: &mut SimRng) {
        self.ages.entry(bucket_of(birth_day)).or_insert([0; 2])[sex_index(sex)] += 1;
        self.count += 1;
        if self.pool.len() < pool_size.max(1) {
            self.pool.push(entry);
        } else if rng.gen_range(0..self.count) < pool_size as u32 {
            let k = rng.gen_range(0..self.pool.len());
            self.pool[k] = entry;
        }
    }

    /// Fold an agent's condition into the means.
    fn blend_state(&mut self, hunger: f32, health: f32, fat: f32) {
        let w = 1.0 / self.count.max(1) as f32;
        self.hunger += (hunger - self.hunger) * w;
        self.health += (health - self.health) * w;
        self.fat += (fat - self.fat) * w;
    }

    /// A uniformly random member among buckets passing `filter`, as
    /// (bucket, sex index).
    fn pick(&self, rng: &mut SimRng, filter: impl Fn(i64) -> bool) -> Option<(i64, usize)> {
        let total: u32 = self.ages.iter().filter(|(&b, _)| filter(b)).map(|(_, c)| c[0] + c[1]).sum();
        if total == 0 {
            return None;
        }
        let mut k = rng.gen_range(0..total);
        for (&b, c) in self.ages.iter().filter(|(&b, _)| filter(b)) {
            for (s, &n) in c.iter().enumerate() {
                if k < n {
                    return Some((b, s));
                }
                k -= n;
            }
        }
        None
    }

    /// Remove one member of bucket `b`, sex `s`. While the pool holds a
    /// genome per member, the member takes one with it; otherwise it gets a
    /// recombinant of two pool genomes (only if `want_genome`).
    fn remove_at(&mut self, b: i64, s: usize, rng: &mut SimRng, want_genome: bool) -> Member {
        let c = self.ages.get_mut(&b).expect("bucket exists");
        c[s] -= 1;
        if c[0] + c[1] == 0 {
            self.ages.remove(&b);
        }
        let before = self.count;
        self.count -= 1;
        let entry = if self.pool.len() as u32 >= before {
            let k = rng.gen_range(0..self.pool.len());
            Some(self.pool.swap_remove(k))
        } else if want_genome && !self.pool.is_empty() {
            Some(self.recombinant(rng))
        } else {
            None
        };
        let birth_day = b as f64 * BIN_DAYS + rng.gen::<f64>() * BIN_DAYS;
        Member { sex: if s == 0 { Sex::Female } else { Sex::Male }, birth_day, entry }
    }

    /// A genome made by independent assortment from two random pool genomes,
    /// without mutation: resampling, not a birth.
    fn recombinant(&self, rng: &mut SimRng) -> PoolEntry {
        let a = &self.pool[rng.gen_range(0..self.pool.len())];
        let b = &self.pool[rng.gen_range(0..self.pool.len())];
        let none = MutationParams { rate: 0.0, sd: 0.0 };
        let genome = Genome::inherit(&a.genome, &b.genome, self.owner, none, 0.0, rng);
        PoolEntry { genome, generation: a.generation.max(b.generation) }
    }

    /// Take a uniformly random member out.
    fn take_any(&mut self, rng: &mut SimRng, want_genome: bool) -> Option<Member> {
        let (b, s) = self.pick(rng, |_| true)?;
        Some(self.remove_at(b, s, rng, want_genome))
    }
}

/// Counters of LOD activity, for reports.
#[derive(Clone, Debug, Default)]
pub struct LodStats {
    /// Region switches.
    pub collapses: u64,
    pub expansions: u64,
    /// Agents folded into cohorts (at collapse or after wandering into an
    /// abstract region) and organisms regenerated from cohorts.
    pub absorbed: u64,
    pub expanded: u64,
    /// Cohort members that migrated into an active region as agents.
    pub emigrated: u64,
}

/// Level-of-detail state of a world.
#[derive(Clone, Debug)]
pub(crate) struct Lod {
    pub focus: BTreeMap<PlayerId, Vec<Vec2>>,
    /// Per region: full agents (true) or cohorts.
    pub active: Vec<bool>,
    /// Per region: cohorts sorted by lineage (empty while active).
    pub regions: Vec<Vec<Cohort>>,
    owners: BTreeMap<PlayerId, usize>,
    lineages: Vec<LineageCount>,
    /// (region, lineage) → (chunk version, tiles the lineage can use).
    passable: HashMap<(usize, u32), (u64, Vec<u32>)>,
    pub stats: LodStats,
}

impl Lod {
    pub fn new(regions: usize) -> Self {
        Lod {
            focus: BTreeMap::new(),
            active: vec![true; regions],
            regions: vec![Vec::new(); regions],
            owners: BTreeMap::new(),
            lineages: Vec::new(),
            passable: HashMap::new(),
            stats: LodStats::default(),
        }
    }

    pub fn owner_count(&self, owner: PlayerId) -> usize {
        self.owners.get(&owner).copied().unwrap_or(0)
    }

    pub fn lineage_count(&self, lineage: u32) -> LineageCount {
        self.lineages.get(lineage as usize).copied().unwrap_or_default()
    }
}

fn cohort_mut(cohorts: &mut Vec<Cohort>, lineage: u32, owner: PlayerId) -> &mut Cohort {
    let k = match cohorts.binary_search_by_key(&lineage, |c| c.lineage) {
        Ok(k) => k,
        Err(k) => {
            cohorts.insert(k, Cohort::new(lineage, owner));
            k
        }
    };
    &mut cohorts[k]
}

/// Binomial draw: exact for small `n`, normal approximation above.
fn binomial(n: u32, p: f64, rng: &mut SimRng) -> u32 {
    if n == 0 || p <= 0.0 {
        return 0;
    }
    if p >= 1.0 {
        return n;
    }
    if n <= 40 {
        return (0..n).filter(|_| rng.gen::<f64>() < p).count() as u32;
    }
    let mean = n as f64 * p;
    let sd = (mean * (1.0 - p)).sqrt();
    (mean + sd * normal(rng) as f64).round().clamp(0.0, n as f64) as u32
}

/// Poisson draw: Knuth for small means, normal approximation above.
fn poisson(lambda: f64, rng: &mut SimRng) -> u32 {
    if lambda <= 0.0 {
        return 0;
    }
    if lambda < 30.0 {
        let limit = (-lambda).exp();
        let mut k = 0;
        let mut p = rng.gen::<f64>();
        while p > limit {
            k += 1;
            p *= rng.gen::<f64>();
        }
        return k;
    }
    (lambda + lambda.sqrt() * normal(rng) as f64).round().max(0.0) as u32
}

/// `x` rounded up or down at random, in proportion to its fraction.
fn stochastic_round(x: f64, rng: &mut SimRng) -> u32 {
    let x = x.max(0.0);
    x.floor() as u32 + u32::from(rng.gen::<f64>() < x.fract())
}

/// A member on its way to another region.
struct Arrival {
    from: usize,
    to: usize,
    lineage: u32,
    owner: PlayerId,
    member: Member,
    hunger: f32,
    health: f32,
    fat: f32,
}

/// Per-cohort working values for one day.
#[derive(Clone, Copy, Default)]
struct Day {
    tiles: usize,
    water: bool,
    dormant: bool,
    /// Metabolic units: adults 1, juveniles growth^0.75.
    units: f32,
    /// Hunger points per unit per day.
    hunger: f32,
    carnivory: f32,
    /// Food units per day for the whole need, at full digestive efficiency.
    food: f32,
    plant_need: f32,
    meat_need: f32,
    veg: f32,
    s_plant: f32,
    s_meat: f32,
}

impl World {
    /// Set the points `player` is watching (their organisms, camera, …).
    /// With LOD on, regions within `LodConfig::active_radius` of any focus
    /// point run as full agents from the next simulated hour. An empty list
    /// clears the player's focus. Possessed organisms are always a focus.
    pub fn set_focus(&mut self, player: PlayerId, points: Vec<Vec2>) {
        if points.is_empty() {
            self.lod.focus.remove(&player);
        } else {
            self.lod.focus.insert(player, points);
        }
    }

    /// Region (chunk) index of a point.
    pub fn region_of(&self, p: Vec2) -> usize {
        self.map.chunk_at(p)
    }

    /// Is the region simulated with full agents? Always true with LOD off.
    pub fn region_is_active(&self, region: usize) -> bool {
        !self.config.lod.enabled || self.lod.active.get(region).copied().unwrap_or(true)
    }

    /// Organisms living as cohort members in abstract regions.
    pub fn abstract_count(&self) -> usize {
        self.lod.owners.values().sum()
    }

    /// Every living organism: agents plus cohort members.
    pub fn population(&self) -> usize {
        self.alive_count() + self.abstract_count()
    }

    /// Living members per lineage, agents and cohort members together.
    pub fn population_counts(&self) -> Vec<LineageCount> {
        let mut counts = self.lineage_counts();
        for (l, c) in counts.iter_mut().enumerate() {
            let a = self.lod.lineage_count(l as u32);
            c.alive += a.alive;
            c.adults += a.adults;
        }
        counts
    }

    /// Cohort members per lineage (indexed like `lineages`).
    pub fn abstract_counts(&self) -> Vec<LineageCount> {
        (0..self.lineages.len()).map(|l| self.lod.lineage_count(l as u32)).collect()
    }

    /// Every cohort with its region.
    pub fn cohorts(&self) -> impl Iterator<Item = (usize, &Cohort)> {
        self.lod.regions.iter().enumerate().flat_map(|(r, cs)| cs.iter().map(move |c| (r, c)))
    }

    pub fn lod_stats(&self) -> &LodStats {
        &self.lod.stats
    }

    fn today(&self) -> f64 {
        self.tick as f64 / self.config.ticks_per_day as f64
    }

    /// Which regions must be active (`Some(true)`), may be abstract
    /// (`Some(false)`), or stay as they are (`None`, the hysteresis band).
    fn lod_desired(&self) -> Vec<Option<bool>> {
        let cfg = self.config.lod;
        let mut points: Vec<Vec2> = self.lod.focus.values().flatten().copied().collect();
        points.extend(self.organisms.iter().filter(|o| o.alive && o.possessed_by.is_some()).map(|o| o.pos));
        let (cx, cy) = (self.map.chunks_x, self.map.chunks_y);
        let mut out = Vec::with_capacity(cx * cy);
        for r in 0..cx * cy {
            let x0 = ((r % cx) * CHUNK) as f32;
            let y0 = ((r / cx) * CHUNK) as f32;
            let x1 = ((r % cx + 1) * CHUNK).min(self.map.width) as f32;
            let y1 = ((r / cx + 1) * CHUNK).min(self.map.height) as f32;
            let d = points
                .iter()
                .map(|p| {
                    let dx = (x0 - p.x).max(p.x - x1).max(0.0);
                    let dy = (y0 - p.y).max(p.y - y1).max(0.0);
                    (dx * dx + dy * dy).sqrt()
                })
                .fold(f32::MAX, f32::min);
            out.push(if d <= cfg.active_radius {
                Some(true)
            } else if d > cfg.active_radius + cfg.hysteresis {
                Some(false)
            } else {
                None
            });
        }
        out
    }

    /// The LOD update, on every simulated hour (before the hourly and daily
    /// updates): regions that came into focus expand at once; at the start
    /// of a day, regions out of focus collapse, stray agents are absorbed and
    /// every cohort lives one day.
    pub(super) fn lod_update(&mut self) {
        let desired = self.lod_desired();
        let mut rng = rng_for(self.config.seed, stream::LOD, self.tick, u64::MAX);
        for (r, want) in desired.iter().enumerate() {
            if *want == Some(true) && !self.lod.active[r] {
                self.lod.active[r] = true;
                self.lod.stats.expansions += 1;
                self.expand_region(r, &mut rng);
            }
        }
        if self.tick.is_multiple_of(self.config.ticks_per_day as u64) {
            for (r, want) in desired.iter().enumerate() {
                if *want == Some(false) && self.lod.active[r] {
                    self.lod.active[r] = false;
                    self.lod.stats.collapses += 1;
                }
            }
            self.absorb_agents(&mut rng);
            self.recount_lod();
            self.step_cohorts();
        }
        self.recount_lod();
    }

    /// Remove an agent that joined a cohort (not a death).
    fn remove_agent(&mut self, i: usize) {
        let o = &mut self.organisms[i];
        o.alive = false;
        let (owner, lineage) = (o.owner, o.lineage);
        if let Some(c) = self.owner_counts.get_mut(&owner) {
            *c = c.saturating_sub(1);
        }
        self.count_lineage(lineage, -1);
    }

    /// Fold every agent (except possessed ones) and clutch standing in an
    /// abstract region into its cohort. Removed agents leave the organism
    /// list at the daily cleanup that follows.
    fn absorb_agents(&mut self, rng: &mut SimRng) {
        let today = self.today();
        let tpd = self.config.ticks_per_day as f64;
        let k = self.config.lod.pool_size.max(1.0) as usize;
        let day = self.day();
        for i in 0..self.organisms.len() {
            let o = &self.organisms[i];
            if !o.alive || o.possessed_by.is_some() {
                continue;
            }
            let r = self.map.chunk_at(o.pos);
            if self.lod.active[r] {
                continue;
            }
            let owner = self.lineages.get(o.lineage as usize).map_or(o.owner, |l| l.owner);
            let birth_day = today - o.age_ticks as f64 / tpd;
            let entry = PoolEntry { genome: o.genome.clone(), generation: o.generation };
            let (sex, hunger, health, fat, raised, lineage) =
                (o.sex, o.needs.hunger, o.health, o.fat, o.offspring_raised, o.lineage);
            let c = cohort_mut(&mut self.lod.regions[r], lineage, owner);
            c.add(sex, birth_day, entry, k, rng);
            c.blend_state(hunger, health, fat);
            c.raised += raised as f64;
            c.ensure_rep(day, false);
            self.remove_agent(i);
            self.lod.stats.absorbed += 1;
        }
        // Clutches: the eggs join as newborns.
        let clutches = std::mem::take(&mut self.clutches);
        let mut keep = Vec::with_capacity(clutches.len());
        for c in clutches {
            let r = self.map.chunk_at(c.pos);
            if self.lod.active[r] {
                keep.push(c);
                continue;
            }
            if let Some(gi) = c.guardian.and_then(|g| self.index.get(&g).copied()) {
                self.organisms[gi].nest = None;
            }
            let owner = self.lineages.get(c.lineage as usize).map_or(c.owner, |l| l.owner);
            let cohort = cohort_mut(&mut self.lod.regions[r], c.lineage, owner);
            for g in c.eggs {
                let sex = if rng.gen::<bool>() { Sex::Female } else { Sex::Male };
                cohort.add(sex, today, PoolEntry { genome: g, generation: c.generation }, k, rng);
                cohort.blend_state(30.0, 100.0, 0.0);
                self.lod.stats.absorbed += 1;
            }
            cohort.ensure_rep(day, false);
        }
        self.clutches = keep;
        self.clutch_at.clear();
        for (k, c) in self.clutches.iter().enumerate() {
            self.clutch_at.insert(self.map.tile_coords(c.pos), k);
        }
    }

    /// Tiles of region `r` the lineage's body can use (cached per chunk
    /// version).
    fn passable_tiles(&mut self, r: usize, lineage: u32, probe: &Organism) -> &[u32] {
        let version = self.map.chunk_version[r];
        let fresh = self.lod.passable.get(&(r, lineage)).is_some_and(|(v, _)| *v == version);
        if !fresh {
            let cx = self.map.chunks_x;
            let (x0, y0) = ((r % cx) * CHUNK, (r / cx) * CHUNK);
            let (x1, y1) = ((x0 + CHUNK).min(self.map.width), (y0 + CHUNK).min(self.map.height));
            let mut tiles = Vec::new();
            for y in y0..y1 {
                for x in x0..x1 {
                    if self.passable_for(probe, Map::tile_center(x, y)) {
                        tiles.push(self.map.idx(x, y) as u32);
                    }
                }
            }
            self.lod.passable.insert((r, lineage), (version, tiles));
        }
        &self.lod.passable[&(r, lineage)].1
    }

    /// A position in region `r` the organism can use: a random usable tile,
    /// else the nearest usable tile around the region.
    fn place_in_region(&self, r: usize, tiles: &[u32], o: &Organism, rng: &mut SimRng) -> Vec2 {
        let w = self.map.width;
        for _ in 0..12 {
            if tiles.is_empty() {
                break;
            }
            let t = tiles[rng.gen_range(0..tiles.len())] as usize;
            let p = Map::tile_center(t % w, t / w) + Vec2::new(rng.gen_range(-0.4..0.4), rng.gen_range(-0.4..0.4));
            if self.passable_for(o, p) {
                return p;
            }
        }
        let cx = self.map.chunks_x;
        let center = ((r % cx) * CHUNK + CHUNK / 2, (r / cx) * CHUNK + CHUNK / 2);
        let (cxt, cyt) = (center.0.min(w - 1) as i64, center.1.min(self.map.height - 1) as i64);
        for radius in 0..64i64 {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx.abs() != radius && dy.abs() != radius {
                        continue;
                    }
                    let (x, y) = (cxt + dx, cyt + dy);
                    if x < 0 || y < 0 || x >= w as i64 || y >= self.map.height as i64 {
                        continue;
                    }
                    let p = Map::tile_center(x as usize, y as usize);
                    if self.passable_for(o, p) {
                        return p;
                    }
                }
            }
        }
        Map::tile_center(cxt as usize, cyt as usize)
    }

    /// Turn region `r`'s cohorts back into agents.
    fn expand_region(&mut self, r: usize, rng: &mut SimRng) {
        let cohorts = std::mem::take(&mut self.lod.regions[r]);
        let today = self.today();
        let tpd = self.config.ticks_per_day as f64;
        for mut c in cohorts {
            if c.pool.is_empty() {
                continue;
            }
            c.ensure_rep(self.day(), false);
            let rep = c.rep.clone().expect("rep");
            let tiles = self.passable_tiles(r, c.lineage, &rep.probe).to_vec();
            let adults = c.adults(today);
            let raised_each = c.raised / (adults[0] + adults[1]).max(1) as f64;
            let mut made: Vec<Organism> = Vec::with_capacity(c.count() as usize);
            while let Some(m) = c.take_any(rng, true) {
                let entry = m.entry.expect("expansion always yields a genome");
                let mut o = Organism::new(0, c.owner, entry.genome, m.sex, Vec2::ZERO);
                o.lineage = c.lineage;
                o.generation = entry.generation;
                let age = (today - m.birth_day).max(0.0);
                o.age_ticks = (age * tpd) as u32;
                let progress = (age as f32 / o.traits.maturity_days).min(1.0);
                o.growth = o.traits.newborn_growth + (1.0 - o.traits.newborn_growth) * progress;
                if progress >= 1.0 {
                    o.matured = true;
                    o.growth = 1.0;
                    o.needs.breeding = rng.gen_range(0.0..40.0);
                    o.offspring_raised = stochastic_round(raised_each, rng);
                    o.fat = c.fat;
                }
                o.needs.hunger = c.hunger;
                o.health = c.health.clamp(1.0, 100.0);
                o.pos = self.place_in_region(r, &tiles, &o, rng);
                o.range = o.pos;
                made.push(o);
            }
            // Dependent young of caring species go to a mother.
            let mothers: Vec<usize> = (0..made.len()).filter(|&k| made[k].matured && made[k].sex == Sex::Female).collect();
            let mut ids: Vec<OrgId> = Vec::with_capacity(made.len());
            let first_id = self.next_id;
            for k in 0..made.len() {
                ids.push(first_id + k as OrgId);
            }
            for k in 0..made.len() {
                let o = &made[k];
                let dependent = !o.matured && o.growth < 0.7 && o.traits.birth != BirthType::AbandonedEggs;
                if !dependent || mothers.is_empty() {
                    continue;
                }
                let m = mothers[rng.gen_range(0..mothers.len())];
                let at = self.map.clamp(made[m].pos + Vec2::new(rng.gen_range(-0.5..0.5), rng.gen_range(-0.5..0.5)));
                let o = &mut made[k];
                o.mother = Some(ids[m]);
                if self.passable_for(o, at) {
                    o.pos = at;
                    o.range = at;
                }
            }
            for o in made {
                self.insert(o);
                self.lod.stats.expanded += 1;
            }
        }
    }

    /// Recompute cohort totals per owner and lineage.
    fn recount_lod(&mut self) {
        let today = self.today();
        self.lod.owners.clear();
        self.lod.lineages.clear();
        self.lod.lineages.resize(self.lineages.len(), LineageCount::default());
        for cs in &self.lod.regions {
            for c in cs {
                let n = c.count() as usize;
                if n == 0 {
                    continue;
                }
                *self.lod.owners.entry(c.owner).or_default() += n;
                if let Some(l) = self.lod.lineages.get_mut(c.lineage as usize) {
                    let a = c.adults(today);
                    l.alive += n;
                    l.adults += (a[0] + a[1]) as usize;
                }
            }
        }
    }

    /// Add cohort adults' alleles to a legacy snapshot: each pool genome
    /// stands for `adults / pool size` breeding adults.
    pub(super) fn lod_legacy(&self, s: &mut LegacySnapshot) {
        let today = self.today();
        for (_, c) in self.cohorts() {
            if c.pool.is_empty() {
                continue;
            }
            let a = c.adults(today);
            let adults = (a[0] + a[1]) as f64;
            if adults == 0.0 {
                continue;
            }
            s.breeding_adults += adults as usize;
            let w = adults / c.pool.len() as f64;
            let rw = c.raised / c.pool.len() as f64;
            for e in &c.pool {
                e.genome.tally_origins(w, &mut s.alleles);
                if rw > 0.0 {
                    e.genome.tally_origins(rw, &mut s.weighted);
                }
            }
        }
        s.total_alleles = s.alleles.values().sum();
        s.weighted_total = s.weighted.values().sum();
    }

    /// Mean temperature of region `r` without the day/night swing.
    fn region_temp(&self, r: usize) -> f32 {
        let cx = self.map.chunks_x;
        let (x0, y0) = ((r % cx) * CHUNK, (r / cx) * CHUNK);
        let (x1, y1) = ((x0 + CHUNK).min(self.map.width), (y0 + CHUNK).min(self.map.height));
        let mut sum = 0.0;
        for y in y0..y1 {
            for x in x0..x1 {
                sum += self.map.tiles[self.map.idx(x, y)].base_temp;
            }
        }
        sum / ((x1 - x0) * (y1 - y0)).max(1) as f32 + self.season_offset
    }

    /// The up to 8 regions around `r`.
    fn region_ring(&self, r: usize) -> Vec<usize> {
        let (cx, cy) = (self.map.chunks_x as i64, self.map.chunks_y as i64);
        let (x, y) = (r as i64 % cx, r as i64 / cx);
        let mut out = Vec::with_capacity(8);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if (dx, dy) != (0, 0) && nx >= 0 && ny >= 0 && nx < cx && ny < cy {
                    out.push((ny * cx + nx) as usize);
                }
            }
        }
        out
    }

    /// The up to 4 regions sharing an edge with `r`.
    fn region_neighbors(&self, r: usize) -> Vec<usize> {
        let (cx, cy) = (self.map.chunks_x, self.map.chunks_y);
        let (x, y) = (r % cx, r / cx);
        let mut out = Vec::with_capacity(4);
        if x > 0 {
            out.push(r - 1);
        }
        if x + 1 < cx {
            out.push(r + 1);
        }
        if y > 0 {
            out.push(r - cx);
        }
        if y + 1 < cy {
            out.push(r + cx);
        }
        out
    }

    /// One day of every cohort.
    fn step_cohorts(&mut self) {
        let mut room: Vec<usize> = (0..self.lineages.len() as u32).map(|l| self.cap_room(l)).collect();
        let mut arrivals: Vec<Arrival> = Vec::new();
        // Adult males per (region, lineage) at the start of the day: females
        // find mates in their own or a neighboring region.
        let today = self.today();
        let mut males: HashMap<(usize, u32), u32> = HashMap::new();
        for (r, cs) in self.lod.regions.iter().enumerate() {
            for c in cs {
                let m = c.adults(today)[1];
                if m > 0 {
                    males.insert((r, c.lineage), m);
                }
            }
        }
        let regions = self.lod.regions.len();
        let live = |w: &World, r: usize| !w.lod.active[r] && !w.lod.regions[r].is_empty();
        // Needs of every cohort, then hunting (over neighboring regions
        // too), then each region's grazing, deaths, births and migration.
        let mut days: Vec<Vec<Day>> = vec![Vec::new(); regions];
        for (r, d) in days.iter_mut().enumerate() {
            if live(self, r) {
                *d = self.cohort_needs(r);
            }
        }
        for r in 0..regions {
            if live(self, r) {
                let mut rng = rng_for(self.config.seed, stream::LOD, self.day(), (regions + r) as u64);
                self.hunt(r, &mut days, &mut rng);
            }
        }
        for (r, d) in days.into_iter().enumerate() {
            if !live(self, r) {
                continue;
            }
            let mut cohorts = std::mem::take(&mut self.lod.regions[r]);
            let mut rng = rng_for(self.config.seed, stream::LOD, self.day(), r as u64);
            self.cohort_day(r, &mut cohorts, d, &mut room, &males, &mut arrivals, &mut rng);
            cohorts.retain(|c| c.count() > 0);
            self.lod.regions[r] = cohorts;
        }
        let k = self.config.lod.pool_size.max(1.0) as usize;
        let mut rng = rng_for(self.config.seed, stream::LOD, self.day(), u64::MAX - 1);
        let day = self.day();
        for a in arrivals {
            let Some(entry) = a.member.entry else { continue };
            if self.lod.active[a.to] {
                self.emigrate_as_agent(a.from, a.to, a.lineage, a.owner, a.member.sex, a.member.birth_day, entry, a.hunger, &mut rng);
            } else {
                let c = cohort_mut(&mut self.lod.regions[a.to], a.lineage, a.owner);
                c.add(a.member.sex, a.member.birth_day, entry, k, &mut rng);
                c.blend_state(a.hunger, a.health, a.fat);
                c.ensure_rep(day, false);
            }
        }
    }

    /// A migrating cohort member enters active region `to` as an agent, on
    /// a usable tile near the border with `from`.
    #[allow(clippy::too_many_arguments)]
    fn emigrate_as_agent(
        &mut self,
        from: usize,
        to: usize,
        lineage: u32,
        owner: PlayerId,
        sex: Sex,
        birth_day: f64,
        entry: PoolEntry,
        hunger: f32,
        rng: &mut SimRng,
    ) {
        let today = self.today();
        let tpd = self.config.ticks_per_day as f64;
        let mut o = Organism::new(0, owner, entry.genome, sex, Vec2::ZERO);
        o.lineage = lineage;
        o.generation = entry.generation;
        let age = (today - birth_day).max(0.0);
        o.age_ticks = (age * tpd) as u32;
        let progress = (age as f32 / o.traits.maturity_days).min(1.0);
        o.growth = o.traits.newborn_growth + (1.0 - o.traits.newborn_growth) * progress;
        if progress >= 1.0 {
            o.matured = true;
            o.growth = 1.0;
        }
        o.needs.hunger = hunger;
        let tiles = self.passable_tiles(to, lineage, &o).to_vec();
        if tiles.is_empty() {
            return;
        }
        let cx = self.map.chunks_x;
        let src = Map::tile_center((from % cx) * CHUNK + CHUNK / 2, (from / cx) * CHUNK + CHUNK / 2);
        let w = self.map.width;
        let mut best = (f32::MAX, Vec2::ZERO);
        for _ in 0..8 {
            let t = tiles[rng.gen_range(0..tiles.len())] as usize;
            let p = Map::tile_center(t % w, t / w);
            let d = p.dist(src);
            if d < best.0 {
                best = (d, p);
            }
        }
        o.pos = best.1;
        o.range = best.1;
        self.insert(o);
        self.lod.stats.emigrated += 1;
    }

    /// Today's needs of the cohorts of region `r` (refreshing their traits).
    fn cohort_needs(&mut self, r: usize) -> Vec<Day> {
        let bal = self.config.balance;
        let today = self.today();
        let day = self.day();
        let season = self.season();
        let temp = self.region_temp(r);
        let mut cs = std::mem::take(&mut self.lod.regions[r]);
        for c in cs.iter_mut() {
            c.ensure_rep(day, true);
        }
        let mut d = vec![Day::default(); cs.len()];
        for (i, c) in cs.iter().enumerate() {
            let Some(rep) = c.rep.as_deref() else { continue };
            let tiles = self.passable_tiles(r, c.lineage, &rep.probe).to_vec();
            let x = &mut d[i];
            x.tiles = tiles.len();
            x.water = tiles.iter().any(|&t| self.map.water_dist[t as usize] <= WATER_REACH);
            let body = rep.probe.traits.body;
            x.veg = tiles.iter().map(|&t| self.map.tiles[t as usize].reachable_veg(&body, bal.canopy_share)).sum();
            x.dormant = (rep.winter > 0.0 && temp < bal.dormancy_temp && matches!(season, Season::Autumn | Season::Winter))
                || (rep.estivation > 0.0 && temp > bal.estivation_temp && season == Season::Summer);
            x.units = c
                .ages
                .iter()
                .map(|(&b, n)| {
                    let age = bucket_age(b, today);
                    let g = if age >= rep.maturity as f64 { 1.0 } else { rep.growth_at(age) };
                    (n[0] + n[1]) as f32 * g.powf(0.75)
                })
                .sum();
            let thermo = if rep.warm() {
                1.0 + 0.04 * (12.0 - 30.0 * rep.insulation - temp).max(0.0)
            } else {
                let activity = ((temp + 5.0 - 2.0) / 20.0).clamp(0.15, 1.0);
                0.4 + 0.6 * activity
            };
            let diet = 1.0 - bal.carnivore_hunger_discount * rep.carnivory;
            let mut h = bal.hunger_rate * rep.metabolism * diet * thermo * ACTIVITY;
            if x.dormant {
                h *= bal.dormant_metabolism;
            }
            x.hunger = h;
            let fph = rep.food_per_hunger();
            let carn = rep.carnivory;
            x.carnivory = carn;
            // Food units for the whole need: hunters (as for agents, carnivory
            // above 0.45) hunt first, and those that can digest plants
            // (omnivores) graze for the rest.
            x.food = x.units * h * fph;
            x.meat_need = if carn > HUNTER { x.food / carn } else { 0.0 };
        }
        self.lod.regions[r] = cs;
        d
    }

    /// Hunting by the predator cohorts of region `r`: each takes prey in
    /// its own and the 8 surrounding abstract regions (its hunting range) in
    /// proportion to what is exposed, up to its need (type II response).
    fn hunt(&mut self, r: usize, days: &mut [Vec<Day>], rng: &mut SimRng) {
        let bal = self.config.balance;
        let cfg = self.config.lod;
        let today = self.today();
        let mut range = vec![r];
        range.extend(self.region_ring(r).into_iter().filter(|&nr| !self.lod.active[nr]));
        let cx = self.map.chunks_x;
        let range_tiles: usize = range
            .iter()
            .map(|&q| {
                let (x0, y0) = ((q % cx) * CHUNK, (q / cx) * CHUNK);
                ((x0 + CHUNK).min(self.map.width) - x0) * ((y0 + CHUNK).min(self.map.height) - y0)
            })
            .sum();
        for p in 0..self.lod.regions[r].len() {
            let x = days[r][p];
            if x.meat_need > 0.0 && x.dormant {
                days[r][p].s_meat = 1.0;
            }
            let pc = &self.lod.regions[r][p];
            if x.meat_need <= 0.0 || x.dormant || pc.count() == 0 {
                continue;
            }
            let Some(prep) = pc.rep.clone() else { continue };
            let plineage = pc.lineage;
            // Pack hunters hunt together once enough adults are around
            // (cooperative hunting, as the group trait gives agents).
            let pack = GroupTrait::CooperativeHunting.is_carrier(&prep.probe.pheno) && {
                let a = pc.adults(today);
                a[0] + a[1] >= 3
            };
            let max_mass = decide::max_prey_mass(&prep.probe, if pack { 1.0 } else { 0.0 });
            // (region, cohort, juveniles?, exposed count, mass each)
            let mut prey: Vec<(usize, usize, bool, f32, f32)> = Vec::new();
            for &nr in &range {
                for (q, qc) in self.lod.regions[nr].iter().enumerate() {
                    if qc.lineage == plineage {
                        continue;
                    }
                    let Some(qrep) = qc.rep.as_deref() else { continue };
                    if qrep.carnivory >= 0.4 {
                        continue;
                    }
                    let dormant = days[nr].get(q).is_some_and(|d| d.dormant);
                    // Toxic prey is avoided, stealthy prey is seen late,
                    // burrowers spend much of their time sheltered, dormant
                    // prey hides in its den.
                    let exposure = (1.0 - qrep.toxins)
                        * (1.0 - bal.stealth_effect * qrep.stealth)
                        * (1.0 - 0.5 * bal.burrow_shelter * qrep.burrow).max(0.0)
                        * if dormant { 1.0 - bal.den_concealment } else { 1.0 };
                    let a = qc.adults(today);
                    let adults = (a[0] + a[1]) as f32;
                    let juveniles = qc.count() as f32 - adults;
                    let juv_mass = qrep.mass * 0.5 * (1.0 + qrep.newborn_growth);
                    if qrep.mass < max_mass && adults > 0.0 {
                        prey.push((nr, q, false, adults * exposure, qrep.mass));
                    }
                    // Young of caring species are guarded by their mothers.
                    let guarded = if qrep.probe.traits.birth == BirthType::AbandonedEggs {
                        1.0
                    } else {
                        (1.0 - 0.5 * bal.parent_defense).max(0.0)
                    };
                    let exposure = exposure * guarded;
                    if juv_mass < max_mass && juveniles > 0.0 {
                        prey.push((nr, q, true, juveniles * exposure, juv_mass));
                    }
                }
            }
            let exposed: f32 = prey.iter().map(|e| e.3).sum();
            if exposed <= 0.0 {
                continue;
            }
            let density = exposed / range_tiles as f32;
            let mean_mass = prey.iter().map(|e| e.3 * e.4).sum::<f32>() / exposed;
            let meat_each = mean_mass * bal.meat_per_kg * cfg.carcass_use;
            let wanted = x.meat_need / meat_each.max(1e-3);
            let kills = wanted * density / (density + cfg.prey_half_density.max(1e-6));
            let mut meat = 0.0;
            for &(nr, q, juvenile, e, mass) in &prey {
                let expected = kills * e / exposed;
                let qc = &mut self.lod.regions[nr][q];
                let mat = qc.maturity_days() as f64;
                let mut killed = 0;
                for _ in 0..stochastic_round(expected as f64, rng) {
                    let Some((b, s)) = qc.pick(rng, |b| (bucket_age(b, today) < mat) == juvenile) else { break };
                    qc.remove_at(b, s, rng, false);
                    killed += 1;
                }
                meat += killed as f32 * mass * bal.meat_per_kg * cfg.carcass_use;
                let lineage = qc.lineage;
                self.counters.kills += killed;
                *self.counters.deaths.entry(DeathCause::Predation).or_default() += killed;
                *self.counters.deaths_by_lineage.entry((lineage, DeathCause::Predation)).or_default() += killed;
                if juvenile && killed > 0 {
                    *self.counters.juvenile_deaths.entry(lineage).or_default() += killed;
                }
            }
            days[r][p].s_meat = (meat / x.meat_need).min(1.0);
        }
    }

    /// One day of the cohorts of region `r` after hunting: grazing,
    /// condition, deaths, births and migration.
    #[allow(clippy::too_many_arguments)]
    fn cohort_day(
        &mut self,
        r: usize,
        cs: &mut [Cohort],
        mut d: Vec<Day>,
        room: &mut [usize],
        males: &HashMap<(usize, u32), u32>,
        arrivals: &mut Vec<Arrival>,
        rng: &mut SimRng,
    ) {
        let bal = self.config.balance;
        let cfg = self.config.lod;
        let today = self.today();
        let season = self.season();
        let breed_factor = season.breeding_factor();
        let k = cfg.pool_size.max(1.0) as usize;
        let cx = self.map.chunks_x;
        let n = cs.len();
        d.resize(n, Day::default());

        // Plant food for what meat didn't cover.
        let mut plant_demand = 0.0f32;
        for x in d.iter_mut() {
            let eff = 1.0 - x.carnivory;
            let rest = if x.meat_need > 0.0 { 1.0 - x.s_meat } else { 1.0 };
            x.plant_need = if eff >= PLANT_EATER { x.food * rest / eff } else { 0.0 };
            plant_demand += x.plant_need;
        }
        let mut eaten = 0.0f32;
        for x in d.iter_mut() {
            x.s_plant = if x.plant_need > 0.0 && plant_demand > 0.0 {
                (cfg.graze_fraction * x.veg / plant_demand).min(1.0)
            } else if 1.0 - x.carnivory >= PLANT_EATER {
                1.0
            } else {
                0.0
            };
            eaten += x.s_plant * x.plant_need;
        }
        if eaten > 0.0 {
            let (x0, y0) = ((r % cx) * CHUNK, (r / cx) * CHUNK);
            let (x1, y1) = ((x0 + CHUNK).min(self.map.width), (y0 + CHUNK).min(self.map.height));
            let mut total = 0.0;
            for y in y0..y1 {
                for x in x0..x1 {
                    total += self.map.tiles[self.map.idx(x, y)].veg;
                }
            }
            let keep = if total > 0.0 { (1.0 - eaten / total).max(0.0) } else { 0.0 };
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = self.map.idx(x, y);
                    self.map.tiles[i].veg *= keep;
                }
            }
        }


        // Condition, deaths, births and migration per cohort.
        let neighbors = self.region_neighbors(r);
        let ring = self.region_ring(r);
        for i in 0..n {
            let Some(rep) = cs[i].rep.clone() else { continue };
            let x = d[i];
            let c = &mut cs[i];
            if c.count() == 0 {
                continue;
            }
            let plant_cap = ((1.0 - x.carnivory) / cfg.plant_diet.max(1e-3)).min(1.0);
            let s = if x.meat_need > 0.0 {
                x.s_meat + (1.0 - x.s_meat) * x.s_plant * plant_cap
            } else {
                x.s_plant * plant_cap
            };
            // Hunters live on plants between kills but breed on meat: plants
            // count toward breeding condition only in proportion to how well
            // they digest them (agents grazing in passing stay hungry, and
            // hungry agents don't court).
            let breed_s = if x.meat_need > 0.0 {
                x.s_meat + (1.0 - x.s_meat) * x.s_plant * plant_cap * ((1.0 - x.carnivory) / (1.0 - HUNTER)).min(1.0)
            } else {
                s
            };
            // Fat pays for a shortfall; surplus in the fattening season and
            // food caching build it up.
            let mut deficit = x.hunger * (1.0 - s);
            let pay = deficit.min(c.fat);
            c.fat -= pay;
            deficit -= pay;
            let fattens = rep.probe.traits.seasonal.fattens_in(season) || (rep.caching > 0.0 && season == Season::Autumn);
            if s >= 0.95 && fattens {
                let cap = bal.fat_capacity * (1.0 + rep.fat_stores) + bal.cache_capacity * rep.caching;
                c.fat = (c.fat + 0.5 * x.hunger.max(10.0)).min(cap);
            }
            c.hunger = if deficit > 0.0 { (c.hunger + deficit).min(100.0) } else { (c.hunger - 30.0).max(10.0) };
            let fed = if x.hunger > 0.0 { 1.0 - deficit / x.hunger } else { 1.0 };
            if c.hunger >= 90.0 {
                c.health = (c.health - bal.starvation_damage * (1.0 - fed)).max(1.0);
            } else if c.hunger < 70.0 {
                c.health = (c.health + 15.0).min(100.0);
            }
            let starve = (1.0 - fed) * 0.5 * ((c.hunger - 60.0) / 40.0).clamp(0.0, 1.0);
            let thirst = if !x.water && !x.dormant { 0.4 } else { 0.0 };
            let stranded = if x.tiles == 0 { 1.0 } else { 0.0 };
            let density = c.count() as f32 / x.tiles.max(1) as f32 * cfg.clump;
            let larval = if rep.birth() == BirthType::AbandonedEggs {
                super::apply::larval_hazard((density * KIN_AREA).min(u16::MAX as f32) as u16, bal.larval_competition)
            } else {
                0.0
            };
            let attrition = bal.adult_mortality * rep.pace;

            // Deaths by bucket and sex.
            let mat = rep.maturity as f64;
            let buckets: Vec<(i64, [u32; 2])> = c.ages.iter().map(|(&b, &n)| (b, n)).collect();
            let mut matured_today = 0u32;
            for (b, counts) in buckets {
                let age = bucket_age(b, today);
                let adult = age >= mat;
                if adult && age - 1.0 < mat {
                    matured_today += counts[0] + counts[1];
                }
                let old = if adult {
                    let frac = age as f32 / rep.lifespan;
                    if frac > 1.3 {
                        50.0
                    } else if frac > 0.8 {
                        4.0 * (frac - 0.8)
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                let causes: [(f32, DeathCause); 6] = [
                    (old, DeathCause::OldAge),
                    (if adult { attrition } else { cfg.juvenile_mortality }, DeathCause::Attrition),
                    (if adult { 0.0 } else { larval }, DeathCause::Attrition),
                    (starve, DeathCause::Starvation),
                    (thirst, DeathCause::Thirst),
                    (stranded, DeathCause::Drowning),
                ];
                let total: f32 = causes.iter().map(|c| c.0).sum();
                if total <= 0.0 {
                    continue;
                }
                let p = 1.0 - (-(total as f64)).exp();
                for (s, &count) in counts.iter().enumerate() {
                    let dead = binomial(count, p, rng);
                    for _ in 0..dead {
                        c.remove_at(b, s, rng, false);
                        let mut pick = rng.gen::<f32>() * total;
                        let cause = causes
                            .iter()
                            .find(|(h, _)| {
                                pick -= h;
                                pick < 0.0
                            })
                            .map_or(DeathCause::Attrition, |c| c.1);
                        *self.counters.deaths.entry(cause).or_default() += 1;
                        *self.counters.deaths_by_lineage.entry((c.lineage, cause)).or_default() += 1;
                        if !adult {
                            *self.counters.juvenile_deaths.entry(c.lineage).or_default() += 1;
                        }
                    }
                }
            }
            // Both parents are credited for young reaching adulthood.
            c.raised += 2.0 * matured_today as f64;

            // Births.
            let a = c.adults(today);
            let room_l = &mut room[c.lineage as usize];
            let lineage = c.lineage;
            let mate = a[1] > 0 || ring.iter().any(|&nr| males.contains_key(&(nr, lineage)));
            if !x.dormant && a[0] > 0 && mate && *room_l > 0 && !c.pool.is_empty() {
                let condition = ((c.health / 100.0) * (1.0 - c.hunger / 200.0)).clamp(0.3, 1.0);
                let suppression = if rep.birth() == BirthType::AbandonedEggs || bal.breeding_density <= 0.0 {
                    1.0
                } else {
                    let dd = density * (1.0 - rep.group_tolerance) * rep.pace.sqrt();
                    1.0 / (1.0 + (dd / bal.breeding_density).powi(4))
                };
                let cycle = rep.gestation + rep.recovery + cfg.mate_search_days / breed_factor.max(0.05);
                let lambda = a[0] as f32 * rep.litter * condition * fed.min(breed_s) * suppression / cycle;
                let births = (poisson(lambda as f64, rng) as usize).min(*room_l);
                *room_l -= births;
                let mutation = self.config.mutation;
                for _ in 0..births {
                    let ma = &c.pool[rng.gen_range(0..c.pool.len())];
                    let pa = &c.pool[rng.gen_range(0..c.pool.len())];
                    let genome = Genome::inherit(&ma.genome, &pa.genome, c.owner, mutation, bal.recessive_mutation, rng);
                    let generation = ma.generation.max(pa.generation) + 1;
                    let sex = if rng.gen::<bool>() { Sex::Female } else { Sex::Male };
                    c.add(sex, today, PoolEntry { genome, generation }, k, rng);
                }
                if births > 0 {
                    let b = births as u64;
                    if rep.birth().lays_eggs() {
                        self.counters.hatched += b;
                    } else {
                        self.counters.births += b;
                    }
                    *self.counters.born.entry(c.lineage).or_default() += b;
                }
            }

            // Migration to neighboring regions the body can use.
            let push = 1.0
                + 2.0 * (1.0 - fed)
                + if mate { 0.0 } else { 2.0 }
                + if x.water { 0.0 } else { 3.0 }
                + if x.tiles == 0 { 10.0 } else { 0.0 };
            let rate = cfg.migration * (rep.wanderlust + 0.25) * push;
            let movers = binomial(c.count(), 1.0 - (-(rate as f64)).exp(), rng);
            if movers > 0 && !neighbors.is_empty() {
                let lineage = c.lineage;
                let (hunger, health, fat) = (c.hunger, c.health, c.fat);
                let usable: Vec<usize> =
                    neighbors.iter().copied().filter(|&nr| !self.passable_tiles(nr, lineage, &rep.probe).is_empty()).collect();
                if usable.is_empty() {
                    continue;
                }
                let c = &mut cs[i];
                for _ in 0..movers {
                    let Some(member) = c.take_any(rng, true) else { break };
                    arrivals.push(Arrival {
                        from: r,
                        to: usable[rng.gen_range(0..usable.len())],
                        lineage,
                        owner: c.owner,
                        member,
                        hunger,
                        health,
                        fat,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldConfig;
    use crate::presets;

    fn lod_world(seed: u64, size: usize) -> World {
        let mut config = WorldConfig { seed, width: size, height: size, ..Default::default() };
        config.lod.enabled = true;
        presets::standard_world(config, 1)
    }

    fn allele_shares(genomes: &[&Genome]) -> BTreeMap<PlayerId, f64> {
        let mut out = BTreeMap::new();
        for g in genomes {
            g.tally_origins(1.0, &mut out);
        }
        let total: f64 = out.values().sum();
        out.values_mut().for_each(|v| *v /= total.max(1.0));
        out
    }

    /// Collapse every region of a world with mixed (hybrid) genomes, expand
    /// it again, and compare.
    fn round_trip(pool_size: f32, share_tolerance: f64) {
        let mut w = lod_world(5, 64);
        w.config.lod.pool_size = pool_size;
        // Everything in focus while the agents run.
        w.set_focus(1, vec![Vec2::new(32.0, 32.0)]);
        w.config.lod.active_radius = 1000.0;
        w.run_days(3);
        // Mix origin tags so the shares are not trivially 1.0.
        let n = w.organisms.len();
        let mut rng = rng_for(1, 99, 0, 0);
        for i in 0..n {
            let j = rng.gen_range(0..n);
            if w.organisms[i].alive && w.organisms[j].alive && w.organisms[i].lineage != w.organisms[j].lineage {
                let none = MutationParams { rate: 0.0, sd: 0.0 };
                let g = Genome::inherit(&w.organisms[i].genome, &w.organisms[j].genome, 0, none, 0.0, &mut rng);
                w.organisms[i].genome = g;
            }
        }
        // Clutches would join as newborns; leave them out of the comparison.
        w.clutches.clear();
        w.clutch_at.clear();
        let before = w.lineage_counts();
        let alive: Vec<&Organism> = w.organisms.iter().filter(|o| o.alive).collect();
        let shares_before = allele_shares(&alive.iter().map(|o| &o.genome).collect::<Vec<_>>());
        let adults_before: usize = before.iter().map(|c| c.adults).sum();

        // Collapse everything.
        for a in w.lod.active.iter_mut() {
            *a = false;
        }
        let mut rng = rng_for(2, 2, 2, 2);
        w.absorb_agents(&mut rng);
        w.recount_lod();
        assert_eq!(w.alive_count(), 0);
        let abs = w.abstract_counts();
        for (l, c) in before.iter().enumerate() {
            assert_eq!(abs[l].alive, c.alive, "lineage {l} count after collapse");
        }

        // Expand everything.
        for r in 0..w.lod.active.len() {
            w.lod.active[r] = true;
            w.expand_region(r, &mut rng);
        }
        w.recount_lod();
        assert_eq!(w.abstract_count(), 0);
        let after = w.lineage_counts();
        for (l, c) in before.iter().enumerate() {
            assert_eq!(after[l].alive, c.alive, "lineage {l} count after expansion");
        }
        let adults_after: usize = after.iter().map(|c| c.adults).sum();
        let tol = (adults_before as f32 * 0.1).max(3.0) as usize;
        assert!(adults_after.abs_diff(adults_before) <= tol, "adults {adults_before} -> {adults_after}");
        let alive: Vec<&Organism> = w.organisms.iter().filter(|o| o.alive).collect();
        for o in &alive {
            assert!(w.passable_for(o, o.pos), "{} placed on an unusable tile", w.lineages[o.lineage as usize].name);
        }
        let shares_after = allele_shares(&alive.iter().map(|o| &o.genome).collect::<Vec<_>>());
        for (p, s) in &shares_before {
            let t = shares_after.get(p).copied().unwrap_or(0.0);
            assert!((s - t).abs() <= share_tolerance, "player {p}: allele share {s:.4} -> {t:.4}");
        }
    }

    #[test]
    fn round_trip_with_exact_pools() {
        // The pool holds every genome: the same multiset of genomes comes back.
        round_trip(10_000.0, 1e-9);
    }

    #[test]
    fn round_trip_with_sampled_pools() {
        // Sampled pools conserve tags in expectation; one draw is noisy.
        round_trip(16.0, 0.05);
    }

    #[test]
    fn lod_off_changes_nothing() {
        let build = |lod: bool| {
            let mut config = WorldConfig { seed: 9, width: 64, height: 64, ..Default::default() };
            config.lod.enabled = false;
            // A focus is ignored with LOD off.
            let mut w = presets::standard_world(config, 1);
            if lod {
                w.set_focus(1, vec![Vec2::new(10.0, 10.0)]);
            }
            w.run_days(2);
            w
        };
        let (a, b) = (build(false), build(true));
        let pa: Vec<_> = a.organisms.iter().map(|o| (o.id, o.pos, o.needs.hunger)).collect();
        let pb: Vec<_> = b.organisms.iter().map(|o| (o.id, o.pos, o.needs.hunger)).collect();
        assert_eq!(pa, pb);
        assert_eq!(a.legacy, b.legacy);
        assert_eq!(b.abstract_count(), 0);
    }

    #[test]
    fn focus_switches_regions() {
        let mut w = lod_world(3, 96);
        let start = w.population();
        let focus = Vec2::new(48.0, 48.0);
        w.set_focus(1, vec![focus]);
        w.step();
        let r = w.region_of(focus);
        assert!(w.region_is_active(r));
        let far = w.region_of(Vec2::new(0.5, 0.5));
        w.config.lod.active_radius = 10.0;
        w.config.lod.hysteresis = 4.0;
        w.run_days(1);
        assert!(!w.region_is_active(far), "far corner should be abstract");
        assert!(w.abstract_count() > 0);
        // Strays are absorbed once a day, so agents may cross into an
        // abstract region between day boundaries; right after absorption none
        // are left outside active regions.
        let mut rng = rng_for(w.config.seed, stream::LOD, w.tick(), 7);
        w.absorb_agents(&mut rng);
        assert!(w.organisms.iter().filter(|o| o.alive).all(|o| w.region_is_active(w.region_of(o.pos))));
        // Total population carried over (allowing a day of births and deaths).
        assert!(w.population() as f32 > start as f32 * 0.7);
        // Moving the focus to the corner expands it within an hour.
        w.set_focus(1, vec![Vec2::new(2.0, 2.0)]);
        for _ in 0..w.config.ticks_per_hour() {
            w.step();
        }
        assert!(w.region_is_active(far));
        // Dropping all focus collapses everything at the next day.
        w.set_focus(1, Vec::new());
        w.run_days(1);
        assert_eq!(w.alive_count(), 0);
        assert!(w.abstract_count() > 0);
        // Legacy counts the cohorts.
        let s = w.latest_legacy().unwrap();
        assert!(s.breeding_adults > 0 && s.total_alleles > 0.0);
        let total: f64 = s.alleles.values().sum();
        assert!((total - s.total_alleles).abs() < 1e-6);
    }

    #[test]
    fn possessed_organism_keeps_its_region_active() {
        let mut w = lod_world(4, 96);
        let id = w.organisms.iter().find(|o| o.owner == 1).unwrap().id;
        w.possess(1, id).unwrap();
        w.run_days(1);
        let o = w.organism(id).expect("possessed organism stays an agent");
        assert!(w.region_is_active(w.region_of(o.pos)));
    }

    /// A one-region world with hand-made cohorts.
    fn cohort_world() -> (World, usize) {
        let mut config = WorldConfig { seed: 2, width: 64, height: 64, ..Default::default() };
        config.lod.enabled = true;
        config.lod.migration = 0.0;
        let w = presets::standard_world(config, 1);
        // Pick a region with plenty of tiles every species can use.
        let r = (0..w.lod.regions.len())
            .max_by_key(|&r| {
                let cx = w.map.chunks_x;
                let (x0, y0) = ((r % cx) * CHUNK, (r / cx) * CHUNK);
                let mut land = 0;
                for y in y0..y0 + CHUNK {
                    for x in x0..x0 + CHUNK {
                        let t = &w.map.tiles[w.map.idx(x, y)];
                        land += (t.water_depth == 0.0 && w.map.water_dist[w.map.idx(x, y)] <= 4) as i32;
                    }
                }
                land
            })
            .unwrap();
        (w, r)
    }

    fn fill(w: &mut World, r: usize, lineage: u32, n: u32) {
        let mut rng = rng_for(7, 7, lineage as u64, 0);
        let src: Vec<Genome> = w.organisms.iter().filter(|o| o.lineage == lineage).map(|o| o.genome.clone()).collect();
        let owner = w.lineages[lineage as usize].owner;
        let today = w.today();
        let c = cohort_mut(&mut w.lod.regions[r], lineage, owner);
        for k in 0..n {
            let sex = if k % 2 == 0 { Sex::Female } else { Sex::Male };
            c.add(sex, today - 60.0, PoolEntry { genome: src[k as usize % src.len()].clone(), generation: 0 }, 64, &mut rng);
        }
        c.blend_state(20.0, 100.0, 0.0);
        c.ensure_rep(0, false);
    }

    fn only_cohorts(w: &mut World) {
        for o in w.organisms.iter_mut() {
            o.alive = false;
        }
        w.clutches.clear();
        for a in w.lod.active.iter_mut() {
            *a = false;
        }
        for cs in w.lod.regions.iter_mut() {
            cs.clear();
        }
    }

    fn run_cohort_days(w: &mut World, days: u32) {
        for _ in 0..days {
            w.recount_lod();
            w.step_cohorts();
            w.tick += w.config.ticks_per_day as u64;
        }
        w.recount_lod();
    }

    #[test]
    fn cohort_without_food_declines() {
        let make = |bare: bool| {
            let (mut w, r) = cohort_world();
            only_cohorts(&mut w);
            fill(&mut w, r, 1, 40); // antelope
            if bare {
                // Environment updates don't run here, so the map stays bare.
                for t in w.map.tiles.iter_mut() {
                    t.veg = 0.0;
                }
            }
            run_cohort_days(&mut w, 15);
            w
        };
        let (bare, fed) = (make(true), make(false));
        let (nb, nf) = (bare.abstract_count(), fed.abstract_count());
        assert!(nb < 20, "starving cohort should collapse, has {nb}");
        assert!(nf > 30, "fed cohort should hold, has {nf}");
        assert!(bare.counters.deaths.get(&DeathCause::Starvation).copied().unwrap_or(0) > 10);
    }

    #[test]
    fn predators_reduce_prey() {
        let make = |foxes: u32| {
            let (mut w, r) = cohort_world();
            only_cohorts(&mut w);
            fill(&mut w, r, 0, 120); // voles
            if foxes > 0 {
                fill(&mut w, r, 3, foxes); // foxes
            }
            run_cohort_days(&mut w, 20);
            w
        };
        let alone = make(0);
        let hunted = make(16);
        let voles = |w: &World| w.abstract_counts()[0].alive;
        assert!(hunted.counters.kills > 0);
        assert!(voles(&hunted) < voles(&alone), "voles: {} hunted vs {} alone", voles(&hunted), voles(&alone));
    }

    #[test]
    fn lod_runs_deterministically() {
        let run = || {
            let mut w = lod_world(8, 96);
            w.set_focus(1, vec![Vec2::new(30.0, 30.0)]);
            w.run_days(4);
            (w.population(), w.alive_count(), w.organisms.iter().map(|o| (o.id, o.pos)).collect::<Vec<_>>(), w.legacy.clone())
        };
        let (a, b) = (run(), run());
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
        assert_eq!(a.2, b.2);
        assert_eq!(a.3, b.3);
    }
}
