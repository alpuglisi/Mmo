//! `evo-sim rankcheck`: how well do short screens predict full-run ranks?
//!
//! The sweep's successive-halving screens rank trials on a short run (say 40
//! days) and continue only the best. That is only worth it if the short-run
//! rank predicts the full-run rank. This runs a set of perturbed balances (one
//! sweep round, trial 0 the center) on the same seeds, records every world's
//! score at each checkpoint on the way to the full run (one world per trial
//! and seed, resumed from checkpoint to checkpoint, so it costs one full run),
//! and reports for each checkpoint the Spearman correlation with the full-run
//! ranking, how much of the true top `1/eta` a screen there would keep, and
//! the split-half reliability of the full-run ranking itself (the ceiling any
//! screen could reach with this many seeds).

use std::fmt::Write as _;
use std::fs::File;
use std::io::Write as _;
use std::time::Instant;

use evo_sim::Balance;
use rayon::prelude::*;

use crate::stats::{spearman, Summary};
use crate::sweep::{keep_count, perturb, ScoredRun};

pub struct RankCheckArgs {
    pub base_seed: u64,
    pub size: usize,
    pub days: u32,
    pub trials: usize,
    pub seeds: u64,
    pub spread: f32,
    pub center: Balance,
    /// Checkpoints (days) to compare with the full run.
    pub checkpoints: Vec<u32>,
    pub eta: f64,
    pub parallel_apply: bool,
    pub out: Option<String>,
}

/// Indices of the `k` largest values (ties broken by index).
fn top(values: &[f64], k: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[b].total_cmp(&values[a]).then(a.cmp(&b)));
    idx.truncate(k);
    idx
}

/// Mean over a subset of seeds for each trial: `scores[trial][seed]`.
fn means(scores: &[Vec<f64>], keep: impl Fn(usize) -> bool) -> Vec<f64> {
    scores
        .iter()
        .map(|row| {
            let v: Vec<f64> = row.iter().enumerate().filter(|(s, _)| keep(*s)).map(|(_, x)| *x).collect();
            v.iter().sum::<f64>() / v.len().max(1) as f64
        })
        .collect()
}

pub fn rankcheck(args: &RankCheckArgs) {
    let mut checkpoints: Vec<u32> = args.checkpoints.iter().copied().filter(|&d| d > 0 && d < args.days).collect();
    checkpoints.sort_unstable();
    checkpoints.dedup();
    checkpoints.push(args.days);
    println!(
        "Rank check: {} trials (spread ±{:.0}%, trial 0 the center) × {} seeds from {} · {}×{} map · \
         checkpoints {:?} · {} threads",
        args.trials,
        (args.spread.exp() - 1.0) * 100.0,
        args.seeds,
        args.base_seed,
        args.size,
        args.size,
        checkpoints,
        rayon::current_num_threads()
    );
    let started = Instant::now();
    let balances: Vec<Balance> =
        (0..args.trials).map(|k| perturb(&args.center, args.spread, args.base_seed, 0, k)).collect();
    let jobs: Vec<(usize, u64)> = (0..args.trials).flat_map(|k| (0..args.seeds).map(move |s| (k, s))).collect();
    // `results[job][checkpoint]` = score at that checkpoint.
    let results: Vec<Vec<f64>> = jobs
        .par_iter()
        .map(|&(k, s)| {
            let mut run = ScoredRun::new(balances[k], args.base_seed + s, args.size, args.parallel_apply, None);
            checkpoints
                .iter()
                .map(|&d| {
                    run.advance_to(d);
                    run.metrics(d).score()
                })
                .collect()
        })
        .collect();
    // `scores[checkpoint][trial][seed]`.
    let seeds = args.seeds as usize;
    let scores: Vec<Vec<Vec<f64>>> = (0..checkpoints.len())
        .map(|c| (0..args.trials).map(|k| (0..seeds).map(|s| results[k * seeds + s][c]).collect()).collect())
        .collect();
    let full = scores.last().expect("the full run");
    let full_means = means(full, |_| true);
    let keep = keep_count(args.trials, args.eta);
    let true_top = top(&full_means, keep);
    let best = true_top[0];
    // Rank (1 = best) of each trial in the full run.
    let full_order = top(&full_means, args.trials);
    let full_rank = |k: usize| full_order.iter().position(|&t| t == k).expect("ranked") + 1;
    let full_worlds: Vec<f64> = full.iter().flatten().copied().collect();

    println!("\n  Full run ({} days): trial means {}", args.days, describe(&full_means));
    println!(
        "\n  {:>5} {:>13} {:>10} {:>14} {:>12} {:>16} {:>13}",
        "day", "trial means", "ρ trials", "ρ worlds", "top kept", "full best kept", "screen best"
    );
    for (c, &day) in checkpoints.iter().enumerate().take(checkpoints.len() - 1) {
        let m = means(&scores[c], |_| true);
        let rho = spearman(&m, &full_means);
        let worlds: Vec<f64> = scores[c].iter().flatten().copied().collect();
        let rho_w = spearman(&worlds, &full_worlds);
        let kept = top(&m, keep);
        let overlap = true_top.iter().filter(|k| kept.contains(k)).count();
        println!(
            "  {:>5} {:>13} {:>10.2} {:>14.2} {:>12} {:>16} {:>13}",
            day,
            format!("{:.2}–{:.2}", min(&m), max(&m)),
            rho,
            rho_w,
            format!("{overlap}/{keep}"),
            if kept.contains(&best) { "yes" } else { "no" },
            format!("#{} at {}", full_rank(kept[0]), args.days)
        );
    }
    println!(
        "  (ρ trials: Spearman of trial means against the {}-day means; ρ worlds: of single worlds;\n   \
         top kept: how many of the {}-day top {keep} a screen keeping ceil(n/{}) there would keep)",
        args.days, args.days, args.eta
    );
    if seeds >= 2 {
        let even = means(full, |s| s % 2 == 0);
        let odd = means(full, |s| s % 2 == 1);
        let r = spearman(&even, &odd);
        let brown = 2.0 * r / (1.0 + r);
        println!(
            "\n  Split-half reliability of the {}-day ranking: ρ = {r:.2} between even and odd seeds \
             ({} vs {} seeds), ≈ {brown:.2} for all {seeds} (Spearman-Brown)",
            args.days,
            seeds.div_ceil(2),
            seeds / 2
        );
        let sds: Vec<f64> = full.iter().map(|row| Summary::of(row).sd).collect();
        let within = (sds.iter().map(|s| s * s).sum::<f64>() / sds.len() as f64).sqrt();
        println!(
            "  Spread of trial means (SD) {:.2} vs seed-to-seed SD within a trial {:.2} (SE of a mean {:.2})",
            Summary::of(&full_means).sd,
            within,
            within / (seeds as f64).sqrt()
        );
    }
    println!("\n  {:.0}s", started.elapsed().as_secs_f64());

    if let Some(path) = &args.out {
        let mut csv = String::from("trial,seed,day,score\n");
        for (c, day) in checkpoints.iter().enumerate() {
            for (k, row) in scores[c].iter().enumerate() {
                for (s, x) in row.iter().enumerate() {
                    let _ = writeln!(csv, "{k},{},{day},{x:.4}", args.base_seed + s as u64);
                }
            }
        }
        match File::create(path).and_then(|mut f| f.write_all(csv.as_bytes())) {
            Ok(()) => println!("Wrote {path}"),
            Err(e) => eprintln!("could not write {path}: {e}"),
        }
    }
}

fn min(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::INFINITY, f64::min)
}

fn max(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

fn describe(v: &[f64]) -> String {
    let s = Summary::of(v);
    format!("{:.2}–{:.2} (mean {:.2}, SD {:.2})", min(v), max(v), s.mean, s.sd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_breaks_ties_by_index() {
        assert_eq!(top(&[1.0, 3.0, 3.0, 2.0], 2), vec![1, 2]);
        assert_eq!(top(&[1.0, 3.0, 3.0, 2.0], 4), vec![1, 2, 3, 0]);
        assert_eq!(means(&[vec![1.0, 2.0, 3.0, 4.0]], |s| s % 2 == 0), vec![2.0]);
    }
}
