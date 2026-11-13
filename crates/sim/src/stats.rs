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
    /// Seasonal strategy ("Seasons and biomes" in the design doc).
    pub seasonal: Seasonal,
}

/// Expression above which a seasonal-strategy gene switches its strategy on.
/// Above it the strategy works better the higher the expression, so it can
/// evolve gradually once it appears.
pub const STRATEGY_THRESHOLD: f32 = 0.5;

/// How well a winter-dormancy form suits the body (the doc's thermoregulation
/// affinity table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinterForm {
    /// Warm-blooded hibernation: sleeps through winter and wakes in spring by
    /// heating itself.
    Hibernation,
    /// Cold-blooded brumation: dormant while it is cold, wakes with the
    /// weather.
    Brumation,
    /// Mismatch: a cold-blooded hibernator can't heat itself to wake. It wakes
    /// only with the weather, and may never wake (`Balance::dormancy_mismatch`).
    ColdHibernator,
    /// Mismatch: a warm-blooded brumator burns fat while dormant
    /// (`Balance::brumation_mismatch`).
    WarmBrumator,
}

impl WinterForm {
    /// Wakes in spring by heating itself, whatever the weather.
    pub fn self_waking(self) -> bool {
        matches!(self, WinterForm::Hibernation)
    }
}

/// Which seasonal strategies are expressed. Each strength is 0 when the gene
/// is below [`STRATEGY_THRESHOLD`], otherwise its efficiency in (0, 1]:
/// `(expression - threshold) / (1 - threshold)`, floored at 0.1 so a
/// just-expressed strategy already does something.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seasonal {
    /// Winter dormancy, if any: the form that suits the body when both genes
    /// are expressed, else the expressed one.
    pub winter: Option<(WinterForm, f32)>,
    pub migration: f32,
    pub estivation: f32,
    pub caching: f32,
    pub torpor: f32,
}

impl Seasonal {
    pub const NONE: Seasonal = Seasonal { winter: None, migration: 0.0, estivation: 0.0, caching: 0.0, torpor: 0.0 };

    pub fn from_phenotype(p: &Phenotype, warm: bool) -> Seasonal {
        let strength = |g: Gene| {
            let x = p[g];
            if x < STRATEGY_THRESHOLD {
                0.0
            } else {
                ((x - STRATEGY_THRESHOLD) / (1.0 - STRATEGY_THRESHOLD)).max(0.1)
            }
        };
        let (hib, brum) = (strength(Gene::Hibernation), strength(Gene::Brumation));
        let winter = match (warm, hib > 0.0, brum > 0.0) {
            (true, true, _) => Some((WinterForm::Hibernation, hib)),
            (true, false, true) => Some((WinterForm::WarmBrumator, brum)),
            (false, _, true) => Some((WinterForm::Brumation, brum)),
            (false, true, false) => Some((WinterForm::ColdHibernator, hib)),
            _ => None,
        };
        Seasonal {
            winter,
            migration: strength(Gene::Migration),
            estivation: strength(Gene::Estivation),
            caching: strength(Gene::Caching),
            torpor: strength(Gene::Torpor),
        }
    }

    /// Does it fatten up in `season` ahead of a dormancy?
    pub fn fattens_in(&self, season: crate::terrain::Season) -> bool {
        use crate::terrain::Season::*;
        (self.winter.is_some() && season == Autumn) || (self.estivation > 0.0 && season == Spring)
    }
}

/// What keeping both guts costs a mixed diet: `1 - tax * (4c(1 - c))^3`
/// for carnivory `c` and `tax` = `Balance::generalist_tax`. The full tax
/// falls on an even omnivore (carnivory 0.5: a quarter of each food at the
/// default 0.5 instead of half); it fades fast toward either specialist
/// (carnivory 0.1 keeps 98%, 0.85 keeps 93%, 0.3 or 0.7 keeps 70%), so
/// specializing pays and herbivores and carnivores barely notice.
pub fn generalist_factor(carnivory: f32, tax: f32) -> f32 {
    let mix = 4.0 * carnivory * (1.0 - carnivory);
    1.0 - tax * mix.clamp(0.0, 1.0).powi(3)
}

impl Traits {
    /// Share of plant food's value the gut extracts: `(1 - carnivory)`
    /// times the generalist factor (see [`generalist_factor`]).
    pub fn plant_efficiency(&self, tax: f32) -> f32 {
        (1.0 - self.carnivory).clamp(0.0, 1.0) * generalist_factor(self.carnivory, tax)
    }

    /// Share of meat's value the gut extracts: `carnivory` times the
    /// generalist factor (see [`generalist_factor`]).
    pub fn meat_efficiency(&self, tax: f32) -> f32 {
        self.carnivory.clamp(0.0, 1.0) * generalist_factor(self.carnivory, tax)
    }

    pub fn litter_fraction(&self) -> f32 {
        (self.litter - 1) as f32 / (MAX_LITTER - 1) as f32
    }

    /// Pace of life, 0–1: small bodies with big litters live fast and die
    /// young (r-selection); large bodies with few young are sturdy. Squared
    /// so it falls on the extreme fast breeders (frogs ~0.5, voles ~0.16,
    /// predators ~0.03). Scales adult background mortality
    /// (`Balance::adult_mortality`) and, square-rooted, density-dependent
    /// breeding (`Balance::breeding_density`).
    pub fn pace_of_life(&self, body_size: f32) -> f32 {
        (self.litter_fraction() * (1.0 - body_size).clamp(0.0, 1.0)).powi(2)
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
        metabolism: (0.6 + 0.3 * p[Muscle]) * if warm { 1.0 } else { 0.35 } * (1.0 + upkeep),
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
        seasonal: Seasonal::from_phenotype(p, warm),
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

    /// Specialists keep (nearly) full value of their food; a generalist at
    /// carnivory 0.5 gets clearly less than half of each once taxed.
    #[test]
    fn diet_efficiency_taxes_generalists() {
        let eff = |c: f32, tax: f32| {
            let t = traits_of(&FounderDesign::new("d", Thermo::Warm, BirthType::Live, 2).with(Gene::Carnivory, c));
            (t.plant_efficiency(tax), t.meat_efficiency(tax))
        };
        // Untaxed: the old linear rule.
        let (pl, me) = eff(0.5, 0.0);
        assert!((pl - 0.5).abs() < 1e-6 && (me - 0.5).abs() < 1e-6);
        let tax = crate::Balance::default().generalist_tax;
        assert!(tax > 0.0);
        let (pl, me) = eff(0.5, tax);
        assert!((pl - 0.5 * (1.0 - tax)).abs() < 1e-5 && pl < 0.4 && (me - pl).abs() < 1e-6, "{pl} {me}");
        assert_eq!(eff(0.0, tax).0, 1.0);
        assert_eq!(eff(1.0, tax).1, 1.0);
        // Near-specialists lose little.
        assert!(eff(0.1, tax).0 > 0.95 * 0.9);
        assert!(eff(0.85, tax).1 > 0.93 * 0.85);
        // Specializing pays: total extraction is lowest in the middle.
        let total = |c| eff(c, tax).0 + eff(c, tax).1;
        assert!(total(0.5) < total(0.3) && total(0.3) < total(0.1) && total(0.5) < total(0.85));
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
    fn small_big_litter_bodies_live_fast() {
        let pace = |d: &FounderDesign| traits_of(d).pace_of_life(d.values()[Gene::BodySize.index()]);
        let frog = FounderDesign::new("f", Thermo::Cold, BirthType::AbandonedEggs, 10).with(Gene::BodySize, 0.12);
        let grazer = FounderDesign::new("g", Thermo::Warm, BirthType::Live, 1).with(Gene::BodySize, 0.6);
        let fox = FounderDesign::new("x", Thermo::Warm, BirthType::Live, 4).with(Gene::BodySize, 0.35);
        assert!(pace(&frog) > 0.4);
        assert!(pace(&fox) < 0.05);
        assert_eq!(pace(&grazer), 0.0);
    }

    #[test]
    fn winter_dormancy_form_follows_the_body() {
        let form = |thermo, g: Gene| {
            traits_of(&FounderDesign::new("d", thermo, BirthType::Live, 2).with(g, 0.9)).seasonal.winter.map(|w| w.0)
        };
        assert_eq!(form(Thermo::Warm, Gene::Hibernation), Some(WinterForm::Hibernation));
        assert_eq!(form(Thermo::Cold, Gene::Brumation), Some(WinterForm::Brumation));
        assert_eq!(form(Thermo::Cold, Gene::Hibernation), Some(WinterForm::ColdHibernator));
        assert_eq!(form(Thermo::Warm, Gene::Brumation), Some(WinterForm::WarmBrumator));
        // Below the threshold a strategy is not expressed at all.
        let weak = traits_of(&FounderDesign::new("w", Thermo::Warm, BirthType::Live, 2).with(Gene::Migration, 0.45));
        assert_eq!(weak.seasonal, Seasonal::NONE);
        let strong = traits_of(&FounderDesign::new("s", Thermo::Warm, BirthType::Live, 2).with(Gene::Migration, 1.0));
        assert_eq!(strong.seasonal.migration, 1.0);
    }

    #[test]
    fn wings_cap_flying_weight() {
        let light = traits_of(&FounderDesign::new("l", Thermo::Warm, BirthType::IncubatedEggs, 2).with(Gene::Wings, 0.9).with(Gene::BodySize, 0.15));
        let heavy = traits_of(&FounderDesign::new("h", Thermo::Warm, BirthType::IncubatedEggs, 2).with(Gene::Wings, 0.9).with(Gene::BodySize, 0.9));
        assert!(light.body.can_fly);
        assert!(!heavy.body.can_fly);
    }
}
