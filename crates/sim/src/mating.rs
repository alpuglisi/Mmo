//! Mate preference, directed-mating override cost and outbreeding depression
//! ("Genetics" in the design doc).

use crate::genome::{Gene, Phenotype};

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

/// Fertility multiplier from genetic distance between mates. Close relatives
/// of the same kind are fully fertile; distant pairs risk sterility, which
/// drives speciation.
pub fn fertility(distance: f32) -> f32 {
    const FULL: f32 = 0.08;
    const STERILE: f32 = 0.3;
    if distance <= FULL {
        1.0
    } else {
        (1.0 - (distance - FULL) / (STERILE - FULL)).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{BirthType, FounderDesign, Thermo};
    use crate::genome::Genome;

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
        assert_eq!(fertility(0.05), 1.0);
        assert!(fertility(0.2) < 1.0 && fertility(0.2) > 0.0);
        assert_eq!(fertility(0.5), 0.0);
    }
}
