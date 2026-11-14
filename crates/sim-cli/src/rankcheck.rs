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
//!
//! With 4 or more seeds it also splits the seeds into random halves to
//! separate the two reasons a screen disagrees with the full run: seed noise
//! (in the screen and in the full-run reference) and screen error proper (the
//! short run measuring something else). A screen ranked on one half against
//! the full run on the other half shares no seed noise; dividing by both
//! split-half reliabilities gives the screen's agreement with the full run
//! once both are freed of noise. Finally it simulates whole screened sweeps on
//! these trials (worlds resampled per trial around shrunken trial means) to
//! estimate how often a screen keeps the true best, how often the final pick
//! is the true best, and the points lost, against unscreened sweeps.
//! `--from` re-analyzes a CSV written by `--out` without simulating.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Write as _;
use std::time::Instant;

use evo_sim::rng::rng_for;
use evo_sim::Balance;
use rand::seq::SliceRandom;
use rand::Rng;
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
    /// Re-analyze a CSV written by `--out` instead of simulating.
    pub from: Option<String>,
}

/// Every world's score: `scores[checkpoint][trial][seed]`, the last
/// checkpoint the full run.
struct Data {
    checkpoints: Vec<u32>,
    base_seed: u64,
    scores: Vec<Vec<Vec<f64>>>,
}

impl Data {
    fn trials(&self) -> usize {
        self.scores[0].len()
    }

    fn seeds(&self) -> usize {
        self.scores[0][0].len()
    }

    fn days(&self) -> u32 {
        *self.checkpoints.last().expect("the full run")
    }

    fn to_csv(&self) -> String {
        let mut csv = String::from("trial,seed,day,score\n");
        for (c, day) in self.checkpoints.iter().enumerate() {
            for (k, row) in self.scores[c].iter().enumerate() {
                for (s, x) in row.iter().enumerate() {
                    let _ = writeln!(csv, "{k},{},{day},{x:.4}", self.base_seed + s as u64);
                }
            }
        }
        csv
    }

    /// Parses `to_csv`'s format (every trial × seed × day present).
    fn from_csv(text: &str) -> Result<Data, String> {
        let mut rows = Vec::new();
        for (i, line) in text.lines().enumerate().skip(1) {
            if line.trim().is_empty() {
                continue;
            }
            let bad = || format!("line {}: expected trial,seed,day,score, got '{line}'", i + 1);
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            if f.len() != 4 {
                return Err(bad());
            }
            let trial: usize = f[0].parse().map_err(|_| bad())?;
            let seed: u64 = f[1].parse().map_err(|_| bad())?;
            let day: u32 = f[2].parse().map_err(|_| bad())?;
            let score: f64 = f[3].parse().map_err(|_| bad())?;
            rows.push((trial, seed, day, score));
        }
        let trials = rows.iter().map(|r| r.0).max().ok_or("no rows")? + 1;
        let mut seeds: Vec<u64> = rows.iter().map(|r| r.1).collect();
        seeds.sort_unstable();
        seeds.dedup();
        let mut days: Vec<u32> = rows.iter().map(|r| r.2).collect();
        days.sort_unstable();
        days.dedup();
        let mut scores = vec![vec![vec![f64::NAN; seeds.len()]; trials]; days.len()];
        for (k, seed, day, x) in rows {
            let c = days.binary_search(&day).expect("listed");
            let s = seeds.binary_search(&seed).expect("listed");
            scores[c][k][s] = x;
        }
        if scores.iter().flatten().flatten().any(|x| x.is_nan()) {
            return Err(format!(
                "incomplete: expected every trial × seed × day ({trials} × {} × {})",
                seeds.len(),
                days.len()
            ));
        }
        Ok(Data { checkpoints: days, base_seed: seeds[0], scores })
    }
}

/// Indices of the `k` largest values (ties broken by index).
fn top(values: &[f64], k: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[b].total_cmp(&values[a]).then(a.cmp(&b)));
    idx.truncate(k);
    idx
}

/// Indices of the `k` largest values, ties broken at random (as if the
/// trials' order were shuffled before an index tie-break).
fn top_shuffled(values: &[f64], k: usize, rng: &mut impl Rng) -> Vec<usize> {
    let tiebreak: Vec<u64> = values.iter().map(|_| rng.gen()).collect();
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[b].total_cmp(&values[a]).then(tiebreak[a].cmp(&tiebreak[b])));
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

pub fn rankcheck(args: &RankCheckArgs) -> Result<(), String> {
    let started = Instant::now();
    let data = match &args.from {
        Some(path) => {
            let data = fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|text| Data::from_csv(&text))
                .map_err(|e| format!("could not read {path}: {e}"))?;
            println!(
                "Rank check from {path}: {} trials × {} seeds from {} · checkpoints {:?}",
                data.trials(),
                data.seeds(),
                data.base_seed,
                data.checkpoints
            );
            data
        }
        None => simulate(args),
    };
    report(&data, args.eta);
    println!("\n  {:.0}s", started.elapsed().as_secs_f64());

    if let Some(path) = &args.out {
        match File::create(path).and_then(|mut f| f.write_all(data.to_csv().as_bytes())) {
            Ok(()) => println!("Wrote {path}"),
            Err(e) => eprintln!("could not write {path}: {e}"),
        }
    }
    Ok(())
}

fn simulate(args: &RankCheckArgs) -> Data {
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
    let seeds = args.seeds as usize;
    let scores: Vec<Vec<Vec<f64>>> = (0..checkpoints.len())
        .map(|c| (0..args.trials).map(|k| (0..seeds).map(|s| results[k * seeds + s][c]).collect()).collect())
        .collect();
    Data { checkpoints, base_seed: args.base_seed, scores }
}

fn report(data: &Data, eta: f64) {
    let (checkpoints, trials, seeds, days) = (&data.checkpoints, data.trials(), data.seeds(), data.days());
    let full = data.scores.last().expect("the full run");
    let full_means = means(full, |_| true);
    let keep = keep_count(trials, eta);
    let true_top = top(&full_means, keep);
    let best = true_top[0];
    // Rank (1 = best) of each trial in the full run.
    let full_order = top(&full_means, trials);
    let full_rank = |k: usize| full_order.iter().position(|&t| t == k).expect("ranked") + 1;
    let full_worlds: Vec<f64> = full.iter().flatten().copied().collect();

    println!("\n  Full run ({days} days): trial means {}", describe(&full_means));
    println!(
        "\n  {:>5} {:>13} {:>10} {:>14} {:>12} {:>16} {:>13}",
        "day", "trial means", "ρ trials", "ρ worlds", "top kept", "full best kept", "screen best"
    );
    for (c, &day) in checkpoints.iter().enumerate().take(checkpoints.len() - 1) {
        let m = means(&data.scores[c], |_| true);
        let rho = spearman(&m, &full_means);
        let worlds: Vec<f64> = data.scores[c].iter().flatten().copied().collect();
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
            format!("#{} at {days}", full_rank(kept[0]))
        );
    }
    println!(
        "  (ρ trials: Spearman of trial means against the {days}-day means; ρ worlds: of single worlds;\n   \
         top kept: how many of the {days}-day top {keep} a screen keeping ceil(n/{eta}) there would keep)"
    );
    if seeds >= 2 {
        let even = means(full, |s| s % 2 == 0);
        let odd = means(full, |s| s % 2 == 1);
        let r = spearman(&even, &odd);
        let brown = 2.0 * r / (1.0 + r);
        println!(
            "\n  Split-half reliability of the {days}-day ranking: ρ = {r:.2} between even and odd seeds \
             ({} vs {} seeds), ≈ {brown:.2} for all {seeds} (Spearman-Brown)",
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
    if seeds >= 4 && trials >= 4 && checkpoints.len() >= 2 {
        seed_splits(data, eta);
        screened_sweeps(data, eta);
    }
}

// ---------------------------------------------------------------------------
// Seed splits: noise vs screen error.

const SPLITS: usize = 200;
const BOOTSTRAP: usize = 300;
const BOOTSTRAP_SPLITS: usize = 25;
const SPLIT_STREAM: u64 = 0x5b17;
const BOOT_STREAM: u64 = 0xb007;
const TIE_STREAM: u64 = 0x71e5;

/// `count` random halves of `n` seeds (true = half A, which has `n / 2`).
fn half_splits(n: usize, count: usize, a: u64, b: u64) -> Vec<Vec<bool>> {
    let mut rng = rng_for(a, SPLIT_STREAM, b, 0);
    (0..count)
        .map(|_| {
            let mut idx: Vec<usize> = (0..n).collect();
            idx.shuffle(&mut rng);
            let mut half = vec![false; n];
            for &s in &idx[..n / 2] {
                half[s] = true;
            }
            half
        })
        .collect()
}

/// Split-averaged statistics over a set of trials (indices, may repeat).
struct SplitStats {
    /// Full run on one half against the full run on the other.
    full: f64,
    /// Per screen checkpoint: screen half against screen half.
    screen: Vec<f64>,
    /// Per screen checkpoint: screen on one half against the full run on the
    /// other (no shared seeds).
    cross: Vec<f64>,
    /// Per checkpoint (the full run last): how often a screen there on one
    /// half keeps the trial that is best on the other half's full run (ties
    /// at the cut broken at random).
    best_kept: Vec<f64>,
}

fn nan_mean(v: &[f64]) -> f64 {
    let ok: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if ok.is_empty() {
        f64::NAN
    } else {
        ok.iter().sum::<f64>() / ok.len() as f64
    }
}

fn split_stats(
    scores: &[Vec<Vec<f64>>],
    trials: &[usize],
    splits: &[Vec<bool>],
    keep: usize,
    tie_seed: u64,
) -> SplitStats {
    let last = scores.len() - 1;
    let mut rng = rng_for(tie_seed, TIE_STREAM, 0, 0);
    let half = |c: usize, split: &[bool], side: bool| -> Vec<f64> {
        trials
            .iter()
            .map(|&k| {
                let v: Vec<f64> = (0..split.len()).filter(|&s| split[s] == side).map(|s| scores[c][k][s]).collect();
                v.iter().sum::<f64>() / v.len() as f64
            })
            .collect()
    };
    let mut full = Vec::new();
    let mut screen = vec![Vec::new(); last];
    let mut cross = vec![Vec::new(); last];
    let mut best_kept = vec![Vec::new(); last + 1];
    for split in splits {
        let (fa, fb) = (half(last, split, true), half(last, split, false));
        full.push(spearman(&fa, &fb));
        let (best_a, best_b) = (top(&fa, 1)[0], top(&fb, 1)[0]);
        for c in 0..=last {
            let (sa, sb) = (half(c, split, true), half(c, split, false));
            let mut kept =
                |s: &[f64], best: usize| if top_shuffled(s, keep, &mut rng).contains(&best) { 1.0 } else { 0.0 };
            let k = (kept(&sa, best_b) + kept(&sb, best_a)) / 2.0;
            best_kept[c].push(k);
            if c < last {
                screen[c].push(spearman(&sa, &sb));
                cross[c].push((spearman(&sa, &fb) + spearman(&sb, &fa)) / 2.0);
            }
        }
    }
    SplitStats {
        full: nan_mean(&full),
        screen: screen.iter().map(|v| nan_mean(v)).collect(),
        cross: cross.iter().map(|v| nan_mean(v)).collect(),
        best_kept: best_kept.iter().map(|v| nan_mean(v)).collect(),
    }
}

/// Spearman-Brown: the reliability of a mean over `factor` times as many
/// seeds as the ones with reliability `r`.
fn stepped(r: f64, factor: f64) -> f64 {
    factor * r / (1.0 + (factor - 1.0) * r)
}

/// Seeds needed for reliability `target`, given reliability `r` on `h` seeds.
fn seeds_for(r: f64, h: f64, target: f64) -> f64 {
    if r <= 0.0 {
        f64::INFINITY
    } else {
        h * target * (1.0 - r) / (r * (1.0 - target))
    }
}

/// The screen's rank agreement with the full run with seed noise removed
/// from both (the correction for attenuation).
fn true_score(cross: f64, screen: f64, full: f64) -> f64 {
    if screen > 0.0 && full > 0.0 {
        cross / (screen * full).sqrt()
    } else {
        f64::NAN
    }
}

/// 2.5% and 97.5% percentiles of the finite values.
fn interval(values: impl Iterator<Item = f64>) -> (f64, f64) {
    let mut v: Vec<f64> = values.filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    (at(0.025), at(0.975))
}

fn with_ci(x: f64, (lo, hi): (f64, f64)) -> String {
    format!("{x:.2} ({lo:.2}…{hi:.2})")
}

fn seed_splits(data: &Data, eta: f64) {
    let (trials, seeds, days) = (data.trials(), data.seeds(), data.days());
    let screens = &data.checkpoints[..data.checkpoints.len() - 1];
    let keep = keep_count(trials, eta);
    let all: Vec<usize> = (0..trials).collect();
    let point =
        split_stats(&data.scores, &all, &half_splits(seeds, SPLITS, data.base_seed, u64::MAX), keep, data.base_seed);
    let boots: Vec<SplitStats> = (0..BOOTSTRAP as u64)
        .into_par_iter()
        .map(|b| {
            let mut rng = rng_for(data.base_seed, BOOT_STREAM, b, 0);
            let sample: Vec<usize> = (0..trials).map(|_| rng.gen_range(0..trials)).collect();
            split_stats(&data.scores, &sample, &half_splits(seeds, BOOTSTRAP_SPLITS, data.base_seed, b), keep, b)
        })
        .collect();
    let h = seeds as f64 / 2.0;
    let rel = |r: f64, m: f64| stepped(r, m / h);
    let full_ci = interval(boots.iter().map(|b| b.full));
    let rel_ci = interval(boots.iter().map(|b| rel(b.full, seeds as f64)));
    println!(
        "\n  Seed splits: {SPLITS} random halves ({} vs {} seeds); 95% intervals bootstrap the {trials} trials \
         ({BOOTSTRAP} resamples)",
        seeds / 2,
        seeds - seeds / 2
    );
    println!(
        "  {days}-day ranking: split-half ρ {}; reliability of the {seeds}-seed ranking {}, of a 4-seed one {:.2};\n  \
         reliability 0.8 needs ≈ {:.0} seeds, 0.9 ≈ {:.0}",
        with_ci(point.full, full_ci),
        with_ci(rel(point.full, seeds as f64), rel_ci),
        rel(point.full, 4.0),
        seeds_for(point.full, h, 0.8).ceil(),
        seeds_for(point.full, h, 0.9).ceil()
    );
    println!(
        "\n  {:>5} {:>18} {:>18} {:>18} {:>19} {:>10}",
        "day", "ρ screen halves", "ρ screen↔full", "true-score ρ", "ρ vs truth 4|12 s.", "best kept"
    );
    for (c, &day) in screens.iter().enumerate() {
        let truth = true_score(point.cross[c], point.screen[c], point.full);
        let vs = |m: f64| truth.min(1.0) * rel(point.screen[c], m).max(0.0).sqrt();
        println!(
            "  {:>5} {:>18} {:>18} {:>18} {:>19} {:>10}",
            day,
            with_ci(point.screen[c], interval(boots.iter().map(|b| b.screen[c]))),
            with_ci(point.cross[c], interval(boots.iter().map(|b| b.cross[c]))),
            with_ci(truth, interval(boots.iter().map(|b| true_score(b.cross[c], b.screen[c], b.full)))),
            format!("{:.2} | {:.2}", vs(4.0), vs(12.0)),
            format!("{:.0}%", 100.0 * point.best_kept[c])
        );
    }
    let vs_full = |m: f64| rel(point.full, m).max(0.0).sqrt();
    println!(
        "  {:>5} {:>18} {:>18} {:>18} {:>19} {:>10}",
        days,
        with_ci(point.full, full_ci),
        "",
        "1",
        format!("{:.2} | {:.2}", vs_full(4.0), vs_full(12.0)),
        format!("{:.0}%", 100.0 * point.best_kept[screens.len()])
    );
    println!(
        "  (ρ screen halves: the ranking on one half of the seeds against the other half; ρ screen↔full: the\n   \
         screen on one half against the {days}-day run on the other, so no seed noise is shared; true-score ρ:\n   \
         that divided by √(both split-half ρ), the agreement left once both rankings are freed of seed noise\n   \
         (1: the screen ranks like the full run and only noise separates them); ρ vs truth: estimated agreement\n   \
         of a 4- or 12-seed ranking with the infinite-seed {days}-day ranking; best kept: how often a screen on\n   \
         one half keeping ceil(n/{eta}) keeps the best trial of the other half's {days}-day run; on the\n   \
         {days}-day row, how often the full run itself on one half does)"
    );
}

// ---------------------------------------------------------------------------
// Simulated screened sweeps.

const REPLICATES: usize = 4000;
const SWEEP_STREAM: u64 = 0x5ee9;

/// Trial means shrunk toward the grand mean by their reliability (the
/// empirical-Bayes estimate of each trial's true mean): `rows[trial][seed]`.
fn shrunk(rows: &[Vec<f64>]) -> Vec<f64> {
    let n = rows[0].len() as f64;
    let m: Vec<f64> = rows.iter().map(|r| r.iter().sum::<f64>() / n).collect();
    let var: Vec<f64> = rows.iter().map(|r| Summary::of(r).sd.powi(2)).collect();
    let grand = m.iter().sum::<f64>() / m.len() as f64;
    let tau2 = (Summary::of(&m).sd.powi(2) - var.iter().sum::<f64>() / var.len() as f64 / n).max(0.0);
    m.iter()
        .zip(&var)
        .map(|(x, v)| {
            let noise = v / n;
            let weight = if tau2 + noise > 0.0 { tau2 / (tau2 + noise) } else { 1.0 };
            grand + weight * (x - grand)
        })
        .collect()
}

/// A trial-level model of the data: true means per checkpoint and each
/// world's deviation from its trial's mean (kept together across checkpoints,
/// as one world is resumed).
struct Model {
    /// `truth[checkpoint][trial]`.
    truth: Vec<Vec<f64>>,
    /// `resid[trial][seed][checkpoint]`, scaled by √(n/(n−1)).
    resid: Vec<Vec<Vec<f64>>>,
}

impl Model {
    fn of(scores: &[Vec<Vec<f64>>]) -> Model {
        let truth = scores.iter().map(|c| shrunk(c)).collect();
        let (trials, seeds) = (scores[0].len(), scores[0][0].len());
        let scale = (seeds as f64 / (seeds as f64 - 1.0)).sqrt();
        let resid = (0..trials)
            .map(|k| {
                (0..seeds)
                    .map(|s| {
                        scores.iter().map(|c| (c[k][s] - c[k].iter().sum::<f64>() / seeds as f64) * scale).collect()
                    })
                    .collect()
            })
            .collect();
        Model { truth, resid }
    }
}

/// Outcome of simulated sweeps.
#[derive(Default)]
struct Outcome {
    /// P(the screen keeps the true best) at eta 2, 3 and 4.
    kept: [f64; 3],
    /// P(the final pick is the true best).
    pick: f64,
    /// Mean true points lost by the final pick against the true best.
    regret: f64,
}

/// Simulated sweeps of the model's trials, `seeds` finalist seeds: with a
/// screen `(checkpoint, screen seeds)` keeping `keep` trials, or none.
fn simulate_sweeps(model: &Model, screen: Option<(usize, usize)>, seeds: usize, keep: usize, stream: u64) -> Outcome {
    let last = model.truth.len() - 1;
    let trials = model.truth[0].len();
    let pool = model.resid[0].len();
    let best = top(&model.truth[last], 1)[0];
    let mut rng = rng_for(stream, SWEEP_STREAM, screen.map_or(0, |(c, m)| (c * 1000 + m) as u64 + 1), seeds as u64);
    let mut out = Outcome::default();
    for _ in 0..REPLICATES {
        let draws: Vec<Vec<usize>> =
            (0..trials).map(|_| (0..seeds).map(|_| rng.gen_range(0..pool)).collect()).collect();
        let mean = |c: usize, k: usize, m: usize| {
            draws[k][..m].iter().map(|&j| model.truth[c][k] + model.resid[k][j][c]).sum::<f64>() / m as f64
        };
        let finals: Vec<f64> = (0..trials).map(|k| mean(last, k, seeds)).collect();
        let pick = match screen {
            None => top_shuffled(&finals, 1, &mut rng)[0],
            Some((c, m)) => {
                let s: Vec<f64> = (0..trials).map(|k| mean(c, k, m)).collect();
                let ranked = top_shuffled(&s, trials, &mut rng);
                for (i, e) in [2.0, 3.0, 4.0].iter().enumerate() {
                    if ranked[..keep_count(trials, *e)].contains(&best) {
                        out.kept[i] += 1.0;
                    }
                }
                let finalists: Vec<f64> = (0..trials)
                    .map(|k| if ranked[..keep].contains(&k) { finals[k] } else { f64::NEG_INFINITY })
                    .collect();
                top_shuffled(&finalists, 1, &mut rng)[0]
            }
        };
        if pick == best {
            out.pick += 1.0;
        }
        out.regret += model.truth[last][best] - model.truth[last][pick];
    }
    let n = REPLICATES as f64;
    out.kept.iter_mut().for_each(|x| *x /= n);
    out.pick /= n;
    out.regret /= n;
    out
}

fn screened_sweeps(data: &Data, eta: f64) {
    let (trials, seeds, days) = (data.trials(), data.seeds(), data.days());
    let screens = &data.checkpoints[..data.checkpoints.len() - 1];
    let keep = keep_count(trials, eta);
    let model = Model::of(&data.scores);
    let truth = &model.truth[model.truth.len() - 1];
    let order = top(truth, trials);
    println!(
        "\n  Simulated sweeps ({REPLICATES} each): worlds resampled per trial around shrunken trial means (true SD of \
         trials {:.2};\n  true best trial {} leads the next by {:.2}); finalists get {seeds} seeds, screen worlds are \
         resumed, a screen keeps\n  ceil(n/{eta}) = {keep}; cost in world-days against an unscreened {seeds}-seed sweep",
        Summary::of(truth).sd,
        order[0],
        truth[order[0]] - truth[order[1]]
    );
    println!("\n  {:>16} {:>6} {:>22} {:>11} {:>8}", "screen", "cost", "best kept eta 2|3|4", "pick best", "regret");
    let mut unscreened: Vec<usize> = vec![seeds, seeds / 2, seeds / 3];
    unscreened.retain(|&s| s >= 1);
    unscreened.dedup();
    // One simulated design: label, screen `(checkpoint, seeds)`, finalist
    // seeds and cost.
    struct Design(String, Option<(usize, usize)>, usize, f64);
    let mut rows: Vec<Design> = unscreened
        .iter()
        .map(|&s| Design(format!("none, {s} seed{}", if s == 1 { "" } else { "s" }), None, s, s as f64 / seeds as f64))
        .collect();
    for (c, &day) in screens.iter().enumerate() {
        for m in [2, 4, 6].into_iter().filter(|&m| m <= seeds) {
            let total = (trials * seeds) as f64 * days as f64;
            let cost =
                (trials * m) as f64 * day as f64 + keep as f64 * (seeds as f64 * days as f64 - (m as u32 * day) as f64);
            rows.push(Design(format!("{day} d × {m} seeds"), Some((c, m)), seeds, cost / total));
        }
    }
    let outcomes: Vec<Outcome> = rows
        .par_iter()
        .map(|Design(_, screen, s, _)| {
            simulate_sweeps(&model, *screen, *s, if screen.is_some() { keep } else { trials }, data.base_seed)
        })
        .collect();
    for (Design(label, screen, _, cost), o) in rows.iter().zip(&outcomes) {
        let kept = if screen.is_some() {
            format!("{:.0}% | {:.0}% | {:.0}%", 100.0 * o.kept[0], 100.0 * o.kept[1], 100.0 * o.kept[2])
        } else {
            String::new()
        };
        println!(
            "  {:>16} {:>6} {:>22} {:>11} {:>8.2}",
            label,
            format!("{:.0}%", 100.0 * cost),
            kept,
            format!("{:.0}%", 100.0 * o.pick),
            o.regret
        );
    }
    println!(
        "  (best kept: chance the screen keeps the true best; pick best: chance the sweep's winner is the true best;\n   \
         regret: true score points the winner is below the true best, on average)"
    );
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

    #[test]
    fn csv_round_trips() {
        let data = Data {
            checkpoints: vec![40, 120],
            base_seed: 5,
            scores: vec![vec![vec![1.0, 2.0], vec![3.0, 4.0]], vec![vec![5.0, 6.0], vec![7.0, 8.5]]],
        };
        let back = Data::from_csv(&data.to_csv()).expect("parses");
        assert_eq!(back.checkpoints, data.checkpoints);
        assert_eq!(back.base_seed, 5);
        assert_eq!(back.scores, data.scores);
        assert!(Data::from_csv("trial,seed,day,score\n0,1,40,1.0\n1,1,40,2.0\n0,1,120,3.0\n").is_err());
    }

    #[test]
    fn split_half_helpers() {
        assert!((stepped(0.5, 2.0) - 2.0 / 3.0).abs() < 1e-12);
        // Reliability 0.5 on 6 seeds reaches 0.8 at 24 seeds.
        assert!((seeds_for(0.5, 6.0, 0.8) - 24.0).abs() < 1e-9);
        assert!((stepped(0.5, 24.0 / 6.0) - 0.8).abs() < 1e-12);
        assert!((true_score(0.3, 0.5, 0.5) - 0.6).abs() < 1e-12);
        let halves = half_splits(12, 10, 1, 2);
        assert!(halves.iter().all(|h| h.iter().filter(|&&a| a).count() == 6));
        assert_ne!(halves[0], halves[1]);
    }

    #[test]
    fn noiseless_trials_are_always_found() {
        // Distinct trials and no seed noise: every screen keeps the best and
        // every sweep picks it.
        let rows: Vec<Vec<f64>> = (0..6).map(|k| vec![k as f64; 4]).collect();
        let model = Model::of(&[rows.clone(), rows]);
        assert_eq!(model.truth[1], vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        let o = simulate_sweeps(&model, Some((0, 2)), 4, 2, 1);
        assert_eq!((o.kept, o.pick, o.regret), ([1.0; 3], 1.0, 0.0));
    }
}
