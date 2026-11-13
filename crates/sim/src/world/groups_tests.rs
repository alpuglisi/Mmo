//! Group commands in the world: splitting and merging groups, per-group
//! orders, territories (`world::groups`).

use super::commands::CommandError;
use super::groups::*;
use super::World;
use crate::genome::PlayerId;
use crate::math::Vec2;
use crate::organism::{Action, OrgId, Organism};
use crate::{presets, WorldConfig};

/// Player 2's lineage is the sample herd grazers (40 founders).
const HERD: PlayerId = 2;

fn world() -> World {
    presets::standard_world(WorldConfig { seed: 7, width: 64, height: 64, ..Default::default() }, 1)
}

fn members(w: &World, player: PlayerId, group: u8) -> Vec<&Organism> {
    w.organisms.iter().filter(|o| o.alive && o.owner == player && o.group == group).collect()
}

fn centroid(orgs: &[&Organism]) -> Vec2 {
    orgs.iter().fold(Vec2::ZERO, |s, o| s + o.pos) * (1.0 / orgs.len().max(1) as f32)
}

type Snap = (OrgId, bool, u8, [u32; 6], Action);

/// Everything that steers the future, bit for bit.
fn state(w: &World) -> Vec<Snap> {
    w.organisms
        .iter()
        .map(|o| {
            let n = &o.needs;
            let bits = [o.pos.x, o.pos.y, n.hunger, n.thirst, n.energy, o.health].map(f32::to_bits);
            (o.id, o.alive, o.group, bits, o.action)
        })
        .collect()
}

#[test]
fn a_split_cuts_the_group_across_its_widest_spread() {
    let mut w = world();
    let n = members(&w, HERD, 0).len();
    assert!(n >= 8, "{n}");
    let out = w.split_group(HERD, 0, None).unwrap();
    assert_eq!(out, SplitOutcome { group: 1, moved: n / 2, kept: n - n / 2 });
    let (kept, moved) = (members(&w, HERD, 0), members(&w, HERD, 1));
    assert_eq!((kept.len(), moved.len()), (n - n / 2, n / 2));
    // The two sides of a line across the wider axis.
    let spread = |f: fn(&Organism) -> f32| {
        let v: Vec<f32> = kept.iter().chain(&moved).map(|o| f(o)).collect();
        let m = v.iter().sum::<f32>() / v.len() as f32;
        v.iter().map(|x| (x - m) * (x - m)).sum::<f32>()
    };
    let axis: fn(&Organism) -> f32 = if spread(|o| o.pos.y) > spread(|o| o.pos.x) { |o| o.pos.y } else { |o| o.pos.x };
    let kept_max = kept.iter().map(|o| axis(o)).fold(f32::MIN, f32::max);
    assert!(moved.iter().all(|o| axis(o) >= kept_max), "the new group is the far half");
    // Other players are untouched.
    assert!(w.organisms.iter().filter(|o| o.owner != HERD).all(|o| o.group == 0));
    let ids: Vec<u8> = w.groups(HERD).iter().map(|g| g.id).collect();
    assert_eq!(ids, vec![0, 1]);
    assert_eq!(w.groups(1).iter().map(|g| g.id).collect::<Vec<_>>(), vec![0]);

    // Up to MAX_GROUPS, then a merge frees an id for the next split.
    assert_eq!(w.split_group(HERD, 0, None).unwrap().group, 2);
    assert_eq!(w.split_group(HERD, 1, None).unwrap().group, 3);
    assert_eq!(w.split_group(HERD, 0, None), Err(CommandError::GroupLimit));
    let in_three = members(&w, HERD, 3).len();
    assert_eq!(w.merge_groups(HERD, 3, 0), Ok(in_three));
    assert!(members(&w, HERD, 3).is_empty());
    assert_eq!(w.split_group(HERD, 2, None).unwrap().group, 3);
    let total: usize = (0..MAX_GROUPS).map(|g| members(&w, HERD, g).len()).sum();
    assert_eq!(total, n);
}

#[test]
fn split_and_merge_rules_are_enforced() {
    let mut w = world();
    let herd: Vec<OrgId> = members(&w, HERD, 0).iter().map(|o| o.id).collect();
    let other = members(&w, 1, 0)[0].id;
    assert_eq!(w.split_group(HERD, 0, Some(&[herd[0], other])), Err(CommandError::NotOwned(other)));
    assert_eq!(w.split_group(HERD, 0, Some(&herd)), Err(CommandError::GroupTooSmall(0)));
    assert_eq!(w.split_group(HERD, 0, Some(&[])), Err(CommandError::GroupTooSmall(0)));
    assert_eq!(w.split_group(HERD, MAX_GROUPS, None), Err(CommandError::NoSuchGroup(MAX_GROUPS)));
    assert_eq!(w.split_group(HERD, 2, None), Err(CommandError::NoSuchGroup(2)));
    assert_eq!(w.split_group(HERD, 0, Some(&[999_999])), Err(CommandError::NotFound(999_999)));
    // Named organisms (a duplicate counts once) form the new group.
    let out = w.split_group(HERD, 0, Some(&[herd[3], herd[1], herd[3]])).unwrap();
    assert_eq!((out.group, out.moved), (1, 2));
    assert_eq!(w.organism(herd[1]).unwrap().group, 1);
    assert_eq!(w.split_group(HERD, 0, Some(&[herd[1]])), Err(CommandError::NotInGroup(herd[1])));
    assert_eq!(w.merge_groups(HERD, 1, 1), Err(CommandError::SameGroup(1)));
    assert_eq!(w.merge_groups(HERD, 2, 0), Err(CommandError::NoSuchGroup(2)));
    assert_eq!(w.merge_groups(HERD, 1, 0), Ok(2));
    assert!(w.organisms.iter().all(|o| o.group == 0));
    assert_eq!(w.groups(HERD).len(), 1);
}

#[test]
fn split_groups_follow_separate_orders_and_young_join_their_mothers_group() {
    let mut w = world();
    w.split_group(HERD, 0, None).unwrap();
    let start = (centroid(&members(&w, HERD, 0)), centroid(&members(&w, HERD, 1)));
    // Send the halves to opposite sides of the map.
    let (west, east) = (Vec2::new(6.0, start.0.y), Vec2::new(58.0, start.1.y));
    assert_eq!(w.command_migrate_group(HERD, 0, west), Ok(west));
    w.command_migrate_group(HERD, 1, east).unwrap();
    assert_eq!(w.migration_order(HERD, 1), Some(east));
    assert_eq!(w.command_migrate_group(HERD, MAX_GROUPS, east), Err(CommandError::NoSuchGroup(MAX_GROUPS)));
    let gap0 = start.0.dist(start.1);
    w.run_days(4);
    let (a, b) = (members(&w, HERD, 0), members(&w, HERD, 1));
    let gap = centroid(&a).dist(centroid(&b));
    assert!(gap > gap0 + 15.0, "groups {gap0:.1} -> {gap:.1} tiles apart");
    assert!(centroid(&a).dist(west) < start.0.dist(west) && centroid(&b).dist(east) < start.1.dist(east));
    // Young are born into their mother's group (if she is the same
    // player's).
    let mother = members(&w, HERD, 1)[0];
    let (genome, lineage, pos, id) = (mother.genome.clone(), mother.lineage, mother.pos, mother.id);
    let mut rng = crate::rng::rng_for(1, crate::rng::stream::APPLY, 0, 0);
    let calf = w.spawn_newborn(genome.clone(), HERD, lineage, pos, 1, id, id, 0.2, &mut rng);
    assert_eq!(w.organism(calf).unwrap().group, 1);
    let adopted = w.spawn_newborn(genome, 1, lineage, pos, 1, id, id, 0.2, &mut rng);
    assert_eq!(w.organism(adopted).unwrap().group, 0);
    // A whole-lineage order reaches every group; clearing clears all.
    w.command_migrate(HERD, west);
    assert_eq!((w.migration_order(HERD, 0), w.migration_order(HERD, 1)), (Some(west), Some(west)));
    w.clear_migration_group(HERD, 1);
    assert_eq!(w.migration_order(HERD, 1), None);
    w.clear_migration(HERD);
    assert_eq!(w.migration_order(HERD, 0), None);
}

#[test]
fn territory_claims_are_validated() {
    let mut w = world();
    let c = centroid(&members(&w, HERD, 0));
    let adults = members(&w, HERD, 0).iter().filter(|o| o.matured).count();
    let limit = World::territory_radius_limit(adults);
    assert!(limit > TERRITORY_MIN_RADIUS && limit <= TERRITORY_MAX_RADIUS, "{limit}");
    assert_eq!(World::territory_radius_limit(0), TERRITORY_BASE_RADIUS);
    assert_eq!(World::territory_radius_limit(100_000), TERRITORY_MAX_RADIUS);
    w.command_migrate(HERD, Vec2::new(1.0, 1.0));
    let t = w.claim_territory(HERD, 0, c, 1000.0).unwrap();
    assert_eq!((t.center, t.radius, t.claimed_tick), (c, limit, 0));
    // The claim replaced the migration order.
    assert_eq!(w.migration_order(HERD, 0), None);
    let g = w.groups(HERD)[0];
    assert_eq!((g.territory, g.max_radius), (Some(t), limit));
    let cooldown = (CLAIM_COOLDOWN_DAYS * w.config.ticks_per_day as f32) as u64;
    assert_eq!(g.claim_ready_tick, cooldown);
    assert_eq!(w.claim_territory(HERD, 0, c, 5.0), Err(CommandError::ClaimCooldown { ticks_left: cooldown }));
    assert_eq!(w.claim_territory(HERD, 1, c, 5.0), Err(CommandError::NoSuchGroup(1)));
    assert_eq!(w.claim_territory(HERD, 9, c, 5.0), Err(CommandError::NoSuchGroup(9)));
    for _ in 0..cooldown {
        w.step();
    }
    assert_eq!(w.groups(HERD)[0].claim_ready_tick, 0);
    let t = w.claim_territory(HERD, 0, Vec2::new(-5.0, 500.0), 0.5).unwrap();
    assert_eq!(t.radius, TERRITORY_MIN_RADIUS, "rounded up");
    assert_eq!(t.center, w.map.clamp(Vec2::new(-5.0, 500.0)), "clamped to the map");
    // A migration order replaces the territory; abandoning twice fails.
    w.command_migrate_group(HERD, 0, c).unwrap();
    assert_eq!(w.territory(HERD, 0), None);
    assert_eq!(w.abandon_territory(HERD, 0), Err(CommandError::NoTerritory(0)));
    assert!(w.abandon_territory(HERD, 9).is_err());
}

/// Share of the herd inside `t`, over the last of `days` days.
fn share_inside(w: &mut World, t: Territory, days: u32) -> f32 {
    let (mut inside, mut all) = (0usize, 0usize);
    let tpd = w.config.ticks_per_day;
    for k in 0..days * tpd {
        w.step();
        if k >= (days - 1) * tpd {
            for o in members(w, HERD, 0) {
                all += 1;
                inside += usize::from(t.contains(o.pos));
            }
        }
    }
    inside as f32 / all.max(1) as f32
}

#[test]
fn a_territory_keeps_the_group_inside_it() {
    let mut held = world();
    let c = centroid(&members(&held, HERD, 0));
    let t = held.claim_territory(HERD, 0, c, 7.0).unwrap();
    let mut free = world();
    let (a, b) = (share_inside(&mut held, t, 4), share_inside(&mut free, t, 4));
    assert!(a > 0.8 && a > b + 0.15, "inside the territory: held {a:.2}, free {b:.2}");
}

#[test]
fn members_outside_walk_back_and_defenders_chase_strangers_off() {
    let mut w = world();
    let herd: Vec<usize> =
        (0..w.organisms.len()).filter(|&i| w.organisms[i].owner == HERD && w.organisms[i].matured).collect();
    let vole = w.organisms.iter().position(|o| o.lineage == 0 && o.matured).unwrap();
    for o in w.organisms.iter_mut() {
        o.pos = Vec2::new(2.0, 2.0);
    }
    let center = Vec2::new(32.0, 32.0);
    for &i in &herd[..2] {
        let o = &mut w.organisms[i];
        (o.needs.hunger, o.needs.thirst, o.needs.energy, o.needs.breeding) = (5.0, 5.0, 95.0, 0.0);
    }
    let (d, out) = (herd[0], herd[1]);
    w.organisms[d].pos = center;
    w.organisms[out].pos = center + Vec2::new(14.0, 0.0);
    w.organisms[vole].pos = center + Vec2::new(2.0, 0.0);
    w.claim_territory(HERD, 0, center, 8.0).unwrap();
    w.grid.rebuild(&w.organisms);
    // Outside: back toward the territory.
    let back = w.decide(out);
    assert_eq!(back.action, Action::Patrol);
    let to = back.target.unwrap();
    assert!(to.dist(center) < 8.0 && to.x > center.x, "{to:?}");
    // Inside, with a stranger in sight: after it.
    let chase = w.decide(d);
    assert_eq!(chase.action, Action::Defend, "{chase:?}");
    assert_eq!(chase.target, Some(w.organisms[vole].pos));
    // The stranger sees a defender coming at it as a threat, while it is
    // inside the territory.
    let calm = w.decide(vole).threat;
    w.organisms[d].action = Action::Defend;
    w.grid.rebuild(&w.organisms);
    let scared = w.decide(vole);
    assert!(scared.threat > calm + 0.2, "threat {calm} -> {}", scared.threat);
    w.abandon_territory(HERD, 0).unwrap();
    assert_eq!(w.decide(vole).threat, calm);
    assert_ne!(w.decide(d).action, Action::Defend);
}

#[test]
fn group_commands_are_deterministic_and_unused_ones_change_nothing() {
    let run = |commands: bool, undo: bool| {
        let mut w = world();
        for day in 0..3u64 {
            if commands && day == 1 {
                let c = centroid(&members(&w, HERD, 0));
                let split = w.split_group(HERD, 0, None).unwrap();
                w.claim_territory(HERD, 0, c, 6.0).unwrap();
                w.command_migrate_group(HERD, split.group, Vec2::new(60.0, 60.0)).unwrap();
                if undo {
                    w.abandon_territory(HERD, 0).unwrap();
                    w.clear_migration_group(HERD, split.group);
                    w.merge_groups(HERD, split.group, 0).unwrap();
                }
            }
            // Between sub-steps, as on a server.
            for _ in 0..w.config.ticks_per_day {
                while w.substeps_left() > 0 {
                    w.substep();
                }
                w.step();
            }
        }
        w
    };
    let (a, b) = (run(true, false), run(true, false));
    assert_eq!(state(&a), state(&b));
    assert_eq!(format!("{:?}", a.counters), format!("{:?}", b.counters));
    assert!(a.organisms.iter().any(|o| o.group == 1 && o.alive));
    // Given and taken back at once, or never given: the same world.
    let (c, d) = (run(true, true), run(false, false));
    assert_eq!(state(&c), state(&d));
    assert_ne!(state(&a), state(&d));
}
