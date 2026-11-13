//! What an organism looks like from the outside: a handful of coarse body-plan
//! classes the client draws a sprite from. Derived from the phenotype and the
//! derived traits with a few comparisons (no allocation), so it costs nothing
//! to compute per organism per state push. Coarse on purpose: it reveals only
//! what anyone could see, and few classes keep the client's texture cache
//! small.

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
        Self { size, diet, warm: t.warm, locomotion, cover, coloring }
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
        let mouse = Appearance::of(&org(&[(Gene::BodySize, 0.0), (Gene::Carnivory, 0.0), (Gene::Endothermy, 0.9), (Gene::Fur, 0.8)]));
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
            serde_json::json!({"size": 0, "diet": "herbivore", "warm": false, "locomotion": "walk", "cover": "armor", "coloring": "drab"})
        );
    }
}
