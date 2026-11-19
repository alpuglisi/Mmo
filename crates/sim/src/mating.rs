//! Mate preference, directed-mating override cost and outbreeding depression
//! ("Genetics" in the design doc).

use crate::genome::{Gene, Genome, Phenotype};

/// What a potential mate shows: display traits as currently expressed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// Coloration × body condition, so bright displays are honest signals of
    /// health.
    pub coloration: f32,
    /// Body size × growth.
    pub body_size: f32,
}

/// Root-sum-of-squares mismatch between a chooser's preferences and a
/// candidate's display. ≤ 1 means the chooser would accept on its own.
///
/// Per trait: `|candidate - preferred| / tolerance`. Pickiness narrows the
/// effective tolerance.
pub fn mismatch(chooser: &Phenotype, candidate: Display) -> f32 {
    let narrowing = 1.6 - 1.2 * chooser[Gene::Pickiness];
    let tol = |g: Gene| (chooser[g] * 0.5 * narrowing).max(0.02);
    let mc = (candidate.coloration - chooser[Gene::PrefColoration]).abs() / tol(Gene::TolColoration);
    let ms = (candidate.body_size - chooser[Gene::PrefBodySize]).abs() / tol(Gene::TolBodySize);
    (mc * mc + ms * ms).sqrt()
}

/// `cost = k * max(0, mismatch - 1)^2`.
pub fn override_cost(mismatch: f32, k: f32) -> f32 {
    let over = (mismatch - 1.0).max(0.0);
    k * over * over
}

/// Probability that an overridden mating fails, rising with cost.
pub fn override_failure_chance(cost: f32) -> f32 {
    1.0 - (-0.5 * cost).exp()
}

/// Fertility multiplier for a pair of mates: the lower of outbreeding
/// depression over the whole genome ([`distance_fertility`]) and body-plan
/// compatibility ([`body_plan_fertility`]).
pub fn fertility(a: &Genome, b: &Genome) -> f32 {
    distance_fertility(a.distance(b)).min(body_plan_fertility(a, b))
}

/// Fertility from genetic distance over all genes. Close relatives of the same
/// kind are fully fertile; distant pairs risk sterility, which drives
/// speciation.
pub fn distance_fertility(distance: f32) -> f32 {
    ramp(distance, 0.08, 0.3)
}

/// The genes that define a body plan: thermoregulation, birth mode, body size
/// and diet.
pub const BODY_PLAN_GENES: [Gene; 4] = [Gene::Endothermy, Gene::BirthMode, Gene::BodySize, Gene::Carnivory];
/// Largest difference in any body-plan gene that is still fully fertile.
/// Founder variation (expressed values differ by about 0.05 between two
/// founders) stays well inside it.
pub const BODY_PLAN_FULL: f32 = 0.12;
/// Difference in any body-plan gene at which mates are sterile. Below the
/// gap between design choices for thermoregulation (0.8) and adjacent birth
/// modes (0.35).
pub const BODY_PLAN_STERILE: f32 = 0.3;

/// Fertility from the largest difference in any [`BODY_PLAN_GENES`]. The
/// whole-genome distance averages over every gene, so behavior and preference
/// genes dilute the few that make two designs different animals: a
/// warm-blooded live-born vole and a cold-blooded egg-laying newt differ in
/// almost nothing else. This keeps them (and a vole and a fox) sterile, while
/// similar designs of different players stay fertile.
pub fn body_plan_fertility(a: &Genome, b: &Genome) -> f32 {
    fertility_from_gap(body_plan_gap(a, b))
}

/// [`body_plan_fertility`] from an already computed [`body_plan_gap`].
pub fn fertility_from_gap(gap: f32) -> f32 {
    ramp(gap, BODY_PLAN_FULL, BODY_PLAN_STERILE)
}

/// Largest difference in expressed value of any [`BODY_PLAN_GENES`], or the
/// morphology's body-plan gap if larger ([`crate::morph::morph_terms`]: 0.1
/// per step of eye or leg count; 0 between equal counts, so then exactly the
/// genes' gap).
pub fn body_plan_gap(a: &Genome, b: &Genome) -> f32 {
    let morph = crate::morph::morph_terms(&a.morph().key(), &b.morph().key()).1;
    BODY_PLAN_GENES.iter().map(|&g| (a.expr(g) - b.expr(g)).abs()).fold(0.0, f32::max).max(morph)
}

/// Position of birth mode in [`BODY_PLAN_GENES`].
pub const BIRTH_SLOT: usize = 1;
/// Position of endothermy in [`BODY_PLAN_GENES`].
pub const THERMO_SLOT: usize = 0;

/// How much less some body-plan differences count for fertility than for
/// kinship: birth mode (`Balance::egg_hybrid`), eye count
/// (`Balance::eye_hybrid`) and warm/cold blood (`Balance::thermo_hybrid`).
/// All 0: the full body-plan gap.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Relief {
    pub egg: f32,
    pub eye: f32,
    pub thermo: f32,
}

impl Relief {
    pub fn of(b: &crate::Balance) -> Relief {
        Relief { egg: b.egg_hybrid, eye: b.eye_hybrid, thermo: b.thermo_hybrid }
    }

    /// Weight of an eye-count step (`1 − eye`).
    pub fn eye_weight(self) -> f32 {
        1.0 - self.eye.clamp(0.0, 1.0)
    }

    pub fn is_none(self) -> bool {
        self.egg <= 0.0 && self.eye <= 0.0 && self.thermo <= 0.0
    }
}

/// The weight of each [`BODY_PLAN_GENES`] difference in the fertility gap:
/// 1, birth mode `1 − relief.egg` (`Balance::egg_hybrid`), endothermy
/// `1 − relief.thermo` (`Balance::thermo_hybrid`).
pub fn plan_weights(relief: Relief) -> [f32; 4] {
    let mut w = [1.0; 4];
    w[BIRTH_SLOT] = 1.0 - relief.egg.clamp(0.0, 1.0);
    w[THERMO_SLOT] = 1.0 - relief.thermo.clamp(0.0, 1.0);
    w
}

/// [`body_plan_gap`] for fertility, birth mode weighted by `1 − relief.egg`
/// (`Balance::egg_hybrid`: adjacent birth modes, 0.35 apart, count 0.28 at
/// 0.2) and eye-count steps by `1 − relief.eye` (`Balance::eye_hybrid`:
/// 2 → 5 eyes counts 0.12 at 0.6 instead of a sterile 0.30) and warm/cold
/// blood by `1 − relief.thermo` (`Balance::thermo_hybrid`: 0.8 apart counts
/// 0.24 at 0.7, a third as fertile instead of sterile). The same as
/// [`body_plan_gap`] with no relief. Kinship and species keep the
/// unweighted gap.
pub fn fertility_gap(a: &Genome, b: &Genome, relief: Relief) -> f32 {
    let w = plan_weights(relief);
    let morph = crate::morph::morph_fertility_gap(&a.morph().key(), &b.morph().key(), relief.eye_weight());
    BODY_PLAN_GENES.iter().zip(w).map(|(&g, w)| (a.expr(g) - b.expr(g)).abs() * w).fold(0.0, f32::max).max(morph)
}

/// [`fertility`] with the body-plan term's relief ([`fertility_gap`]); the
/// same as [`fertility`] with none.
pub fn fertility_with(a: &Genome, b: &Genome, relief: Relief) -> f32 {
    distance_fertility(a.distance(b)).min(fertility_from_gap(fertility_gap(a, b, relief)))
}

/// How many times the fertility gap counts between animals of `a` and `b`
/// (owners): `1 + Balance::wild_isolation` when exactly one is wild, so
/// player herds of most sizes aren't diluted by a wild species; 1 otherwise.
pub fn wild_gap_scale(bal: &crate::Balance, a: crate::genome::PlayerId, b: crate::genome::PlayerId) -> f32 {
    if bal.wild_isolation > 0.0 && (a == crate::WILD) != (b == crate::WILD) {
        1.0 + bal.wild_isolation
    } else {
        1.0
    }
}

/// The share of a player × wild litter that comes of it
/// (`Balance::wild_barrier`): `1 − wild_barrier` when exactly one owner is
/// wild; 1 otherwise. A gap scale ([`wild_gap_scale`]) can't isolate a
/// design whose body plan is the wild species' own (a gap near 0).
pub fn wild_fertility(bal: &crate::Balance, a: crate::genome::PlayerId, b: crate::genome::PlayerId) -> f32 {
    if (a == crate::WILD) != (b == crate::WILD) {
        (1.0 - bal.wild_barrier).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// [`fertility_with`] with the body-plan gap counted `scale` times
/// ([`wild_gap_scale`]); the same at 1.
pub fn fertility_scaled(a: &Genome, b: &Genome, relief: Relief, scale: f32) -> f32 {
    if scale == 1.0 {
        return fertility_with(a, b, relief);
    }
    distance_fertility(a.distance(b)).min(fertility_from_gap(fertility_gap(a, b, relief) * scale))
}

/// Whether two organisms recognize each other as their own kind: never hunted
/// or fled from, and counted for grouping, herd defense, crowding and larval
/// competition. `distance` is `a.distance(b)`, passed in because the caller
/// usually needs it anyway.
///
/// Whole-genome and body-plan differences add up:
/// `distance / species_distance + body_plan_gap / body_plan_gap_limit < 1`
/// (the limit is [`crate::WorldConfig::kin_body_plan_gap`]). Whole-genome
/// distance alone lets behavior genes dilute the few that make two designs
/// different animals (a vole and a fox differ by 0.10 overall but 0.85 in
/// diet); the body-plan term keeps different body plans and diets apart while
/// founders of one design and mild drift stay kin.
pub fn is_kin(a: &Genome, b: &Genome, distance: f32, species_distance: f32, body_plan_gap_limit: f32) -> bool {
    let rest = 1.0 - distance / species_distance;
    rest > 0.0 && body_plan_gap(a, b) < rest * body_plan_gap_limit
}

/// [`is_kin`] from an already computed [`body_plan_gap`] (`gap`).
pub fn is_kin_by_gap(gap: f32, distance: f32, species_distance: f32, body_plan_gap_limit: f32) -> bool {
    let rest = 1.0 - distance / species_distance;
    rest > 0.0 && gap < rest * body_plan_gap_limit
}

/// 1 up to `full`, falling linearly to 0 at `sterile`.
fn ramp(x: f32, full: f32, sterile: f32) -> f32 {
    if x <= full {
        1.0
    } else {
        (1.0 - (x - full) / (sterile - full)).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{BirthType, FounderDesign, Thermo};

    /// `Balance::egg_hybrid` weights birth mode in the fertility gap only:
    /// an egg layer next to a live bearer of the same plan is a little
    /// fertile at 0.2, sterile at 0; a thermoregulation gap stays sterile.
    #[test]
    fn egg_hybrid_softens_only_the_birth_gap() {
        let g = |birth: BirthType, thermo: Thermo| {
            let d = FounderDesign::new("x", thermo, birth, 2);
            Genome::uniform(&d.values(), 1)
        };
        let (live, eggs) = (g(BirthType::Live, Thermo::Warm), g(BirthType::IncubatedEggs, Thermo::Warm));
        assert_eq!(fertility_with(&live, &eggs, Relief { egg: 0.0, eye: 0.0, thermo: 0.0 }), 0.0);
        assert_eq!(fertility_with(&live, &eggs, Relief { egg: 0.0, eye: 0.0, thermo: 0.0 }), fertility(&live, &eggs));
        let f = fertility_with(&live, &eggs, Relief { egg: 0.2, eye: 0.0, thermo: 0.0 });
        assert!(f > 0.05 && f < 0.2, "{f}");
        assert!((fertility_gap(&live, &eggs, Relief { egg: 0.2, eye: 0.0, thermo: 0.0 }) - 0.28).abs() < 1e-4);
        assert_eq!(
            body_plan_gap(&live, &eggs),
            fertility_gap(&live, &eggs, Relief { egg: 0.0, eye: 0.0, thermo: 0.0 })
        );
        let cold = g(BirthType::Live, Thermo::Cold);
        assert_eq!(fertility_with(&live, &cold, Relief { egg: 0.2, eye: 0.0, thermo: 0.0 }), 0.0);
    }

    /// `Balance::thermo_hybrid` makes a warm and a cold lineage of one plan
    /// partly fertile (about a third at 0.7), not when they also differ in
    /// a full body-plan gene: a cold frog laying abandoned eggs stays
    /// sterile with a warm live bearer.
    #[test]
    fn thermo_hybrid_softens_only_the_thermo_gap() {
        let g = |birth: BirthType, thermo: Thermo| {
            let d = FounderDesign::new("x", thermo, birth, 2);
            Genome::uniform(&d.values(), 1)
        };
        let relief = Relief { egg: 0.2, eye: 0.6, thermo: 0.7 };
        let (warm, cold) = (g(BirthType::Live, Thermo::Warm), g(BirthType::Live, Thermo::Cold));
        let f = fertility_with(&warm, &cold, relief);
        assert!(f > 0.25 && f < 0.4, "{f}");
        assert_eq!(fertility_with(&warm, &cold, Relief { thermo: 0.0, ..relief }), 0.0);
        let frog = g(BirthType::AbandonedEggs, Thermo::Cold);
        assert_eq!(fertility_with(&warm, &frog, relief), 0.0);
        // Kinship and species keep the full gap.
        assert!(body_plan_gap(&warm, &cold) > 0.7);
    }

    fn chooser(pickiness: f32) -> Phenotype {
        let d = FounderDesign::new("c", Thermo::Warm, BirthType::Live, 2)
            .with(Gene::Pickiness, pickiness)
            .with(Gene::PrefColoration, 0.8)
            .with(Gene::TolColoration, 0.5)
            .with(Gene::PrefBodySize, 0.5)
            .with(Gene::TolBodySize, 0.5);
        Genome::uniform(&d.values(), 1).phenotype()
    }

    #[test]
    fn ideal_candidate_has_zero_mismatch() {
        let m = mismatch(&chooser(0.5), Display { coloration: 0.8, body_size: 0.5 });
        assert!(m < 1e-6);
    }

    #[test]
    fn picky_choosers_see_bigger_mismatch() {
        let c = Display { coloration: 0.5, body_size: 0.5 };
        assert!(mismatch(&chooser(0.9), c) > mismatch(&chooser(0.1), c));
    }

    #[test]
    fn override_cost_is_zero_inside_preferences_and_quadratic_outside() {
        assert_eq!(override_cost(0.7, 2.0), 0.0);
        assert_eq!(override_cost(1.0, 2.0), 0.0);
        assert!((override_cost(3.0, 2.0) - 8.0).abs() < 1e-6);
    }

    #[test]
    fn fertility_falls_with_distance() {
        assert_eq!(distance_fertility(0.05), 1.0);
        assert!(distance_fertility(0.2) < 1.0 && distance_fertility(0.2) > 0.0);
        assert_eq!(distance_fertility(0.5), 0.0);
    }

    fn genome(d: &FounderDesign) -> Genome {
        Genome::uniform(&d.values(), 1)
    }

    fn preset(name: &str) -> Genome {
        let designs =
            crate::presets::wild_designs().into_iter().map(|(_, d)| d).chain(crate::presets::player_designs());
        genome(&designs.into_iter().find(|d| d.name == name).unwrap())
    }

    #[test]
    fn founder_variation_is_fully_fertile() {
        let mut rng = crate::rng::rng_for(7, 0, 0, 0);
        let d = FounderDesign::new("v", Thermo::Warm, BirthType::Live, 4).with(Gene::BodySize, 0.3);
        let founders: Vec<Genome> =
            (0..40).map(|_| Genome::founder(&d.values(), 1, crate::design::FOUNDER_VARIATION, &mut rng)).collect();
        let mean: f32 = founders.windows(2).map(|w| fertility(&w[0], &w[1])).sum::<f32>() / 39.0;
        assert!(mean > 0.98, "mean founder fertility {mean}");
    }

    #[test]
    fn different_thermoregulation_or_birth_mode_is_sterile() {
        let base = FounderDesign::new("a", Thermo::Warm, BirthType::Live, 4);
        let cold = FounderDesign::new("b", Thermo::Cold, BirthType::Live, 4);
        let eggs = FounderDesign::new("c", Thermo::Warm, BirthType::IncubatedEggs, 4);
        let left = FounderDesign::new("d", Thermo::Warm, BirthType::AbandonedEggs, 4);
        // Nothing else differs, so whole-genome distance alone would allow it.
        assert!(distance_fertility(genome(&base).distance(&genome(&cold))) > 0.9);
        assert_eq!(fertility(&genome(&base), &genome(&cold)), 0.0);
        assert_eq!(fertility(&genome(&base), &genome(&eggs)), 0.0);
        assert_eq!(fertility(&genome(&eggs), &genome(&left)), 0.0);
    }

    #[test]
    fn mild_body_divergence_is_partially_fertile() {
        let a = FounderDesign::new("a", Thermo::Warm, BirthType::Live, 4).with(Gene::BodySize, 0.3);
        let f = fertility(&genome(&a), &genome(&a.clone().with(Gene::BodySize, 0.5)));
        assert!(f > 0.0 && f < 1.0, "{f}");
    }

    fn kin(a: &Genome, b: &Genome) -> bool {
        let cfg = crate::WorldConfig::default();
        is_kin(a, b, a.distance(b), cfg.species_distance, cfg.kin_body_plan_gap)
    }

    #[test]
    fn founding_designs_recognize_only_their_own_kind() {
        let names = [
            "Meadow vole",
            "Steppe antelope",
            "Marsh newt",
            "Red fox",
            "Pack hunters",
            "Herd grazers",
            "Toxic frogs",
            "Burrowing omnivores",
        ];
        let g: Vec<Genome> = names.iter().map(|n| preset(n)).collect();
        // Similar herbivore designs of different owners still herd together.
        let kin_pairs = [(1, 5)];
        for i in 0..names.len() {
            assert!(kin(&g[i], &g[i]), "{} with itself", names[i]);
            for j in i + 1..names.len() {
                let expected = kin_pairs.contains(&(i, j));
                assert_eq!(kin(&g[i], &g[j]), expected, "{} vs {}", names[i], names[j]);
            }
        }
        // Whole-genome distance alone would make these kin.
        let cfg = crate::WorldConfig::default();
        for (a, b) in [(0, 3), (0, 7), (3, 4)] {
            assert!(g[a].distance(&g[b]) < cfg.species_distance, "{} vs {}", names[a], names[b]);
        }
    }

    #[test]
    fn founders_of_one_design_and_mild_drift_are_kin() {
        let mut rng = crate::rng::rng_for(11, 0, 0, 0);
        for (_, d) in crate::presets::wild_designs() {
            let founders: Vec<Genome> = (0..60).map(|_| d.founder_genome(0, &mut rng)).collect();
            let pairs = founders.windows(2).filter(|w| kin(&w[0], &w[1])).count();
            assert!(pairs >= 58, "{}: {pairs}/59 founder pairs are kin", d.name);
        }
        let a = FounderDesign::new("a", Thermo::Warm, BirthType::Live, 4).with(Gene::BodySize, 0.3);
        assert!(kin(&genome(&a), &genome(&a.clone().with(Gene::BodySize, 0.38))));
        assert!(!kin(&genome(&a), &genome(&a.clone().with(Gene::BodySize, 0.6))));
    }

    #[test]
    fn distinct_presets_are_sterile_and_similar_ones_fertile() {
        assert_eq!(fertility(&preset("Meadow vole"), &preset("Marsh newt")), 0.0);
        assert_eq!(fertility(&preset("Meadow vole"), &preset("Red fox")), 0.0);
        assert_eq!(fertility(&preset("Meadow vole"), &preset("Burrowing omnivores")), 0.0);
        // Similar designs of different owners can still interbreed.
        assert!(fertility(&preset("Steppe antelope"), &preset("Herd grazers")) > 0.9);
        // The standard fox (carnivory 0.6) and pack hunters (0.9) are a full
        // body-plan gap apart (*The fox in its own niche*); at 0.85 the fox
        // was fully fertile with them.
        assert!(fertility(&preset("Red fox"), &preset("Pack hunters")) < 1e-5);
        assert!(!kin(&preset("Red fox"), &preset("Pack hunters")));
        // In their niches (`presets::niche_world`) the fox (carnivory 0.65)
        // and the pack hunters (0.95) are a full body-plan gap apart: no
        // hybrids, and neither counts the other as its own kind.
        let (fox, pack) = (genome(&crate::presets::niche_fox()), genome(&crate::presets::niche_pack_hunters()));
        assert_eq!(fertility(&fox, &pack), 0.0);
        assert!(!kin(&fox, &pack));
    }
}
