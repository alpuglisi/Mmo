//! Bounded A* over the tile grid ("Movement and terrain" in the design doc:
//! impassable tiles are avoided by pathfinding, costly ones are routed
//! around when a detour is quicker).
//!
//! Searches are rare: movement steers straight at its target and only plans
//! when the direct step is blocked (see `World::move_toward`), then follows
//! the cached [`Path`] on the organism. So each search uses the organism's
//! own body against `terrain::traverse` (exact, no body classes) and a node
//! budget (`Balance::path_nodes`) caps the worst case.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::math::Vec2;
use crate::terrain::{Ground, Map, Tile, Traverse};

/// Fastest speed multiplier movement uses (`move_toward` caps tile speed).
pub const MAX_TILE_SPEED: f32 = 1.5;

/// A route to a target, kept on the organism between ticks.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Path {
    /// Tile indices still to visit, last element next.
    pub waypoints: Vec<u32>,
    /// The target the path was planned for.
    pub goal: Vec2,
    /// The search ran out of budget: the path ends at the tile it got closest
    /// to, and is re-planned from there.
    pub partial: bool,
    /// No route was found; don't search again for this goal until the
    /// terrain changes (`epoch`).
    pub failed: bool,
    /// Terrain epoch (hour index) of the search.
    pub epoch: u64,
}

impl Path {
    pub fn new(goal: Vec2) -> Self {
        Path { waypoints: Vec::new(), goal, partial: false, failed: false, epoch: 0 }
    }

    /// Forget the route (keeping the allocation for the next plan).
    pub fn clear(&mut self) {
        self.waypoints.clear();
        self.failed = false;
        self.partial = false;
    }

    /// Does this path (or failed search) still answer for `target`, seen from
    /// `pos`? A target that moved more than a quarter of the remaining
    /// distance (at least 1.5 tiles) needs a new plan.
    pub fn serves(&self, pos: Vec2, target: Vec2) -> bool {
        self.goal.dist(target) <= (0.25 * pos.dist(target)).max(1.5)
    }

    pub fn next_tile(&self) -> Option<u32> {
        self.waypoints.last().copied()
    }
}

/// Reusable search buffers, sized to the map. Stamps mark which entries
/// belong to the current search, so nothing is cleared between searches.
/// Nothing in them carries over from one search to the next, so a clone
/// starts empty (as after a snapshot restore): a world clone, which the
/// server's saver takes daily, doesn't copy a map-sized scratch.
#[derive(Debug, Default)]
pub struct PathScratch {
    stamp: u32,
    seen: Vec<u32>,
    g: Vec<f32>,
    parent: Vec<u32>,
    closed: Vec<bool>,
    /// Cost multiplier of entering each tile (`1 / speed`), or infinity if
    /// blocked; evaluated once per tile per search.
    enter: Vec<f32>,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
}

/// Result of a search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Found,
    /// Ran out of budget; the path leads to the closest tile reached.
    Partial,
    /// No route (or none at all within budget that gets any closer).
    None,
}

/// Octile distance between two tiles.
fn octile(ax: usize, ay: usize, bx: usize, by: usize) -> f32 {
    let (dx, dy) = (ax.abs_diff(bx) as f32, ay.abs_diff(by) as f32);
    dx.max(dy) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dy)
}

impl Clone for PathScratch {
    fn clone(&self) -> Self {
        PathScratch::default()
    }
}

impl PathScratch {
    fn begin(&mut self, n: usize) {
        if self.seen.len() != n {
            *self = PathScratch {
                seen: vec![0; n],
                g: vec![0.0; n],
                parent: vec![0; n],
                closed: vec![false; n],
                enter: vec![0.0; n],
                ..Default::default()
            };
        }
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.seen.fill(0);
            self.stamp = 1;
        }
        self.heap.clear();
    }

    fn touch(&mut self, i: usize, cost: &impl Fn(&Ground, &Tile) -> Option<f32>, map: &Map) {
        if self.seen[i] != self.stamp {
            self.seen[i] = self.stamp;
            self.g[i] = f32::INFINITY;
            self.closed[i] = false;
            self.enter[i] = cost(&map.ground()[i], &map.tiles[i]).unwrap_or(f32::INFINITY);
        }
    }

    /// A* from the tile under `from` to the tile under `to` over 8-neighbor
    /// moves (no corner cutting past a blocked tile). `cost` gives the time
    /// multiplier of entering a tile, `None` if blocked. If the goal tile is
    /// blocked, any tile next to it will do (drinking from the shore, a
    /// target in the water). Expands at most `budget` tiles. Writes the
    /// waypoints (next one last, start tile excluded) into `out`.
    pub fn search(
        &mut self,
        map: &Map,
        from: Vec2,
        to: Vec2,
        budget: usize,
        cost: impl Fn(&Ground, &Tile) -> Option<f32>,
        out: &mut Vec<u32>,
    ) -> Outcome {
        out.clear();
        let w = map.width;
        let n = w * map.height;
        self.begin(n);
        let (sx, sy) = map.tile_coords(from);
        let (gx, gy) = map.tile_coords(to);
        let start = sy * w + sx;
        let goal = gy * w + gx;
        self.touch(goal, &cost, map);
        let goal_open = self.enter[goal].is_finite();
        if !goal_open {
            // Out in a lake (or up a cliff): unless some tile next to it can
            // be stood on, nothing would reach it, however long the search.
            let mut shore = false;
            for y in gy.saturating_sub(1)..=(gy + 1).min(map.height - 1) {
                for x in gx.saturating_sub(1)..=(gx + 1).min(w - 1) {
                    self.touch(y * w + x, &cost, map);
                    shore |= self.enter[y * w + x].is_finite();
                }
            }
            if !shore {
                return Outcome::None;
            }
        }
        let at_goal = |x: usize, y: usize| {
            if goal_open {
                x == gx && y == gy
            } else {
                x.abs_diff(gx) <= 1 && y.abs_diff(gy) <= 1
            }
        };
        // Octile tiles at speed 1. Only tiles faster than that (a strong
        // swimmer in water) make it overestimate, and then only slightly;
        // the admissible bound (`/ MAX_TILE_SPEED`) would expand far more.
        let h = |x: usize, y: usize| octile(x, y, gx, gy);
        self.touch(start, &cost, map);
        self.g[start] = 0.0;
        self.parent[start] = start as u32;
        self.heap.push(Reverse((h(sx, sy).to_bits(), start as u32)));
        let mut best = (h(sx, sy), start);
        let mut expanded = 0usize;
        let mut found = None;
        while let Some(Reverse((_, i))) = self.heap.pop() {
            let i = i as usize;
            if self.closed[i] {
                continue;
            }
            self.closed[i] = true;
            let (x, y) = (i % w, i / w);
            if at_goal(x, y) {
                found = Some(i);
                break;
            }
            let hi = h(x, y);
            if hi < best.0 {
                best = (hi, i);
            }
            expanded += 1;
            if expanded > budget {
                break;
            }
            let gi = self.g[i];
            // Orthogonal neighbors first (their passability gates diagonals).
            let mut open = [false; 4];
            for (k, (dx, dy)) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)].into_iter().enumerate() {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= map.height as i32 {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                self.touch(j, &cost, map);
                open[k] = self.enter[j].is_finite();
                if open[k] {
                    self.relax(i, j, gi + self.enter[j], h(nx as usize, ny as usize));
                }
            }
            for (dx, dy, a, b) in [(-1i32, -1i32, 0, 2), (1, -1, 1, 2), (-1, 1, 0, 3), (1, 1, 1, 3)] {
                if !(open[a] && open[b]) {
                    continue;
                }
                let (nx, ny) = ((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                let j = ny * w + nx;
                self.touch(j, &cost, map);
                if self.enter[j].is_finite() {
                    self.relax(i, j, gi + std::f32::consts::SQRT_2 * self.enter[j], h(nx, ny));
                }
            }
        }
        let (end, outcome) = match found {
            Some(e) => (e, Outcome::Found),
            None if best.1 != start && expanded > budget => (best.1, Outcome::Partial),
            None => return Outcome::None,
        };
        let mut k = end;
        while k != start {
            out.push(k as u32);
            k = self.parent[k] as usize;
        }
        outcome
    }

    fn relax(&mut self, i: usize, j: usize, g: f32, h: f32) {
        if !self.closed[j] && g < self.g[j] {
            self.g[j] = g;
            self.parent[j] = i as u32;
            // Positive floats order like their bits; ties go to the lower
            // tile index, so the search is deterministic.
            self.heap.push(Reverse(((g + h).to_bits(), j as u32)));
        }
    }
}

/// Time multiplier of entering a tile under a movement rule: `1 / speed`
/// (capped like `move_toward`), `None` if blocked.
pub fn enter_cost(rule: Traverse) -> Option<f32> {
    match rule {
        Traverse::Blocked => None,
        Traverse::Passable { speed, .. } => Some(1.0 / speed.min(MAX_TILE_SPEED)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::Biome;

    /// All-grass map with the given tiles turned to deep water.
    fn map_with_water(w: usize, h: usize, water: impl Fn(usize, usize) -> bool) -> Map {
        let mut m = Map::generate(1, w, h);
        let terrain = m.edit_terrain();
        for y in 0..h {
            for x in 0..w {
                let g = &mut terrain.ground[y * w + x];
                g.biome = if water(x, y) { Biome::DeepWater } else { Biome::Grassland };
            }
        }
        m
    }

    fn walk(g: &Ground, _: &Tile) -> Option<f32> {
        (g.biome != Biome::DeepWater).then_some(1.0)
    }

    fn walkable(m: &Map, k: usize) -> bool {
        walk(&m.ground()[k], &m.tiles[k]).is_some()
    }

    #[test]
    fn routes_around_a_wall_without_cutting_corners() {
        // Wall at x = 5 from y = 0 to 8; gap at the bottom.
        let m = map_with_water(12, 12, |x, y| x == 5 && y <= 8);
        let mut s = PathScratch::default();
        let mut out = Vec::new();
        let r = s.search(&m, Vec2::new(2.5, 2.5), Vec2::new(8.5, 2.5), 500, walk, &mut out);
        assert_eq!(r, Outcome::Found);
        assert_eq!(*out.first().unwrap(), (2 * 12 + 8) as u32);
        let mut prev = (2usize, 2usize);
        for &k in out.iter().rev() {
            let (x, y) = (k as usize % 12, k as usize / 12);
            assert!(walkable(&m, k as usize));
            assert!(x.abs_diff(prev.0) <= 1 && y.abs_diff(prev.1) <= 1);
            // A diagonal step needs both orthogonal tiles open.
            assert!(walkable(&m, prev.1 * 12 + x) && walkable(&m, y * 12 + prev.0));
            prev = (x, y);
        }
        assert!(out.iter().any(|&k| k as usize / 12 >= 9), "went through the gap");
    }

    #[test]
    fn unreachable_goal_and_budget() {
        // Goal enclosed by water.
        let m = map_with_water(12, 12, |x, y| {
            (x == 7 || x == 10) && (4..=7).contains(&y) || (y == 4 || y == 7) && (7..=10).contains(&x)
        });
        let mut s = PathScratch::default();
        let mut out = Vec::new();
        assert_eq!(s.search(&m, Vec2::new(1.5, 1.5), Vec2::new(8.5, 5.5), 10_000, walk, &mut out), Outcome::None);
        // A blocked goal is reached by standing next to it.
        assert_eq!(s.search(&m, Vec2::new(1.5, 1.5), Vec2::new(7.5, 5.5), 10_000, walk, &mut out), Outcome::Found);
        // A tight budget gives a partial path that gets closer.
        let open = map_with_water(40, 40, |_, _| false);
        let r = s.search(&open, Vec2::new(1.5, 1.5), Vec2::new(38.5, 38.5), 5, walk, &mut out);
        assert_eq!(r, Outcome::Partial);
        assert!(!out.is_empty());
    }
}
