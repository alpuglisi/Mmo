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
//! **Dominance.** Most genes are additive (the mean of the two alleles). A few
//! have a dominance coefficient `h` ([`dominance`]): expressed =
//! `mean + h * |a - b| / 2`, so `h = 1` expresses the larger allele (dominant
//! high values), `h = -1` the smaller (recessive high values) and `h = 0` the
//! mean. Homozygotes express their allele whatever `h` is.
//!
//! **Hidden recessives.** Besides the genes, every genome carries
//! [`LOAD_LOCI`] load loci whose alleles are either functional or harmful
//! ([`Allele::is_harmful`]). A harmful allele does nothing in a carrier
//! (heterozygous); an organism homozygous for one at any load locus suffers
//! (`Balance::recessive_hazard`). Harmful alleles arise by mutation
//! (`Balance::recessive_mutation`), are inherited and origin-tagged like any
//! allele, and can be planted (a carrier founder or stud, [`Genome::set_load`]),
//! so inbreeding exposes them and outcrossing hides them.

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

/// Dominance coefficient `h` of a gene, in `[-1, 1]`: expressed =
/// `mean + h * |a - b| / 2` (see the module docs).
///
/// Almost every gene is additive (`h = 0`): body, behavior and preference
/// genes are quantitative traits, and the body-plan genes decide fertility and
/// kin recognition, whose thresholds assume the mean. The exceptions are
/// visible traits where dominance changes what a mating shows:
///
/// - **Coloration** is partly dominant: one bright allele makes a showy
///   animal, so a cross with a bright lineage shows at once.
/// - **Fur** is partly dominant (a thick coat, as in many mammals).
/// - **Camouflage** is partly recessive: crypsis needs both alleles, so an
///   outcross to a conspicuous lineage loses most of it for a generation and
///   inbreeding brings it back. A cost of mating widely.
///
/// Founder alleles differ by about 0.05, so these shift founders' expressed
/// values by about 0.01: existing designs and balance barely move.
pub const fn dominance(gene: Gene) -> f32 {
    match gene {
        Gene::Coloration => 0.5,
        Gene::Fur => 0.5,
        Gene::Camouflage => -0.5,
        _ => 0.0,
    }
}

/// [`dominance`] for every gene, indexed by [`Gene::index`].
pub const DOMINANCE: [f32; Gene::COUNT] = {
    let mut h = [0.0; Gene::COUNT];
    let mut k = 0;
    while k < Gene::COUNT {
        h[k] = dominance(Gene::ALL[k]);
        k += 1;
    }
    h
};

/// Number of load loci (hidden recessives) per genome.
pub const LOAD_LOCI: usize = 4;

/// Value of a harmful allele at a load locus (functional alleles are 0).
const HARMFUL: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Allele {
    pub value: f32,
    pub origin: PlayerId,
}

impl Allele {
    /// A harmful recessive allele at a load locus.
    pub fn harmful(origin: PlayerId) -> Self {
        Allele { value: HARMFUL, origin }
    }

    /// At a load locus: is this allele harmful?
    pub fn is_harmful(&self) -> bool {
        self.value >= 0.5
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Locus {
    pub a: Allele,
    pub b: Allele,
}

impl Locus {
    /// Expressed value with dominance coefficient `h` ([`dominance`]).
    #[inline]
    pub fn expressed(&self, h: f32) -> f32 {
        let (a, b) = (self.a.value, self.b.value);
        0.5 * (a + b) + 0.5 * h * (a - b).abs()
    }

    /// At a load locus: both alleles harmful, so the defect shows.
    pub fn homozygous_harmful(&self) -> bool {
        self.a.is_harmful() && self.b.is_harmful()
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
    /// Hidden-recessive loci: not genes, never expressed in the phenotype or
    /// counted in [`Genome::distance`].
    load: [Locus; LOAD_LOCI],
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
        let functional = Allele { value: 0.0, origin };
        Self { loci, load: [Locus { a: functional, b: functional }; LOAD_LOCI] }
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
        let k = gene.index();
        self.loci[k].expressed(DOMINANCE[k])
    }

    pub fn phenotype(&self) -> Phenotype {
        let mut p = [0.0; Gene::COUNT];
        for ((v, locus), &h) in p.iter_mut().zip(&self.loci).zip(&DOMINANCE) {
            *v = locus.expressed(h);
        }
        Phenotype(p)
    }

    /// The hidden-recessive loci.
    pub fn load(&self) -> &[Locus; LOAD_LOCI] {
        &self.load
    }

    /// Set one allele (`second` picks `b`) at load locus `k`: plant a harmful
    /// recessive ([`Allele::harmful`], tagged with the planter) in a founder or
    /// stud, or clear one.
    pub fn set_load(&mut self, k: usize, second: bool, allele: Allele) {
        let locus = &mut self.load[k];
        *if second { &mut locus.b } else { &mut locus.a } = allele;
    }

    /// Load loci where both alleles are harmful: the defects that show.
    pub fn homozygous_load(&self) -> u32 {
        self.load.iter().filter(|l| l.homozygous_harmful()).count() as u32
    }

    /// Harmful alleles carried, shown or hidden.
    pub fn harmful_alleles(&self) -> u32 {
        self.load.iter().map(|l| l.a.is_harmful() as u32 + l.b.is_harmful() as u32).sum()
    }

    /// Sexual reproduction: one random allele per locus from each parent
    /// (independent assortment), then mutation. Mutations are tagged with the
    /// offspring's owner. At load loci a functional allele turns harmful with
    /// probability `load_rate`. [`Genome::inherit_genes`] and
    /// [`Genome::inherit_load`] do the two halves, so a caller can give them
    /// separate random streams.
    pub fn inherit(
        mother: &Genome,
        father: &Genome,
        child_owner: PlayerId,
        mutation: MutationParams,
        load_rate: f32,
        rng: &mut impl Rng,
    ) -> Genome {
        let mut child = Self::inherit_genes(mother, father, child_owner, mutation, rng);
        child.inherit_load(mother, father, child_owner, load_rate, rng);
        child
    }

    /// The genes half of [`Genome::inherit`]; the load loci are copied from
    /// the mother until [`Genome::inherit_load`] runs.
    pub fn inherit_genes(
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
        Genome { loci, load: mother.load }
    }

    /// The load-loci half of [`Genome::inherit`]: one allele from each
    /// parent, then harmful mutations at `load_rate` (no draws when it is 0).
    pub fn inherit_load(&mut self, mother: &Genome, father: &Genome, child_owner: PlayerId, load_rate: f32, rng: &mut impl Rng) {
        for (k, locus) in self.load.iter_mut().enumerate() {
            let m = &mother.load[k];
            let f = &father.load[k];
            let from_m = if rng.gen::<bool>() { m.a } else { m.b };
            let from_f = if rng.gen::<bool>() { f.a } else { f.b };
            *locus = Locus {
                a: mutate_load(from_m, child_owner, load_rate, rng),
                b: mutate_load(from_f, child_owner, load_rate, rng),
            };
        }
    }

    /// Mean absolute difference in expressed values over the genes (load loci
    /// excluded). Used for kin/species recognition and outbreeding depression.
    pub fn distance(&self, other: &Genome) -> f32 {
        let sum: f32 = self
            .loci
            .iter()
            .zip(&other.loci)
            .zip(&DOMINANCE)
            .map(|((a, b), &h)| (a.expressed(h) - b.expressed(h)).abs())
            .sum();
        sum / Gene::COUNT as f32
    }

    /// Add `weight` per allele to each allele's origin, load loci included:
    /// a harmful recessive planted in a rival lineage still counts for the
    /// player it is tagged with.
    pub fn tally_origins(&self, weight: f64, out: &mut BTreeMap<PlayerId, f64>) {
        for locus in self.loci.iter().chain(&self.load) {
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

fn mutate_load(allele: Allele, owner: PlayerId, rate: f32, rng: &mut impl Rng) -> Allele {
    if rate > 0.0 && !allele.is_harmful() && rng.gen::<f32>() < rate {
        Allele::harmful(owner)
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
        let child = Genome::inherit(&mother, &father, 3, none, 0.0, &mut rng);
        for (locus, &h) in child.loci().iter().zip(&DOMINANCE) {
            assert_eq!(locus.a.origin, 1);
            assert_eq!(locus.b.origin, 2);
            assert!((locus.expressed(h) - (0.5 + 0.3 * h)).abs() < 1e-6);
        }
        for locus in child.load() {
            assert_eq!((locus.a.origin, locus.b.origin), (1, 2));
        }
    }

    #[test]
    fn mutations_are_tagged_with_child_owner() {
        let mut rng = rng_for(1, 0, 0, 0);
        let parent = Genome::uniform(&flat(0.5), 1);
        let always = MutationParams { rate: 1.0, sd: 0.1 };
        let child = Genome::inherit(&parent, &parent, 7, always, 0.0, &mut rng);
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
        assert_eq!(out[&4], (2 * (Gene::COUNT + LOAD_LOCI)) as f64);
    }

    fn locus(a: f32, b: f32) -> Locus {
        Locus { a: Allele { value: a, origin: 1 }, b: Allele { value: b, origin: 1 } }
    }

    #[test]
    fn dominance_mixes_mean_with_max_or_min() {
        let het = locus(0.2, 0.8);
        assert!((het.expressed(0.0) - 0.5).abs() < 1e-6);
        assert!((het.expressed(1.0) - 0.8).abs() < 1e-6);
        assert!((het.expressed(-1.0) - 0.2).abs() < 1e-6);
        assert!((het.expressed(0.5) - 0.65).abs() < 1e-6);
        // Homozygotes express their allele whatever the dominance.
        for h in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            assert!((locus(0.7, 0.7).expressed(h) - 0.7).abs() < 1e-6);
        }
    }

    #[test]
    fn display_genes_have_dominance_and_the_rest_are_additive() {
        // A bright × dull cross shows brighter than the mean, a cryptic ×
        // conspicuous cross less cryptic.
        let dull = Genome::uniform(&flat(0.0), 1);
        let bright = Genome::uniform(&flat(1.0), 2);
        let none = MutationParams { rate: 0.0, sd: 0.0 };
        let mut rng = rng_for(2, 0, 0, 0);
        let f1 = Genome::inherit(&dull, &bright, 1, none, 0.0, &mut rng);
        assert!(f1.expr(Gene::Coloration) > 0.7);
        assert!(f1.expr(Gene::Fur) > 0.7);
        assert!(f1.expr(Gene::Camouflage) < 0.3);
        let additive = Gene::ALL.iter().filter(|&&g| dominance(g) == 0.0).count();
        assert!(additive >= Gene::COUNT - 4, "most genes stay additive");
        for g in [Gene::Endothermy, Gene::BirthMode, Gene::BodySize, Gene::Carnivory, Gene::Pickiness] {
            assert_eq!(dominance(g), 0.0, "{g:?}");
            assert!((f1.expr(g) - 0.5).abs() < 1e-6);
        }
        // The phenotype and distance use the same expression.
        let p = f1.phenotype();
        for &g in Gene::ALL {
            assert_eq!(p[g], f1.expr(g));
        }
        let manual: f32 = Gene::ALL.iter().map(|&g| (f1.expr(g) - dull.expr(g)).abs()).sum::<f32>() / Gene::COUNT as f32;
        assert!((f1.distance(&dull) - manual).abs() < 1e-6);
    }

    #[test]
    fn harmful_recessives_show_only_in_homozygotes() {
        let mut g = Genome::uniform(&flat(0.5), 1);
        assert_eq!((g.homozygous_load(), g.harmful_alleles()), (0, 0));
        g.set_load(0, false, Allele::harmful(2));
        assert_eq!((g.homozygous_load(), g.harmful_alleles()), (0, 1), "a carrier shows nothing");
        g.set_load(0, true, Allele::harmful(2));
        assert_eq!((g.homozygous_load(), g.harmful_alleles()), (1, 2));
        // Load loci are not genes: phenotype and distance ignore them.
        assert_eq!(g.phenotype(), Genome::uniform(&flat(0.5), 1).phenotype());
        assert_eq!(g.distance(&Genome::uniform(&flat(0.5), 1)), 0.0);
    }

    #[test]
    fn harmful_mutations_arise_tagged_with_the_child_owner() {
        let parent = Genome::uniform(&flat(0.5), 1);
        let none = MutationParams { rate: 0.0, sd: 0.0 };
        let mut rng = rng_for(3, 0, 0, 0);
        let child = Genome::inherit(&parent, &parent, 9, none, 1.0, &mut rng);
        assert_eq!(child.homozygous_load(), LOAD_LOCI as u32);
        assert!(child.load().iter().all(|l| l.a.origin == 9 && l.b.origin == 9));
        let clean = Genome::inherit(&parent, &parent, 9, none, 0.0, &mut rng);
        assert_eq!(clean.harmful_alleles(), 0);
    }

    /// Share of offspring homozygous at a load locus over `generations` of
    /// matings within a line founded by one planted carrier stud (tagged 2)
    /// and clean mates of player 1: `inbred` lines mate siblings, outcrossed
    /// lines mate each child to a fresh clean partner.
    fn exposure(inbred: bool, seed: u64) -> (f64, u32) {
        let none = MutationParams { rate: 0.0, sd: 0.0 };
        let mut rng = rng_for(seed, 0, 0, 0);
        let clean = Genome::uniform(&flat(0.5), 1);
        let mut stud = clean.clone();
        for k in 0..LOAD_LOCI {
            stud.set_load(k, false, Allele::harmful(2));
        }
        let (mut shown, mut born, mut planted_tags) = (0u32, 0u32, 0u32);
        for _ in 0..200 {
            let mut pair = (Genome::inherit(&stud, &clean, 1, none, 0.0, &mut rng), Genome::inherit(&stud, &clean, 1, none, 0.0, &mut rng));
            for _ in 0..4 {
                let mate = if inbred { pair.1.clone() } else { clean.clone() };
                let a = Genome::inherit(&pair.0, &mate, 1, none, 0.0, &mut rng);
                let b = Genome::inherit(&pair.0, &mate, 1, none, 0.0, &mut rng);
                for c in [&a, &b] {
                    born += 1;
                    shown += c.homozygous_load().min(1);
                    for l in c.load() {
                        for x in [l.a, l.b] {
                            if x.is_harmful() {
                                assert_eq!(x.origin, 2, "harmful alleles keep the planter's tag");
                                planted_tags += 1;
                            }
                        }
                    }
                }
                pair = (a, b);
            }
        }
        (shown as f64 / born as f64, planted_tags)
    }

    #[test]
    fn inbreeding_exposes_recessives_and_outcrossing_hides_them() {
        let (inbred, tags) = exposure(true, 5);
        let (outcrossed, _) = exposure(false, 5);
        assert!(tags > 0);
        assert!(inbred > 0.25, "inbred line shows defects in {inbred:.2} of offspring");
        assert_eq!(outcrossed, 0.0, "outcrossing to clean mates never pairs two harmful alleles");
    }

    #[test]
    fn planted_recessives_count_for_the_planter() {
        let mut g = Genome::uniform(&flat(0.5), 1);
        g.set_load(1, false, Allele::harmful(2));
        let mut out = BTreeMap::new();
        g.tally_origins(1.0, &mut out);
        assert_eq!(out[&2], 1.0);
        assert_eq!(out[&1], (2 * (Gene::COUNT + LOAD_LOCI) - 1) as f64);
    }
}
