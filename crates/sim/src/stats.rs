//! Phenotype → stats and physical properties ("Stats layer" and "Derived
//! physical properties" in the design doc).
//!
//! Computed once per organism at birth: genes don't change during a life.
//! Situational modifiers (cold, hunger, growth, pregnancy) are applied at
//! runtime in the world tick.

use crate::design::{litter_from_gene, BirthType, MAX_LITTER};
use crate::genome::{Gene, Phenotype};

/// Normalized stats, roughly 0–1.5.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub speed: f32,
    pub strength: f32,
    pub endurance: f32,
    pub agility: f32,
    pub perception: f32,
    pub stealth: f32,
    pub defense: f32,
    /// Hunger-rate multiplier per unit of metabolic mass.
    pub metabolism: f32,
    pub insulation: f32,
}

/// Physical properties that terrain checks compare against tile conditions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// Adult mass in kg.
    pub mass: f32,
    /// Roughly meters at the shoulder.
    pub height: f32,
    pub width: f32,
    pub foot_area: f32,
    /// Weight ÷ foot area, normalized so ~1 is a large narrow-footed animal.
    pub ground_pressure: f32,
    /// 0 sinks, 1 floats.
    pub buoyancy: f32,
    pub wing_loading: f32,
    pub can_fly: bool,
    pub can_glide: bool,
    pub grip: f32,
    /// Hooves slip on ice.
    pub hoof_slip: f32,
    pub swim: f32,
    pub climb: f32,
    pub burrow: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Traits {
    pub stats: Stats,
    pub body: Body,
    pub warm: bool,
    pub birth: BirthType,
    pub litter: u32,
    pub carnivory: f32,
    pub lifespan_days: f32,
    pub maturity_days: f32,
    pub gestation_days: f32,
    pub incubation_days: f32,
    /// Days after a birth before the mother can conceive again.
    pub recovery_days: f32,
    /// Starting growth of a newborn (fraction of adult mass). Big litters
    /// mean small, fragile young.
    pub newborn_growth: f32,
    /// Total litter mass as a fraction of the mother's: what a birth costs.
    pub litter_burden: f32,
    /// Thermoregulation mismatch: fur on a cold-blooded body blocks basking.
    pub fur_mismatch: f32,
}

impl Traits {
    pub fn litter_fraction(&self) -> f32 {
        (self.litter - 1) as f32 / (MAX_LITTER - 1) as f32
    }
}

pub fn derive(p: &Phenotype) -> Traits {
    use Gene::*;
    let warm = p[Endothermy] >= 0.5;
    let birth = BirthType::from_gene(p[BirthMode]);
    let litter = litter_from_gene(p[LitterSize]);
    let litter_frac = (litter - 1) as f32 / (MAX_LITTER - 1) as f32;
    let size = p[BodySize];
    let cold_factor = if warm { 1.0 } else { 1.4 };

    let mass = 0.2 * 10f32.powf(2.5 * size) * (1.0 + 0.3 * p[Muscle] + 0.3 * p[FatStores] + 0.4 * p[Armor]);

    // Thermoregulation mismatch (doc: "Thick fur — cold-blooded can't bask").
    let fur_mismatch = if warm { 0.0 } else { p[Fur] };
    let fur_insulation = if warm { p[Fur] } else { 0.0 };

    // Running costs of expensive organs keep "max everything" genomes in check
    // even after mutation drifts them beyond the founding budget.
    let upkeep = 0.1 * (p[Eyes] + p[Ears] + p[Nose])
        + 0.15 * p[Muscle]
        + 0.1 * (p[Toxins] + p[Armor] + p[Claws] + p[Fangs])
        + 0.2 * p[Wings]
        + 0.05 * p[Fur];

    let stats = Stats {
        speed: (0.45 + 0.45 * p[LegLength] + 0.25 * p[Hooves] + 0.25 * p[Muscle]
            - 0.25 * size
            - 0.25 * p[FatStores]
            - 0.35 * p[Armor])
            .max(0.08),
        strength: 0.15 + 0.6 * size + 0.4 * p[Muscle] + 0.25 * p[Claws] + 0.3 * p[Fangs],
        endurance: (0.4 + 0.3 * p[LegLength] + 0.2 * p[Hooves] + 0.25 * p[FatStores] - 0.2 * size
            + if warm { 0.1 } else { 0.0 })
        .max(0.1),
        agility: (0.35 + 0.3 * p[Claws] + 0.3 * p[Tail] - 0.25 * p[Hooves] - 0.3 * p[Armor] - 0.15 * size)
            .max(0.05),
        perception: 0.15 + 0.3 * p[Eyes] + 0.3 * p[Ears] + 0.3 * p[Nose],
        stealth: (0.25 + 0.6 * p[Camouflage] - 0.35 * size - 0.35 * p[Coloration]).clamp(0.0, 1.0),
        defense: 0.1 + 0.45 * size + 0.3 * p[Scales] + 0.5 * p[Armor] + 0.2 * p[Toxins],
        // Carnivores eat big, infrequent meals: hunger rises more slowly.
        metabolism: (0.6 + 0.3 * p[Muscle]) * if warm { 1.0 } else { 0.35 } * (1.0 + upkeep) * (1.0 - 0.3 * p[Carnivory]),
        insulation: (0.6 * fur_insulation + 0.35 * p[FatStores] - 0.15 * p[Ears]).clamp(0.0, 1.0),
    };

    let foot_area = (0.4 + 0.9 * p[FootWidth] - 0.2 * p[Hooves]).max(0.15);
    let wing_loading = mass / (p[Wings] * 6.0 + 0.01);
    let body = Body {
        mass,
        height: 0.15 + 0.6 * size + 0.4 * p[LegLength],
        width: 0.15 + 0.6 * size + 0.2 * p[Armor],
        foot_area,
        ground_pressure: (mass.cbrt() / 4.0) / foot_area,
        buoyancy: (0.4 + 0.5 * p[FatStores] - 0.5 * p[Armor] - 0.2 * p[Muscle]).clamp(0.0, 1.0),
        wing_loading,
        can_fly: p[Wings] > 0.5 && wing_loading < 2.0,
        can_glide: p[Wings] > 0.25 && wing_loading < 6.0,
        grip: (0.3 + 0.6 * p[Claws] + 0.2 * p[Climbing]).clamp(0.0, 1.0),
        hoof_slip: p[Hooves],
        swim: (p[Swimming] + 0.3 * p[FootWidth]).clamp(0.0, 1.0),
        climb: p[Climbing],
        burrow: p[Burrowing],
    };

    let newborn_growth = 0.05 + 0.2 * (1.0 - litter_frac);
    let gestation_days = match birth {
        BirthType::Live => 8.0 * (0.6 + size) * (1.0 + 0.3 * litter_frac),
        _ => 2.0,
    };

    Traits {
        stats,
        body,
        warm,
        birth,
        litter,
        carnivory: p[Carnivory],
        lifespan_days: 240.0 * (0.6 + size) * cold_factor,
        maturity_days: 15.0 * (0.6 + size) * cold_factor * (1.0 + 0.6 * (1.0 - litter_frac)),
        gestation_days,
        incubation_days: 6.0 * (0.6 + size) * cold_factor,
        recovery_days: 3.0 + 6.0 * litter_frac,
        newborn_growth,
        litter_burden: litter as f32 * newborn_growth,
        fur_mismatch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{FounderDesign, Thermo};
    use crate::genome::Genome;

    fn traits_of(d: &FounderDesign) -> Traits {
        derive(&Genome::uniform(&d.values(), 1).phenotype())
    }

    #[test]
    fn size_trades_speed_for_strength() {
        let small = traits_of(&FounderDesign::new("s", Thermo::Warm, BirthType::Live, 2).with(Gene::BodySize, 0.1));
        let big = traits_of(&FounderDesign::new("b", Thermo::Warm, BirthType::Live, 2).with(Gene::BodySize, 0.9));
        assert!(big.stats.strength > small.stats.strength);
        assert!(big.stats.speed < small.stats.speed);
        assert!(big.body.mass > 10.0 * small.body.mass);
    }

    #[test]
    fn cold_blooded_burn_less_and_fur_is_a_mismatch() {
        let warm = traits_of(&FounderDesign::new("w", Thermo::Warm, BirthType::Live, 2).with(Gene::Fur, 0.8));
        let cold = traits_of(&FounderDesign::new("c", Thermo::Cold, BirthType::Live, 2).with(Gene::Fur, 0.8));
        assert!(cold.stats.metabolism < warm.stats.metabolism);
        assert!(cold.stats.insulation < warm.stats.insulation);
        assert!(cold.fur_mismatch > 0.0 && warm.fur_mismatch == 0.0);
    }

    #[test]
    fn wings_cap_flying_weight() {
        let light = traits_of(&FounderDesign::new("l", Thermo::Warm, BirthType::IncubatedEggs, 2).with(Gene::Wings, 0.9).with(Gene::BodySize, 0.15));
        let heavy = traits_of(&FounderDesign::new("h", Thermo::Warm, BirthType::IncubatedEggs, 2).with(Gene::Wings, 0.9).with(Gene::BodySize, 0.9));
        assert!(light.body.can_fly);
        assert!(!heavy.body.can_fly);
    }
}
