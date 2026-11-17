//! The carnivore-diet rules and predator brakes for LOD cohorts
//! (`world::lod`), approximately (*LOD cohorts and the carnivore rules* in
//! the README). Agents follow these rules one by one (`world::apply`,
//! `world::decide`); a cohort has only its members' counts by age and sex
//! and their mean hunger, health and fat, so each rule becomes its effect on
//! a day of the cohort:
//!
//! - **Carcasses** (`Balance::carcass_keep`): what the killer doesn't eat of
//!   a kill is left on a tile of the prey's region, where it rots at the
//!   agents' rate (`terrain::update_tile`) and meat eaters scavenge it.
//! - **Scavenging** (with carcasses or carrion on): a hunting cohort first
//!   eats the carcass meat it finds: all of its own region's and of the
//!   surrounding regions of its hunting range the share its carcass scent
//!   covers (`Balance::carcass_scent × reliance × (0.5 + nose)` tiles), and
//!   hunts only for the rest.
//! - **Carrion** (`Balance::carrion`): a cohort member dead of anything but
//!   predation or drowning leaves its body on its region's centre tile (a
//!   starved one at half), as an agent leaves it where it fell.
//! - **Gorging** (`Balance::carnivore_fat`): a hunting cohort hunts for its
//!   fat room too, up to the agents' gorging capacity, and banks the meat
//!   beyond its need as fat in any season.
//! - **Provisioning** (`Balance::provisioning`): providers feed mothers meat,
//!   so a hunting cohort's breeding condition rises from its meat share
//!   toward its whole food share by `2 × provisioning × care` (care: the
//!   higher of pack instinct and group tolerance).
//! - **Breeding territories** (`Balance::breeding_territory`): one litter at
//!   a time per territory, so a cohort starts at most `territories × litter
//!   / (gestation + weaning)` young a day, with `territories` its usable
//!   tiles over the territory's area (radius as for an agent).
//! - **Prey switching** (`Balance::prey_switching`): a hunting cohort exposes
//!   each prey lineage less by `min(1, 2 × pickiness) × prey_scarcity`, the
//!   share of agents that would give it up today.
//! - **Litters follow condition** (`Balance::condition_litter`): births scale
//!   by the agents' litter share at the cohort's mean hunger and fat.
//!
//! Each rule is a no-op at its tunable's default 0, and none draws random
//! numbers, so cohorts with the rules off run bit for bit as before. The
//! genes are read from the cohort's representative (mean carnivory, litter
//! and fat stores; the probe genome for temper, care and pickiness), the
//! same genes the agents' rules read.

use crate::genome::Gene;
use crate::organism::{DeathCause, Organism};
use crate::terrain::CHUNK;

use super::World;

/// How far the carnivore-diet rules apply at mean carnivory `carnivory`
/// (`apply::meat_reliance` for a cohort).
pub(super) fn reliance(carnivory: f32) -> f32 {
    let lo = super::decide::HUNTER_CARNIVORY;
    ((carnivory - lo) / (super::apply::OBLIGATE - lo)).clamp(0.0, 1.0)
}

/// Fat (hunger points per member) a cohort can still bank from meat
/// (`Balance::carnivore_fat`, `apply::gorge_capacity`); 0 with the rule off.
pub(super) fn gorge_room(bal: &crate::Balance, carnivory: f32, fat_stores: f32, fat: f32) -> f32 {
    if bal.carnivore_fat <= 0.0 {
        return 0.0;
    }
    (bal.carnivore_fat * reliance(carnivory) * (0.5 + fat_stores) - fat).max(0.0)
}

/// Whether hunting cohorts eat carcass meat off the tiles (carcasses kept or
/// carrion left).
pub(super) fn scavenges(bal: &crate::Balance) -> bool {
    bal.carcass_keep > 0.0 || bal.carrion > 0.0
}

/// The share of the carcass meat in each region of its hunting `range` a
/// hunting cohort finds: all of its own region's (`range[0]`), and of each
/// surrounding region the share its carcass scent covers, `π × scent² /
/// CHUNK²` up to 1 (scent `Balance::carcass_scent × reliance × (0.5 +
/// nose)` tiles, as for an agent; 0 without carcass scent).
pub(super) fn scavenge_shares(
    bal: &crate::Balance,
    carnivory: f32,
    probe: &Organism,
    range: &[usize],
) -> Vec<(usize, f32)> {
    let scent = bal.carcass_scent * reliance(carnivory) * (0.5 + probe.pheno[Gene::Nose]);
    let ring = (std::f32::consts::PI * scent * scent / (CHUNK * CHUNK) as f32).min(1.0);
    range.iter().enumerate().map(|(k, &q)| (q, if k == 0 { 1.0 } else { ring })).collect()
}

/// Breeding condition of a hunting cohort with providers
/// (`Balance::provisioning`): `breed_s` raised toward its whole food share
/// `s` by `2 × provisioning × care`; `breed_s` with the rule off.
pub(super) fn provisioned(bal: &crate::Balance, probe: &Organism, breed_s: f32, s: f32) -> f32 {
    if bal.provisioning <= 0.0 || s <= breed_s {
        return breed_s;
    }
    let care = super::apply::provision_care(probe);
    breed_s + (s - breed_s) * (2.0 * bal.provisioning * care).min(1.0)
}

/// Most births a day the breeding territories allow a cohort
/// (`Balance::breeding_territory`): its usable tiles over the territory's
/// area, each raising one litter of `litter` through `gestation +
/// weaning` days; None with the rule off or for a cohort the rule doesn't
/// reach (no meat reliance).
pub(super) fn territory_births(
    bal: &crate::Balance,
    carnivory: f32,
    probe: &Organism,
    tiles: usize,
    litter: f32,
    busy_days: f32,
) -> Option<f32> {
    if bal.breeding_territory <= 0.0 {
        return None;
    }
    let temper = (1.0 + probe.pheno[Gene::Aggression] - probe.pheno[Gene::GroupTolerance]).clamp(0.25, 2.0);
    let radius = bal.breeding_territory * reliance(carnivory) * temper;
    if radius <= 0.0 {
        return None;
    }
    // Agents of a tolerant kind keep together, so a group shares one
    // territory: a region holds fewer breeding units.
    let together = (1.0 - probe.pheno[Gene::GroupTolerance]).max(0.1);
    let territories = together * tiles as f32 / (std::f32::consts::PI * radius * radius);
    Some(territories * litter / busy_days.max(1.0))
}

/// Share of a litter carried to term at the cohort's mean hunger and fat
/// (`Balance::condition_litter`, `apply::litter_condition`): 1 with the
/// rule off.
pub(super) fn litter_share(bal: &crate::Balance, hunger: f32, fat: f32, litter: f32) -> f32 {
    let k = bal.condition_litter;
    if k <= 0.0 {
        return 1.0;
    }
    let nourished = ((90.0 - hunger + fat) / 60.0).clamp(0.0, 1.0);
    let fraction = ((litter - 1.0) / (crate::design::MAX_LITTER - 1) as f32).clamp(0.0, 1.0);
    let flex = (2.0 * fraction).min(1.0);
    (1.0 - k * flex * (1.0 - nourished)).clamp(0.0, 1.0)
}

impl World {
    /// Tile rectangle of region `r`: `(x0, y0, x1, y1)`, ends exclusive.
    fn region_rect(&self, r: usize) -> (usize, usize, usize, usize) {
        let cx = self.map.chunks_x;
        let (x0, y0) = ((r % cx) * CHUNK, (r / cx) * CHUNK);
        (x0, y0, (x0 + CHUNK).min(self.map.width), (y0 + CHUNK).min(self.map.height))
    }

    /// Prey switching for a hunting cohort (`Balance::prey_switching`): the
    /// share of prey lineage `l` it still hunts, `1 − min(1, 2 ×
    /// pickiness) × prey_scarcity(l)` (`decide::gives_up_on`, in
    /// expectation); 1 with the rule off.
    pub(super) fn cohort_switch(&self, probe: &Organism, l: u32) -> f32 {
        if self.config.balance.prey_switching <= 0.0 {
            return 1.0;
        }
        1.0 - (2.0 * probe.pheno[Gene::Pickiness]).min(1.0) * self.prey_scarcity(l)
    }

    /// Leave `meat` (food units) as a carcass on the centre tile of region
    /// `r`, where it rots as agents' carcasses do.
    pub(super) fn lod_leave_meat(&mut self, r: usize, meat: f32) {
        if meat <= 0.0 {
            return;
        }
        let (x0, y0, x1, y1) = self.region_rect(r);
        let i = self.map.idx((x0 + x1) / 2, (y0 + y1) / 2);
        self.map.tiles[i].meat += meat;
    }

    /// The body of a cohort member of `mass` kg dead of `cause` in region
    /// `r` (`Balance::carrion`, `apply::leave_carrion`): left as a carcass,
    /// a starved one at half; nothing after predation or drowning.
    pub(super) fn lod_carrion(&mut self, r: usize, mass: f32, cause: DeathCause) {
        let carrion = self.config.balance.carrion;
        if carrion <= 0.0 || matches!(cause, DeathCause::Drowning | DeathCause::Predation) {
            return;
        }
        let lean = if cause == DeathCause::Starvation { 0.5 } else { 1.0 };
        let meat = mass * self.config.balance.meat_per_kg * carrion * lean;
        self.lod_leave_meat(r, meat);
    }

    /// Eat up to `want` food units of the carcass meat a cohort finds in
    /// `regions` (each region's meat times its share), taken from every
    /// tile in proportion; returns what was eaten.
    pub(super) fn lod_scavenge(&mut self, regions: &[(usize, f32)], want: f32) -> f32 {
        if want <= 0.0 {
            return 0.0;
        }
        let mut total = 0.0f32;
        for &(q, share) in regions {
            let (x0, y0, x1, y1) = self.region_rect(q);
            for y in y0..y1 {
                for x in x0..x1 {
                    total += share * self.map.tiles[self.map.idx(x, y)].meat;
                }
            }
        }
        if total <= 0.0 {
            return 0.0;
        }
        let eaten = want.min(total);
        for &(q, share) in regions {
            let keep = 1.0 - share * eaten / total;
            let (x0, y0, x1, y1) = self.region_rect(q);
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = self.map.idx(x, y);
                    if self.map.tiles[i].meat > 0.0 {
                        self.map.tiles[i].meat *= keep;
                    }
                }
            }
        }
        eaten
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldConfig;
    use crate::presets;
    use crate::Balance;

    /// Every rule is a no-op at 0 (`Balance::carnivore_diet_off`).
    #[test]
    fn rules_off_change_nothing() {
        let bal = Balance::default().carnivore_diet_off();
        let probe = Organism::new(
            0,
            0,
            presets::niche_pack_hunters().founder_genome(1, &mut crate::rng::rng_for(1, 1, 1, 1)),
            crate::organism::Sex::Female,
            crate::math::Vec2::ZERO,
        );
        assert_eq!(gorge_room(&bal, 0.95, 0.5, 0.0), 0.0);
        assert!(!scavenges(&bal));
        assert_eq!(provisioned(&bal, &probe, 0.2, 0.9), 0.2);
        assert_eq!(territory_births(&bal, 0.95, &probe, 256, 4.0, 30.0), None);
        assert_eq!(litter_share(&bal, 95.0, 0.0, 8.0), 1.0);
    }

    /// With the rules on: a hungry, lean mother of a big litter carries
    /// fewer young, a well-fed one all; providers lift breeding condition;
    /// territories cap births (fewer for a tolerant pack); gorging has room
    /// up to the agents' capacity; carrion is left except after predation.
    #[test]
    fn rules_on_follow_the_agents() {
        let bal = Balance::default().carnivore_diet();
        let mut rng = crate::rng::rng_for(1, 1, 1, 1);
        let sex = crate::organism::Sex::Female;
        let pack = Organism::new(
            0,
            0,
            presets::niche_pack_hunters().founder_genome(1, &mut rng),
            sex,
            crate::math::Vec2::ZERO,
        );
        let fox = Organism::new(0, 0, presets::niche_fox().founder_genome(1, &mut rng), sex, crate::math::Vec2::ZERO);
        assert!(litter_share(&bal, 95.0, 0.0, 8.0) < 0.2);
        assert_eq!(litter_share(&bal, 20.0, 0.0, 8.0), 1.0);
        assert_eq!(litter_share(&bal, 95.0, 0.0, 1.0), 1.0);
        assert!(provisioned(&bal, &pack, 0.2, 0.9) > 0.8);
        let (p, f) = (
            territory_births(&bal, 0.95, &pack, 256, 4.0, 30.0).unwrap(),
            territory_births(&bal, 0.65, &fox, 256, 4.0, 30.0).unwrap(),
        );
        assert!(p > 0.0 && f > 0.0 && p < 0.1, "pack {p}, fox {f}");
        assert_eq!(territory_births(&bal, 0.3, &pack, 256, 4.0, 30.0), None);
        assert!((gorge_room(&bal, 0.95, 0.5, 10.0) - 50.0).abs() < 1e-3);

        let config = WorldConfig { seed: 3, width: 32, height: 32, balance: bal, ..Default::default() };
        let mut w = presets::niche_world(config, 1);
        for t in w.map.tiles.iter_mut() {
            t.meat = 0.0;
        }
        w.lod_carrion(0, 2.0, DeathCause::Predation);
        let meat = |w: &World| w.map.tiles.iter().map(|t| t.meat).sum::<f32>();
        assert_eq!(meat(&w), 0.0);
        w.lod_carrion(0, 2.0, DeathCause::Starvation);
        assert!((meat(&w) - bal.meat_per_kg).abs() < 1e-3);
        let eaten = w.lod_scavenge(&[(0, 1.0)], 1.0);
        assert!((eaten - 1.0).abs() < 1e-4 && (meat(&w) - (bal.meat_per_kg - 1.0)).abs() < 1e-3);
    }

    /// In a world of cohorts, the breeding territories hold the pack hunters
    /// to fewer births than without them.
    #[test]
    fn territories_cap_cohort_births() {
        let born = |territory: f32| {
            let mut bal = Balance::default().carnivore_diet();
            bal.breeding_territory = territory;
            let mut config = WorldConfig { seed: 5, width: 64, height: 64, balance: bal, ..Default::default() };
            config.lod.enabled = true;
            let mut w = presets::niche_world(config, 1);
            w.run_days(60);
            w.counters.born.get(&4).copied().unwrap_or(0)
        };
        let (with, without) = (born(12.0), born(0.0));
        assert!(with < without, "pack hunters born: {with} with territories, {without} without");
    }
}
