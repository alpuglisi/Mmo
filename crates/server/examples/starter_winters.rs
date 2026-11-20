//! Does the starter keep its herd through winter? Playtest 12: the
//! round-12 starter (hibernation 0.5, fat stores 0.6) climbed to #2-3 but
//! lost 50-60% of its herd in every winter sleep, mostly to starvation
//! while asleep. This founds the playtests' strong designs on day 0 and a
//! newcomer's candidate on `--join`, as `starter_rivals` does, and follows
//! the newcomer for `--days` (default 600: five 120-day years), reporting
//! for each winter the herd at its start, its low and the next spring, and
//! the deaths by cause over the winters.
//!
//! `cargo run --release -p evo-server --example starter_winters -- [--seeds N]
//!  [--first-seed N] [--size N] [--join DAY] [--days DAYS] [--only NAME,..]`

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use evo_server::game::{build_world, Game, GameConfig, WorldSettings};
use evo_sim::terrain::Season;
use evo_sim::{BirthType, FounderDesign, Gene, PlayerId, Thermo};

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
    let mut a = Args { seeds: 6, first_seed: 1, size: 128, join: 60, days: 600, only: None };
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

/// The starter (`client/src/designMorph.ts`, round 13: hibernation and fat
/// stores 0.8): warm, incubated eggs, 3 a clutch, 30 founders.
fn starter(name: &str) -> FounderDesign {
    use Gene::*;
    FounderDesign::new(name, Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(BodySize, 0.5)
        .with(GroupTolerance, 0.95)
        .with(Boldness, 0.3)
        .with(Fur, 0.6)
        .with(FatStores, 0.8)
        .with(Hibernation, 0.8)
        .with(Toxins, 0.5)
        .with(Coloration, 0.6)
        .with(ForagingRange, 0.6)
        .founders(30)
}

fn candidates() -> Vec<(&'static str, FounderDesign)> {
    use Gene::*;
    vec![
        ("current", starter("current")),
        // Round 12's, for reference.
        ("r12", starter("r12").with(Hibernation, 0.5).with(FatStores, 0.6)),
        ("estivate", starter("estivate").with(Estivation, 0.3)),
        ("burrow", starter("burrow").with(Burrowing, 0.4)),
        ("migrate", starter("migrate").with(Migration, 0.5)),
        ("camo", starter("camo").with(Camouflage, 0.5)),
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

/// One world: each winter's (start, low, next spring) herd, the deaths by
/// cause in winters and in all, and the herd at the end.
type Outcome = (Vec<(usize, usize, usize)>, BTreeMap<String, u64>, BTreeMap<String, u64>, usize);

fn run(seed: u64, design: &FounderDesign, a: &Args) -> Outcome {
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
    let lineage = g.found_at_any_price(NEWCOMER, design).expect("the newcomer founds").lineage;
    let deaths = |g: &Game| -> BTreeMap<String, u64> {
        (g.world.counters.deaths_by_lineage.iter())
            .filter(|((l, _), _)| *l == lineage)
            .map(|((_, c), &n)| (format!("{c:?}"), n))
            .collect()
    };
    let mut winters = Vec::new();
    let mut winter_deaths: BTreeMap<String, u64> = BTreeMap::new();
    let mut open: Option<(usize, usize, BTreeMap<String, u64>)> = None;
    let mut was = g.world.season();
    for _ in 0..a.days {
        step(&mut g);
        let n = g.world.owner_count(NEWCOMER);
        let now = g.world.season();
        if now == Season::Winter && was != Season::Winter {
            open = Some((n, n, deaths(&g)));
        } else if now == Season::Winter {
            if let Some(o) = open.as_mut() {
                o.1 = o.1.min(n);
            }
        } else if was == Season::Winter {
            if let Some((start, low, before)) = open.take() {
                winters.push((start, low, n));
                for (k, v) in deaths(&g) {
                    *winter_deaths.entry(k.clone()).or_default() += v - before.get(&k).copied().unwrap_or(0);
                }
            }
        }
        was = now;
        if n == 0 {
            break;
        }
    }
    let end = g.world.owner_count(NEWCOMER);
    (winters, winter_deaths, deaths(&g), end)
}

fn main() {
    let a = args();
    let cands: Vec<(&str, FounderDesign)> =
        candidates().into_iter().filter(|(n, _)| a.only.as_ref().is_none_or(|o| o.iter().any(|x| x == n))).collect();
    println!(
        "{} seeds from {}, {}×{} map, rivals on day 0, newcomer on day {} for {} days",
        a.seeds, a.first_seed, a.size, a.size, a.join, a.days
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
        // Each winter's kept share (next spring over its start), all worlds.
        let mut kept: Vec<f64> = results
            .iter()
            .flat_map(|(w, ..)| w.iter().filter(|w| w.0 >= 10).map(|w| w.2 as f64 / w.0 as f64))
            .collect();
        kept.sort_by(f64::total_cmp);
        let med = |v: &[f64]| if v.is_empty() { f64::NAN } else { v[v.len() / 2] };
        let mut ends: Vec<usize> = results.iter().map(|r| r.3).collect();
        ends.sort_unstable();
        let mut wd: BTreeMap<&str, u64> = BTreeMap::new();
        let mut all: BTreeMap<&str, u64> = BTreeMap::new();
        for (_, w, d, _) in &results {
            for (k, n) in w {
                *wd.entry(k.as_str()).or_default() += n;
            }
            for (k, n) in d {
                *all.entry(k.as_str()).or_default() += n;
            }
        }
        let alive = ends.iter().filter(|&&n| n > 0).count();
        println!(
            "{name:>10} ({:.1} pts): winters kept median {:.0}% (quartiles {:.0}%-{:.0}%, {} winters); end median {} (alive {alive}/{}); \
             winter deaths {wd:?}; all deaths {all:?}",
            design.point_cost(),
            100.0 * med(&kept),
            100.0 * kept.get(kept.len() / 4).copied().unwrap_or(f64::NAN),
            100.0 * kept.get(3 * kept.len() / 4).copied().unwrap_or(f64::NAN),
            kept.len(),
            ends[ends.len() / 2],
            results.len()
        );
        for (w, ..) in &results {
            println!("    {w:?}");
        }
    }
}
