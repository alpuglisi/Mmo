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

/// Which genes count toward [`Genome::distance`], by gene index.
pub const IN_DISTANCE: [bool; Gene::COUNT] = {
    let mut mask = [true; Gene::COUNT];
    let mut k = 0;
    while k < SEASONAL_GENES.len() {
        mask[SEASONAL_GENES[k] as usize] = false;
        k += 1;
    }
    mask
};

/// Number of genes [`Genome::distance`] averages over.
pub const DISTANCE_GENES: f32 = (Gene::COUNT - SEASONAL_GENES.len()) as f32;

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
}

impl Genome {
    /// Every allele set exactly to the given values.
    pub fn uniform(values: &[f32; Gene::COUNT], origin: PlayerId) -> Self {
        let mut loci = [Locus { a: Allele { value: 0.0, origin }, b: Allele { value: 0.0, origin } }; Gene::COUNT];
        for (locus, &v) in loci.iter_mut().zip(values) {
            let allele = Allele { value: v.clamp(0.0, 1.0), origin };
            *locus = Locus { a: allele, b: allele };
        }
        let functional = Allele { value: 0.0, origin };
        Self { loci, load: [Locus { a: functional, b: functional }; LOAD_LOCI] }
    }

    /// A founder: each allele varies around the design value so selection has
    /// material from day one.
    pub fn founder(values: &[f32; Gene::COUNT], origin: PlayerId, variation: f32, rng: &mut impl Rng) -> Self {
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
            *locus =
                Locus { a: mutate(from_m, child_owner, mutation, rng), b: mutate(from_f, child_owner, mutation, rng) };
        }
        Genome { loci, load: mother.load }
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
    /// [`IN_DISTANCE`] (seasonal genes and load loci excluded). Used for
    /// kin/species recognition and outbreeding depression.
    pub fn distance(&self, other: &Genome) -> f32 {
        let sum: f32 = self
            .loci
            .iter()
            .zip(&other.loci)
            .zip(&DOMINANCE)
            .zip(IN_DISTANCE)
            .map(|(((a, b), &h), counts)| if counts { (a.expressed(h) - b.expressed(h)).abs() } else { 0.0 })
            .sum();
        sum / DISTANCE_GENES
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
#[cfg(feature = "serde")]
pub mod wire {
    use serde::de::{Error, SeqAccess, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::*;

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
        pub(super) fn alleles(&self) -> impl Iterator<Item = &Allele> {
            self.loci.iter().chain(&self.load).flat_map(|l| [&l.a, &l.b])
        }

        pub(super) fn alleles_mut(&mut self) -> impl Iterator<Item = &mut Allele> {
            self.loci.iter_mut().chain(&mut self.load).flat_map(|l| [&mut l.a, &mut l.b])
        }

        /// The genome in its compact snapshot form (see [`wire`]).
        pub fn to_wire(&self) -> Vec<u8> {
            let mut tags: Vec<PlayerId> = Vec::new();
            let mut index = [0u8; ALLELES];
            let mut classes = [0u8; CLASS_BYTES];
            let mut stored = [0u32; ALLELES];
            let (mut n_stored, mut prev) = (0, None);
            for (k, a) in self.alleles().enumerate() {
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
            let bits = bits_for(tags.len());
            let mut out =
                Vec::with_capacity(1 + 4 * tags.len() + CLASS_BYTES + ALLELES * bits as usize / 8 + 4 * n_stored);
            out.push(tags.len() as u8);
            for t in &tags {
                out.extend_from_slice(&t.to_le_bytes());
            }
            out.extend_from_slice(&classes);
            let mut w = BitWriter { out, acc: 0, len: 0 };
            for &i in &index {
                w.put(i as u32, bits);
            }
            for &v in &stored[..n_stored] {
                let exp = v >> 23;
                if (LOW_EXP..LOW_EXP + ESCAPE).contains(&exp) {
                    w.put(exp - LOW_EXP, 3);
                    w.put(v & 0x7f_ffff, 23);
                } else {
                    w.put(ESCAPE, 3);
                    w.put(v, 32);
                }
            }
            w.finish()
        }

        /// A genome from [`Genome::to_wire`] bytes.
        pub fn from_wire(bytes: &[u8]) -> Result<Genome, String> {
            let (&n, rest) = bytes.split_first().ok_or("empty genome")?;
            let n = n as usize;
            let (tag_bytes, rest) = rest.split_at_checked(4 * n).ok_or("genome cut short (tags)")?;
            let tags: Vec<PlayerId> =
                tag_bytes.chunks_exact(4).map(|c| PlayerId::from_le_bytes(c.try_into().unwrap())).collect();
            let (classes, rest) = rest.split_at_checked(CLASS_BYTES).ok_or("genome cut short (classes)")?;
            let bits = bits_for(n);
            let mut r = BitReader { bytes: rest, at: 0, acc: 0, len: 0 };
            let blank = Locus { a: Allele { value: 0.0, origin: WILD }, b: Allele { value: 0.0, origin: WILD } };
            let mut g = Genome { loci: [blank; Gene::COUNT], load: [blank; LOAD_LOCI] };
            for a in g.alleles_mut() {
                let i = r.get(bits)? as usize;
                a.origin = *tags.get(i).ok_or("genome origin out of range")?;
            }
            let mut prev = None;
            for (k, a) in g.alleles_mut().enumerate() {
                let v = match (classes[k / 4] >> (2 * (k % 4))) & 3 {
                    0 => ZERO,
                    1 => ONE,
                    2 => prev.ok_or("genome repeats a value before the first")?,
                    _ => match r.get(3)? {
                        ESCAPE => r.get(32)?,
                        e => ((e + LOW_EXP) << 23) | r.get(23)?,
                    },
                };
                prev = Some(v);
                a.value = f32::from_bits(v);
            }
            if r.at != rest.len() || r.len >= 8 || r.acc != 0 {
                return Err("genome has trailing bytes".into());
            }
            Ok(g)
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

    impl Serialize for Genome {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_bytes(&self.to_wire())
        }
    }

    struct WireVisitor;

    impl<'de> Visitor<'de> for WireVisitor {
        type Value = Genome;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a compact genome (bytes)")
        }

        fn visit_bytes<E: Error>(self, v: &[u8]) -> Result<Genome, E> {
            Genome::from_wire(v).map_err(E::custom)
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
        // stored once (26 bits) then repeated.
        assert_eq!(uniform.to_wire().len(), 1 + 4 + ALLELES.div_ceil(4) + 4);
        assert!(founder.to_wire().len() <= 1 + 4 + ALLELES.div_ceil(4) + (35 * 2 * Gene::COUNT).div_ceil(8));
        // An origin index beyond the tags is refused.
        let mut w = three.to_wire();
        let n = w[0] as usize;
        assert_eq!(n, 3, "tags 3, 8 and 5");
        w[1 + 4 * n + ALLELES.div_ceil(4)] |= 0xff;
        assert!(Genome::from_wire(&w).is_err());
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
