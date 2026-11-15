//! Tick cost at a high population, reproducibly:
//! `cargo run --release --features serde --example highpop -- warm DIR [--players 500] [--until 65000] [--max-days 200] [--seed 1] [--size 512]`
//! then `... --example highpop -- bench DIR/day0060.snap [--ticks 480] [--threads 4]`.
//!
//! `warm` builds a world the way a server does (`evo-server`'s
//! `game::build_world`: the wild ecosystem scaled to the map's area,
//! catastrophes on) and founds `--players` player lineages with the load
//! generator's designs (12 founders, mostly one warm-blooded body plan,
//! every tenth a cold-blooded egg layer), then runs day by day, printing
//! the population, and writes a snapshot (`DIR/dayNNNN.snap`, with the
//! terrain in `DIR/terrain.bin`) every `--every` days once the population
//! reaches `--until` organisms (stopping after `--keep` of them or at
//! `--max-days`). No commands are given, so it is a sim-only stand-in for
//! a 500-player server world grown for an hour or two.
//!
//! `bench` restores a snapshot and steps it `--ticks` times (a multiple of
//! 48 covers whole days: one tick in 48 runs the daily phase), timing every
//! tick and every phase of it (`World::phase_times`), and prints the tick
//! p50 / p99 / mean, the per-phase means, the daily tick on its own, and
//! a hash of the final state (FNV-1a of `World::snapshot`), which must not
//! change with an optimisation or with the thread count.

use std::sync::Arc;
use std::time::Instant;

use evo_sim::terrain::Terrain;
use evo_sim::{presets, BirthType, FounderDesign, Gene, PhaseTimes, PlayerId, Thermo, World, WorldConfig};
use rand::{Rng, SeedableRng};

fn arg<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|v| v.parse().unwrap_or_else(|_| panic!("{name}: bad value '{v}'")))
        .unwrap_or(default)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The load generator's founding design for player `i` (`loadgen::design`).
fn load_design(i: usize, rng: &mut rand::rngs::StdRng) -> FounderDesign {
    let odd = i % 10 == 9;
    let (thermo, birth) = if odd { (Thermo::Cold, BirthType::IncubatedEggs) } else { (Thermo::Warm, BirthType::Live) };
    FounderDesign::new(format!("Load lineage {i}"), thermo, birth, 2)
        .with(Gene::BodySize, rng.gen_range(0.3..0.5))
        .with(Gene::Fur, rng.gen_range(0.2..0.5))
        .with(Gene::Nose, rng.gen_range(0.1..0.4))
        .founders(12)
}

fn warm(args: &[String]) {
    let dir = args.get(1).expect("warm DIR");
    std::fs::create_dir_all(dir).expect("create DIR");
    let size: usize = arg(args, "--size", 512);
    let players: usize = arg(args, "--players", 500);
    let until: usize = arg(args, "--until", 65_000);
    let max_days: u32 = arg(args, "--max-days", 200);
    let every: u32 = arg(args, "--every", 10);
    let keep: u32 = arg(args, "--keep", 3);
    let seed: u64 = arg(args, "--seed", 1);
    // `evo-server`'s `game::world_config` and `game::build_world`.
    let area = (size * size) as f32 / (presets::REFERENCE_SIZE * presets::REFERENCE_SIZE) as f32;
    let mut config = WorldConfig { seed, width: size, height: size, ..Default::default() };
    config.wild_cap = ((config.wild_cap as f32 * area) as usize).max(50);
    config.lineage_cap = ((config.lineage_cap as f32 * area.max(0.5)) as usize).max(50);
    config.scoring.catastrophes = true;
    let mut w = World::new(config);
    for (owner, design) in presets::wild_designs() {
        let n = ((design.founders as f32 * area).round() as u32).max(4);
        w.found_lineage_unchecked(owner, &design.founders(n));
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let t = Instant::now();
    for i in 0..players {
        let d = load_design(i, &mut rng);
        w.found_lineage(i as PlayerId + 1, &d).expect("load design fits the budget");
    }
    println!("founded {} organisms ({players} player lineages) in {:.1} s", w.alive_count(), t.elapsed().as_secs_f64());
    std::fs::write(format!("{dir}/terrain.bin"), w.map.terrain().encode()).expect("write terrain");
    let mut saved = 0;
    let mut first_full = None;
    for day in 1..=max_days {
        let t = Instant::now();
        w.run_days(1);
        let players_alive = w.organisms.iter().filter(|o| o.alive && o.owner != evo_sim::WILD).count();
        println!(
            "day {day:4}: {:6} organisms ({players_alive} player), {:.0} ms per tick",
            w.alive_count(),
            t.elapsed().as_secs_f64() * 1e3 / w.config.ticks_per_day as f64
        );
        if first_full.is_none() && w.alive_count() >= until {
            first_full = Some(day);
        }
        if let Some(f) = first_full {
            if (day - f).is_multiple_of(every) {
                let path = format!("{dir}/day{day:04}.snap");
                std::fs::write(&path, w.snapshot()).expect("write snapshot");
                println!("  wrote {path}");
                saved += 1;
                if saved >= keep {
                    break;
                }
            }
        }
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn bench(args: &[String]) {
    let path = args.get(1).expect("bench SNAPSHOT");
    let ticks: usize = arg(args, "--ticks", 480);
    let threads: usize = arg(args, "--threads", rayon::current_num_threads());
    let dir = std::path::Path::new(path).parent().expect("snapshot directory");
    let terrain = Terrain::decode(&std::fs::read(dir.join("terrain.bin")).expect("read terrain")).expect("terrain");
    let bytes = std::fs::read(path).expect("read snapshot");
    let mut w = World::restore(&bytes, Some(Arc::new(terrain))).expect("restore");
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("pool");
    println!("{path}: day {}, {} organisms, {ticks} ticks on {threads} threads", w.day(), w.alive_count());
    let mut tick_ms = Vec::with_capacity(ticks);
    let mut phases: Vec<PhaseTimes> = Vec::with_capacity(ticks);
    let mut daily_ms = Vec::new();
    let mut pop = 0usize;
    let (searches, failures) = (w.counters.path_searches, w.counters.path_failures);
    pool.install(|| {
        for _ in 0..ticks {
            let daily = w.tick().is_multiple_of(w.config.ticks_per_day as u64);
            pop += w.alive_count();
            w.phase_times = PhaseTimes::default();
            let t = Instant::now();
            w.step();
            let ms = t.elapsed().as_secs_f64() * 1e3;
            tick_ms.push(ms);
            phases.push(w.phase_times);
            if daily {
                daily_ms.push(ms);
            }
        }
    });
    let mut sorted = tick_ms.clone();
    sorted.sort_by(f64::total_cmp);
    let mean = |f: &dyn Fn(&PhaseTimes) -> f64| phases.iter().map(f).sum::<f64>() * 1e3 / ticks as f64;
    println!("  {:.0} organisms on average, {} at the end", pop as f64 / ticks as f64, w.alive_count());
    println!(
        "  {:.0} route searches per tick ({:.0} failed)",
        (w.counters.path_searches - searches) as f64 / ticks as f64,
        (w.counters.path_failures - failures) as f64 / ticks as f64
    );
    println!(
        "  tick p50 {:.1} ms, p99 {:.1} ms, mean {:.1} ms, max {:.1} ms",
        percentile(&sorted, 0.5),
        percentile(&sorted, 0.99),
        sorted.iter().sum::<f64>() / ticks as f64,
        sorted[ticks - 1]
    );
    println!(
        "  phases, mean ms per tick: environment {:.2}, daily {:.2}, grid {:.2}, decide {:.2}, apply {:.2} (sequential sweeps {:.2})",
        mean(&|p| p.environment),
        mean(&|p| p.daily),
        mean(&|p| p.grid),
        mean(&|p| p.decide),
        mean(&|p| p.apply),
        mean(&|p| p.apply_serial)
    );
    if !daily_ms.is_empty() {
        let daily_phase: Vec<f64> = phases.iter().filter(|p| p.daily > 1e-4).map(|p| p.daily * 1e3).collect();
        println!(
            "  daily ticks: {:?} ms (daily phase {:?} ms)",
            daily_ms.iter().map(|v| v.round()).collect::<Vec<_>>(),
            daily_phase.iter().map(|v| v.round()).collect::<Vec<_>>()
        );
    }
    println!("  final state hash {:016x} (tick {})", fnv1a(&w.snapshot()), w.tick());
}

/// What a decision's neighbor scan meets in this world: living organisms
/// within the largest perception radius (`WorldConfig::max_perception`) of
/// each one, the share with more than the 32 the scan looks at closely, and
/// how many of those neighbors are of another lineage.
fn stats(args: &[String]) {
    let path = args.get(1).expect("stats SNAPSHOT");
    let dir = std::path::Path::new(path).parent().expect("snapshot directory");
    let terrain = Terrain::decode(&std::fs::read(dir.join("terrain.bin")).expect("read terrain")).expect("terrain");
    let w = World::restore(&std::fs::read(path).expect("read snapshot"), Some(Arc::new(terrain))).expect("restore");
    let r = w.config.max_perception;
    let alive: Vec<&evo_sim::organism::Organism> = w.organisms.iter().filter(|o| o.alive).collect();
    let cell = r.max(1.0);
    let cols = (w.config.width as f32 / cell).ceil() as usize;
    let rows = (w.config.height as f32 / cell).ceil() as usize;
    let at = |v: f32, n: usize| ((v.max(0.0) / cell) as usize).min(n - 1);
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); cols * rows];
    for (k, o) in alive.iter().enumerate() {
        grid[at(o.pos.y, rows) * cols + at(o.pos.x, cols)].push(k);
    }
    let (mut counts, mut other_lineage, mut total) = (Vec::with_capacity(alive.len()), 0u64, 0u64);
    for (i, o) in alive.iter().enumerate() {
        let (cx, cy) = (at(o.pos.x, cols), at(o.pos.y, rows));
        let mut n = 0u32;
        for y in cy.saturating_sub(1)..=(cy + 1).min(rows - 1) {
            for x in cx.saturating_sub(1)..=(cx + 1).min(cols - 1) {
                for &k in &grid[y * cols + x] {
                    let q = alive[k];
                    if k != i && q.pos.dist(o.pos) <= r {
                        n += 1;
                        other_lineage += u64::from(q.lineage != o.lineage);
                    }
                }
            }
        }
        total += n as u64;
        counts.push(n);
    }
    counts.sort_unstable();
    let pct = |p: f64| counts[((counts.len() - 1) as f64 * p) as usize];
    let crowded = counts.iter().filter(|&&n| n > 32).count();
    let players = alive.iter().filter(|o| o.owner != evo_sim::WILD).count();
    let dormant = alive.iter().filter(|o| o.dormancy.is_deep()).count();
    let lineages: std::collections::BTreeSet<u32> = alive.iter().map(|o| o.lineage).collect();
    println!(
        "{path}: day {} ({:?}), {} organisms ({players} in player lineages, {} lineages alive, {dormant} deeply dormant)",
        w.day(),
        w.season(),
        alive.len(),
        lineages.len()
    );
    println!(
        "  neighbors within {r} tiles: mean {:.1}, p50 {}, p90 {}, p99 {}, max {}; {:.1}% have more than 32; \
         {:.0}% of neighbors are of another lineage",
        total as f64 / alive.len() as f64,
        pct(0.5),
        pct(0.9),
        pct(0.99),
        counts[counts.len() - 1],
        100.0 * crowded as f64 / alive.len() as f64,
        100.0 * other_lineage as f64 / total.max(1) as f64
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("warm") => warm(&args),
        Some("bench") => bench(&args),
        Some("stats") => stats(&args),
        _ => eprintln!("usage: highpop warm DIR [...] | bench SNAPSHOT [--ticks N] [--threads N] | stats SNAPSHOT"),
    }
}
