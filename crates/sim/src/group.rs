//! Critical-mass group traits ("Critical-mass group traits" in the design doc).
//!
//! Each trait has a carrier test (a combination of designed traits) and an
//! eligible population (who counts in the denominator). Once per simulated
//! day the world measures carrier frequency among breeding adults per region
//! and turns it into a benefit with an S-shaped curve. The benefit is also
//! scaled down when there are fewer carriers than the trait needs.
//!
//! Implemented: cooperative hunting, warning coloration, herd defense, alarm
//! calls, shared burrows, sentries, and the seasonal-strategy combinations
//! communal dens, group navigation and shared caches (their effects live in
//! `world::seasonal`).

use crate::genome::{Gene, Phenotype};
use crate::math::Vec2;
use crate::stats::STRATEGY_THRESHOLD;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupTrait {
    CooperativeHunting,
    WarningColoration,
    HerdDefense,
    AlarmCalls,
    SharedBurrows,
    Sentries,
    CommunalDens,
    GroupNavigation,
    SharedCaches,
}

pub const GROUP_TRAIT_COUNT: usize = 9;

impl GroupTrait {
    pub const ALL: [GroupTrait; GROUP_TRAIT_COUNT] = [
        GroupTrait::CooperativeHunting,
        GroupTrait::WarningColoration,
        GroupTrait::HerdDefense,
        GroupTrait::AlarmCalls,
        GroupTrait::SharedBurrows,
        GroupTrait::Sentries,
        GroupTrait::CommunalDens,
        GroupTrait::GroupNavigation,
        GroupTrait::SharedCaches,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GroupTrait::CooperativeHunting => "cooperative hunting",
            GroupTrait::WarningColoration => "warning coloration",
            GroupTrait::HerdDefense => "herd defense",
            GroupTrait::AlarmCalls => "alarm calls",
            GroupTrait::SharedBurrows => "shared burrows",
            GroupTrait::Sentries => "sentries",
            GroupTrait::CommunalDens => "communal dens",
            GroupTrait::GroupNavigation => "group navigation",
            GroupTrait::SharedCaches => "shared caches",
        }
    }

    pub fn is_carrier(self, p: &Phenotype) -> bool {
        use Gene::*;
        match self {
            GroupTrait::CooperativeHunting => p[Carnivory] > 0.6 && p[PackInstinct] > 0.5,
            GroupTrait::WarningColoration => p[Toxins] > 0.5 && p[Coloration] > 0.6,
            GroupTrait::HerdDefense => p[Carnivory] < 0.4 && p[BodySize] > 0.45 && p[GroupTolerance] > 0.5,
            GroupTrait::AlarmCalls => p[Ears] > 0.5 && p[Vocal] > 0.5,
            GroupTrait::SharedBurrows => is_digger(p) && p[GroupTolerance] > 0.5,
            GroupTrait::Sentries => p[GroupTolerance] > 0.6 && p[Vigilance] > 0.5,
            GroupTrait::CommunalDens => is_winter_sleeper(p) && p[GroupTolerance] > 0.5,
            GroupTrait::GroupNavigation => p[Migration] >= STRATEGY_THRESHOLD && p[GroupTolerance] > 0.5,
            GroupTrait::SharedCaches => is_group_forager(p) && p[Caching] >= STRATEGY_THRESHOLD,
        }
    }

    /// Who counts in the frequency denominator. Flooding an area with eligible
    /// non-carriers dilutes the trait.
    pub fn is_eligible(self, p: &Phenotype) -> bool {
        use Gene::*;
        match self {
            GroupTrait::CooperativeHunting => p[Carnivory] > 0.6,
            // Predators learn from everything bright they try to eat.
            GroupTrait::WarningColoration => p[Coloration] > 0.6,
            GroupTrait::HerdDefense => p[Carnivory] < 0.4,
            GroupTrait::AlarmCalls => true,
            // Solitary diggers share nothing and crowd out the colony.
            GroupTrait::SharedBurrows => is_digger(p),
            // Group species: herd members that never stand watch dilute it.
            GroupTrait::Sentries => p[GroupTolerance] > 0.6,
            // Solitary sleepers share no heat and crowd out the den.
            GroupTrait::CommunalDens => is_winter_sleeper(p),
            // Migrants that go their own way scatter the group.
            GroupTrait::GroupNavigation => p[Migration] >= STRATEGY_THRESHOLD,
            // Free riders: plant-eating group members that never cache but
            // may draw from the shared cache.
            GroupTrait::SharedCaches => is_group_forager(p),
        }
    }

    pub fn threshold(self) -> f32 {
        match self {
            GroupTrait::CooperativeHunting => 0.5,
            GroupTrait::WarningColoration => 0.6,
            GroupTrait::HerdDefense => 0.5,
            GroupTrait::AlarmCalls => 0.4,
            GroupTrait::SharedBurrows => 0.5,
            GroupTrait::Sentries => 0.4,
            GroupTrait::CommunalDens => 0.5,
            GroupTrait::GroupNavigation => 0.5,
            GroupTrait::SharedCaches => 0.5,
        }
    }

    /// Carriers needed in the region for the full benefit.
    pub fn min_carriers(self) -> u32 {
        match self {
            GroupTrait::CooperativeHunting => 4,
            GroupTrait::WarningColoration => 6,
            GroupTrait::HerdDefense => 5,
            GroupTrait::AlarmCalls => 3,
            GroupTrait::SharedBurrows => 4,
            GroupTrait::Sentries => 4,
            GroupTrait::CommunalDens => 4,
            GroupTrait::GroupNavigation => 4,
            GroupTrait::SharedCaches => 4,
        }
    }
}

/// Can dig (claws or burrowing): the first half of the shared-burrow test.
fn is_digger(p: &Phenotype) -> bool {
    p[Gene::Claws] > 0.5 || p[Gene::Burrowing] > 0.4
}

/// Expresses hibernation or brumation: the first half of the communal-den test.
fn is_winter_sleeper(p: &Phenotype) -> bool {
    p[Gene::Hibernation] >= STRATEGY_THRESHOLD || p[Gene::Brumation] >= STRATEGY_THRESHOLD
}

/// A group member that can eat cached plant food (`seasonal::CACHE_DIET`):
/// who may draw from a shared cache.
pub(crate) fn is_group_forager(p: &Phenotype) -> bool {
    p[Gene::GroupTolerance] > 0.5 && p[Gene::Carnivory] <= 0.7
}

/// `1 / (1 + e^(-k (freq - threshold)))`.
pub fn logistic(freq: f32, threshold: f32, k: f32) -> f32 {
    1.0 / (1.0 + (-k * (freq - threshold)).exp())
}

const STEEPNESS: f32 = 12.0;

/// Regional frequencies and benefits, recomputed daily.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GroupField {
    pub region_size: usize,
    pub cols: usize,
    pub rows: usize,
    pub frequency: Vec<[f32; GROUP_TRAIT_COUNT]>,
    pub benefit: Vec<[f32; GROUP_TRAIT_COUNT]>,
}

impl GroupField {
    pub fn new(width: usize, height: usize, region_size: usize) -> Self {
        let cols = width.div_ceil(region_size);
        let rows = height.div_ceil(region_size);
        Self {
            region_size,
            cols,
            rows,
            frequency: vec![[0.0; GROUP_TRAIT_COUNT]; cols * rows],
            benefit: vec![[0.0; GROUP_TRAIT_COUNT]; cols * rows],
        }
    }

    pub(crate) fn region(&self, p: Vec2) -> usize {
        let c = ((p.x.max(0.0) as usize) / self.region_size).min(self.cols - 1);
        let r = ((p.y.max(0.0) as usize) / self.region_size).min(self.rows - 1);
        r * self.cols + c
    }

    /// Recompute from `(position, phenotype)` of every breeding adult.
    pub fn update<'a>(&mut self, adults: impl Iterator<Item = (Vec2, &'a Phenotype)>) {
        let n = self.cols * self.rows;
        let mut carriers = vec![[0u32; GROUP_TRAIT_COUNT]; n];
        let mut eligible = vec![[0u32; GROUP_TRAIT_COUNT]; n];
        for (pos, p) in adults {
            let r = self.region(pos);
            for (k, t) in GroupTrait::ALL.iter().enumerate() {
                if t.is_eligible(p) {
                    eligible[r][k] += 1;
                    if t.is_carrier(p) {
                        carriers[r][k] += 1;
                    }
                }
            }
        }
        for r in 0..n {
            for (k, t) in GroupTrait::ALL.iter().enumerate() {
                let (c, e) = (carriers[r][k], eligible[r][k]);
                let freq = if e == 0 { 0.0 } else { c as f32 / e as f32 };
                let mass = (c as f32 / t.min_carriers() as f32).min(1.0);
                self.frequency[r][k] = freq;
                self.benefit[r][k] = if c == 0 { 0.0 } else { logistic(freq, t.threshold(), STEEPNESS) * mass };
            }
        }
    }

    pub fn benefit(&self, t: GroupTrait, p: Vec2) -> f32 {
        self.benefit[self.region(p)][t as usize]
    }

    pub fn frequency(&self, t: GroupTrait, p: Vec2) -> f32 {
        self.frequency[self.region(p)][t as usize]
    }

    /// Number of regions where the trait is above half benefit.
    pub fn active_regions(&self, t: GroupTrait) -> usize {
        self.benefit.iter().filter(|b| b[t as usize] > 0.5).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{BirthType, FounderDesign, Thermo};
    use crate::genome::Genome;

    fn pheno(d: FounderDesign) -> Phenotype {
        Genome::uniform(&d.values(), 1).phenotype()
    }

    #[test]
    fn logistic_is_half_at_threshold() {
        assert!((logistic(0.5, 0.5, 10.0) - 0.5).abs() < 1e-6);
        assert!(logistic(0.9, 0.5, 10.0) > 0.95);
    }

    #[test]
    fn dilution_by_non_carriers_drops_the_benefit() {
        let wolf = pheno(
            FounderDesign::new("w", Thermo::Warm, BirthType::Live, 3)
                .with(Gene::Carnivory, 0.9)
                .with(Gene::PackInstinct, 0.9),
        );
        let loner = pheno(FounderDesign::new("l", Thermo::Warm, BirthType::Live, 3).with(Gene::Carnivory, 0.9).with(Gene::PackInstinct, 0.1));
        let at = Vec2::new(1.0, 1.0);
        let mut field = GroupField::new(32, 32, 16);
        let pack: Vec<_> = (0..6).map(|_| (at, &wolf)).collect();
        field.update(pack.iter().cloned());
        let strong = field.benefit(GroupTrait::CooperativeHunting, at);
        let flooded: Vec<_> = pack.iter().cloned().chain((0..12).map(|_| (at, &loner))).collect();
        field.update(flooded.into_iter());
        let weak = field.benefit(GroupTrait::CooperativeHunting, at);
        assert!(strong > 0.9, "strong {strong}");
        assert!(weak < 0.2, "weak {weak}");
    }

    /// Benefit of `t` in a region of `carriers` and `others` (all adults).
    fn benefit_of(t: GroupTrait, carrier: &Phenotype, n_carriers: usize, other: &Phenotype, n_others: usize) -> f32 {
        let at = Vec2::new(1.0, 1.0);
        let mut field = GroupField::new(32, 32, 16);
        let adults: Vec<_> = (0..n_carriers).map(|_| (at, carrier)).chain((0..n_others).map(|_| (at, other))).collect();
        field.update(adults.into_iter());
        field.benefit(t, at)
    }

    fn digger(group_tolerance: f32) -> Phenotype {
        pheno(
            FounderDesign::new("d", Thermo::Warm, BirthType::Live, 3)
                .with(Gene::Burrowing, 0.6)
                .with(Gene::GroupTolerance, group_tolerance),
        )
    }

    #[test]
    fn shared_burrows_need_diggers_that_tolerate_each_other() {
        let t = GroupTrait::SharedBurrows;
        let (social, solitary) = (digger(0.7), digger(0.2));
        let clawed = pheno(FounderDesign::new("c", Thermo::Warm, BirthType::Live, 3).with(Gene::Claws, 0.7).with(Gene::GroupTolerance, 0.7));
        let surface = pheno(FounderDesign::new("s", Thermo::Warm, BirthType::Live, 3).with(Gene::GroupTolerance, 0.9));
        assert!(t.is_carrier(&social) && t.is_carrier(&clawed));
        assert!(!t.is_carrier(&solitary) && t.is_eligible(&solitary));
        // A social animal that can't dig neither carries nor dilutes it.
        assert!(!t.is_carrier(&surface) && !t.is_eligible(&surface));
        let colony = benefit_of(t, &social, 6, &surface, 20);
        assert!(colony > 0.9, "colony {colony}");
        // Below threshold (2 of 6) and too few carriers: little benefit.
        assert!(benefit_of(t, &social, 2, &solitary, 4) < 0.2);
        // Solitary diggers moving in dilute it.
        let diluted = benefit_of(t, &social, 6, &solitary, 12);
        assert!(diluted < 0.2, "diluted {diluted}");
    }

    #[test]
    fn sentries_need_vigilant_group_members() {
        let t = GroupTrait::Sentries;
        let herd = |vigilance: f32, gt: f32| {
            pheno(FounderDesign::new("h", Thermo::Warm, BirthType::Live, 1).with(Gene::Vigilance, vigilance).with(Gene::GroupTolerance, gt))
        };
        let (sentry, member, loner) = (herd(0.8, 0.9), herd(0.0, 0.9), herd(0.8, 0.3));
        assert!(t.is_carrier(&sentry));
        assert!(!t.is_carrier(&member) && t.is_eligible(&member));
        // Vigilance without a group to watch over is nothing.
        assert!(!t.is_carrier(&loner) && !t.is_eligible(&loner));
        assert!(benefit_of(t, &sentry, 6, &loner, 30) > 0.9);
        assert!(benefit_of(t, &sentry, 6, &member, 4) > 0.8);
        // Herd members that never stand watch dilute it.
        let diluted = benefit_of(t, &sentry, 6, &member, 24);
        assert!(diluted < 0.15, "diluted {diluted}");
    }

    fn seasonal(strategy: Gene, gt: f32) -> Phenotype {
        pheno(FounderDesign::new("s", Thermo::Warm, BirthType::Live, 2).with(strategy, 0.8).with(Gene::GroupTolerance, gt))
    }

    #[test]
    fn communal_dens_need_sleepers_that_tolerate_each_other() {
        let t = GroupTrait::CommunalDens;
        let (social, solitary) = (seasonal(Gene::Hibernation, 0.8), seasonal(Gene::Hibernation, 0.2));
        let brumator =
            pheno(FounderDesign::new("b", Thermo::Cold, BirthType::Live, 2).with(Gene::Brumation, 0.8).with(Gene::GroupTolerance, 0.8));
        let awake = pheno(FounderDesign::new("a", Thermo::Warm, BirthType::Live, 2).with(Gene::GroupTolerance, 0.9));
        assert!(t.is_carrier(&social) && t.is_carrier(&brumator));
        assert!(!t.is_carrier(&solitary) && t.is_eligible(&solitary));
        assert!(!t.is_carrier(&awake) && !t.is_eligible(&awake));
        assert!(benefit_of(t, &social, 6, &awake, 20) > 0.9);
        assert!(benefit_of(t, &social, 2, &solitary, 4) < 0.2);
        assert!(benefit_of(t, &social, 6, &solitary, 12) < 0.2);
    }

    #[test]
    fn group_navigation_needs_migrants_that_keep_together() {
        let t = GroupTrait::GroupNavigation;
        let (social, solitary) = (seasonal(Gene::Migration, 0.8), seasonal(Gene::Migration, 0.2));
        let resident = seasonal(Gene::Hibernation, 0.9);
        assert!(t.is_carrier(&social) && !t.is_carrier(&solitary) && t.is_eligible(&solitary));
        assert!(!t.is_eligible(&resident));
        assert!(benefit_of(t, &social, 6, &resident, 20) > 0.9);
        assert!(benefit_of(t, &social, 6, &solitary, 12) < 0.2);
    }

    /// Group members that never cache dilute shared caches: free riders.
    #[test]
    fn shared_caches_are_diluted_by_free_riders() {
        let t = GroupTrait::SharedCaches;
        let cacher = seasonal(Gene::Caching, 0.8);
        let free_rider = pheno(FounderDesign::new("f", Thermo::Warm, BirthType::Live, 2).with(Gene::GroupTolerance, 0.8));
        let loner = seasonal(Gene::Caching, 0.2);
        let hunter = pheno(
            FounderDesign::new("h", Thermo::Warm, BirthType::Live, 2)
                .with(Gene::Caching, 0.8)
                .with(Gene::GroupTolerance, 0.8)
                .with(Gene::Carnivory, 0.9),
        );
        assert!(t.is_carrier(&cacher));
        assert!(!t.is_carrier(&free_rider) && t.is_eligible(&free_rider));
        // A solitary hoarder shares nothing and takes nothing; a carnivore
        // can't eat cached plants.
        assert!(!t.is_eligible(&loner) && !t.is_eligible(&hunter));
        assert!(benefit_of(t, &cacher, 6, &loner, 30) > 0.9);
        assert!(benefit_of(t, &cacher, 6, &free_rider, 3) > 0.8);
        let flooded = benefit_of(t, &cacher, 6, &free_rider, 18);
        assert!(flooded < 0.1, "flooded {flooded}");
    }
}
