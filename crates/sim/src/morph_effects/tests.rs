use super::*;
use crate::design::{BirthType, FounderDesign, Thermo};
use crate::stats::derive as derive_traits;
use crate::terrain::{traverse, Biome, Ground, Tile, Traverse, Weather};

const ON: Knobs = Knobs { effects: 1.0, ..Knobs::OFF };

fn pheno(d: &FounderDesign) -> Phenotype {
    Phenotype(d.values())
}

/// A mid-sized warm-blooded body with a bit of everything the formulas read.
fn base_design() -> FounderDesign {
    design(Thermo::Warm)
}

fn design(thermo: Thermo) -> FounderDesign {
    use Gene::*;
    FounderDesign::new("t", thermo, BirthType::Live, 2)
        .with(BodySize, 0.3)
        .with(Ears, 0.5)
        .with(Nose, 0.4)
        .with(Fangs, 0.3)
        .with(Fur, 0.4)
        .with(LegLength, 0.4)
        .with(FootWidth, 0.2)
        .with(Claws, 0.2)
        .with(Tail, 0.5)
        .with(Muscle, 0.2)
        .with(Climbing, 0.2)
        .with(Swimming, 0.1)
        .with(Burrowing, 0.2)
}

fn plan(p: &Phenotype) -> BodyPlan {
    body_plan(p)
}

fn traits(p: &Phenotype, plan: &BodyPlan, k: Knobs) -> Traits {
    let mut t = derive_traits(p);
    apply_plan(&mut t, p, plan, k);
    t
}

fn ground(slope: f32, sand: f32, veg: f32) -> Ground {
    Ground {
        biome: Biome::Grassland,
        elevation: 0.5,
        moisture: 0.5,
        base_temp: 15.0,
        base_water: 0.0,
        river: 0.0,
        sand,
        slope,
        veg_density: veg,
        canopy: 0.0,
        veg_max: 40.0,
        plain: 0.0,
    }
}

fn tile() -> Tile {
    Tile {
        temp: 15.0,
        water_depth: 0.0,
        current: 0.0,
        ice: 0.0,
        snow: 0.0,
        mud: 0.0,
        veg: 30.0,
        meat: 0.0,
        wind: 0.3,
        rain: 0.0,
        wetness: 0.5,
        silt: 0.0,
    }
}

fn speed_on(t: &Traits, g: &Ground, tile: &Tile) -> f32 {
    match traverse(&t.body, g, tile, &Weather::still(0.1), false) {
        Traverse::Passable { speed, .. } => speed,
        Traverse::Blocked => 0.0,
    }
}

/// Every preset's founders, and a spread of size genes, keep bit-identical
/// traits at the default body plan with the effects at full strength.
#[test]
fn default_body_is_exactly_neutral() {
    let mut designs: Vec<FounderDesign> = crate::presets::player_designs();
    designs.extend(crate::presets::wild_designs().into_iter().map(|(_, d)| d));
    designs.push(crate::presets::migrant_design());
    designs.push(base_design());
    for x in [0.0, 0.03, 0.1, 0.37, 0.8, 1.0] {
        use Gene::*;
        let mut d = base_design();
        for g in [Ears, Nose, Fangs, Fur, LegLength, Hooves, FootWidth, Claws, Tail, Armor, Wings, Muscle, BodySize] {
            d = d.with(g, x);
        }
        designs.push(d);
    }
    for d in &designs {
        let p = pheno(d);
        let raw = derive_traits(&p);
        let e = derive(&p, &plan(&p), ON, &raw);
        for f in [e.stability_factor, e.ground_pressure_factor, e.mass_factor, e.climb_factor, e.burrow_factor] {
            assert_eq!(f, 1.0, "{}", d.name);
        }
        assert_eq!((e.small_prey_escape, e.bite_factor), (1.0, 1.0));
        for x in [
            e.d_speed,
            e.d_strength,
            e.d_endurance,
            e.d_agility,
            e.d_perception,
            e.d_stealth,
            e.d_defense,
            e.d_insulation,
            e.d_upkeep,
            e.d_height,
            e.d_width,
            e.d_swim,
            e.d_climb,
            e.d_burrow,
            e.d_grip,
            e.heat_tolerance,
            e.rest_insulation,
            e.fight_back,
        ] {
            assert_eq!(x, 0.0, "{}", d.name);
        }
        let mut t = raw;
        apply_plan(&mut t, &p, &plan(&p), ON);
        assert_eq!(t, raw, "{}", d.name);
        // And through the Balance knob.
        let mut t = raw;
        apply(&mut t, &p, &Balance { morph_effects: 1.0, ..Balance::default() });
        assert_eq!(t, raw, "{}", d.name);
    }
}

/// With every tunable at 0 nothing changes, whatever the body plan.
#[test]
fn knobs_off_change_nothing() {
    let p = pheno(&base_design());
    let raw = derive_traits(&p);
    let odd = BodyPlan {
        ear_count: 4,
        ear_shape: EarShape::Fan,
        leg_count: 0,
        tail_shape: TailShape::Club,
        snout_length: 1.0,
        ..plan(&p)
    };
    assert_eq!(traits(&p, &odd, Knobs::OFF), raw);
    let mut t = raw;
    apply(&mut t, &p, &Balance::default());
    assert_eq!(t, raw);
}

/// Six legs are steadier on slopes and ice and carry armor better; they
/// cost speed, endurance, food and mass.
#[test]
fn six_legs_are_steadier() {
    let d = base_design().with(Gene::Hooves, 0.6).with(Gene::Armor, 0.5);
    let p = pheno(&d);
    let four = traits(&p, &plan(&p), ON);
    let six = traits(&p, &BodyPlan { leg_count: 6, ..plan(&p) }, ON);
    let e = derive(&p, &BodyPlan { leg_count: 6, ..plan(&p) }, ON, &derive_traits(&p));
    assert!(e.stability > STABILITY_REF && e.stability_factor < 1.0);
    let slope = ground(0.5, 0.0, 0.0);
    assert!(speed_on(&six, &slope, &tile()) > speed_on(&four, &slope, &tile()));
    let mut ice = tile();
    (ice.water_depth, ice.ice) = (0.5, 2.0);
    assert!(speed_on(&six, &ground(0.0, 0.0, 0.0), &ice) > speed_on(&four, &ground(0.0, 0.0, 0.0), &ice));
    // Armor's speed penalty shrinks, but the gait costs more than it saves.
    assert!(e.d_speed < 0.0 && e.d_speed > (0.92 - 1.0) * (0.45 * 0.4 + 0.25 * 0.6));
    assert!(six.stats.endurance < four.stats.endurance);
    assert!(six.stats.metabolism > four.stats.metabolism);
    assert!(six.body.mass > four.body.mass);
    assert!(six.stats.defense > four.stats.defense);
    // More feet spread the weight: easier on sand.
    let sand = ground(0.0, 0.8, 0.0);
    assert!(six.body.ground_pressure < four.body.ground_pressure);
    assert!(speed_on(&six, &sand, &tile()) > speed_on(&four, &sand, &tile()) * 0.99);
    // Eight legs climb as well as six (capped) and pay more.
    let eight = traits(&p, &BodyPlan { leg_count: 8, ..plan(&p) }, ON);
    assert_eq!(eight.body.climb, six.body.climb);
    assert!(eight.stats.speed < six.stats.speed && eight.stats.metabolism > six.stats.metabolism);
}

/// A legless body is slower but stealthier, burrows and swims better, is
/// low and good on sand.
#[test]
fn legless_is_slow_low_and_stealthy() {
    let p = pheno(&base_design());
    let four = traits(&p, &plan(&p), ON);
    let none = traits(&p, &BodyPlan { leg_count: 0, ..plan(&p) }, ON);
    assert!(none.stats.speed < 0.8 * four.stats.speed);
    assert!(none.stats.stealth > four.stats.stealth);
    assert!(none.body.burrow > four.body.burrow + 0.15);
    assert!(none.body.swim > four.body.swim + 0.1);
    assert!(none.body.height < four.body.height);
    assert!(none.body.climb < four.body.climb);
    assert!(none.stats.metabolism < four.stats.metabolism);
    // Belly area 1.5 against a foot area of 0.58: much lighter on sand.
    assert!(none.body.ground_pressure < 0.5 * four.body.ground_pressure);
    let sand = ground(0.0, 0.8, 0.0);
    let (rel_none, rel_four) = (
        speed_on(&none, &sand, &tile()) / speed_on(&none, &ground(0.0, 0.0, 0.0), &tile()),
        speed_on(&four, &sand, &tile()) / speed_on(&four, &ground(0.0, 0.0, 0.0), &tile()),
    );
    assert!(rel_none > rel_four, "sand: legless keeps {rel_none}, four legs {rel_four}");
}

/// A biped is tall and tippy and needs a tail to balance.
#[test]
fn biped_needs_a_tail() {
    let d = base_design().with(Gene::Tail, 0.0);
    let p = pheno(&d);
    let biped = BodyPlan { leg_count: 2, ..plan(&p) };
    let tailless = traits(&p, &biped, ON);
    let four = traits(&p, &plan(&p), ON);
    assert!((tailless.stats.agility - (four.stats.agility - 0.15)).abs() < 1e-6);
    assert!(tailless.body.height > four.body.height);
    let pt = pheno(&base_design());
    let tailed = traits(&pt, &BodyPlan { leg_count: 2, ..plan(&pt) }, ON);
    let e = derive(&pt, &BodyPlan { leg_count: 2, ..plan(&pt) }, ON, &derive_traits(&pt));
    assert!(e.stability < STABILITY_REF && e.stability > 0.45);
    assert_eq!(tailed.stats.agility, traits(&pt, &plan(&pt), ON).stats.agility);
    // Wings as forelimbs pay no extra-limb mass or upkeep.
    let pw = pheno(&base_design().with(Gene::Wings, 0.6));
    let bird = traits(&pw, &BodyPlan { leg_count: 2, ..plan(&pw) }, ON);
    let dragon = traits(&pw, &plan(&pw), ON);
    assert!(bird.body.mass < dragon.body.mass && bird.stats.metabolism < dragon.stats.metabolism);
}

/// A fin tail swims better and is slower on land.
#[test]
fn fin_tail_swims() {
    let p = pheno(&base_design().with(Gene::Swimming, 0.4));
    let thin = traits(&p, &plan(&p), ON);
    let fin = traits(&p, &BodyPlan { tail_shape: TailShape::Fin, ..plan(&p) }, ON);
    assert!((fin.body.swim - thin.body.swim - 0.2).abs() < 1e-6);
    assert!(fin.stats.speed < thin.stats.speed);
    let mut deep = tile();
    deep.water_depth = 3.0;
    assert!(speed_on(&fin, &ground(0.0, 0.0, 0.0), &deep) > speed_on(&thin, &ground(0.0, 0.0, 0.0), &deep));
}

/// A club tail defends and fights back, and is heavy and slow.
#[test]
fn club_tail_defends() {
    let p = pheno(&base_design());
    let thin = traits(&p, &plan(&p), ON);
    let club = traits(&p, &BodyPlan { tail_shape: TailShape::Club, ..plan(&p) }, ON);
    assert!((club.stats.defense - thin.stats.defense - 0.15).abs() < 1e-6);
    assert!((club.morph.fight_back - 0.15).abs() < 1e-6 && thin.morph.fight_back == 0.0);
    assert!(club.body.mass > thin.body.mass && club.stats.speed < thin.stats.speed);
    assert!(club.stats.agility < thin.stats.agility);
}

/// Bushy tails warm a resting warm body and give it away; prehensile tails
/// climb and grip and cost food.
#[test]
fn bushy_and_prehensile_tails() {
    let p = pheno(&base_design());
    let thin = traits(&p, &plan(&p), ON);
    let bushy = traits(&p, &BodyPlan { tail_shape: TailShape::Bushy, ..plan(&p) }, ON);
    assert!((bushy.morph.rest_insulation - 0.075).abs() < 1e-6);
    assert!(bushy.stats.stealth < thin.stats.stealth);
    let cold = pheno(&design(Thermo::Cold));
    assert_eq!(traits(&cold, &BodyPlan { tail_shape: TailShape::Bushy, ..plan(&cold) }, ON).morph.rest_insulation, 0.0);
    let pre = traits(&p, &BodyPlan { tail_shape: TailShape::Prehensile, ..plan(&p) }, ON);
    assert!((pre.body.climb - thin.body.climb - 0.15).abs() < 1e-6);
    assert!(pre.body.grip > thin.body.grip && pre.stats.metabolism > thin.stats.metabolism);
}

/// A long tail drags in burrows and dense vegetation, and a thin one loses
/// heat; both only with their tunables on, and longer tails lose more.
#[test]
fn long_tail_drags_and_chills() {
    let k = Knobs { tail_drag: 1.0, tail_chill: 1.0, ..Knobs::OFF };
    let short = pheno(&base_design().with(Gene::Tail, 0.1));
    let long = pheno(&base_design().with(Gene::Tail, 0.9));
    let (s, l) = (traits(&short, &plan(&short), k), traits(&long, &plan(&long), k));
    let (s0, l0) = (derive_traits(&short), derive_traits(&long));
    assert!(l.body.burrow < l0.body.burrow && (l.body.burrow - 0.2 * (1.0 - 0.18)).abs() < 1e-6);
    assert!(l0.body.burrow - l.body.burrow > s0.body.burrow - s.body.burrow);
    let forest = ground(0.0, 0.0, 0.9);
    let drag = |t: &Traits, raw: &Traits| speed_on(t, &forest, &tile()) / speed_on(raw, &forest, &tile());
    assert!(drag(&l, &l0) < drag(&s, &s0) && drag(&l, &l0) < 0.9);
    // Open ground: no drag.
    assert_eq!(speed_on(&l, &ground(0.0, 0.0, 0.0), &tile()), speed_on(&l0, &ground(0.0, 0.0, 0.0), &tile()));
    assert!((l0.stats.insulation - l.stats.insulation - 0.045).abs() < 1e-6);
    let bushy = traits(&long, &BodyPlan { tail_shape: TailShape::Bushy, ..plan(&long) }, k);
    assert_eq!(bushy.stats.insulation, l0.stats.insulation);
    // No tail, no drag.
    let none = pheno(&base_design().with(Gene::Tail, 0.0));
    assert_eq!(traits(&none, &plan(&none), k), derive_traits(&none));
}

/// Long legs are tippy and dig worse with their tunable on.
#[test]
fn long_legs_are_tippy() {
    let k = Knobs { long_legs: 1.0, ..Knobs::OFF };
    let p = pheno(&base_design().with(Gene::LegLength, 0.9));
    let raw = derive_traits(&p);
    let t = traits(&p, &plan(&p), k);
    assert!(t.body.grip < raw.body.grip && t.body.burrow < raw.body.burrow);
    assert!(speed_on(&t, &ground(0.5, 0.0, 0.0), &tile()) < speed_on(&raw, &ground(0.5, 0.0, 0.0), &tile()));
}

/// Fan ears shed heat (a desert asset), cost warmth in the cold unless
/// furred, hear further and give a standing animal away.
#[test]
fn fan_ears_shed_heat() {
    let p = pheno(&base_design().with(Gene::Fur, 0.0).with(Gene::FatStores, 0.8));
    let round = traits(&p, &plan(&p), ON);
    let fan = traits(&p, &BodyPlan { ear_shape: EarShape::Fan, ..plan(&p) }, ON);
    assert!((fan.morph.heat_tolerance - 6.0 * 0.5).abs() < 1e-6);
    assert!(fan.stats.insulation < round.stats.insulation);
    assert!(fan.stats.stealth < round.stats.stealth);
    assert!(fan.stats.perception > round.stats.perception);
    assert!(fan.morph.hearing > round.morph.hearing);
    // Fur blunts the cold liability.
    let furred = pheno(&base_design().with(Gene::Fur, 0.8).with(Gene::FatStores, 0.8));
    let loss = |p: &Phenotype| {
        traits(p, &plan(p), ON).stats.insulation
            - traits(p, &BodyPlan { ear_shape: EarShape::Fan, ..plan(p) }, ON).stats.insulation
    };
    assert!(loss(&furred) < loss(&p));
}

/// Four ears hear further and locate better; no ears feel vibration only
/// and pay no ear upkeep; pointed ears locate best of one pair.
#[test]
fn ear_count_and_localization() {
    let p = pheno(&base_design());
    let two = traits(&p, &plan(&p), ON);
    let four = traits(&p, &BodyPlan { ear_count: 4, ..plan(&p) }, ON);
    let none = traits(&p, &BodyPlan { ear_count: 0, ..plan(&p) }, ON);
    assert!(four.morph.hearing > two.morph.hearing && four.morph.localization > two.morph.localization);
    assert!((none.morph.hearing - 0.01).abs() < 1e-6);
    assert_eq!(none.morph.localization, 0.0);
    assert!(none.stats.metabolism < two.stats.metabolism && four.stats.metabolism > two.stats.metabolism);
    assert!(none.stats.perception < two.stats.perception);
    let pointed = BodyPlan { ear_shape: EarShape::Pointed, ..plan(&p) };
    assert_eq!(localization(&pointed), 0.7);
    assert!((bearing_error_deg(localization(&plan(&p))) - 42.0).abs() < 1e-4);
    assert_eq!(heard_bearing(10.0, 1.0, 1.0), 10.0);
    assert!((heard_bearing(0.0, 0.4, -2.0) + 42.0).abs() < 1e-4);
    // The channels add up to the perception stat.
    let eyes = 0.10 + 0.3 * p[Gene::Eyes];
    for t in [&two, &four, &none] {
        assert!((eyes + t.morph.hearing + t.morph.smell - t.stats.perception).abs() < 1e-6);
    }
}

/// An earless body can't carry alarm calls (once the genome expresses ear
/// count; until then every body has two ears).
#[test]
fn alarm_calls_need_ears() {
    let p = pheno(&base_design().with(Gene::Ears, 0.8).with(Gene::Vocal, 0.8));
    assert!(crate::group::GroupTrait::AlarmCalls.is_carrier(&p));
}

/// A long snout smells better and bites weaker; a flat face bites hardest,
/// cools worse and keeps its binocular overlap; long snouts snap small prey.
#[test]
fn snout_trades_smell_for_bite() {
    let p = pheno(&base_design());
    let mid = traits(&p, &plan(&p), ON);
    let long = traits(&p, &BodyPlan { snout_length: 1.0, ..plan(&p) }, ON);
    let flat = traits(&p, &BodyPlan { snout_length: 0.0, ..plan(&p) }, ON);
    assert!(long.stats.perception > mid.stats.perception && mid.stats.perception > flat.stats.perception);
    assert!((long.morph.smell - (0.02 + 0.3 * 0.4 * 1.3)).abs() < 1e-6);
    assert!(flat.stats.strength > mid.stats.strength && mid.stats.strength > long.stats.strength);
    assert!((flat.stats.strength - mid.stats.strength - 0.3 * 0.3 * 0.3).abs() < 1e-6);
    assert!(long.morph.small_prey_escape < 1.0 && flat.morph.small_prey_escape > 1.0);
    assert!(long.morph.heat_tolerance > 0.0 && flat.morph.heat_tolerance < 0.0);
    assert!(long.body.burrow > flat.body.burrow);
    let e = derive(&p, &BodyPlan { snout_length: 1.0, ..plan(&p) }, ON, &derive_traits(&p));
    assert_eq!(e.snout_occlusion_deg, 20.0);
    assert_eq!(derive(&p, &BodyPlan { snout_length: 0.2, ..plan(&p) }, ON, &mid).snout_occlusion_deg, 0.0);
}

/// The eye geometry (`Traits::vision`) follows the body plan: its snout
/// hides the same overlap `snout_occlusion_deg` reports, and the sight
/// channel's share is of the perception the body plan changed.
#[test]
fn vision_follows_the_body_plan() {
    let p = pheno(&base_design());
    let mid = traits(&p, &plan(&p), ON);
    let long = traits(&p, &BodyPlan { snout_length: 1.0, ..plan(&p) }, ON);
    let e = derive(&p, &BodyPlan { snout_length: 1.0, ..plan(&p) }, ON, &derive_traits(&p));
    assert!((mid.vision.binocular - long.vision.binocular - e.snout_occlusion_deg).abs() < 1e-4);
    let sight = crate::vision::sight(p[Gene::Eyes]);
    assert!((long.vision.visual_share - sight / long.stats.perception).abs() < 1e-6);
    assert!((sight + long.morph.hearing + long.morph.smell - long.stats.perception).abs() < 1e-5);
    // Off, the geometry is the birth-time one.
    assert_eq!(traits(&p, &plan(&p), Knobs::OFF).vision, derive_traits(&p).vision);
}

/// Half strength gives half of each additive change.
#[test]
fn effects_scale_with_their_tunable() {
    let p = pheno(&base_design());
    let fan = BodyPlan { ear_shape: EarShape::Fan, leg_count: 6, snout_length: 0.9, ..plan(&p) };
    let raw = derive_traits(&p);
    let full = derive(&p, &fan, ON, &raw);
    let half = derive(&p, &fan, Knobs { effects: 0.5, ..Knobs::OFF }, &raw);
    assert!((half.d_perception - 0.5 * full.d_perception).abs() < 1e-6);
    assert!((half.d_strength - 0.5 * full.d_strength).abs() < 1e-6);
    assert!((half.heat_tolerance - 0.5 * full.heat_tolerance).abs() < 1e-6);
    assert!((half.mass_factor - 1.0 - 0.5 * (full.mass_factor - 1.0)).abs() < 1e-6);
}

/// A standard world whose bodies are all default runs bit for bit the same
/// with the count/shape effects at full strength as without them.
#[test]
fn default_world_is_unchanged_by_the_effects() {
    let run = |k: f32| {
        let mut config = crate::WorldConfig { seed: 7, width: 48, height: 48, ..Default::default() };
        config.balance.morph_effects = k;
        let mut w = crate::presets::standard_world(config, 1);
        for _ in 0..150 {
            w.step();
        }
        w.organisms.iter().map(|o| (o.id, o.pos, o.health, o.needs.hunger, o.traits)).collect::<Vec<_>>()
    };
    assert!(run(1.0) == run(0.0));
}

/// A restore applies the effects of the restored config, as birth did.
#[cfg(feature = "serde")]
#[test]
fn restore_reapplies_effects() {
    let mut config = crate::WorldConfig { seed: 3, width: 32, height: 32, ..Default::default() };
    config.balance.morph_tail_drag = 1.0;
    config.balance.morph_long_legs = 1.0;
    let mut w = crate::presets::standard_world(config, 1);
    w.step();
    let tailed = w.organisms.iter().find(|o| o.pheno[Gene::Tail] > 0.2).expect("a tailed organism");
    assert_ne!(tailed.traits, derive_traits(&tailed.pheno));
    let r = crate::World::restore(&w.snapshot(), Some(w.map.terrain().clone())).unwrap();
    for (a, b) in w.organisms.iter().zip(&r.organisms) {
        assert_eq!(a.traits, b.traits);
    }
}

// ---------------------------------------------------------------------------
// Fixture for the client's readout (`docs/morphology.md` §7: "the client copy
// of `morph::derive` is tested against a fixture generated from Rust").
// ---------------------------------------------------------------------------

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/morph_effects.json");

fn num(x: f32) -> String {
    let s = format!("{x:.6}");
    if s == "-0.000000" {
        "0.000000".into()
    } else {
        s
    }
}

fn samples() -> Vec<(&'static str, FounderDesign, BodyPlan)> {
    use Gene::*;
    let d = base_design;
    let p = |d: &FounderDesign| plan(&pheno(d));
    let with = |d: FounderDesign, f: &dyn Fn(BodyPlan) -> BodyPlan| {
        let pl = f(p(&d));
        (d, pl)
    };
    let mut v: Vec<(&'static str, FounderDesign, BodyPlan)> = Vec::new();
    let mut add = |name: &'static str, (d, pl): (FounderDesign, BodyPlan)| v.push((name, d, pl));
    add("default", with(d(), &|b| b));
    add("no ears", with(d(), &|b| BodyPlan { ear_count: 0, ..b }));
    add("pointed ears", with(d(), &|b| BodyPlan { ear_shape: EarShape::Pointed, ..b }));
    add("long ears", with(d(), &|b| BodyPlan { ear_shape: EarShape::Long, ..b }));
    add("fan ears", with(d(), &|b| BodyPlan { ear_shape: EarShape::Fan, ..b }));
    add("fan ears, no fur", with(d().with(Fur, 0.0), &|b| BodyPlan { ear_shape: EarShape::Fan, ..b }));
    add("four round ears", with(d(), &|b| BodyPlan { ear_count: 4, ..b }));
    add("four fan ears", with(d(), &|b| BodyPlan { ear_count: 4, ear_shape: EarShape::Fan, ..b }));
    add("legless", with(d(), &|b| BodyPlan { leg_count: 0, ..b }));
    add("biped", with(d(), &|b| BodyPlan { leg_count: 2, ..b }));
    add("biped, short tail", with(d().with(Tail, 0.1), &|b| BodyPlan { leg_count: 2, ..b }));
    add("biped with wings", with(d().with(Wings, 0.6), &|b| BodyPlan { leg_count: 2, ..b }));
    add("six legs", with(d(), &|b| BodyPlan { leg_count: 6, ..b }));
    add("six legs, armor, hooves", with(d().with(Armor, 0.6).with(Hooves, 0.6), &|b| BodyPlan { leg_count: 6, ..b }));
    add("eight legs", with(d(), &|b| BodyPlan { leg_count: 8, ..b }));
    add("long legs", with(d().with(LegLength, 0.9), &|b| b));
    add("no tail", with(d().with(Tail, 0.0), &|b| b));
    add("bushy tail", with(d(), &|b| BodyPlan { tail_shape: TailShape::Bushy, ..b }));
    add("fin tail", with(d(), &|b| BodyPlan { tail_shape: TailShape::Fin, ..b }));
    add("club tail", with(d(), &|b| BodyPlan { tail_shape: TailShape::Club, ..b }));
    add("prehensile tail", with(d(), &|b| BodyPlan { tail_shape: TailShape::Prehensile, ..b }));
    add("long tail", with(d().with(Tail, 1.0), &|b| b));
    add("flat face", with(d(), &|b| BodyPlan { snout_length: 0.0, ..b }));
    add("long snout", with(d(), &|b| BodyPlan { snout_length: 1.0, ..b }));
    add(
        "cold-blooded fan ears, long snout",
        with(design(Thermo::Cold), &|b| BodyPlan { ear_shape: EarShape::Fan, snout_length: 1.0, ..b }),
    );
    add(
        "burrower: legless, flat face",
        with(d().with(Burrowing, 0.6), &|b| BodyPlan { leg_count: 0, snout_length: 0.1, ..b }),
    );
    add("swimmer: fin tail", with(d().with(Swimming, 0.6), &|b| BodyPlan { tail_shape: TailShape::Fin, ..b }));
    add(
        "six-legged climber",
        with(d().with(Climbing, 0.6).with(Claws, 0.6), &|b| BodyPlan {
            leg_count: 6,
            tail_shape: TailShape::Prehensile,
            ..b
        }),
    );
    v
}

fn traits_json(t: &Traits) -> String {
    let (s, b) = (&t.stats, &t.body);
    let fields = [
        ("speed", s.speed),
        ("strength", s.strength),
        ("endurance", s.endurance),
        ("agility", s.agility),
        ("perception", s.perception),
        ("stealth", s.stealth),
        ("defense", s.defense),
        ("metabolism", s.metabolism),
        ("insulation", s.insulation),
        ("mass", b.mass),
        ("height", b.height),
        ("width", b.width),
        ("foot_area", b.foot_area),
        ("ground_pressure", b.ground_pressure),
        ("wing_loading", b.wing_loading),
        ("grip", b.grip),
        ("swim", b.swim),
        ("climb", b.climb),
        ("burrow", b.burrow),
        ("hearing", t.morph.hearing),
        ("smell", t.morph.smell),
        ("localization", t.morph.localization),
        ("heat_tolerance", t.morph.heat_tolerance),
        ("rest_insulation", t.morph.rest_insulation),
        ("fight_back", t.morph.fight_back),
        ("small_prey_escape", t.morph.small_prey_escape),
    ];
    let mut out: Vec<String> = fields.iter().map(|(k, v)| format!("\"{k}\": {}", num(*v))).collect();
    out.push(format!("\"can_fly\": {}", b.can_fly));
    out.push(format!("\"can_glide\": {}", b.can_glide));
    format!("{{{}}}", out.join(", "))
}

fn effects_json(e: &MorphEffects) -> String {
    let fields = [
        ("hearing", e.hearing),
        ("localization", e.localization),
        ("bearing_error_deg", e.bearing_error_deg),
        ("radiating_area", e.radiating_area),
        ("gait", e.gait),
        ("stability", e.stability),
        ("stability_factor", e.stability_factor),
        ("foot_area", e.foot_area),
        ("ground_pressure_factor", e.ground_pressure_factor),
        ("mass_factor", e.mass_factor),
        ("climb_factor", e.climb_factor),
        ("tail", e.tail),
        ("burrow_factor", e.burrow_factor),
        ("smell", e.smell),
        ("bite_factor", e.bite_factor),
        ("snout_occlusion_deg", e.snout_occlusion_deg),
        ("heat_tolerance", e.heat_tolerance),
        ("rest_insulation", e.rest_insulation),
        ("fight_back", e.fight_back),
        ("small_prey_escape", e.small_prey_escape),
        ("d_speed", e.d_speed),
        ("d_strength", e.d_strength),
        ("d_endurance", e.d_endurance),
        ("d_agility", e.d_agility),
        ("d_perception", e.d_perception),
        ("d_stealth", e.d_stealth),
        ("d_defense", e.d_defense),
        ("d_insulation", e.d_insulation),
        ("d_upkeep", e.d_upkeep),
        ("d_metabolism", e.d_metabolism),
        ("d_height", e.d_height),
        ("d_width", e.d_width),
        ("d_swim", e.d_swim),
        ("d_climb", e.d_climb),
        ("d_burrow", e.d_burrow),
        ("d_grip", e.d_grip),
    ];
    let out: Vec<String> = fields.iter().map(|(k, v)| format!("\"{k}\": {}", num(*v))).collect();
    format!("{{{}}}", out.join(", "))
}

fn fixture_json() -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("  \"about\": \"Body plan effects (crates/sim/src/morph_effects.rs) at full strength (every morph_* tunable 1), for the client readout. Regenerate: EVO_UPDATE_FIXTURES=1 cargo test -p evo-sim --lib morph_effects::tests::fixture\",\n");
    s.push_str("  \"version\": 1,\n");
    s.push_str("  \"samples\": [\n");
    let all = samples();
    for (i, (name, d, pl)) in all.iter().enumerate() {
        let p = pheno(d);
        let raw = derive_traits(&p);
        let e = derive(&p, pl, Knobs::FULL, &raw);
        let mut t = raw;
        e.apply_to(&mut t);
        let genes: Vec<String> =
            Gene::ALL.iter().map(|&g| format!("\"{}\": {}", g.label().replace(' ', "_"), num(p[g]))).collect();
        let plan = format!(
            "{{\"eye_count\": {}, \"eye_azimuth\": {}, \"ear_count\": {}, \"ear_shape\": \"{}\", \"leg_count\": {}, \"tail_present\": {}, \"tail_shape\": \"{}\", \"snout_length\": {}}}",
            pl.eye_count,
            num(pl.eye_azimuth),
            pl.ear_count,
            pl.ear_shape.name(),
            pl.leg_count,
            pl.tail_present,
            pl.tail_shape.name(),
            num(pl.snout_length)
        );
        s.push_str("    {\n");
        s.push_str(&format!("      \"name\": \"{name}\",\n"));
        s.push_str(&format!("      \"warm\": {},\n", raw.warm));
        s.push_str(&format!("      \"foot\": \"{}\",\n", Foot::of(&p).name()));
        s.push_str(&format!("      \"plan\": {plan},\n"));
        s.push_str(&format!("      \"genes\": {{{}}},\n", genes.join(", ")));
        s.push_str(&format!("      \"effects\": {},\n", effects_json(&e)));
        s.push_str(&format!("      \"base\": {},\n", traits_json(&raw)));
        s.push_str(&format!("      \"effective\": {}\n", traits_json(&t)));
        s.push_str(if i + 1 == all.len() { "    }\n" } else { "    },\n" });
    }
    s.push_str("  ]\n}\n");
    s
}

/// The checked-in fixture is what the code computes now. Regenerate with
/// `EVO_UPDATE_FIXTURES=1`.
#[test]
fn fixture_is_current() {
    let json = fixture_json();
    if std::env::var_os("EVO_UPDATE_FIXTURES").is_some() {
        std::fs::create_dir_all(std::path::Path::new(FIXTURE).parent().unwrap()).unwrap();
        std::fs::write(FIXTURE, &json).unwrap();
        return;
    }
    let stored = std::fs::read_to_string(FIXTURE).expect("fixture missing: run with EVO_UPDATE_FIXTURES=1");
    assert!(stored == json, "{FIXTURE} is stale: regenerate with EVO_UPDATE_FIXTURES=1");
}
