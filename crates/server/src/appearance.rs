//! What an organism looks like from the outside: a handful of coarse body-plan
//! classes the client draws a sprite from. Derived from the phenotype and the
//! derived traits with a few comparisons (no allocation), so it costs nothing
//! to compute per organism per state push. Coarse on purpose: it reveals only
//! what anyone could see, and few classes keep the client's texture cache
//! small.
//!
//! The morphology part (eyes, ears, legs, tail, snout, fangs;
//! `docs/morphology-contract.md`) comes from [`Morphology::of`], the one place
//! that reads it off the organism, and is quantized by
//! [`Appearance::with_morphology`]: counts and shapes as they are, continuous
//! sizes into three buckets (small, medium, large), so a sprite key has few
//! values per feature.

use evo_sim::genome::Gene;
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
}

impl Morphology {
    /// A tail shows from this `Tail` expression.
    pub const TAIL_FROM: f32 = 0.05;
    /// Fangs below this size are not there.
    pub const NO_FANGS: f32 = 0.1;

    /// The organism's morphology. Until the genome carries the new discrete
    /// loci: the contract defaults (two eyes, two round ears, four legs, a
    /// thin tail when the `Tail` gene shows, a medium snout) with sizes from
    /// the existing genes. The one function to change when it does.
    pub fn of(o: &Organism) -> Self {
        let p = &o.pheno;
        Self {
            eye_count: 2,
            eye_size: p[Gene::Eyes],
            ear_count: 2,
            ear_shape: EarShape::Round,
            ear_size: p[Gene::Ears],
            leg_count: 4,
            tail_present: p[Gene::Tail] >= Self::TAIL_FROM,
            tail_shape: TailShape::Thin,
            tail_length: p[Gene::Tail],
            snout_length: 0.5,
            fang_size: p[Gene::Fangs],
        }
    }
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
        let diet = if t.carnivory < 0.34 {
            Diet::Herbivore
        } else if t.carnivory < 0.67 {
            Diet::Omnivore
        } else {
            Diet::Carnivore
        };
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
            eye_size: 1,
            ears: 2,
            ear_shape: EarShape::Round,
            ear_size: 1,
            legs: 4,
            tail: TailLook::Thin,
            tail_size: 1,
            snout: 1,
            fangs: 0,
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
        Self {
            eyes: m.eye_count.clamp(1, 6),
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
        };
        let a = base.with_morphology(&m);
        assert_eq!(a.eyes, 6);
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
