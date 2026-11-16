//! The genome's body plan end to end: a founded design's morphology plays out
//! in its organisms' traits (`morph_effects`) and eyes (`vision`) once the
//! tunables are on, and its young inherit it by the dominance rule.

use super::World;
use crate::genome::{Gene, Genome, MutationParams};
use crate::math::Vec2;
use crate::morph::{MorphDesign, MorphLocus, Morphology, TailShape};
use crate::morph_effects::{apply_plan, BodyPlan, Knobs};
use crate::organism::{Organism, Sex};
use crate::rng::rng_for;
use crate::stats::{derive_with, Traits};
use crate::{presets, WorldConfig};

/// Six legs, three eyes (a midline eye between side eyes), a fin tail.
const BUG: MorphDesign = MorphDesign {
    eye_count: 3,
    eye_azimuth: 100.0,
    ear_count: 2,
    ear_shape: crate::morph::EarShape::Round,
    leg_count: 6,
    tail_shape: TailShape::Fin,
    snout_length: 0.5,
};

/// A world with one designed lineage (voles with a tail and some swimming,
/// built as [`BUG`]) and one default lineage, at tunable strength `k` for
/// both the body plan effects and directional vision (the other morphology
/// rules off).
fn world(k: f32) -> (World, u32, u32) {
    let mut config = WorldConfig { seed: 5, width: 48, height: 48, ..Default::default() };
    config.balance = config.balance.morphology_off();
    config.balance.morph_effects = k;
    config.balance.directional_vision = k;
    let mut w = World::new(config);
    let mut designs = presets::wild_designs().into_iter();
    let (owner, d) = designs.next().unwrap();
    let d = d.with(Gene::Tail, 0.5).with(Gene::Swimming, 0.3);
    let bugs = w.found_lineage_unchecked(owner, &d.clone().founders(12).with_morph(BUG));
    let plain = w.found_lineage_unchecked(owner, &d.founders(12));
    let (b, p) = (w.organisms[w.index[&bugs[0]]].lineage, w.organisms[w.index[&plain[0]]].lineage);
    (w, b, p)
}

/// What `World::insert` gives a body with this plan at these knobs.
fn expected(o: &Organism, plan: &BodyPlan, k: Knobs) -> Traits {
    let mut t = derive_with(&o.pheno, plan);
    apply_plan(&mut t, &o.pheno, plan, k);
    t
}

fn is_bug(m: &Morphology) -> bool {
    (m.leg_count, m.eye_count, m.tail_shape) == (6, 3, TailShape::Fin)
}

#[test]
fn a_designed_body_plan_plays_out_in_traits_and_eyes() {
    let (w, bugs, _) = world(1.0);
    let k = Knobs::of(&w.config.balance);
    let founders: Vec<&Organism> = w.organisms.iter().filter(|o| o.lineage == bugs).collect();
    assert_eq!(founders.len(), 12);
    for o in founders {
        assert!(is_bug(&o.morph), "{:?}", o.morph);
        assert!(o.morph.tail_present);
        // The organism's traits are its own body plan's.
        assert_eq!(o.traits, expected(o, &o.morph, k));
        // Against the same genes on the wild-type plan: six legs cost
        // endurance and add mass, and steady the body (defense); the fin
        // swims.
        let wild = expected(o, &Morphology::wild_type(&o.pheno), k);
        assert!(o.traits.stats.endurance < wild.stats.endurance);
        assert!(o.traits.body.mass > wild.body.mass);
        assert!(o.traits.stats.defense > wild.stats.defense);
        assert!(o.traits.body.swim > wild.body.swim + 0.1);
        assert!(o.traits.stats.metabolism > wild.stats.metabolism);
        // Three eyes: the midline eye closes the side eyes' front gap.
        let layout = crate::vision::eye_layout(&o.morph);
        assert_eq!(layout.count, 3);
        assert!((layout.azimuth - 100.0).abs() < 15.0, "azimuth {}", layout.azimuth);
        assert_eq!(o.traits.vision.front_gap, 0.0);
        let two = BodyPlan { eye_count: 2, ..o.morph };
        assert!(crate::vision::Vision::of(&two, o.traits.stats.perception).front_gap > 0.0);
        assert!(o.traits.vision.coverage > wild.vision.coverage);
        assert!(o.traits.vision.binocular > wild.vision.binocular);
    }
}

#[test]
fn with_the_tunables_off_only_the_eyes_follow_the_plan() {
    let (w, bugs, plain) = world(0.0);
    for o in &w.organisms {
        // No stat effects; the eye geometry (read only by directional
        // vision, also off) is the plan's.
        assert_eq!(o.traits, derive_with(&o.pheno, &o.morph));
        if o.lineage == plain {
            let m = o.morph;
            assert_eq!((m.eye_count, m.ear_count, m.leg_count, m.tail_shape), (2, 2, 4, TailShape::Thin));
        } else {
            assert_eq!(o.lineage, bugs);
            assert!(is_bug(&o.morph));
        }
    }
}

#[test]
fn young_inherit_the_body_plan() {
    let (mut w, bugs, plain) = world(1.0);
    let k = Knobs::of(&w.config.balance);
    w.run_days(8);
    let young: Vec<&Organism> = w.organisms.iter().filter(|o| o.lineage == bugs && o.generation > 0).collect();
    assert!(!young.is_empty(), "want young of the designed lineage");
    for o in &young {
        assert!(is_bug(&o.morph), "young {:?}", o.morph);
        assert_eq!(o.traits, expected(o, &o.morph, k));
    }

    // A cross with the default lineage shows the wild type (dominant) and
    // carries the design; crossing two such carriers brings it back.
    let pick = |l: u32| w.organisms.iter().find(|o| o.lineage == l).unwrap().genome.clone();
    let (bug, wild) = (pick(bugs), pick(plain));
    let none = MutationParams { rate: 0.0, sd: 0.0 };
    let mut rng = rng_for(11, 0, 0, 0);
    let hybrid = Genome::inherit(&bug, &wild, 1, none, 0.0, &mut rng);
    let m = hybrid.morphology();
    assert_eq!((m.leg_count, m.eye_count, m.tail_shape), (4, 2, TailShape::Thin));
    let legs = hybrid.morph().discrete[MorphLocus::LegCount.index()];
    assert_eq!((legs.a.value, legs.b.value), (6, 4));
    let id = w.insert(Organism::new(0, 1, hybrid.clone(), Sex::Female, Vec2::new(24.0, 24.0)));
    let o = &w.organisms[w.index[&id]];
    assert_eq!(o.traits, expected(o, &o.morph, k));
    assert_eq!(o.traits.vision.front_gap, crate::vision::Vision::of(&o.morph, o.traits.stats.perception).front_gap);
    let f2: Vec<Morphology> =
        (0..64).map(|_| Genome::inherit(&hybrid, &hybrid, 1, none, 0.0, &mut rng).morphology()).collect();
    assert!(f2.iter().any(|m| m.leg_count == 6), "six legs return in the second generation");
    assert!(f2.iter().any(|m| m.leg_count == 4));
    assert!(f2.iter().all(|m| [4, 6].contains(&m.leg_count) && [2, 3].contains(&m.eye_count)));
}
