//! Founder designs and the point budget ("Organism design" in the design doc).
//!
//! A player picks thermoregulation, birth type and offspring count, then
//! spends points on the remaining traits. After launch the design changes only
//! through breeding, mutation and selection.
//!
//! Point costs are calibrated by simulation (`examples/calibrate.rs`; method
//! and evidence in the README): each trait is priced in proportion to the
//! legacy share it buys, so a point buys about as much fitness wherever it is
//! spent. They are all in [`trait_cost`] and the constants below.

use std::fmt;

use rand::Rng;

use crate::genome::{Gene, Genome, PlayerId};

pub const DESIGN_BUDGET: f32 = 100.0;
/// Founding population included in the base price.
pub const BASE_FOUNDERS: u32 = 20;
/// Points per extra founder above [`BASE_FOUNDERS`].
pub const COST_PER_EXTRA_FOUNDER: f32 = 0.7;
/// Points per offspring per birth above one.
pub const COST_PER_EXTRA_OFFSPRING: f32 = 2.5;
/// Points per unit of body size below its default: small bodies fit more
/// breeders on the same food, which the legacy score rewards.
pub const COST_PER_SMALLER_BODY: f32 = 40.0;
/// Points for cold-blooded thermoregulation: a third of the warm-blooded food
/// need is worth more than its cold-weather sluggishness.
pub const COST_COLD_BLOODED: f32 = 20.0;
/// Standard deviation of founder alleles around the design value.
pub const FOUNDER_VARIATION: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thermo {
    Warm,
    Cold,
}

impl Thermo {
    pub fn gene_value(self) -> f32 {
        match self {
            Thermo::Warm => 0.9,
            Thermo::Cold => 0.1,
        }
    }

    pub fn from_gene(v: f32) -> Self {
        if v >= 0.5 {
            Thermo::Warm
        } else {
            Thermo::Cold
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BirthType {
    Live,
    IncubatedEggs,
    /// Eggs laid and left: no parental care.
    AbandonedEggs,
}

impl BirthType {
    pub fn gene_value(self) -> f32 {
        match self {
            BirthType::Live => 0.85,
            BirthType::IncubatedEggs => 0.5,
            BirthType::AbandonedEggs => 0.15,
        }
    }

    pub fn from_gene(v: f32) -> Self {
        if v >= 2.0 / 3.0 {
            BirthType::Live
        } else if v >= 1.0 / 3.0 {
            BirthType::IncubatedEggs
        } else {
            BirthType::AbandonedEggs
        }
    }

    pub fn lays_eggs(self) -> bool {
        self != BirthType::Live
    }
}

pub const MAX_LITTER: u32 = 12;

pub fn litter_from_gene(v: f32) -> u32 {
    1 + (v.clamp(0.0, 1.0) * (MAX_LITTER - 1) as f32).round() as u32
}

pub fn litter_to_gene(n: u32) -> f32 {
    (n.clamp(1, MAX_LITTER) - 1) as f32 / (MAX_LITTER - 1) as f32
}

/// Value a gene takes if the design doesn't set it.
pub fn default_value(g: Gene) -> f32 {
    use Gene::*;
    match g {
        BodySize => 0.3,
        Boldness | Aggression | Wanderlust | GroupTolerance | ForagingRange | PackInstinct | Pickiness => 0.5,
        TolColoration | TolBodySize => 0.5,
        Carnivory => 0.0,
        Vigilance => 0.0,
        _ => 0.0,
    }
}

/// Points per unit of trait value above the default, roughly proportional to
/// the legacy share a unit buys (`examples/calibrate.rs`). Traits that cost
/// more fitness than they bring in every test (armor, fat stores, large eyes
/// and ears) keep a small floor price so they are never free. Behavior, diet
/// and preference genes are free: they are tradeoffs, not upgrades.
pub fn trait_cost(g: Gene) -> f32 {
    use Gene::*;
    match g {
        // Bigger bodies already mean fewer, hungrier breeders (less legacy);
        // smallness is what's charged (`COST_PER_SMALLER_BODY`).
        BodySize => 0.0,
        Muscle => 7.0,
        FatStores => 2.0,
        LegLength => 25.0,
        Claws => 6.0,
        Hooves => 16.0,
        FootWidth => 6.0,
        Tail => 4.0,
        Swimming => 6.0,
        Wings => 24.0,
        Climbing => 5.0,
        Burrowing => 6.0,
        Fur => 3.0,
        Scales => 4.0,
        Armor => 2.0,
        Fangs => 7.0,
        Eyes => 4.0,
        Ears => 4.0,
        Nose => 4.0,
        Camouflage => 5.0,
        Toxins => 18.0,
        Coloration => 5.0,
        Vocal => 5.0,
        // Seasonal strategies.
        Hibernation => 8.0,
        Brumation => 6.0,
        Migration => 8.0,
        Estivation => 6.0,
        Caching => 5.0,
        Torpor => 4.0,
        // Priced like torpor: a cheap add-on with a running cost (food per
        // molt) or a real tradeoff (no breeding outside the window).
        SeasonalCoat => 4.0,
        TimedBreeding => 4.0,
        // Sentry instinct: cheap, since standing watch costs its carrier food.
        Vigilance => 4.0,
        // Set through the design's own fields, never through `traits`.
        Endothermy | BirthMode | LitterSize => 0.0,
        Carnivory | Boldness | Aggression | Wanderlust | GroupTolerance | ForagingRange | PackInstinct | Pickiness
        | PrefColoration | TolColoration | PrefBodySize | TolBodySize => 0.0,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FounderDesign {
    pub name: String,
    pub thermo: Thermo,
    pub birth: BirthType,
    pub litter_size: u32,
    /// Trait values that differ from [`default_value`].
    pub traits: Vec<(Gene, f32)>,
    pub founders: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DesignError {
    OverBudget {
        cost: f32,
        budget: f32,
    },
    ValueOutOfRange(Gene),
    /// Thermoregulation, birth type and litter size have their own fields.
    ReservedGene(Gene),
    LitterSize(u32),
    NoFounders,
}

impl fmt::Display for DesignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DesignError::OverBudget { cost, budget } => {
                write!(f, "design costs {cost:.1} points, budget is {budget:.1}")
            }
            DesignError::ValueOutOfRange(g) => write!(f, "{} must be in [0, 1]", g.label()),
            DesignError::ReservedGene(g) => {
                write!(f, "{} is set by the design's own field", g.label())
            }
            DesignError::LitterSize(n) => write!(f, "litter size {n} not in 1..={MAX_LITTER}"),
            DesignError::NoFounders => write!(f, "a design needs at least two founders"),
        }
    }
}

impl std::error::Error for DesignError {}

impl FounderDesign {
    pub fn new(name: impl Into<String>, thermo: Thermo, birth: BirthType, litter_size: u32) -> Self {
        Self { name: name.into(), thermo, birth, litter_size, traits: Vec::new(), founders: BASE_FOUNDERS }
    }

    pub fn with(mut self, gene: Gene, value: f32) -> Self {
        self.traits.retain(|(g, _)| *g != gene);
        self.traits.push((gene, value));
        self
    }

    pub fn founders(mut self, n: u32) -> Self {
        self.founders = n;
        self
    }

    /// The design's value for a gene (explicit or default).
    pub fn get(&self, gene: Gene) -> f32 {
        self.traits.iter().find(|(g, _)| *g == gene).map_or(default_value(gene), |&(_, v)| v)
    }

    pub fn values(&self) -> [f32; Gene::COUNT] {
        let mut v = [0.0; Gene::COUNT];
        for &g in Gene::ALL {
            v[g.index()] = default_value(g);
        }
        // Unless set, organisms prefer mates that look like their own design.
        v[Gene::PrefColoration.index()] = self.get(Gene::Coloration);
        v[Gene::PrefBodySize.index()] = self.get(Gene::BodySize);
        for &(g, x) in &self.traits {
            v[g.index()] = x;
        }
        v[Gene::Endothermy.index()] = self.thermo.gene_value();
        v[Gene::BirthMode.index()] = self.birth.gene_value();
        v[Gene::LitterSize.index()] = litter_to_gene(self.litter_size);
        v
    }

    pub fn point_cost(&self) -> f32 {
        let v = self.values();
        let traits: f32 = Gene::ALL.iter().map(|&g| (v[g.index()] - default_value(g)).max(0.0) * trait_cost(g)).sum();
        let litter = self.litter_size.saturating_sub(1) as f32 * COST_PER_EXTRA_OFFSPRING;
        let founders = self.founders.saturating_sub(BASE_FOUNDERS) as f32 * COST_PER_EXTRA_FOUNDER;
        let small = (default_value(Gene::BodySize) - v[Gene::BodySize.index()]).max(0.0) * COST_PER_SMALLER_BODY;
        let thermo = if self.thermo == Thermo::Cold { COST_COLD_BLOODED } else { 0.0 };
        traits + litter + founders + small + thermo
    }

    pub fn validate(&self) -> Result<(), DesignError> {
        for &(g, x) in &self.traits {
            if matches!(g, Gene::Endothermy | Gene::BirthMode | Gene::LitterSize) {
                return Err(DesignError::ReservedGene(g));
            }
            if !(0.0..=1.0).contains(&x) {
                return Err(DesignError::ValueOutOfRange(g));
            }
        }
        if !(1..=MAX_LITTER).contains(&self.litter_size) {
            return Err(DesignError::LitterSize(self.litter_size));
        }
        if self.founders < 2 {
            return Err(DesignError::NoFounders);
        }
        let cost = self.point_cost();
        if cost > DESIGN_BUDGET + 1e-3 {
            return Err(DesignError::OverBudget { cost, budget: DESIGN_BUDGET });
        }
        Ok(())
    }

    pub fn founder_genome(&self, owner: PlayerId, rng: &mut impl Rng) -> Genome {
        Genome::founder(&self.values(), owner, FOUNDER_VARIATION, rng)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_design_is_nearly_free() {
        let d = FounderDesign::new("blank", Thermo::Warm, BirthType::Live, 1);
        assert_eq!(d.point_cost(), 0.0);
        assert!(d.validate().is_ok());
    }

    #[test]
    fn maxing_everything_is_over_budget() {
        let mut d = FounderDesign::new("god", Thermo::Warm, BirthType::Live, 12);
        for &g in Gene::ALL {
            if trait_cost(g) > 0.0 {
                d = d.with(g, 1.0);
            }
        }
        assert!(matches!(d.validate(), Err(DesignError::OverBudget { .. })));
    }

    #[test]
    fn reserved_genes_are_rejected() {
        let d = FounderDesign::new("x", Thermo::Cold, BirthType::Live, 2).with(Gene::Endothermy, 1.0);
        assert_eq!(d.validate(), Err(DesignError::ReservedGene(Gene::Endothermy)));
    }

    #[test]
    fn cold_blood_and_small_bodies_cost_points_but_size_up_is_free() {
        let base = FounderDesign::new("b", Thermo::Warm, BirthType::Live, 1);
        let cold = FounderDesign { thermo: Thermo::Cold, ..base.clone() };
        assert_eq!(cold.point_cost(), COST_COLD_BLOODED);
        let small = base.clone().with(Gene::BodySize, 0.1);
        assert!((small.point_cost() - 0.2 * COST_PER_SMALLER_BODY).abs() < 1e-4);
        assert_eq!(base.with(Gene::BodySize, 0.9).point_cost(), 0.0);
    }

    #[test]
    fn gene_round_trips() {
        for n in 1..=MAX_LITTER {
            assert_eq!(litter_from_gene(litter_to_gene(n)), n);
        }
        for b in [BirthType::Live, BirthType::IncubatedEggs, BirthType::AbandonedEggs] {
            assert_eq!(BirthType::from_gene(b.gene_value()), b);
        }
    }
}
