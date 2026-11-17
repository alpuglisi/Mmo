//! Effects of the Phase 3 morphology loci: horns, wing shape and coat
//! pattern (`docs/morphology.md` §1 "Phase 3 candidates", decisions 18–23).
//!
//! Like [`crate::morph_effects`], everything is computed once per organism,
//! at birth, from its phenotype and expressed [`Morphology`], and folded into
//! its [`Traits`] by [`apply`] (which `morph_effects::apply` calls, so every
//! birth, restore and LOD representative gets it). The one per-tick read is
//! [`camouflage`], the pattern's stealth on the ground an animal stands on,
//! which detection adds to its stealth only when [`MorphContext::camo`] is
//! above 0.
//!
//! - **Neutral at the default body.** No horns (`horn_size` below
//!   [`crate::morph::HORNS_SHOW`]), wing aspect exactly 0.5 (or no wings)
//!   and a plain coat change no bit. Founders vary `horn_size` and
//!   `wing_aspect` around the design value, so with the tunables on a
//!   default-designed lineage sees a few small horns and small wing effects
//!   around zero, as with snout length.
//! - **Behind tunables.** `Balance::morph_horns`, `morph_wing_shape` and
//!   `morph_pattern` scale each group; at 0 [`apply`] changes nothing.
//!
//! [`MorphContext::camo`]: crate::morph_effects::MorphContext::camo

use crate::config::Balance;
use crate::genome::{Gene, Phenotype};
use crate::morph::{HornForm, Morphology, Pattern};
use crate::stats::Traits;
use crate::terrain::Biome;

/// The tunables of this module (`Balance::morph_horns`,
/// `morph_wing_shape`, `morph_pattern`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtraKnobs {
    pub horns: f32,
    pub wing_shape: f32,
    pub pattern: f32,
}

impl ExtraKnobs {
    /// The formulas at full strength (readouts and tests).
    pub const FULL: ExtraKnobs = ExtraKnobs { horns: 1.0, wing_shape: 1.0, pattern: 1.0 };
    pub const OFF: ExtraKnobs = ExtraKnobs { horns: 0.0, wing_shape: 0.0, pattern: 0.0 };

    pub fn of(b: &Balance) -> ExtraKnobs {
        ExtraKnobs { horns: b.morph_horns, wing_shape: b.morph_wing_shape, pattern: b.morph_pattern }
    }

    pub fn is_off(&self) -> bool {
        *self == ExtraKnobs::OFF
    }
}

impl HornForm {
    /// `(defense, fight-back, display, snag)` per unit of horn size: a rack
    /// parries and shows off best but snags in undergrowth; straight spikes
    /// stab hardest; curved horns are the all-round, least snagging form.
    pub fn coefficients(self) -> (f32, f32, f32, f32) {
        match self {
            HornForm::Curved => (0.15, 0.25, 0.03, 0.10),
            HornForm::Straight => (0.10, 0.40, 0.03, 0.15),
            HornForm::Branched => (0.20, 0.20, 0.06, 0.30),
        }
    }
}

impl Pattern {
    /// How well the pattern matches a biome: 1 blends in, 0 neutral, −0.5
    /// stands out. Plain is neutral everywhere.
    pub fn biome_match(self, b: Biome) -> f32 {
        use Biome::*;
        match (self, b) {
            (Pattern::Plain, _) => 0.0,
            (Pattern::Spots, Forest) => 1.0,
            (Pattern::Spots, Marsh) => 0.5,
            (Pattern::Spots, Desert | Tundra) => -0.5,
            (Pattern::Stripes, Grassland | Marsh) => 1.0,
            (Pattern::Stripes, Desert | Tundra | Mountain) => -0.5,
            (Pattern::Mottled, Mountain | Tundra) => 1.0,
            (Pattern::Mottled, Desert) => 0.5,
            (Pattern::Mottled, Marsh) => -0.5,
            _ => 0.0,
        }
    }
}

/// Largest stealth a perfectly matched pattern adds (with the `Camouflage`
/// gene at 1; half of it at 0).
pub const PATTERN_STEALTH: f32 = 0.12;

/// Every change of the Phase 3 loci at the given strengths. Zero (or 1 for
/// factors) at the default body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtraEffects {
    /// Horn size `h` (0 without horns).
    pub horns: f32,
    pub d_defense: f32,
    /// Added to `MorphContext::fight_back`.
    pub fight_back: f32,
    pub d_stealth: f32,
    pub mass_factor: f32,
    pub d_upkeep: f32,
    pub d_metabolism: f32,
    /// Effective width in dense vegetation (horn snag, narrow wings).
    pub d_width: f32,
    pub d_speed: f32,
    pub d_agility: f32,
    pub d_endurance: f32,
    /// Factor on wing loading (broad wings take off easier).
    pub wing_loading_factor: f32,
    /// The `Wings` gene (the flight thresholds read it).
    pub wings: f32,
    /// Pattern stealth at a perfect match (`MorphContext::camo`).
    pub camo: f32,
}

/// The effects of `plan`'s horns, wing shape and pattern on a body whose
/// traits so far are `base`.
pub fn derive(p: &Phenotype, plan: &Morphology, k: ExtraKnobs, base: &Traits) -> ExtraEffects {
    // Horns: defense, fight-back and display; mass, food and snagging.
    let h = if plan.horns_present { plan.horn_size } else { 0.0 };
    let (def, fight, display, snag) = plan.horn_form.coefficients();
    let kh = k.horns;
    let horn_mass = 1.0 + kh * 0.08 * h;
    let horn_upkeep = kh * 0.06 * h;
    let base_metabolism = (0.6 + 0.3 * p[Gene::Muscle]) * if base.warm { 1.0 } else { 0.35 };

    // Wing shape: only for a body with working wings (gliders and fliers).
    let w = if base.body.can_glide || base.body.can_fly { p[Gene::Wings] } else { 0.0 };
    let da = (plan.wing_aspect - 0.5) * w * k.wing_shape;

    // Pattern.
    let camo = if plan.pattern == Pattern::Plain {
        0.0
    } else {
        k.pattern * PATTERN_STEALTH * (0.5 + 0.5 * p[Gene::Camouflage])
    };

    ExtraEffects {
        horns: h,
        d_defense: kh * def * h,
        fight_back: kh * fight * h,
        d_stealth: -kh * display * h,
        mass_factor: horn_mass,
        d_upkeep: horn_upkeep,
        d_metabolism: base_metabolism * horn_upkeep,
        d_width: kh * snag * h / 3.0 + 0.1 * da.max(0.0),
        // Narrow wings: faster and cheaper over distance, clumsier; broad
        // wings the reverse.
        d_speed: 0.08 * da,
        d_agility: -0.15 * da,
        d_endurance: 0.1 * da,
        wing_loading_factor: 1.0 + 0.2 * da,
        wings: p[Gene::Wings],
        camo,
    }
}

impl ExtraEffects {
    /// Fold into `t`. Exact at the default body (`+ 0`, `× 1`).
    pub fn apply_to(&self, t: &mut Traits) {
        let st = &mut t.stats;
        st.defense += self.d_defense;
        st.stealth = (st.stealth + self.d_stealth).clamp(0.0, 1.0);
        st.metabolism += self.d_metabolism;
        st.speed = (st.speed + self.d_speed).max(0.08);
        st.agility = (st.agility + self.d_agility).max(0.05);
        st.endurance = (st.endurance + self.d_endurance).max(0.1);
        let b = &mut t.body;
        b.width += self.d_width;
        if self.mass_factor != 1.0 || self.wing_loading_factor != 1.0 {
            b.mass *= self.mass_factor;
            b.ground_pressure *= self.mass_factor.cbrt();
            b.wing_loading *= self.mass_factor * self.wing_loading_factor;
            // Same thresholds as `stats::derive`.
            let wings = self.wings;
            b.can_fly = wings > 0.5 && b.wing_loading < 2.0;
            b.can_glide = wings > 0.25 && b.wing_loading < 6.0;
        }
        t.morph.fight_back += self.fight_back;
        t.morph.camo = self.camo;
        t.morph.pattern = if self.camo > 0.0 { t.morph.pattern } else { Pattern::Plain };
    }
}

/// Apply the Phase 3 loci of `plan` to fresh traits at the strengths `bal`
/// sets (called by `morph_effects::apply`). With the three tunables at 0 it
/// changes nothing.
pub fn apply(t: &mut Traits, p: &Phenotype, plan: &Morphology, bal: &Balance) {
    apply_knobs(t, p, plan, ExtraKnobs::of(bal));
}

/// [`apply`] at explicit strengths.
pub fn apply_knobs(t: &mut Traits, p: &Phenotype, plan: &Morphology, k: ExtraKnobs) {
    if k.is_off() {
        return;
    }
    t.morph.pattern = plan.pattern;
    derive(p, plan, k, t).apply_to(t);
}

/// The coat pattern's stealth on ground of biome `b`: the organism's
/// `camo` times how well its pattern matches (see [`Pattern::biome_match`]).
/// 0 for a plain coat or with `Balance::morph_pattern` at 0.
pub fn camouflage(m: &crate::morph_effects::MorphContext, b: Biome) -> f32 {
    m.camo * m.pattern.biome_match(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::Genome;
    use crate::morph::MorphDesign;

    fn traits_of(v: &[f32; Gene::COUNT], d: &MorphDesign, k: ExtraKnobs) -> (Traits, Traits) {
        let g = Genome::uniform_with_morph(v, d, 1);
        let p = g.phenotype();
        let plan = g.morphology();
        let base = crate::stats::derive_with(&p, &plan);
        let mut t = base;
        apply_knobs(&mut t, &p, &plan, k);
        (base, t)
    }

    fn values() -> [f32; Gene::COUNT] {
        let mut v = [0.3; Gene::COUNT];
        v[Gene::Endothermy.index()] = 0.9;
        v[Gene::Wings.index()] = 0.9;
        v[Gene::BodySize.index()] = 0.1;
        v
    }

    #[test]
    fn default_body_is_exactly_neutral() {
        let (base, t) = traits_of(&values(), &MorphDesign::default(), ExtraKnobs::FULL);
        assert_eq!(format!("{base:?}"), format!("{t:?}"));
        let (base, t) = traits_of(
            &values(),
            &MorphDesign { horn_size: 0.8, pattern: Pattern::Stripes, wing_aspect: 0.9, ..Default::default() },
            ExtraKnobs::OFF,
        );
        assert_eq!(format!("{base:?}"), format!("{t:?}"));
    }

    #[test]
    fn horns_defend_and_cost() {
        let horned = |f| MorphDesign { horn_size: 1.0, horn_form: f, ..Default::default() };
        let (base, curved) = traits_of(&values(), &horned(HornForm::Curved), ExtraKnobs::FULL);
        let (_, straight) = traits_of(&values(), &horned(HornForm::Straight), ExtraKnobs::FULL);
        let (_, rack) = traits_of(&values(), &horned(HornForm::Branched), ExtraKnobs::FULL);
        assert!(curved.stats.defense > base.stats.defense);
        assert!(curved.body.mass > base.body.mass && curved.stats.metabolism > base.stats.metabolism);
        assert!(straight.morph.fight_back > curved.morph.fight_back);
        assert!(rack.stats.defense > curved.stats.defense);
        // A rack snags most in undergrowth.
        assert!(rack.body.width > straight.body.width && straight.body.width > curved.body.width);
        assert!(rack.stats.stealth < curved.stats.stealth);
        // Below the threshold: no horns, no effect.
        let (b, t) = traits_of(&values(), &MorphDesign { horn_size: 0.05, ..Default::default() }, ExtraKnobs::FULL);
        assert_eq!(format!("{b:?}"), format!("{t:?}"));
    }

    #[test]
    fn wing_shape_trades_speed_for_agility() {
        let shape = |a| MorphDesign { wing_aspect: a, ..Default::default() };
        let (base, narrow) = traits_of(&values(), &shape(1.0), ExtraKnobs::FULL);
        let (_, broad) = traits_of(&values(), &shape(0.0), ExtraKnobs::FULL);
        assert!(base.body.can_glide);
        assert!(narrow.stats.speed > base.stats.speed && broad.stats.speed < base.stats.speed);
        assert!(narrow.stats.agility < base.stats.agility && broad.stats.agility > base.stats.agility);
        assert!(broad.body.wing_loading < narrow.body.wing_loading);
        // No wings: nothing.
        let mut v = values();
        v[Gene::Wings.index()] = 0.0;
        let (b, t) = traits_of(&v, &shape(1.0), ExtraKnobs::FULL);
        assert_eq!(format!("{b:?}"), format!("{t:?}"));
    }

    #[test]
    fn patterns_hide_in_their_biomes() {
        let (_, t) =
            traits_of(&values(), &MorphDesign { pattern: Pattern::Spots, ..Default::default() }, ExtraKnobs::FULL);
        assert!(camouflage(&t.morph, Biome::Forest) > 0.0);
        assert!(camouflage(&t.morph, Biome::Desert) < 0.0);
        assert_eq!(camouflage(&t.morph, Biome::Grassland), 0.0);
        let (_, plain) = traits_of(&values(), &MorphDesign::default(), ExtraKnobs::FULL);
        for b in [Biome::Forest, Biome::Grassland, Biome::Mountain] {
            assert_eq!(camouflage(&plain.morph, b), 0.0);
        }
    }
}
