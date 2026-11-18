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
    // Past sight (8) but within the range a standing order is kept (12):
    // the player's view lags the world (playtest 4).
    let lagged = place(&mut w, PREY, &walker(), Vec2::new(20.5, 10.5));
    grown(&mut w, lagged);
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
    assert_eq!(w.possess_attack(HUNTER, aid, id(&w, lagged)), Ok(()));
    w.attack_orders.clear();
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
    assert_eq!(w.pack_allies(pk[0], prey, false), 2);
    w.config.balance.pack_hunt = 0.0;
    assert_eq!(w.pack_allies(pk[0], prey, false), 0);
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

#[test]
fn an_attack_order_outside_the_territory_is_followed() {
    let mut w = world();
    let pk = pack(&mut w, HUNTER, &hunter(), 3, Vec2::new(8.5, 8.5));
    let prey = place(&mut w, PREY, &walker(), Vec2::new(30.5, 12.5));
    grown(&mut w, prey);
    // The group holds a territory around the pack; the attacker is outside it.
    w.claim_territory(HUNTER, 0, Vec2::new(8.5, 8.5), 6.0).unwrap();
    let a = pk[0];
    w.organisms[a].pos = Vec2::new(26.5, 12.5);
    let (aid, pid) = (w.organisms[a].id, w.organisms[prey].id);
    w.possess(HUNTER, aid).unwrap();
    w.possess_attack(HUNTER, aid, pid).unwrap();
    for t in 0..20 {
        w.step();
        if !w.organisms[w.index[&pid]].alive || w.attack_target(aid).is_none() {
            break;
        }
        let o = &w.organisms[w.index[&aid]];
        assert_eq!(o.action, Action::Hunt, "tick {t}: {:?} at {:?}", o.action, o.pos);
    }
}

/// A drab toxic prey (a walker with strong toxins).
fn toxic(color: f32) -> FounderDesign {
    walker().with(Gene::Toxins, 0.9).with(Gene::Coloration, color)
}

#[test]
fn poisoned_lineages_learn_bright_prey_faster_and_forget_slowly() {
    let mut w = world();
    // The version 44 rates (`toxic_avoidance` doubles them).
    w.config.balance.toxic_avoidance = 0.0;
    w.learn_toxic(1, 2, 0.0);
    let drab = w.toxic_lesson(1, 2);
    w.learn_toxic(1, 3, 1.0);
    let bright = w.toxic_lesson(1, 3);
    assert!((drab - super::toxic::LESSON_STEP).abs() < 1e-6, "{drab}");
    assert!((bright - 3.0 * drab).abs() < 1e-5, "bright teaches 3×: {bright} vs {drab}");
    for _ in 0..20 {
        w.learn_toxic(1, 2, 0.0);
    }
    assert_eq!(w.toxic_lesson(1, 2), 1.0, "capped at 1");
    // A month later it is still mostly remembered; faint lessons go.
    for _ in 0..30 {
        w.fade_toxic_lessons();
    }
    let kept = w.toxic_lesson(1, 2);
    assert!(kept > 0.55 && kept < 0.65, "{kept}");
    for _ in 0..300 {
        w.fade_toxic_lessons();
    }
    assert!(w.toxic_lessons.is_empty());
    // Off: nothing learned.
    w.config.balance.toxic_learning = 0.0;
    w.learn_toxic(1, 2, 1.0);
    assert_eq!(w.toxic_lesson(1, 2), 0.0);
}

/// `Balance::toxic_avoidance`: a learned lesson is acted on at 70% for a
/// drab coat and near 90% for a bright one, two drab bites teach most of
/// it, and the pass-up holds for a hunter and prey lineage all day instead
/// of being rolled afresh at every decision.
#[test]
fn strong_avoidance_holds_for_a_whole_day() {
    let mut w = world();
    assert!(w.config.balance.toxic_avoidance > 0.0);
    w.learn_toxic(1, 2, 0.0);
    assert!((w.toxic_lesson(1, 2) - 2.0 * super::toxic::LESSON_STEP).abs() < 1e-6);
    let pk = pack(&mut w, HUNTER, &hunter(), 2, Vec2::new(10.5, 10.5));
    let preys = pack(&mut w, PREY, &toxic(0.0), 4, Vec2::new(11.5, 10.5));
    let (hl, pl) = (w.organisms[pk[0]].lineage, w.organisms[preys[0]].lineage);
    w.toxic_lessons.insert((hl, pl), 1.0);
    assert!((w.toxic_avoidance(hl, &w.organisms[preys[0]]) - 0.7).abs() < 0.02);
    for &j in &preys {
        w.organisms[j].pheno.0[Gene::Coloration.index()] = 0.6;
    }
    // Capped (`Balance::avoid_cap`): an honest signal is not immunity.
    assert!((w.toxic_avoidance(hl, &w.organisms[preys[0]]) - w.config.balance.avoid_cap).abs() < 1e-6);
    w.config.balance.avoid_cap = 0.0;
    assert!(w.toxic_avoidance(hl, &w.organisms[preys[0]]) > 0.87);
    for &i in &pk {
        w.organisms[i].needs.hunger = 80.0;
    }
    w.grid.rebuild(&w.organisms);
    // The same hunter decides the same way all day about this prey.
    let first = w.decide(pk[1]).target_org.is_some_and(|j| preys.contains(&j));
    for _ in 0..20 {
        let again = w.decide(pk[1]).target_org.is_some_and(|j| preys.contains(&j));
        assert_eq!(again, first);
    }
}

#[test]
fn a_hunter_poisoned_by_drab_prey_teaches_its_lineage_to_pass_it_up() {
    let mut w = world();
    // The version 44 recognition (`strong_avoidance_holds_for_a_whole_day`
    // covers `toxic_avoidance`).
    w.config.balance.toxic_avoidance = 0.0;
    w.config.balance.escape = 0.0;
    w.config.balance.fight_damage = 0.0;
    w.config.balance.toxin_damage = 1.0;
    let pk = pack(&mut w, HUNTER, &hunter(), 2, Vec2::new(10.5, 10.5));
    let preys = pack(&mut w, PREY, &toxic(0.1), 2, Vec2::new(11.5, 10.5));
    let (prey, spare) = (preys[0], preys[1]);
    let (hl, pl) = (w.organisms[pk[0]].lineage, w.organisms[prey].lineage);
    for &i in &pk {
        w.organisms[i].needs.hunger = 80.0;
    }
    w.grid.rebuild(&w.organisms);
    // Naive: the drab toxic prey is hunted, and the strike poisons.
    let it = w.decide(pk[1]);
    assert!(it.action == Action::Hunt && it.target_org.is_some_and(|j| preys.contains(&j)), "{it:?}");
    let mut rng = crate::rng::rng_for(1, crate::rng::stream::APPLY, 0, 0);
    let strike = super::Intent {
        action: Action::Hunt,
        target: Some(w.organisms[prey].pos),
        target_org: Some(prey),
        ..super::Intent::idle()
    };
    w.apply(pk[0], &strike, &mut rng);
    let lesson = w.toxic_lesson(hl, pl);
    assert!(lesson > 0.1 && lesson < 0.25, "one drab poisoning: {lesson}");
    // Taught well, the lineage passes it up as far as it can tell it apart:
    // a drab coat at little more than a third, a bright one almost always.
    w.toxic_lessons.insert((hl, pl), 1.0);
    let drab = w.toxic_avoidance(hl, &w.organisms[spare]);
    assert!((drab - (0.3 + 0.7 * 0.1)).abs() < 0.02, "{drab}");
    for &j in &preys {
        w.organisms[j].pheno.0[Gene::Coloration.index()] = 1.0;
    }
    assert!((w.toxic_avoidance(hl, &w.organisms[spare]) - w.config.balance.avoid_cap).abs() < 1e-6);
    w.config.balance.avoid_cap = 0.0;
    assert!((w.toxic_avoidance(hl, &w.organisms[spare]) - 1.0).abs() < 1e-6);
    let it = w.decide(pk[1]);
    assert!(!it.target_org.is_some_and(|j| preys.contains(&j)), "{it:?}");
    // It knows the toxin, so a bite harms it less.
    assert!((w.toxin_harm(hl, pl) - 0.3).abs() < 1e-6);
    // The player is told, but may still order the attack.
    let (a, t) = (w.organisms[pk[1]].id, w.organisms[spare].id);
    assert!(w.attack_warning(a, t).is_some_and(|s| s.contains("poisonous")));
    assert!(w.looks_poisonous_to(HUNTER, pl) && !w.looks_poisonous_to(PREY, pl));
    w.possess(HUNTER, a).unwrap();
    assert_eq!(w.possess_attack(HUNTER, a, t), Ok(()));
}

#[test]
fn attack_orders_report_how_they_end() {
    use super::commands::AttackEnd;
    let mut w = world();
    w.config.balance.escape = 0.0;
    w.config.balance.attack_power = 50.0;
    w.config.balance.fight_damage = 0.0;
    let a = place(&mut w, HUNTER, &hunter(), Vec2::new(10.5, 10.5));
    grown(&mut w, a);
    let prey = place(&mut w, PREY, &walker(), Vec2::new(12.5, 10.5));
    grown(&mut w, prey);
    let other = place(&mut w, PREY, &walker(), Vec2::new(13.5, 12.5));
    let (aid, pid, oid) = (w.organisms[a].id, w.organisms[prey].id, w.organisms[other].id);
    w.possess(HUNTER, aid).unwrap();
    w.possess_attack(HUNTER, aid, pid).unwrap();
    for _ in 0..200 {
        w.step();
        if w.attack_target(aid).is_none() {
            break;
        }
    }
    let ev = w.take_attack_events();
    assert_eq!(ev.len(), 1, "{ev:?}");
    assert_eq!((ev[0].player, ev[0].attacker, ev[0].target, ev[0].end), (HUNTER, aid, pid, AttackEnd::Killed));
    // Gone to ground: the order ends, and says why.
    if w.organisms[w.index[&oid]].alive && w.organisms[w.index[&aid]].alive {
        w.possess(HUNTER, aid).unwrap();
        let j = w.index[&oid];
        w.organisms[j].pos = w.organisms[w.index[&aid]].pos + Vec2::new(3.0, 0.0);
        w.possess_attack(HUNTER, aid, oid).unwrap();
        w.organisms[j].sheltered = true;
        w.settle_orders();
        assert_eq!(w.take_attack_events().iter().map(|e| e.end).collect::<Vec<_>>(), vec![AttackEnd::WentToGround]);
        // A release is the player's doing: no event.
        w.organisms[j].sheltered = false;
        w.possess_attack(HUNTER, aid, oid).unwrap();
        w.release(HUNTER, aid).unwrap();
        w.settle_orders();
        assert!(w.take_attack_events().is_empty());
    }
}

#[test]
fn the_parched_drink_first_and_possessed_animals_break_off_for_water() {
    use super::path_tests::set;
    let mut w = world();
    for y in 0..super::path_tests::H {
        set(&mut w, 30, y, "lake");
    }
    let i = place(&mut w, PREY, &walker(), Vec2::new(26.5, 10.5));
    grown(&mut w, i);
    // Playtest 3: starving and parched, 4 tiles from a lake, it grazed on
    // until it died of thirst. With the rule it drinks first.
    let o = &mut w.organisms[i];
    (o.needs.thirst, o.needs.hunger, o.fat) = (95.0, 100.0, 0.0);
    w.grid.rebuild(&w.organisms);
    w.config.balance.drink_priority = 1.0;
    assert_eq!(w.decide(i).action, Action::Drink);
    // A possessed animal under a move order, thirst 65, water 4 tiles off.
    let id = w.organisms[i].id;
    w.organisms[i].needs = crate::organism::Needs { thirst: 65.0, hunger: 10.0, energy: 90.0, ..w.organisms[i].needs };
    w.possess(PREY, id).unwrap();
    w.order_move(PREY, id, Vec2::new(5.5, 10.5)).unwrap();
    assert_eq!(w.decide(i).action, Action::Drink);
    w.config.balance.possess_drink = 0.0;
    assert_eq!(w.decide(i).action, Action::Ordered, "the old rule: on until 85");
}

/// Playtest 3: a starving hunter that makes a kill eats its fill from the
/// carcass, rather than starving days later beside it.
#[test]
fn a_kill_feeds_a_starving_hunter() {
    let mut w = world();
    w.config.balance.escape = 0.0;
    w.config.balance.attack_power = 50.0;
    w.config.balance.fight_damage = 0.0;
    let a = place(&mut w, HUNTER, &hunter(), Vec2::new(10.5, 10.5));
    grown(&mut w, a);
    let prey = place(&mut w, PREY, &walker().with(Gene::BodySize, 0.45), Vec2::new(11.5, 10.5));
    grown(&mut w, prey);
    let (aid, pid) = (w.organisms[a].id, w.organisms[prey].id);
    let o = &mut w.organisms[a];
    (o.needs.hunger, o.fat) = (95.0, 0.0);
    w.possess(HUNTER, aid).unwrap();
    w.possess_attack(HUNTER, aid, pid).unwrap();
    let mut least = 100.0f32;
    let mut caught = false;
    for _ in 0..w.config.ticks_per_day * 2 {
        w.step();
        caught |= w.organism(pid).is_none();
        let o = w.organism(aid).expect("the hunter lives");
        least = least.min(o.needs.hunger);
    }
    assert!(caught, "the prey should be caught");
    assert!(least < 30.0, "the kill should feed it: hunger never fell below {least}");
}

/// Playtest 4: player carnivore bands starved on their own. A hungry
/// player's pack hunter is carried by its band's kills
/// (`Balance::player_packs`); wild packs, loners and fed ones aren't.
#[test]
fn player_packs_share_their_kills() {
    let mut w = world();
    let mine = place(&mut w, HUNTER, &hunter(), Vec2::new(10.5, 10.5));
    let wild = place(&mut w, crate::WILD, &hunter(), Vec2::new(20.5, 10.5));
    let loner = place(&mut w, HUNTER, &hunter().with(Gene::PackInstinct, 0.2), Vec2::new(30.5, 10.5));
    let b = w.config.balance;
    for &i in &[mine, wild, loner] {
        w.organisms[i].needs.hunger = 60.0;
    }
    let larder = |w: &World, i: usize| super::apply::player_pack_larder(&b, &w.organisms[i], 1.0);
    assert!(super::decide::player_pack(&b, &w.organisms[mine]));
    assert!(larder(&w, mine) > 0.15, "{}", larder(&w, mine));
    assert_eq!((larder(&w, wild), larder(&w, loner)), (0.0, 0.0));
    assert!(!super::decide::player_pack(&b, &w.organisms[wild]));
    // In lean times only: a fed pack hunter is spared nothing.
    w.organisms[mine].needs.hunger = 10.0;
    assert_eq!(larder(&w, mine), 0.0);
    w.organisms[mine].needs.hunger = 60.0;
    let mut off = b;
    off.player_packs = 0.0;
    assert!(!super::decide::player_pack(&off, &w.organisms[mine]));
    assert_eq!(super::apply::player_pack_larder(&off, &w.organisms[mine], 1.0), 0.0);
    // A big population competes for its kills rather than sharing them.
    assert_eq!(super::apply::player_pack_larder(&b, &w.organisms[mine], 0.0), 0.0);
    assert!(super::apply::player_pack_larder(&b, &w.organisms[mine], 0.5) < larder(&w, mine));
}

/// Playtest 6: a hunter killed by prey fighting back was "eaten by" the
/// plant eaters' lineage. It dies fighting back now (`FoughtBack`), the prey
/// remembered but not credited with a kill; killed by another hunter, it is
/// still predation.
#[test]
fn a_hunter_killed_by_its_prey_is_not_eaten() {
    let mut w = world();
    let a = place(&mut w, HUNTER, &hunter(), Vec2::new(10.5, 10.5));
    let prey = place(&mut w, PREY, &walker(), Vec2::new(11.0, 10.5));
    let rival = place(&mut w, PREY, &hunter(), Vec2::new(12.0, 10.5));
    let b = place(&mut w, HUNTER, &hunter(), Vec2::new(12.5, 10.5));
    let kills = w.counters.kills_by_lineage.clone();
    w.hunter_killed_by(a, prey);
    let o = &w.organisms[a];
    assert_eq!((o.alive, o.death, o.killer), (false, Some(DeathCause::FoughtBack), Some(w.organisms[prey].lineage)));
    assert_eq!(w.counters.kills_by_lineage, kills, "no kill credited to the prey");
    w.hunter_killed_by(b, rival);
    assert_eq!(w.organisms[b].death, Some(DeathCause::Predation));
}

/// Playtest 6: the prey count behind the territory and rally warnings
/// counted burrowed voles, poisonous herds and other predators, and warned
/// only under half a prey animal per hunter. Only prey the pack can take
/// counts now, and the warning compares what it feeds with the pack.
#[test]
fn the_prey_check_counts_only_prey_the_pack_can_take() {
    let mut w = world();
    let pack: Vec<usize> = (0..3).map(|k| place(&mut w, HUNTER, &hunter(), Vec2::new(10.5 + k as f32, 10.5))).collect();
    for &i in &pack {
        grown(&mut w, i);
    }
    let prey: Vec<usize> = (0..6).map(|k| place(&mut w, PREY, &walker(), Vec2::new(12.5, 8.5 + k as f32))).collect();
    let rival = place(&mut w, PREY, &hunter(), Vec2::new(13.5, 10.5));
    grown(&mut w, rival);
    let c = Vec2::new(11.5, 10.5);
    let all = w.prey_check(HUNTER, 0, c, 4.0).expect("a meat eaters' group");
    assert_eq!((all.prey, all.members), (6, 3), "{all:?}");
    // Down a burrow: out of reach.
    w.organisms[prey[0]].sheltered = true;
    w.organisms[prey[1]].sheltered = true;
    let fewer = w.prey_check(HUNTER, 0, c, 4.0).unwrap();
    assert_eq!(fewer.prey, 4);
    assert!(fewer.feeds <= all.feeds);
    // Six small walkers can't carry three hunters for good.
    assert!(all.feeds < all.members, "{all:?}");
}
