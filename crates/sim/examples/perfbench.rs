//! Reproducible speed benchmark:
//! `cargo run --release --features serde --example perfbench -- [--days N] [--seed N] [--seeds N] [--only default|meat]`.
//!
//! Runs a 96×96 world for two simulated years (240 days by default) twice:
//! with the default balance (`presets::standard_world`) and with the
//! carnivore-diet rules and predators in their niches
//! (`Balance::carnivore_diet()` + `presets::niche_world`). For each it
//! prints the wall time, the time per phase of `World::step`, the final
//! population and a determinism fingerprint (FNV-1a of the final world
//! snapshot): an optimisation must leave every fingerprint unchanged.
//! `--seeds N` runs N seeds (from `--seed` up) at once, one per rayon task,
//! as the long experiments (`predlong`, `evo-sim sweep`) do, and reports the
//! wall time of the batch: the throughput when every core has its own world.
//! Thread count: `RAYON_NUM_THREADS`.

use std::time::Instant;

use evo_sim::{presets, Balance, PhaseTimes, WorldConfig};
use rayon::prelude::*;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

struct Run {
    secs: f64,
    org_ticks: u64,
    alive: usize,
    phases: PhaseTimes,
    fingerprint: u64,
}

fn run(seed: u64, meat: bool, days: Option<u32>) -> Run {
    let balance = if meat { Balance::default().carnivore_diet() } else { Balance::default() };
    let config = WorldConfig { seed, width: 96, height: 96, balance, ..Default::default() };
    let start = Instant::now();
    let mut w = if meat { presets::niche_world(config, 1) } else { presets::standard_world(config, 1) };
    let days = days.unwrap_or(2 * w.config.days_per_year);
    let mut org_ticks = 0u64;
    for _ in 0..days * w.config.ticks_per_day {
        org_ticks += w.alive_count() as u64;
        w.step();
    }
    let secs = start.elapsed().as_secs_f64();
    Run { secs, org_ticks, alive: w.alive_count(), phases: w.phase_times, fingerprint: fnv1a(&w.snapshot()) }
}

fn main() {
    let mut days: Option<u32> = None;
    let mut seed = 31u64;
    let mut seeds = 1u64;
    let mut only: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--days" => days = Some(args.next().expect("--days N").parse().expect("--days N")),
            "--seed" => seed = args.next().expect("--seed N").parse().expect("--seed N"),
            "--seeds" => seeds = args.next().expect("--seeds N").parse().expect("--seeds N"),
            "--only" => only = Some(args.next().expect("--only default|meat")),
            other => panic!("unknown option {other}"),
        }
    }
    println!("{} threads", rayon::current_num_threads());
    for (name, meat) in [("default", false), ("meat", true)] {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let start = Instant::now();
        // One seed steps from the main thread, as `evo-sim run` and the
        // server do; several from inside the pool, as the experiments do.
        let runs: Vec<Run> = if seeds == 1 {
            vec![run(seed, meat, days)]
        } else {
            (seed..seed + seeds).into_par_iter().map(|s| run(s, meat, days)).collect()
        };
        let wall = start.elapsed().as_secs_f64();
        for (k, r) in runs.iter().enumerate() {
            let p = r.phases;
            println!(
                "{name:8} seed {} : {:8.2} s  ({:.2} µs/organism-tick, {} alive)  fingerprint {:016x}",
                seed + k as u64,
                r.secs,
                r.secs * 1e6 / r.org_ticks.max(1) as f64,
                r.alive,
                r.fingerprint
            );
            println!(
                "           phases: environment {:.2} s, daily {:.2} s, grid {:.2} s, decide {:.2} s, apply {:.2} s (serial {:.2} s)",
                p.environment, p.daily, p.grid, p.decide, p.apply, p.apply_serial
            );
        }
        if seeds > 1 {
            println!("{name:8} {seeds} seeds at once: {wall:.2} s wall");
        }
    }
}
