//! Coarse per-species density: living members of each lineage per cell of a
//! fixed grid, rebuilt hourly. Unlike `kin_nearby` (what an organism saw this
//! tick, which saturates in a crowd), this keeps counting however dense a
//! colony gets, so it can drive density-dependent breeding.

use crate::math::Vec2;
use crate::organism::Organism;

/// Cell side in tiles.
const CELL: usize = 8;

pub(crate) struct SpeciesDensity {
    cols: usize,
    rows: usize,
    lineages: usize,
    counts: Vec<u16>,
}

impl SpeciesDensity {
    pub fn new(width: usize, height: usize) -> Self {
        SpeciesDensity { cols: width.div_ceil(CELL).max(1), rows: height.div_ceil(CELL).max(1), lineages: 0, counts: Vec::new() }
    }

    fn cell(&self, p: Vec2) -> usize {
        let x = ((p.x.max(0.0) as usize) / CELL).min(self.cols - 1);
        let y = ((p.y.max(0.0) as usize) / CELL).min(self.rows - 1);
        y * self.cols + x
    }

    pub fn rebuild(&mut self, organisms: &[Organism], lineages: usize) {
        self.lineages = lineages;
        self.counts.clear();
        self.counts.resize(self.cols * self.rows * lineages, 0);
        for o in organisms.iter().filter(|o| o.alive && (o.lineage as usize) < lineages) {
            let k = self.cell(o.pos) * lineages + o.lineage as usize;
            self.counts[k] = self.counts[k].saturating_add(1);
        }
    }

    /// Members of `lineage` per tile around `p`.
    pub fn per_tile(&self, p: Vec2, lineage: u32) -> f32 {
        if lineage as usize >= self.lineages {
            return 0.0;
        }
        self.counts[self.cell(p) * self.lineages + lineage as usize] as f32 / (CELL * CELL) as f32
    }
}
