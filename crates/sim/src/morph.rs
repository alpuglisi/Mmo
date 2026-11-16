//! Morphology loci: the body-plan features of `docs/morphology-contract.md`
//! (v1) that the 0–1 genes don't cover (eye, ear and leg counts, ear and tail
//! shapes, eye placement, snout length), their encoding in the genome and the
//! design, and the expressed [`Morphology`] other packages read.
//!
//! **Encoding.** Every genome carries a separate segment, [`MorphGenes`],
//! beside its genes (own wire field, own choice bits when written against
//! the parents, own random stream):
//!
//! - five **discrete loci** ([`MorphLocus`]): `eye_count` (1–6),
//!   `ear_count` (0, 2 or 4), `ear_shape` ([`EarShape`]), `leg_count` (0, 2,
//!   4, 6 or 8) and `tail_shape` ([`TailShape`]). An allele is a small integer
//!   (the count, or the shape's code) with an origin tag, like any allele.
//!   Expression picks **one** allele, never an average (no "2.5 legs"), by the
//!   contract's dominance rule ([`MorphLocus::dominance_key`]): the default
//!   (wild type: 2 eyes, 2 ears, round ears, 4 legs, a thin tail) is
//!   expressed over any other; between two others the one nearer the default
//!   wins, the lower count on a tie; shapes in a fixed order (ears round >
//!   pointed > long > fan, tails thin > bushy > prehensile > fin > club). So
//!   new mutations hide in carriers, a designed variant crossed with a
//!   default lineage shows the default in the first generation, and the
//!   expression never depends on which parent gave which allele.
//! - two **continuous loci** ([`MorphGene`]): `eye_azimuth` and
//!   `snout_length`, ordinary additive [`Locus`] alleles in `[0, 1]`
//!   (`eye_azimuth` is degrees / 180), mutated like the genes.
//!
//! `tail_present` has no locus: it is derived from the `Tail` gene (tail
//! length ≥ [`TAIL_PRESENT`], as the contract says), so existing lineages keep
//! the tails they had and a design gets one by buying length.
//!
//! **Mutation.** The continuous alleles mutate at inheritance exactly like
//! gene alleles (`MutationParams`). Each discrete allele mutates with
//! probability `Balance::morph_step_rate` (`Genome::mutate_morph`, called by
//! the world after inheritance): a count moves one step (legs and ears two)
//! up or down, a shape to an adjacent one in its dominance order; at the end
//! of the range it moves inward, so a mutation always changes the allele. The
//! spec recommends a rate of 0.0005; the default is **0 (off)**: an expressed
//! mutant count changes kinship and fertility, so with it on no existing
//! world would be guaranteed to run bit for bit as before (the calibrated
//! sweep, `evo-sim sweep --size 96 --trials 1 --seeds 12 --rounds 1 --seed
//! 31`, prints 11.76 at 0 and at 0.0005 too, but an earlier variant drawing
//! the same rate from another stream printed 11.77). A balance package turns
//! it on. Mutations are
//! tagged with the offspring's owner. All of it draws from a stream of the
//! genome's own (`Genome::morph_rng`), never from the shared streams, so a
//! world founds, breeds and mutates its genes bit for bit as before these
//! loci existed.
//!
//! **Founders** get the design's discrete values exactly (both alleles, no
//! variation) and continuous alleles varied around the design value by the
//! usual founder variation (on the genome's own stream too).
//!
//! **Kin, species and fertility.** Organisms are compared on a [`MorphKey`]
//! (the expressed discrete loci as positions: steps from the bottom of the
//! range for counts, rank in the dominance order for shapes);
//! [`morph_terms`] gives
//!
//! - a **distance term** added to the numerator of `Genome::distance` (the
//!   divisor `DISTANCE_GENES` is unchanged): `min(1, steps / 2)` per discrete
//!   locus, as the spec says;
//! - a **body-plan gap**, [`BODY_PLAN_STEP`] (0.1) per step of `leg_count`
//!   or `eye_count` difference, which `mating::body_plan_gap` takes the
//!   largest of with the body-plan genes: one step stays fertile, two halve
//!   fertility, three are sterile and not kin.
//!
//! Both are exactly 0 between equal morphologies, so a default-morphology
//! world's distances, gaps, kinship and species are what they were, bit for
//! bit. **Deviation from the spec:** the spec also adds the continuous loci
//! (`eye_azimuth`, `snout_length`) to the distance with weight 1. That can't
//! leave existing worlds bit-identical (their founders vary on those loci and
//! the loci mutate at the gene rate), so they stay out of the distance here,
//! like the seasonal genes; a balance package can add them.
//!
//! **Legacy.** Morphology alleles carry origin tags but are not counted in
//! `Genome::tally_origins` / `count_origins` yet (the spec counts them):
//! counting them shifts every existing legacy share. A balance package
//! decides, with the season-boundary deploy the spec asks for.
//!
//! **Effects.** [`Morphology`] is the one body plan every package reads: it
//! is computed at birth (`Organism::morph`), `stats::derive_with` builds the
//! eye geometry and hearing context from it, `morph_effects` takes it as its
//! `BodyPlan` (ears, legs, tail shape, snout; behind the `morph_*` tunables)
//! and the server's appearance shows it. Kinship and fertility read its key.

use rand::Rng;

use crate::genome::{Allele, Gene, Locus, Phenotype, PlayerId};

/// Ear shapes. Codes (the allele values) in the order listed, which is also
/// the dominance order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum EarShape {
    #[default]
    Round,
    Pointed,
    Long,
    Fan,
}

impl EarShape {
    pub const ALL: [EarShape; 4] = [EarShape::Round, EarShape::Pointed, EarShape::Long, EarShape::Fan];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Option<EarShape> {
        Self::ALL.get(c as usize).copied()
    }

    /// Wire name (`"round"`, `"pointed"`, …).
    pub fn key(self) -> &'static str {
        match self {
            EarShape::Round => "round",
            EarShape::Pointed => "pointed",
            EarShape::Long => "long",
            EarShape::Fan => "fan",
        }
    }

    pub fn from_key(s: &str) -> Option<EarShape> {
        Self::ALL.into_iter().find(|e| e.key() == s)
    }
}

/// Tail shapes. Codes (the allele values) in the order listed (the
/// contract's); the dominance order is [`TailShape::DOMINANCE`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum TailShape {
    #[default]
    Thin,
    Bushy,
    Fin,
    Club,
    Prehensile,
}

impl TailShape {
    pub const ALL: [TailShape; 5] =
        [TailShape::Thin, TailShape::Bushy, TailShape::Fin, TailShape::Club, TailShape::Prehensile];
    /// Most dominant first (contract v1).
    pub const DOMINANCE: [TailShape; 5] =
        [TailShape::Thin, TailShape::Bushy, TailShape::Prehensile, TailShape::Fin, TailShape::Club];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Option<TailShape> {
        Self::ALL.get(c as usize).copied()
    }

    /// Wire name (`"thin"`, `"bushy"`, …).
    pub fn key(self) -> &'static str {
        match self {
            TailShape::Thin => "thin",
            TailShape::Bushy => "bushy",
            TailShape::Fin => "fin",
            TailShape::Club => "club",
            TailShape::Prehensile => "prehensile",
        }
    }

    pub fn from_key(s: &str) -> Option<TailShape> {
        Self::ALL.into_iter().find(|e| e.key() == s)
    }
}

/// Feet, derived from the `Hooves`, `Claws` and `FootWidth` genes: the
/// strongest of the three if it shows (≥ [`FOOT_SHOWS`]; ties in that
/// order), else plain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Foot {
    #[default]
    Plain,
    Hooves,
    Claws,
    Webbed,
}

/// Expression from which hooves, claws or wide feet show (the threshold the
/// server's appearance classes use for visible genes).
pub const FOOT_SHOWS: f32 = 0.5;
/// Tail length from which an organism has a tail (`tail_present`).
pub const TAIL_PRESENT: f32 = 0.05;
/// Fang size below which an organism shows no fangs.
pub const FANGS_SHOW: f32 = 0.1;

/// Discrete morphology loci, in genome (and wire) order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MorphLocus {
    EyeCount,
    EarCount,
    EarShape,
    LegCount,
    TailShape,
}

impl MorphLocus {
    pub const ALL: [MorphLocus; 5] =
        [MorphLocus::EyeCount, MorphLocus::EarCount, MorphLocus::EarShape, MorphLocus::LegCount, MorphLocus::TailShape];
    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    /// Wire name, as in the contract.
    pub fn key(self) -> &'static str {
        match self {
            MorphLocus::EyeCount => "eye_count",
            MorphLocus::EarCount => "ear_count",
            MorphLocus::EarShape => "ear_shape",
            MorphLocus::LegCount => "leg_count",
            MorphLocus::TailShape => "tail_shape",
        }
    }

    /// The contract default (the wild type).
    pub const fn default_value(self) -> u8 {
        match self {
            MorphLocus::EyeCount => 2,
            MorphLocus::EarCount => 2,
            MorphLocus::EarShape => 0,
            MorphLocus::LegCount => 4,
            MorphLocus::TailShape => 0,
        }
    }

    /// Smallest and largest allele value.
    pub const fn range(self) -> (u8, u8) {
        match self {
            MorphLocus::EyeCount => (1, 6),
            MorphLocus::EarCount => (0, 4),
            MorphLocus::EarShape => (0, EarShape::ALL.len() as u8 - 1),
            MorphLocus::LegCount => (0, 8),
            MorphLocus::TailShape => (0, TailShape::ALL.len() as u8 - 1),
        }
    }

    /// A count's step (legs and ears come in pairs); 1 for a shape.
    pub const fn step(self) -> u8 {
        match self {
            MorphLocus::LegCount | MorphLocus::EarCount => 2,
            _ => 1,
        }
    }

    /// A shape (enum) rather than a count.
    pub const fn is_shape(self) -> bool {
        matches!(self, MorphLocus::EarShape | MorphLocus::TailShape)
    }

    pub fn is_valid(self, v: u8) -> bool {
        let (lo, hi) = self.range();
        (lo..=hi).contains(&v) && (v - lo).is_multiple_of(self.step())
    }

    /// Position of a valid value on the locus's axis: for a count the steps
    /// from the bottom of the range, for a shape its rank in the dominance
    /// order (0 the default). Steps between alleles are differences of
    /// positions.
    pub fn position(self, v: u8) -> u8 {
        match self {
            MorphLocus::TailShape => {
                TailShape::DOMINANCE.iter().position(|t| t.code() == v).unwrap_or(v as usize) as u8
            }
            MorphLocus::EarShape => v,
            _ => v.saturating_sub(self.range().0) / self.step(),
        }
    }

    /// The value at a position ([`MorphLocus::position`]'s inverse).
    pub fn value_at(self, p: u8) -> u8 {
        match self {
            MorphLocus::TailShape => TailShape::DOMINANCE[p as usize].code(),
            MorphLocus::EarShape => p,
            _ => self.range().0 + p * self.step(),
        }
    }

    /// Dominance key: of two alleles the one with the smaller key is
    /// expressed. Steps from the default first (so the default beats
    /// everything), then the lower value on a tie (counts; shape ranks never
    /// tie).
    pub fn dominance_key(self, v: u8) -> (u8, u8) {
        (self.position(v).abs_diff(self.position(self.default_value())), v)
    }

    /// The expressed allele value of a locus.
    pub fn express(self, l: &MorphLocusPair) -> u8 {
        let (a, b) = (l.a.value, l.b.value);
        if self.dominance_key(a) <= self.dominance_key(b) {
            a
        } else {
            b
        }
    }

    fn tag_gene(self) -> Gene {
        match self {
            MorphLocus::EyeCount => Gene::Eyes,
            MorphLocus::EarCount | MorphLocus::EarShape => Gene::Ears,
            MorphLocus::LegCount => Gene::LegLength,
            MorphLocus::TailShape => Gene::Tail,
        }
    }
}

/// Continuous morphology loci (additive, alleles in `[0, 1]`), in genome
/// (and wire) order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MorphGene {
    /// Degrees / 180: the angle of the first eye pair from straight ahead.
    EyeAzimuth,
    SnoutLength,
}

impl MorphGene {
    pub const ALL: [MorphGene; 2] = [MorphGene::EyeAzimuth, MorphGene::SnoutLength];
    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn key(self) -> &'static str {
        match self {
            MorphGene::EyeAzimuth => "eye_azimuth",
            MorphGene::SnoutLength => "snout_length",
        }
    }

    /// The parameter's units per allele unit (degrees for `eye_azimuth`).
    pub fn scale(self) -> f32 {
        match self {
            MorphGene::EyeAzimuth => 180.0,
            MorphGene::SnoutLength => 1.0,
        }
    }

    /// The contract default, in the parameter's units (60°, 0.5).
    pub fn default_param(self) -> f32 {
        match self {
            MorphGene::EyeAzimuth => 60.0,
            MorphGene::SnoutLength => 0.5,
        }
    }

    /// The gene whose alleles' origin tags a missing locus copies (snapshots
    /// from before these loci; see [`MorphGenes::from_genes`]).
    fn tag_gene(self) -> Gene {
        match self {
            MorphGene::EyeAzimuth => Gene::Eyes,
            MorphGene::SnoutLength => Gene::Nose,
        }
    }
}

/// A discrete allele: a count or a shape code, and its origin tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MorphAllele {
    pub value: u8,
    pub origin: PlayerId,
}

/// The two alleles of a discrete locus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MorphLocusPair {
    pub a: MorphAllele,
    pub b: MorphAllele,
}

/// The spec's recommended `Balance::morph_step_rate` (the default is 0).
pub const RECOMMENDED_STEP_RATE: f32 = 0.0005;

/// Alleles in [`MorphGenes`]: two per discrete and per continuous locus.
pub const MORPH_ALLELES: usize = 2 * (MorphLocus::COUNT + MorphGene::COUNT);

/// A genome's morphology segment (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphGenes {
    pub discrete: [MorphLocusPair; MorphLocus::COUNT],
    pub continuous: [Locus; MorphGene::COUNT],
}

impl MorphGenes {
    /// Every allele at the given design, tagged `origin`.
    pub fn uniform(d: &MorphDesign, origin: PlayerId) -> MorphGenes {
        let disc = d.discrete_values();
        let cont = d.continuous_values();
        MorphGenes {
            discrete: std::array::from_fn(|k| {
                let a = MorphAllele { value: disc[k], origin };
                MorphLocusPair { a, b: a }
            }),
            continuous: std::array::from_fn(|k| {
                let a = Allele { value: cont[k].clamp(0.0, 1.0), origin };
                Locus { a, b: a }
            }),
        }
    }

    /// The contract defaults, each locus tagged like its anchor gene's
    /// alleles (eye loci: `Eyes`; ear loci: `Ears`; legs: `LegLength`; tail
    /// shape: `Tail`; snout: `Nose`): what a genome from before these loci
    /// gets.
    pub fn from_genes(loci: &[Locus]) -> MorphGenes {
        let mut m = MorphGenes::uniform(&MorphDesign::default(), crate::genome::WILD);
        for l in MorphLocus::ALL {
            let g = &loci[l.tag_gene().index()];
            let p = &mut m.discrete[l.index()];
            (p.a.origin, p.b.origin) = (g.a.origin, g.b.origin);
        }
        for c in MorphGene::ALL {
            let g = &loci[c.tag_gene().index()];
            let p = &mut m.continuous[c.index()];
            (p.a.origin, p.b.origin) = (g.a.origin, g.b.origin);
        }
        m
    }

    /// Expressed value of a discrete locus.
    pub fn expressed(&self, l: MorphLocus) -> u8 {
        l.express(&self.discrete[l.index()])
    }

    /// Expressed value (allele units, `[0, 1]`) of a continuous locus.
    pub fn expressed_gene(&self, g: MorphGene) -> f32 {
        self.continuous[g.index()].expressed(0.0)
    }

    /// Every discrete allele holds a valid value for its locus.
    pub fn is_valid(&self) -> bool {
        MorphLocus::ALL.iter().all(|&l| {
            let p = &self.discrete[l.index()];
            l.is_valid(p.a.value) && l.is_valid(p.b.value)
        })
    }

    /// The kinship key of the expressed discrete loci ([`MorphKey`]).
    pub fn key(&self) -> MorphKey {
        std::array::from_fn(|k| {
            let l = MorphLocus::ALL[k];
            l.position(self.expressed(l)) as f32
        })
    }

    /// Inherit one allele per locus from each parent, then mutate the
    /// continuous ones (the discrete ones mutate in
    /// [`MorphGenes::mutate_steps`]). `rng` is the genome's own stream.
    pub(crate) fn inherit(
        m: &MorphGenes,
        f: &MorphGenes,
        owner: PlayerId,
        mutation: crate::genome::MutationParams,
        rng: &mut impl Rng,
    ) -> MorphGenes {
        let discrete = std::array::from_fn(|k| {
            let (pm, pf) = (&m.discrete[k], &f.discrete[k]);
            let from_m = if rng.gen::<bool>() { pm.a } else { pm.b };
            let from_f = if rng.gen::<bool>() { pf.a } else { pf.b };
            MorphLocusPair { a: from_m, b: from_f }
        });
        let continuous = std::array::from_fn(|k| {
            let (pm, pf) = (&m.continuous[k], &f.continuous[k]);
            let from_m = if rng.gen::<bool>() { pm.a } else { pm.b };
            let from_f = if rng.gen::<bool>() { pf.a } else { pf.b };
            Locus {
                a: crate::genome::mutate(from_m, owner, mutation, rng),
                b: crate::genome::mutate(from_f, owner, mutation, rng),
            }
        });
        MorphGenes { discrete, continuous }
    }

    /// Step mutations of the discrete alleles, each with probability `rate`
    /// (no draws when it is 0), tagged `owner`.
    pub(crate) fn mutate_steps(&mut self, owner: PlayerId, rate: f32, rng: &mut impl Rng) {
        if rate <= 0.0 {
            return;
        }
        for (l, p) in MorphLocus::ALL.iter().zip(&mut self.discrete) {
            p.a = mutate_discrete(*l, p.a, owner, rate, rng);
            p.b = mutate_discrete(*l, p.b, owner, rate, rng);
        }
    }

    /// The alleles in wire order (discrete loci `a` then `b`, then the
    /// continuous ones) as `(value bits, origin)`: a discrete allele's value
    /// as its integer, a continuous one's as its `f32` bits.
    pub fn wire_alleles(&self) -> [(u32, PlayerId); MORPH_ALLELES] {
        let mut out = [(0, 0); MORPH_ALLELES];
        let mut k = 0;
        for p in &self.discrete {
            for a in [p.a, p.b] {
                out[k] = (a.value as u32, a.origin);
                k += 1;
            }
        }
        for p in &self.continuous {
            for a in [p.a, p.b] {
                out[k] = (a.value.to_bits(), a.origin);
                k += 1;
            }
        }
        out
    }

    /// From [`MorphGenes::wire_alleles`]; fails on a discrete value that is
    /// not valid for its locus.
    pub fn from_wire_alleles(v: &[(u32, PlayerId); MORPH_ALLELES]) -> Result<MorphGenes, String> {
        let bad = || "morphology allele out of range".to_string();
        let d = |k: usize| -> Result<MorphAllele, String> {
            let (bits, origin) = v[k];
            Ok(MorphAllele { value: u8::try_from(bits).map_err(|_| bad())?, origin })
        };
        let c = |k: usize| Allele { value: f32::from_bits(v[k].0), origin: v[k].1 };
        let blank = MorphAllele { value: 0, origin: 0 };
        let mut discrete = [MorphLocusPair { a: blank, b: blank }; MorphLocus::COUNT];
        for (l, p) in discrete.iter_mut().enumerate() {
            *p = MorphLocusPair { a: d(2 * l)?, b: d(2 * l + 1)? };
        }
        let base = 2 * MorphLocus::COUNT;
        let continuous = std::array::from_fn(|l| Locus { a: c(base + 2 * l), b: c(base + 2 * l + 1) });
        let g = MorphGenes { discrete, continuous };
        if !g.is_valid() {
            return Err(bad());
        }
        Ok(g)
    }
}

fn mutate_discrete(l: MorphLocus, a: MorphAllele, owner: PlayerId, rate: f32, rng: &mut impl Rng) -> MorphAllele {
    if rate <= 0.0 || rng.gen::<f32>() >= rate {
        return a;
    }
    let p = l.position(a.value);
    // Shape ranks run over every code; a count's positions up to its maximum's.
    let top = if l.is_shape() { l.range().1 } else { l.position(l.range().1) };
    let up = if p == 0 {
        true
    } else if p >= top {
        false
    } else {
        rng.gen::<bool>()
    };
    MorphAllele { value: l.value_at(if up { p + 1 } else { p - 1 }), origin: owner }
}

/// The expressed discrete loci of an organism as positions on their axes
/// ([`MorphLocus::position`], in [`MorphLocus`] order), as `f32` so they pack
/// beside the expressed genes the kinship tests read.
pub const MORPH_KEY: usize = MorphLocus::COUNT;

pub type MorphKey = [f32; MORPH_KEY];

/// Body-plan gap per step of `eye_count` or `leg_count` difference.
pub const BODY_PLAN_STEP: f32 = 0.1;

/// `(distance term, body-plan gap)` between two organisms' morphology keys
/// (see the module docs): `Σ min(1, steps / 2)` over the discrete loci, and
/// [`BODY_PLAN_STEP`] × the larger of the eye and leg steps. Both are sums
/// and products of small integers and halves, exact in `f32`, and exactly 0
/// for equal keys.
pub fn morph_terms(a: &MorphKey, b: &MorphKey) -> (f32, f32) {
    let steps = |k: usize| (a[k] - b[k]).abs();
    let distance = (0..MORPH_KEY).map(|k| (steps(k) * 0.5).min(1.0)).sum();
    let gap_steps = steps(MorphLocus::EyeCount.index()).max(steps(MorphLocus::LegCount.index()));
    (distance, BODY_PLAN_STEP * gap_steps)
}

/// A founder design's morphology (the contract's parameters).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphDesign {
    pub eye_count: u8,
    /// Degrees, 0–180.
    pub eye_azimuth: f32,
    pub ear_count: u8,
    pub ear_shape: EarShape,
    pub leg_count: u8,
    pub tail_shape: TailShape,
    /// 0–1.
    pub snout_length: f32,
}

impl Default for MorphDesign {
    fn default() -> Self {
        MorphDesign {
            eye_count: MorphLocus::EyeCount.default_value(),
            eye_azimuth: MorphGene::EyeAzimuth.default_param(),
            ear_count: MorphLocus::EarCount.default_value(),
            ear_shape: EarShape::Round,
            leg_count: MorphLocus::LegCount.default_value(),
            tail_shape: TailShape::Thin,
            snout_length: MorphGene::SnoutLength.default_param(),
        }
    }
}

/// Why a design's morphology was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MorphError {
    EyeCount(u8),
    EarCount(u8),
    LegCount(u8),
    EyeAzimuth(f32),
    SnoutLength(f32),
}

impl MorphError {
    /// The parameter's wire name.
    pub fn field(&self) -> &'static str {
        match self {
            MorphError::EyeCount(_) => "eye_count",
            MorphError::EarCount(_) => "ear_count",
            MorphError::LegCount(_) => "leg_count",
            MorphError::EyeAzimuth(_) => "eye_azimuth",
            MorphError::SnoutLength(_) => "snout_length",
        }
    }
}

impl std::fmt::Display for MorphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MorphError::EyeCount(n) => write!(f, "eye count {n} not in 1..=6"),
            MorphError::EarCount(n) => write!(f, "ear count {n} not one of 0, 2, 4"),
            MorphError::LegCount(n) => write!(f, "leg count {n} not one of 0, 2, 4, 6, 8"),
            MorphError::EyeAzimuth(x) => write!(f, "eye azimuth {x} not in [0, 180]"),
            MorphError::SnoutLength(x) => write!(f, "snout length {x} not in [0, 1]"),
        }
    }
}

impl MorphDesign {
    pub fn validate(&self) -> Result<(), MorphError> {
        if !MorphLocus::EyeCount.is_valid(self.eye_count) {
            return Err(MorphError::EyeCount(self.eye_count));
        }
        if !MorphLocus::EarCount.is_valid(self.ear_count) {
            return Err(MorphError::EarCount(self.ear_count));
        }
        if !MorphLocus::LegCount.is_valid(self.leg_count) {
            return Err(MorphError::LegCount(self.leg_count));
        }
        if !(0.0..=180.0).contains(&self.eye_azimuth) {
            return Err(MorphError::EyeAzimuth(self.eye_azimuth));
        }
        if !(0.0..=1.0).contains(&self.snout_length) {
            return Err(MorphError::SnoutLength(self.snout_length));
        }
        Ok(())
    }

    /// Allele values of the discrete loci, in [`MorphLocus`] order.
    pub fn discrete_values(&self) -> [u8; MorphLocus::COUNT] {
        [self.eye_count, self.ear_count, self.ear_shape.code(), self.leg_count, self.tail_shape.code()]
    }

    /// Allele values of the continuous loci, in [`MorphGene`] order.
    pub fn continuous_values(&self) -> [f32; MorphGene::COUNT] {
        [self.eye_azimuth / MorphGene::EyeAzimuth.scale(), self.snout_length / MorphGene::SnoutLength.scale()]
    }
}

/// One organism's expressed morphology (the contract's parameters, in their
/// units), computed with the phenotype at birth (`Organism::morph`) or by
/// [`morphology_of`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Morphology {
    /// 1–6.
    pub eye_count: u8,
    /// Degrees, 0–180: the first eye pair's angle from straight ahead.
    pub eye_azimuth: f32,
    /// The `Eyes` gene, 0–1.
    pub eye_size: f32,
    /// Derived: each eye's field width in degrees ([`eye_field`]).
    pub eye_field: f32,
    /// 0, 2 or 4.
    pub ear_count: u8,
    pub ear_shape: EarShape,
    /// The `Ears` gene, 0–1.
    pub ear_size: f32,
    /// 0, 2, 4, 6 or 8.
    pub leg_count: u8,
    /// The `LegLength` gene, 0–1.
    pub leg_length: f32,
    /// Derived from `Hooves`, `Claws` and `FootWidth` ([`Foot`]).
    pub foot: Foot,
    /// Derived: `tail_length ≥ TAIL_PRESENT`.
    pub tail_present: bool,
    pub tail_shape: TailShape,
    /// The `Tail` gene, 0–1.
    pub tail_length: f32,
    /// 0–1.
    pub snout_length: f32,
    /// The `Fangs` gene, 0–1 (none shown below [`FANGS_SHOW`]).
    pub fang_size: f32,
}

/// Eye field width in degrees per eye (contract v1): `140 + 60 × eye_size`.
pub fn eye_field(eye_size: f32) -> f32 {
    140.0 + 60.0 * eye_size
}

/// Most eyes: one on the midline and three pairs.
pub const MAX_EYES: usize = 6;

impl Morphology {
    /// From a genome's morphology loci and its phenotype.
    pub fn new(m: &MorphGenes, p: &Phenotype) -> Morphology {
        let options =
            [(Foot::Hooves, p[Gene::Hooves]), (Foot::Claws, p[Gene::Claws]), (Foot::Webbed, p[Gene::FootWidth])];
        let best = options.iter().fold(options[0], |b, &o| if o.1 > b.1 { o } else { b });
        let foot = if best.1 >= FOOT_SHOWS { best.0 } else { Foot::Plain };
        let eye_size = p[Gene::Eyes];
        let tail_length = p[Gene::Tail];
        Morphology {
            eye_count: m.expressed(MorphLocus::EyeCount),
            eye_azimuth: m.expressed_gene(MorphGene::EyeAzimuth) * MorphGene::EyeAzimuth.scale(),
            eye_size,
            eye_field: eye_field(eye_size),
            ear_count: m.expressed(MorphLocus::EarCount),
            ear_shape: EarShape::from_code(m.expressed(MorphLocus::EarShape)).unwrap_or_default(),
            ear_size: p[Gene::Ears],
            leg_count: m.expressed(MorphLocus::LegCount),
            leg_length: p[Gene::LegLength],
            foot,
            tail_present: tail_length >= TAIL_PRESENT,
            tail_shape: TailShape::from_code(m.expressed(MorphLocus::TailShape)).unwrap_or_default(),
            tail_length,
            snout_length: m.expressed_gene(MorphGene::SnoutLength) * MorphGene::SnoutLength.scale(),
            fang_size: p[Gene::Fangs],
        }
    }

    /// The wild-type (default design) body plan with `p`'s sizes: 2 eyes at
    /// exactly 60°, 2 round ears, 4 legs, a thin tail when `Tail` ≥
    /// [`TAIL_PRESENT`], snout exactly 0.5. What a phenotype alone implies
    /// (`stats::derive`, readouts and tests); an organism's own body plan is
    /// [`Morphology::new`] of its genome (`Organism::morph`).
    pub fn wild_type(p: &Phenotype) -> Morphology {
        Morphology {
            eye_azimuth: MorphGene::EyeAzimuth.default_param(),
            snout_length: MorphGene::SnoutLength.default_param(),
            ..Morphology::new(&MorphGenes::uniform(&MorphDesign::default(), crate::genome::WILD), p)
        }
    }

    /// Leg pairs, `leg_count / 2` (at most 4).
    pub fn leg_pairs(&self) -> u8 {
        (self.leg_count / 2).min(4)
    }

    pub fn has_fangs(&self) -> bool {
        self.fang_size >= FANGS_SHOW
    }

    /// Each eye's azimuth in degrees (contract v1 placement): an odd count
    /// puts one eye at 0; pair `k` (0, 1, 2) sits at
    /// `±min(170, eye_azimuth + 50·k)`. The first `eye_count` entries are
    /// used (midline first, then each pair right then left).
    pub fn eye_azimuths(&self) -> ([f32; MAX_EYES], usize) {
        let mut out = [0.0; MAX_EYES];
        let n = (self.eye_count as usize).min(MAX_EYES);
        let mut i = n % 2;
        for k in 0..n / 2 {
            let a = (self.eye_azimuth + 50.0 * k as f32).min(170.0);
            out[i] = a;
            out[i + 1] = -a;
            i += 2;
        }
        (out, n)
    }

    /// The kinship key of these expressed counts and shapes.
    pub fn key(&self) -> MorphKey {
        let v = [self.eye_count, self.ear_count, self.ear_shape.code(), self.leg_count, self.tail_shape.code()];
        std::array::from_fn(|k| MorphLocus::ALL[k].position(v[k]) as f32)
    }
}

impl Default for Morphology {
    /// The default design's morphology with every gene at 0 (a placeholder
    /// until an organism's is computed).
    fn default() -> Self {
        Morphology::new(
            &MorphGenes::uniform(&MorphDesign::default(), crate::genome::WILD),
            &Phenotype([0.0; Gene::COUNT]),
        )
    }
}

/// An organism's expressed morphology from its genome.
pub fn morphology_of(g: &crate::genome::Genome) -> Morphology {
    Morphology::new(g.morph(), &g.phenotype())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::{Genome, MutationParams};
    use crate::rng::rng_for;

    fn pair(a: u8, b: u8) -> MorphLocusPair {
        MorphLocusPair { a: MorphAllele { value: a, origin: 1 }, b: MorphAllele { value: b, origin: 2 } }
    }

    fn values(l: MorphLocus) -> Vec<u8> {
        let (lo, hi) = l.range();
        (lo..=hi).filter(|&v| l.is_valid(v)).collect()
    }

    #[test]
    fn expression_picks_one_allele_by_the_contract_rule() {
        use MorphLocus::*;
        // The wild type dominates; otherwise the nearer to it, then the lower.
        assert_eq!(LegCount.express(&pair(4, 8)), 4);
        assert_eq!(LegCount.express(&pair(8, 4)), 4);
        assert_eq!(LegCount.express(&pair(6, 8)), 6);
        assert_eq!(LegCount.express(&pair(2, 0)), 2);
        assert_eq!(LegCount.express(&pair(6, 2)), 2);
        assert_eq!(EyeCount.express(&pair(6, 1)), 1);
        assert_eq!(EyeCount.express(&pair(3, 1)), 1);
        assert_eq!(EarCount.express(&pair(4, 0)), 0);
        assert_eq!(EarShape.express(&pair(3, 1)), 1);
        // Tails: thin > bushy > prehensile > fin > club.
        use super::TailShape as T;
        let t = |s: T| s.code();
        assert_eq!(TailShape.express(&pair(t(T::Club), t(T::Thin))), t(T::Thin));
        assert_eq!(TailShape.express(&pair(t(T::Fin), t(T::Prehensile))), t(T::Prehensile));
        assert_eq!(TailShape.express(&pair(t(T::Club), t(T::Fin))), t(T::Fin));
        assert_eq!(TailShape.express(&pair(t(T::Bushy), t(T::Prehensile))), t(T::Bushy));
        // Never an average, and symmetric in the parents.
        for l in MorphLocus::ALL {
            for &a in &values(l) {
                assert_eq!(l.value_at(l.position(a)), a);
                for &b in &values(l) {
                    let e = l.express(&pair(a, b));
                    assert!(e == a || e == b);
                    assert_eq!(e, l.express(&pair(b, a)));
                    if a == l.default_value() {
                        assert_eq!(e, a, "{l:?}: the wild type is expressed over {b}");
                    }
                }
            }
        }
    }

    #[test]
    fn validity_and_design_validation() {
        assert!(MorphLocus::LegCount.is_valid(6) && !MorphLocus::LegCount.is_valid(3));
        assert!(!MorphLocus::LegCount.is_valid(10));
        assert!(!MorphLocus::EarCount.is_valid(1) && !MorphLocus::EarCount.is_valid(3));
        assert!(!MorphLocus::EyeCount.is_valid(0) && !MorphLocus::EyeCount.is_valid(7));
        assert!(MorphDesign::default().validate().is_ok());
        let d = MorphDesign::default();
        assert_eq!(MorphDesign { leg_count: 5, ..d }.validate(), Err(MorphError::LegCount(5)));
        assert_eq!(MorphDesign { eye_count: 0, ..d }.validate(), Err(MorphError::EyeCount(0)));
        assert_eq!(MorphDesign { ear_count: 3, ..d }.validate(), Err(MorphError::EarCount(3)));
        assert_eq!(MorphDesign { eye_azimuth: 181.0, ..d }.validate(), Err(MorphError::EyeAzimuth(181.0)));
        assert!(matches!(MorphDesign { snout_length: f32::NAN, ..d }.validate(), Err(MorphError::SnoutLength(_))));
    }

    #[test]
    fn kinship_terms_follow_the_spec() {
        let key = |d: MorphDesign| MorphGenes::uniform(&d, 1).key();
        let base = MorphDesign::default();
        assert_eq!(morph_terms(&key(base), &key(base)), (0.0, 0.0));
        // One step of legs: half a gene, 0.1 of body-plan gap; two: a whole
        // gene, 0.2; three: still 1, 0.3 (sterile).
        assert_eq!(morph_terms(&key(base), &key(MorphDesign { leg_count: 6, ..base })), (0.5, 0.1));
        assert_eq!(morph_terms(&key(base), &key(MorphDesign { leg_count: 8, ..base })), (1.0, 0.2));
        assert_eq!(morph_terms(&key(base), &key(MorphDesign { eye_count: 5, ..base })), (1.0, BODY_PLAN_STEP * 3.0));
        // Ears and shapes count in the distance only.
        assert_eq!(morph_terms(&key(base), &key(MorphDesign { ear_count: 0, ..base })), (0.5, 0.0));
        let fan = MorphDesign { ear_shape: EarShape::Fan, tail_shape: TailShape::Bushy, ..base };
        assert_eq!(morph_terms(&key(base), &key(fan)), (1.5, 0.0));
        // `Morphology::key` is the genome's.
        let g = Genome::uniform_with_morph(&[0.3; Gene::COUNT], &fan, 1);
        assert_eq!(g.morphology().key(), g.morph().key());
    }

    #[test]
    fn mutation_steps_once_and_tags_the_owner() {
        let d = MorphDesign { eye_count: 1, leg_count: 8, tail_shape: TailShape::Club, ..Default::default() };
        let base = MorphGenes::uniform(&d, 1);
        let mut rng = rng_for(9, 0, 0, 0);
        // Inheritance alone never changes a discrete allele.
        let always = MutationParams { rate: 1.0, sd: 0.1 };
        for _ in 0..20 {
            assert_eq!(MorphGenes::inherit(&base, &base, 7, always, &mut rng).discrete, base.discrete);
        }
        for _ in 0..200 {
            let mut c = base;
            c.mutate_steps(7, 1.0, &mut rng);
            assert!(c.is_valid());
            for l in MorphLocus::ALL {
                let was = base.discrete[l.index()].a.value;
                for a in [c.discrete[l.index()].a, c.discrete[l.index()].b] {
                    assert_eq!(a.origin, 7);
                    assert_eq!(l.position(a.value).abs_diff(l.position(was)), 1, "{l:?} {was} -> {}", a.value);
                }
            }
            // At the end of a range it moves inward.
            assert_eq!(c.expressed(MorphLocus::EyeCount), 2);
            assert_eq!(c.discrete[MorphLocus::LegCount.index()].a.value, 6);
            assert_eq!(c.discrete[MorphLocus::TailShape.index()].a.value, TailShape::Fin.code());
        }
        let none = MutationParams { rate: 0.0, sd: 0.0 };
        assert_eq!(MorphGenes::inherit(&base, &base, 7, none, &mut rng), base);
        let mut c = base;
        c.mutate_steps(7, 0.0, &mut rng);
        assert_eq!(c, base);
        // Through the genome, on its own stream: the same genome and owner
        // mutate the same way.
        let g = Genome::uniform_with_morph(&[0.4; Gene::COUNT], &d, 1);
        let (mut x, mut y) = (g.clone(), g.clone());
        x.mutate_morph(5, 0.5);
        y.mutate_morph(5, 0.5);
        assert_eq!(x, y);
        assert_ne!(x.morph(), g.morph());
        assert_eq!(x.loci(), g.loci());
    }

    #[test]
    fn morphology_reads_the_genome_and_phenotype() {
        let mut v = [0.0; Gene::COUNT];
        v[Gene::Eyes.index()] = 0.5;
        v[Gene::Tail.index()] = 0.04;
        v[Gene::Claws.index()] = 0.6;
        v[Gene::Hooves.index()] = 0.55;
        let d = MorphDesign { eye_count: 5, eye_azimuth: 90.0, ear_shape: EarShape::Fan, ..Default::default() };
        let g = Genome::uniform_with_morph(&v, &d, 1);
        let m = morphology_of(&g);
        assert_eq!((m.eye_count, m.ear_count, m.leg_count), (5, 2, 4));
        assert!((m.eye_azimuth - 90.0).abs() < 1e-4);
        assert_eq!(m.eye_field, 170.0);
        assert_eq!(m.ear_shape, EarShape::Fan);
        assert_eq!(m.foot, Foot::Claws);
        assert!(!m.tail_present);
        assert!(!m.has_fangs());
        let (az, n) = m.eye_azimuths();
        assert_eq!(n, 5);
        assert_eq!(az[0], 0.0);
        assert!((az[1] - 90.0).abs() < 1e-4 && (az[2] + 90.0).abs() < 1e-4);
        assert!((az[3] - 140.0).abs() < 1e-4 && (az[4] + 140.0).abs() < 1e-4);
        // Defaults: today's creatures.
        let plain = morphology_of(&Genome::uniform(&v, 1));
        assert_eq!((plain.eye_count, plain.ear_count, plain.leg_count), (2, 2, 4));
        assert!((plain.eye_azimuth - 60.0).abs() < 1e-4);
        assert_eq!((plain.ear_shape, plain.tail_shape, plain.snout_length), (EarShape::Round, TailShape::Thin, 0.5));
    }
}
