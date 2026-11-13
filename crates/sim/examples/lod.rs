//! Compare level of detail against the full simulation:
//! `cargo run --release --example lod -- [size] [days] [seed] [X,Y ...]`.
//!
//! Runs the standard world twice on one thread, once with every organism an
//! agent and once with LOD on and the given focus points (none: the whole
//! world runs as cohorts), and prints the cost per tick of each and the
//! per-lineage populations (agents plus cohort members) every 10 days, with
//! the legacy shares at the end. `LOD_SET="name=v,name=v"` overrides
//! `LodConfig` fields.

use std::time::Instant;

use evo_sim::math::Vec2;
use evo_sim::{presets, World, WorldConfig};

struct Run {
    /// Per checkpoint: population per lineage.
    counts: Vec<Vec<usize>>,
    agents: Vec<usize>,
    ms_per_tick: f64,
    world: World,
}

fn run(size: usize, days: u32, seed: u64, lod: Option<&[Vec2]>) -> Run {
    let mut config = WorldConfig { seed, width: size, height: size, ..Default::default() };
    if let Some(focus) = lod {
        config.lod.enabled = true;
        if let Ok(sets) = std::env::var("LOD_SET") {
            for kv in sets.split(',').filter(|s| !s.is_empty()) {
                let (k, v) = kv.split_once('=').expect("LOD_SET: name=value");
                assert!(config.lod.set(k, v.parse().expect("LOD_SET value")), "unknown LOD field {k}");
            }
        }
        let mut w = presets::standard_world(config, 1);
        w.set_focus(1, focus.to_vec());
        return finish(w, days);
    }
    finish(presets::standard_world(config, 1), days)
}

fn finish(mut w: World, days: u32) -> Run {
    let mut counts = vec![w.population_counts().iter().map(|c| c.alive).collect()];
    let mut agents = vec![w.alive_count()];
    let start = Instant::now();
    for day in 1..=days {
        w.run_days(1);
        if day % 10 == 0 || day == days {
            counts.push(w.population_counts().iter().map(|c| c.alive).collect());
            agents.push(w.alive_count());
        }
    }
    let ms_per_tick = start.elapsed().as_secs_f64() * 1e3 / (days * w.config.ticks_per_day) as f64;
    Run { counts, agents, ms_per_tick, world: w }
}

fn main() {
    rayon::ThreadPoolBuilder::new().num_threads(1).build_global().expect("thread pool");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let size: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(256);
    let days: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(90);
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let focus: Vec<Vec2> = args[3.min(args.len())..]
        .iter()
        .map(|s| {
            let (x, y) = s.split_once(',').expect("focus X,Y");
            Vec2::new(x.parse().expect("x"), y.parse().expect("y"))
        })
        .collect();

    let full = run(size, days, seed, None);
    let lod = run(size, days, seed, Some(&focus));
    println!(
        "{size}×{size} seed {seed}, {days} days, {} focus point(s), 1 thread",
        focus.len()
    );
    println!("  full: {:.2} ms/tick", full.ms_per_tick);
    println!(
        "  LOD:  {:.2} ms/tick ({:.1}× faster); agents at the end {} of {}",
        lod.ms_per_tick,
        full.ms_per_tick / lod.ms_per_tick,
        lod.agents.last().unwrap(),
        lod.world.population()
    );
    let names: Vec<String> = full.world.lineages.iter().map(|l| l.name.clone()).collect();
    print!("\n  {:>4}", "day");
    for n in &names {
        print!(" {:>15.15}", n);
    }
    println!("\n  {:>4} {}", "", names.iter().map(|_| format!("{:>15}", "full / LOD")).collect::<String>());
    let mut log_err = Vec::new();
    for (k, (a, b)) in full.counts.iter().zip(&lod.counts).enumerate() {
        let day = if k == 0 { 0 } else { (k as u32 * 10).min(days) };
        print!("  {day:>4}");
        for (x, y) in a.iter().zip(b) {
            print!(" {:>15}", format!("{x} / {y}"));
        }
        println!();
        if k > 0 {
            let errs: Vec<f64> = a.iter().zip(b).map(|(&x, &y)| ((y as f64 + 1.0) / (x as f64 + 1.0)).ln().abs()).collect();
            log_err.push(errs.iter().sum::<f64>() / errs.len() as f64);
        }
    }
    let tot = |r: &Run| r.counts.last().unwrap().iter().sum::<usize>();
    println!(
        "\n  total at the end: full {} · LOD {} · mean |log ratio| per lineage over checkpoints {:.2} (final {:.2})",
        tot(&full),
        tot(&lod),
        log_err.iter().sum::<f64>() / log_err.len().max(1) as f64,
        log_err.last().copied().unwrap_or(0.0)
    );
    let (lf, ll) = (full.world.latest_legacy().unwrap(), lod.world.latest_legacy().unwrap());
    println!("  legacy share (full / LOD):");
    for owner in lf.alleles.keys().chain(ll.alleles.keys()).collect::<std::collections::BTreeSet<_>>() {
        println!("    player {owner}: {:.1}% / {:.1}%", lf.share(*owner) * 100.0, ll.share(*owner) * 100.0);
    }
}
