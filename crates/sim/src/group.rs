//! Critical-mass group traits ("Critical-mass group traits" in the design doc).
//!
//! Each trait has a carrier test (a combination of designed traits) and an
//! eligible population (who counts in the denominator). Once per simulated
//! day the world measures carrier frequency among breeding adults per region
//! and turns it into a benefit with an S-shaped curve. The benefit is also
//! scaled down when there are fewer carriers than the trait needs.
//!
//! Implemented: cooperative hunting, warning coloration, herd defense, alarm
//! calls. The rest of the doc's table are on the roadmap.

use crate::genome::{Gene, Phenotype};
use crate::math::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupTrait {
    CooperativeHunting,
    WarningColoration,
    HerdDefense,
    AlarmCalls,
}

pub const GROUP_TRAIT_COUNT: usize = 4;

impl GroupTrait {
    pub const ALL: [GroupTrait; GROUP_TRAIT_COUNT] = [
        GroupTrait::CooperativeHunting,
        GroupTrait::WarningColoration,
        GroupTrait::HerdDefense,
        GroupTrait::AlarmCalls,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GroupTrait::CooperativeHunting => "cooperative hunting",
            GroupTrait::WarningColoration => "warning coloration",
            GroupTrait::HerdDefense => "herd defense",
            GroupTrait::AlarmCalls => "alarm calls",
        }
    }

    pub fn is_carrier(self, p: &Phenotype) -> bool {
        use Gene::*;
        match self {
            GroupTrait::CooperativeHunting => p[Carnivory] > 0.6 && p[PackInstinct] > 0.5,
            GroupTrait::WarningColoration => p[Toxins] > 0.5 && p[Coloration] > 0.6,
            GroupTrait::HerdDefense => p[Carnivory] < 0.4 && p[BodySize] > 0.45 && p[GroupTolerance] > 0.5,
            GroupTrait::AlarmCalls => p[Ears] > 0.5 && p[Vocal] > 0.5,
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
        }
    }

    pub fn threshold(self) -> f32 {
        match self {
            GroupTrait::CooperativeHunting => 0.5,
            GroupTrait::WarningColoration => 0.6,
            GroupTrait::HerdDefense => 0.5,
            GroupTrait::AlarmCalls => 0.4,
        }
    }

    /// Carriers needed in the region for the full benefit.
    pub fn min_carriers(self) -> u32 {
        match self {
            GroupTrait::CooperativeHunting => 4,
            GroupTrait::WarningColoration => 6,
            GroupTrait::HerdDefense => 5,
            GroupTrait::AlarmCalls => 3,
        }
    }
}

/// `1 / (1 + e^(-k (freq - threshold)))`.
pub fn logistic(freq: f32, threshold: f32, k: f32) -> f32 {
    1.0 / (1.0 + (-k * (freq - threshold)).exp())
}

const STEEPNESS: f32 = 12.0;

/// Regional frequencies and benefits, recomputed daily.
#[derive(Clone, Debug)]
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

    fn region(&self, p: Vec2) -> usize {
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
}
