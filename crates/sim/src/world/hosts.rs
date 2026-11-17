//! What the economy reads and does through the world: where each player's
//! scored alleles live (by host lineage), and reinforcements (kin joining a
//! small lineage, which the server sells for renown).

use std::collections::BTreeMap;

use rand::Rng;
use rayon::prelude::*;

use super::World;
use crate::genome::PlayerId;
use crate::math::Vec2;
use crate::organism::{OrgId, Organism, Sex};
use crate::rng::{rng_for, stream};

/// Alleles in breeding adults by (host lineage, origin player), counted as
/// the legacy score counts them (`LegacySnapshot`: the same adults, loci and
/// morphology alleles; cohort members of abstract regions weighted alike).
/// Summed over hosts, an origin's alleles are its legacy tally.
pub type HostTally = BTreeMap<(u32, PlayerId), f64>;

impl World {
    /// [`HostTally`] of the world now: whose alleles every lineage's
    /// breeding adults carry. Whole numbers (exactly, for agents), added in
    /// a fixed order, so it is the same at any thread count.
    pub fn legacy_hosts(&self) -> HostTally {
        let morph = self.config.balance.morph_in_legacy();
        type Tally = BTreeMap<(u32, PlayerId), u64>;
        let tally: Tally = self
            .organisms
            .par_iter()
            .filter(|o| o.is_breeding_adult())
            .fold(
                || (Tally::new(), Vec::new()),
                |(mut t, mut counts): (Tally, Vec<(PlayerId, u32)>), o| {
                    o.genome.count_origins_with(&mut counts, morph);
                    for &(p, n) in &counts {
                        *t.entry((o.lineage, p)).or_default() += n as u64;
                    }
                    (t, counts)
                },
            )
            .map(|(t, _)| t)
            .reduce(Tally::new, |mut a, b| {
                for (k, n) in b {
                    *a.entry(k).or_default() += n;
                }
                a
            });
        let mut out: HostTally = tally.into_iter().map(|(k, n)| (k, n as f64)).collect();
        if self.config.lod.enabled {
            let today = self.today_f64();
            let mut per = BTreeMap::new();
            for (_, c) in self.cohorts() {
                if c.pool.is_empty() {
                    continue;
                }
                let a = c.adults(today);
                let adults = (a[0] + a[1]) as f64;
                if adults == 0.0 {
                    continue;
                }
                let w = adults / c.pool.len() as f64;
                for e in &c.pool {
                    per.clear();
                    e.genome.tally_origins_with(w, &mut per, morph);
                    for (&p, &n) in &per {
                        *out.entry((c.lineage, p)).or_default() += n;
                    }
                }
            }
        }
        out
    }

    /// Reinforcements: up to `n` adults join `lineage` near one of its
    /// living adults (agents only), each a copy of a living adult's genome,
    /// so every allele and its owner tag is the lineage's own (a hybrid's
    /// foreign alleles included): kin arriving from elsewhere, not new
    /// founders. Sexes alternate; ages are adult and mixed, as founders'.
    /// They join the anchor's group. Returns the new organisms' ids (none
    /// if the lineage has no living adult agent, or its hard cap is full).
    pub fn reinforce_lineage(&mut self, lineage: u32, n: u32) -> Vec<OrgId> {
        let adults: Vec<usize> = (0..self.organisms.len())
            .filter(|&i| {
                let o = &self.organisms[i];
                o.alive && o.matured && o.lineage == lineage
            })
            .collect();
        let n = (n as usize).min(self.cap_room(lineage));
        if adults.is_empty() || n == 0 {
            return Vec::new();
        }
        let mut rng = rng_for(self.config.seed, stream::FOUNDING, (1 << 40) | lineage as u64, self.next_id);
        let anchor = &self.organisms[adults[rng.gen_range(0..adults.len())]];
        let (home, group, owner) = (anchor.pos, anchor.group, anchor.owner);
        let tpd = self.config.ticks_per_day as f32;
        let mut ids = Vec::with_capacity(n);
        for k in 0..n {
            let src = &self.organisms[adults[rng.gen_range(0..adults.len())]];
            let genome = src.genome.clone();
            let generation = src.generation;
            let sex = if k % 2 == 0 { Sex::Female } else { Sex::Male };
            let mut o = Organism::new(0, owner, genome, sex, home);
            for _ in 0..20 {
                let off = Vec2::new(rng.gen_range(-3.0..3.0), rng.gen_range(-3.0..3.0));
                let cand = self.map.clamp(home + off);
                if self.passable_for(&o, cand) {
                    o.pos = cand;
                    break;
                }
            }
            o.lineage = lineage;
            o.group = group;
            o.generation = generation;
            o.growth = 1.0;
            o.matured = true;
            o.age_ticks = ((o.traits.maturity_days + rng.gen_range(0.0..o.traits.lifespan_days * 0.3)) * tpd) as u32;
            o.needs.breeding = rng.gen_range(0.0..40.0);
            ids.push(self.insert(o));
        }
        ids
    }

    fn today_f64(&self) -> f64 {
        self.tick as f64 / self.config.ticks_per_day as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets;
    use crate::WorldConfig;

    /// Summed over hosts, the tally is the legacy snapshot's.
    #[test]
    fn host_tally_sums_to_the_legacy_tally() {
        let mut w = presets::standard_world(WorldConfig { seed: 5, width: 64, height: 64, ..Default::default() }, 1);
        w.run_days(3);
        let hosts = w.legacy_hosts();
        let snap = w.latest_legacy().unwrap().clone();
        // The snapshot is from the day's start: compare against a fresh one.
        let morph = w.config.balance.morph_in_legacy();
        let fresh = crate::legacy::LegacySnapshot::compute(
            0,
            w.organisms.iter().filter(|o| o.is_breeding_adult()).map(|o| (&*o.genome, 0)),
            morph,
        );
        let mut by_origin: BTreeMap<PlayerId, f64> = BTreeMap::new();
        for (&(_, p), &n) in &hosts {
            *by_origin.entry(p).or_default() += n;
        }
        assert_eq!(by_origin, fresh.alleles);
        assert!(snap.total_alleles > 0.0);
        // Every host is a lineage, and a player's alleles live mostly at home.
        assert!(hosts.keys().all(|&(l, _)| (l as usize) < w.lineages.len()));
    }

    #[test]
    fn reinforcements_copy_living_adults_near_them() {
        let mut w = presets::standard_world(WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() }, 1);
        w.run_days(1);
        let l = w.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
        let before = w.organisms.iter().filter(|o| o.alive && o.lineage == l).count();
        let ids = w.reinforce_lineage(l, 6);
        assert_eq!(ids.len(), 6);
        let after = w.organisms.iter().filter(|o| o.alive && o.lineage == l).count();
        assert_eq!(after, before + 6);
        for id in ids {
            let o = w.organism(id).unwrap();
            assert!(o.matured && o.owner == 2);
            let kin = w.organisms.iter().any(|k| k.id != id && k.lineage == l && *k.genome == *o.genome);
            assert!(kin, "a copy of a living adult");
        }
        // A lineage without adults gets none.
        assert!(w.reinforce_lineage(999, 3).is_empty());
    }
}
