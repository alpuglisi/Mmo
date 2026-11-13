//! Measure how often organisms get stuck: `cargo run --release --example stuck -- [days] [seed...]`.
//!
//! An organism is *stuck* when it keeps heading for the same spot (a target
//! that stays within 2 tiles of where it was when the episode began, until
//! it gets within a tile of it) for
//! `WINDOW` ticks without getting 0.5 tiles closer than its best so far,
//! while more than 1.5 tiles away and the spot is reachable on foot (a BFS
//! over the tiles its body can use, ending next to the target). Prints, per
//! lineage, the organism-ticks spent heading somewhere, the stuck episodes
//! per 1000 of them, and the share of them spent stuck. Episodes toward
//! unreachable spots are counted apart. Ticks without a target (a rest, a
//! meal) pause an episode rather than end it.
//!
//! "Frozen" is the share of those ticks in which it didn't move at all
//! (heading more than 0.1 tiles away, position unchanged since the last
//! tick it was heading somewhere).
//!
//! Set `SIZE` (default 128) to change the map, `PATH_NODES` to override
//! `Balance::path_nodes` (0: greedy steering only), `VERBOSE` to print each
//! stuck episode.

use std::collections::{BTreeMap, VecDeque};

use evo_sim::math::Vec2;
use evo_sim::terrain::{traverse, Traverse};
use evo_sim::{presets, World, WorldConfig};

const WINDOW: u32 = 24;

#[derive(Clone, Copy)]
struct Episode {
    anchor: Vec2,
    best: f32,
    since_best: u32,
    stuck: Option<bool>,
}

#[derive(Default, Clone, Copy)]
struct Tally {
    heading: u64,
    episodes: u64,
    stuck_ticks: u64,
    unreachable: u64,
    /// Heading somewhere more than 0.1 tiles away but not moving at all.
    frozen: u64,
}

fn reachable(w: &World, i: usize, goal: Vec2) -> bool {
    let o = &w.organisms[i];
    let map = &w.map;
    let ok = |x: usize, y: usize| {
        matches!(traverse(&o.traits.body, &map.tiles[map.idx(x, y)], &w.weather, false), Traverse::Passable { .. })
    };
    let (gx, gy) = map.tile_coords(goal);
    let (sx, sy) = map.tile_coords(o.pos);
    let mut seen = vec![false; map.width * map.height];
    let mut queue = VecDeque::from([(sx, sy)]);
    seen[map.idx(sx, sy)] = true;
    while let Some((x, y)) = queue.pop_front() {
        if x.abs_diff(gx) <= 1 && y.abs_diff(gy) <= 1 {
            return true;
        }
        for ny in y.saturating_sub(1)..=(y + 1).min(map.height - 1) {
            for nx in x.saturating_sub(1)..=(x + 1).min(map.width - 1) {
                let k = map.idx(nx, ny);
                if !seen[k] && ok(nx, ny) {
                    seen[k] = true;
                    queue.push_back((nx, ny));
                }
            }
        }
    }
    false
}

fn main() {
    let mut args = std::env::args().skip(1);
    let days: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
    let mut seeds: Vec<u64> = args.filter_map(|s| s.parse().ok()).collect();
    if seeds.is_empty() {
        seeds = vec![1, 2];
    }
    let size = std::env::var("SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(128);
    let mut total: BTreeMap<String, Tally> = BTreeMap::new();
    let mut by_action: BTreeMap<String, u64> = BTreeMap::new();
    let verbose = std::env::var("VERBOSE").is_ok();
    for &seed in &seeds {
        let mut config = WorldConfig { seed, width: size, height: size, ..Default::default() };
        if let Some(n) = std::env::var("PATH_NODES").ok().and_then(|s| s.parse().ok()) {
            config.balance.path_nodes = n;
        }
        let mut w = presets::standard_world(config, 1);
        let mut episodes: BTreeMap<u64, Episode> = BTreeMap::new();
        let mut last_pos: BTreeMap<u64, Vec2> = BTreeMap::new();
        let mut tally = vec![Tally::default(); w.lineages.len()];
        for _ in 0..days * w.config.ticks_per_day {
            w.step();
            for i in 0..w.organisms.len() {
                let o = &w.organisms[i];
                if !o.alive {
                    episodes.remove(&o.id);
                    continue;
                }
                // Interruptions (a drink, a rest) don't end an episode.
                let Some(t) = o.target else {
                    continue;
                };
                let d = o.pos.dist(t);
                let t_l = &mut tally[o.lineage as usize];
                t_l.heading += 1;
                if d > 0.1 && last_pos.insert(o.id, o.pos) == Some(o.pos) {
                    t_l.frozen += 1;
                }
                let e = episodes.entry(o.id).or_insert(Episode { anchor: t, best: d, since_best: 0, stuck: None });
                // A new target: far from the old one, or any move once the
                // old one was reached.
                if e.anchor.dist(t) > 2.0 || (e.best < 1.0 && e.anchor.dist(t) > 0.25) {
                    *e = Episode { anchor: t, best: d, since_best: 0, stuck: None };
                }
                if d < e.best - 0.5 {
                    e.best = d;
                    e.since_best = 0;
                    e.stuck = None;
                } else {
                    e.since_best += 1;
                }
                if e.since_best >= WINDOW && d > 1.5 {
                    if e.stuck.is_none() {
                        let r = reachable(&w, i, t);
                        e.stuck = Some(r);
                        if r {
                            t_l.episodes += 1;
                            *by_action.entry(format!("{:?}", o.action)).or_default() += 1;
                            if verbose {
                                let tile = w.map.tile_at(o.pos);
                                println!(
                                    "    {:?} {} at ({:.1},{:.1}) -> ({:.1},{:.1}) d {:.1} best {:.1} {:?} slope {:.2} path {:?}",
                                    o.action,
                                    w.lineages[o.lineage as usize].name,
                                    o.pos.x,
                                    o.pos.y,
                                    t.x,
                                    t.y,
                                    d,
                                    e.best,
                                    tile.biome,
                                    tile.slope,
                                    o.path.as_ref().map(|p| (p.waypoints.len(), p.failed, p.partial)),
                                );
                            }
                        } else {
                            t_l.unreachable += 1;
                        }
                    }
                    if e.stuck == Some(true) {
                        t_l.stuck_ticks += 1;
                    }
                }
            }
        }
        println!("seed {seed}, {size}x{size}, {days} days");
        print_table(&w, &tally);
        for (l, t) in w.lineages.iter().zip(&tally) {
            let s = total.entry(l.name.clone()).or_default();
            s.heading += t.heading;
            s.episodes += t.episodes;
            s.stuck_ticks += t.stuck_ticks;
            s.unreachable += t.unreachable;
            s.frozen += t.frozen;
        }
    }
    if seeds.len() > 1 {
        println!("all seeds");
        let sum = total.values().fold(Tally::default(), |a, t| Tally {
            heading: a.heading + t.heading,
            episodes: a.episodes + t.episodes,
            stuck_ticks: a.stuck_ticks + t.stuck_ticks,
            unreachable: a.unreachable + t.unreachable,
            frozen: a.frozen + t.frozen,
        });
        print_row("TOTAL", &sum);
    }
    println!("stuck episodes by action: {by_action:?}");
}

fn print_table(w: &World, tally: &[Tally]) {
    println!(
        "  {:<22} {:>10} {:>9} {:>9} {:>11} {:>9}",
        "lineage", "heading", "stuck/1k", "% stuck", "unreach/1k", "% frozen"
    );
    let mut sum = Tally::default();
    for (l, t) in w.lineages.iter().zip(tally) {
        print_row(&l.name, t);
        sum.heading += t.heading;
        sum.episodes += t.episodes;
        sum.stuck_ticks += t.stuck_ticks;
        sum.unreachable += t.unreachable;
        sum.frozen += t.frozen;
    }
    print_row("TOTAL", &sum);
}

fn print_row(name: &str, t: &Tally) {
    let h = t.heading.max(1) as f64;
    println!(
        "  {:<22} {:>10} {:>9.2} {:>8.2}% {:>11.2} {:>8.2}%",
        name,
        t.heading,
        t.episodes as f64 * 1000.0 / h,
        t.stuck_ticks as f64 * 100.0 / h,
        t.unreachable as f64 * 1000.0 / h,
        t.frozen as f64 * 100.0 / h
    );
}
