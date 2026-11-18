//! Predators against players (version 36; *Predators against players* in
//! the README): attack orders while possessing, pack hunts on larger prey
//! and territory defence against larger intruders.

use super::commands::CommandError;
use super::path_tests::{flat_world, place, walker};
use super::World;
use crate::design::{BirthType, FounderDesign, Thermo};
use crate::genome::{Gene, PlayerId};
use crate::math::Vec2;
use crate::organism::{Action, DeathCause};

const HUNTER: PlayerId = 1;
const PREY: PlayerId = 2;

fn hunter() -> FounderDesign {
    FounderDesign::new("hunter", Thermo::Warm, BirthType::Live, 2)
        .with(Gene::BodySize, 0.5)
        .with(Gene::Carnivory, 0.9)
        .with(Gene::PackInstinct, 0.95)
        .with(Gene::GroupTolerance, 0.85)
        .with(Gene::LegLength, 0.8)
        .with(Gene::Muscle, 0.7)
        .with(Gene::Claws, 0.6)
        .with(Gene::Fangs, 0.6)
        .founders(1)
}

/// A big armed animal of another kind (a cold-blooded body plan: never kin
/// of the walkers), one that could threaten them.
fn big() -> FounderDesign {
    FounderDesign::new("brute", Thermo::Cold, BirthType::Live, 2)
        .with(Gene::BodySize, 0.95)
        .with(Gene::Carnivory, 0.7)
        .with(Gene::Fangs, 0.5)
        .with(Gene::Muscle, 0.6)
        .with(Gene::Claws, 0.4)
        .founders(1)
}

/// A flat world a few ticks in (lineages founded now are newcomers, with a
/// founding grace when `attack_grace` is set), with no attack grace, so
/// they can be attacked at once.
fn world() -> World {
    let mut w = flat_world(1500.0);
    w.config.balance.attack_grace = 0.0;
    w.tick = 5;
    w
}

/// `n` grown organisms of one lineage of `design` around `at`, rested and
/// fed, with the design's exact genes; their indices.
fn pack(w: &mut World, owner: PlayerId, design: &FounderDesign, n: usize, at: Vec2) -> Vec<usize> {
    w.found_lineage_unchecked(owner, &design.clone().founders(n as u32));
    let first = w.organisms.len() - n;
    let genome: std::sync::Arc<_> = crate::Genome::uniform(&design.values(), owner).into();
    for (k, i) in (first..w.organisms.len()).enumerate() {
        let o = &mut w.organisms[i];
        o.genome = genome.clone();
        o.pheno = o.genome.phenotype();
        o.traits = crate::stats::derive(&o.pheno);
        (o.growth, o.matured) = (1.0, true);
        o.pos = at + Vec2::new(0.4 * k as f32, 0.3 * (k % 2) as f32);
        (o.needs.energy, o.needs.hunger, o.needs.thirst, o.health) = (100.0, 10.0, 0.0, 100.0);
    }
    (first..w.organisms.len()).collect()
}

fn grown(w: &mut World, i: usize) {
    let o = &mut w.organisms[i];
    (o.growth, o.matured, o.health) = (1.0, true, 100.0);
}

#[test]
fn attack_orders_are_refused_with_reasons() {
    let mut w = world();
    let a = place(&mut w, HUNTER, &hunter(), Vec2::new(10.5, 10.5));
    grown(&mut w, a);
    let mine = place(&mut w, HUNTER, &walker(), Vec2::new(12.5, 10.5));
    let prey = place(&mut w, PREY, &walker(), Vec2::new(12.5, 10.5));
    grown(&mut w, prey);
    let far = place(&mut w, PREY, &walker(), Vec2::new(30.5, 20.5));
    let huge = place(&mut w, PREY, &big(), Vec2::new(11.5, 11.5));
    grown(&mut w, huge);
    let id = |w: &World, i: usize| w.organisms[i].id;
    let (aid, pid) = (id(&w, a), id(&w, prey));
    // Not possessed yet.
    assert_eq!(w.possess_attack(HUNTER, aid, pid), Err(CommandError::NotPossessed(aid)));
    assert_eq!(w.possess_attack(PREY, aid, pid), Err(CommandError::NotOwned(aid)));
    w.possess(HUNTER, aid).unwrap();
    assert_eq!(w.possess_attack(HUNTER, aid, id(&w, mine)), Err(CommandError::OwnTarget(id(&w, mine))));
    assert_eq!(w.possess_attack(HUNTER, aid, id(&w, far)), Err(CommandError::OutOfSight(id(&w, far))));
    assert!(matches!(w.possess_attack(HUNTER, aid, id(&w, huge)), Err(CommandError::TooBig { .. })));
    w.organisms[prey].sheltered = true;
    assert_eq!(w.possess_attack(HUNTER, aid, pid), Err(CommandError::Sheltered(pid)));
    w.organisms[prey].sheltered = false;
    // A freshly founded player lineage has its grace.
    w.config.balance.attack_grace = 10.0;
    assert!(
        matches!(w.possess_attack(HUNTER, aid, pid), Err(CommandError::NewcomerGrace { days_left }) if days_left > 9.0)
    );
    w.config.balance.attack_grace = 0.0;
    // Every message reads as a sentence for the player.
    for e in [CommandError::OwnTarget(1), CommandError::TooBig { mass: 300.0, limit: 40.0 }] {
        assert!(e.to_string().len() > 20, "{e}");
    }
    assert_eq!(w.possess_attack(HUNTER, aid, pid), Ok(()));
    assert_eq!(w.attack_target(aid), Some(pid));
    // A move order replaces it, and releasing drops it.
    w.order_move(HUNTER, aid, Vec2::new(5.0, 5.0)).unwrap();
    assert_eq!(w.attack_target(aid), None);
    w.possess_attack(HUNTER, aid, pid).unwrap();
    assert_eq!(w.organisms[a].order, None);
    w.release(HUNTER, aid).unwrap();
    assert_eq!(w.attack_target(aid), None);
}

#[test]
fn the_grace_runs_out() {
    let mut w = world();
    w.config.balance.attack_grace = 2.0;
    let a = place(&mut w, HUNTER, &hunter(), Vec2::new(10.5, 10.5));
    grown(&mut w, a);
    let prey = place(&mut w, PREY, &walker(), Vec2::new(12.5, 10.5));
    let (aid, pid) = (w.organisms[a].id, w.organisms[prey].id);
    w.possess(HUNTER, aid).unwrap();
    assert!(matches!(w.possess_attack(HUNTER, aid, pid), Err(CommandError::NewcomerGrace { .. })));
    // Other players' animals don't hunt it either; the wild's do.
    assert!(w.grace_spares(&w.organisms[a], &w.organisms[prey]));
    let wild = place(&mut w, crate::WILD, &hunter(), Vec2::new(30.5, 20.5));
    assert!(!w.grace_spares(&w.organisms[wild], &w.organisms[prey]));
    // The world's first players (founded at tick 0) have none.
    let mut first = flat_world(1500.0);
    first.config.balance.attack_grace = 2.0;
    let f = place(&mut first, PREY, &walker(), Vec2::new(12.5, 10.5));
    assert_eq!(first.attack_grace_left(&first.organisms[f]), None);
    w.run_days(3);
    assert!(!w.grace_spares(&w.organisms[a], &w.organisms[prey]));
    if w.organisms[a].alive && w.organisms[prey].alive {
        w.organisms[prey].pos = w.organisms[a].pos + Vec2::new(2.0, 0.0);
        w.organisms[prey].sheltered = false;
        w.possess(HUNTER, aid).unwrap();
        assert_eq!(w.possess_attack(HUNTER, aid, pid), Ok(()));
    }
}

#[test]
fn the_grace_is_two_sided_lasts_in_real_time_and_survives_refounding() {
    let mut w = flat_world(1500.0);
    w.config.balance.attack_grace = 2.0;
    let tpd = w.config.ticks_per_day as u64;
    // An established player (founded at tick 0, no grace) and a newcomer.
    let prey = place(&mut w, PREY, &walker(), Vec2::new(12.5, 10.5));
    grown(&mut w, prey);
    w.tick = 5;
    let a = place(&mut w, HUNTER, &hunter(), Vec2::new(10.5, 10.5));
    grown(&mut w, a);
    let (aid, pid) = (w.organisms[a].id, w.organisms[prey].id);
    w.possess(HUNTER, aid).unwrap();
    // The graced newcomer can't attack other players, by order or on its own.
    assert!(matches!(w.possess_attack(HUNTER, aid, pid), Err(CommandError::OwnGrace { days_left }) if days_left > 1.9));
    assert!(w.grace_spares(&w.organisms[a], &w.organisms[prey]));
    assert!(CommandError::OwnGrace { days_left: 1.5 }.to_string().contains("2 days"));
    // The wild's animals are fair game for it.
    let wild = place(&mut w, crate::WILD, &walker(), Vec2::new(30.5, 20.5));
    assert!(!w.grace_spares(&w.organisms[a], &w.organisms[wild]));
    // A minimum of real time: at a fast clock the grace lasts longer.
    let l = w.organisms[a].lineage;
    w.set_attack_grace_min_ticks(10 * tpd);
    assert!(w.lineage_grace_left(l).unwrap() > 9.9);
    w.set_attack_grace_min_ticks(0);
    // Re-founding a day later continues the first grace instead of a new one.
    w.tick += tpd;
    let b = place(&mut w, HUNTER, &hunter(), Vec2::new(40.5, 40.5));
    let left = w.lineage_grace_left(w.organisms[b].lineage).unwrap();
    assert!((left - w.lineage_grace_left(l).unwrap()).abs() < 1e-6 && left < 1.01, "{left}");
    // Its grace ends with the first one's, then the newcomer may attack.
    w.tick += tpd;
    assert_eq!(w.lineage_grace_left(w.organisms[b].lineage), None);
    assert_eq!(w.possess_attack(HUNTER, aid, pid), Ok(()));
    // After a long enough break, founding again starts a new grace.
    w.tick += 5 * tpd;
    let c = place(&mut w, HUNTER, &hunter(), Vec2::new(60.5, 60.5));
    assert!(w.lineage_grace_left(w.organisms[c].lineage).unwrap() > 1.99);
    // And it survives a snapshot (the starts are rebuilt on restore).
    #[cfg(feature = "serde")]
    {
        let back = World::restore(&w.snapshot(), Some(w.map.terrain().clone())).unwrap();
        assert_eq!(back.lineage_grace_left(w.organisms[b].lineage), None);
        assert!(back.lineage_grace_left(w.organisms[c].lineage).unwrap() > 1.99);
    }
}

#[test]
fn refounding_chains_on_the_whole_grace_and_an_ended_grace_stays_ended() {
    let mut w = flat_world(1500.0);
    w.config.balance.attack_grace = 2.0;
    let tpd = w.config.ticks_per_day as u64;
    // A real-time minimum of 10 days: the grace's whole length.
    w.set_attack_grace_min_ticks(10 * tpd);
    assert_eq!(w.grace_length(), 10 * tpd);
    w.tick = 5;
    let a = place(&mut w, HUNTER, &hunter(), Vec2::new(10.5, 10.5));
    let la = w.organisms[a].lineage;
    // Re-founding 6 days on (past twice the 2 game days, within the
    // running grace): the 4 days left carry over, nothing restarts.
    w.tick += 6 * tpd;
    let b = place(&mut w, HUNTER, &hunter(), Vec2::new(20.5, 20.5));
    let lb = w.organisms[b].lineage;
    assert_eq!(w.lineage_grace_ticks(lb), Some(4 * tpd));
    assert_eq!(w.lineage_grace_ticks(lb), w.lineage_grace_ticks(la));
    // Ending it: no grace, and none back by re-founding within the cooldown.
    assert_eq!(w.end_grace(PREY), Err(CommandError::NoGrace));
    w.end_grace(HUNTER).unwrap();
    assert_eq!(w.lineage_grace_ticks(lb), None);
    assert_eq!(w.end_grace(HUNTER), Err(CommandError::NoGrace));
    w.tick += tpd;
    let c = place(&mut w, HUNTER, &hunter(), Vec2::new(30.5, 30.5));
    assert_eq!(w.lineage_grace_ticks(w.organisms[c].lineage), None);
    let prey = place(&mut w, PREY, &walker(), Vec2::new(31.5, 30.5));
    w.organisms[prey].owner = PREY;
    // The waiver is the host's to keep: restored, it holds.
    let waived = w.grace_waived().clone();
    w.set_grace_waived(Default::default());
    assert!(w.lineage_grace_ticks(w.organisms[c].lineage).is_some());
    w.set_grace_waived(waived);
    assert_eq!(w.lineage_grace_ticks(w.organisms[c].lineage), None);
    // Long after (past twice the whole length), a founding starts afresh.
    w.tick += 25 * tpd;
    let d = place(&mut w, HUNTER, &hunter(), Vec2::new(40.5, 40.5));
    assert_eq!(w.lineage_grace_ticks(w.organisms[d].lineage), Some(10 * tpd));
}

#[test]
fn a_possessed_hunter_pursues_and_kills_its_target() {
    let mut w = world();
    w.config.balance.escape = 0.0;
    w.config.balance.attack_power = 50.0;
    w.config.balance.fight_damage = 0.0;
    let a = place(&mut w, HUNTER, &hunter(), Vec2::new(10.5, 10.5));
    grown(&mut w, a);
    let prey = place(&mut w, PREY, &walker(), Vec2::new(16.5, 10.5));
    grown(&mut w, prey);
    let (aid, pid) = (w.organisms[a].id, w.organisms[prey].id);
    w.possess(HUNTER, aid).unwrap();
    w.possess_attack(HUNTER, aid, pid).unwrap();
    let mut ticks = 0;
    while w.organisms[prey].alive && ticks < 200 {
        // The order isn't a free pass to the prey: it is chased on foot.
        w.step();
        ticks += 1;
        let i = w.index[&aid];
        if w.organisms[w.index[&pid]].alive {
            assert_eq!(w.organisms[i].action, Action::Hunt, "tick {ticks}");
        }
    }
    let p = &w.organisms[w.index[&pid]];
    assert!(!p.alive && p.death == Some(DeathCause::Predation), "{:?} after {ticks} ticks", p.death);
    // The order ends with its target.
    w.step();
    assert_eq!(w.attack_target(aid), None);
}

#[test]
fn pack_mates_join_a_possessed_leaders_attack() {
    let mut w = world();
    let pk = pack(&mut w, HUNTER, &hunter(), 3, Vec2::new(10.5, 10.5));
    let prey = place(&mut w, PREY, &walker(), Vec2::new(14.5, 10.5));
    grown(&mut w, prey);
    for &i in &pk {
        w.organisms[i].needs.hunger = 50.0;
    }
    let leader = w.organisms[pk[0]].id;
    w.possess(HUNTER, leader).unwrap();
    w.possess_attack(HUNTER, leader, w.organisms[prey].id).unwrap();
    w.grid.rebuild(&w.organisms);
    for &i in &pk {
        let it = w.decide(i);
        assert_eq!((it.action, it.target_org), (Action::Hunt, Some(prey)), "{i}: {it:?}");
    }
    // A fed one rests instead.
    w.organisms[pk[2]].needs.hunger = 10.0;
    assert_ne!(w.decide(pk[2]).target_org, Some(prey));
    // Not with the rule off: the others hunt on their own account.
    w.config.balance.pack_hunt = 0.0;
    assert_ne!(w.decide(pk[1]).target_org, Some(prey));
    assert_eq!(w.decide(pk[0]).target_org, Some(prey));
}

#[test]
fn a_pack_takes_larger_prey_and_strikes_harder_together() {
    let mut w = world();
    let pk = pack(&mut w, HUNTER, &hunter(), 4, Vec2::new(10.5, 10.5));
    let b = w.config.balance;
    let o = &mut w.organisms[pk[0]];
    o.kin_nearby = 0;
    assert_eq!(super::decide::pack_scale(&b, o), 1.0);
    o.kin_nearby = 3;
    let scaled = super::decide::pack_scale(&b, o);
    let want = 1.0 + b.pack_hunt * super::decide::PACK_PREY * 3.0;
    assert!(scaled > 1.0 && (scaled - want).abs() < 1e-4, "{scaled}");
    let mut off = b;
    off.pack_hunt = 0.0;
    assert_eq!(super::decide::pack_scale(&off, o), 1.0);
    // A loner gets nothing from company.
    let lone = place(&mut w, 3, &walker().with(Gene::PackInstinct, 0.1), Vec2::new(35.5, 20.5));
    w.organisms[lone].kin_nearby = 4;
    assert_eq!(super::decide::pack_scale(&b, &w.organisms[lone]), 1.0);
    // Pack-mates hunting the same prey beside the striker count.
    let prey = place(&mut w, PREY, &walker(), Vec2::new(11.5, 11.5));
    let at = w.organisms[prey].pos;
    for &i in &pk[1..] {
        let o = &mut w.organisms[i];
        o.action = Action::Hunt;
        o.target = Some(at);
        o.pos = at + Vec2::new(1.0, 0.0);
    }
    w.organisms[pk[3]].target = Some(Vec2::new(30.0, 20.0));
    w.grid.rebuild(&w.organisms);
    assert_eq!(w.pack_allies(pk[0], prey), 2);
    w.config.balance.pack_hunt = 0.0;
    assert_eq!(w.pack_allies(pk[0], prey), 0);
}

#[test]
fn defenders_that_outnumber_a_larger_intruder_see_it_off() {
    let mut w = world();
    let center = Vec2::new(20.5, 12.5);
    let herd = pack(&mut w, PREY, &walker().with(Gene::Aggression, 0.8), 4, center);
    let intruder = place(&mut w, 3, &big(), center + Vec2::new(1.5, 1.0));
    grown(&mut w, intruder);
    for &i in &herd {
        w.organisms[i].kin_nearby = 3;
    }
    let (d, q) = (&w.organisms[herd[0]], &w.organisms[intruder]);
    assert!(q.power() > d.power(), "the intruder must be the stronger: {} vs {}", q.power(), d.power());
    assert!(4.0 * d.power() > q.power(), "four must outweigh it");
    let t = w.claim_territory(PREY, 0, center, 8.0).unwrap();
    assert!(t.radius > 3.5, "{t:?}");
    w.grid.rebuild(&w.organisms);
    let chase = w.decide(herd[0]);
    assert_eq!(chase.action, Action::Defend, "{chase:?}");
    // The mob scares the intruder more than one defender would.
    w.organisms[herd[0]].action = Action::Defend;
    w.grid.rebuild(&w.organisms);
    let mobbed = w.decide(intruder).threat;
    w.config.balance.territory_harass = 0.0;
    let alone = w.decide(intruder).threat;
    assert!(mobbed > alone + 0.1, "threat {alone} alone, {mobbed} mobbed");
    // With the rule off, the larger intruder is left alone.
    w.organisms[herd[0]].action = Action::Idle;
    w.grid.rebuild(&w.organisms);
    assert_ne!(w.decide(herd[0]).action, Action::Defend);
    // A lone defender doesn't take it on either way.
    w.config.balance.territory_harass = 1.0;
    for &i in &herd {
        w.organisms[i].kin_nearby = 0;
    }
    assert_ne!(w.decide(herd[0]).action, Action::Defend);
}
