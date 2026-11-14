//! Shared burrows and sentries (critical-mass group traits) in the world.

use super::{Intent, World};
use crate::genome::Gene;
use crate::group::GroupTrait;
use crate::math::Vec2;
use crate::organism::{Action, Organism};
use crate::rng::{rng_for, stream};
use crate::{presets, WorldConfig};

const CENTER: Vec2 = Vec2 { x: 24.0, y: 24.0 };

fn world() -> World {
    presets::standard_world(WorldConfig { seed: 4, width: 48, height: 48, ..Default::default() }, 1)
}

fn set_benefit(w: &mut World, t: GroupTrait, b: f32) {
    for r in &mut w.group.benefit {
        r[t as usize] = b;
    }
}

fn set_gene(o: &mut Organism, g: Gene, v: f32) {
    o.pheno.0[g.index()] = v;
    if g == Gene::Burrowing {
        o.traits.body.burrow = v;
    }
}

/// A grown pack hunter at `pos`, with the 2.6 kg body (body size 0.42) the
/// threat levels below were set for: the sample design's own 0.8 kg hunter
/// is too small to threaten a herd grazer or an antelope.
fn big_hunter(w: &mut World, pos: Vec2) -> usize {
    let h = w.organisms.iter().position(|o| o.lineage == 4).unwrap();
    let o = &mut w.organisms[h];
    o.pheno.0[Gene::BodySize.index()] = 0.42;
    o.traits = crate::stats::derive(&o.pheno);
    (o.growth, o.matured, o.pos) = (1.0, true, pos);
    h
}

/// A grown member of `lineage`, everyone else moved to a far corner.
fn isolate(w: &mut World, lineage: u32) -> usize {
    let i = w.organisms.iter().position(|o| o.lineage == lineage).unwrap();
    for o in w.organisms.iter_mut() {
        o.pos = Vec2::new(1.0, 1.0);
    }
    let o = &mut w.organisms[i];
    (o.growth, o.matured, o.mother, o.pos) = (1.0, true, None, CENTER);
    (o.needs.hunger, o.needs.thirst, o.needs.energy, o.needs.breeding) = (10.0, 10.0, 90.0, 0.0);
    w.map.tile_at_mut(CENTER).water_depth = 0.0;
    i
}

/// Rest in place for one tick.
fn rest(w: &mut World, i: usize) {
    let intent = Intent { action: Action::Rest, ..Intent::idle() };
    let mut rng = rng_for(1, stream::APPLY, w.tick, 0);
    w.apply(i, &intent, &mut rng);
}

/// A burrowing omnivore with only modest burrowing shelters in a shared
/// burrow where the trait holds and it carries it, and nowhere else. Its
/// young share the burrow even if they don't carry the trait.
#[test]
fn shared_burrow_shelters_carriers_where_it_holds() {
    let t = GroupTrait::SharedBurrows;
    let mut w = world();
    let i = isolate(&mut w, 7);
    set_gene(&mut w.organisms[i], Gene::Burrowing, 0.45);
    set_gene(&mut w.organisms[i], Gene::GroupTolerance, 0.8);
    assert!(t.is_carrier(&w.organisms[i].pheno));
    let mut loner = w.organisms[i].clone();
    set_gene(&mut loner, Gene::GroupTolerance, 0.2);

    set_benefit(&mut w, t, 0.0);
    assert_eq!(w.burrow_depth(&w.organisms[i]), 0.45);
    rest(&mut w, i);
    assert!(!w.organisms[i].sheltered, "no shared burrow without the trait");

    set_benefit(&mut w, t, 1.0);
    assert!(w.burrow_depth(&w.organisms[i]) > 0.6);
    assert_eq!(w.burrow_depth(&loner), 0.45, "non-carriers get nothing");
    rest(&mut w, i);
    assert!(w.organisms[i].sheltered, "carrier shelters in the shared burrow");

    // Dependent young of a carrier mother share her burrow.
    let mut young = loner.clone();
    (young.matured, young.growth, young.mother) = (false, 0.3, Some(w.organisms[i].id));
    assert!(!t.is_carrier(&young.pheno));
    assert_eq!(w.shared_burrow_bonus(&young), 1.0);
    young.growth = 0.9;
    assert_eq!(w.shared_burrow_bonus(&young), 0.0);

    w.config.balance.shared_burrow = 0.0;
    assert_eq!(w.burrow_depth(&w.organisms[i]), 0.45, "0 disables");
}

/// A shared burrow keeps out more cold than a lone burrow just as deep.
#[test]
fn shared_burrow_buffers_weather_better() {
    let t = GroupTrait::SharedBurrows;
    let mut w = world();
    let i = isolate(&mut w, 7);
    set_gene(&mut w.organisms[i], Gene::Burrowing, 0.9);
    w.organisms[i].traits.stats.insulation = 0.0;
    w.map.tile_at_mut(CENTER).temp = -40.0;
    set_benefit(&mut w, t, 1.0);
    let cold_in = |w: &mut World, gt: f32| {
        set_gene(&mut w.organisms[i], Gene::GroupTolerance, gt);
        rest(w, i);
        assert!(w.organisms[i].sheltered);
        w.organisms[i].needs.cold
    };
    let lone = cold_in(&mut w, 0.2);
    let shared = cold_in(&mut w, 0.8);
    assert!(shared < lone - 5.0, "shared {shared} vs lone {lone}");
}

/// Sets up a herd grazer with a kin sentry next to it that has spotted a
/// threat out of the grazer's own sight. Returns (grazer, sentry).
fn herd_with_sentry(w: &mut World) -> (usize, usize) {
    let a = isolate(w, 5);
    let s = w.organisms.iter().position(|o| o.lineage == 5 && o.id != w.organisms[a].id).unwrap();
    let o = &mut w.organisms[s];
    (o.growth, o.matured, o.mother, o.pos) = (1.0, true, None, CENTER + Vec2::new(1.5, 0.0));
    o.action = Action::Sentry;
    o.alert = Some((CENTER + Vec2::new(7.0, 0.0), 1.0));
    w.grid.rebuild(&w.organisms);
    (a, s)
}

/// A kin sentry's warning reaches group members only where sentries hold,
/// and only group members.
#[test]
fn sentry_warns_group_members_where_the_trait_holds() {
    let t = GroupTrait::Sentries;
    let mut w = world();
    let (a, _) = herd_with_sentry(&mut w);
    set_benefit(&mut w, t, 0.0);
    assert_eq!(w.decide(a).threat, 0.0, "nothing in sight");
    set_benefit(&mut w, t, 1.0);
    let warned = w.decide(a);
    assert!(warned.threat > 0.5, "warned {}", warned.threat);
    assert_eq!(warned.action, Action::Flee);
    assert!(warned.alert.is_none(), "warnings are not relayed");
    set_benefit(&mut w, t, 0.3);
    let weak = w.decide(a).threat;
    assert!(weak < warned.threat && weak > 0.0, "weak {weak}");
    // A loner that wandered into the herd's region isn't a group member.
    set_benefit(&mut w, t, 1.0);
    set_gene(&mut w.organisms[a], Gene::GroupTolerance, 0.3);
    assert_eq!(w.decide(a).threat, 0.0);
}

/// A vigilant herd member takes turns on watch, pays for it in food, and
/// raises the alarm when it spots a predator.
#[test]
fn sentries_take_turns_on_watch() {
    let t = GroupTrait::Sentries;
    let mut w = world();
    let s = isolate(&mut w, 5);
    set_gene(&mut w.organisms[s], Gene::Vigilance, 0.8);
    assert!(t.is_carrier(&w.organisms[s].pheno));
    // Some of its herd around it.
    let herd: Vec<usize> =
        w.organisms.iter().enumerate().filter(|(j, o)| o.lineage == 5 && *j != s).map(|(j, _)| j).take(4).collect();
    for (k, &j) in herd.iter().enumerate() {
        w.organisms[j].pos = CENTER + Vec2::new(1.0, 0.0).rotate(k as f32 * 1.5);
    }
    w.grid.rebuild(&w.organisms);
    set_benefit(&mut w, t, 1.0);
    let shift = 2 * w.config.ticks_per_hour();
    let shifts = 400;
    let on: Vec<u64> = (0..shifts)
        .map(|k| k * shift)
        .filter(|&tick| {
            w.tick = tick;
            w.on_watch_duty(&w.organisms[s])
        })
        .collect();
    let share = on.len() as f32 / shifts as f32;
    assert!((share - w.config.balance.sentry_share).abs() < 0.07, "share {share}");
    w.tick = on[0];
    assert_eq!(w.decide(s).action, Action::Sentry);
    set_benefit(&mut w, t, 0.0);
    assert_ne!(w.decide(s).action, Action::Sentry, "no watch where the trait doesn't hold");
    set_benefit(&mut w, t, 1.0);
    let mut idler = w.organisms[s].clone();
    set_gene(&mut idler, Gene::Vigilance, 0.0);
    assert!(!w.on_watch_duty(&idler));

    // On watch it doesn't eat and burns more than standing idle.
    let hunger_after = |w: &mut World, action: Action| {
        let tile = w.map.tile_at_mut(CENTER);
        (tile.veg, tile.meat) = (0.0, 0.0);
        w.organisms[s].needs.hunger = 40.0;
        let intent = Intent { action, ..Intent::idle() };
        let mut rng = rng_for(1, stream::APPLY, w.tick, 0);
        w.apply(s, &intent, &mut rng);
        w.organisms[s].needs.hunger
    };
    let watching = hunger_after(&mut w, Action::Sentry);
    let idle = hunger_after(&mut w, Action::Idle);
    assert!(watching > idle, "watching {watching} vs idle {idle}");

    // A hungry sentry leaves its post to graze.
    w.organisms[s].needs.hunger = 60.0;
    assert_ne!(w.decide(s).action, Action::Sentry);

    // A wolf comes into view: the sentry raises the alarm.
    (w.organisms[s].action, w.organisms[s].needs.hunger) = (Action::Sentry, 10.0);
    let wolf = w.organisms.iter().position(|o| o.lineage == 4).unwrap();
    (w.organisms[wolf].growth, w.organisms[wolf].traits.body.mass) = (1.0, 20.0);
    assert!(super::decide::can_threaten(&w.organisms[wolf], &w.organisms[s]));
    w.organisms[wolf].pos = CENTER + Vec2::new(5.0, 0.0);
    w.grid.rebuild(&w.organisms);
    let seen = w.decide(s);
    assert!(seen.threat > 0.0, "wolf unseen");
    let (at, level) = seen.alert.expect("alarm raised");
    assert!(at.dist(w.organisms[wolf].pos) < 1e-3 && level > 0.0);
}

/// Breeding-season risk: high breeding drive blunts the urge to flee a
/// predator in sight (`breeding_recklessness`), not just perception.
#[test]
fn breeding_drive_blunts_threat_response() {
    let mut w = world();
    let a = isolate(&mut w, 5);
    set_gene(&mut w.organisms[a], Gene::Vigilance, 0.0);
    big_hunter(&mut w, CENTER + Vec2::new(2.0, 0.0));
    w.grid.rebuild(&w.organisms);
    w.organisms[a].needs.breeding = 60.0;
    w.config.balance.breeding_recklessness = 0.0;
    let calm = w.decide(a);
    assert!(calm.threat > 0.0);
    assert_eq!(calm.action, Action::Flee, "threat {}", calm.threat);
    // (With a threat this close, courting is on hold too: see below.)
    w.config.balance.courtship_caution = 0.0;
    w.config.balance.breeding_recklessness = 1.0;
    assert_ne!(w.decide(a).action, Action::Flee, "high drive and full recklessness: courts instead");
    // Without breeding drive it flees whatever the setting.
    w.organisms[a].needs.breeding = 0.0;
    assert_eq!(w.decide(a).action, Action::Flee);
}

/// A threat in sight puts courting on hold (`courtship_caution`): an animal
/// in full breeding drive with a willing mate beside it flees a hunter it
/// would otherwise ignore for the mate.
#[test]
fn a_seen_threat_puts_courting_on_hold() {
    let mut w = world();
    let a = isolate(&mut w, 5);
    set_gene(&mut w.organisms[a], Gene::Vigilance, 0.0);
    w.organisms[a].breed_ready_tick = 0;
    w.organisms[a].needs.breeding = 100.0;
    let sex = w.organisms[a].sex;
    let m = w.organisms.iter().position(|o| o.lineage == 5 && o.sex != sex).unwrap();
    let o = &mut w.organisms[m];
    (o.growth, o.matured, o.mother, o.pos, o.breed_ready_tick) = (1.0, true, None, CENTER + Vec2::new(-1.0, 0.0), 0);
    (o.needs.breeding, o.needs.hunger, o.pregnancy) = (100.0, 10.0, None);
    let h = big_hunter(&mut w, CENTER + Vec2::new(2.0, 0.0));
    w.grid.rebuild(&w.organisms);
    w.config.balance.courtship_caution = 0.0;
    let reckless = w.decide(a);
    assert!(reckless.threat > 0.2 && reckless.threat < 1.0, "threat {}", reckless.threat);
    assert_eq!(reckless.action, Action::SeekMate, "full drive outranks a moderate threat");
    w.config.balance.courtship_caution = 1.0;
    assert_eq!(w.decide(a).action, Action::Flee);
    // With nothing in sight it courts as before.
    w.organisms[h].pos = Vec2::new(1.0, 1.0);
    w.grid.rebuild(&w.organisms);
    assert_eq!(w.decide(a).action, Action::SeekMate);
}

/// The herd guards its young (`herd_guard`): where herd defense holds, a
/// sub-adult or an orphan with kin around gets the protection a mother
/// gives a dependent calf, scaled by the benefit; nothing alone, nothing
/// where the trait doesn't hold, nothing with the rule off.
#[test]
fn the_herd_guards_its_young() {
    let t = GroupTrait::HerdDefense;
    let mut w = world();
    let i = isolate(&mut w, 5);
    assert!(t.is_carrier(&w.organisms[i].pheno));
    let o = &mut w.organisms[i];
    (o.matured, o.growth, o.mother, o.kin_nearby) = (false, 0.8, None, 5);
    set_benefit(&mut w, t, 1.0);
    let (rs, power) = w.guardian(&w.organisms[i]);
    assert!(rs > 0.5 * w.organisms[i].resistance() && power > 0.0, "sub-adult orphan: rs {rs} power {power}");
    // An orphaned calf too.
    w.organisms[i].growth = 0.3;
    assert!(w.guardian(&w.organisms[i]).0 > 0.0);
    set_benefit(&mut w, t, 0.5);
    let half = w.guardian(&w.organisms[i]).0;
    assert!(half > 0.0 && half < rs, "half {half} vs {rs}");
    set_benefit(&mut w, t, 1.0);
    w.organisms[i].kin_nearby = 1;
    assert_eq!(w.guardian(&w.organisms[i]), (0.0, 0.0), "no herd around");
    w.organisms[i].kin_nearby = 5;
    set_benefit(&mut w, t, 0.0);
    assert_eq!(w.guardian(&w.organisms[i]), (0.0, 0.0), "trait doesn't hold");
    set_benefit(&mut w, t, 1.0);
    w.organisms[i].matured = true;
    assert_eq!(w.guardian(&w.organisms[i]), (0.0, 0.0), "adults defend themselves");
    w.organisms[i].matured = false;
    w.config.balance.herd_guard = 0.0;
    assert_eq!(w.guardian(&w.organisms[i]), (0.0, 0.0), "0 disables");
}

/// Vigilance at water and at rest (`threat_caution`): a thirsty antelope
/// with a pack hunter in sight flees instead of drinking, unless it is
/// parched; a tired one flees instead of resting.
#[test]
fn a_seen_threat_puts_drinking_and_resting_on_hold() {
    let mut w = world();
    let a = isolate(&mut w, 1);
    set_gene(&mut w.organisms[a], Gene::Vigilance, 0.0);
    big_hunter(&mut w, CENTER + Vec2::new(2.5, 0.0));
    w.grid.rebuild(&w.organisms);
    w.organisms[a].needs.thirst = 75.0;
    w.config.balance.threat_caution = 0.0;
    let careless = w.decide(a);
    assert!(careless.threat > 0.1 && careless.threat < 0.7, "threat {}", careless.threat);
    assert_eq!(careless.action, Action::Drink, "thirst outranks a moderate threat");
    w.config.balance.threat_caution = 1.0;
    assert_eq!(w.decide(a).action, Action::Flee);
    // The parched still drink.
    w.organisms[a].needs.thirst = 97.0;
    assert_eq!(w.decide(a).action, Action::Drink);
    // Tired, not thirsty: flees rather than rests.
    (w.organisms[a].needs.thirst, w.organisms[a].needs.energy) = (10.0, 15.0);
    w.config.balance.threat_caution = 0.0;
    assert_eq!(w.decide(a).action, Action::Rest);
    w.config.balance.threat_caution = 1.0;
    assert_eq!(w.decide(a).action, Action::Flee);
}

/// Foraging on stripped ground (`pasture_search`): a hungry grazer on a bare
/// tile steps to the remnant on the next tile instead of passing it by.
#[test]
fn a_forager_on_bare_ground_eats_the_remnant_next_door() {
    let mut w = world();
    let a = isolate(&mut w, 5);
    set_gene(&mut w.organisms[a], Gene::Vigilance, 0.0);
    w.organisms[a].needs.hunger = 70.0;
    for t in w.map.tiles.iter_mut() {
        t.veg = 0.0;
    }
    let (x, y) = w.map.tile_coords(CENTER);
    let k = w.map.idx(x + 1, y);
    w.map.tiles[k].veg = 40.0;
    w.map.tiles[k].water_depth = 0.0;
    w.grid.rebuild(&w.organisms);
    let intent = w.decide(a);
    assert_eq!(intent.action, Action::Graze);
    let next = crate::terrain::Map::tile_center(x + 1, y);
    assert_eq!(intent.target, Some(next), "{:?}", intent.target);
}
