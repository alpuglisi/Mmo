//! Uniform grid for neighbor queries, rebuilt every tick with a counting sort.

use crate::math::Vec2;
use crate::organism::Organism;

pub(crate) struct SpatialGrid {
    cell: f32,
    cols: usize,
    rows: usize,
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl SpatialGrid {
    pub fn new(width: usize, height: usize, cell: f32) -> Self {
        let cols = (width as f32 / cell).ceil() as usize;
        let rows = (height as f32 / cell).ceil() as usize;
        Self { cell, cols, rows, starts: vec![0; cols * rows + 1], items: Vec::new() }
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
        self.items.resize(self.starts[n] as usize, 0);
        for (i, o) in orgs.iter().enumerate().filter(|(_, o)| o.alive) {
            let (c, r) = self.cell_of(o.pos);
            let slot = &mut cursor[r * self.cols + c];
            self.items[*slot as usize] = i as u32;
            *slot += 1;
        }
    }

    /// Calls `f` for every organism index in cells overlapping the radius.
    /// Callers still check exact distance.
    pub fn query(&self, p: Vec2, radius: f32, mut f: impl FnMut(usize)) {
        let (c0, r0) = self.cell_of(Vec2::new(p.x - radius, p.y - radius));
        let (c1, r1) = self.cell_of(Vec2::new(p.x + radius, p.y + radius));
        for r in r0..=r1 {
            for c in c0..=c1 {
                let k = r * self.cols + c;
                for &i in &self.items[self.starts[k] as usize..self.starts[k + 1] as usize] {
                    f(i as usize);
                }
            }
        }
    }
}
