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
pub use environment::CLUTCH_COVERED;
pub use lod::{Cohort, LodStats, PoolEntry};
#[cfg(feature = "serde")]
pub use snapshot::{SnapshotError, SnapshotHeader, SNAPSHOT_VERSION, TERRAIN_VERSION};
use spatial::SpatialGrid;

/// Founding: longest walk (tiles) from a home to a spot where it can drink
/// (water within 6 tiles, as drinking reaches the next tile).
const WATER_STEPS: usize = 5;
/// `Ground::base_water` from which a lake's water lasts a drought
/// (`World::water_access`): the shallows below it dry out in droughts.
const LASTING_WATER: f32 = 1.0;
/// Drought (share of land) from which a founding home must have water in
/// reach that lasts a drought (`Balance::founding_drought`).
pub const FOUNDING_DROUGHT: f32 = 0.3;
/// Founders' ages: maturity plus up to this share of their lifespan, or
/// [`FOUNDER_AGE_YOUNG`] for a player's founding in a running world with
/// `Balance::founder_youth`.
const FOUNDER_AGE_SPREAD: f32 = 0.3;
const FOUNDER_AGE_YOUNG: f32 = 0.1;
/// Radius (tiles) within which other players' animals count against a
/// founding home (`Balance::founding_rivals`).
const RIVAL_RANGE: f32 = 35.0;
/// Most a crowd of rivals marks a home down, and the distance-weighted
/// count of rival animals that gives about 63% of it: big enough that a
/// herd of a thousand counts for more than one of a hundred (a scale of 15
/// saturated near any herd, so it told crowds apart only at the edges).
const RIVAL_WEIGHT: f32 = 4.0;
const RIVAL_SCALE: f32 = 200.0;
/// How much a home at another player's territory edge is marked down,
/// falling to nothing `RIVAL_RANGE` tiles out.
const RIVAL_LAND_WEIGHT: f32 = 1.5;
/// No founding home within this many tiles outside another player's
/// territory (`Balance::founding_rivals`).
const RIVAL_TERRITORY_MARGIN: f32 = 4.0;
/// How much more a predator (carnivory above 0.6) counts against a founding
/// home than another rival animal, and how much farther (`Balance::rival_reach`).
const RIVAL_PACK_WEIGHT: f32 = 3.0;
const RIVAL_PACK_RANGE: f32 = 2.0;
/// Tiles a rival group's foraging reaches beyond its home circle, counted
/// as its ground (`Balance::rival_reach`).
const RIVAL_FORAGE: f32 = 8.0;
/// Days a new founding's founders lose water more slowly in a drought
/// (`Balance::founding_drought`).
pub const FOUNDER_WATER_DAYS: f32 = 6.0;
/// Within this many tiles of a map edge, a cold-season founding home is
/// marked down (`Balance::founding_conditions`).
const COLD_EDGE: f32 = 12.0;
/// How much a cold-season home right at the edge is marked down: enough to
/// outweigh the crowd and threats that otherwise push foundings into the
/// corners.
const COLD_EDGE_WEIGHT: f32 = 3.0;
/// Radius around a home where the owner's last founding died that the next
/// founding avoids (`Balance::founding_conditions`).
const AVOID_RADIUS: f32 = 20.0;
/// Plants (`Tile::veg`, summed within 4 tiles) a founding home should hold
/// per founder to count as fully fed (`Balance::founding_conditions`).
const FOOD_STOCK: f32 = 20.0;
/// Least share of the ground within the founders' scatter that must be dry
/// land for a home (`Balance::founding_safety`).
const SETTLE_SHARE: f32 = 0.5;
/// Within this many tiles of a map edge a founding home is marked down in
/// any season (`Balance::founding_safety`; the cold season's `COLD_EDGE`
/// comes on top).
const EDGE_MARGIN: f32 = 12.0;
/// No founding home within this many tiles of a map edge
/// (`Balance::founding_safety`; only the fallback round may go there).
const EDGE_KEEP: f32 = 6.0;
/// Within this many tiles of a corner (both edges) a home is marked down
/// further (`Balance::founding_safety`).
const CORNER_MARGIN: f32 = 16.0;
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
    /// Homes where the owner's earlier foundings died out
    /// (`Balance::founding_conditions`): avoided.
    avoid: Vec<Vec2>,
    /// Founders placed at each home (the group size).
    founders: u32,
    /// Tiles founders may settle on (`Balance::founding_safety`, in a
    /// running world): walkable dry land with no lake bed or river channel
    /// under it, away from water. `None`: the region map decides.
    settle: Option<Vec<bool>>,
    /// Other players' living animals (`Balance::founding_rivals`, in a
    /// running world; empty otherwise), each with its weight and range
    /// (`Balance::rival_reach`: predators more, and farther).
    rivals: PointGrid<(f32, f32)>,
    /// The farthest range in `rivals`.
    rival_range: f32,
    /// Where rival groups range and are headed (`Balance::rival_reach`):
    /// kept out of first, then only marked down, so water that lasts a
    /// drought still wins on a crowded map.
    rival_ranges: Vec<groups::Territory>,
    /// Other players' territories (same rule).
    rival_lands: Vec<groups::Territory>,
}

impl HomeSearch {
    fn new(w: &World, probe: &Organism, lineage: u32, founders: u32) -> Self {
        let hunter = probe.traits.carnivory > 0.45;
        // Regions only tell a hunter's prey apart (reachable from home or
        // not); for anyone else every passable tile reads 0.
        // In a running world (`Balance::founding_conditions`), ice over deep
        // water is no home: founders scattered onto a frozen lake drown at
        // the thaw (playtest 3).
        let no_ice = w.config.balance.founding_conditions > 0.0 && w.tick > 0;
        let region = if hunter { w.passable_regions(probe, no_ice) } else { w.passable_map(probe, no_ice) };
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
        // Where this owner's earlier foundings died out (not retired: at
        // least half their founders died), so a re-founding doesn't land on
        // the same fatal spot (playtest 3: one player, the same peninsula,
        // three times).
        let avoid = if w.config.balance.founding_conditions > 0.0 {
            (0..lineage as usize)
                .filter(|&l| w.lineages[l].owner == probe.owner && w.lineage_alive.get(l).copied().unwrap_or(0) == 0)
                .filter(|&l| {
                    let died: u64 =
                        w.counters.deaths_by_lineage.iter().filter(|((k, _), _)| *k == l as u32).map(|(_, n)| n).sum();
                    2 * died >= w.lineages[l].founders as u64
                })
                .filter_map(|l| w.lineage_homes.get(l).copied().flatten())
                .collect()
        } else {
            Vec::new()
        };
        let settle = (w.config.balance.founding_safety > 0.0 && w.tick > 0).then(|| {
            let ground = w.map.ground();
            (0..w.map.width * w.map.height)
                .map(|i| {
                    let (t, g) = (&w.map.tiles[i], &ground[i]);
                    region[i] != u32::MAX
                        && t.water_depth == 0.0
                        && t.ice == 0.0
                        && g.base_water <= 0.0
                        && g.river <= 0.0
                        && w.map.water_dist[i] >= DRY_WATER_DIST
                })
                .collect()
        });
        // Other players' animals and territories (`Balance::founding_rivals`):
        // playtest 7's late joiner landed inside a rival's territory with
        // four rivals within 35 tiles. Only for a player's founding in a
        // running world, so a new world's founding is unchanged.
        let mut rivals = PointGrid::new(&w.map);
        let mut rival_lands = Vec::new();
        let mut rival_ranges = Vec::new();
        let reach = w.config.balance.rival_reach > 0.0;
        let mut rival_range = RIVAL_RANGE;
        if w.config.balance.founding_rivals > 0.0 && w.tick > 0 && probe.owner != crate::WILD {
            let mut owners = std::collections::BTreeSet::new();
            for o in &w.organisms {
                if o.alive && o.owner != crate::WILD && o.owner != probe.owner {
                    // Predators count more, and farther (`Balance::rival_reach`).
                    let pack = reach && o.traits.carnivory > 0.6;
                    let (weight, range) =
                        if pack { (RIVAL_PACK_WEIGHT, RIVAL_RANGE * RIVAL_PACK_RANGE) } else { (1.0, RIVAL_RANGE) };
                    rival_range = rival_range.max(range);
                    rivals.insert(o.pos, (weight, range));
                    owners.insert(o.owner);
                }
            }
            rival_lands = w.territories.iter().filter(|(&(p, _), _)| p != probe.owner).map(|(_, &t)| t).collect();
            // Where each rival group ranges and is headed counts as its
            // ground (`Balance::rival_reach`; playtest 8: late joiners landed
            // inside a third group's territory 8 days later).
            if reach {
                for &p in &owners {
                    for g in w.groups(p) {
                        if g.members == 0 {
                            continue;
                        }
                        let home = World::home_radius(g.members);
                        let circle = |center: Vec2, radius: f32| groups::Territory { center, radius, claimed_tick: 0 };
                        if let Some(mid) = w.group_middle(p, g.id) {
                            rival_ranges.push(circle(mid, home + RIVAL_FORAGE));
                        }
                        if let Some(&target) = w.migrate_orders.get(&(p, g.id)) {
                            rival_ranges.push(circle(target, home + RIVAL_FORAGE));
                        }
                        if let Some(r) = g.rally {
                            rival_ranges.push(circle(r.center, r.radius.max(home)));
                        }
                    }
                }
            }
        }
        HomeSearch {
            region,
            threats,
            threat_grid,
            prey,
            prey_grid,
            crowd,
            avoid,
            founders,
            settle,
            rivals,
            rival_range,
            rival_ranges,
            rival_lands,
        }
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
    /// The shortest span after a founding within which the owner's next
    /// founding continues its grace (`World::set_grace_carry_min_ticks`:
    /// the server's re-founding window); not saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    grace_carry_min_ticks: u64,
    /// Each lineage's grace start (`World::grace_start`), derived from
    /// `lineage_founded`; rebuilt on founding and restore, not saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    grace_starts: Vec<u64>,
    /// The cooldown `grace_starts` was computed with; a change of
    /// `Balance::attack_grace` falls back to computing afresh.
    #[cfg_attr(feature = "serde", serde(skip))]
    grace_starts_cooldown: u64,
    /// When each player ended their newcomer grace (`World::end_grace`);
    /// saved by the host, not in the snapshot.
    #[cfg_attr(feature = "serde", serde(skip))]
    grace_waived: std::collections::BTreeMap<PlayerId, u64>,
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
    rendezvous: BTreeMap<OrgId, (OrgId, Vec2)>,
    /// Organisms barred from natural mating with another player's animals
    /// until a tick (`World::bar_cross_mating`): set by the server while one
    /// is possessed and a while after, so a player can't walk a male into a
    /// rival's herd to sire its young without consent or a fee (playtest 8's
    /// "stud raid"). Not saved, like rendezvous.
    #[cfg_attr(feature = "serde", serde(skip))]
    cross_mate_bar: BTreeMap<OrgId, u64>,
    /// Attack orders of possessed organisms, attacker → target
    /// (`World::possess_attack`; version 36).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v36::list"))]
    attack_orders: BTreeMap<OrgId, OrgId>,
    /// The tick each lineage was founded, indexed like `lineages` (for
    /// `Balance::attack_grace`; version 36: empty from older snapshots,
    /// whose lineages count as founded at tick 0).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v36::list"))]
    lineage_founded: Vec<u64>,
    /// Each lineage's first home, indexed like `lineages` (`None` before
    /// version 47 and for none recorded), so a player's re-founding avoids
    /// the spot where their last founding died (`Balance::founding_conditions`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v47::list"))]
    lineage_homes: Vec<Option<Vec2>>,
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
    /// Each player's room for the pack larder as the tick starts (0–1,
    /// `apply::player_pack_larder`, `World::refresh_pack_room`): the same in
    /// the sequential and parallel apply, whatever dies mid-tick.
    /// Recomputed before every tick, so not saved.
    #[cfg_attr(feature = "serde", serde(skip))]
    pack_room: BTreeMap<PlayerId, f32>,
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
            cross_mate_bar: BTreeMap::new(),
            lineage_founded: Vec::new(),
            lineage_homes: Vec::new(),
            group_plans: groups::GroupPlans::default(),
            attack_events: Vec::new(),
            toxic_lessons: BTreeMap::new(),
            crowding: Vec::new(),
            pack_room: BTreeMap::new(),
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
            grace_carry_min_ticks: 0,
            grace_starts: Vec::new(),
            grace_starts_cooldown: 0,
            grace_waived: Default::default(),
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
    /// fat stores gene). Egg-layers also wait for spring to breed: a winter
    /// clutch laid by founders who must leave it to feed chills within half
    /// a day (playtest 3: every founding clutch of a winter founding was
    /// lost). Deterministic: no randomness.
    pub fn provision_for_winter(&mut self, ids: &[OrgId]) {
        let tpd = self.config.ticks_per_day as u64;
        let spring = (self.day() - self.day_of_year() as u64 + self.config.days_per_year.max(1) as u64) * tpd;
        for &id in ids {
            let Some(&i) = self.index.get(&id) else { continue };
            let cap = self.fat_capacity(&self.organisms[i]);
            let o = &mut self.organisms[i];
            if o.alive {
                o.needs.hunger = 0.0;
                o.fat = o.fat.max(cap);
                if o.traits.birth != crate::design::BirthType::Live {
                    o.breed_ready_tick = o.breed_ready_tick.max(spring);
                }
            }
        }
    }

    /// Factor on a player's founder's thirst for the first
    /// [`FOUNDER_WATER_DAYS`] after its lineage's founding
    /// (`Balance::founding_drought`): from 1 at half [`FOUNDING_DROUGHT`]
    /// down to a half from 1.5 × it. 1 for everyone else.
    pub(crate) fn founder_water_relief(&self, o: &Organism) -> f32 {
        if self.config.balance.founding_drought <= 0.0 || o.generation != 0 || o.owner == crate::WILD {
            return 1.0;
        }
        let founded = self.lineage_founded.get(o.lineage as usize).copied().unwrap_or(0);
        let tpd = self.config.ticks_per_day as f32;
        if self.tick.saturating_sub(founded) as f32 >= FOUNDER_WATER_DAYS * tpd {
            return 1.0;
        }
        self.founding_water_relief().unwrap_or(1.0)
    }

    /// The factor on a new founding's founders' thirst for their first
    /// [`FOUNDER_WATER_DAYS`] if one were founded now
    /// ([`World::founder_water_relief`]), when it is below 1: the drought
    /// provision applies. For the server's founding reply.
    pub fn founding_water_relief(&self) -> Option<f32> {
        if self.config.balance.founding_drought <= 0.0 {
            return None;
        }
        let dry = ((self.map.drought - 0.5 * FOUNDING_DROUGHT) / FOUNDING_DROUGHT).clamp(0.0, 1.0);
        (dry > 0.0).then_some(1.0 - 0.5 * dry)
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

    /// `owner`'s room for the pack larder as this tick started (0–1).
    pub(crate) fn pack_room(&self, owner: PlayerId) -> f32 {
        self.pack_room.get(&owner).copied().unwrap_or(0.0)
    }

    /// Each player's room for the pack larder (`apply::player_pack_larder`):
    /// with `Balance::prey_larder`, by the prey there is per hunter (plant
    /// eaters not the player's, per pack hunter of the player's: full from
    /// `apply::PREY_FULL`, none at `apply::PREY_NONE`); without it, by the
    /// band's size (full up to `apply::BAND_SMALL` animals, none from
    /// `apply::BAND_CROWD`).
    fn refresh_pack_room(&mut self) {
        self.pack_room.clear();
        let bal = &self.config.balance;
        if bal.player_packs <= 0.0 {
            return;
        }
        if bal.prey_larder <= 0.0 {
            for (&p, &n) in &self.owner_counts {
                let room = (apply::BAND_CROWD - n as f32) / (apply::BAND_CROWD - apply::BAND_SMALL);
                self.pack_room.insert(p, room.clamp(0.0, 1.0));
            }
            self.first_winter_room();
            return;
        }
        let (mut grazers, mut grazers_of, mut hunters) =
            (0usize, BTreeMap::<PlayerId, usize>::new(), BTreeMap::<PlayerId, usize>::new());
        // A lineage under `prey_floor` living plant eaters isn't counted as
        // prey: a small population doesn't feed a bigger pack that would
        // then hunt it out.
        let mut per_lineage: BTreeMap<u32, usize> = BTreeMap::new();
        if bal.prey_floor > 0.0 {
            for o in self.organisms.iter().filter(|o| o.alive && o.traits.carnivory <= decide::HUNTER_CARNIVORY) {
                *per_lineage.entry(o.lineage).or_default() += 1;
            }
        }
        let counted =
            |l: u32| bal.prey_floor <= 0.0 || per_lineage.get(&l).copied().unwrap_or(0) as f32 >= bal.prey_floor;
        for o in self.organisms.iter().filter(|o| o.alive) {
            if o.traits.carnivory <= decide::HUNTER_CARNIVORY {
                if !counted(o.lineage) {
                    continue;
                }
                grazers += 1;
                *grazers_of.entry(o.owner).or_default() += 1;
            } else if decide::player_pack(bal, o) {
                *hunters.entry(o.owner).or_default() += 1;
            }
        }
        let (full, none) = if bal.pack_prey > 0.0 {
            (bal.pack_prey, bal.pack_prey * 0.5)
        } else {
            (apply::PREY_FULL, apply::PREY_NONE)
        };
        for (p, h) in hunters {
            let prey = grazers - grazers_of.get(&p).copied().unwrap_or(0);
            let per = prey as f32 / h.max(1) as f32;
            self.pack_room.insert(p, ((per - none) / (full - none)).clamp(0.0, 1.0));
        }
        self.first_winter_room();
    }

    /// A founding-sized pack's first winter (`Balance::winter_larder`): a
    /// player with up to `apply::BAND_SMALL` animals whose lineage was
    /// founded within the year has the full larder room in winter, whatever
    /// the prey. A strong pack that outgrew that by its first winter gets
    /// nothing extra (it overshot: ApexHunter2's wolves took 36% of six
    /// players' animals with the rule at any size).
    fn first_winter_room(&mut self) {
        let bal = &self.config.balance;
        if bal.winter_larder <= 0.0 || self.season() != Season::Winter {
            return;
        }
        let year = self.config.days_per_year.max(1) as u64 * self.config.ticks_per_day as u64;
        let mut young: BTreeMap<PlayerId, bool> = BTreeMap::new();
        for (l, lin) in self.lineages.iter().enumerate() {
            if lin.owner == crate::WILD {
                continue;
            }
            let founded = self.lineage_founded.get(l).copied().unwrap_or(0);
            *young.entry(lin.owner).or_default() |= self.tick.saturating_sub(founded) < year;
        }
        for (p, first) in young {
            if first && (self.owner_count(p) as f32) <= apply::BAND_SMALL {
                self.pack_room.insert(p, 1.0);
            }
        }
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
        self.lineage_homes.resize(lineage as usize + 1, None);
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
                let s = search
                    .get_or_insert_with(|| HomeSearch::new(self, &probe, lineage, group_size.min(design.founders)));
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
                if k == 0 {
                    self.lineage_homes[lineage as usize] = Some(home);
                }
            }
            let genome = design.founder_genome(owner, &mut rng);
            let sex = if k % 2 == 0 { Sex::Female } else { Sex::Male };
            let mut pos = home;
            let settle = search.as_ref().and_then(|s| s.settle.as_ref());
            let tries = if settle.is_some() {
                60
            } else if dry_sites {
                40
            } else {
                20
            };
            for _ in 0..tries {
                let r = if dry_sites || settle.is_some() { &mut site_rng } else { &mut rng };
                let off = Vec2::new(r.gen_range(-spread..spread), r.gen_range(-spread..spread));
                let cand = self.map.clamp(home + off);
                // Passable (`passable_for`), from the search's region map;
                // with `founding_safety`, settled dry land only.
                let (cx, cy) = self.map.tile_coords(cand);
                let i = self.map.idx(cx, cy);
                let ok = match settle {
                    Some(settle) => settle[i],
                    None => {
                        search.as_ref().is_some_and(|s| s.region[i] != u32::MAX)
                            && (!dry_sites || self.dry_ground(cand))
                    }
                } && search.as_ref().is_none_or(|s| s.rival_lands.iter().all(|t| !t.contains(cand)));
                if ok {
                    pos = cand;
                    break;
                }
            }
            let mut o = Organism::new(self.next_id, owner, genome, sex, pos);
            o.lineage = lineage;
            // Founders start as adults of mixed ages.
            o.growth = 1.0;
            o.matured = true;
            // Young adults for a player's founding in a running world
            // (`Balance::founder_youth`; playtest 8: founders died of old age
            // 69 days after landing). The same draw either way.
            let spread = if self.config.balance.founder_youth > 0.0 && owner != crate::WILD && self.tick > 0 {
                FOUNDER_AGE_YOUNG
            } else {
                FOUNDER_AGE_SPREAD
            };
            o.age_ticks = ((o.traits.maturity_days + rng.gen_range(0.0..o.traits.lifespan_days * spread))
                * self.config.ticks_per_day as f32) as u32;
            o.needs.breeding = rng.gen_range(0.0..40.0);
            // Players' founders start watered (`Balance::founding_drought`).
            if owner != crate::WILD && self.config.balance.founding_drought > 0.0 {
                o.needs.thirst = 0.0;
            }
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

    /// Share of the tiles within `r` of `p` that founders may settle on.
    fn settle_share(&self, settle: &[bool], p: Vec2, r: f32) -> f32 {
        let (cx, cy) = self.map.tile_coords(p);
        let r = r.max(1.0).ceil() as usize;
        let (mut ok, mut all) = (0u32, 0u32);
        for y in cy.saturating_sub(r)..(cy + r + 1).min(self.map.height) {
            for x in cx.saturating_sub(r)..(cx + r + 1).min(self.map.width) {
                all += 1;
                ok += u32::from(settle[self.map.idx(x, y)]);
            }
        }
        ok as f32 / all.max(1) as f32
    }

    /// Plants `founders` like `probe` eat in their first 20 days, in units
    /// of `Tile::veg` (`Balance::hunger_rate` × metabolism a day, converted
    /// as `eat_plants` converts it); 0 for a body that eats no leaves.
    fn founding_need(&self, probe: &Organism, founders: u32) -> f32 {
        let bal = &self.config.balance;
        let eff = probe.traits.plant_efficiency(bal);
        if eff <= 0.0 {
            return 0.0;
        }
        founders as f32 * 20.0 * bal.hunger_rate * probe.traits.stats.metabolism * probe.food_per_hunger() / eff
    }

    /// Plants within `r` tiles of `p` that `probe`'s body can reach.
    fn reachable_veg_near(&self, probe: &Organism, p: Vec2, r: usize) -> f32 {
        let (cx, cy) = self.map.tile_coords(p);
        let (body, share) = (&probe.traits.body, self.config.balance.canopy_share);
        let ground = self.map.ground();
        let mut sum = 0.0f32;
        for y in cy.saturating_sub(r)..(cy + r + 1).min(self.map.height) {
            for x in cx.saturating_sub(r)..(cx + r + 1).min(self.map.width) {
                let i = self.map.idx(x, y);
                sum += self.map.tiles[i].reachable_veg(&ground[i], body, share);
            }
        }
        sum
    }

    /// Plants standing within 4 tiles of `p` now.
    fn veg_near(&self, p: Vec2) -> f32 {
        let (cx, cy) = self.map.tile_coords(p);
        let mut sum = 0.0f32;
        for y in cy.saturating_sub(4)..(cy + 5).min(self.map.height) {
            for x in cx.saturating_sub(4)..(cx + 5).min(self.map.width) {
                sum += self.map.tiles[self.map.idx(x, y)].veg;
            }
        }
        sum
    }

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
        let HomeSearch {
            region,
            threats,
            threat_grid,
            prey,
            prey_grid,
            crowd,
            avoid,
            founders,
            settle,
            rivals,
            rival_range,
            rival_ranges,
            rival_lands,
        } = search;
        // Judge the spot as it is now (`Balance::founding_conditions`): only
        // in a running world, so a new world's founding is unchanged.
        let now = self.config.balance.founding_conditions > 0.0 && self.tick > 0;
        let cold_season = matches!(self.season(), crate::terrain::Season::Winter) || self.days_until_winter() <= 10;
        let dry_founding = now && self.config.balance.founding_drought > 0.0 && self.map.drought >= FOUNDING_DROUGHT;

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
        // With `founding_safety`, 200 picks on dry land; if none has room,
        // 200 more under the old rules (better somewhere than nowhere).
        let rounds = if settle.is_some() { 2 } else { 1 };
        for k in 0..200 * rounds {
            if k == 200 && best.0 > f32::MIN {
                break;
            }
            let strict = k < 200;
            let settle = settle.as_ref().filter(|_| strict);
            let p = Vec2::new(rng.gen_range(0.0..self.map.width as f32), rng.gen_range(0.0..self.map.height as f32));
            // Passable (`passable_for`): the region map marks every tile.
            if region_at(p) == u32::MAX {
                continue;
            }
            // Never inside (or at the edge of) another player's territory
            // (`Balance::founding_rivals`), in any round.
            if rival_lands.iter().any(|t| t.center.dist(p) < t.radius + RIVAL_TERRITORY_MARGIN) {
                continue;
            }
            // Nor, in the first half of the strict pass, where rival groups
            // range and are headed (`Balance::rival_reach`).
            if k < 100 && rival_ranges.iter().any(|t| t.center.dist(p) < t.radius + RIVAL_TERRITORY_MARGIN) {
                continue;
            }
            // With `founding_safety`: dry land, with room for the founders.
            if let Some(settle) = settle {
                let (x, y) = self.map.tile_coords(p);
                let edge = p.x.min(p.y).min(self.map.width as f32 - p.x).min(self.map.height as f32 - p.y);
                if edge < EDGE_KEEP
                    || !settle[self.map.idx(x, y)]
                    || self.settle_share(settle, p, scatter - 8.0) < SETTLE_SHARE
                {
                    continue;
                }
            }
            let t = self.map.ground_at(p);
            // The year's mean temperature (today's would draw a winter
            // founding into a warm desert with nothing to eat).
            let mut comfort = if probe.traits.warm {
                -(2.0 - t.base_temp).max(0.0) * 0.1
            } else {
                -(18.0 - t.base_temp).max(0.0) * 0.1
            };
            // A short walk to drinking water that avoids the threats; with
            // `now`, water that lasts a drought (a lake's deep water) is
            // worth more than shallows that may dry up within days.
            let access = now.then(|| self.water_access(region, p, threat_grid));
            // In a drought (`Balance::founding_drought`), the strict rounds
            // take only homes with water in reach that lasts it: playtest 6's
            // founders at 52% drought had shallows that dried, and all died.
            if settle.is_some() && dry_founding && !access.is_some_and(|a| a.1) {
                continue;
            }
            let water = if let Some(access) = access {
                match access {
                    (_, true) => 1.0,
                    (true, false) => 0.6 - 0.6 * self.map.drought.clamp(0.0, 1.0),
                    _ => -2.0,
                }
            } else if self.water_reachable(region, p, threat_grid) {
                1.0
            } else {
                -2.0
            };
            if now && probe.traits.warm {
                // Summer heat the founders' coat can't shed (the upper end
                // of their comfort, as `needs` reckons it): a furred design
                // founded in a hot land dies of exposure in its first summer.
                let insulation = probe.traits.insulation(self.config.balance.scale_cover);
                let high = 30.0 - 8.0 * insulation + probe.traits.morph.heat_tolerance;
                comfort -= 0.15 * (t.base_temp + crate::terrain::SEASONAL_AMPLITUDE - high).max(0.0);
            }
            if now {
                // Snow to wade through and sleep in.
                comfort -= 0.5 * self.map.tile_at(p).snow.min(1.0);
                // In the cold season, the map's edges (the far north and
                // south are the coldest, and a herd pressed to an edge has
                // nowhere to go): away from them.
                if cold_season {
                    let (x, y) = (p.x, p.y);
                    let edge = x.min(y).min(self.map.width as f32 - x).min(self.map.height as f32 - y);
                    comfort -= COLD_EDGE_WEIGHT * (1.0 - edge / COLD_EDGE).max(0.0);
                }
            }
            // Where this owner's last foundings died: never again.
            let fatal: f32 = avoid.iter().map(|h| (1.0 - h.dist(p) / AVOID_RADIUS).max(0.0)).fold(0.0, f32::max);
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
            let standing = if self.tick > 0 && (self.config.balance.founding_food > 0.0 || now) {
                self.standing_share(p)
            } else {
                1.0
            };
            // With `now`: plants standing within 4 tiles for this many
            // founders (a winter pasture or a grazed-out one feeds few).
            let cover = if now { (self.veg_near(p) / (*founders as f32 * FOOD_STOCK)).min(1.0) } else { 1.0 };
            let food = t.veg_max / 40.0 * productivity * plant_eff * standing * (0.4 + 0.6 * cover)
                + meat_eff * 0.04 * prey_count.min(60.0);
            // With `founding_safety`: food for the founders' first 20 days
            // (the plants within reach against what they will eat), and the
            // map's edges and corners in any season (a herd pressed into a
            // corner has nowhere to go; playtest 4's tundra rim and
            // water-ringed corner starved 21 of 27).
            let mut safety = 0.0;
            if settle.is_some() {
                let need = self.founding_need(probe, *founders);
                if need > 0.0 {
                    let cover20 = (self.reachable_veg_near(probe, p, 8) / need).min(1.0);
                    safety -= 2.5 * (1.0 - cover20) * (1.0 - carn).clamp(0.0, 1.0);
                }
                let (dx, dy) = (p.x.min(self.map.width as f32 - p.x), p.y.min(self.map.height as f32 - p.y));
                safety -= 1.5 * (1.0 - dx.min(dy) / EDGE_MARGIN).max(0.0);
                safety -= 1.5 * (1.0 - dx.max(dy) / CORNER_MARGIN).max(0.0);
            }
            // Other players' animals within `RIVAL_RANGE` (nearer counts
            // more) and their territories' edges: uncontested ground first
            // (`Balance::founding_rivals`).
            let mut rival_near = 0.0f32;
            rivals
                .near(p, *rival_range, |q, &(weight, range)| rival_near += weight * (1.0 - q.dist(p) / range).max(0.0));
            let mut rivalry = RIVAL_WEIGHT * (1.0 - (-rival_near / RIVAL_SCALE).exp());
            for t in rival_lands.iter().chain(rival_ranges.iter()) {
                let gap = (t.center.dist(p) - t.radius).max(0.0);
                rivalry += RIVAL_LAND_WEIGHT * (1.0 - gap / RIVAL_RANGE).max(0.0);
            }
            let score = water + food + comfort + safety
                - rivalry
                - 0.05 * crowd_n
                - (0.3 * danger).min(4.0)
                - (0.1 * carn * on_top).min(2.0)
                - 2.0 * close
                - 6.0 * fatal
                + rng.gen_range(0.0..0.5);
            if score > best.0 {
                best = (score, p);
            }
        }
        best.1
    }

    /// The dry tiles beside drinkable water today (`Map::can_drink_at` and no
    /// water of their own) and how many of them `body` can stand on
    /// (`terrain::passable`: a bank steeper than it can climb is out):
    /// `(reachable, all)`. For the builder's warning that a design can't
    /// reach most water.
    pub fn shore_access(&self, body: &crate::stats::Body) -> (usize, usize) {
        let ground = self.map.ground();
        let (mut reachable, mut all) = (0, 0);
        for y in 0..self.map.height {
            for x in 0..self.map.width {
                let i = self.map.idx(x, y);
                let t = &self.map.tiles[i];
                if t.water_depth > 0.0 || !self.map.can_drink_at(Map::tile_center(x, y)) {
                    continue;
                }
                all += 1;
                reachable += usize::from(crate::terrain::passable(body, &ground[i], t));
            }
        }
        (reachable, all)
    }

    /// Whether the organism can walk each tile (`passable_for`, with only
    /// the tests that block): 0 if so, `u32::MAX` if not.
    /// With `no_ice`, ice holds nobody: deep water under it counts as open
    /// water.
    fn passable_map(&self, o: &Organism, no_ice: bool) -> Vec<u32> {
        let (ground, body) = (&self.map.terrain().ground, &o.traits.body);
        (0..self.map.width * self.map.height)
            .map(|i| {
                let t = &self.map.tiles[i];
                let open = if no_ice && t.ice > 0.0 && t.water_depth > 0.0 {
                    crate::terrain::passable(body, &ground[i], &crate::terrain::Tile { ice: 0.0, ..*t })
                } else {
                    crate::terrain::passable(body, &ground[i], t)
                };
                if open {
                    0
                } else {
                    u32::MAX
                }
            })
            .collect()
    }

    /// Connected regions of tiles the organism can walk (8-neighbor), as a
    /// label per tile; `u32::MAX` for impassable tiles. Regions are numbered
    /// in the order of their first tile (row by row), from runs of open
    /// tiles joined to the overlapping runs of the row above.
    fn passable_regions(&self, o: &Organism, no_ice: bool) -> Vec<u32> {
        let w = self.map.width;
        let open = self.passable_map(o, no_ice);
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

    /// [`World::water_reachable`], telling also whether any water reached
    /// lasts a drought: a lake's deep water (`Ground::base_water` of
    /// [`LASTING_WATER`] or more), not shallows or a stream that dry up.
    /// `(reachable, lasting)`.
    fn water_access(&self, region: &[u32], p: Vec2, threats: &PointGrid<u32>) -> (bool, bool) {
        const STEPS: usize = WATER_STEPS;
        const CLEARANCE: f32 = 5.0;
        let side = 2 * STEPS + 1;
        let ground = self.map.ground();
        let lasting_beside = |x: usize, y: usize| {
            (y.saturating_sub(1)..=(y + 1).min(self.map.height - 1)).any(|ny| {
                (x.saturating_sub(1)..=(x + 1).min(self.map.width - 1)).any(|nx| {
                    let i = self.map.idx(nx, ny);
                    self.map.tiles[i].has_drinkable_water() && ground[i].base_water >= LASTING_WATER
                })
            })
        };
        let (cx, cy) = self.map.tile_coords(p);
        let mut seen = vec![false; side * side];
        let mut frontier = vec![(cx, cy)];
        seen[STEPS * side + STEPS] = true;
        let mut reachable = false;
        for step in 0..=STEPS {
            let mut next = Vec::new();
            for &(x, y) in &frontier {
                if self.map.can_drink_at(Map::tile_center(x, y)) {
                    reachable = true;
                    if lasting_beside(x, y) {
                        return (true, true);
                    }
                }
                if step == STEPS {
                    continue;
                }
                for ny in y.saturating_sub(1)..=(y + 1).min(self.map.height - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(self.map.width - 1) {
                        let k = (ny + STEPS - cy) * side + (nx + STEPS - cx);
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
        (reachable, false)
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
    /// Hunter `i` killed by prey `prey` fighting back: the prey's lineage
    /// is remembered (`Organism::killer`) for the notice, but it isn't a
    /// kill by the prey's lineage (`Counters::kills_by_lineage`), nor
    /// predation (playtest 6: a hunter gored by a herd was "eaten by" it).
    pub(super) fn kill_fighting_back(&mut self, i: usize, prey: usize) {
        if !self.organisms[i].alive {
            return;
        }
        self.organisms[i].killer = Some(self.organisms[prey].lineage);
        self.kill(i, DeathCause::FoughtBack);
    }

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
        self.refresh_pack_room();
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
                let open = w.passable_map(o, false);
                let (mw, mh) = (w.map.width, w.map.height);
                assert_eq!(w.passable_regions(o, false), flood_labels(&open, mw, mh));
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

    pub(super) fn small_world(seed: u64) -> World {
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
    use super::tests::small_world;
    use super::*;

    /// Playtest 3: one player's founding landed three times on the same
    /// fatal peninsula. A re-founding by a player whose last founding died
    /// out lands away from where it died; with the old search
    /// (`founding_conditions` 0) the same founding lands on the same spot.
    #[test]
    fn a_refounding_avoids_where_the_last_founding_died() {
        let d = crate::design::FounderDesign::new("t", crate::design::Thermo::Warm, crate::design::BirthType::Live, 2)
            .with(crate::Gene::BodySize, 0.4)
            .founders(12);
        for seed in 1..4 {
            let mut w = small_world(seed);
            // (The spots this test checks were found without the
            // prey-scaled pack larder, version 58.)
            w.config.balance.prey_larder = 0.0;
            w.run_days(2);
            let found = |w: &mut World| {
                let ids = w.found_lineage_unchecked(77, &d);
                w.organism(ids[0]).unwrap().pos
            };
            let mut old = w.clone();
            old.config.balance.founding_conditions = 0.0;
            let first = found(&mut w);
            let old_first = found(&mut old);
            for v in [&mut w, &mut old] {
                v.kill_all_of(77, DeathCause::Thirst);
                v.run_days(1);
            }
            let again = found(&mut w);
            assert!(again.dist(first) > 0.5 * AVOID_RADIUS, "seed {seed}: {first:?} then {again:?}");
            let _ = (found(&mut old), old_first);
        }
    }

    /// In the cold season the search keeps founding homes off the map's
    /// edges where it can (playtest 3: a winter founding in the north
    /// snowfield): against the old search on the same worlds, homes sit
    /// farther in, and fewer on the edge itself. (On a crowded small map
    /// the interior can be too dangerous to win outright.)
    #[test]
    fn cold_season_homes_keep_off_the_edges() {
        let d = crate::design::FounderDesign::new("t", crate::design::Thermo::Warm, crate::design::BirthType::Live, 2)
            .with(crate::Gene::BodySize, 0.4)
            .founders(12);
        let (mut new_in, mut old_in, mut new_edge, mut old_edge) = (0.0f32, 0.0f32, 0, 0);
        for seed in 1..7 {
            let mut w = World::new(WorldConfig { seed, width: 96, height: 96, ..Default::default() });
            for (owner, design) in crate::presets::wild_designs() {
                let n = (design.founders / 3).max(4);
                w.found_lineage_unchecked(owner, &design.founders(n));
            }
            while w.days_until_winter() > 3 {
                w.run_days(1);
            }
            for on in [true, false] {
                let mut c = w.clone();
                c.config.balance.founding_conditions = if on { 1.0 } else { 0.0 };
                c.config.balance.founding_safety = if on { 1.0 } else { 0.0 };
                c.found_lineage_unchecked(77, &d);
                let p = c.lineage_homes.last().copied().flatten().expect("home recorded");
                let edge = p.x.min(p.y).min(c.map.width as f32 - p.x).min(c.map.height as f32 - p.y);
                if on {
                    (new_in, new_edge) = (new_in + edge, new_edge + usize::from(edge < 4.0));
                } else {
                    (old_in, old_edge) = (old_in + edge, old_edge + usize::from(edge < 4.0));
                }
            }
        }
        assert!(
            new_in > old_in && new_edge <= old_edge && new_edge <= 1,
            "in {new_in} vs {old_in}, on the edge {new_edge} vs {old_edge}"
        );
    }

    /// Water that lasts a drought beats shallows: with the shallows dried to
    /// a trickle a lake's deep water still counts, and the search tells them
    /// apart (`World::water_access`).
    #[test]
    fn water_access_tells_lasting_water_from_shallows() {
        let mut w = small_world(2);
        w.run_days(1);
        let probe = w.organisms.iter().find(|o| o.traits.carnivory < 0.3 && !o.traits.body.can_fly).unwrap().clone();
        let region = w.passable_map(&probe, false);
        let none = PointGrid::new(&w.map);
        let ground = w.map.ground().to_vec();
        let mut kinds = [0usize; 3];
        for y in (0..w.map.height).step_by(3) {
            for x in (0..w.map.width).step_by(3) {
                let p = Map::tile_center(x, y);
                if region[w.map.idx(x, y)] == u32::MAX {
                    continue;
                }
                let (reach, lasting) = w.water_access(&region, p, &none);
                assert_eq!(reach, w.water_reachable(&region, p, &none), "at {p:?}");
                assert!(!lasting || reach);
                kinds[usize::from(reach) + usize::from(lasting)] += 1;
                if lasting {
                    // Some lake tile of base water >= LASTING_WATER within reach.
                    let (cx, cy) = (x as i64, y as i64);
                    let near = (0..w.map.height as i64).any(|ty| {
                        (0..w.map.width as i64).any(|tx| {
                            (tx - cx).abs() <= WATER_STEPS as i64 + 1
                                && (ty - cy).abs() <= WATER_STEPS as i64 + 1
                                && ground[w.map.idx(tx as usize, ty as usize)].base_water >= LASTING_WATER
                        })
                    });
                    assert!(near, "lasting water at {p:?} without a lake near");
                }
            }
        }
        assert!(kinds.iter().all(|&n| n > 0), "spots with no water, shallows only, lasting water: {kinds:?}");
    }

    /// Playtest 6 (H4): 20 founders at 52% drought all died within 10
    /// days, 16 of thirst. In a drought (`Balance::founding_drought`) every
    /// home has water in reach that lasts it, and the founders start
    /// watered and lose water at half the rate for their first days.
    #[test]
    fn drought_foundings_land_by_lasting_water_and_start_watered() {
        let mut w = small_world(2);
        w.run_days(1);
        w.map.drought = 0.5;
        let design = crate::design::FounderDesign::new(
            "dry",
            crate::design::Thermo::Warm,
            crate::design::BirthType::IncubatedEggs,
            3,
        )
        .with(crate::Gene::BodySize, 0.25)
        .founders(20);
        for owner in 50..56 {
            let ids = w.found_lineage_unchecked(owner, &design);
            let l = w.lineages.len() - 1;
            let home = w.lineage_homes[l].expect("a home");
            let probe = w.organism(ids[0]).unwrap().clone();
            let region = w.passable_map(&probe, false);
            let (reach, lasting) = w.water_access(&region, home, &PointGrid::new(&w.map));
            assert!(reach && lasting, "owner {owner}: home {home:?} without lasting water");
            assert_eq!(probe.needs.thirst, 0.0);
            assert_eq!(w.founder_water_relief(&probe), 0.5);
        }
        // Past the first days, or out of the drought, no relief.
        let probe = w.organisms.iter().rev().find(|o| o.owner == 55).unwrap().clone();
        w.map.drought = 0.1;
        assert_eq!(w.founder_water_relief(&probe), 1.0);
        w.map.drought = 0.5;
        w.tick += (FOUNDER_WATER_DAYS * w.config.ticks_per_day as f32) as u64;
        assert_eq!(w.founder_water_relief(&probe), 1.0);
    }

    /// Playtest 7 (M10): a late joiner landed inside a rival's territory
    /// with four rivals within 35 tiles. With `Balance::founding_rivals` no
    /// founder lands in another player's territory and homes have fewer
    /// rival animals around them.
    #[test]
    fn foundings_keep_out_of_rival_territories_and_crowds() {
        let mut w = small_world(3);
        w.run_days(1);
        let land = groups::Territory { center: Vec2::new(32.0, 32.0), radius: 18.0, claimed_tick: w.tick };
        w.territories.insert((2, 0), land);
        let design =
            crate::design::FounderDesign::new("late", crate::design::Thermo::Warm, crate::design::BirthType::Live, 2)
                .founders(20);
        let rivals_near = |w: &World, owner: PlayerId, p: Vec2| {
            w.organisms
                .iter()
                .filter(|o| o.alive && o.owner != crate::WILD && o.owner != owner && o.pos.dist(p) < 35.0)
                .count()
        };
        let (mut on, mut off) = (0, 0);
        for owner in 60..68 {
            for rule in [1.0, 0.0] {
                let mut c = w.clone();
                c.config.balance.founding_rivals = rule;
                let ids = c.found_lineage_unchecked(owner, &design);
                let home = c.lineage_homes[c.lineages.len() - 1].unwrap();
                let n = rivals_near(&c, owner, home);
                if rule > 0.0 {
                    on += n;
                    for &id in &ids {
                        let p = c.organism(id).unwrap().pos;
                        assert!(!land.contains(p), "owner {owner}: founder at {p:?} inside the rival's territory");
                    }
                    assert!(land.center.dist(home) >= land.radius + RIVAL_TERRITORY_MARGIN, "home {home:?}");
                } else {
                    off += n;
                }
            }
        }
        assert!(on < off, "rival animals within 35 tiles of the homes: {on} with the rule, {off} without");
    }

    /// Playtest 8: late joiners landed inside a third group's territory 8
    /// days later, and beside a pack. With `Balance::rival_reach` a rival
    /// group's migration target counts as its ground (no home there), and
    /// predators keep homes farther away than plant eaters do.
    #[test]
    fn foundings_keep_clear_of_where_rivals_are_headed_and_of_packs() {
        let mut w = small_world(3);
        w.run_days(1);
        // A rival group (player 2, the sample herd grazers) headed for the
        // middle of the map.
        let owner2 = w.organisms.iter().find(|o| o.alive && o.owner != crate::WILD).map(|o| o.owner).unwrap();
        let target = Vec2::new(32.0, 32.0);
        w.migrate_orders.insert((owner2, 0), target);
        let members = w.organisms.iter().filter(|o| o.alive && o.owner == owner2 && o.group == 0).count();
        let keep = World::home_radius(members) + RIVAL_FORAGE;
        let design =
            crate::design::FounderDesign::new("late", crate::design::Thermo::Warm, crate::design::BirthType::Live, 2)
                .founders(20);
        for owner in 60..66 {
            let mut c = w.clone();
            c.found_lineage_unchecked(owner, &design);
            let home = c.lineage_homes[c.lineages.len() - 1].unwrap();
            assert!(home.dist(target) >= keep, "owner {owner}: home {home:?} where the rival is headed");
        }
        // Predators weigh 3 times as much, out to twice the range.
        let probe_owner = 60;
        let pred = w.organisms.iter().find(|o| o.alive && o.owner != crate::WILD && o.traits.carnivory > 0.6);
        let herb = w.organisms.iter().find(|o| o.alive && o.owner != crate::WILD && o.traits.carnivory < 0.3);
        let (Some(pred), Some(herb)) = (pred, herb) else { panic!("the sample players: a pack and a herd") };
        let mut probe = herb.clone();
        probe.owner = probe_owner;
        let search = HomeSearch::new(&w, &probe, w.lineages.len() as u32, 20);
        let payload = |at: Vec2| {
            let mut found = None;
            search.rivals.near(at, 0.01, |q, &v| {
                if q == at {
                    found = Some(v);
                }
            });
            found.expect("the animal is in the search")
        };
        assert_eq!(payload(pred.pos), (RIVAL_PACK_WEIGHT, RIVAL_RANGE * RIVAL_PACK_RANGE));
        assert_eq!(payload(herb.pos), (1.0, RIVAL_RANGE));
        assert_eq!(search.rival_range, RIVAL_RANGE * RIVAL_PACK_RANGE);
    }

    /// Playtest 8: founders landed old (85 days 11 days after landing).
    /// With `Balance::founder_youth` a player's founding in a running world
    /// lands at most 10% of the lifespan past maturity.
    #[test]
    fn founders_land_as_young_adults() {
        let mut w = small_world(4);
        w.run_days(1);
        let design =
            crate::design::FounderDesign::new("y", crate::design::Thermo::Warm, crate::design::BirthType::Live, 2)
                .founders(30);
        let ages = |w: &mut World, owner: PlayerId| -> Vec<f32> {
            let tpd = w.config.ticks_per_day as f32;
            w.found_lineage_unchecked(owner, &design)
                .iter()
                .map(|&id| {
                    let o = w.organism(id).unwrap();
                    (o.age_ticks as f32 / tpd - o.traits.maturity_days) / o.traits.lifespan_days
                })
                .collect()
        };
        let young = ages(&mut w.clone(), 70);
        assert!(young.iter().all(|&a| (0.0..=FOUNDER_AGE_YOUNG + 1e-3).contains(&a)), "{young:?}");
        let mut old = w.clone();
        old.config.balance.founder_youth = 0.0;
        let old = ages(&mut old, 70);
        assert!(old.iter().any(|&a| a > FOUNDER_AGE_YOUNG), "{old:?}");
    }

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
