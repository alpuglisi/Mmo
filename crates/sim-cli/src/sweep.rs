//! Balance sweep: random search over `Balance` parameters, scored on whether
//! every lineage (wild species and sample players) coexists for a year.
//!
//! Each round perturbs every parameter of the current best by a random factor
//! in `[e^-spread, e^spread]`, evaluates every trial on several seeds in
//! parallel, and recenters on the best trial. The spread halves each round.
//!
//! Single worlds are chaotic, so every score comes with its standard error and
//! a 95% confidence interval over seeds, and every trial is also compared with
//! the round's center on the same seeds (a paired difference, which cancels
//! most of the map-to-map variation). With `--screen-days` a round is a
//! successive-halving tournament: all trials run a short screen, only the best
//! `1/eta` (plus the center) go on, and those worlds are resumed rather than
//! rerun, so a promotion costs only the extra days. `compare` runs two fixed
//! configurations on the same seeds and reports their paired difference.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Write as _;
use std::time::Instant;

use evo_sim::rng::rng_for;
use evo_sim::{presets, Balance, World, WorldConfig};
use rand::Rng;
use rayon::prelude::*;

use crate::stats::{wilson, Summary};

pub struct SweepArgs {
    pub base_seed: u64,
    pub size: usize,
    pub days: u32,
    pub trials: usize,
    pub seeds: u64,
    pub rounds: u32,
    pub spread: f32,
    pub center: Balance,
    pub out: Option<String>,
    /// `WorldConfig::parallel_apply`.
    pub parallel_apply: bool,
    /// Successive-halving screens: the lengths in days of the short runs that
    /// come before the full `days` run (ascending, each shorter than `days`).
    /// Empty: every trial gets the full run, as before.
    pub screen_days: Vec<u32>,
    /// Seeds per trial in the screens (default: `seeds`).
    pub screen_seeds: Option<u64>,
    /// Each screen keeps the best `ceil(n / eta)` trials (plus the center).
    pub eta: f64,
}

/// Outcome of one world run.
#[derive(Clone, Debug, PartialEq)]
pub struct RunMetrics {
    pub names: Vec<String>,
    pub founders: Vec<u32>,
    pub alive: Vec<usize>,
    pub adults: Vec<usize>,
    /// Day each lineage went extinct (no living members), if it did.
    pub extinct_day: Vec<Option<u64>>,
    /// Days on which any owner was at ≥ 95% of its population cap.
    pub capped_days: u32,
    pub days: u32,
    pub mean_population: f64,
}

impl RunMetrics {
    pub fn persisted(&self, l: usize) -> bool {
        self.adults[l] >= 2 && self.alive[l] >= 4
    }

    /// Up to 1.5 per lineage (1 for surviving, 0.5 for holding its founding
    /// size), minus up to 3 for spending the run pinned at a population cap.
    pub fn score(&self) -> f64 {
        let mut s = 0.0;
        for l in 0..self.names.len() {
            if self.persisted(l) {
                s += 1.0;
            } else if let Some(d) = self.extinct_day[l] {
                // Partial credit for lasting longer.
                s += 0.3 * d as f64 / self.days as f64;
            }
            let health = ((1 + self.alive[l]) as f64).ln() / ((1 + self.founders[l]) as f64).ln();
            s += 0.5 * health.min(1.0);
        }
        s - 3.0 * self.capped_days as f64 / self.days as f64
    }
}

/// A world being scored, which can be run further: a screened trial that is
/// promoted continues its worlds instead of starting them again. Running to
/// day 30 and then on to day 120 gives exactly the world (and metrics) of a
/// straight 120-day run, since the world advances one day at a time either way.
pub struct ScoredRun {
    world: World,
    extinct_day: Vec<Option<u64>>,
    capped_days: u32,
    pop_sum: f64,
    days_run: u32,
    /// Everything died: later days change nothing.
    collapsed: bool,
}

impl ScoredRun {
    pub fn new(balance: Balance, seed: u64, size: usize, parallel_apply: bool) -> ScoredRun {
        let config = WorldConfig { seed, width: size, height: size, balance, parallel_apply, ..Default::default() };
        let world = presets::standard_world(config, 1);
        let n = world.lineages.len();
        ScoredRun { world, extinct_day: vec![None; n], capped_days: 0, pop_sum: 0.0, days_run: 0, collapsed: false }
    }

    /// Run on until `days` days have been simulated (or everything died).
    pub fn advance_to(&mut self, days: u32) {
        let n = self.world.lineages.len();
        while self.days_run < days && !self.collapsed {
            self.world.run_days(1);
            self.days_run += 1;
            let counts = self.world.lineage_counts();
            for (l, c) in counts.iter().enumerate() {
                if c.alive == 0 && self.extinct_day[l].is_none() {
                    self.extinct_day[l] = Some(self.world.day());
                }
            }
            let capped = (0..n as u32).any(|l| {
                let (count, cap) = self.world.cap_usage(l);
                count as f32 >= 0.95 * cap as f32
            });
            if capped {
                self.capped_days += 1;
            }
            self.pop_sum += self.world.alive_count() as f64;
            if self.world.alive_count() == 0 {
                self.collapsed = true;
            }
        }
    }

    pub fn days_run(&self) -> u32 {
        self.days_run
    }

    /// Metrics of a run scored over a horizon of `days` days (the length it
    /// was run to; a collapsed world counts its empty remaining days).
    pub fn metrics(&self, days: u32) -> RunMetrics {
        let counts = self.world.lineage_counts();
        RunMetrics {
            names: self.world.lineages.iter().map(|l| l.name.clone()).collect(),
            founders: self.world.lineages.iter().map(|l| l.founders).collect(),
            alive: counts.iter().map(|c| c.alive).collect(),
            adults: counts.iter().map(|c| c.adults).collect(),
            extinct_day: self.extinct_day.clone(),
            capped_days: self.capped_days,
            days,
            mean_population: self.pop_sum / days as f64,
        }
    }
}

pub fn run_one(balance: Balance, seed: u64, size: usize, days: u32, parallel_apply: bool) -> RunMetrics {
    let mut run = ScoredRun::new(balance, seed, size, parallel_apply);
    run.advance_to(days);
    run.metrics(days)
}

/// Per-seed results of one configuration, ordered by seed.
struct Scored {
    /// `(seed, metrics)`, seeds ascending.
    runs: Vec<(u64, RunMetrics)>,
}

impl Scored {
    fn scores(&self) -> Vec<f64> {
        self.runs.iter().map(|(_, r)| r.score()).collect()
    }

    fn score(&self) -> f64 {
        self.runs.iter().map(|(_, r)| r.score()).sum::<f64>() / self.runs.len() as f64
    }

    fn summary(&self) -> Summary {
        Summary::of(&self.scores())
    }

    fn survivors(&self, l: usize) -> usize {
        self.runs.iter().filter(|(_, r)| r.persisted(l)).count()
    }

    fn survival(&self, l: usize) -> f64 {
        self.survivors(l) as f64 / self.runs.len() as f64
    }

    fn alive(&self, l: usize) -> Vec<f64> {
        self.runs.iter().map(|(_, r)| r.alive[l] as f64).collect()
    }

    fn mean_alive(&self, l: usize) -> f64 {
        self.runs.iter().map(|(_, r)| r.alive[l] as f64).sum::<f64>() / self.runs.len() as f64
    }

    fn mean_population(&self) -> f64 {
        self.runs.iter().map(|(_, r)| r.mean_population).sum::<f64>() / self.runs.len() as f64
    }

    /// Paired per-seed differences `f(self) − f(other)` on the seeds both have.
    fn paired(&self, other: &Scored, f: impl Fn(&RunMetrics) -> f64) -> Vec<f64> {
        self.runs
            .iter()
            .filter_map(|(s, a)| other.runs.iter().find(|(t, _)| t == s).map(|(_, b)| f(a) - f(b)))
            .collect()
    }
}

struct Trial {
    round: u32,
    index: usize,
    balance: Balance,
    /// Results of the last stage this trial reached.
    scored: Scored,
    /// Length of that stage in days (`SweepArgs::days` for finalists).
    days: u32,
    /// Paired score difference to the round's center on the same seeds.
    delta: Option<Summary>,
}

impl Trial {
    fn score(&self) -> f64 {
        self.scored.score()
    }
}

fn perturb(center: &Balance, spread: f32, seed: u64, round: u32, index: usize) -> Balance {
    let mut b = *center;
    if index == 0 || spread <= 0.0 {
        return b;
    }
    let mut rng = rng_for(seed, 0x5eed, round as u64, index as u64);
    for (_, v) in b.fields_mut() {
        *v *= rng.gen_range(-spread..spread).exp();
    }
    b
}

/// How many of `n` trials a screen promotes on merit: `ceil(n / eta)`.
fn keep_count(n: usize, eta: f64) -> usize {
    ((n as f64 / eta.max(1.0)).ceil() as usize).clamp(1, n.max(1))
}

/// Successive-halving promotion: the best `ceil(n / eta)` of `ranked` (trial
/// indices, best first), plus trial 0 (the round's center, the reference for
/// paired differences) if it missed the cut.
fn promote(ranked: &[usize], eta: f64) -> Vec<usize> {
    let keep = keep_count(ranked.len(), eta);
    let mut kept: Vec<usize> = ranked[..keep].to_vec();
    if ranked.contains(&0) && !kept.contains(&0) {
        kept.push(0);
    }
    kept
}

/// The stages of one round: `(days, seeds)`, screens first, the full run last.
fn stages(args: &SweepArgs) -> Vec<(u32, u64)> {
    let screen_seeds = args.screen_seeds.unwrap_or(args.seeds).max(1);
    let mut v: Vec<(u32, u64)> =
        args.screen_days.iter().filter(|&&d| d > 0 && d < args.days).map(|&d| (d, screen_seeds)).collect();
    v.sort_unstable();
    v.dedup();
    v.push((args.days, args.seeds));
    v
}

fn lineage_names() -> Vec<String> {
    presets::standard_world(WorldConfig { width: 16, height: 16, ..Default::default() }, 1)
        .lineages
        .iter()
        .map(|l| l.name.clone())
        .collect()
}

pub fn sweep(args: &SweepArgs) {
    let mut center = args.center;
    let mut csv = String::new();
    let mut all: Vec<Trial> = Vec::new();
    let lineage_names = lineage_names();
    let stages = stages(args);

    println!(
        "Sweep: {} rounds × {} trials × {} seeds · {}×{} map · {} days · {} threads",
        args.rounds,
        args.trials,
        args.seeds,
        args.size,
        args.size,
        args.days,
        rayon::current_num_threads()
    );
    if stages.len() > 1 {
        let screens: Vec<String> =
            stages[..stages.len() - 1].iter().map(|(d, s)| format!("{d} days × {s} seeds")).collect();
        println!("Screens: {} · each keeps the best 1/{} (and the center)", screens.join(", then "), args.eta);
    }
    let mut world_days: u64 = 0;
    let started_all = Instant::now();
    for round in 0..args.rounds {
        let spread = args.spread / 2f32.powi(round as i32);
        let started = Instant::now();
        let balances: Vec<Balance> =
            (0..args.trials).map(|k| perturb(&center, spread, args.base_seed, round, k)).collect();
        // Worlds kept between stages, indexed `[trial][seed]`.
        let mut held: Vec<Vec<Option<ScoredRun>>> = (0..args.trials).map(|_| Vec::new()).collect();
        let mut active: Vec<usize> = (0..args.trials).collect();
        let mut stage_scores: Vec<Option<Scored>> = (0..args.trials).map(|_| None).collect();
        let mut stage_days = vec![0u32; args.trials];
        let mut screen_lines = Vec::new();
        for (si, &(days, seeds)) in stages.iter().enumerate() {
            let last = si + 1 == stages.len();
            let jobs: Vec<(usize, u64, Option<ScoredRun>)> = active
                .iter()
                .flat_map(|&k| (0..seeds).map(move |s| (k, s)))
                .map(|(k, s)| (k, s, held[k].get_mut(s as usize).and_then(Option::take)))
                .collect();
            world_days +=
                jobs.iter().map(|(_, _, r)| (days - r.as_ref().map_or(0, ScoredRun::days_run)) as u64).sum::<u64>();
            let results: Vec<(usize, u64, RunMetrics, Option<ScoredRun>)> = jobs
                .into_par_iter()
                .map(|(k, s, run)| {
                    let mut run = run.unwrap_or_else(|| {
                        ScoredRun::new(balances[k], args.base_seed + s, args.size, args.parallel_apply)
                    });
                    run.advance_to(days);
                    let m = run.metrics(days);
                    (k, s, m, if last { None } else { Some(run) })
                })
                .collect();
            for &k in &active {
                stage_scores[k] = Some(Scored { runs: Vec::new() });
                stage_days[k] = days;
                held[k] = Vec::new();
            }
            for (k, s, m, run) in results {
                stage_scores[k].as_mut().expect("active trial").runs.push((args.base_seed + s, m));
                if let Some(run) = run {
                    let h = &mut held[k];
                    if h.len() <= s as usize {
                        h.resize_with(s as usize + 1, || None);
                    }
                    h[s as usize] = Some(run);
                }
            }
            if last {
                break;
            }
            let mut ranked = active.clone();
            let score = |k: &usize| stage_scores[*k].as_ref().expect("scored").score();
            ranked.sort_by(|a, b| score(b).total_cmp(&score(a)));
            let kept = promote(&ranked, args.eta);
            let top = keep_count(ranked.len(), args.eta);
            let center_added = kept.len() > top;
            screen_lines.push(format!(
                "  screen {days} days × {seeds} seeds: {} trials → {} kept (best {:.2}, cut at {:.2}{})",
                active.len(),
                kept.len(),
                score(&ranked[0]),
                score(&ranked[top - 1]),
                if center_added { ", plus the center as reference" } else { "" }
            ));
            for &k in &active {
                if !kept.contains(&k) {
                    held[k].clear();
                }
            }
            active = kept;
            active.sort_unstable();
        }
        drop(held);

        let mut trials: Vec<Trial> = balances
            .iter()
            .enumerate()
            .map(|(k, b)| Trial {
                round,
                index: k,
                balance: *b,
                scored: stage_scores[k].take().expect("every trial ran at least one stage"),
                days: stage_days[k],
                delta: None,
            })
            .collect();
        // Paired differences to the center on the full run.
        if let Some(c) = trials.iter().position(|t| t.index == 0 && t.days == args.days) {
            let deltas: Vec<Option<Summary>> = trials
                .iter()
                .map(|t| {
                    (t.days == args.days).then(|| Summary::of(&t.scored.paired(&trials[c].scored, RunMetrics::score)))
                })
                .collect();
            for (t, d) in trials.iter_mut().zip(deltas) {
                t.delta = d;
            }
        }
        // Finalists first (by score), then the screened-out trials.
        trials.sort_by(|a, b| (b.days, b.score()).partial_cmp(&(a.days, a.score())).expect("finite scores"));

        println!(
            "\n── Round {} (spread ±{:.0}%) · {:.0}s ──",
            round + 1,
            (spread.exp() - 1.0) * 100.0,
            started.elapsed().as_secs_f64()
        );
        for line in &screen_lines {
            println!("{line}");
        }
        print_header(&lineage_names);
        let baseline = trials.iter().find(|t| t.index == 0 && t.days == args.days);
        for t in trials.iter().filter(|t| t.days == args.days).take(6) {
            print_trial(t, &lineage_names);
        }
        if let Some(b) = baseline {
            let s = b.scored.summary();
            println!("  (round center scored {:.2} ± {})", b.score(), fmt_ci(s.ci95));
        }
        let winner = &trials[0];
        if winner.index != 0 {
            if let Some(d) = winner.delta.filter(|d| !d.significant()) {
                println!(
                    "  (the winner beats the center by {:+.2} ± {}: not significant on these seeds)",
                    d.mean,
                    fmt_ci(d.ci95)
                );
            }
        }
        center = winner.balance;
        all.extend(trials);
    }

    // CSV of every trial. New columns go at the end of each row so the
    // original layout (round, trial, score, survival and alive per lineage,
    // the parameters) is unchanged.
    let fields = Balance::default().fields();
    let _ = write!(csv, "round,trial,score");
    for name in &lineage_names {
        let _ = write!(csv, ",survival {name},alive {name}");
    }
    for (f, _) in fields {
        let _ = write!(csv, ",{f}");
    }
    let _ = write!(csv, ",days,seeds,score se,score ci95,delta center,delta ci95");
    for name in &lineage_names {
        let _ = write!(csv, ",wilson lo {name},wilson hi {name},se alive {name}");
    }
    csv.push('\n');
    for t in &all {
        let _ = write!(csv, "{},{},{:.3}", t.round + 1, t.index, t.score());
        for l in 0..lineage_names.len() {
            let _ = write!(csv, ",{:.2},{:.1}", t.scored.survival(l), t.scored.mean_alive(l));
        }
        for (_, v) in t.balance.fields() {
            let _ = write!(csv, ",{v:.4}");
        }
        let s = t.scored.summary();
        let _ = write!(csv, ",{},{},{:.3},{:.3}", t.days, t.scored.runs.len(), s.se, s.ci95);
        match t.delta {
            Some(d) => {
                let _ = write!(csv, ",{:.3},{:.3}", d.mean, d.ci95);
            }
            None => csv.push_str(",,"),
        }
        for l in 0..lineage_names.len() {
            let (lo, hi) = wilson(t.scored.survivors(l), t.scored.runs.len());
            let _ = write!(csv, ",{lo:.2},{hi:.2},{:.1}", Summary::of(&t.scored.alive(l)).se);
        }
        csv.push('\n');
    }
    if let Some(path) = &args.out {
        match File::create(path).and_then(|mut f| f.write_all(csv.as_bytes())) {
            Ok(()) => println!("\nWrote {path}"),
            Err(e) => eprintln!("could not write {path}: {e}"),
        }
    }

    if stages.len() > 1 {
        let full = args.rounds as u64 * args.trials as u64 * args.seeds * args.days as u64;
        println!(
            "\nSimulated {world_days} world-days in {:.0}s ({:.0}% of the {full} a sweep without screens would run)",
            started_all.elapsed().as_secs_f64(),
            100.0 * world_days as f64 / full as f64
        );
    }

    let best = all
        .iter()
        .filter(|t| t.days == args.days)
        .max_by(|a, b| a.score().total_cmp(&b.score()))
        .expect("at least one trial");
    let s = best.scored.summary();
    let spread = if s.n < 2 {
        "1 seed: no CI".to_string()
    } else {
        format!("± {:.2} 95% CI, SE {:.2}, {} seeds", s.ci95, s.se, s.n)
    };
    println!("\nBest: round {} trial {} · score {:.2} ({spread})", best.round + 1, best.index, best.score());
    print_lineage_table(&best.scored, &lineage_names);
    println!("Balance {{");
    for ((name, v), (_, d)) in best.balance.fields().iter().zip(Balance::default().fields()) {
        println!("    {name}: {v:.3},  // default {d}");
    }
    println!("}}");
    let sets: Vec<String> = best.balance.fields().iter().map(|(n, v)| format!("--set {n}={v:.3}")).collect();
    println!("\nTry it: evo-sim run {}", sets.join(" "));
    let changed = changed_sets(&best.balance, &Balance::default());
    if changed.len() > 4 {
        println!("Check it on unseen seeds: evo-sim compare --seed 31 --seeds 24 <the --set flags above> vs defaults");
    } else if !changed.is_empty() {
        println!("Check it on unseen seeds: evo-sim compare --seed 31 --seeds 24 {} vs defaults", changed.join(" "));
    }
}

/// A CI half-width rounded to a whole number (∞ for too few seeds).
fn whole(ci: f64) -> String {
    if ci.is_finite() {
        format!("{ci:.0}")
    } else {
        "∞".into()
    }
}

fn fmt_ci(ci: f64) -> String {
    if ci.is_finite() {
        format!("{ci:.2}")
    } else {
        "∞".into()
    }
}

/// `--set` flags for the parameters of `b` that differ from `base`.
fn changed_sets(b: &Balance, base: &Balance) -> Vec<String> {
    b.fields()
        .iter()
        .zip(base.fields())
        .filter(|((_, v), (_, d))| v != d)
        .map(|((n, v), _)| format!("--set {n}={v}"))
        .collect()
}

/// Survival per lineage with its Wilson 95% interval, and mean alive ± 95% CI.
fn print_lineage_table(s: &Scored, names: &[String]) {
    let n = s.runs.len();
    println!("  {:<22} {:>9} {:>13} {:>16}", "lineage", "survived", "95% CI", "mean alive");
    for (l, name) in names.iter().enumerate() {
        let k = s.survivors(l);
        let (lo, hi) = wilson(k, n);
        let alive = Summary::of(&s.alive(l));
        println!(
            "  {:<22} {:>4} of {:<2} {:>13} {:>8.0} ± {:<6}",
            name,
            k,
            n,
            format!("{:.0}–{:.0}%", lo * 100.0, hi * 100.0),
            alive.mean,
            whole(alive.ci95)
        );
    }
}

fn short(name: &str) -> String {
    name.split_whitespace().map(|w| &w[..w.len().min(5)]).collect::<Vec<_>>().join(" ")
}

fn print_header(names: &[String]) {
    let cols: Vec<String> = names.iter().map(|n| format!("{:>11.11}", short(n))).collect();
    println!(
        "  {:>5} {:>6} {:>5} {:>12} {:>6} {}  (survival rate / mean alive)",
        "trial",
        "score",
        "±95%",
        "Δ center",
        "pop",
        cols.join("")
    );
}

fn print_trial(t: &Trial, names: &[String]) {
    let cols: Vec<String> = (0..names.len())
        .map(|l| format!("{:>5.0}%/{:<5.0}", t.scored.survival(l) * 100.0, t.scored.mean_alive(l)))
        .collect();
    let s = t.scored.summary();
    let delta = match t.delta {
        Some(d) if t.index != 0 => {
            format!("{:+.2}±{}{}", d.mean, fmt_ci(d.ci95), if d.significant() { "*" } else { " " })
        }
        _ => "-".into(),
    };
    println!(
        "  {:>5} {:>6.2} {:>5} {:>12} {:>6.0} {}",
        t.index,
        t.score(),
        fmt_ci(s.ci95),
        delta,
        t.scored.mean_population(),
        cols.join("")
    );
    let changed: Vec<String> = t
        .balance
        .fields()
        .iter()
        .zip(Balance::default().fields())
        .filter(|((_, v), (_, d))| (v / d - 1.0).abs() > 0.02)
        .map(|((n, v), (_, d))| format!("{n} {:+.0}%", (v / d - 1.0) * 100.0))
        .collect();
    if !changed.is_empty() {
        println!("        {}", changed.join(", "));
    }
}

pub struct CompareArgs {
    pub base_seed: u64,
    pub size: usize,
    pub days: u32,
    pub seeds: u64,
    pub a: Balance,
    pub b: Balance,
    pub out: Option<String>,
    pub parallel_apply: bool,
}

/// Label of a configuration: its parameters that differ from the defaults.
fn label(b: &Balance) -> String {
    let changed: Vec<String> = b
        .fields()
        .iter()
        .zip(Balance::default().fields())
        .filter(|((_, v), (_, d))| v != d)
        .map(|((n, v), _)| format!("{n}={v}"))
        .collect();
    if changed.is_empty() {
        "defaults".into()
    } else {
        changed.join(" ")
    }
}

/// Paired comparison: run `a` and `b` on the same seeds and report the
/// per-seed difference `a − b` with a 95% CI. Returns that difference.
pub fn compare(args: &CompareArgs) -> Summary {
    let names = lineage_names();
    let (la, lb) = (label(&args.a), label(&args.b));
    println!(
        "Compare: A = {la} vs B = {lb} · seeds {}–{} · {}×{} map · {} days · {} threads",
        args.base_seed,
        args.base_seed + args.seeds - 1,
        args.size,
        args.size,
        args.days,
        rayon::current_num_threads()
    );
    let started = Instant::now();
    let jobs: Vec<(usize, u64)> = (0..2).flat_map(|c| (0..args.seeds).map(move |s| (c, s))).collect();
    let results: Vec<(usize, u64, RunMetrics)> = jobs
        .par_iter()
        .map(|&(c, s)| {
            let balance = if c == 0 { args.a } else { args.b };
            (c, s, run_one(balance, args.base_seed + s, args.size, args.days, args.parallel_apply))
        })
        .collect();
    let mut sides = [Scored { runs: Vec::new() }, Scored { runs: Vec::new() }];
    for (c, s, m) in results {
        sides[c].runs.push((args.base_seed + s, m));
    }
    let [a, b] = &sides;

    println!("\n  {:>6} {:>7} {:>7} {:>7}  lineages lost (A | B)", "seed", "A", "B", "A − B");
    let mut csv = String::from("seed,score a,score b,diff\n");
    for ((seed, ra), (_, rb)) in a.runs.iter().zip(&b.runs) {
        let lost = |r: &RunMetrics| -> String {
            let v: Vec<String> = (0..names.len()).filter(|&l| !r.persisted(l)).map(|l| short(&names[l])).collect();
            if v.is_empty() {
                "-".into()
            } else {
                v.join(", ")
            }
        };
        println!(
            "  {:>6} {:>7.2} {:>7.2} {:>+7.2}  {} | {}",
            seed,
            ra.score(),
            rb.score(),
            ra.score() - rb.score(),
            lost(ra),
            lost(rb)
        );
        let _ = writeln!(csv, "{seed},{:.3},{:.3},{:.3}", ra.score(), rb.score(), ra.score() - rb.score());
    }

    let (sa, sb) = (a.summary(), b.summary());
    let diffs = a.paired(b, RunMetrics::score);
    let d = Summary::of(&diffs);
    let wins = diffs.iter().filter(|&&x| x > 1e-9).count();
    let losses = diffs.iter().filter(|&&x| x < -1e-9).count();
    // What the CI would be if the two sets of worlds were independent.
    let unpaired = crate::stats::t95(sa.n + sb.n - 2) * (sa.se.powi(2) + sb.se.powi(2)).sqrt();
    println!(
        "\n  A: {:.2} ± {} (SE {:.2})   B: {:.2} ± {} (SE {:.2})",
        sa.mean,
        fmt_ci(sa.ci95),
        sa.se,
        sb.mean,
        fmt_ci(sb.ci95),
        sb.se
    );
    println!(
        "  Paired difference A − B: {:+.2} ± {} (95% CI {:+.2} … {:+.2}, SE {:.2}, t = {:.2}, n = {})",
        d.mean,
        fmt_ci(d.ci95),
        d.mean - d.ci95,
        d.mean + d.ci95,
        d.se,
        d.t(),
        d.n
    );
    println!(
        "  A better on {wins} seeds, B on {losses}, tied on {}; unpaired the CI would be ±{}",
        d.n - wins - losses,
        fmt_ci(unpaired)
    );

    println!("\n  {:<22} {:>9} {:>9} {:>16} {:>18}", "lineage", "survival A", "B", "Δ survival", "Δ mean alive");
    for (l, name) in names.iter().enumerate() {
        let ds = Summary::of(&a.paired(b, |r| r.persisted(l) as u8 as f64));
        let dn = Summary::of(&a.paired(b, |r| r.alive[l] as f64));
        let star = |s: &Summary| if s.significant() { "*" } else { " " };
        println!(
            "  {:<22} {:>9.0}% {:>8.0}% {:>15}{} {:>+9.0} ± {:<5}{}",
            name,
            a.survival(l) * 100.0,
            b.survival(l) * 100.0,
            format!("{:+.0} ± {}%", ds.mean * 100.0, whole(ds.ci95 * 100.0)),
            star(&ds),
            dn.mean,
            whole(dn.ci95),
            star(&dn)
        );
    }
    println!("  (* : the 95% CI of the paired difference excludes 0)");

    let verdict = if d.n < 2 {
        "need at least 2 seeds for a confidence interval".to_string()
    } else if d.significant() {
        format!("{} is better (95% CI excludes 0)", if d.mean > 0.0 { "A" } else { "B" })
    } else {
        // How many seeds would resolve a true difference of this size, if it
        // is one: CI half-width shrinks as 1/sqrt(n).
        let need =
            if d.mean.abs() > 1e-9 { ((d.ci95 / d.mean.abs()).powi(2) * d.n as f64).ceil() } else { f64::INFINITY };
        if need.is_finite() {
            format!("no significant difference (a true difference of {:+.2} would need about {need:.0} seeds)", d.mean)
        } else {
            "no difference".to_string()
        }
    };
    println!("\nVerdict: {verdict} · {:.0}s", started.elapsed().as_secs_f64());
    if let Some(path) = &args.out {
        match File::create(path).and_then(|mut f| f.write_all(csv.as_bytes())) {
            Ok(()) => println!("Wrote {path}"),
            Err(e) => eprintln!("could not write {path}: {e}"),
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumed_runs_match_straight_runs() {
        let b = Balance::default();
        let straight = run_one(b, 5, 48, 6, true);
        let mut run = ScoredRun::new(b, 5, 48, true);
        run.advance_to(2);
        let early = run.metrics(2);
        assert_eq!(early.days, 2);
        run.advance_to(6);
        assert_eq!(run.days_run(), 6);
        assert_eq!(run.metrics(6), straight);
    }

    #[test]
    fn promotion_keeps_the_best_and_the_center() {
        assert_eq!(promote(&[3, 0, 1, 2], 2.0), vec![3, 0]);
        assert_eq!(promote(&[3, 1, 2, 0], 2.0), vec![3, 1, 0]);
        assert_eq!(promote(&[2, 0, 1], 3.0), vec![2, 0]);
        assert_eq!(promote(&[0, 4, 1, 2, 3], 3.0), vec![0, 4]);
        assert_eq!(promote(&[5], 3.0), vec![5]);
        // eta below 1 keeps everything.
        assert_eq!(promote(&[1, 0], 0.5), vec![1, 0]);
    }

    #[test]
    fn stages_put_screens_before_the_full_run() {
        let args = SweepArgs {
            base_seed: 1,
            size: 32,
            days: 120,
            trials: 4,
            seeds: 12,
            rounds: 1,
            spread: 0.5,
            center: Balance::default(),
            out: None,
            parallel_apply: true,
            screen_days: vec![60, 30, 200, 30],
            screen_seeds: Some(4),
            eta: 3.0,
        };
        assert_eq!(stages(&args), vec![(30, 4), (60, 4), (120, 12)]);
        assert_eq!(stages(&SweepArgs { screen_days: Vec::new(), ..args }), vec![(120, 12)]);
    }

    #[test]
    fn labels_name_the_changed_parameters() {
        assert_eq!(label(&Balance::default()), "defaults");
        let mut b = Balance::default();
        assert!(b.set("larval_competition", 18.0));
        assert_eq!(label(&b), "larval_competition=18");
    }
}
