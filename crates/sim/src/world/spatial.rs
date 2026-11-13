//! Uniform grid for neighbor queries, rebuilt every tick with a counting sort.
//!
//! Each slot holds a small summary of its organism ([`Entry`]) next to its
//! neighbors in the same cell, so a scan can find everything within range,
//! and rule most of it out, without touching the organisms themselves.

use rayon::prelude::*;

use super::decide::{genes_of, Genes};
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
        Entry { pos: o.pos, idx: i as u32, lineage: o.lineage, mass: o.mass(), flags }
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
        }
    }

    fn cell_of(&self, p: Vec2) -> (usize, usize) {
        let c = ((p.x / self.cell).max(0.0) as usize).min(self.cols - 1);
        let r = ((p.y / self.cell).max(0.0) as usize).min(self.rows - 1);
        (c, r)
    }

    pub fn rebuild(&mut self, orgs: &[Organism]) {
        let n = self.cols * self.rows;
        self.starts.iter_mut().for_each(|s| *s = 0);
        for o in orgs.iter().filter(|o| o.alive) {
            let (c, r) = self.cell_of(o.pos);
            self.starts[r * self.cols + c + 1] += 1;
        }
        for k in 0..n {
            self.starts[k + 1] += self.starts[k];
        }
        let mut cursor = self.starts.clone();
        let blank = Entry { pos: Vec2::ZERO, idx: 0, lineage: 0, mass: 0.0, flags: 0 };
        self.items.clear();
        self.items.resize(self.starts[n] as usize, blank);
        for (i, o) in orgs.iter().enumerate().filter(|(_, o)| o.alive) {
            let (c, r) = self.cell_of(o.pos);
            let slot = &mut cursor[r * self.cols + c];
            self.items[*slot as usize] = Entry::of(i, o);
            *slot += 1;
        }
        self.pos.clear();
        self.pos.extend(self.items.iter().map(|e| e.pos));
        self.items.par_iter().map(|e| genes_of(&orgs[e.idx as usize].pheno)).collect_into_vec(&mut self.genes);
    }

    /// Kinship genes of the organism in `slot` (`decide::Genes`).
    pub fn genes(&self, slot: u32) -> &Genes {
        &self.genes[slot as usize]
    }

    pub fn entry(&self, slot: u32) -> &Entry {
        &self.items[slot as usize]
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
