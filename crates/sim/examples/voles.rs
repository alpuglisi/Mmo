//! Why a prey lineage collapses: `cargo run --release --example voles -- [years] [seeds..] [--prey L] [--every DAYS] [--design SPEC] [--set NAME=V]`.
//!
//! Runs the standard world (96×96) on each seed (in parallel; default 3
//! years on seed 32) and prints for prey lineage `--prey` (default 0, the
//! meadow voles), every `--every` days (default 30): the season, head count
//! and adults, births, deaths by cause and predation by killer lineage since
//! the last line, and the mean hunger and fat of the living. Read only
//! (*Voles and burrowing omnivores* in the README).

use evo_sim::organism::DeathCause;
use evo_sim::{presets, Balance, WorldConfig};
use rayon::prelude::*;

fn main() {
    let mut balance = Balance::default();
    let mut designs = presets::DesignTweaks::default();
    let mut prey = 0u32;
    let mut every = 30u32;
    let mut pos: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--prey" => prey = args.next().expect("--prey L").parse().unwrap(),
            "--every" => every = args.next().expect("--every DAYS").parse().unwrap(),
            "--design" => {
                let t = presets::DesignTweaks::parse(&args.next().expect("--design SPEC")).expect("design spec");
                designs.genes.extend(t.genes);
                designs.founders.extend(t.founders);
            }
            "--set" => {
                let s = args.next().expect("--set NAME=V");
                let (n, v) = s.split_once('=').expect("--set NAME=V");
                assert!(balance.set(n, v.parse().unwrap()), "unknown parameter {n}");
            }
            _ => pos.push(a),
        }
    }
    let years: u32 = pos.first().and_then(|s| s.parse().ok()).unwrap_or(3);
    let seeds: Vec<u64> = if pos.len() > 1 { pos[1..].iter().map(|s| s.parse().unwrap()).collect() } else { vec![32] };

    let logs: Vec<String> = seeds
        .par_iter()
        .map(|&seed| {
            let config = WorldConfig { seed, width: 96, height: 96, balance, ..Default::default() };
            let mut w = presets::standard_world_tweaked(config, 1, &designs);
            let names: Vec<String> = w.lineages.iter().map(|l| l.name.chars().take(6).collect()).collect();
            let days = years * w.config.days_per_year;
            let mut log = String::new();
            let (mut last_deaths, mut last_kills) =
                (w.counters.deaths_by_lineage.clone(), w.counters.kills_by_lineage.clone());
            let mut last_born = 0u64;
            for day in 1..=days {
                w.run_days(1);
                if day % every != 0 && day != days {
                    continue;
                }
                let c = w.lineage_counts()[prey as usize];
                let born = w.counters.born.get(&prey).copied().unwrap_or(0);
                let living: Vec<_> = w.organisms.iter().filter(|o| o.alive && o.lineage == prey).collect();
                let n = living.len().max(1) as f32;
                let hunger = living.iter().map(|o| o.needs.hunger).sum::<f32>() / n;
                let fat = living.iter().map(|o| o.fat).sum::<f32>() / n;
                let mut line = format!(
                    "seed {seed} day {day:4} {:?}: {} alive ({} adults), +{} born, hunger {hunger:.0}, fat {fat:.0} |",
                    w.season(),
                    c.alive,
                    c.adults,
                    born - last_born
                );
                for cause in DeathCause::ALL {
                    let k = (prey, cause);
                    let d = w.counters.deaths_by_lineage.get(&k).copied().unwrap_or(0)
                        - last_deaths.get(&k).copied().unwrap_or(0);
                    if d > 0 {
                        line += &format!(" {cause:?}={d}");
                    }
                }
                line += " | killed by";
                for (l, name) in names.iter().enumerate() {
                    let k = (prey, l as u32);
                    let d = w.counters.kills_by_lineage.get(&k).copied().unwrap_or(0)
                        - last_kills.get(&k).copied().unwrap_or(0);
                    if d > 0 {
                        line += &format!(" {name}={d}");
                    }
                }
                log += &line;
                log.push('\n');
                last_deaths = w.counters.deaths_by_lineage.clone();
                last_kills = w.counters.kills_by_lineage.clone();
                last_born = born;
                if c.alive == 0 {
                    break;
                }
            }
            log
        })
        .collect();
    for l in logs {
        print!("{l}");
    }
}
