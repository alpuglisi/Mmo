//! Movement around terrain: greedy steering versus pathfinding.

use super::World;
use crate::design::{BirthType, FounderDesign, Thermo};
use crate::genome::Gene;
use crate::math::Vec2;
use crate::terrain::{traverse, Biome, Map, Traverse};
use crate::WorldConfig;

const W: usize = 40;
const H: usize = 24;

/// A flat grassland world, `path_nodes` as given.
fn flat_world(path_nodes: f32) -> World {
    let mut config = WorldConfig { seed: 2, width: W, height: H, ..Default::default() };
    config.balance.path_nodes = path_nodes;
    let mut w = World::new(config);
    for t in &mut w.map.tiles {
        t.biome = Biome::Grassland;
        t.water_depth = 0.0;
        t.ice = 0.0;
        t.snow = 0.0;
        t.mud = 0.0;
        t.sand = 0.0;
        t.slope = 0.0;
        t.veg_density = 0.1;
    }
    w
}

fn set(w: &mut World, x: usize, y: usize, what: &str) {
    let t = &mut w.map.tiles[y * W + x];
    match what {
        "lake" => {
            t.biome = Biome::DeepWater;
            t.water_depth = 3.0;
        }
        "cliff" => {
            t.biome = Biome::Mountain;
            t.slope = 0.9;
        }
        "ice" => t.ice = 1.0,
        _ => unreachable!(),
    }
}

fn walker() -> FounderDesign {
    FounderDesign::new("walker", Thermo::Warm, BirthType::Live, 2).founders(1)
}

fn swimmer() -> FounderDesign {
    walker().with(Gene::Swimming, 0.9).with(Gene::FatStores, 0.5)
}

/// Found one organism of `design` at `at`, rested and fed; its index.
fn place(w: &mut World, owner: u32, design: &FounderDesign, at: Vec2) -> usize {
    w.found_lineage_unchecked(owner, design);
    let i = w.organisms.len() - 1;
    let o = &mut w.organisms[i];
    o.pos = at;
    o.needs.energy = 100.0;
    o.needs.hunger = 0.0;
    i
}

/// Walk organism `i` toward `target` for up to `ticks` steps, asserting it
/// never stands on a tile its body can't use. Returns the steps taken to
/// arrive (None if it never did) and whether it ever was in deep water.
fn walk(w: &mut World, i: usize, target: Vec2, ticks: u32) -> (Option<u32>, bool) {
    let mut wet = false;
    for t in 0..ticks {
        if w.organisms[i].pos.dist(target) < 0.5 {
            return (Some(t), wet);
        }
        w.move_toward(i, target, false);
        w.organisms[i].needs.energy = 100.0;
        let o = &w.organisms[i];
        let tile = w.map.tile_at(o.pos);
        assert!(
            traverse(&o.traits.body, tile, &w.weather, false) != Traverse::Blocked,
            "stepped onto a blocked tile at ({:.1}, {:.1})",
            o.pos.x,
            o.pos.y
        );
        wet |= tile.water_depth > 1.0 && tile.ice < 0.5;
    }
    (None, wet)
}

/// A U of lake and cliff opening toward the walker: steering straight at
/// the target walks into the pocket and stays there; a route leads around.
#[test]
fn pathing_escapes_a_concave_barrier_where_greedy_steering_sticks() {
    let build = |path_nodes: f32| {
        let mut w = flat_world(path_nodes);
        for y in 4..=20 {
            set(&mut w, 22, y, if y < 12 { "lake" } else { "cliff" });
        }
        for x in 12..=22 {
            set(&mut w, x, 4, "lake");
            set(&mut w, x, 20, "cliff");
        }
        let i = place(&mut w, 1, &walker(), Map::tile_center(5, 12));
        (w, i)
    };
    let target = Map::tile_center(34, 12);
    let (mut greedy, i) = build(0.0);
    let (arrived, _) = walk(&mut greedy, i, target, 400);
    assert_eq!(arrived, None, "greedy steering got out of the pocket");
    assert!(greedy.organisms[i].pos.x > 15.0, "greedy walker should be stuck in the pocket");
    let (mut pathed, i) = build(1500.0);
    let (arrived, _) = walk(&mut pathed, i, target, 400);
    assert!(arrived.is_some(), "pathing walker stuck at {:?}", pathed.organisms[i].pos);
    assert!(pathed.counters.path_searches > 0);
}

/// A cliff forces both to plan. The walker goes around the lake through
/// the gap at the bottom, never touching deep water; the swimmer cuts
/// straight across.
#[test]
fn only_swimmers_path_through_deep_water() {
    let build = |design: &FounderDesign| {
        let mut w = flat_world(1500.0);
        for y in 0..=8 {
            set(&mut w, 10, y, "cliff");
        }
        for x in 15..=25 {
            for y in 0..=18 {
                set(&mut w, x, y, "lake");
            }
        }
        let i = place(&mut w, 1, design, Map::tile_center(5, 3));
        (w, i)
    };
    let target = Map::tile_center(34, 3);
    let (mut w, i) = build(&walker());
    assert!(w.organisms[i].traits.body.swim < 0.25);
    let (walked, wet) = walk(&mut w, i, target, 600);
    let walked = walked.expect("walker never got around the lake");
    assert!(!wet);

    let (mut w, i) = build(&swimmer());
    let (swum, wet) = walk(&mut w, i, target, 600);
    let swum = swum.expect("swimmer never arrived");
    assert!(wet, "swimmer never swam");
    assert!(swum < walked, "swimmer {swum} ticks, walker {walked}");
}

/// A lake splits the map. In summer there's no way across for a walker;
/// once a strip of it freezes (away from the straight line), the walker
/// finds the ice bridge, which steering straight at the target never would.
#[test]
fn an_ice_bridge_opens_a_route_in_winter() {
    let build = |path_nodes: f32| {
        let mut w = flat_world(path_nodes);
        for x in 15..=25 {
            for y in 0..H {
                set(&mut w, x, y, "lake");
            }
        }
        let i = place(&mut w, 1, &walker(), Map::tile_center(5, 14));
        (w, i)
    };
    let freeze = |w: &mut World| {
        for x in 15..=25 {
            for y in 2..=3 {
                set(w, x, y, "ice");
            }
        }
    };
    let target = Map::tile_center(34, 14);
    let (mut w, i) = build(1500.0);
    let (arrived, _) = walk(&mut w, i, target, 100);
    assert_eq!(arrived, None);
    assert!(w.counters.path_failures > 0);
    assert!(w.organisms[i].pos.x < 15.0);

    freeze(&mut w);
    // Terrain changes on the hour; the failed search is retried then.
    w.tick += w.config.ticks_per_hour();
    let (arrived, _) = walk(&mut w, i, target, 400);
    assert!(arrived.is_some(), "walker stuck at {:?}", w.organisms[i].pos);

    let (mut greedy, i) = build(0.0);
    freeze(&mut greedy);
    assert_eq!(walk(&mut greedy, i, target, 400).0, None);
}
