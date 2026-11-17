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
//!
//! **Morphology loci.** Every genome also carries [`MorphGenes`]: discrete
//! loci for eye, ear and leg counts and ear and tail shapes, and continuous
//! loci for eye placement and snout length (`docs/morphology-contract.md`).
//! They are not genes: not in the phenotype or the gene mutation streams;
//! their expressed counts and shapes count toward [`Genome::distance`], and
//! their alleles count in the legacy tally under `Balance::morph_legacy`
//! ([`Genome::count_origins_with`]). See [`crate::morph`].

use std::collections::BTreeMap;
use std::ops::Index;

use rand::Rng;

use crate::morph::{MorphDesign, MorphGenes};
use crate::rng::{normal, splitmix64};

pub type PlayerId = u32;

/// Owner ID of the wild ecosystem.
pub const WILD: PlayerId = 0;

/// Seasonal-strategy genes. They are left out of [`Genome::distance`]:
/// they are 0 in most designs, so counting them would dilute the distance
/// (making different species kin), while founder variation on them would
/// split founders of one design into strangers. Kin, mating and fertility
/// thresholds stay as tuned on the other genes. (`Vigilance`, a behavior
/// gene, counts like the other instinct genes.)
pub const SEASONAL_GENES: [Gene; 8] = [
    Gene::Hibernation,
    Gene::Brumation,
    Gene::Migration,
    Gene::Estivation,
    Gene::Caching,
    Gene::Torpor,
    Gene::SeasonalCoat,
    Gene::TimedBreeding,
];

/// Genes besides the [`SEASONAL_GENES`] that [`Genome::distance`] leaves
/// out. `Obedience` sets how a group heeds its player's commands, not what
/// an animal is: counting it would only dilute the distance, and kin,
/// mating and fertility thresholds stay as tuned on the other genes.
pub const OUT_OF_DISTANCE: [Gene; 1] = [Gene::Obedience];

/// Which genes count toward [`Genome::distance`], by gene index.
pub const IN_DISTANCE: [bool; Gene::COUNT] = {
    let mut mask = [true; Gene::COUNT];
    let mut k = 0;
    while k < SEASONAL_GENES.len() {
        mask[SEASONAL_GENES[k] as usize] = false;
        k += 1;
    }
    let mut k = 0;
    while k < OUT_OF_DISTANCE.len() {
        mask[OUT_OF_DISTANCE[k] as usize] = false;
        k += 1;
    }
    mask
};

/// Number of genes [`Genome::distance`] averages over.
pub const DISTANCE_GENES: f32 = (Gene::COUNT - SEASONAL_GENES.len() - OUT_OF_DISTANCE.len()) as f32;

macro_rules! define_genes {
    ($( $(#[$m:meta])* $name:ident => $label:literal ),* $(,)?) => {
        /// Every heritable locus. Physical, behavioral and mate-preference
        /// traits are all genes, so all of them evolve.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    // Seasonal strategy (see `stats::Seasonal`). Each is expressed above
    // `stats::STRATEGY_THRESHOLD`, and works better the higher it is.
    /// Winter dormancy on stored fat, warm-leaning: a cold-blooded body can't
    /// heat itself to wake.
    Hibernation => "hibernation",
    /// Winter dormancy that wakes with the weather, cold-leaning: a
    /// warm-blooded body keeps burning fat while dormant.
    Brumation => "brumation",
    /// Seasonal range shift: toward warmer latitude in autumn, back in spring.
    Migration => "migration",
    /// Summer dormancy through heat.
    Estivation => "estivation",
    /// Food caching (add-on): hoards plant food in late summer and autumn.
    Caching => "food caching",
    /// Short-term torpor (add-on): cheap, shallow dormancy on cold nights.
    Torpor => "torpor",
    // Group behavior (critical-mass group traits).
    /// Sentry instinct: in a group species, takes turns standing watch
    /// (`GroupTrait::Sentries`).
    Vigilance => "vigilance",
    // Seasonal add-ons appended after the group genes (kept out of
    // `Genome::distance` with the other seasonal genes).
    /// Seasonal coat (add-on): molts into a thick winter coat in autumn and
    /// back in spring, paying food for each molt. Warm-leaning: a
    /// cold-blooded body can't use it (it blocks basking, like fur).
    SeasonalCoat => "seasonal coat",
    /// Timed breeding (add-on): breeds only in a narrow early-spring window,
    /// with a strong drive and better conception there, and none outside it.
    TimedBreeding => "timed breeding",
    // Group commands as genes (appended last; kept out of
    // `Genome::distance`, see [`OUT_OF_DISTANCE`]).
    /// How strongly its group's commands (an ordered migration, a held
    /// territory) outweigh its own instincts, and how long it keeps to them
    /// when hungry. 0.5 obeys as the commands were tuned; the price is
    /// tameness: an obedient animal flees a threat later
    /// (`Balance::tameness`). See `world::groups`.
    Obedience => "obedience",
}

impl Gene {
    /// The gene's wire key: its label in `snake_case` (`"large_ears"`). The
    /// storage and protocol name; never shown to players.
    pub fn key(self) -> String {
        self.label().replace(' ', "_")
    }

    /// The gene's display name ("Ear size"), the one players see; see
    /// [`trait_display_name`].
    pub fn display_name(self) -> &'static str {
        trait_display_name(self)
    }

    /// [`Gene::display_name`] for use inside a sentence ("ear size").
    pub fn display_name_lower(self) -> String {
        let n = self.display_name();
        let mut c = n.chars();
        match c.next() {
            // Keep an all-capitals word (an acronym) as it is, like the client.
            Some(a) if !(a.is_ascii_uppercase() && c.next().is_some_and(|b| b.is_ascii_uppercase())) => {
                a.to_lowercase().chain(n[a.len_utf8()..].chars()).collect()
            }
            _ => n.to_string(),
        }
    }
}

/// The display name of a gene's slider: the one place server and sim text
/// names a trait to players. It must match the client's map,
/// `client/src/ui/traitNames.ts` (`TRAIT_NAMES`), name for name; the test
/// `the_client_trait_name_fixture_is_current` writes
/// `client/src/ui/traitNames.fixture.json`, which the client's
/// `traitNames.test.ts` checks against that map.
///
/// The labels ([`Gene::label`]) and wire keys ([`Gene::key`]) stay as they
/// are: a key such as `large_ears` names one end of its slider (0 gives small
/// ears), so the display name names the axis instead ("Ear size").
pub fn trait_display_name(gene: Gene) -> &'static str {
    use Gene::*;
    match gene {
        Endothermy => "Warm-bloodedness",
        BirthMode => "Birth mode",
        LitterSize => "Offspring per birth",
        BodySize => "Body size",
        Muscle => "Muscle mass",
        FatStores => "Body fat",
        LegLength => "Leg length",
        Claws => "Claws",
        Hooves => "Hooves",
        FootWidth => "Foot width",
        Tail => "Tail length",
        Swimming => "Swimming",
        Wings => "Wings",
        Climbing => "Climbing",
        Burrowing => "Burrowing",
        Fur => "Fur",
        Scales => "Scales",
        Armor => "Armor",
        Fangs => "Fang size",
        Eyes => "Eye size",
        Ears => "Ear size",
        Nose => "Sense of smell",
        Carnivory => "Diet",
        Camouflage => "Camouflage",
        Toxins => "Toxicity",
        Coloration => "Colour",
        Vocal => "Voice",
        Boldness => "Boldness",
        Aggression => "Aggression",
        Wanderlust => "Roaming",
        GroupTolerance => "Sociability",
        ForagingRange => "Foraging range",
        PackInstinct => "Pack hunting",
        Pickiness => "Mate choosiness",
        PrefColoration => "Preferred mate colour",
        TolColoration => "Mate colour tolerance",
        PrefBodySize => "Preferred mate size",
        TolBodySize => "Mate size tolerance",
        Hibernation => "Hibernation",
        Brumation => "Brumation",
        Migration => "Migration",
        Estivation => "Estivation",
        Caching => "Food caching",
        Torpor => "Torpor",
        SeasonalCoat => "Seasonal coat",
        TimedBreeding => "Timed breeding",
        Vigilance => "Sentry instinct",
        Obedience => "Obedience",
    }
}

/// Index of `Gene::Obedience`, the last gene (see [`Genome::founder`]).
const OBEDIENCE: usize = Gene::Obedience as usize;
const _: () = assert!(OBEDIENCE == Gene::COUNT - 1, "obedience is the last gene");

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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MutationParams {
    /// Probability per allele per generation.
    pub rate: f32,
    /// Standard deviation of the change in allele value.
    pub sd: f32,
}

/// In snapshots a genome is written compactly and exactly ([`wire`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Genome {
    loci: [Locus; Gene::COUNT],
    /// Hidden-recessive loci: not genes, never expressed in the phenotype or
    /// counted in [`Genome::distance`].
    load: [Locus; LOAD_LOCI],
    /// Morphology loci ([`crate::morph`]).
    morph: MorphGenes,
}

impl Genome {
    /// Every allele set exactly to the given values, the morphology loci at
    /// the contract defaults.
    pub fn uniform(values: &[f32; Gene::COUNT], origin: PlayerId) -> Self {
        Self::uniform_with_morph(values, &MorphDesign::default(), origin)
    }

    /// Every allele set exactly to the given values and morphology.
    pub fn uniform_with_morph(values: &[f32; Gene::COUNT], morph: &MorphDesign, origin: PlayerId) -> Self {
        let mut loci = [Locus { a: Allele { value: 0.0, origin }, b: Allele { value: 0.0, origin } }; Gene::COUNT];
        for (locus, &v) in loci.iter_mut().zip(values) {
            let allele = Allele { value: v.clamp(0.0, 1.0), origin };
            *locus = Locus { a: allele, b: allele };
        }
        let functional = Allele { value: 0.0, origin };
        Self {
            loci,
            load: [Locus { a: functional, b: functional }; LOAD_LOCI],
            morph: MorphGenes::uniform(morph, origin),
        }
    }

    /// A founder: each allele varies around the design value so selection has
    /// material from day one.
    ///
    /// The obedience locus draws from a stream of its own ([`Genome::own_rng`]),
    /// so the caller's stream, and every founder drawn after this one, is
    /// what it was before `Gene::Obedience` existed.
    pub fn founder(values: &[f32; Gene::COUNT], origin: PlayerId, variation: f32, rng: &mut impl Rng) -> Self {
        Self::founder_with_morph(values, &MorphDesign::default(), origin, variation, rng)
    }

    /// [`Genome::founder`] with a design's morphology: the discrete alleles
    /// exactly the design's, the continuous ones varied around it on the
    /// genome's own stream ([`Genome::morph_rng`]), so the caller's stream is
    /// what it would be without them.
    pub fn founder_with_morph(
        values: &[f32; Gene::COUNT],
        morph: &MorphDesign,
        origin: PlayerId,
        variation: f32,
        rng: &mut impl Rng,
    ) -> Self {
        let mut g = Self::uniform_with_morph(values, morph, origin);
        for locus in &mut g.loci[..OBEDIENCE] {
            for allele in [&mut locus.a, &mut locus.b] {
                allele.value = (allele.value + normal(rng) * variation).clamp(0.0, 1.0);
            }
        }
        let mut own = g.own_rng(0);
        let locus = &mut g.loci[OBEDIENCE];
        for allele in [&mut locus.a, &mut locus.b] {
            allele.value = (allele.value + normal(&mut own) * variation).clamp(0.0, 1.0);
        }
        let mut own = g.morph_rng(0);
        for locus in &mut g.morph.continuous {
            for allele in [&mut locus.a, &mut locus.b] {
                allele.value = (allele.value + normal(&mut own) * variation).clamp(0.0, 1.0);
            }
        }
        g
    }

    /// A random stream for the obedience locus, seeded by every other gene
    /// allele (and `salt`): the locus added last draws nothing from the
    /// shared streams, so worlds found, breed and mutate the other genes
    /// exactly as before it existed, and its own alleles still vary and
    /// mutate at the usual rates.
    fn own_rng(&self, salt: u64) -> crate::rng::SimRng {
        let h = self.loci[..OBEDIENCE].iter().fold(splitmix64(salt ^ 0x0bed_1e5c), |h, l| {
            let bits = (l.a.value.to_bits() as u64) << 32 | l.b.value.to_bits() as u64;
            splitmix64(h ^ bits ^ ((l.a.origin as u64) << 7) ^ ((l.b.origin as u64) << 19))
        });
        <crate::rng::SimRng as rand::SeedableRng>::seed_from_u64(h)
    }

    /// A random stream for the morphology loci, seeded by every gene allele
    /// (and `salt`), like [`Genome::own_rng`] for obedience: the morphology
    /// loci draw nothing from the shared streams.
    fn morph_rng(&self, salt: u64) -> crate::rng::SimRng {
        let h = self.loci.iter().fold(splitmix64(salt ^ 0x6d02_9b0d_7e11), |h, l| {
            let bits = (l.a.value.to_bits() as u64) << 32 | l.b.value.to_bits() as u64;
            splitmix64(h ^ bits ^ ((l.a.origin as u64) << 11) ^ ((l.b.origin as u64) << 23))
        });
        <crate::rng::SimRng as rand::SeedableRng>::seed_from_u64(h)
    }

    /// Step mutations of the discrete morphology alleles, each with
    /// probability `rate` (`Balance::morph_step_rate`; nothing happens, and
    /// nothing is drawn, at 0), tagged `owner`, on the genome's own stream
    /// (salted with `owner`): the shared streams are untouched.
    pub fn mutate_morph(&mut self, owner: PlayerId, rate: f32) {
        if rate <= 0.0 {
            return;
        }
        // Salted with the core alleles only, so the Phase 3 loci leave the
        // core loci's draws as they were.
        let core = &self.morph.wire_alleles()[..crate::morph::CORE_MORPH_ALLELES];
        let salt = core.iter().fold(splitmix64(owner as u64 ^ 0x57e9_0b1d), |h, &(bits, o)| {
            splitmix64(h ^ bits as u64 ^ (o as u64) << 32)
        });
        let mut own = self.morph_rng(salt);
        self.morph.mutate_steps(owner, rate, &mut own);
    }

    /// The morphology loci.
    pub fn morph(&self) -> &MorphGenes {
        &self.morph
    }

    /// Replace the morphology loci: a test or tool that needs a particular
    /// genotype.
    pub fn set_morph(&mut self, morph: MorphGenes) {
        self.morph = morph;
    }

    /// The expressed morphology ([`crate::morph::morphology_of`]).
    pub fn morphology(&self) -> crate::morph::Morphology {
        crate::morph::morphology_of(self)
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

    /// Set both alleles of `gene` (keeping their origin tags): a test or
    /// tool that needs a particular genotype.
    pub fn set_gene_alleles(&mut self, gene: Gene, a: f32, b: f32) {
        let locus = &mut self.loci[gene.index()];
        locus.a.value = a.clamp(0.0, 1.0);
        locus.b.value = b.clamp(0.0, 1.0);
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
        fn pick(m: &Locus, f: &Locus, owner: PlayerId, mutation: MutationParams, rng: &mut impl Rng) -> Locus {
            let from_m = if rng.gen::<bool>() { m.a } else { m.b };
            let from_f = if rng.gen::<bool>() { f.a } else { f.b };
            Locus { a: mutate(from_m, owner, mutation, rng), b: mutate(from_f, owner, mutation, rng) }
        }
        for (k, locus) in loci[..OBEDIENCE].iter_mut().enumerate() {
            *locus = pick(&mother.loci[k], &father.loci[k], child_owner, mutation, rng);
        }
        let mut child = Genome { loci, load: mother.load, morph: mother.morph };
        // The obedience locus on a stream of its own (see `own_rng`), salted
        // with the parents' obedience alleles.
        let (m, f) = (&mother.loci[OBEDIENCE], &father.loci[OBEDIENCE]);
        let salt =
            [m.a, m.b, f.a, f.b].iter().fold(child_owner as u64, |h, a| splitmix64(h ^ a.value.to_bits() as u64));
        let mut own = child.own_rng(salt);
        child.loci[OBEDIENCE] = pick(m, f, child_owner, mutation, &mut own);
        // The morphology loci on a stream of their own too, salted with the
        // parents' morphology alleles.
        let core = crate::morph::CORE_MORPH_ALLELES;
        let salt = mother.morph.wire_alleles()[..core]
            .iter()
            .chain(&father.morph.wire_alleles()[..core])
            .fold(splitmix64(child_owner as u64 ^ 0x3c7a), |h, &(bits, origin)| {
                splitmix64(h ^ bits as u64 ^ (origin as u64) << 32)
            });
        let mut own = child.morph_rng(salt);
        child.morph = MorphGenes::inherit(&mother.morph, &father.morph, child_owner, mutation, &mut own);
        child
    }

    /// The load-loci half of [`Genome::inherit`]: one allele from each
    /// parent, then harmful mutations at `load_rate` (no draws when it is 0).
    pub fn inherit_load(
        &mut self,
        mother: &Genome,
        father: &Genome,
        child_owner: PlayerId,
        load_rate: f32,
        rng: &mut impl Rng,
    ) {
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

    /// Mean absolute difference in expressed values over the genes in
    /// [`IN_DISTANCE`] (seasonal genes and load loci excluded), plus the
    /// difference in expressed morphology counts and shapes
    /// ([`crate::morph::morph_terms`]; 0.0 between equal morphologies, so the
    /// distance is then exactly the genes'), over [`DISTANCE_GENES`]. Used
    /// for kin/species recognition and outbreeding depression.
    pub fn distance(&self, other: &Genome) -> f32 {
        let sum: f32 = self
            .loci
            .iter()
            .zip(&other.loci)
            .zip(&DOMINANCE)
            .zip(IN_DISTANCE)
            .map(|(((a, b), &h), counts)| if counts { (a.expressed(h) - b.expressed(h)).abs() } else { 0.0 })
            .sum();
        (sum + crate::morph::morph_terms(&self.morph.key(), &other.morph.key()).0) / DISTANCE_GENES
    }

    /// Add `weight` per allele to each allele's origin, load loci included:
    /// a harmful recessive planted in a rival lineage still counts for the
    /// player it is tagged with. The morphology loci are not counted (see
    /// [`Self::tally_origins_with`]).
    pub fn tally_origins(&self, weight: f64, out: &mut BTreeMap<PlayerId, f64>) {
        self.tally_origins_with(weight, out, false);
    }

    /// [`Self::tally_origins`], and with `morph` the morphology segment's
    /// 14 alleles too (the legacy tally under `Balance::morph_legacy`).
    pub fn tally_origins_with(&self, weight: f64, out: &mut BTreeMap<PlayerId, f64>, morph: bool) {
        for locus in self.loci.iter().chain(&self.load) {
            *out.entry(locus.a.origin).or_default() += weight;
            *out.entry(locus.b.origin).or_default() += weight;
        }
        if morph {
            for (_, origin) in self.morph.wire_alleles() {
                *out.entry(origin).or_default() += weight;
            }
        }
    }

    /// Alleles per origin, load loci included (what `tally_origins` adds up
    /// with weight 1), into `out` (cleared first), in order of first
    /// appearance. A genome holds a handful of origins, so this is a short
    /// linear scan instead of a map update per allele; adding the counts to
    /// a tally whose values are whole numbers gives the same values, bit
    /// for bit, as `tally_origins` with a whole-number weight.
    pub fn count_origins(&self, out: &mut Vec<(PlayerId, u32)>) {
        self.count_origins_with(out, false);
    }

    /// [`Self::count_origins`], and with `morph` the morphology segment's
    /// alleles too (what [`Self::tally_origins_with`] adds up with weight 1).
    pub fn count_origins_with(&self, out: &mut Vec<(PlayerId, u32)>, morph: bool) {
        out.clear();
        let mut add = |origin: PlayerId| match out.iter_mut().find(|(p, _)| *p == origin) {
            Some((_, n)) => *n += 1,
            None => out.push((origin, 1)),
        };
        for locus in self.loci.iter().chain(&self.load) {
            add(locus.a.origin);
            add(locus.b.origin);
        }
        if morph {
            for (_, origin) in self.morph.wire_alleles() {
                add(origin);
            }
        }
    }
}

pub(crate) fn mutate(allele: Allele, owner: PlayerId, m: MutationParams, rng: &mut impl Rng) -> Allele {
    if rng.gen::<f32>() < m.rate {
        Allele { value: (allele.value + normal(rng) * m.sd).clamp(0.0, 1.0), origin: owner }
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

/// Alleles in a genome: two per gene and per load locus.
pub const ALLELES: usize = 2 * (Gene::COUNT + LOAD_LOCI);

/// The compact, exact snapshot form of a [`Genome`] (feature `serde`): one
/// byte string of
///
/// 1. `n`, the number of distinct origin tags (u8), then those tags (u32
///    little-endian each) in order of first appearance;
/// 2. a 2-bit class per allele: `0` the value is exactly 0.0, `1` exactly
///    1.0, `2` bit for bit the previous allele's value, `3` stored;
/// 3. a bit stream (least significant bit first): each allele's origin as
///    an index into the tags, `ceil(log2 n)` bits (none when the genome has
///    one tag), then each stored value: values in `[2^-7, 1)` as a 3-bit
///    exponent (0–6, the f32 exponent less 120) and their 23-bit mantissa,
///    any other as `7` and its 32 bits.
///
/// Alleles run gene by gene (`a` then `b`), then over the load loci. Values
/// are kept to the bit (packed, not quantized), so a restored world
/// continues exactly. A founder genome (one tag, load loci 0.0) takes about
/// 31 bytes plus 3.25 per stored value, against 816 in memory.
///
/// From world snapshot version 23 the morphology segment follows
/// ([`MORPH_ALLELES`] alleles: the discrete loci, then the continuous ones,
/// `a` then `b`): its origins join the tags (counted in `n`, after the
/// others'), and after the stored values the bit stream holds per morphology
/// allele its origin index (`ceil(log2 n)` bits), then for a discrete allele
/// its value (4 bits), for a continuous one a 2-bit class as above (`2`: the
/// previous continuous allele's value) and, if stored, its packed value.
/// Before version 23 (`serde_util::before_v23`) the segment is left out and
/// reads as the contract defaults ([`MorphGenes::from_genes`]).
#[cfg(feature = "serde")]
pub mod wire {
    use serde::de::{Error, SeqAccess, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::*;
    use crate::morph::{wire_is_discrete, CORE_MORPH_ALLELES, MORPH_ALLELES};

    const ZERO: u32 = 0;
    const ONE: u32 = 0x3f80_0000;
    const CLASS_BYTES: usize = ALLELES.div_ceil(4);
    /// Lowest f32 exponent (sign included) of a packed value: 2^-7.
    const LOW_EXP: u32 = 120;
    const ESCAPE: u32 = 7;

    fn bits_for(tags: usize) -> u32 {
        usize::BITS - (tags.max(1) - 1).leading_zeros()
    }

    impl Genome {
        #[cfg(test)]
        pub(super) fn alleles(&self) -> impl Iterator<Item = &Allele> {
            self.alleles_of(Gene::COUNT)
        }

        /// The alleles of the first `genes` loci, then of the load loci.
        fn alleles_of(&self, genes: usize) -> impl Iterator<Item = &Allele> {
            self.loci[..genes].iter().chain(&self.load).flat_map(|l| [&l.a, &l.b])
        }

        #[cfg(test)]
        pub(super) fn alleles_mut(&mut self) -> impl Iterator<Item = &mut Allele> {
            self.alleles_mut_of(Gene::COUNT)
        }

        fn alleles_mut_of(&mut self, genes: usize) -> impl Iterator<Item = &mut Allele> {
            self.loci[..genes].iter_mut().chain(&mut self.load).flat_map(|l| [&mut l.a, &mut l.b])
        }

        /// The genome in its compact snapshot form (see [`wire`]).
        pub fn to_wire(&self) -> Vec<u8> {
            self.to_wire_of(Gene::COUNT, MORPH_ALLELES)
        }

        /// [`Genome::to_wire`] of the first `genes` gene loci only (the
        /// layout before `Gene::Obedience` with [`OLD_GENES`]), and the
        /// morphology segment if `morph` (from version 23).
        fn to_wire_of(&self, genes: usize, morph_n: usize) -> Vec<u8> {
            let morph = morph_n > 0;
            let alleles = 2 * (genes + LOAD_LOCI);
            let mut tags: Vec<PlayerId> = Vec::new();
            let mut index = [0u8; ALLELES];
            let mut classes = [0u8; CLASS_BYTES];
            let mut stored = [0u32; ALLELES];
            let (mut n_stored, mut prev) = (0, None);
            for (k, a) in self.alleles_of(genes).enumerate() {
                index[k] = match tags.iter().position(|&t| t == a.origin) {
                    Some(i) => i as u8,
                    None => {
                        tags.push(a.origin);
                        (tags.len() - 1) as u8
                    }
                };
                let v = a.value.to_bits();
                let class = match v {
                    ZERO => 0,
                    ONE => 1,
                    _ if prev == Some(v) => 2,
                    _ => {
                        stored[n_stored] = v;
                        n_stored += 1;
                        3
                    }
                };
                classes[k / 4] |= class << (2 * (k % 4));
                prev = Some(v);
            }
            let (index, classes) = (&index[..alleles], &classes[..alleles.div_ceil(4)]);
            let all_morph = self.morph.wire_alleles();
            let morph_alleles = &all_morph[..morph_n];
            let mut morph_index = [0u8; MORPH_ALLELES];
            if morph {
                for (k, &(_, origin)) in morph_alleles.iter().enumerate() {
                    morph_index[k] = match tags.iter().position(|&t| t == origin) {
                        Some(i) => i as u8,
                        None => {
                            tags.push(origin);
                            (tags.len() - 1) as u8
                        }
                    };
                }
            }
            let bits = bits_for(tags.len());
            let mut out =
                Vec::with_capacity(1 + 4 * tags.len() + classes.len() + alleles * bits as usize / 8 + 4 * n_stored);
            out.push(tags.len() as u8);
            for t in &tags {
                out.extend_from_slice(&t.to_le_bytes());
            }
            out.extend_from_slice(classes);
            let mut w = BitWriter { out, acc: 0, len: 0 };
            for &i in index {
                w.put(i as u32, bits);
            }
            for &v in &stored[..n_stored] {
                put_value(&mut w, v);
            }
            if morph {
                let mut prev = None;
                for (k, (&(v, _), &i)) in morph_alleles.iter().zip(&morph_index).enumerate() {
                    w.put(i as u32, bits);
                    if wire_is_discrete(k) {
                        w.put(v, 4);
                        continue;
                    }
                    match v {
                        ZERO => w.put(0, 2),
                        ONE => w.put(1, 2),
                        _ if prev == Some(v) => w.put(2, 2),
                        _ => {
                            w.put(3, 2);
                            put_value(&mut w, v);
                        }
                    }
                    prev = Some(v);
                }
            }
            w.finish()
        }

        /// A genome from [`Genome::to_wire`] bytes.
        pub fn from_wire(bytes: &[u8]) -> Result<Genome, String> {
            Self::from_wire_of(bytes, Gene::COUNT, MORPH_ALLELES)
        }

        /// A genome from bytes holding its first `genes` gene loci. With
        /// [`OLD_GENES`] (a snapshot from before `Gene::Obedience`), the
        /// missing obedience locus takes the design default (the value the
        /// commands were tuned at, so the organism behaves as before) and the
        /// origin tags of the organism's wanderlust alleles. Without `morph`
        /// (before version 23) the morphology segment takes the contract
        /// defaults ([`MorphGenes::from_genes`]).
        fn from_wire_of(bytes: &[u8], genes: usize, morph_n: usize) -> Result<Genome, String> {
            let morph = morph_n > 0;
            let (&n, rest) = bytes.split_first().ok_or("empty genome")?;
            let n = n as usize;
            let (tag_bytes, rest) = rest.split_at_checked(4 * n).ok_or("genome cut short (tags)")?;
            let tags: Vec<PlayerId> =
                tag_bytes.chunks_exact(4).map(|c| PlayerId::from_le_bytes(c.try_into().unwrap())).collect();
            let class_bytes = (2 * (genes + LOAD_LOCI)).div_ceil(4);
            let (classes, rest) = rest.split_at_checked(class_bytes).ok_or("genome cut short (classes)")?;
            let bits = bits_for(n);
            let mut r = BitReader { bytes: rest, at: 0, acc: 0, len: 0 };
            let blank = Locus { a: Allele { value: 0.0, origin: WILD }, b: Allele { value: 0.0, origin: WILD } };
            let mut g = Genome {
                loci: [blank; Gene::COUNT],
                load: [blank; LOAD_LOCI],
                morph: MorphGenes::uniform(&MorphDesign::default(), WILD),
            };
            for a in g.alleles_mut_of(genes) {
                let i = r.get(bits)? as usize;
                a.origin = *tags.get(i).ok_or("genome origin out of range")?;
            }
            let mut prev = None;
            for (k, a) in g.alleles_mut_of(genes).enumerate() {
                let v = match (classes[k / 4] >> (2 * (k % 4))) & 3 {
                    0 => ZERO,
                    1 => ONE,
                    2 => prev.ok_or("genome repeats a value before the first")?,
                    _ => get_value(&mut r)?,
                };
                prev = Some(v);
                a.value = f32::from_bits(v);
            }
            if morph {
                let mut m = [(0u32, WILD); MORPH_ALLELES];
                let mut prev = None;
                for (k, slot) in m[..morph_n].iter_mut().enumerate() {
                    let i = r.get(bits)? as usize;
                    slot.1 = *tags.get(i).ok_or("genome origin out of range")?;
                    if wire_is_discrete(k) {
                        slot.0 = r.get(4)?;
                        continue;
                    }
                    let v = match r.get(2)? {
                        0 => ZERO,
                        1 => ONE,
                        2 => prev.ok_or("genome repeats a value before the first")?,
                        _ => get_value(&mut r)?,
                    };
                    prev = Some(v);
                    slot.0 = v;
                }
                g.morph = MorphGenes::from_wire_alleles(&m[..morph_n])?;
                if morph_n < MORPH_ALLELES {
                    g.morph.reset_extra_from(&g.loci);
                }
            }
            if r.at != rest.len() || r.len >= 8 || r.acc != 0 {
                return Err("genome has trailing bytes".into());
            }
            if genes < Gene::COUNT {
                let w = g.loci[Gene::Wanderlust.index()];
                let v = crate::design::default_value(Gene::Obedience);
                g.loci[Gene::Obedience.index()] =
                    Locus { a: Allele { value: v, origin: w.a.origin }, b: Allele { value: v, origin: w.b.origin } };
            }
            if !morph {
                g.morph = MorphGenes::from_genes(&g.loci);
            }
            Ok(g)
        }
    }

    /// A stored value: a 3-bit exponent and the mantissa for values in the
    /// packed range, else [`ESCAPE`] and all 32 bits.
    fn put_value(w: &mut BitWriter, v: u32) {
        let exp = v >> 23;
        if (LOW_EXP..LOW_EXP + ESCAPE).contains(&exp) {
            w.put(exp - LOW_EXP, 3);
            w.put(v & 0x7f_ffff, 23);
        } else {
            w.put(ESCAPE, 3);
            w.put(v, 32);
        }
    }

    fn get_value(r: &mut BitReader) -> Result<u32, String> {
        Ok(match r.get(3)? {
            ESCAPE => r.get(32)?,
            e => ((e + LOW_EXP) << 23) | r.get(23)?,
        })
    }

    /// Most alleles a genome written against its parents may take from
    /// elsewhere (mutations, re-tagged genomes); with more it is written
    /// whole, which is then about as small.
    const MAX_EXCEPTIONS: usize = 16;
    /// Bytes of one exception: allele index, value bits, origin.
    const EXCEPTION_BYTES: usize = 1 + 4 + 4;
    const _: () = assert!(ALLELES <= 128, "a genome's choice bits fit a u128");

    /// An allele as one number: its value bits (so 0.0 and -0.0 differ) and
    /// its origin.
    fn key(a: &Allele) -> u64 {
        (a.value.to_bits() as u64) << 32 | a.origin as u64
    }

    fn allele(k: u64) -> Allele {
        Allele { value: f32::from_bits((k >> 32) as u32), origin: k as u32 }
    }

    impl Genome {
        /// Every allele as its [`key`], in [`wire`] order.
        pub(super) fn keys(&self) -> [u64; ALLELES] {
            let mut out = [0; ALLELES];
            for (l, locus) in self.loci.iter().chain(&self.load).enumerate() {
                (out[2 * l], out[2 * l + 1]) = (key(&locus.a), key(&locus.b));
            }
            out
        }

        /// The morphology segment's alleles as keys (value bits, origin).
        pub(super) fn morph_keys(&self) -> [u64; MORPH_ALLELES] {
            self.morph.wire_alleles().map(|(bits, origin)| (bits as u64) << 32 | origin as u64)
        }

        /// A genome from its [`Genome::keys`] and, if `morph` (from version
        /// 23), its [`Genome::morph_keys`]; without them the morphology
        /// segment takes the contract defaults.
        fn from_keys(k: &[u64], morph: Option<&[u64]>) -> Result<Genome, String> {
            let locus = |l: usize| Locus { a: allele(k[2 * l]), b: allele(k[2 * l + 1]) };
            let loci: [Locus; Gene::COUNT] = std::array::from_fn(locus);
            let morph = match morph {
                Some(m) => {
                    let v: Vec<(u32, PlayerId)> = m.iter().map(|&x| ((x >> 32) as u32, x as u32)).collect();
                    let mut g = MorphGenes::from_wire_alleles(&v)?;
                    if m.len() < MORPH_ALLELES {
                        g.reset_extra_from(&loci);
                    }
                    g
                }
                None => MorphGenes::from_genes(&loci),
            };
            Ok(Genome { loci, load: std::array::from_fn(|l| locus(Gene::COUNT + l)), morph })
        }

        /// The genome written against its parents' ([`Parents::encode`]),
        /// or `None` if it is not mostly inherited from them.
        pub fn to_wire_by_parents(&self, mother: &Genome, father: &Genome) -> Option<Vec<u8>> {
            Parents::new(mother, father).encode(self)
        }

        /// A genome from [`Genome::to_wire_by_parents`] bytes and the same
        /// parents' genomes.
        pub fn from_wire_by_parents(bytes: &[u8], mother: &Genome, father: &Genome) -> Result<Genome, String> {
            Parents::new(mother, father).decode(bytes)
        }
    }

    /// Two parents' genomes, to write their young's (a child's or an egg's)
    /// against (world snapshots from version 18). Inheritance gives allele
    /// `a` of every locus one of the mother's two alleles at that locus and
    /// allele `b` one of the father's ([`Genome::inherit`]), so each allele
    /// is a choice bit, and none where that parent's two alleles are the
    /// same. Bytes:
    ///
    /// 1. `e`, the number of exceptions (alleles that are neither of the
    ///    parent's: mutations), u8, then per exception its allele index (u8,
    ///    in [`wire`] order), value bits and origin (u32 little-endian
    ///    each);
    /// 2. the choice bits (least significant first; 0 the parent's `a`, 1
    ///    its `b`) of the other alleles in order, skipping those whose parent
    ///    is homozygous there, zero-padded to a byte.
    ///
    /// About 11 bytes for an egg or a newborn, against about 250 whole.
    ///
    /// From version 23 the morphology segment follows as a section of the
    /// same shape with its own exceptions and choice bits (allele indices
    /// counted within the segment, in [`MorphGenes::wire_alleles`] order, a
    /// discrete allele's value bits its integer), so the genes' choice bits
    /// keep their `u128` and later loci don't press on it. Before version 23
    /// there is no such section and the segment reads as the defaults.
    pub struct Parents {
        /// Per allele of a child, the two its parent had at its locus (the
        /// mother for an `a`, the father for a `b`), as [`key`]s.
        from: [[u64; 2]; ALLELES],
        /// The same for the morphology segment's alleles, if written.
        morph: Option<Vec<[u64; 2]>>,
    }

    /// Per allele of a child, the two alleles its parent has at that locus.
    fn choices(m: &[u64], f: &[u64]) -> Vec<[u64; 2]> {
        (0..m.len())
            .map(|k| {
                let p = if k % 2 == 0 { m } else { f };
                [p[k & !1], p[k | 1]]
            })
            .collect()
    }

    /// `child` against `from` (one section: exceptions, then choice bits),
    /// appended to `out`; `false` if it has too many exceptions.
    fn encode_section(child: &[u64], from: &[[u64; 2]], out: &mut Vec<u8>) -> bool {
        let mut exceptions = [0u8; MAX_EXCEPTIONS];
        let (mut e, mut bits, mut n) = (0, 0u128, 0usize);
        for (k, (&c, &[a, b])) in child.iter().zip(from).enumerate() {
            if c == a || c == b {
                if a != b {
                    bits |= u128::from(c != a) << n;
                    n += 1;
                }
            } else if e == MAX_EXCEPTIONS {
                return false;
            } else {
                exceptions[e] = k as u8;
                e += 1;
            }
        }
        out.push(e as u8);
        for &k in &exceptions[..e] {
            let a = allele(child[k as usize]);
            out.push(k);
            out.extend_from_slice(&a.value.to_bits().to_le_bytes());
            out.extend_from_slice(&a.origin.to_le_bytes());
        }
        out.extend_from_slice(&bits.to_le_bytes()[..n.div_ceil(8)]);
        true
    }

    /// One section from the front of `bytes` ([`encode_section`]): the
    /// child's keys and the rest of the bytes.
    fn decode_section<'a>(bytes: &'a [u8], from: &[[u64; 2]]) -> Result<(Vec<u64>, &'a [u8]), String> {
        let n_alleles = from.len();
        let (&e, mut rest) = bytes.split_first().ok_or("empty genome")?;
        let mut child = vec![0u64; n_alleles];
        let mut exception = vec![false; n_alleles];
        let mut last = None;
        for _ in 0..e {
            let (x, r) = rest.split_at_checked(EXCEPTION_BYTES).ok_or("genome cut short (exceptions)")?;
            rest = r;
            let k = x[0] as usize;
            if k >= n_alleles || last.is_some_and(|l| k <= l) {
                return Err("genome exception out of order".into());
            }
            last = Some(k);
            let value = f32::from_bits(u32::from_le_bytes(x[1..5].try_into().unwrap()));
            child[k] = key(&Allele { value, origin: PlayerId::from_le_bytes(x[5..9].try_into().unwrap()) });
            exception[k] = true;
        }
        let n = (0..n_alleles).filter(|&k| !exception[k] && from[k][0] != from[k][1]).count();
        let (choice, rest) =
            rest.split_at_checked(n.div_ceil(8)).ok_or("genome has the wrong number of choice bytes")?;
        let mut buf = [0u8; 16];
        buf[..choice.len()].copy_from_slice(choice);
        let mut bits = u128::from_le_bytes(buf);
        if n < 128 && bits >> n != 0 {
            return Err("genome has trailing bits".into());
        }
        for (k, c) in child.iter_mut().enumerate() {
            if exception[k] {
                continue;
            }
            let [a, b] = from[k];
            *c = if a == b {
                a
            } else {
                let pick = if bits & 1 == 0 { a } else { b };
                bits >>= 1;
                pick
            };
        }
        Ok((child, rest))
    }

    impl Parents {
        pub fn new(mother: &Genome, father: &Genome) -> Parents {
            let n = stored_morph();
            let from: [[u64; 2]; ALLELES] = choices(&mother.keys(), &father.keys()).try_into().unwrap();
            Parents { from, morph: (n > 0).then(|| choices(&mother.morph_keys()[..n], &father.morph_keys()[..n])) }
        }

        /// `g` against these parents, or `None` if more than a few of its
        /// alleles are not inherited unchanged from them (it is then written
        /// whole). At most [`MAX_EXCEPTIONS`] per section.
        pub fn encode(&self, g: &Genome) -> Option<Vec<u8>> {
            let mut out = Vec::with_capacity(16);
            if !encode_section(&g.keys(), &self.from, &mut out) {
                return None;
            }
            if let Some(from) = &self.morph {
                if !encode_section(&g.morph_keys()[..from.len()], from, &mut out) {
                    return None;
                }
            }
            Some(out)
        }

        /// A genome from [`Parents::encode`] bytes.
        pub fn decode(&self, bytes: &[u8]) -> Result<Genome, String> {
            let (child, rest) = decode_section(bytes, &self.from)?;
            let (morph, rest) = match &self.morph {
                Some(from) => {
                    let (m, rest) = decode_section(rest, from)?;
                    (Some(m), rest)
                }
                None => (None, rest),
            };
            if !rest.is_empty() {
                return Err("genome has the wrong number of choice bytes".into());
            }
            Genome::from_keys(&child, morph.as_deref())
        }
    }

    struct BitWriter {
        out: Vec<u8>,
        acc: u64,
        len: u32,
    }

    impl BitWriter {
        fn put(&mut self, v: u32, bits: u32) {
            debug_assert!(bits <= 32 && (bits == 32 || v >> bits == 0));
            self.acc |= (v as u64) << self.len;
            self.len += bits;
            while self.len >= 8 {
                self.out.push(self.acc as u8);
                self.acc >>= 8;
                self.len -= 8;
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.len > 0 {
                self.out.push(self.acc as u8);
            }
            self.out
        }
    }

    struct BitReader<'a> {
        bytes: &'a [u8],
        /// Bytes taken into `acc`.
        at: usize,
        acc: u64,
        len: u32,
    }

    impl BitReader<'_> {
        fn get(&mut self, bits: u32) -> Result<u32, String> {
            while self.len < bits {
                let &b = self.bytes.get(self.at).ok_or("genome cut short")?;
                self.acc |= (b as u64) << self.len;
                self.at += 1;
                self.len += 8;
            }
            let v = (self.acc & ((1u64 << bits) - 1)) as u32;
            self.acc >>= bits;
            self.len -= bits;
            Ok(v)
        }
    }

    /// Gene loci in a genome's wire form before `Gene::Obedience` (world
    /// snapshots before version 14): every gene but that last one.
    pub const OLD_GENES: usize = Gene::COUNT - 1;
    const _: () = assert!(Gene::Obedience as usize == OLD_GENES, "obedience is the last gene");

    /// Gene loci written and read on this thread: [`OLD_GENES`] while a
    /// `serde_util::before_v14::Guard` lives.
    fn stored_genes() -> usize {
        if crate::serde_util::before_v14::active() {
            OLD_GENES
        } else {
            Gene::COUNT
        }
    }

    /// Morphology alleles written and read on this thread: none while a
    /// `serde_util::before_v23::Guard` lives, the core loci only
    /// ([`CORE_MORPH_ALLELES`]) while a `serde_util::before_v29::Guard`
    /// does, else all of them.
    fn stored_morph() -> usize {
        if crate::serde_util::before_v23::active() {
            0
        } else if crate::serde_util::before_v29::active() {
            CORE_MORPH_ALLELES
        } else {
            MORPH_ALLELES
        }
    }

    impl Serialize for Genome {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_bytes(&self.to_wire_of(stored_genes(), stored_morph()))
        }
    }

    struct WireVisitor;

    impl<'de> Visitor<'de> for WireVisitor {
        type Value = Genome;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a compact genome (bytes)")
        }

        fn visit_bytes<E: Error>(self, v: &[u8]) -> Result<Genome, E> {
            Genome::from_wire_of(v, stored_genes(), stored_morph()).map_err(E::custom)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Genome, A::Error> {
            let mut v = Vec::new();
            while let Some(b) = seq.next_element::<u8>()? {
                v.push(b);
            }
            self.visit_bytes(&v)
        }
    }

    impl<'de> Deserialize<'de> for Genome {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Genome, D::Error> {
            d.deserialize_bytes(WireVisitor)
        }
    }
}

/// Expressed gene values.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Phenotype(#[cfg_attr(feature = "serde", serde(with = "crate::serde_util::array"))] pub [f32; Gene::COUNT]);

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

    /// `client/src/ui/traitNames.fixture.json`: every gene's wire key and
    /// [`trait_display_name`], which the client's `traitNames.test.ts`
    /// checks against `TRAIT_NAMES`, so the two maps cannot drift. Rerun
    /// with `UPDATE_TRAIT_NAMES_FIXTURE=1` after renaming a trait (and
    /// rename it in `traitNames.ts` too).
    #[test]
    fn the_client_trait_name_fixture_is_current() {
        // Genes, then the morphology parameters (the client's `MORPH_NAMES`).
        let morph = crate::morph::MorphLocus::ALL
            .iter()
            .map(|l| (l.key().to_string(), l.display_name()))
            .chain(crate::morph::MorphGene::ALL.iter().map(|g| (g.key().to_string(), g.display_name())));
        let rows: Vec<String> = Gene::ALL
            .iter()
            .map(|&g| (g.key(), trait_display_name(g)))
            .chain(morph)
            .map(|(key, name)| {
                assert!(!name.contains(['"', '\\']), "{name}");
                format!("  \"{key}\": \"{name}\"")
            })
            .collect();
        let text = format!("{{\n{}\n}}\n", rows.join(",\n"));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/src/ui/traitNames.fixture.json");
        if std::env::var_os("UPDATE_TRAIT_NAMES_FIXTURE").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == text, "{} is stale: rerun with UPDATE_TRAIT_NAMES_FIXTURE=1", path.display());
    }

    #[test]
    fn display_names_name_the_axis_and_lower_for_sentences() {
        assert_eq!(Gene::Ears.display_name(), "Ear size");
        assert_eq!(Gene::Nose.display_name(), "Sense of smell");
        assert_eq!(Gene::Armor.display_name(), "Armor");
        assert_eq!(Gene::Ears.key(), "large_ears");
        assert_eq!(Gene::GroupTolerance.display_name_lower(), "sociability");
        assert_eq!(Gene::Endothermy.display_name_lower(), "warm-bloodedness");
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
        assert!((a.distance(&b) - 0.3).abs() < 1e-5);
        // Seasonal strategies don't count.
        let mut v = flat(0.2);
        v[Gene::Migration.index()] = 1.0;
        v[Gene::SeasonalCoat.index()] = 1.0;
        v[Gene::TimedBreeding.index()] = 1.0;
        assert_eq!(Genome::uniform(&v, 1).distance(&a), 0.0);
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
        let counted = Gene::ALL.iter().filter(|&&g| IN_DISTANCE[g.index()]);
        let manual: f32 = counted.map(|&g| (f1.expr(g) - dull.expr(g)).abs()).sum::<f32>() / DISTANCE_GENES;
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
            let mut pair = (
                Genome::inherit(&stud, &clean, 1, none, 0.0, &mut rng),
                Genome::inherit(&stud, &clean, 1, none, 0.0, &mut rng),
            );
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

    /// The compact snapshot form keeps every allele to the bit, with any
    /// number of origin tags.
    #[cfg(feature = "serde")]
    #[test]
    fn wire_form_is_exact_and_compact() {
        let mut rng = rng_for(4, 0, 0, 0);
        let mut v = flat(0.3);
        v[Gene::Migration.index()] = 0.0;
        v[Gene::Fur.index()] = 1.0;
        let founder = Genome::founder(&v, 3, 0.05, &mut rng);
        let other = Genome::founder(&flat(0.6), 8, 0.05, &mut rng);
        let always = MutationParams { rate: 1.0, sd: 0.1 };
        let mut hybrid = Genome::inherit(&founder, &other, 5, MutationParams { rate: 0.2, sd: 0.1 }, 0.5, &mut rng);
        let three = hybrid.clone();
        hybrid.set_load(2, true, Allele { value: 0.25, origin: u32::MAX });
        hybrid.loci[4].a.value = -0.0;
        hybrid.loci[5].b.value = 1e-9;
        hybrid.loci[6].a.value = f32::from_bits(0x3f7f_ffff);
        // As many tags as alleles: 7 bits each.
        let mut many = Genome::inherit(&founder, &other, 1, always, 0.0, &mut rng);
        for (k, a) in many.alleles_mut().enumerate() {
            a.origin = 1000 + k as u32;
        }
        let uniform = Genome::uniform(&flat(0.5), 2);
        for g in [&founder, &other, &hybrid, &three, &many, &uniform] {
            let w = g.to_wire();
            let back = Genome::from_wire(&w).unwrap();
            let bits = |g: &Genome| g.alleles().map(|a| (a.value.to_bits(), a.origin)).collect::<Vec<_>>();
            assert_eq!(bits(&back), bits(g));
            assert!(Genome::from_wire(&w[..w.len() - 1]).is_err());
            let mut long = w.clone();
            long.push(0);
            assert!(Genome::from_wire(&long).is_err());
            let raw: Vec<u8> = bincode::serialize(g).unwrap();
            assert_eq!(bincode::deserialize::<Genome>(&raw).unwrap(), *g);
        }
        // One tag (no origin bits), load loci 0.0 (not stored), and 0.5
        // stored once (26 bits) then repeated; then the morphology segment:
        // 4 bits per discrete allele, and per continuous locus a stored
        // value (2 + 26 bits) and a repeat (2 bits), except horn size, two
        // zeros (2 bits each).
        let morph_bits: usize = 4 * 14 + 3 * (2 + 26 + 2) + 2 * 2;
        assert_eq!(uniform.to_wire().len(), 1 + 4 + ALLELES.div_ceil(4) + (26 + morph_bits).div_ceil(8));
        assert!(
            founder.to_wire().len()
                <= 1 + 4 + ALLELES.div_ceil(4) + (35 * 2 * Gene::COUNT + 4 * 14 + 8 * 28).div_ceil(8)
        );
        // The morphology segment round-trips, mixed tags and all.
        let mut odd = hybrid.clone();
        let mut m = *odd.morph();
        m.discrete[3].a = crate::morph::MorphAllele { value: 8, origin: 77 };
        m.discrete[4].b = crate::morph::MorphAllele { value: 4, origin: 3 };
        m.continuous[1].b = Allele { value: 1.0, origin: 78 };
        m.continuous[0].a = Allele { value: -0.0, origin: 8 };
        odd.set_morph(m);
        assert_eq!(Genome::from_wire(&odd.to_wire()).unwrap(), odd);
        // A discrete value out of its range is refused.
        m.discrete[3].a.value = 5;
        odd.set_morph(m);
        assert!(Genome::from_wire(&odd.to_wire()).is_err());
        // An origin index beyond the tags is refused.
        let mut w = three.to_wire();
        let n = w[0] as usize;
        assert_eq!(n, 3, "tags 3, 8 and 5");
        w[1 + 4 * n + ALLELES.div_ceil(4)] |= 0xff;
        assert!(Genome::from_wire(&w).is_err());
    }

    /// A child's or an egg's genome written against its parents' comes back
    /// to the bit: inherited alleles as choice bits (none where the parent
    /// is homozygous), mutations and odd values as exceptions; a genome
    /// with too many of those is left to the whole form.
    #[cfg(feature = "serde")]
    #[test]
    fn wire_form_by_parents_is_exact() {
        let bits = |g: &Genome| g.alleles().map(|a| (a.value.to_bits(), a.origin)).collect::<Vec<_>>();
        let mut rng = rng_for(5, 0, 0, 0);
        let mut v = flat(0.4);
        v[Gene::Migration.index()] = 0.0;
        let mother = Genome::founder(&v, 3, 0.05, &mut rng);
        let mut father = Genome::founder(&flat(0.6), 8, 0.05, &mut rng);
        father.set_load(1, true, Allele::harmful(8));
        let none = MutationParams { rate: 0.0, sd: 0.0 };
        let some = MutationParams { rate: 0.05, sd: 0.1 };
        let plain = Genome::inherit(&mother, &father, 3, none, 0.0, &mut rng);
        let mutated = Genome::inherit(&mother, &father, 3, some, 0.2, &mut rng);
        let mut odd = plain.clone();
        odd.loci[2].a.value = -0.0;
        odd.loci[3].b.value = f32::NAN;
        odd.loci[4].a.origin = 99;
        odd.load[3].b = Allele { value: 1e-30, origin: u32::MAX };
        let mut cases = 0;
        for g in [&plain, &mutated, &odd, &mother] {
            let Some(w) = g.to_wire_by_parents(&mother, &father) else { continue };
            cases += 1;
            let back = Genome::from_wire_by_parents(&w, &mother, &father).unwrap();
            assert_eq!(bits(&back), bits(g));
            assert!(Genome::from_wire_by_parents(&w[..w.len() - 1], &mother, &father).is_err());
            let mut long = w.clone();
            long.push(0);
            assert!(Genome::from_wire_by_parents(&long, &mother, &father).is_err());
        }
        assert!(cases >= 3, "plain, mutated and odd are written against the parents");
        // No exceptions: a byte, then a bit per allele whose parent is
        // heterozygous there (most genes of a founder, one load locus of
        // the father's).
        let w = plain.to_wire_by_parents(&mother, &father).unwrap();
        assert_eq!(w[0], 0);
        let het = (0..ALLELES).filter(|&k| {
            let p = if k % 2 == 0 { mother.keys() } else { father.keys() };
            p[k & !1] != p[k | 1]
        });
        // Then the morphology section: no exceptions, a bit per allele whose
        // parent is heterozygous there (the continuous loci of founders).
        let morph_het = (0..crate::morph::MORPH_ALLELES).filter(|&k| {
            let p = if k % 2 == 0 { mother.morph_keys() } else { father.morph_keys() };
            p[k & !1] != p[k | 1]
        });
        let (genes, morph) = (het.count().div_ceil(8), morph_het.count().div_ceil(8));
        assert!(morph > 0);
        assert_eq!(w.len(), 1 + genes + 1 + morph);
        assert_eq!(w[1 + genes], 0);
        // A mutated morphology allele is an exception of its own section.
        let mut mutant = plain.clone();
        let mut m = *mutant.morph();
        m.discrete[0].a = crate::morph::MorphAllele { value: 6, origin: 3 };
        mutant.set_morph(m);
        let wm = mutant.to_wire_by_parents(&mother, &father).unwrap();
        assert_eq!((wm[0], wm[1 + genes]), (0, 1));
        assert_eq!(Genome::from_wire_by_parents(&wm, &mother, &father).unwrap(), mutant);
        assert_eq!(odd.to_wire_by_parents(&mother, &father).unwrap()[0], 4);
        // Unrelated genomes are mostly exceptions: written whole instead.
        let stranger = Genome::founder(&flat(0.5), 4, 0.05, &mut rng);
        assert!(stranger.to_wire_by_parents(&mother, &father).is_none());
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
