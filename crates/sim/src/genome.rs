//! Diploid genomes with origin-tagged alleles.
//!
//! Every gene is a locus with two alleles. Each allele carries a value in
//! `[0, 1]` and the ID of the player it originated with (see "Scoring:
//! genetic legacy" in the design doc):
//!
//! 1. Founder alleles carry the designing player's ID.
//! 2. A new mutation carries the ID of whoever owns the organism it appears in.
//! 3. Tags never change; they pass through any mating.
//!
//! Expression is additive (the mean of the two alleles) for every gene in this
//! prototype. Dominance and hidden recessives are on the roadmap.

use std::collections::BTreeMap;
use std::ops::Index;

use rand::Rng;

use crate::rng::normal;

pub type PlayerId = u32;

/// Owner ID of the wild ecosystem.
pub const WILD: PlayerId = 0;

macro_rules! define_genes {
    ($( $(#[$m:meta])* $name:ident => $label:literal ),* $(,)?) => {
        /// Every heritable locus. Physical, behavioral and mate-preference
        /// traits are all genes, so all of them evolve.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Gene { $( $(#[$m])* $name ),* }

        impl Gene {
            pub const ALL: &'static [Gene] = &[$(Gene::$name),*];
            pub const COUNT: usize = Gene::ALL.len();

            pub fn label(self) -> &'static str {
                match self { $(Gene::$name => $label),* }
            }

            pub fn index(self) -> usize {
                self as usize
            }
        }
    };
}

define_genes! {
    // Fundamental design choices (expressed as thresholds; see `stats`).
    /// ≥ 0.5 = warm-blooded.
    Endothermy => "endothermy",
    /// < 1/3 abandoned eggs, < 2/3 incubated eggs, otherwise live birth.
    BirthMode => "birth mode",
    /// Maps to 1–12 offspring per birth.
    LitterSize => "litter size",
    // Body
    BodySize => "body size",
    Muscle => "muscle",
    FatStores => "fat stores",
    // Limbs
    LegLength => "leg length",
    Claws => "claws",
    Hooves => "hooves",
    /// Wide paws / webbed feet.
    FootWidth => "foot width",
    Tail => "tail",
    // Locomotion
    Swimming => "swimming",
    Wings => "wings",
    Climbing => "climbing",
    Burrowing => "burrowing",
    // Covering
    Fur => "fur",
    Scales => "scales",
    Armor => "armor plates",
    // Head
    Fangs => "fangs",
    Eyes => "large eyes",
    Ears => "large ears",
    Nose => "keen nose",
    // Other
    /// 0 = herbivore, 0.5 = omnivore/scavenger, 1 = carnivore.
    Carnivory => "carnivory",
    Camouflage => "camouflage",
    Toxins => "toxins",
    /// Bright display coloration: a mate signal that costs stealth.
    Coloration => "coloration",
    Vocal => "vocal ability",
    // Instinct (behavior weights)
    Boldness => "boldness",
    Aggression => "aggression",
    Wanderlust => "wanderlust",
    GroupTolerance => "group tolerance",
    ForagingRange => "foraging range",
    PackInstinct => "pack instinct",
    Pickiness => "pickiness",
    // Mate preference: preferred value and tolerance per display trait.
    PrefColoration => "preferred coloration",
    TolColoration => "coloration tolerance",
    PrefBodySize => "preferred body size",
    TolBodySize => "body size tolerance",
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Allele {
    pub value: f32,
    pub origin: PlayerId,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Locus {
    pub a: Allele,
    pub b: Allele,
}

impl Locus {
    pub fn expressed(&self) -> f32 {
        0.5 * (self.a.value + self.b.value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MutationParams {
    /// Probability per allele per generation.
    pub rate: f32,
    /// Standard deviation of the change in allele value.
    pub sd: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Genome {
    loci: [Locus; Gene::COUNT],
}

impl Genome {
    /// Every allele set exactly to the given values.
    pub fn uniform(values: &[f32; Gene::COUNT], origin: PlayerId) -> Self {
        let mut loci = [Locus {
            a: Allele { value: 0.0, origin },
            b: Allele { value: 0.0, origin },
        }; Gene::COUNT];
        for (locus, &v) in loci.iter_mut().zip(values) {
            let allele = Allele { value: v.clamp(0.0, 1.0), origin };
            *locus = Locus { a: allele, b: allele };
        }
        Self { loci }
    }

    /// A founder: each allele varies around the design value so selection has
    /// material from day one.
    pub fn founder(
        values: &[f32; Gene::COUNT],
        origin: PlayerId,
        variation: f32,
        rng: &mut impl Rng,
    ) -> Self {
        let mut g = Self::uniform(values, origin);
        for locus in g.loci.iter_mut() {
            for allele in [&mut locus.a, &mut locus.b] {
                allele.value = (allele.value + normal(rng) * variation).clamp(0.0, 1.0);
            }
        }
        g
    }

    pub fn locus(&self, gene: Gene) -> &Locus {
        &self.loci[gene.index()]
    }

    pub fn loci(&self) -> &[Locus] {
        &self.loci
    }

    pub fn expr(&self, gene: Gene) -> f32 {
        self.loci[gene.index()].expressed()
    }

    pub fn phenotype(&self) -> Phenotype {
        let mut p = [0.0; Gene::COUNT];
        for (v, locus) in p.iter_mut().zip(&self.loci) {
            *v = locus.expressed();
        }
        Phenotype(p)
    }

    /// Sexual reproduction: one random allele per locus from each parent
    /// (independent assortment), then mutation. Mutations are tagged with the
    /// offspring's owner.
    pub fn inherit(
        mother: &Genome,
        father: &Genome,
        child_owner: PlayerId,
        mutation: MutationParams,
        rng: &mut impl Rng,
    ) -> Genome {
        let mut loci = mother.loci;
        for (k, locus) in loci.iter_mut().enumerate() {
            let m = &mother.loci[k];
            let f = &father.loci[k];
            let from_m = if rng.gen::<bool>() { m.a } else { m.b };
            let from_f = if rng.gen::<bool>() { f.a } else { f.b };
            *locus = Locus {
                a: mutate(from_m, child_owner, mutation, rng),
                b: mutate(from_f, child_owner, mutation, rng),
            };
        }
        Genome { loci }
    }

    /// Mean absolute difference in expressed values. Used for kin/species
    /// recognition and outbreeding depression.
    pub fn distance(&self, other: &Genome) -> f32 {
        let sum: f32 = self
            .loci
            .iter()
            .zip(&other.loci)
            .map(|(a, b)| (a.expressed() - b.expressed()).abs())
            .sum();
        sum / Gene::COUNT as f32
    }

    /// Add `weight` per allele to each allele's origin.
    pub fn tally_origins(&self, weight: f64, out: &mut BTreeMap<PlayerId, f64>) {
        for locus in &self.loci {
            *out.entry(locus.a.origin).or_default() += weight;
            *out.entry(locus.b.origin).or_default() += weight;
        }
    }
}

fn mutate(allele: Allele, owner: PlayerId, m: MutationParams, rng: &mut impl Rng) -> Allele {
    if rng.gen::<f32>() < m.rate {
        Allele {
            value: (allele.value + normal(rng) * m.sd).clamp(0.0, 1.0),
            origin: owner,
        }
    } else {
        allele
    }
}

/// Expressed gene values.
#[derive(Clone, Debug, PartialEq)]
pub struct Phenotype(pub [f32; Gene::COUNT]);

impl Index<Gene> for Phenotype {
    type Output = f32;
    fn index(&self, g: Gene) -> &f32 {
        &self.0[g.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::rng_for;

    fn flat(v: f32) -> [f32; Gene::COUNT] {
        [v; Gene::COUNT]
    }

    #[test]
    fn inheritance_keeps_origin_tags_without_mutation() {
        let mut rng = rng_for(1, 0, 0, 0);
        let mother = Genome::uniform(&flat(0.2), 1);
        let father = Genome::uniform(&flat(0.8), 2);
        let none = MutationParams { rate: 0.0, sd: 0.0 };
        let child = Genome::inherit(&mother, &father, 3, none, &mut rng);
        for locus in child.loci() {
            assert_eq!(locus.a.origin, 1);
            assert_eq!(locus.b.origin, 2);
            assert!((locus.expressed() - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn mutations_are_tagged_with_child_owner() {
        let mut rng = rng_for(1, 0, 0, 0);
        let parent = Genome::uniform(&flat(0.5), 1);
        let always = MutationParams { rate: 1.0, sd: 0.1 };
        let child = Genome::inherit(&parent, &parent, 7, always, &mut rng);
        assert!(child.loci().iter().all(|l| l.a.origin == 7 && l.b.origin == 7));
    }

    #[test]
    fn distance_is_mean_expression_difference() {
        let a = Genome::uniform(&flat(0.2), 1);
        let b = Genome::uniform(&flat(0.5), 1);
        assert!((a.distance(&b) - 0.3).abs() < 1e-6);
        assert_eq!(a.distance(&a), 0.0);
    }

    #[test]
    fn tally_counts_two_alleles_per_gene() {
        let g = Genome::uniform(&flat(0.5), 4);
        let mut out = BTreeMap::new();
        g.tally_origins(1.0, &mut out);
        assert_eq!(out[&4], (2 * Gene::COUNT) as f64);
    }
}
