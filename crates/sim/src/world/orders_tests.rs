//! Group and possession orders meeting the terrain and their own end:
//! migration targets on water, arrival, breaking off for needs, keeping off
//! thin ice, emptied groups, unreachable move orders, rallies off the map,
//! and retiring a lineage.

use super::commands::{CommandError, GroupEvent, MIGRATE_SNAP_RADIUS};
use super::path_tests::{flat_world, place, set, walker, H};
use super::World;
use crate::genome::PlayerId;
use crate::math::Vec2;
use crate::organism::{Action, DeathCause};
use crate::terrain::Map;
use crate::{presets, WorldConfig};

/// Player 2's lineage is the sample herd grazers (40 founders).
const HERD: PlayerId = 2;

fn standard() -> World {
    presets::standard_world(WorldConfig { seed: 7, width: 64, height: 64, ..Default::default() }, 1)
}

/// A flat world with a lake over columns `x0..=x1` (every row).
fn lake_world(x0: usize, x1: usize) -> World {
    let mut w = flat_world(1500.0);
    for x in x0..=x1 {
        for y in 0..H {
            set(&mut w, x, y, "lake");
        }
    }
    w
}

fn is_lake(w: &World, p: Vec2) -> bool {
    w.map.tile_at(p).water_depth > 1.0
}

#[test]
fn migration_targets_on_water_move_to_the_shore_or_are_refused() {
    let mut w = lake_world(10, 39);
    place(&mut w, 1, &walker(), Map::tile_center(3, 10));
    // Just off the shore: the nearest land tile.
    let got = w.command_migrate(1, Map::tile_center(12, 10)).unwrap();
    assert!(!is_lake(&w, got) && got.x < 10.0, "{got:?}");
    assert_eq!(w.migration_order(1, 0), Some(got));
    // Far out (beyond MIGRATE_SNAP_RADIUS): refused, and the order stands.
    let far = Map::tile_center(10 + MIGRATE_SNAP_RADIUS as usize + 4, 10);
    assert_eq!(w.command_migrate(1, far), Err(CommandError::Unreachable));
    assert_eq!(w.migration_order(1, 0), Some(got));
    // Off the map: refused, not clamped.
    assert_eq!(w.command_migrate(1, Vec2::new(99_999.0, -5.0)), Err(CommandError::OffMap));
    assert_eq!(w.command_migrate_group(1, 0, Vec2::new(-1.0, 3.0)), Err(CommandError::OffMap));
    // A group with no members is refused (the main group always exists).
    assert_eq!(w.command_migrate_group(1, 1, Vec2::new(4.0, 3.0)), Err(CommandError::NoSuchGroup(1)));
    // Land is taken as given.
    let land = Vec2::new(4.2, 7.9);
    assert_eq!(w.command_migrate(1, land), Ok(land));
    // A frozen lake counts as water: the ice goes in spring.
    for y in 0..H {
        set(&mut w, 11, y, "ice");
    }
    let got = w.command_migrate(1, Map::tile_center(11, 5)).unwrap();
    assert!(got.x < 10.0, "a target on the ice moved ashore: {got:?}");
}

#[test]
fn a_migrating_walker_goes_around_thin_ice_instead_of_over_it() {
    // A frozen lake with open land below it; the target is straight across.
    let mut w = flat_world(1500.0);
    for x in 15..=25 {
        for y in 0..=16 {
            set(&mut w, x, y, "lake");
            set(&mut w, x, y, "ice");
        }
    }
    let i = place(&mut w, 1, &walker(), Map::tile_center(5, 5));
    let target = Map::tile_center(34, 5);
    // The route a client draws goes the same way.
    assert_eq!(w.command_migrate(1, target), Ok(target));
    let route = w.migration_route(1, 0);
    assert!(route.len() <= super::commands::ROUTE_POINTS, "{}", route.len());
    assert_eq!((route.first(), route.last()), (Some(&w.organisms[i].pos), Some(&target)));
    assert!(route.iter().all(|&p| w.map.tile_at(p).ice == 0.0), "{route:?}");
    assert!(route.iter().any(|p| p.y > 16.0), "around the lake's open end: {route:?}");
    let reckless = {
        let mut w = w.clone();
        let mut crossed = false;
        for _ in 0..200 {
            w.move_toward_with(i, target, false, 1.0, false);
            w.organisms[i].needs.energy = 100.0;
            crossed |= w.map.tile_at(w.organisms[i].pos).ice > 0.0;
        }
        crossed
    };
    assert!(reckless, "without care the walker crosses the ice");
    let mut arrived = false;
    for _ in 0..600 {
        w.move_toward_with(i, target, false, 1.0, true);
        w.organisms[i].needs.energy = 100.0;
        let p = w.organisms[i].pos;
        assert_eq!(w.map.tile_at(p).ice, 0.0, "stepped onto the ice at {p:?}");
        if p.dist(target) < 0.5 {
            arrived = true;
            break;
        }
    }
    assert!(arrived, "walker stuck at {:?}", w.organisms[i].pos);
}

#[test]
fn an_ordered_migrant_breaks_off_to_drink_and_then_goes_back() {
    let mut w = standard();
    let target = w.command_migrate(HERD, Vec2::new(60.0, 60.0)).unwrap();
    let id = w.organisms.iter().find(|o| o.alive && o.owner == HERD && o.matured).unwrap().id;
    let set_needs = |w: &mut World, thirst: f32, action: Action| {
        let i = w.index[&id];
        let o = &mut w.organisms[i];
        (o.needs.thirst, o.needs.hunger, o.needs.energy, o.needs.breeding) = (thirst, 0.0, 100.0, 0.0);
        o.action = action;
    };
    let action = |w: &World| w.organism(id).unwrap().action;
    // Parched: it drinks, order or not.
    set_needs(&mut w, 90.0, Action::Migrate);
    w.step();
    assert_eq!(action(&w), Action::Drink);
    // Half drunk: it doesn't turn back to the order yet.
    set_needs(&mut w, 45.0, Action::Drink);
    w.step();
    assert_ne!(action(&w), Action::Migrate);
    // Done: back on the way.
    set_needs(&mut w, 5.0, Action::Drink);
    w.step();
    assert_eq!(action(&w), Action::Migrate);
    assert_eq!(w.organism(id).unwrap().target, Some(target));
}

#[test]
fn a_group_emptied_by_deaths_loses_its_orders() {
    let mut w = standard();
    let split = w.split_group(HERD, 0, None).unwrap();
    let g = split.group;
    let c = w.organisms.iter().find(|o| o.alive && o.owner == HERD && o.group == g).unwrap().pos;
    w.claim_territory(HERD, g, c, 5.0).unwrap();
    w.command_migrate_group(HERD, 0, Vec2::new(60.0, 60.0)).unwrap();
    let doomed: Vec<usize> = (0..w.organisms.len())
        .filter(|&k| w.organisms[k].alive && w.organisms[k].owner == HERD && w.organisms[k].group == g)
        .collect();
    for k in doomed {
        w.kill(k, DeathCause::Predation);
    }
    for _ in 0..w.config.ticks_per_hour() {
        w.step();
    }
    assert_eq!(w.territory(HERD, g), None, "the territory is released");
    assert!(w.groups(HERD).iter().all(|x| x.id != g));
    let events = w.take_group_events();
    assert!(events.contains(&GroupEvent::Emptied { player: HERD, group: g }), "{events:?}");
    // The main group, still alive, keeps its order.
    assert!(w.migration_order(HERD, 0).is_some());
}

#[test]
fn a_move_order_the_organism_cannot_reach_ends_where_it_can() {
    // Into the lake: the order stops at the shore, and is done there.
    let mut w = lake_world(20, 39);
    let i = place(&mut w, 1, &walker(), Map::tile_center(10, 10));
    let id = w.organisms[i].id;
    assert_eq!(w.order_move(1, id, Map::tile_center(30, 10)), Err(CommandError::NotPossessed(id)));
    assert_eq!(w.order_move(2, id, Map::tile_center(30, 10)), Err(CommandError::NotOwned(id)));
    assert_eq!(w.release(1, id), Err(CommandError::NotPossessed(id)));
    w.possess(1, id).unwrap();
    let got = w.order_move(1, id, Map::tile_center(30, 10)).unwrap();
    assert!(!is_lake(&w, got) && got.x < 20.0 && got.x > 17.0, "{got:?}");
    for _ in 0..200 {
        w.organisms[i].needs.energy = 100.0;
        w.organisms[i].needs.thirst = 0.0;
        w.organisms[i].needs.hunger = 0.0;
        w.step();
        if w.organisms[i].order.is_none() {
            break;
        }
    }
    assert_eq!(w.organisms[i].order, None, "never arrived; at {:?}", w.organisms[i].pos);

    // Onto an island: the nearest reachable tile.
    let mut w = lake_world(15, 25);
    for x in 19..=21 {
        for y in 9..=11 {
            let k = w.map.idx(x, y);
            w.map.edit_terrain().ground[k].biome = crate::terrain::Biome::Grassland;
            w.map.tiles[k].water_depth = 0.0;
        }
    }
    let i = place(&mut w, 1, &walker(), Map::tile_center(5, 10));
    let id = w.organisms[i].id;
    w.possess(1, id).unwrap();
    let got = w.order_move(1, id, Map::tile_center(20, 10)).unwrap();
    assert!(got.x < 15.0 && got.x > 13.0 && (got.y - 10.5).abs() < 1.5, "{got:?}");
}

#[test]
fn rallies_refuse_off_map_targets_and_last_the_hosts_floor() {
    let mut w = standard();
    assert_eq!(w.rally_group(HERD, 0, Vec2::new(99_999.0, -5.0)), Err(CommandError::OffMap));
    let r = w.rally_group(HERD, 0, Vec2::new(20.0, 20.0)).unwrap();
    let days = r.expires_tick - r.issued_tick;
    assert_eq!(days, w.rally_ticks());
    w.set_rally_min_ticks(days * 10);
    let r = w.rally_group(HERD, 0, Vec2::new(20.0, 20.0)).unwrap();
    assert_eq!(r.expires_tick - r.issued_tick, days * 10);
}

#[test]
fn retiring_a_lineage_removes_it_without_counting_deaths() {
    let mut w = standard();
    w.run_days(1);
    let lineages = w.lineages.len();
    let deaths: u64 = w.counters.deaths.values().sum();
    let others = w.alive_count() - w.owner_count(HERD);
    w.command_migrate(HERD, Vec2::new(60.0, 60.0)).unwrap();
    let id = w.organisms.iter().find(|o| o.alive && o.owner == HERD).unwrap().id;
    w.possess(HERD, id).unwrap();
    let n = w.retire_player(HERD);
    assert!(n >= 30, "{n}");
    assert_eq!(w.owner_count(HERD), 0);
    assert_eq!(w.alive_count(), others);
    assert!(w.organisms.iter().all(|o| !o.alive || o.owner != HERD));
    assert!(w.organisms.iter().all(|o| o.possessed_by != Some(HERD)));
    assert!(w.clutches.iter().all(|c| c.owner != HERD));
    assert_eq!(w.counters.deaths.values().sum::<u64>(), deaths, "not counted as deaths");
    assert_eq!(w.migration_order(HERD, 0), None);
    assert_eq!(w.lineages.len(), lineages, "the lineage stays, extinct");
    // The world steps on, and the player can found again.
    w.step();
    let again = w.found_lineage_unchecked(HERD, &walker());
    assert_eq!(w.owner_count(HERD), again.len());
}

/// Player `p`'s members of group 0, by index.
fn herd(w: &World, p: PlayerId) -> Vec<usize> {
    (0..w.organisms.len())
        .filter(|&k| w.organisms[k].alive && w.organisms[k].owner == p && w.organisms[k].group == 0)
        .collect()
}

/// Run the hourly group check now.
fn settle_now(w: &mut World) {
    let tph = w.config.ticks_per_hour();
    w.tick = w.tick.div_ceil(tph) * tph;
    w.settle_groups();
}

#[test]
fn a_migration_arrives_when_half_the_group_is_there_not_the_first_few() {
    let mut w = standard();
    let target = w.command_migrate(HERD, Vec2::new(32.0, 32.0)).unwrap();
    let members = herd(&w, HERD);
    let n = members.len();
    // A few at the target, the rest 27 tiles off: not there yet.
    for (k, &i) in members.iter().enumerate() {
        w.organisms[i].pos = if k < n / 4 { target } else { target + Vec2::new(27.0, 0.0) };
    }
    settle_now(&mut w);
    assert_eq!(w.migration_order(HERD, 0), Some(target), "ended with the median member 27 tiles off");
    assert!(w.take_group_events().is_empty());
    // Over half there: arrived, and the group settles at the target.
    for &i in &members[..n / 2 + 1] {
        w.organisms[i].pos = target;
    }
    settle_now(&mut w);
    assert_eq!(w.migration_order(HERD, 0), None);
    assert!(w.take_group_events().contains(&GroupEvent::Arrived { player: HERD, group: 0, target }));
    let s = w.settle_circle(HERD, 0).expect("settles");
    assert_eq!(s.center, target);
    assert!(s.radius >= World::arrival_radius(n));
    assert!(s.until_tick > w.tick);
}

#[test]
fn a_migration_that_never_gathers_times_out() {
    let mut w = standard();
    let members = herd(&w, HERD);
    let middle = w.group_middle(HERD, 0).unwrap();
    let target = w.command_migrate(HERD, Vec2::new(60.0, 4.0)).unwrap();
    let plan = w.migration_plan(HERD, 0).unwrap();
    assert_eq!(plan.issued_tick, w.tick);
    assert_eq!(plan.deadline_tick, w.tick + w.migration_timeout_ticks(middle.dist(target)));
    let tpd = w.config.ticks_per_day as u64;
    assert!(w.migration_timeout_ticks(0.0) >= 4 * tpd && w.migration_timeout_ticks(1e6) <= 30 * tpd);
    // Scattered all over the map: it never arrives, and ends at the deadline.
    for (k, &i) in members.iter().enumerate() {
        w.organisms[i].pos = Vec2::new(2.0 + (k % 8) as f32 * 7.0, 10.0 + (k / 8) as f32 * 9.0);
    }
    w.tick = plan.deadline_tick - 1;
    settle_now(&mut w);
    if w.tick < plan.deadline_tick {
        assert!(w.migration_order(HERD, 0).is_some());
    }
    w.tick = plan.deadline_tick;
    settle_now(&mut w);
    assert_eq!(w.migration_order(HERD, 0), None);
    let events = w.take_group_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(*e, GroupEvent::TimedOut { player: HERD, group: 0, median, .. } if median > 3.0)),
        "{events:?}"
    );
    assert_eq!(w.settle_circle(HERD, 0), None, "no settling after a timeout");
}

#[test]
fn a_reroute_keeps_the_orders_age_and_never_shortens_it() {
    let mut w = standard();
    w.command_migrate(HERD, Vec2::new(60.0, 60.0)).unwrap();
    let first = w.migration_plan(HERD, 0).unwrap();
    w.tick += 10;
    let to = w.reroute_migration(HERD, 0, Vec2::new(30.0, 30.0)).unwrap();
    let now = w.migration_plan(HERD, 0).unwrap();
    assert_eq!(w.migration_order(HERD, 0), Some(to));
    assert_eq!(now.issued_tick, first.issued_tick);
    assert!(now.deadline_tick >= first.deadline_tick);
    assert_eq!(w.reroute_migration(HERD, 2, to), Err(CommandError::NoSuchGroup(2)));
}

#[test]
fn an_arrived_group_is_pulled_back_to_where_it_settled() {
    let mut w = standard();
    let target = w.command_migrate(HERD, Vec2::new(32.0, 32.0)).unwrap();
    let members = herd(&w, HERD);
    for &i in &members {
        w.organisms[i].pos = target;
    }
    settle_now(&mut w);
    let s = w.settle_circle(HERD, 0).unwrap();
    // One wanders off, fed and watered: it heads back.
    let i = members.iter().copied().find(|&i| w.organisms[i].matured).unwrap();
    let id = w.organisms[i].id;
    let away = w.map.clamp(target + Vec2::new(s.radius + 8.0, 0.0));
    let o = &mut w.organisms[i];
    (o.pos, o.needs.thirst, o.needs.hunger, o.needs.energy, o.needs.breeding) = (away, 0.0, 0.0, 100.0, 0.0);
    w.step();
    let o = w.organism(id).unwrap();
    assert_eq!((o.action, o.target), (Action::Rally, Some(target)));
    // Parched, it drinks instead.
    // (Decisions may be staggered over a few ticks.)
    let mut drank = false;
    for _ in 0..8 {
        let i = w.index[&id];
        let o = &mut w.organisms[i];
        (o.pos, o.needs.thirst) = (away, 90.0);
        w.step();
        drank |= w.organism(id).unwrap().action == Action::Drink;
    }
    assert!(drank, "never broke off to drink");
    // A new order replaces the settling; it lapses on its own after the days.
    w.rally_group(HERD, 0, target).unwrap();
    assert_eq!(w.settle_circle(HERD, 0), None);
}

#[test]
fn thirst_takes_members_out_of_a_territory_or_rally() {
    let mut w = standard();
    let members = herd(&w, HERD);
    let c = w.organisms[members[0]].pos;
    w.claim_territory(HERD, 0, c, 6.0).unwrap();
    let i = members[1];
    w.organisms[i].needs.hunger = 0.0;
    w.organisms[i].needs.thirst = 10.0;
    assert!(w.roam_bound(&w.organisms[i]).is_some());
    w.organisms[i].needs.thirst = 75.0;
    assert!(w.roam_bound(&w.organisms[i]).is_none(), "parched: free to go and drink");
    // Drinking, it keeps at it until thirst is down.
    w.organisms[i].needs.thirst = 45.0;
    w.organisms[i].action = Action::Drink;
    assert!(w.roam_bound(&w.organisms[i]).is_none());
    w.organisms[i].needs.thirst = 20.0;
    assert!(w.roam_bound(&w.organisms[i]).is_some());
    // Outside the circle and parched, it doesn't walk back in.
    w.organisms[i].pos = w.map.clamp(c + Vec2::new(20.0, 0.0));
    w.organisms[i].needs.thirst = 80.0;
    w.step();
    assert_ne!(w.organisms[i].action, Action::Patrol);
}

#[test]
fn claims_and_rallies_over_water_are_refused_and_water_is_reported() {
    let mut w = lake_world(10, 39);
    place(&mut w, 1, &walker(), Map::tile_center(3, 10));
    let lake = Map::tile_center(25, 10);
    assert!(matches!(w.claim_territory(1, 0, lake, 4.0), Err(CommandError::MostlyWater { share }) if share > 0.9));
    assert!(matches!(w.rally_group(1, 0, lake), Err(CommandError::MostlyWater { .. })));
    // On the shore: some water, drinkable within reach, accepted.
    let shore = Map::tile_center(7, 10);
    let check = w.water_check(1, 0, shore, 4.0);
    assert!(check.water_share > 0.0 && check.water_share <= 0.5 && check.drinkable > 0, "{check:?}");
    assert!(w.claim_territory(1, 0, shore, 4.0).is_ok());
    // Dry ground far from any water: accepted, but nothing to drink.
    let mut dry = flat_world(1500.0);
    place(&mut dry, 1, &walker(), Map::tile_center(20, 12));
    for t in &mut dry.map.tiles {
        t.snow = 0.0;
    }
    let check = dry.water_check(1, 0, Map::tile_center(20, 12), 5.0);
    assert_eq!((check.water_share, check.drinkable), (0.0, 0));
}

#[test]
fn abandoning_a_territory_does_not_reset_the_claim_cooldown() {
    let mut w = standard();
    w.set_claim_cooldown_min_ticks(1000);
    assert_eq!(w.claim_cooldown_ticks(), 1000);
    let c = w.organisms[herd(&w, HERD)[0]].pos;
    w.claim_territory(HERD, 0, c, 5.0).unwrap();
    w.abandon_territory(HERD, 0).unwrap();
    w.tick += 50;
    assert_eq!(w.claim_territory(HERD, 0, c, 5.0), Err(CommandError::ClaimCooldown { ticks_left: 950 }));
    assert_eq!(w.groups(HERD)[0].claim_ready_tick, 1000);
    w.tick += 950;
    assert!(w.claim_territory(HERD, 0, c, 5.0).is_ok());
}
