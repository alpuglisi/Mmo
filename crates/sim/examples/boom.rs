//! The first-year prey boom: `cargo run --release --example boom -- [days] [seeds..] [--every DAYS] [--set NAME=V] [--founding SPEC] [--standard]`.
//!
//! Runs the niche world (`presets::niche_world`, 96×96) with the
//! carnivore-diet rules and predator brakes (`Balance::carnivore_diet`) on
//! each seed (seeds in parallel; default 120 days on seeds 31–34) and
//! prints, every `--every` days (default 15), each lineage's head count as a
//! share of its population cap, then per lineage the days it spent at 95%
//! of its cap or more (the sweep's cap penalty counts a day on which any
//! lineage is there) and the day it first got there. `--founding SPEC` founds
//! with `presets::NicheFounding::parse(SPEC)` (*First-year prey boom:
//! founding* in the README). `--standard` runs the standard world with the
//! default rules instead. Read only.

use evo_sim::presets::{self, NicheFounding};
use evo_sim::{Balance, WorldConfig};
use rayon::prelude::*;

fn main() {
    let mut balance = Balance::default().carnivore_diet();
    let mut founding = NicheFounding::default();
    let mut pos: Vec<String> = Vec::new();
    let mut every = 15u32;
    let mut standard = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--set" {
            let s = args.next().expect("--set NAME=V");
            let (n, v) = s.split_once('=').expect("--set NAME=V");
            assert!(balance.set(n, v.parse().unwrap()), "unknown parameter {n}");
        } else if a == "--founding" {
            founding = NicheFounding::parse(&args.next().expect("--founding SPEC")).expect("founding spec");
        } else if a == "--standard" {
            balance = Balance::default();
            standard = true;
        } else if a == "--every" {
            every = args.next().expect("--every DAYS").parse().unwrap();
        } else {
            pos.push(a);
        }
    }
    let days: u32 = pos.first().and_then(|s| s.parse().ok()).unwrap_or(120);
    let seeds: Vec<u64> =
        if pos.len() > 1 { pos[1..].iter().map(|s| s.parse().unwrap()).collect() } else { (31..=34).collect() };

    let results: Vec<(String, Vec<u32>, u32)> = seeds
        .par_iter()
        .map(|&seed| {
            let config = WorldConfig { seed, width: 96, height: 96, balance, ..Default::default() };
            let mut w = if standard {
                presets::standard_world(config, 1)
            } else {
                presets::niche_world_with(config, 1, &founding)
            };
            let n = w.lineages.len();
            let mut log = String::new();
            let mut capped = vec![0u32; n];
            let mut first = vec![None; n];
            let mut any = 0u32;
            let short: Vec<String> = w.lineages.iter().map(|l| l.name.chars().take(6).collect()).collect();
            for day in 1..=days {
                w.run_days(1);
                let mut hit = false;
                for l in 0..n {
                    let (c, cap) = w.cap_usage(l as u32);
                    if c as f32 >= 0.95 * cap as f32 {
                        capped[l] += 1;
                        first[l].get_or_insert(day);
                        hit = true;
                    }
                }
                any += u32::from(hit);
                if day % every == 0 || day == days {
                    let mut line = format!("seed {seed} day {day:4}:");
                    for (l, name) in short.iter().enumerate() {
                        let (c, cap) = w.cap_usage(l as u32);
                        line += &format!(" {name} {c}/{cap}");
                    }
                    log += &line;
                    log.push('\n');
                }
            }
            let mut line = format!("seed {seed} capped days {any}:");
            for l in 0..n {
                if capped[l] > 0 {
                    line += &format!(" {} {} (from day {})", short[l], capped[l], first[l].unwrap());
                }
            }
            log += &line;
            log.push('\n');
            (log, capped, any)
        })
        .collect();
    let mut total = 0;
    for (log, _, any) in &results {
        print!("{log}");
        total += any;
    }
    println!(
        "mean capped days {:.1} of {days} (sweep penalty {:.2})",
        total as f64 / results.len() as f64,
        3.0 * total as f64 / results.len() as f64 / days as f64
    );
}
