//! Uniform grid for neighbor queries, rebuilt every tick with a counting sort.
//!
//! Each slot holds a small summary of its organism ([`Entry`]) next to its
//! neighbors in the same cell, so a scan can find everything within range,
//! and rule most of it out, without touching the organisms themselves.

use rayon::prelude::*;

use super::decide::{genes_of, DistanceBounds, Genes};
use crate::math::Vec2;
use crate::organism::{Action, Organism, Sex};

/// Grown and carnivorous enough to hunt: could threaten something
/// (`decide::is_armed`, the same test `decide::can_threaten` uses).
pub(crate) const ARMED: u8 = 1;
/// Mature, not pregnant and wanting to breed: could be courted this tick
/// (the breed-ready tick is checked on the organism).
pub(crate) const COURTING: u8 = 2;
pub(crate) const FEMALE: u8 = 4;
/// A sentry giving a warning (`Organism::alert`).
pub(crate) const ALERT: u8 = 8;
/// Chasing an intruder out of its group's territory (`Action::Defend`).
pub(crate) const DEFENDING: u8 = 16;

/// What a neighbor scan needs to know about an organism to decide whether it
/// is worth a closer look.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    pub pos: Vec2,
    pub idx: u32,
    pub lineage: u32,
    /// `Organism::mass()`.
    pub mass: f32,
    /// `Organism::owner` and `Organism::group`: whether a kin neighbor
    /// belongs to the viewer's herd, without reading the organism.
    pub owner: u32,
    pub group: u8,
    pub flags: u8,
}

impl Entry {
    fn of(i: usize, o: &Organism) -> Self {
        let mut flags = 0;
        if super::decide::is_armed(o) {
            flags |= ARMED;
        }
        if o.matured && o.pregnancy.is_none() && o.needs.breeding > 20.0 {
            flags |= COURTING;
        }
        if o.sex == Sex::Female {
            flags |= FEMALE;
        }
        if o.alert.is_some() {
            flags |= ALERT;
        }
        if o.action == Action::Defend {
            flags |= DEFENDING;
        }
        Entry { pos: o.pos, idx: i as u32, lineage: o.lineage, mass: o.mass(), owner: o.owner, group: o.group, flags }
    }
}

/// `Default` is an empty placeholder (snapshots skip the grid); `rebuild` on
/// a grid from `new` before querying.
#[derive(Clone, Default)]
pub(crate) struct SpatialGrid {
    cell: f32,
    cols: usize,
    rows: usize,
    starts: Vec<u32>,
    items: Vec<Entry>,
    /// Each slot's position again, packed: all a scan reads of the slots it
    /// rules out.
    pos: Vec<Vec2>,
    /// Kinship genes of each slot's organism (`decide::Genes`), alongside
    /// `items`: a crowd's genes lie together instead of scattered over the
    /// organisms.
    genes: Vec<Genes>,
    /// A reference point per lineage (index), in the space of [`Genes`]:
    /// the mean of its members' genes when last refreshed. Any point would
    /// do; one central to the lineage gives the tightest bounds.
    refs: Vec<Genes>,
    /// Rebuilds until `refs` is refreshed.
    refs_age: u32,
    /// Bounds on each slot's distance to its lineage's reference point.
    ref_bounds: Vec<DistanceBounds>,
    /// `rebuild`'s buffer: each organism's cell and entry, if alive.
    placed: Vec<Option<(u32, Entry)>>,
    /// Slot of each living organism, by organism index.
    slot_of: Vec<u32>,
}

impl SpatialGrid {
    pub fn new(width: usize, height: usize, cell: f32) -> Self {
        let cols = (width as f32 / cell).ceil() as usize;
        let rows = (height as f32 / cell).ceil() as usize;
        Self {
            cell,
            cols,
            rows,
            starts: vec![0; cols * rows + 1],
            items: Vec::new(),
            pos: Vec::new(),
            genes: Vec::new(),
            refs: Vec::new(),
            refs_age: 0,
            ref_bounds: Vec::new(),
            placed: Vec::new(),
            slot_of: Vec::new(),
        }
    }

    fn cell_of(&self, p: Vec2) -> (usize, usize) {
        let c = ((p.x / self.cell).max(0.0) as usize).min(self.cols - 1);
        let r = ((p.y / self.cell).max(0.0) as usize).min(self.rows - 1);
        (c, r)
    }

    pub fn rebuild(&mut self, orgs: &[Organism]) {
        let n = self.cols * self.rows;
        // Each living organism's cell and summary, on all cores: reading
        // them touches several cache lines of every organism.
        let mut placed = std::mem::take(&mut self.placed);
        let this = &*self;
        orgs.par_iter()
            .with_min_len(super::light_min_len(orgs.len()))
            .enumerate()
            .map(|(i, o)| {
                o.alive.then(|| {
                    let (c, r) = this.cell_of(o.pos);
                    ((r * this.cols + c) as u32, Entry::of(i, o))
                })
            })
            .collect_into_vec(&mut placed);
        // Counting sort by cell, in organism order within a cell.
        self.starts.iter_mut().for_each(|s| *s = 0);
        for &(cell, _) in placed.iter().flatten() {
            self.starts[cell as usize + 1] += 1;
        }
        for k in 0..n {
            self.starts[k + 1] += self.starts[k];
        }
        let mut cursor = self.starts.clone();
        let blank = Entry { pos: Vec2::ZERO, idx: 0, lineage: 0, mass: 0.0, owner: 0, group: 0, flags: 0 };
        self.items.clear();
        self.items.resize(self.starts[n] as usize, blank);
        self.slot_of.clear();
        self.slot_of.resize(orgs.len(), u32::MAX);
        for &(cell, e) in placed.iter().flatten() {
            let slot = &mut cursor[cell as usize];
            self.items[*slot as usize] = e;
            self.slot_of[e.idx as usize] = *slot;
            *slot += 1;
        }
        self.placed = placed;
        self.pos.clear();
        self.pos.extend(self.items.iter().map(|e| e.pos));
        let min_len = super::light_min_len(self.items.len());
        self.items
            .par_iter()
            .with_min_len(min_len)
            .map(|e| genes_of(&orgs[e.idx as usize]))
            .collect_into_vec(&mut self.genes);
        self.refresh_refs();
        let (refs, genes) = (&self.refs, &self.genes);
        self.items
            .par_iter()
            .with_min_len(min_len)
            .zip(genes)
            .map(|(e, g)| match refs.get(e.lineage as usize) {
                Some(r) => DistanceBounds::new(g, r),
                // No reference: bounds that settle nothing.
                None => DistanceBounds { lo: 0.0, hi: f32::INFINITY },
            })
            .collect_into_vec(&mut self.ref_bounds);
    }

    /// Recompute the lineages' reference points (`refs`) every
    /// `REFS_EVERY` rebuilds, or when a lineage has none. They only tighten
    /// or loosen bounds, never change a result.
    fn refresh_refs(&mut self) {
        const REFS_EVERY: u32 = 240;
        let lineages = self.items.iter().map(|e| e.lineage as usize + 1).max().unwrap_or(0);
        if self.refs_age > 0 && self.refs.len() >= lineages {
            self.refs_age -= 1;
            return;
        }
        self.refs_age = REFS_EVERY;
        let mut sums = vec![([0.0f64; std::mem::size_of::<Genes>() / 4], 0u32); lineages];
        for (e, g) in self.items.iter().zip(&self.genes) {
            let (sum, n) = &mut sums[e.lineage as usize];
            for (s, &x) in sum.iter_mut().zip(g) {
                *s += x as f64;
            }
            *n += 1;
        }
        self.refs = sums
            .iter()
            .map(|(sum, n)| {
                let mut r: Genes = [0.0; std::mem::size_of::<Genes>() / 4];
                for (r, &s) in r.iter_mut().zip(sum) {
                    *r = (s / (*n).max(1) as f64) as f32;
                }
                r
            })
            .collect();
    }

    /// Bounds on the distance from the organism in `slot` to its lineage's
    /// reference point (`decide::DistanceBounds::via_reference`).
    /// Bounds that settle nothing for a slot not in the grid.
    pub fn ref_bounds(&self, slot: u32) -> DistanceBounds {
        self.ref_bounds.get(slot as usize).copied().unwrap_or(DistanceBounds { lo: 0.0, hi: f32::INFINITY })
    }

    /// Kinship genes of the organism in `slot` (`decide::Genes`).
    pub fn genes(&self, slot: u32) -> &Genes {
        &self.genes[slot as usize]
    }

    /// Slot of living organism `i` (as of the last rebuild).
    pub fn slot_of(&self, i: usize) -> u32 {
        self.slot_of[i]
    }

    pub fn entry(&self, slot: u32) -> &Entry {
        &self.items[slot as usize]
    }

    /// Every slot's entry, in slot (cell) order: the living organisms
    /// grouped by where they stand.
    pub fn entries(&self) -> &[Entry] {
        &self.items
    }

    /// Calls `f(slot, squared distance)` for every organism within `radius`
    /// of `p`, in cell order. `slot` is for [`Self::entry`].
    pub fn query(&self, p: Vec2, radius: f32, mut f: impl FnMut(u32, f32)) {
        let (c0, r0) = self.cell_of(Vec2::new(p.x - radius, p.y - radius));
        let (c1, r1) = self.cell_of(Vec2::new(p.x + radius, p.y + radius));
        let r2 = radius * radius;
        for r in r0..=r1 {
            let row = r * self.cols;
            let (a, b) = (self.starts[row + c0], self.starts[row + c1 + 1]);
            for (slot, q) in (a..b).zip(&self.pos[a as usize..b as usize]) {
                let (dx, dy) = (q.x - p.x, q.y - p.y);
                let d2 = dx * dx + dy * dy;
                if d2 <= r2 {
                    f(slot, d2);
                }
            }
        }
    }
}
