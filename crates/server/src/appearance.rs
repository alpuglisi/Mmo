//! What an organism looks like from the outside: a handful of coarse body-plan
//! classes the client draws a sprite from. Derived from the phenotype and the
//! derived traits with a few comparisons (no allocation), so it costs nothing
//! to compute per organism per state push. Coarse on purpose: it reveals only
//! what anyone could see, and few classes keep the client's texture cache
//! small.
//!
//! The morphology part (eyes and their placement, ears, legs, tail, snout,
//! fangs; `docs/morphology-contract.md`) comes from [`Morphology::of`], the
//! one place that reads it off the organism (its genome's expressed
//! `Organism::morph`), and is quantized by
//! [`Appearance::with_morphology`]: counts and shapes as they are, continuous
//! sizes into three buckets (small, medium, large), so a sprite key has few
//! values per feature.

use evo_sim::genome::Gene;
pub use evo_sim::morph::{HornForm, Pattern};
use evo_sim::organism::Organism;
use serde::{Deserialize, Serialize};

/// Expression from which a visible gene shows on the body.
const SHOWS: f32 = 0.5;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Diet {
    Herbivore,
    Omnivore,
    Carnivore,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Locomotion {
    Walk,
    Swim,
    Glide,
    Fly,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Cover {
    Skin,
    Fur,
    Scales,
    Armor,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Coloring {
    /// Plain browns and greys.
    Drab,
    /// Mottled to match the ground.
    Camo,
    /// Display colors (a mate signal).
    Bright,
    /// Bright and toxic: aposematic warning colors.
    Warning,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum EarShape {
    #[default]
    Round,
    Pointed,
    Long,
    Fan,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum TailShape {
    #[default]
    Thin,
    Bushy,
    Fin,
    Club,
    Prehensile,
}

/// What a tail looks like on the sprite: none, or its shape.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TailLook {
    None,
    Thin,
    Bushy,
    Fin,
    Club,
    Prehensile,
}

/// The morphology parameters of `docs/morphology-contract.md`, unquantized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Morphology {
    /// 1–6.
    pub eye_count: u8,
    /// Degrees, 0–180: the first eye pair's angle from straight ahead.
    pub eye_azimuth: f32,
    /// 0–1.
    pub eye_size: f32,
    /// 0, 2 or 4 (contract v1).
    pub ear_count: u8,
    pub ear_shape: EarShape,
    /// 0–1.
    pub ear_size: f32,
    /// 0, 2, 4, 6 or 8.
    pub leg_count: u8,
    pub tail_present: bool,
    pub tail_shape: TailShape,
    /// 0–1.
    pub tail_length: f32,
    /// 0–1.
    pub snout_length: f32,
    /// 0–1; no fangs below [`Morphology::NO_FANGS`].
    pub fang_size: f32,
    /// 0–1; no horns below `evo_sim::morph::HORNS_SHOW`.
    pub horn_size: f32,
    pub horn_form: HornForm,
    /// 0 broad to 1 narrow.
    pub wing_aspect: f32,
    pub pattern: Pattern,
}

impl Morphology {
    /// A tail shows from this `Tail` expression.
    pub const TAIL_FROM: f32 = 0.05;
    /// Fangs below this size are not there.
    pub const NO_FANGS: f32 = 0.1;

    /// The organism's expressed morphology (`Organism::morph`, from its
    /// genome's morphology loci at birth): counts, shapes, eye placement and
    /// snout from the loci, sizes from the existing genes.
    pub fn of(o: &Organism) -> Self {
        let m = &o.morph;
        Self {
            eye_count: m.eye_count,
            eye_azimuth: m.eye_azimuth,
            eye_size: m.eye_size,
            ear_count: m.ear_count,
            ear_shape: m.ear_shape.into(),
            ear_size: m.ear_size,
            leg_count: m.leg_count,
            tail_present: m.tail_present,
            tail_shape: m.tail_shape.into(),
            tail_length: m.tail_length,
            snout_length: m.snout_length,
            fang_size: m.fang_size,
            horn_size: if m.horns_present { m.horn_size } else { 0.0 },
            horn_form: m.horn_form,
            wing_aspect: m.wing_aspect,
            pattern: m.pattern,
        }
    }
}

impl From<evo_sim::morph::EarShape> for EarShape {
    fn from(e: evo_sim::morph::EarShape) -> Self {
        use evo_sim::morph::EarShape as E;
        match e {
            E::Round => EarShape::Round,
            E::Pointed => EarShape::Pointed,
            E::Long => EarShape::Long,
            E::Fan => EarShape::Fan,
        }
    }
}

impl From<evo_sim::morph::TailShape> for TailShape {
    fn from(t: evo_sim::morph::TailShape) -> Self {
        use evo_sim::morph::TailShape as T;
        match t {
            T::Thin => TailShape::Thin,
            T::Bushy => TailShape::Bushy,
            T::Fin => TailShape::Fin,
            T::Club => TailShape::Club,
            T::Prehensile => TailShape::Prehensile,
        }
    }
}

/// Eye placement as a bucket: 0 forward (first pair under 35° from straight
/// ahead: a hunter's binocular face), 1 the default (35°–85°, around 60°),
/// 2 on the sides (85° and more: a grazer's all-round view).
fn azimuth_bucket(deg: f32) -> u8 {
    if deg < 35.0 {
        0
    } else if deg < 85.0 {
        1
    } else {
        2
    }
}

fn medium() -> u8 {
    1
}

fn is_zero(x: &u8) -> bool {
    *x == 0
}

fn is_curved(x: &HornForm) -> bool {
    *x == HornForm::Curved
}

fn is_plain(x: &Pattern) -> bool {
    *x == Pattern::Plain
}

fn is_medium(x: &u8) -> bool {
    *x == 1
}

/// A 0–1 size as a bucket: 0 small (< 1/3), 1 medium, 2 large (≥ 2/3).
fn bucket(x: f32) -> u8 {
    if x < 1.0 / 3.0 {
        0
    } else if x < 2.0 / 3.0 {
        1
    } else {
        2
    }
}

/// The visible body plan of an adult (the client scales juveniles down).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Appearance {
    /// Adult size class from mass: 0 tiny (< 1 kg), 1 small (< 8 kg),
    /// 2 medium (< 40 kg), 3 large.
    pub size: u8,
    pub diet: Diet,
    /// Warm-blooded.
    pub warm: bool,
    pub locomotion: Locomotion,
    pub cover: Cover,
    pub coloring: Coloring,
    /// Eye count, 1–6 (contract `eye_count`).
    pub eyes: u8,
    /// Eye placement bucket: 0 forward, 1 default, 2 sides. Sent only when
    /// not the default, so default bodies' looks are what they were.
    #[serde(default = "medium", skip_serializing_if = "is_medium")]
    pub eye_azimuth: u8,
    /// Eye size bucket: 0 small, 1 medium, 2 large.
    pub eye_size: u8,
    /// Ear count: 0, 2 or 4.
    pub ears: u8,
    pub ear_shape: EarShape,
    /// Ear size bucket: 0 small, 1 medium, 2 large.
    pub ear_size: u8,
    /// Leg count: 0, 2, 4, 6 or 8.
    pub legs: u8,
    pub tail: TailLook,
    /// Tail length bucket: 0 short, 1 medium, 2 long.
    pub tail_size: u8,
    /// Snout length bucket: 0 short, 1 medium, 2 long.
    pub snout: u8,
    /// Fangs: 0 none, 1 small, 2 large.
    pub fangs: u8,
    /// Horns (Phase 3): 0 none, 1 small, 2 large. Sent only when present.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub horns: u8,
    /// Horn form, sent only for horns that aren't curved (the default).
    #[serde(default, skip_serializing_if = "is_curved")]
    pub horn_form: HornForm,
    /// Wing shape bucket of a glider or flier: 0 broad, 1 default, 2
    /// narrow. Sent only when not the default.
    #[serde(default = "medium", skip_serializing_if = "is_medium")]
    pub wing_aspect: u8,
    /// Coat pattern, sent only when not plain.
    #[serde(default, skip_serializing_if = "is_plain")]
    pub pattern: Pattern,
}

impl Appearance {
    pub fn of(o: &Organism) -> Self {
        let p = &o.pheno;
        let t = &o.traits;
        let mass = t.body.mass;
        let size = if mass < 1.0 {
            0
        } else if mass < 8.0 {
            1
        } else if mass < 40.0 {
            2
        } else {
            3
        };
        let diet = diet_of(t.carnivory);
        let locomotion = if t.body.can_fly {
            Locomotion::Fly
        } else if t.body.can_glide {
            Locomotion::Glide
        } else if t.body.swim >= 0.6 {
            Locomotion::Swim
        } else {
            Locomotion::Walk
        };
        let (fur, scales, armor) = (p[Gene::Fur], p[Gene::Scales], p[Gene::Armor]);
        let cover = if armor >= SHOWS && armor >= fur && armor >= scales {
            Cover::Armor
        } else if fur >= SHOWS && fur >= scales {
            Cover::Fur
        } else if scales >= SHOWS {
            Cover::Scales
        } else {
            Cover::Skin
        };
        let coloring = if p[Gene::Coloration] >= SHOWS {
            if p[Gene::Toxins] >= SHOWS {
                Coloring::Warning
            } else {
                Coloring::Bright
            }
        } else if p[Gene::Camouflage] >= SHOWS {
            Coloring::Camo
        } else {
            Coloring::Drab
        };
        Self {
            size,
            diet,
            warm: t.warm,
            locomotion,
            cover,
            coloring,
            eyes: 2,
            eye_azimuth: 1,
            eye_size: 1,
            ears: 2,
            ear_shape: EarShape::Round,
            ear_size: 1,
            legs: 4,
            tail: TailLook::Thin,
            tail_size: 1,
            snout: 1,
            fangs: 0,
            horns: 0,
            horn_form: HornForm::Curved,
            wing_aspect: 1,
            pattern: Pattern::Plain,
        }
        .with_morphology(&Morphology::of(o))
    }

    /// This appearance with the morphology fields quantized from `m`: counts
    /// clamped to the contract's ranges (legs to an even count), sizes in
    /// three buckets, ear shape and size and tail length only when there are
    /// ears and a tail (so absent features add no distinct values).
    pub fn with_morphology(self, m: &Morphology) -> Self {
        let tail = if !m.tail_present {
            TailLook::None
        } else {
            match m.tail_shape {
                TailShape::Thin => TailLook::Thin,
                TailShape::Bushy => TailLook::Bushy,
                TailShape::Fin => TailLook::Fin,
                TailShape::Club => TailLook::Club,
                TailShape::Prehensile => TailLook::Prehensile,
            }
        };
        let fangs = if m.fang_size < Morphology::NO_FANGS {
            0
        } else if m.fang_size < 0.55 {
            1
        } else {
            2
        };
        let ears = (m.ear_count.min(4) / 2) * 2;
        let horns = if m.horn_size < evo_sim::morph::HORNS_SHOW {
            0
        } else if m.horn_size < 0.55 {
            1
        } else {
            2
        };
        let winged = matches!(self.locomotion, Locomotion::Glide | Locomotion::Fly);
        Self {
            horns,
            horn_form: if horns == 0 { HornForm::Curved } else { m.horn_form },
            // Broad below 0.35, narrow from 0.65 (founder variation around
            // the default 0.5 stays in the middle bucket).
            wing_aspect: if !winged {
                1
            } else if m.wing_aspect < 0.35 {
                0
            } else if m.wing_aspect < 0.65 {
                1
            } else {
                2
            },
            pattern: m.pattern,
            eyes: m.eye_count.clamp(1, 6),
            eye_azimuth: azimuth_bucket(m.eye_azimuth),
            eye_size: bucket(m.eye_size),
            ears,
            ear_shape: if ears == 0 { EarShape::Round } else { m.ear_shape },
            ear_size: if ears == 0 { 1 } else { bucket(m.ear_size) },
            legs: (m.leg_count.min(8) / 2) * 2,
            tail,
            tail_size: if tail == TailLook::None { 1 } else { bucket(m.tail_length) },
            snout: bucket(m.snout_length),
            fangs,
            ..self
        }
    }
}

/// The sprites' diet class of a carnivory (below 0.34 herbivore, below
/// 0.67 omnivore).
fn diet_of(carnivory: f32) -> Diet {
    if carnivory < 0.34 {
        Diet::Herbivore
    } else if carnivory < 0.67 {
        Diet::Omnivore
    } else {
        Diet::Carnivore
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_sim::genome::Genome;
    use evo_sim::math::Vec2;
    use evo_sim::organism::Sex;

    fn org(set: &[(Gene, f32)]) -> Organism {
        let mut v = [0.2; Gene::COUNT];
        for &(g, x) in set {
            v[g.index()] = x;
        }
        Organism::new(1, 1, Genome::uniform(&v, 1), Sex::Female, Vec2::new(0.0, 0.0))
    }

    #[test]
    fn classifies_body_plans() {
        let mouse = Appearance::of(&org(&[
            (Gene::BodySize, 0.0),
            (Gene::Carnivory, 0.0),
            (Gene::Endothermy, 0.9),
            (Gene::Fur, 0.8),
        ]));
        assert_eq!(mouse.size, 0);
        assert_eq!(mouse.diet, Diet::Herbivore);
        assert!(mouse.warm);
        assert_eq!(mouse.cover, Cover::Fur);
        assert_eq!(mouse.locomotion, Locomotion::Walk);
        assert_eq!(mouse.coloring, Coloring::Drab);

        let croc = Appearance::of(&org(&[
            (Gene::BodySize, 0.9),
            (Gene::Carnivory, 1.0),
            (Gene::Endothermy, 0.1),
            (Gene::Scales, 0.9),
            (Gene::Swimming, 0.9),
            (Gene::Camouflage, 0.8),
        ]));
        assert_eq!(croc.size, 3);
        assert_eq!(croc.diet, Diet::Carnivore);
        assert!(!croc.warm);
        assert_eq!(croc.cover, Cover::Scales);
        assert_eq!(croc.locomotion, Locomotion::Swim);
        assert_eq!(croc.coloring, Coloring::Camo);

        let frog = Appearance::of(&org(&[(Gene::Carnivory, 0.5), (Gene::Coloration, 0.9), (Gene::Toxins, 0.9)]));
        assert_eq!(frog.diet, Diet::Omnivore);
        assert_eq!(frog.coloring, Coloring::Warning);

        let bird = Appearance::of(&org(&[(Gene::BodySize, 0.05), (Gene::Wings, 0.95)]));
        assert_eq!(bird.locomotion, Locomotion::Fly);
    }

    #[test]
    fn serializes_compactly() {
        let a = Appearance::of(&org(&[(Gene::BodySize, 0.0), (Gene::Armor, 0.9)]));
        let v = serde_json::to_value(a).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "size": 0, "diet": "herbivore", "warm": false, "locomotion": "walk", "cover": "armor", "coloring": "drab",
                "eyes": 2, "eye_size": 0, "ears": 2, "ear_shape": "round", "ear_size": 0, "legs": 4,
                "tail": "thin", "tail_size": 0, "snout": 1, "fangs": 1
            })
        );
    }

    #[test]
    fn morphology_defaults_follow_the_contract() {
        let o = org(&[(Gene::Eyes, 0.9), (Gene::Ears, 0.5), (Gene::Tail, 0.0), (Gene::Fangs, 0.05)]);
        let m = Morphology::of(&o);
        assert_eq!((m.eye_count, m.ear_count, m.ear_shape, m.leg_count), (2, 2, EarShape::Round, 4));
        assert!(!m.tail_present);
        assert_eq!(m.snout_length, 0.5);
        let a = Appearance::of(&o);
        assert_eq!((a.eyes, a.eye_size, a.ears, a.ear_size, a.legs), (2, 2, 2, 1, 4));
        assert_eq!((a.tail, a.tail_size, a.snout, a.fangs), (TailLook::None, 1, 1, 0));
        let a = Appearance::of(&org(&[(Gene::Tail, 0.8), (Gene::Fangs, 0.9)]));
        assert_eq!((a.tail, a.tail_size, a.fangs), (TailLook::Thin, 2, 2));
    }

    #[test]
    fn quantizes_morphology_into_few_values() {
        let base = Appearance::of(&org(&[]));
        let m = Morphology {
            eye_count: 9,
            eye_azimuth: 120.0,
            eye_size: 0.5,
            ear_count: 0,
            ear_shape: EarShape::Fan,
            ear_size: 0.9,
            leg_count: 7,
            tail_present: true,
            tail_shape: TailShape::Club,
            tail_length: 0.2,
            snout_length: 1.0,
            fang_size: 0.3,
            horn_size: 0.0,
            horn_form: HornForm::Branched,
            wing_aspect: 0.9,
            pattern: Pattern::Plain,
        };
        let a = base.with_morphology(&m);
        // No horns: the form collapses; a walker's wing shape stays default.
        assert_eq!((a.horns, a.horn_form, a.wing_aspect), (0, HornForm::Curved, 1));
        let horned = base.with_morphology(&Morphology { horn_size: 0.8, pattern: Pattern::Spots, ..m });
        assert_eq!((horned.horns, horned.horn_form, horned.pattern), (2, HornForm::Branched, Pattern::Spots));
        let v = serde_json::to_value(horned).unwrap();
        assert_eq!(
            (v["horns"].as_u64(), v["horn_form"].as_str(), v["pattern"].as_str()),
            (Some(2), Some("branched"), Some("spots"))
        );
        let flier = Appearance { locomotion: Locomotion::Fly, ..base };
        assert_eq!(flier.with_morphology(&m).wing_aspect, 2);
        assert_eq!(flier.with_morphology(&Morphology { wing_aspect: 0.1, ..m }).wing_aspect, 0);
        assert_eq!(a.eyes, 6);
        assert_eq!(a.eye_azimuth, 2);
        assert_eq!(base.with_morphology(&Morphology { eye_azimuth: 20.0, ..m }).eye_azimuth, 0);
        assert_eq!(base.with_morphology(&Morphology { eye_azimuth: 60.0, ..m }).eye_azimuth, 1);
        assert_eq!(a.eye_size, 1);
        // No ears: their shape and size collapse to one value.
        assert_eq!((a.ears, a.ear_shape, a.ear_size), (0, EarShape::Round, 1));
        assert_eq!(a.legs, 6);
        assert_eq!(base.with_morphology(&Morphology { ear_count: 3, ..m }).ears, 2);
        assert_eq!((a.tail, a.tail_size), (TailLook::Club, 0));
        assert_eq!((a.snout, a.fangs), (2, 1));
        let no_tail = base.with_morphology(&Morphology { tail_present: false, ..m });
        assert_eq!((no_tail.tail, no_tail.tail_size), (TailLook::None, 1));
        // A continuous size sweep gives at most three values per feature.
        let mut sizes = std::collections::HashSet::new();
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            let a = base.with_morphology(&Morphology {
                eye_size: x,
                ear_count: 2,
                ear_size: x,
                tail_length: x,
                snout_length: x,
                fang_size: x,
                ..m
            });
            sizes.insert((a.eye_size, a.ear_size, a.tail_size, a.snout, a.fangs));
        }
        assert!(sizes.len() <= 5, "{sizes:?}");
    }
}
