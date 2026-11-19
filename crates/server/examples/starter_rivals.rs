//! Does the starter grow among rivals? Playtests 10 and 11: every newcomer
//! who founded the client's starter design stalled at about 40 animals and
//! retired it. This founds the strong designs of the playtests first (a
//! toxic, bright, egg-incubating herd, the small toxic sleepers and a
//! predator pack, all on day 0), then a newcomer's starter candidate on
//! `--join`, through the server's own rules (`Game::found`, grace, the
//! winter provision), and reports the newcomer's population over a year.
//!
//! `cargo run --release -p evo-server --example starter_rivals -- [--seeds N]
//!  [--first-seed N] [--size N] [--join DAY] [--days DAYS] [--only NAME,..]`
//!
//! Each candidate runs in its own worlds (the same seeds for all).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use evo_server::game::{build_world, Game, GameConfig, WorldSettings};
use evo_sim::{BirthType, FounderDesign, Gene, PlayerId, Thermo};

const CHECKPOINTS: [u64; 4] = [60, 120, 240, 360];
const NEWCOMER: PlayerId = 1_000_001;

struct Args {
    seeds: u64,
    first_seed: u64,
    size: usize,
    join: u64,
    days: u64,
    only: Option<Vec<String>>,
}

fn args() -> Args {
    let mut a = Args { seeds: 8, first_seed: 1, size: 128, join: 60, days: 360, only: None };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().expect("a value");
        match k.as_str() {
            "--seeds" => a.seeds = v().parse().expect("--seeds"),
            "--first-seed" => a.first_seed = v().parse().expect("--first-seed"),
            "--size" => a.size = v().parse().expect("--size"),
            "--join" => a.join = v().parse().expect("--join"),
            "--days" => a.days = v().parse().expect("--days"),
            "--only" => a.only = Some(v().split(',').map(|x| x.trim().to_string()).collect()),
            _ => panic!("unknown argument {k}"),
        }
    }
    a
}

/// The client's starter (`client/src/designMorph.ts` `STARTER_TRAITS`):
/// warm, incubated eggs, 3 a clutch, 20 founders.
fn starter(name: &str) -> FounderDesign {
    use Gene::*;
    FounderDesign::new(name, Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(BodySize, 0.5)
        .with(GroupTolerance, 0.9)
        .with(Boldness, 0.3)
        .with(Fur, 0.6)
        .with(FatStores, 0.6)
        .with(Hibernation, 0.5)
        .founders(20)
}

/// The candidates: the current starter and variants of it.
fn candidates() -> Vec<(&'static str, FounderDesign)> {
    use Gene::*;
    vec![
        ("current", starter("current")),
        ("burrow", starter("burrow").with(Burrowing, 0.5)),
        ("watchful", starter("watchful").with(Vigilance, 0.6).with(Ears, 0.5)),
        ("toxic", starter("toxic").with(Toxins, 0.5)),
        ("burrow+watch", starter("burrow+watch").with(Burrowing, 0.4).with(Vigilance, 0.5)),
        ("burrow+toxic", starter("burrow+toxic").with(Burrowing, 0.4).with(Toxins, 0.4)),
    ]
}

/// Playtest 6's runaway herd (`examples/dominance.rs` `herd`).
fn herd() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Herd", Thermo::Warm, BirthType::IncubatedEggs, 6)
        .with(BodySize, 0.3)
        .with(FatStores, 0.6)
        .with(Fur, 0.6)
        .with(Hibernation, 0.55)
        .with(GroupTolerance, 0.95)
        .with(Obedience, 0.9)
        .with(Toxins, 0.6)
        .with(Coloration, 0.6)
        .with(Ears, 0.3)
        .with(Vigilance, 0.4)
        .with(LegLength, 0.3)
        .with(ForagingRange, 0.6)
        .founders(50)
}

/// Playtest 7's leading class (`examples/dominance.rs` `sleepers`).
fn sleepers() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Sleepers", Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(BodySize, 0.25)
        .with(Toxins, 0.7)
        .with(Coloration, 0.8)
        .with(Hibernation, 0.6)
        .with(Estivation, 0.4)
        .with(GroupTolerance, 0.95)
        .founders(20)
}

/// A predator pack (`examples/dominance.rs` `pack`).
fn pack() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Fang Pack", Thermo::Warm, BirthType::Live, 3)
        .with(Carnivory, 1.0)
        .with(PackInstinct, 1.0)
        .with(GroupTolerance, 0.9)
        .with(BodySize, 0.45)
        .with(Muscle, 0.9)
        .with(Fangs, 0.9)
        .with(Claws, 0.6)
        .with(Nose, 0.8)
        .with(LegLength, 0.45)
        .with(FatStores, 0.6)
        .with(Fur, 0.5)
        .with(Hibernation, 0.55)
        .with(Caching, 0.4)
        .with(Eyes, 0.3)
        .with(Aggression, 0.85)
        .with(Obedience, 0.9)
        .with(Boldness, 0.7)
        .founders(40)
}

/// The newcomer's population at each checkpoint after it founds, and its
/// deaths by cause.
fn run(seed: u64, design: &FounderDesign, a: &Args) -> (Vec<usize>, BTreeMap<String, u64>) {
    let world = build_world(&WorldSettings { seed, width: a.size, height: a.size, ..Default::default() });
    let mut g = Game::new(world, GameConfig::default());
    for (p, name, d) in [(1, "herd", herd()), (2, "sleepers", sleepers()), (3, "pack", pack())] {
        g.set_name(p, name);
        g.found_at_any_price(p, &d).expect("a rival founds");
    }
    let tpd = g.world.config.ticks_per_day as u64;
    let step = |g: &mut Game| {
        for _ in 0..tpd {
            g.world.step();
            g.after_tick();
        }
    };
    for _ in 0..a.join {
        step(&mut g);
    }
    g.set_name(NEWCOMER, "newcomer");
    let lineage = g.found(NEWCOMER, design).expect("the newcomer founds").lineage;
    let mut at = Vec::new();
    for day in 1..=a.days {
        step(&mut g);
        if CHECKPOINTS.contains(&day) {
            at.push(g.world.owner_count(NEWCOMER));
        }
    }
    let causes = (g.world.counters.deaths_by_lineage.iter())
        .filter(|((l, _), _)| *l == lineage)
        .map(|((_, c), &n)| (format!("{c:?}"), n))
        .collect();
    (at, causes)
}

fn main() {
    let a = args();
    let cands: Vec<(&str, FounderDesign)> =
        candidates().into_iter().filter(|(n, _)| a.only.as_ref().is_none_or(|o| o.iter().any(|x| x == n))).collect();
    println!(
        "{} seeds from {}, {}×{} map, rivals on day 0, newcomer on day {}; population at days {:?} after it founds",
        a.seeds, a.first_seed, a.size, a.size, a.join, CHECKPOINTS
    );
    for (name, design) in &cands {
        let next = AtomicUsize::new(0);
        let results = Mutex::new(Vec::new());
        let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).min(a.seeds as usize).max(1);
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| loop {
                    let k = next.fetch_add(1, Ordering::Relaxed) as u64;
                    if k >= a.seeds {
                        break;
                    }
                    let r = run(a.first_seed + k, design, &a);
                    results.lock().unwrap().push(r);
                });
            }
        });
        let results = results.into_inner().unwrap();
        let median = |i: usize| {
            let mut v: Vec<usize> = results.iter().map(|(at, _)| at.get(i).copied().unwrap_or(0)).collect();
            v.sort_unstable();
            v[v.len() / 2]
        };
        let alive = results.iter().filter(|(at, _)| at.last().copied().unwrap_or(0) > 0).count();
        let over = |n: usize| results.iter().filter(|(at, _)| at.last().copied().unwrap_or(0) >= n).count();
        let mut causes: BTreeMap<&str, u64> = BTreeMap::new();
        for (_, c) in &results {
            for (k, n) in c {
                *causes.entry(k.as_str()).or_default() += n;
            }
        }
        println!(
            "{name:>13} ({:.1} points): medians {:?}; alive {alive}/{}, ≥100 at the end {}/{}; deaths {causes:?}",
            design.point_cost(),
            (0..CHECKPOINTS.len()).map(median).collect::<Vec<_>>(),
            results.len(),
            over(100),
            results.len()
        );
    }
}
