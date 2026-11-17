//! Spectator view (`GET /api/spectate`): what a read-only, logged-out
//! watcher of a world may see, for a stream or a shared link
//! (`client/src/ui/spectate.ts`).
//!
//! Nothing here is private to a player (playtest 1, finding 27):
//!
//! - **No positions or ids of animals.** Each lineage's living members are
//!   counted on a coarse grid of [`SPECTATE_CELL`]-tile cells, the same
//!   resolution for wild and player lineages; no organism id, sex, needs,
//!   action, order, pregnancy or group goes out.
//! - **No genes.** A lineage shows its most common adult [`Appearance`] (the
//!   coarse classes and buckets every player already sees on the map), not
//!   gene values.
//! - **Not live.** The runner answers from a copy taken at most once per
//!   simulated day (`Runner::spectate`), so a watcher can't follow animals
//!   tick by tick.
//! - Player names, head counts and allele shares are those of the public
//!   leaderboard; active catastrophes those of the public chronicle.

use std::collections::{BTreeMap, HashMap};

use evo_sim::WILD;
use serde::{Deserialize, Serialize};

use super::{catastrophe_view, Game};
use crate::appearance::Appearance;
use crate::protocol::{CatastropheView, ClockView};

/// Side of a density cell in tiles.
pub const SPECTATE_CELL: usize = 8;

/// One lineage as a spectator sees it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpectateLineage {
    /// The lineage's index (what `?lineage=` follows).
    pub lineage: u32,
    pub name: String,
    /// The owner's public name, or "wild".
    pub owner: String,
    pub wild: bool,
    pub alive: usize,
    pub adults: usize,
    /// The owner's allele share among breeding adults (player lineages only:
    /// the wild ecosystem shares one owner).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<f64>,
    /// The most common look of its living adults (or of any member while it
    /// has no adult); none once it died out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<Appearance>,
    /// Living members per cell: `[cell x, cell y, count]`, cells with none
    /// left out.
    pub cells: Vec<[u32; 3]>,
}

/// `GET /api/spectate`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Spectate {
    pub clock: ClockView,
    /// Map size in tiles.
    pub width: usize,
    pub height: usize,
    /// Cell side in tiles (`SPECTATE_CELL`).
    pub cell: usize,
    /// Catastrophes under way.
    pub catastrophes: Vec<CatastropheView>,
    /// Every lineage that ever lived here, by index (dead ones with no cells).
    pub lineages: Vec<SpectateLineage>,
}

impl Game {
    /// The spectator view of this moment (the runner caches it per day).
    pub fn spectate(&self) -> Spectate {
        let w = &self.world;
        let snap = w.latest_legacy();
        let n = w.lineages.len();
        let mut cells: Vec<BTreeMap<(u32, u32), u32>> = vec![BTreeMap::new(); n];
        let mut looks: Vec<HashMap<Appearance, usize>> = vec![HashMap::new(); n];
        let mut young: Vec<HashMap<Appearance, usize>> = vec![HashMap::new(); n];
        for o in w.organisms.iter().filter(|o| o.alive) {
            let l = o.lineage as usize;
            if l >= n {
                continue;
            }
            let cx = (o.pos.x.max(0.0) as usize / SPECTATE_CELL) as u32;
            let cy = (o.pos.y.max(0.0) as usize / SPECTATE_CELL) as u32;
            *cells[l].entry((cx, cy)).or_default() += 1;
            let into = if o.matured { &mut looks[l] } else { &mut young[l] };
            *into.entry(Appearance::of(o)).or_default() += 1;
        }
        let counts = w.lineage_counts();
        let lineages = w
            .lineages
            .iter()
            .enumerate()
            .map(|(l, lin)| {
                let wild = lin.owner == WILD;
                let pick = if looks[l].is_empty() { &young[l] } else { &looks[l] };
                // The most common look; a tie goes to the one whose JSON
                // sorts first, so the answer doesn't depend on hash order.
                let look = pick
                    .iter()
                    .max_by(|a, b| {
                        a.1.cmp(b.1).then_with(|| {
                            let (ka, kb) = (serde_json::to_string(a.0).ok(), serde_json::to_string(b.0).ok());
                            kb.cmp(&ka)
                        })
                    })
                    .map(|(a, _)| *a);
                SpectateLineage {
                    lineage: l as u32,
                    name: lin.name.clone(),
                    owner: self.name(lin.owner),
                    wild,
                    alive: counts.get(l).map_or(0, |c| c.alive),
                    adults: counts.get(l).map_or(0, |c| c.adults),
                    share: if wild { None } else { snap.map(|s| s.share(lin.owner)) },
                    look,
                    cells: cells[l].iter().map(|(&(x, y), &c)| [x, y, c]).collect(),
                }
            })
            .collect();
        Spectate {
            clock: self.clock(),
            width: w.map.width,
            height: w.map.height,
            cell: SPECTATE_CELL,
            catastrophes: w.chronicle.catastrophes.iter().filter(|c| c.is_active()).map(catastrophe_view).collect(),
            lineages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{build_world, GameConfig, WorldSettings};

    #[test]
    fn spectators_see_cells_and_looks_but_nothing_private() {
        let world = build_world(&WorldSettings { seed: 3, width: 48, height: 48, wild: true, ..Default::default() });
        let g = Game::new(world, GameConfig::default());
        let s = g.spectate();
        assert_eq!(s.cell, SPECTATE_CELL);
        assert!(!s.lineages.is_empty());
        let total: u32 = s.lineages.iter().flat_map(|l| l.cells.iter().map(|c| c[2])).sum();
        assert_eq!(total as usize, g.world.alive_count());
        for l in &s.lineages {
            assert_eq!(l.cells.iter().map(|c| c[2] as usize).sum::<usize>(), l.alive, "{}", l.name);
            assert!(l.wild && l.share.is_none());
            assert!(l.look.is_some() || l.alive == 0);
            for c in &l.cells {
                assert!((c[0] as usize) * SPECTATE_CELL < s.width && (c[1] as usize) * SPECTATE_CELL < s.height);
            }
        }
        // The JSON names no organism, position, gene or need.
        let json = serde_json::to_string(&s).unwrap();
        for private in ["\"id\"", "\"x\"", "\"y\"", "\"genes\"", "\"hunger\"", "\"pregnant\"", "\"sex\"", "\"order\""] {
            assert!(!json.contains(private), "{private} in {json}");
        }
    }
}
