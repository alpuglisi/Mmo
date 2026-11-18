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
//!    snapshot, reset mating slots at a season change; then the chronicle
//!    (`chronicle`): catastrophes, species detection every few days, era
//!    records and achievements.
//! 3. Rebuild the spatial index.
//! 4. **Decide** (parallel, read-only): every organism scores its actions by
//!    need urgency × behavior genes and picks one.
//!
//! Between ticks, possessed organisms following a move order take
//! `possess_substeps - 1` movement sub-steps (`World::substep`, in
//! `commands`); the tick's own movement is the last one. `step` takes any
//! the caller skipped first.
//!
//! 5. **Apply** (shuffled order): movement (steering, with A* routes
//!    around blocked terrain; see `pathfinding`), eating, predation, mating,
//!    needs, death and birth. Movement and needs run on all cores, the
//!    interactions sequentially (`par_apply`; `parallel_apply = false` for
//!    the fully sequential apply).
//!
//! All randomness comes from per-(tick, organism) streams, so a run is
//! reproducible from its seed regardless of thread count.

mod apply;
mod chronicle;
pub mod commands;
mod decide;
mod density;
mod disease;
mod environment;
#[cfg(test)]
mod group_tests;
pub mod groups;
#[cfg(test)]
mod groups_tests;
mod hosts;
pub use hosts::HostTally;
#[cfg(test)]
mod attack_tests;
pub mod lod;
mod lod_meat;
#[cfg(test)]
mod morph_tests;
pub mod offline;
#[cfg(test)]
mod orders_tests;
mod par_apply;
#[cfg(test)]
mod path_tests;
#[cfg(test)]
mod predator_tests;
mod seasonal;
#[cfg(feature = "serde")]
mod snapshot;
mod spatial;
mod stagger;
pub mod toxic;

use std::collections::{BTreeMap, HashMap};

use rand::seq::SliceRandom;
use rand::Rng;

use crate::config::WorldConfig;
use crate::design::{DesignError, FounderDesign};
use crate::genome::{Genome, PlayerId};
use crate::group::{GroupField, GroupTrait};
use crate::legacy::LegacySnapshot;
use crate::math::Vec2;
use crate::organism::{DeathCause, OrgId, Organism, Sex};
use crate::pathfinding::PathScratch;
use crate::rng::{rng_for, stream};
use crate::terrain::{Map, Season, Traverse, Weather};

pub use chronicle::{biome_niche, diet_niche};
use density::SpeciesDensity;
pub use lod::{Cohort, LodStats, PoolEntry};
#[cfg(feature = "serde")]
pub use snapshot::{SnapshotError, SnapshotHeader, SNAPSHOT_VERSION, TERRAIN_VERSION};
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
/// Spatial index cell side in tiles.
const GRID_CELL: f32 = 4.0;

/// Items below which a parallel pass of light per-item work runs on the
/// calling thread (`light_min_len`).
const LIGHT_PAR: usize = 8192;

/// `with_min_len` for a parallel pass over `n` items of light work (a few
/// dozen nanoseconds each): below [`LIGHT_PAR`] items the whole pass runs
/// on the calling thread, as waking the pool's sleeping workers would cost
/// more than it saves; above it, pieces of at least a thousand. Where the
/// items are processed changes nothing they compute.
pub(crate) fn light_min_len(n: usize) -> usize {
    if n < LIGHT_PAR {
        usize::MAX
    } else {
        1024
    }
}

/// Hasher of the world's lookup maps (organism index by id, things by
/// tile): small integer keys, looked up many times a tick, never chosen by
/// an adversary, so the multiply-rotate hash of rustc's `FxHasher` instead
/// of the standard library's SipHash. Nothing iterates these maps in an
/// order that matters (snapshots write them sorted), so the hasher changes
/// no result.
#[derive(Clone, Copy, Default)]
pub(crate) struct IdHasher(u64);

impl IdHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

impl std::hash::Hasher for IdHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u32(&mut self, n: u32) {
        self.add(n as u64);
    }

    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }

    #[inline]
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

/// A `HashMap` with [`IdHasher`].
pub(crate) type IdMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<IdHasher>>;

/// Founding: side in tiles of a [`PointGrid`] cell.
const HOME_CELL: f32 = 8.0;

/// Points with a payload in coarse cells, for founding's home search: which
/// points could lie within a radius, without a scan of all of them.
struct PointGrid<T> {
    cols: usize,
    rows: usize,
    cells: Vec<Vec<(Vec2, T)>>,
}

impl<T> PointGrid<T> {
    fn new(map: &Map) -> Self {
        let cols = (map.width as f32 / HOME_CELL).ceil().max(1.0) as usize;
        let rows = (map.height as f32 / HOME_CELL).ceil().max(1.0) as usize;
        Self { cols, rows, cells: (0..cols * rows).map(|_| Vec::new()).collect() }
    }

    fn cell_of(&self, p: Vec2) -> (usize, usize) {
        let c = ((p.x / HOME_CELL).max(0.0) as usize).min(self.cols - 1);
        let r = ((p.y / HOME_CELL).max(0.0) as usize).min(self.rows - 1);
        (c, r)
    }

    fn insert(&mut self, p: Vec2, v: T) {
        let (c, r) = self.cell_of(p);
        self.cells[r * self.cols + c].push((p, v));
    }

    /// Calls `f` for every point within `radius` of `p` (and some farther
    /// ones), in no particular order.
    fn near(&self, p: Vec2, radius: f32, mut f: impl FnMut(Vec2, &T)) {
        let (c0, r0) = self.cell_of(Vec2::new(p.x - radius, p.y - radius));
        let (c1, r1) = self.cell_of(Vec2::new(p.x + radius, p.y + radius));
        for r in r0..=r1 {
            for cell in &self.cells[r * self.cols + c0..=r * self.cols + c1] {
                for (q, v) in cell {
                    f(*q, v);
                }
            }
        }
    }
}

/// Connected regions (8-neighbor) of the tiles of a `w` × `h` map that
/// `open` marks with anything but `u32::MAX`: a label per tile, regions
/// numbered in the order of their first tile, `u32::MAX` elsewhere
/// (`World::passable_regions`). Runs of open tiles in a row are joined
/// (union-find) to the runs of the row above that touch them, diagonals
/// included.
fn label_runs(open: &[u32], w: usize, h: usize) -> Vec<u32> {
    fn root(parent: &mut [u32], mut a: u32) -> u32 {
        while parent[a as usize] != a {
            parent[a as usize] = parent[parent[a as usize] as usize];
            a = parent[a as usize];
        }
        a
    }
    // Runs as (row start index + x0, x1 exclusive), row by row.
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    let mut parent: Vec<u32> = Vec::new();
    let mut prev = 0..0;
    for y in 0..h {
        let row = &open[y * w..(y + 1) * w];
        let first = runs.len();
        let mut x = 0;
        while x < w {
            if row[x] == u32::MAX {
                x += 1;
                continue;
            }
            let x0 = x;
            while x < w && row[x] != u32::MAX {
                x += 1;
            }
            let id = runs.len() as u32;
            runs.push((y, x0, x));
            parent.push(id);
            // Runs of the row above touching [x0 - 1, x + 1).
            for k in prev.clone() {
                let (_, a0, a1) = runs[k];
                if a1 + 1 > x0 && a0 < x + 1 {
                    let (ra, rb) = (root(&mut parent, k as u32), root(&mut parent, id));
                    if ra != rb {
                        let (lo, hi) = (ra.min(rb), ra.max(rb));
                        parent[hi as usize] = lo;
                    }
                }
            }
        }
        prev = first..runs.len();
    }
    let mut label = vec![u32::MAX; w * h];
    let mut of_root = vec![u32::MAX; runs.len()];
    let mut next = 0;
    for (k, &(y, x0, x1)) in runs.iter().enumerate() {
        let r = root(&mut parent, k as u32) as usize;
        if of_root[r] == u32::MAX {
            of_root[r] = next;
            next += 1;
        }
        label[y * w + x0..y * w + x1].fill(of_root[r]);
    }
    label
}

/// What founding's home search (`World::pick_home`) reads of the other
/// lineages, gathered once per founded lineage.
struct HomeSearch {
    /// `World::passable_regions` of the probe.
    region: Vec<u32>,
    /// Predators that could eat the founders, packs that only take them
    /// together included, weighted by their odds in a fight (as prey judge
    /// threats when deciding), in organism order.
    threats: Vec<(Vec2, f32)>,
    /// Index into `threats`, by position.
    threat_grid: PointGrid<u32>,
    /// Prey a hunter can reach: position, lineage, region.
    prey: Vec<(Vec2, u32, u32)>,
    prey_grid: PointGrid<u32>,
    /// Every organism (founders placed since included), with its carnivory.
    crowd: PointGrid<f32>,
}

impl HomeSearch {
    fn new(w: &World, probe: &Organism, lineage: u32) -> Self {
        let hunter = probe.traits.carnivory > 0.45;
        // Regions only tell a hunter's prey apart (reachable from home or
        // not); for anyone else every passable tile reads 0.
        let region = if hunter { w.passable_regions(probe) } else { w.passable_map(probe) };
        let region_at = |p: Vec2| {
            let (x, y) = w.map.tile_coords(p);
            region[w.map.idx(x, y)]
        };
        // Prey it can take alone: a pack placed by big prey it can only
        // take together eats through that prey lineage before it settles.
        let max_prey = decide::max_prey_mass(probe, 0.0);
        let resistance = probe.resistance();
        // One pass over the organisms (each read once): threats and prey in
        // organism order, every organism (the dead too) in the crowd.
        let mut threats: Vec<(Vec2, f32)> = Vec::new();
        let mut prey: Vec<(Vec2, u32, u32)> = Vec::new();
        let mut crowd = PointGrid::new(&w.map);
        for o in &w.organisms {
            crowd.insert(o.pos, o.traits.carnivory);
            if !o.alive || o.lineage == lineage {
                continue;
            }
            // Both ways to threaten the founders need a hunter's carnivory:
            // the rest are ruled out before the costlier tests.
            if o.traits.carnivory > 0.45 {
                let coop = if GroupTrait::CooperativeHunting.is_carrier(&o.pheno) { 1.0 } else { 0.0 };
                let can_take = probe.mass() < decide::max_prey_mass(o, coop);
                if decide::can_threaten(o, probe) || can_take {
                    let power = o.power() * (1.0 + coop);
                    threats.push((o.pos, (2.0 * power / (power + resistance + 1e-3)).min(1.0)));
                }
            }
            // Reachable prey: land hunters can't take newts in deep water.
            if hunter
                && o.traits.carnivory < 0.4
                && o.mass() < max_prey
                && !decide::can_threaten(o, probe)
                && region_at(o.pos) != u32::MAX
            {
                prey.push((o.pos, o.lineage, region_at(o.pos)));
            }
        }
        let mut threat_grid = PointGrid::new(&w.map);
        for (k, t) in threats.iter().enumerate() {
            threat_grid.insert(t.0, k as u32);
        }
        let mut prey_grid = PointGrid::new(&w.map);
        for (k, q) in prey.iter().enumerate() {
            prey_grid.insert(q.0, k as u32);
        }
        HomeSearch { region, threats, threat_grid, prey, prey_grid, crowd }
    }
}

/// Eggs waiting to hatch.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Clutch {
    pub pos: Vec2,
    pub owner: PlayerId,
    pub mother: OrgId,
    pub father: OrgId,
    pub generation: u32,
    pub lineage: u32,
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::by_parents::eggs"))]
    pub eggs: Vec<Genome>,
    pub hatch_tick: u64,
    /// Incubated by the mother; `None` for abandoned eggs.
    pub guardian: Option<OrgId>,
    /// Eggs of warm-blooded parents need more heat.
    pub warm_blooded: bool,
    /// Accumulated chilling; the clutch dies at 1.
    pub chill: f32,
    /// Accumulated overheating; the clutch dies at 1 (`Balance::egg_overheat`).
    pub overheat: f32,
    pub newborn_growth: f32,
    /// Mass of one egg in kg (food value when raided).
    pub egg_mass: f32,
}

/// A founding population and its descendants.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lineage {
    pub name: String,
    pub owner: PlayerId,
    pub founders: u32,
}

/// Living members of one lineage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineageCount {
    pub alive: usize,
    pub adults: usize,
}

/// One lineage's eggs (`Counters::eggs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EggCounts {
    pub laid: u64,
    pub hatched: u64,
    pub chilled: u64,
    pub raided: u64,
    /// Killed by heat (`Balance::egg_overheat`).
    pub overheated: u64,
}

/// Cumulative event counters.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Counters {
    pub births: u64,
    pub hatched: u64,
    pub eggs_lost: u64,
    /// Eggs per lineage: laid, hatched, chilled, overheated, raided.
    pub eggs: BTreeMap<u32, EggCounts>,
    pub deaths: BTreeMap<DeathCause, u64>,
    pub deaths_by_lineage: BTreeMap<(u32, DeathCause), u64>,
    /// Predation deaths by (victim's lineage, killer's lineage).
    pub kills_by_lineage: BTreeMap<(u32, u32), u64>,
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

/// With the `serde` feature a world serializes whole except for the
/// derived lookups marked `skip`, which `World::restore` rebuilds
/// (`world::snapshot`).
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct World {
    pub config: WorldConfig,
    pub map: Map,
    /// Map-wide mean weather, for reports. Movement rules use the local
    /// weather, `map.weather_at(pos)`.
    pub weather: Weather,
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::organisms"))]
    pub organisms: Vec<Organism>,
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::clutches"))]
    pub clutches: Vec<Clutch>,
    pub group: GroupField,
    pub counters: Counters,
    /// The last [`crate::legacy::LEGACY_KEPT`] daily legacy snapshots,
    /// oldest first.
    pub legacy: Vec<LegacySnapshot>,
    pub lineages: Vec<Lineage>,
    /// Species, eras, catastrophes and achievements (`world::chronicle`).
    pub chronicle: crate::chronicle::Chronicle,
    tick: u64,
    /// Possession sub-steps already taken since the last tick
    /// (`World::substep`), always below `config.possess_substeps`.
    substep: u32,
    next_id: OrgId,
    #[cfg_attr(feature = "serde", serde(skip))]
    index: IdMap<OrgId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    grid: SpatialGrid,
    owner_counts: BTreeMap<PlayerId, usize>,
    /// Living organisms per lineage, indexed like `lineages`.
    lineage_alive: Vec<usize>,
    density: SpeciesDensity,
    /// Migration orders by (player, group) (`World::command_migrate_group`).
    migrate_orders: BTreeMap<(PlayerId, u8), Vec2>,
    /// Claimed territories by (player, group) (`World::claim_territory`).
    territories: BTreeMap<(PlayerId, u8), groups::Territory>,
    /// Tick of each group's last territory claim (the claim cooldown).
    territory_claims: BTreeMap<(PlayerId, u8), u64>,
    /// Rally orders by (player, group) (`World::rally_group`); absent from
    /// snapshots before version 15 (`serde_util::before_v15`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v15"))]
    rallies: BTreeMap<(PlayerId, u8), groups::Rally>,
    /// Floor on a rally's length in ticks (`World::set_rally_min_ticks`),
    /// set by the host; not saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    rally_min_ticks: u64,
    /// Floor on the claim cooldown in ticks
    /// (`World::set_claim_cooldown_min_ticks`), set by the host; not saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    claim_cooldown_min_ticks: u64,
    /// Group orders that ended on their own, for the host
    /// (`World::take_group_events`); not saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    group_events: Vec<commands::GroupEvent>,
    slots_used: BTreeMap<PlayerId, u32>,
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::sorted_map"))]
    clutch_at: IdMap<(usize, usize), usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    path_scratch: PathScratch,
    /// Seasonal part of today's temperature (`Map::seasonal_offset`).
    season_offset: f32,
    /// Tile → organism index of every food cache (`seasonal::rebuild_caches`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::sorted_map"))]
    cache_at: IdMap<(usize, usize), usize>,
    /// Shared caches by (group region, lineage), and tile → key for raids
    /// (`seasonal`, `GroupTrait::SharedCaches`).
    shared_caches: BTreeMap<(usize, u32), seasonal::SharedCache>,
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::sorted_map"))]
    shared_cache_at: IdMap<(usize, usize), (usize, u32)>,
    /// Winter-dormant communal-den carriers per den cell (`seasonal`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::sorted_map"))]
    den_cells: IdMap<(usize, usize), u16>,
    /// Level of detail: focus points and the cohorts of abstract regions.
    lod: lod::Lod,
    /// Offline protection set by the caller; not saved (`world::offline`).
    #[cfg_attr(feature = "serde", serde(skip))]
    offline: offline::OfflineProtection,
    /// The newcomer grace's minimum length in ticks, set by the caller
    /// (`World::set_attack_grace_min_ticks`); not saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    grace_min_ticks: u64,
    /// Each lineage's grace start (`World::grace_start`), derived from
    /// `lineage_founded`; rebuilt on founding and restore, not saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    grace_starts: Vec<u64>,
    /// The cooldown `grace_starts` was computed with; a change of
    /// `Balance::attack_grace` falls back to computing afresh.
    #[cfg_attr(feature = "serde", serde(skip))]
    grace_starts_cooldown: u64,
    /// Wall-clock time spent in each phase of `step` (for `bench`).
    #[cfg_attr(feature = "serde", serde(skip))]
    pub phase_times: PhaseTimes,
    /// Last tick's intents with their organism's and target's ids, for
    /// staggered decisions (`Balance::decide_interval`; empty when off).
    /// Saved, so a restored world continues exactly (version 32).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v32::list"))]
    last_intents: Vec<(OrgId, Intent, Option<OrgId>)>,
    /// Extra directed-mating slots each player bought this season
    /// (`World::grant_mating_slots`), reset with `slots_used` at a season
    /// change (version 33).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v35::list"))]
    bonus_slots: BTreeMap<PlayerId, u32>,
    /// Rendezvous of agreed matings, organism → its partner
    /// (`World::set_rendezvous`): both walk toward each other unless a
    /// threat or an urgent need comes first. Set by the server for every
    /// accepted agreement and set again when it restores a world, so not
    /// saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    rendezvous: BTreeMap<OrgId, OrgId>,
    /// Attack orders of possessed organisms, attacker → target
    /// (`World::possess_attack`; version 36).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v36::list"))]
    attack_orders: BTreeMap<OrgId, OrgId>,
    /// The tick each lineage was founded, indexed like `lineages` (for
    /// `Balance::attack_grace`; version 36: empty from older snapshots,
    /// whose lineages count as founded at tick 0).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v36::list"))]
    lineage_founded: Vec<u64>,
    /// Migration deadlines and post-arrival settle circles by (player,
    /// group) (`groups::GroupPlans`; version 41: empty from older
    /// snapshots, whose migrations get a deadline from the restore).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v41"))]
    group_plans: groups::GroupPlans,
    /// What each lineage has learned about toxic prey: (predator lineage,
    /// prey lineage) → lesson, 0–1 (`World::toxic_lesson`,
    /// `Balance::toxic_learning`; version 44).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v44::list"))]
    toxic_lessons: BTreeMap<(u32, u32), f32>,
    /// Attack orders that ended on their own, for the host
    /// (`World::take_attack_events`); not saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    attack_events: Vec<commands::AttackEvent>,
    /// Each lineage's crowding this tick, indexed like `lineages`: the
    /// litter multiplier and the hunger multiplier
    /// (`Balance::lineage_crowding`; `[1, 1]` when off and for the wild).
    /// Recomputed from the organisms before every tick's decisions, so not
    /// saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    crowding: Vec<[f32; 2]>,
}

/// Steps from water a founder's tile needs (`Balance::dry_sites`).
const DRY_WATER_DIST: u16 = 2;
/// Share of dry ground around a home that is enough for a founding.
const DRY_HOME_SHARE: f32 = 0.6;
/// Extra home picks tried for one with enough dry ground.
const DRY_HOME_TRIES: usize = 8;

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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
        assert!(
            config.ticks_per_day >= 24 && config.ticks_per_day.is_multiple_of(24),
            "ticks_per_day must be a multiple of 24"
        );
        let map = Map::generate(config.seed, config.width, config.height);
        let weather = map.mean_weather();
        let group = GroupField::new(config.width, config.height, 16);
        let grid = SpatialGrid::new(config.width, config.height, GRID_CELL);
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
            chronicle: Default::default(),
            tick: 0,
            substep: 0,
            next_id: 1,
            index: IdMap::default(),
            grid,
            owner_counts: BTreeMap::new(),
            lineage_alive: Vec::new(),
            density: SpeciesDensity::new(config.width, config.height),
            migrate_orders: BTreeMap::new(),
            territories: BTreeMap::new(),
            territory_claims: BTreeMap::new(),
            rallies: BTreeMap::new(),
            rally_min_ticks: 0,
            claim_cooldown_min_ticks: 0,
            group_events: Vec::new(),
            slots_used: BTreeMap::new(),
            bonus_slots: BTreeMap::new(),
            attack_orders: BTreeMap::new(),
            rendezvous: BTreeMap::new(),
            lineage_founded: Vec::new(),
            group_plans: groups::GroupPlans::default(),
            attack_events: Vec::new(),
            toxic_lessons: BTreeMap::new(),
            crowding: Vec::new(),
            clutch_at: IdMap::default(),
            path_scratch: PathScratch::default(),
            season_offset: Map::seasonal_offset(0, config.days_per_year),
            cache_at: IdMap::default(),
            shared_caches: BTreeMap::new(),
            shared_cache_at: IdMap::default(),
            den_cells: IdMap::default(),
            lod,
            offline: Default::default(),
            grace_min_ticks: 0,
            grace_starts: Vec::new(),
            grace_starts_cooldown: 0,
            phase_times: PhaseTimes::default(),
            last_intents: Vec::new(),
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

    /// Whole days until winter starts (`Season::from_day`); 0 during winter.
    pub fn days_until_winter(&self) -> u32 {
        let year = self.config.days_per_year.max(1);
        // The first day of the year that `Season::from_day` calls winter.
        let start = (3 * year).div_ceil(4);
        start.saturating_sub(self.day_of_year())
    }

    /// Founders arriving just before (or in) winter start provisioned: not
    /// hungry, with their fat reserve full (`fat_capacity`, larger with the
    /// fat stores gene). Deterministic: no randomness.
    pub fn provision_for_winter(&mut self, ids: &[OrgId]) {
        for &id in ids {
            let Some(&i) = self.index.get(&id) else { continue };
            let cap = self.fat_capacity(&self.organisms[i]);
            let o = &mut self.organisms[i];
            if o.alive {
                o.needs.hunger = 0.0;
                o.fat = o.fat.max(cap);
            }
        }
    }

    pub fn is_daytime(&self) -> bool {
        (6..19).contains(&self.hour())
    }

    pub fn alive_count(&self) -> usize {
        self.owner_counts.values().sum()
    }

    /// Kill every living organism a player owns and drop their eggs; the
    /// number killed. For test hooks (the server's `EVO_E2E_HOOKS`) and
    /// tests, never the simulation itself.
    pub fn kill_all_of(&mut self, player: PlayerId, cause: DeathCause) -> usize {
        let doomed: Vec<usize> = (0..self.organisms.len())
            .filter(|&i| self.organisms[i].alive && self.organisms[i].owner == player)
            .collect();
        for &i in &doomed {
            self.kill(i, cause);
        }
        self.clutches.retain(|c| c.owner != player);
        self.clutch_at.clear();
        for (k, c) in self.clutches.iter().enumerate() {
            self.clutch_at.insert(self.map.tile_coords(c.pos), k);
        }
        doomed.len()
    }

    /// Drop a player's group orders (migrations, territories and their
    /// claim cooldowns, rallies): for a lineage founded after the last one
    /// died out, whose groups start afresh.
    pub fn clear_group_orders(&mut self, player: PlayerId) {
        self.migrate_orders.retain(|&(p, _), _| p != player);
        self.territories.retain(|&(p, _), _| p != player);
        self.territory_claims.retain(|&(p, _), _| p != player);
        self.rallies.retain(|&(p, _), _| p != player);
        self.group_plans.migrations.retain(|&(p, _), _| p != player);
        self.group_plans.settles.retain(|&(p, _), _| p != player);
    }

    /// Retire a player's lineage: every organism they own (agents and
    /// cohorts in abstract regions) and their eggs leave the world at once,
    /// and their group orders go. Not deaths: nothing is counted as one,
    /// no carcass is left, nothing goes wild (wild animals carrying the
    /// player's tagged alleles would keep scoring for them). The lineage
    /// stays in `lineages` as an extinct one. The number removed.
    pub fn retire_player(&mut self, player: PlayerId) -> usize {
        let mut removed = 0;
        for k in 0..self.organisms.len() {
            let o = &mut self.organisms[k];
            if !o.alive || o.owner != player {
                continue;
            }
            o.alive = false;
            o.death = None;
            o.possessed_by = None;
            o.order = None;
            let lineage = o.lineage;
            if let Some(c) = self.owner_counts.get_mut(&player) {
                *c = c.saturating_sub(1);
            }
            self.count_lineage(lineage, -1);
            removed += 1;
        }
        removed += self.retire_cohorts(player);
        self.clutches.retain(|c| c.owner != player);
        self.clutch_at.clear();
        for (k, c) in self.clutches.iter().enumerate() {
            self.clutch_at.insert(self.map.tile_coords(c.pos), k);
        }
        self.clear_group_orders(player);
        removed
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
            let wild_founders: u64 =
                self.lineages.iter().filter(|w| w.owner == crate::WILD).map(|w| w.founders as u64).sum();
            let cap = self.config.wild_cap as u64 * l.founders as u64 / wild_founders.max(1);
            let alive = self.lineage_alive.get(lineage as usize).copied().unwrap_or(0);
            (alive + self.lod.lineage_count(lineage).alive, cap as usize)
        } else {
            (self.owner_count(l.owner) + self.lod.owner_count(l.owner), self.player_ceiling())
        }
    }

    /// The hard cap on a player's lineage: `lineage_cap`, or with lineage
    /// crowding on (the soft cap) `crowding_ceiling` times it.
    pub fn player_ceiling(&self) -> usize {
        let bal = &self.config.balance;
        if bal.lineage_crowding > 0.0 {
            (self.config.lineage_cap as f32 * bal.crowding_ceiling.max(1.0)).round() as usize
        } else {
            self.config.lineage_cap
        }
    }

    /// The litter and hunger multipliers lineage crowding gives a player
    /// lineage whose crowding count (adults, plus juveniles at
    /// `crowding_juveniles`) is `count` (`Balance::lineage_crowding`).
    pub fn crowding_effect(&self, count: f32) -> [f32; 2] {
        let bal = &self.config.balance;
        if bal.lineage_crowding <= 0.0 {
            return [1.0, 1.0];
        }
        let x = count / self.config.lineage_cap.max(1) as f32;
        [1.0 / (1.0 + x.powf(bal.lineage_crowding)), 1.0 + bal.crowding_hunger * (x - 1.0).max(0.0)]
    }

    /// Each player's crowding count: adults, plus juveniles at
    /// `crowding_juveniles` (agents and cohort members alike).
    pub fn crowding_counts(&self) -> BTreeMap<PlayerId, f32> {
        let jw = self.config.balance.crowding_juveniles;
        // Per lineage first (a vector, cheap on every tick), then per owner.
        let mut per: Vec<(usize, usize)> = (0..self.lineages.len())
            .map(|l| {
                let c = self.lod.lineage_count(l as u32);
                (c.alive, c.adults)
            })
            .collect();
        for o in self.organisms.iter().filter(|o| o.alive) {
            if let Some(e) = per.get_mut(o.lineage as usize) {
                e.0 += 1;
                e.1 += usize::from(o.matured);
            }
        }
        let mut out: BTreeMap<PlayerId, f32> = BTreeMap::new();
        for (l, &(alive, adults)) in self.lineages.iter().zip(&per) {
            if l.owner != crate::WILD {
                *out.entry(l.owner).or_default() += adults as f32 + jw * alive.saturating_sub(adults) as f32;
            }
        }
        out
    }

    /// The crowding multipliers `[litter, hunger]` of `lineage` this tick.
    pub fn lineage_crowding(&self, lineage: u32) -> [f32; 2] {
        self.crowding.get(lineage as usize).copied().unwrap_or([1.0, 1.0])
    }

    /// Recompute every lineage's crowding (as each tick starts).
    fn refresh_crowding(&mut self) {
        self.crowding.clear();
        if self.config.balance.lineage_crowding <= 0.0 {
            return;
        }
        let counts = self.crowding_counts();
        let crowding: Vec<[f32; 2]> = self
            .lineages
            .iter()
            .map(|l| match counts.get(&l.owner) {
                Some(&n) if l.owner != crate::WILD => self.crowding_effect(n),
                _ => [1.0, 1.0],
            })
            .collect();
        self.crowding = crowding;
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

    /// Where a living organism is in `organisms`.
    pub fn organism_index(&self, id: OrgId) -> Option<usize> {
        self.index.get(&id).copied().filter(|&i| self.organisms[i].alive)
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
        let group_size = if owner == crate::WILD {
            12
        } else if self.config.balance.founding_food > 0.0 {
            // Founding by food (`Balance::founding_food`): a big founding
            // spreads over several homes.
            design.founders.clamp(1, Self::FOUNDING_GROUP)
        } else {
            design.founders.max(1)
        };
        self.found_lineage_grouped(owner, design, group_size)
    }

    /// [`World::found_lineage_unchecked`] with the founders placed in groups
    /// of `group_size`, each at its own home (`presets::NicheFounding`
    /// scatters predators as territorial pairs or small packs). All stay in
    /// the owner's main group.
    pub fn found_lineage_grouped(&mut self, owner: PlayerId, design: &FounderDesign, group_size: u32) -> Vec<OrgId> {
        let group_size = group_size.max(1);
        let mut rng = rng_for(self.config.seed, stream::FOUNDING, owner as u64, self.next_id);
        let mut probe = Organism::new(0, owner, design.founder_genome(owner, &mut rng), Sex::Female, Vec2::ZERO);
        probe.growth = 1.0;
        let spread = 3.0 + 10.0 * (1.0 - probe.pheno[crate::Gene::GroupTolerance]);
        let mut ids = Vec::with_capacity(design.founders as usize);
        let mut home = Vec2::ZERO;
        let lineage = self.lineages.len() as u32;
        self.lineages.push(Lineage { name: design.name.clone(), owner, founders: design.founders });
        // Lineages from before version 36 count as founded at tick 0.
        self.lineage_founded.resize(lineage as usize, 0);
        self.lineage_founded.push(self.tick);
        self.rebuild_grace_starts();
        // What the home search reads of the world, gathered once: founding
        // only adds this lineage's organisms, which it ignores except as a
        // crowd.
        let mut search = None;
        // Founders land on dry ground (`Balance::dry_sites`): playtest 2's
        // founders drowned on landing at coastal homes.
        let dry_sites = self.config.balance.dry_sites > 0.0;
        // Its own stream for the extra home picks and the placements, so the
        // founders' genomes are drawn as without the rule.
        let mut site_rng = rng_for(self.config.seed, stream::FOUNDING, owner as u64 | 1 << 40, self.next_id);
        for k in 0..design.founders {
            if k % group_size == 0 {
                let s = search.get_or_insert_with(|| HomeSearch::new(self, &probe, lineage));
                home = self.pick_home(&probe, s, &mut rng);
                if dry_sites {
                    // A home with room on dry land for the founding: the
                    // first of a few picks with enough of it, else the
                    // driest.
                    let mut best = (self.dry_share(home, spread), home);
                    for _ in 0..DRY_HOME_TRIES {
                        if best.0 >= DRY_HOME_SHARE {
                            break;
                        }
                        let s = search.as_mut().expect("search");
                        let h = self.pick_home(&probe, s, &mut site_rng);
                        let d = self.dry_share(h, spread);
                        if d > best.0 {
                            best = (d, h);
                        }
                    }
                    home = best.1;
                }
            }
            let genome = design.founder_genome(owner, &mut rng);
            let sex = if k % 2 == 0 { Sex::Female } else { Sex::Male };
            let mut pos = home;
            let tries = if dry_sites { 40 } else { 20 };
            for _ in 0..tries {
                let r = if dry_sites { &mut site_rng } else { &mut rng };
                let off = Vec2::new(r.gen_range(-spread..spread), r.gen_range(-spread..spread));
                let cand = self.map.clamp(home + off);
                // Passable (`passable_for`), from the search's region map.
                let (cx, cy) = self.map.tile_coords(cand);
                if search.as_ref().is_some_and(|s| s.region[self.map.idx(cx, cy)] != u32::MAX)
                    && (!dry_sites || self.dry_ground(cand))
                {
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
            if let Some(s) = search.as_mut() {
                s.crowd.insert(o.pos, o.traits.carnivory);
            }
            ids.push(self.insert(o));
        }
        ids
    }

    /// Most founders a player's lineage places at one home with
    /// `Balance::founding_food` on; a bigger founding is split into groups
    /// of at most this many, each at its own home. Above every preset
    /// design's founding (46), so the standard worlds found as before.
    pub const FOUNDING_GROUP: u32 = 48;

    /// Dry ground for a founder or a home (`Balance::dry_sites`): no water
    /// or ice on the tile, and at least [`DRY_WATER_DIST`] steps from water,
    /// so a flood or the melt doesn't reach it at once.
    fn dry_ground(&self, p: Vec2) -> bool {
        let (x, y) = self.map.tile_coords(p);
        let i = self.map.idx(x, y);
        let t = &self.map.tiles[i];
        t.water_depth == 0.0 && t.ice == 0.0 && self.map.water_dist[i] >= DRY_WATER_DIST
    }

    /// Share of the tiles within `r` of `p` that are dry ground.
    fn dry_share(&self, p: Vec2, r: f32) -> f32 {
        let (cx, cy) = self.map.tile_coords(p);
        let r = r.ceil() as usize;
        let (mut dry, mut all) = (0u32, 0u32);
        for y in cy.saturating_sub(r)..(cy + r + 1).min(self.map.height) {
            for x in cx.saturating_sub(r)..(cx + r + 1).min(self.map.width) {
                all += 1;
                dry += u32::from(self.dry_ground(Map::tile_center(x, y)));
            }
        }
        dry as f32 / all.max(1) as f32
    }

    /// Share of the vegetation still standing within 4 tiles of `p` (1 where
    /// the ground grows nothing), against what it held when the world began
    /// (`terrain`'s 0.8 of the most it could grow), capped at 1.
    fn standing_share(&self, p: Vec2) -> f32 {
        let (cx, cy) = self.map.tile_coords(p);
        let (mut now, mut max) = (0.0f32, 0.0f32);
        let ground = self.map.ground();
        for y in cy.saturating_sub(4)..(cy + 5).min(self.map.height) {
            for x in cx.saturating_sub(4)..(cx + 5).min(self.map.width) {
                let i = self.map.idx(x, y);
                now += self.map.tiles[i].veg;
                max += 0.8 * ground[i].veg_max;
            }
        }
        if max <= 0.0 {
            1.0
        } else {
            (now / max).min(1.0)
        }
    }

    /// A start location the design can live on: passable, comfortable, edible,
    /// near water it can walk to without passing its predators, away from
    /// organisms already placed, and out of reach of anything that could eat
    /// it. A hunter's prey only counts if it can reach it, and prey so close
    /// that the hunters would start on top of it counts against the spot.
    /// Every sum and count is the same as a scan of all organisms: the grids
    /// in `search` only skip those too far away to add anything, and sums
    /// add the rest in organism order.
    fn pick_home(&self, probe: &Organism, search: &HomeSearch, rng: &mut impl Rng) -> Vec2 {
        let buffer = self.config.balance.home_buffer.max(0.0);
        let HomeSearch { region, threats, threat_grid, prey, prey_grid, crowd } = search;
        let region_at = |p: Vec2| {
            let (x, y) = self.map.tile_coords(p);
            region[self.map.idx(x, y)]
        };
        let carn = probe.traits.carnivory;
        let tax = self.config.balance.generalist_tax;
        let (plant_eff, meat_eff) =
            (probe.traits.plant_efficiency(&self.config.balance), probe.traits.meat_efficiency(tax));
        // How far founders scatter from home (`found_lineage_unchecked`),
        // plus a margin.
        let scatter = 3.0 + 10.0 * (1.0 - probe.pheno[crate::Gene::GroupTolerance]) + 8.0;
        let reach = buffer.max(1e-3).max(scatter);
        // Prey counted per lineage (index), summed in lineage order below.
        let mut per_lineage: Vec<f32> = vec![0.0; self.lineages.len()];
        let mut near: Vec<u32> = Vec::new();
        let mut best = (f32::MIN, Map::tile_center(self.map.width / 2, self.map.height / 2));
        for _ in 0..200 {
            let p = Vec2::new(rng.gen_range(0.0..self.map.width as f32), rng.gen_range(0.0..self.map.height as f32));
            // Passable (`passable_for`): the region map marks every tile.
            if region_at(p) == u32::MAX {
                continue;
            }
            let t = self.map.ground_at(p);
            let comfort = if probe.traits.warm {
                -(2.0 - t.base_temp).max(0.0) * 0.1
            } else {
                -(18.0 - t.base_temp).max(0.0) * 0.1
            };
            // A short walk to drinking water that avoids the threats.
            let water = if self.water_reachable(region, p, threat_grid) { 1.0 } else { -2.0 };
            // Competitors share a diet: an omnivore competes with grazers and
            // hunters alike (diet overlap `1 - |carnivory difference|` above
            // 0.45), specialists only with their own kind.
            let mut crowd_n = 0u32;
            crowd.near(p, 12.0, |q, &c| {
                if q.dist(p) < 12.0 && (c - carn).abs() < 0.55 {
                    crowd_n += 1;
                }
            });
            let crowd_n = crowd_n as f32;
            // Threats that could reach the spot, in organism order (farther
            // ones add exactly zero to `danger` and nothing to `close`).
            near.clear();
            threat_grid.near(p, reach, |_, &k| near.push(k));
            near.sort_unstable();
            let terms = || near.iter().map(|&k| threats[k as usize]);
            let danger: f32 = terms().map(|(q, w)| w * (1.0 - q.dist(p) / buffer.max(1e-3)).max(0.0)).sum();
            // A threat within the founders' scatter (plus a margin) would start
            // among them: weigh it heavily, not just as a slice of `danger`.
            let close: f32 = terms().filter(|t| t.0.dist(p) < scatter).map(|t| t.1).sum();
            // Reachable prey nearby. Each prey lineage counts up to a share of
            // its founders and counts against the spot beyond twice that, so
            // hunters don't settle on most of one prey lineage; prey right on
            // the spot counts against it too.
            let home_region = region_at(p);
            per_lineage.iter_mut().for_each(|n| *n = 0.0);
            let mut on_top = 0u32;
            prey_grid.near(p, PREY_RANGE, |_, &k| {
                let (q, l, r) = prey[k as usize];
                let d = q.dist(p);
                if d < PREY_ON_TOP {
                    on_top += 1;
                } else if d < PREY_RANGE && r == home_region {
                    per_lineage[l as usize] += 1.0;
                }
            });
            let on_top = on_top as f32;
            let prey_count: f32 = per_lineage
                .iter()
                .enumerate()
                .filter(|&(_, &n)| n > 0.0)
                .map(|(l, &n)| {
                    let share = (PREY_SHARE * self.lineages[l].founders as f32).max(3.0);
                    n.min(share) - (n - 2.0 * share).max(0.0)
                })
                .sum();
            let productivity = (t.base_temp / 12.0).clamp(0.2, 1.0);
            // Founding by food (`Balance::founding_food`): in a running
            // world, what is still standing around the spot, not what the
            // ground could grow.
            let standing =
                if self.tick > 0 && self.config.balance.founding_food > 0.0 { self.standing_share(p) } else { 1.0 };
            let food = t.veg_max / 40.0 * productivity * plant_eff * standing + meat_eff * 0.04 * prey_count.min(60.0);
            let score = water + food + comfort
                - 0.05 * crowd_n
                - (0.3 * danger).min(4.0)
                - (0.1 * carn * on_top).min(2.0)
                - 2.0 * close
                + rng.gen_range(0.0..0.5);
            if score > best.0 {
                best = (score, p);
            }
        }
        best.1
    }

    /// Whether the organism can walk each tile (`passable_for`, with only
    /// the tests that block): 0 if so, `u32::MAX` if not.
    fn passable_map(&self, o: &Organism) -> Vec<u32> {
        let (ground, body) = (&self.map.terrain().ground, &o.traits.body);
        (0..self.map.width * self.map.height)
            .map(|i| if crate::terrain::passable(body, &ground[i], &self.map.tiles[i]) { 0 } else { u32::MAX })
            .collect()
    }

    /// Connected regions of tiles the organism can walk (8-neighbor), as a
    /// label per tile; `u32::MAX` for impassable tiles. Regions are numbered
    /// in the order of their first tile (row by row), from runs of open
    /// tiles joined to the overlapping runs of the row above.
    fn passable_regions(&self, o: &Organism) -> Vec<u32> {
        let w = self.map.width;
        let open = self.passable_map(o);
        label_runs(&open, w, self.map.height)
    }

    /// A spot where it can drink within `WATER_STEPS` steps (8-neighbor) of
    /// `p`, walking only over passable tiles (those `region` labels,
    /// `passable_regions`) that keep clear of `threats`.
    fn water_reachable(&self, region: &[u32], p: Vec2, threats: &PointGrid<u32>) -> bool {
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
                        let clear = || {
                            let mut clear = true;
                            threats.near(q, CLEARANCE, |t, _| clear &= t.dist(q) >= CLEARANCE);
                            clear
                        };
                        if region[self.map.idx(nx, ny)] != u32::MAX && clear() {
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
        matches!(self.map.traverse_at(&o.traits.body, p, false), Traverse::Passable { .. })
    }

    fn insert(&mut self, mut o: Organism) -> OrgId {
        let id = self.next_id;
        self.next_id += 1;
        o.id = id;
        // Body plan effects, once, at birth (`Balance::morph_*`; no-op at 0).
        crate::morph_effects::apply(&mut o.traits, &o.pheno, &o.morph, &self.config.balance);
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

    /// `kill` by another organism (`killer`): predation, or a hunter killed
    /// by prey that fought back.
    fn kill_by(&mut self, i: usize, killer: usize) {
        if !self.organisms[i].alive {
            return;
        }
        let k = self.organisms[killer].lineage;
        self.organisms[i].killer = Some(k);
        *self.counters.kills_by_lineage.entry((self.organisms[i].lineage, k)).or_default() += 1;
        self.kill(i, DeathCause::Predation);
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
        // The tick's parallel parts run on the rayon pool. Called from
        // outside it (a main thread, a server's runner), each of them would
        // hand its work over to the pool and wait for it, a dozen round
        // trips a tick; entering the pool once for the whole tick is far
        // cheaper. Where a closure runs changes nothing it computes.
        if rayon::current_thread_index().is_none() {
            rayon::scope(|_| self.step_in_pool());
        } else {
            self.step_in_pool();
        }
    }

    fn step_in_pool(&mut self) {
        let tpd = self.config.ticks_per_day as u64;
        let mut clock = std::time::Instant::now();
        let mut lap = || {
            let now = std::time::Instant::now();
            let s = (now - clock).as_secs_f64();
            clock = now;
            s
        };
        // Possession sub-steps the caller didn't take between ticks are
        // taken now, so a world steps the same whether or not anyone calls
        // `substep` (only when orders arrive can differ).
        self.finish_substeps();
        // Lineage crowding from the organisms as the tick starts (the same
        // after a restore, which keeps no cache).
        self.refresh_crowding();
        if self.config.lod.enabled && self.tick.is_multiple_of(self.config.ticks_per_hour()) {
            self.lod_update();
        }
        if self.tick.is_multiple_of(self.config.ticks_per_hour()) {
            self.hourly();
            self.refresh_home_ranges();
        }
        if self.tick.is_multiple_of(tpd) {
            self.environment_slice();
            self.phase_times.environment += lap();
            self.daily();
            self.phase_times.daily += lap();
            self.settle_orders();
            self.grid.rebuild(&self.organisms);
            self.phase_times.grid += lap();
        } else {
            // Off the day, the environment chunks (the map's tiles) and the
            // grid rebuild (the organisms) share nothing, and settling
            // orders touches neither the map nor positions: the two run at
            // once, with the same results as one after the other.
            self.settle_orders();
            let tph = self.config.ticks_per_hour();
            let (phase, doy, hour) = (self.tick % tph, self.day_of_year(), self.hour());
            let (dpy, bal) = (self.config.days_per_year, self.config.balance);
            let (map, grid, orgs) = (&mut self.map, &mut self.grid, &self.organisms);
            let timed = |f: &mut dyn FnMut()| {
                let start = std::time::Instant::now();
                f();
                start.elapsed().as_secs_f64()
            };
            let (env, rebuild) = rayon::join(
                || timed(&mut || map.update_chunks(phase, tph, doy, hour, dpy, &bal)),
                || timed(&mut || grid.rebuild(orgs)),
            );
            lap();
            self.phase_times.environment += env;
            self.phase_times.grid += rebuild;
        }
        let n = self.organisms.len();
        let intents = self.decide_all();
        self.remember_intents(&intents);
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
        self.substep = 0;
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

    /// The flood fill `World::passable_regions` used before run labelling:
    /// the reference [`label_runs`] must match tile for tile.
    fn flood_labels(open: &[u32], w: usize, h: usize) -> Vec<u32> {
        let mut label = vec![u32::MAX; w * h];
        let (mut next, mut stack) = (0, Vec::new());
        for start in 0..w * h {
            if open[start] == u32::MAX || label[start] != u32::MAX {
                continue;
            }
            label[start] = next;
            stack.push(start);
            while let Some(i) = stack.pop() {
                let (x, y) = (i % w, i / w);
                for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        let j = ny * w + nx;
                        if open[j] != u32::MAX && label[j] == u32::MAX {
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

    #[test]
    fn run_labels_match_the_flood_fill() {
        let mut rng = crate::rng::rng_for(7, 1, 2, 3);
        for (w, h) in [(1, 1), (1, 9), (9, 1), (7, 5), (33, 17), (64, 64)] {
            for density in [0.1, 0.4, 0.55, 0.7, 0.95, 1.0] {
                let open: Vec<u32> =
                    (0..w * h).map(|_| if rng.gen::<f64>() < density { 0 } else { u32::MAX }).collect();
                assert_eq!(label_runs(&open, w, h), flood_labels(&open, w, h), "{w}x{h} at {density}");
            }
        }
        // Real maps, for every founding body.
        for seed in 1..4 {
            let mut w = small_world(seed);
            w.run_days(1);
            for o in w.organisms.iter().step_by(7) {
                let open = w.passable_map(o);
                let (mw, mh) = (w.map.width, w.map.height);
                assert_eq!(w.passable_regions(o), flood_labels(&open, mw, mh));
            }
        }
    }

    /// `terrain::passable` (founding's whole-map passability) agrees with
    /// `traverse` on every tile, for bodies of every lineage, as the tiles
    /// change through the seasons (water, ice, snow), and for bodies made
    /// to sit on each threshold.
    #[test]
    fn passable_agrees_with_traverse() {
        let mut w = small_world(11);
        let mut bodies: Vec<crate::stats::Body> = w.organisms.iter().map(|o| o.traits.body).collect();
        let base = bodies[0];
        for v in [0.0, 0.1, 0.25, 0.39, 0.4, 0.41, 0.5, 0.75, 1.0] {
            bodies.push(crate::stats::Body { swim: v, climb: v, height: v, mass: 40.0 * v, ..base });
        }
        for day in 0..4 {
            if day > 0 {
                w.run_days(15);
            }
            let ground = &w.map.terrain().ground;
            for b in &bodies {
                for (i, (g, t)) in ground.iter().zip(&w.map.tiles).enumerate() {
                    let weather = crate::terrain::Weather { wind: t.wind, dir: 0.0 };
                    let walk = matches!(crate::terrain::traverse(b, g, t, &weather, false), Traverse::Passable { .. });
                    assert_eq!(crate::terrain::passable(b, g, t), walk, "tile {i}, day {day}, {b:?}");
                }
            }
        }
    }

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

    /// The daily legacy snapshots are a window, not a history: a long-running
    /// world must not grow by a map of every player's share each day.
    #[test]
    fn keeps_only_recent_legacy_snapshots() {
        let mut w = small_world(5);
        let days = crate::legacy::LEGACY_KEPT as u32 + 3;
        w.run_days(days);
        assert_eq!(w.legacy.len(), crate::legacy::LEGACY_KEPT);
        let today = w.day();
        let kept: Vec<u64> = w.legacy.iter().map(|l| l.day).collect();
        // Each snapshot is of the day that just ended.
        let want: Vec<u64> = (today - crate::legacy::LEGACY_KEPT as u64..today).collect();
        assert_eq!(kept, want, "the most recent days, oldest first");
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

#[cfg(test)]
mod winter_tests {
    use super::*;

    #[test]
    fn days_until_winter_counts_down_to_the_first_winter_day() {
        let mut w =
            World::new(WorldConfig { seed: 1, width: 32, height: 32, days_per_year: 120, ..Default::default() });
        let tpd = w.config.ticks_per_day as u64;
        let at = |w: &mut World, day: u64| {
            w.tick = day * tpd;
            (w.days_until_winter(), w.season())
        };
        assert_eq!(at(&mut w, 0), (90, Season::Spring));
        assert_eq!(at(&mut w, 89), (1, Season::Autumn));
        assert_eq!(at(&mut w, 90), (0, Season::Winter));
        assert_eq!(at(&mut w, 119), (0, Season::Winter));
        assert_eq!(at(&mut w, 120), (90, Season::Spring));
        // A year whose length is not a multiple of four: winter starts on the first day `Season::from_day` names.
        w.config.days_per_year = 10;
        let first = (0..10).find(|&d| Season::from_day(d, 10) == Season::Winter).unwrap() as u64;
        assert_eq!(at(&mut w, first - 1).0, 1);
        assert_eq!(at(&mut w, first).0, 0);
    }
}
