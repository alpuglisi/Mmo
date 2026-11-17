//! Does a beginner's design live? Founds one design through the server's
//! own path (`DesignSpec::to_design`, `Game::found`) into fresh standard
//! worlds (wild ecosystem, catastrophes as in the game), after the world
//! has run a while as a newcomer would find it, then runs the world on and
//! reports how many of the lineages are alive at each checkpoint.
//!
//! `cargo run --release -p evo-server --example starter_survival -- [--design FILE.json]
//!  [--seeds N] [--first-seed N] [--size N] [--warmup DAYS] [--days DAYS] [--days-per-year N]`
//!
//! The design file is the client's `POST /api/lineage` body (name, thermo,
//! birth, litter_size, founders, traits, morph); without one, the editor's
//! starting design (every gene at its default, warm, live birth, 2 young,
//! the base founders).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use evo_server::design::DesignSpec;
use evo_server::game::{build_world, Game, GameConfig, WorldSettings};

const CHECKPOINTS: [u64; 4] = [30, 60, 90, 120];

struct Args {
    design: Option<String>,
    seeds: u64,
    first_seed: u64,
    size: usize,
    warmup: u64,
    days: u64,
    days_per_year: u32,
}

fn args() -> Args {
    let mut a = Args { design: None, seeds: 24, first_seed: 1, size: 128, warmup: 30, days: 120, days_per_year: 120 };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| panic!("{k} needs a value"));
        match k.as_str() {
            "--design" => a.design = Some(v()),
            "--seeds" => a.seeds = v().parse().expect("--seeds"),
            "--first-seed" => a.first_seed = v().parse().expect("--first-seed"),
            "--size" => a.size = v().parse().expect("--size"),
            "--warmup" => a.warmup = v().parse().expect("--warmup"),
            "--days" => a.days = v().parse().expect("--days"),
            "--days-per-year" => a.days_per_year = v().parse().expect("--days-per-year"),
            _ => panic!("unknown argument {k}"),
        }
    }
    a
}

fn starting_design() -> DesignSpec {
    serde_json::from_str(r#"{"name":"Starter","thermo":"warm","birth":"live","litter_size":2,"traits":{}}"#).unwrap()
}

/// What one world gave: the population at each checkpoint (after founding),
/// the day the lineage died out, and its deaths by cause.
type Outcome = (Vec<usize>, Option<u64>, BTreeMap<String, u64>);

fn run(seed: u64, spec: &DesignSpec, a: &Args) -> Outcome {
    let world = build_world(&WorldSettings {
        seed,
        width: a.size,
        height: a.size,
        days_per_year: a.days_per_year,
        ..Default::default()
    });
    let mut g = Game::new(world, GameConfig::default());
    let tpd = g.world.config.ticks_per_day as u64;
    for _ in 0..a.warmup * tpd {
        g.world.step();
        g.after_tick();
    }
    let player = 1_000_001;
    g.set_name(player, "starter");
    let design = spec.to_design().expect("the design is valid");
    let lineage = g.found(player, &design).expect("founded").lineage;
    let mut at = Vec::new();
    let mut extinct = None;
    for day in 1..=a.days {
        for _ in 0..tpd {
            g.world.step();
            g.after_tick();
        }
        let alive = g.world.owner_count(player);
        if alive == 0 && extinct.is_none() {
            extinct = Some(day);
        }
        if CHECKPOINTS.contains(&day) || day == a.days {
            at.push(alive);
        }
        if extinct.is_some() {
            // Nothing more to see: the later checkpoints are all 0.
            at.extend(CHECKPOINTS.iter().filter(|&&c| c > day).map(|_| 0));
            break;
        }
    }
    let causes = (g.world.counters.deaths_by_lineage.iter())
        .filter(|((l, _), _)| *l == lineage)
        .map(|((_, c), &n)| (format!("{c:?}"), n))
        .collect();
    (at, extinct, causes)
}

fn main() {
    let a = args();
    let spec = match &a.design {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(path).expect("read design")).expect("design JSON"),
        None => starting_design(),
    };
    let design = spec.to_design().expect("the design is valid");
    println!(
        "design {:?}: {:.1} points, {} founders; {} seeds from {}, {}×{} map, found on day {}, run {} days",
        spec.name,
        design.point_cost(),
        spec.founders,
        a.seeds,
        a.first_seed,
        a.size,
        a.size,
        a.warmup,
        a.days
    );
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
                let seed = a.first_seed + k;
                let (at, extinct, causes) = run(seed, &spec, &a);
                println!(
                    "  seed {seed:>3}: {at:?}{} deaths {causes:?}",
                    extinct.map_or(String::new(), |d| format!(" (died out on day {d})"))
                );
                results.lock().unwrap().push((seed, at, extinct, causes));
            });
        }
    });
    let results = results.into_inner().unwrap();
    let n = results.len() as f64;
    let alive_at_end = results.iter().filter(|(_, _, e, _)| e.is_none()).count();
    println!(
        "alive after {} days: {alive_at_end} of {} ({:.0}%)",
        a.days,
        results.len(),
        100.0 * alive_at_end as f64 / n
    );
    let mut sizes: Vec<usize> = results.iter().map(|(_, at, _, _)| *at.last().unwrap_or(&0)).collect();
    sizes.sort_unstable();
    println!(
        "population at the end: median {}, min {}, max {}",
        sizes[sizes.len() / 2],
        sizes[0],
        sizes[sizes.len() - 1]
    );
    let mut causes: BTreeMap<&str, u64> = BTreeMap::new();
    for (_, _, _, c) in &results {
        for (k, n) in c {
            *causes.entry(k.as_str()).or_default() += n;
        }
    }
    println!("deaths by cause, all worlds: {causes:?}");
}
