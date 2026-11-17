//! Level of detail against agents, over the years:
//! `cargo run --release --example lodmeat -- [years] [seeds..] [--meat] [--every DAYS] [--set NAME=V] [--lod-set NAME=V] [--quiet]`.
//!
//! Runs each seed twice (96×96, in parallel): every organism an agent, and
//! LOD on with no focus point (the whole world as cohorts). `--meat` runs the
//! niche world (`presets::niche_world`) with the carnivore-diet rules and
//! predator brakes (`Balance::carnivore_diet`); without it, the standard
//! world at the defaults. Every `--every` days (default 30) it prints each
//! lineage's head count in both runs (agents / cohorts), then per lineage
//! the disagreement (mean over samples and seeds of `|ln((agents + 1) /
//! (cohorts + 1))|`: 0.69 is a factor of two), the mean head count in each
//! run, and the seeds on which it is alive at the end in each. Read only
//! (*LOD cohorts and the carnivore rules* in the README).

// Parallel per-lineage tables, indexed by lineage.
#![allow(clippy::needless_range_loop)]

use evo_sim::{presets, Balance, WorldConfig};
use rayon::prelude::*;

fn main() {
    let mut balance = Balance::default();
    let mut meat = false;
    let mut every = 30u32;
    let mut quiet = false;
    let mut lod_sets: Vec<(String, f32)> = Vec::new();
    let mut pos: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--meat" => {
                balance = balance.carnivore_diet();
                meat = true;
            }
            "--set" => {
                let s = args.next().expect("--set NAME=V");
                let (n, v) = s.split_once('=').expect("--set NAME=V");
                assert!(balance.set(n, v.parse().unwrap()), "unknown parameter {n}");
            }
            "--lod-set" => {
                let s = args.next().expect("--lod-set NAME=V");
                let (n, v) = s.split_once('=').expect("--lod-set NAME=V");
                lod_sets.push((n.to_string(), v.parse().unwrap()));
            }
            "--every" => every = args.next().expect("--every DAYS").parse().unwrap(),
            "--quiet" => quiet = true,
            _ => pos.push(a),
        }
    }
    let years: u32 = pos.first().and_then(|s| s.parse().ok()).unwrap_or(2);
    let seeds: Vec<u64> =
        if pos.len() > 1 { pos[1..].iter().map(|s| s.parse().unwrap()).collect() } else { vec![31, 32, 33] };

    let jobs: Vec<(u64, bool)> = seeds.iter().flat_map(|&s| [(s, false), (s, true)]).collect();
    let runs: Vec<(Vec<String>, Vec<Vec<usize>>)> = jobs
        .par_iter()
        .map(|&(seed, lod)| {
            let mut config = WorldConfig { seed, width: 96, height: 96, balance, ..Default::default() };
            if lod {
                config.lod.enabled = true;
                for (n, v) in &lod_sets {
                    assert!(config.lod.set(n, *v), "unknown LOD parameter {n}");
                }
            }
            let mut w = if meat { presets::niche_world(config, 1) } else { presets::standard_world(config, 1) };
            let names = w.lineages.iter().map(|l| l.name.chars().take(6).collect()).collect();
            let days = years * w.config.days_per_year;
            let mut samples = Vec::new();
            for day in 1..=days {
                w.run_days(1);
                if day % every == 0 || day == days {
                    samples.push(w.population_counts().iter().map(|c| c.alive).collect());
                }
            }
            (names, samples)
        })
        .collect();

    let names = &runs[0].0;
    let n = names.len();
    let mut err = vec![0.0f64; n];
    let mut mean = vec![[0.0f64; 2]; n];
    let mut alive = vec![[0u32; 2]; n];
    let mut samples = 0usize;
    for (k, &seed) in seeds.iter().enumerate() {
        let (a, c) = (&runs[2 * k].1, &runs[2 * k + 1].1);
        for (j, (sa, sc)) in a.iter().zip(c).enumerate() {
            if !quiet {
                let day = ((j as u32 + 1) * every).min(years * 120);
                let mut line = format!("seed {seed} day {day:4}:");
                for l in 0..n {
                    line += &format!(" {} {}/{}", names[l], sa[l], sc[l]);
                }
                println!("{line}");
            }
            for l in 0..n {
                err[l] += (((sa[l] + 1) as f64) / ((sc[l] + 1) as f64)).ln().abs();
                mean[l][0] += sa[l] as f64;
                mean[l][1] += sc[l] as f64;
            }
            samples += 1;
        }
        for l in 0..n {
            alive[l][0] += u32::from(a.last().unwrap()[l] > 0);
            alive[l][1] += u32::from(c.last().unwrap()[l] > 0);
        }
    }
    println!(
        "{} years, seeds {seeds:?}, {}: lineage, disagreement |ln ratio|, mean agents / cohorts, alive at the end agents / cohorts",
        years,
        if meat { "niche world, carnivore rules on" } else { "standard world, defaults" }
    );
    let mut total = 0.0;
    for l in 0..n {
        let e = err[l] / samples as f64;
        total += e;
        println!(
            "  {:6} {:5.2}   {:6.0} / {:6.0}   {}/{} of {}",
            names[l],
            e,
            mean[l][0] / samples as f64,
            mean[l][1] / samples as f64,
            alive[l][0],
            alive[l][1],
            seeds.len()
        );
    }
    println!("  mean disagreement {:.2}", total / n as f64);
}
