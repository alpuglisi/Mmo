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
/// whole-genome distance averages over 38 genes, so behavior and preference
/// genes dilute the few that make two designs different animals: a
/// warm-blooded live-born vole and a cold-blooded egg-laying newt differ in
/// almost nothing else. This keeps them (and a vole and a fox) sterile, while
/// similar designs of different players stay fertile.
pub fn body_plan_fertility(a: &Genome, b: &Genome) -> f32 {
    let gap = BODY_PLAN_GENES.iter().map(|&g| (a.expr(g) - b.expr(g)).abs()).fold(0.0, f32::max);
    ramp(gap, BODY_PLAN_FULL, BODY_PLAN_STERILE)
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
        let designs = crate::presets::wild_designs().into_iter().map(|(_, d)| d).chain(crate::presets::player_designs());
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

    #[test]
    fn distinct_presets_are_sterile_and_similar_ones_fertile() {
        assert_eq!(fertility(&preset("Meadow vole"), &preset("Marsh newt")), 0.0);
        assert_eq!(fertility(&preset("Meadow vole"), &preset("Red fox")), 0.0);
        assert_eq!(fertility(&preset("Meadow vole"), &preset("Burrowing omnivores")), 0.0);
        // Similar designs of different owners can still interbreed.
        assert!(fertility(&preset("Steppe antelope"), &preset("Herd grazers")) > 0.9);
        assert!(fertility(&preset("Red fox"), &preset("Pack hunters")) > 0.5);
    }
}
