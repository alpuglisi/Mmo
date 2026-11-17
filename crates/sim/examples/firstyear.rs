//! The founding year, lineage by lineage: `cargo run --release --example firstyear -- [days] [seeds..] [--set NAME=V] [--meat] [--niches] [--food]`.
//!
//! Runs the standard world (96×96; default 120 days on seeds 31–34, in
//! parallel) and prints per seed the sweep's score (`evo-sim sweep`'s
//! formula: survival, health and the cap penalty), the days any lineage
//! spent at 95% of its population cap and which lineages did, each
//! lineage's peak and final head count, and the predators' head counts on
//! days 15, 30 and 60. Then the means over the seeds. `--meat` turns the
//! carnivore-diet rules and predator brakes on (`Balance::carnivore_diet`),
//! `--niches` founds `presets::niche_world` (`--niche-fox`, `--niche-pack`:
//! only that predator in its niche), `--food` turns the food values
//! on (`Balance::food_values`), `--off` turns the rules of both off
//! (`Balance::carnivore_diet_off`, `food_values_off`).
//! Read only: the first-year prey boom of *Food by its value* (README,
//! Balance).

use evo_sim::{presets, Balance, WorldConfig};
use rayon::prelude::*;

struct SeedResult {
    seed: u64,
    score: f64,
    capped_days: u32,
    capped_by: Vec<u32>,
    peak: Vec<usize>,
    alive: Vec<usize>,
    pred: [[usize; 3]; 2],
}

fn main() {
    let mut balance = Balance::default();
    let mut pos: Vec<String> = Vec::new();
    let (mut niche_fox, mut niche_pack) = (false, false);
    let mut sets = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--set" => sets.push(args.next().expect("--set NAME=V")),
            "--meat" => balance = balance.carnivore_diet(),
            "--niches" => (niche_fox, niche_pack) = (true, true),
            "--niche-fox" => niche_fox = true,
            "--niche-pack" => niche_pack = true,
            "--food" => balance = balance.food_values(),
            "--off" => {
                balance = balance.carnivore_diet_off().food_values_off();
                (niche_fox, niche_pack) = (false, false);
            }
            _ => pos.push(a),
        }
    }
    for s in sets {
        let (n, v) = s.split_once('=').expect("--set NAME=V");
        assert!(balance.set(n, v.parse().unwrap()), "unknown parameter {n}");
    }
    let days: u32 = pos.first().and_then(|s| s.parse().ok()).unwrap_or(120);
    let seeds: Vec<u64> =
        if pos.len() > 1 { pos[1..].iter().map(|s| s.parse().unwrap()).collect() } else { (31..=34).collect() };
    let results: Vec<SeedResult> = seeds
        .par_iter()
        .map(|&seed| {
            let config = WorldConfig { seed, width: 96, height: 96, balance, ..Default::default() };
            let fox = if niche_fox { presets::niche_wild_designs() } else { presets::wild_designs() };
            let pack = if niche_pack { presets::niche_player_designs() } else { presets::player_designs() };
            let mut w = presets::standard_world_from(config, 1, fox, pack);
            let n = w.lineages.len();
            let founders: Vec<u32> = w.lineages.iter().map(|l| l.founders).collect();
            let (mut peak, mut capped_by, mut capped_days) = (vec![0; n], vec![0; n], 0);
            let mut extinct = vec![None; n];
            let mut pred = [[0; 3]; 2];
            for day in 1..=days {
                w.run_days(1);
                let counts = w.lineage_counts();
                let mut capped = false;
                for l in 0..n {
                    peak[l] = peak[l].max(counts[l].alive);
                    if counts[l].alive == 0 && extinct[l].is_none() {
                        extinct[l] = Some(day);
                    }
                    let (c, cap) = w.cap_usage(l as u32);
                    if c as f32 >= 0.95 * cap as f32 {
                        capped_by[l] += 1;
                        capped = true;
                    }
                }
                capped_days += u32::from(capped);
                for (k, d) in [15, 30, 60].into_iter().enumerate() {
                    if day == d {
                        pred[0][k] = counts[3].alive;
                        pred[1][k] = counts[4].alive;
                    }
                }
            }
            let counts = w.lineage_counts();
            let mut score = 0.0;
            for l in 0..n.min(8) {
                if counts[l].adults >= 2 && counts[l].alive >= 4 {
                    score += 1.0;
                } else if let Some(d) = extinct[l] {
                    score += 0.3 * d as f64 / days as f64;
                }
                let health = ((1 + counts[l].alive) as f64).ln() / ((1 + founders[l]) as f64).ln();
                score += 0.5 * health.min(1.0);
            }
            score -= 3.0 * capped_days as f64 / days as f64;
            SeedResult {
                seed,
                score,
                capped_days,
                capped_by,
                peak,
                alive: counts.iter().map(|c| c.alive).collect(),
                pred,
            }
        })
        .collect();
    let names = ["vole", "ante", "newt", "fox", "pack", "herd", "frog", "omni"];
    for r in &results {
        let mut line = format!("seed {} score {:.2} capped {:3} days", r.seed, r.score, r.capped_days);
        for (l, &d) in r.capped_by.iter().enumerate() {
            if d > 0 {
                line += &format!(" {}={}", names.get(l).unwrap_or(&"?"), d);
            }
        }
        line += " | peak/end";
        for (l, name) in names.iter().enumerate().take(r.alive.len()) {
            line += &format!(" {} {}/{}", name, r.peak[l], r.alive[l]);
        }
        line += &format!(" | fox d15/30/60 {:?} pack {:?}", r.pred[0], r.pred[1]);
        println!("{line}");
    }
    let k = results.len() as f64;
    let mean = |f: &dyn Fn(&SeedResult) -> f64| results.iter().map(f).sum::<f64>() / k;
    let mut line = format!(
        "mean score {:.2} capped {:.1} days | fox end {:.0} pack end {:.0} | peak",
        mean(&|r| r.score),
        mean(&|r| r.capped_days as f64),
        mean(&|r| r.alive[3] as f64),
        mean(&|r| r.alive[4] as f64)
    );
    for (l, name) in names.iter().enumerate() {
        line += &format!(" {} {:.0}", name, mean(&|r| r.peak[l] as f64));
    }
    println!("{line}");
}
