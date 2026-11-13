//! Balance sweep: random search over `Balance` parameters, scored on whether
//! every lineage (wild species and sample players) coexists for a year.
//!
//! Each round perturbs every parameter of the current best by a random factor
//! in `[e^-spread, e^spread]`, evaluates every trial on several seeds in
//! parallel, and recenters on the best trial. The spread halves each round.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Write as _;
use std::time::Instant;

use evo_sim::rng::rng_for;
use evo_sim::{presets, Balance, WorldConfig};
use rand::Rng;
use rayon::prelude::*;

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
}

/// Outcome of one world run.
#[derive(Clone, Debug)]
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

pub fn run_one(balance: Balance, seed: u64, size: usize, days: u32, parallel_apply: bool) -> RunMetrics {
    let config = WorldConfig { seed, width: size, height: size, balance, parallel_apply, ..Default::default() };
    let mut world = presets::standard_world(config, 1);
    let n = world.lineages.len();
    let mut extinct_day = vec![None; n];
    let mut capped_days = 0;
    let mut pop_sum = 0.0;
    for _ in 0..days {
        world.run_days(1);
        let counts = world.lineage_counts();
        for (l, c) in counts.iter().enumerate() {
            if c.alive == 0 && extinct_day[l].is_none() {
                extinct_day[l] = Some(world.day());
            }
        }
        let capped = (0..n as u32).any(|l| {
            let (count, cap) = world.cap_usage(l);
            count as f32 >= 0.95 * cap as f32
        });
        if capped {
            capped_days += 1;
        }
        pop_sum += world.alive_count() as f64;
        if world.alive_count() == 0 {
            break;
        }
    }
    let counts = world.lineage_counts();
    RunMetrics {
        names: world.lineages.iter().map(|l| l.name.clone()).collect(),
        founders: world.lineages.iter().map(|l| l.founders).collect(),
        alive: counts.iter().map(|c| c.alive).collect(),
        adults: counts.iter().map(|c| c.adults).collect(),
        extinct_day,
        capped_days,
        days,
        mean_population: pop_sum / days as f64,
    }
}

struct Trial {
    round: u32,
    index: usize,
    balance: Balance,
    runs: Vec<RunMetrics>,
}

impl Trial {
    fn score(&self) -> f64 {
        self.runs.iter().map(RunMetrics::score).sum::<f64>() / self.runs.len() as f64
    }

    fn survival(&self, l: usize) -> f64 {
        self.runs.iter().filter(|r| r.persisted(l)).count() as f64 / self.runs.len() as f64
    }

    fn mean_alive(&self, l: usize) -> f64 {
        self.runs.iter().map(|r| r.alive[l] as f64).sum::<f64>() / self.runs.len() as f64
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

pub fn sweep(args: &SweepArgs) {
    let mut center = args.center;
    let mut csv = String::new();
    let mut all: Vec<Trial> = Vec::new();
    let lineage_names = presets::standard_world(WorldConfig { width: 16, height: 16, ..Default::default() }, 1)
        .lineages
        .iter()
        .map(|l| l.name.clone())
        .collect::<Vec<_>>();

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
    for round in 0..args.rounds {
        let spread = args.spread / 2f32.powi(round as i32);
        let started = Instant::now();
        let balances: Vec<Balance> =
            (0..args.trials).map(|k| perturb(&center, spread, args.base_seed, round, k)).collect();
        let jobs: Vec<(usize, u64)> = (0..args.trials).flat_map(|k| (0..args.seeds).map(move |s| (k, s))).collect();
        let results: Vec<(usize, RunMetrics)> = jobs
            .par_iter()
            .map(|&(k, s)| (k, run_one(balances[k], args.base_seed + s, args.size, args.days, args.parallel_apply)))
            .collect();
        let mut trials: Vec<Trial> = balances
            .iter()
            .enumerate()
            .map(|(k, b)| Trial { round, index: k, balance: *b, runs: Vec::new() })
            .collect();
        for (k, m) in results {
            trials[k].runs.push(m);
        }
        trials.sort_by(|a, b| b.score().total_cmp(&a.score()));

        println!(
            "\n── Round {} (spread ±{:.0}%) · {:.0}s ──",
            round + 1,
            (spread.exp() - 1.0) * 100.0,
            started.elapsed().as_secs_f64()
        );
        print_header(&lineage_names);
        let baseline = trials.iter().find(|t| t.index == 0).map(|t| t.score());
        for t in trials.iter().take(6) {
            print_trial(t, &lineage_names);
        }
        if let Some(b) = baseline {
            println!("  (round center scored {b:.2})");
        }
        center = trials[0].balance;
        all.extend(trials);
    }

    // CSV of every trial.
    let fields = Balance::default().fields();
    let _ = write!(csv, "round,trial,score");
    for name in &lineage_names {
        let _ = write!(csv, ",survival {name},alive {name}");
    }
    for (f, _) in fields {
        let _ = write!(csv, ",{f}");
    }
    csv.push('\n');
    for t in &all {
        let _ = write!(csv, "{},{},{:.3}", t.round + 1, t.index, t.score());
        for l in 0..lineage_names.len() {
            let _ = write!(csv, ",{:.2},{:.1}", t.survival(l), t.mean_alive(l));
        }
        for (_, v) in t.balance.fields() {
            let _ = write!(csv, ",{v:.4}");
        }
        csv.push('\n');
    }
    if let Some(path) = &args.out {
        match File::create(path).and_then(|mut f| f.write_all(csv.as_bytes())) {
            Ok(()) => println!("\nWrote {path}"),
            Err(e) => eprintln!("could not write {path}: {e}"),
        }
    }

    let best = all.iter().max_by(|a, b| a.score().total_cmp(&b.score())).expect("at least one trial");
    println!("\nBest: round {} trial {} · score {:.2}", best.round + 1, best.index, best.score());
    println!("Balance {{");
    for ((name, v), (_, d)) in best.balance.fields().iter().zip(Balance::default().fields()) {
        println!("    {name}: {v:.3},  // default {d}");
    }
    println!("}}");
    let sets: Vec<String> = best.balance.fields().iter().map(|(n, v)| format!("--set {n}={v:.3}")).collect();
    println!("\nTry it: evo-sim run {}", sets.join(" "));
}

fn short(name: &str) -> String {
    name.split_whitespace().map(|w| &w[..w.len().min(5)]).collect::<Vec<_>>().join(" ")
}

fn print_header(names: &[String]) {
    let cols: Vec<String> = names.iter().map(|n| format!("{:>11.11}", short(n))).collect();
    println!("  {:>5} {:>6} {:>6} {}  (survival rate / mean alive)", "trial", "score", "pop", cols.join(""));
}

fn print_trial(t: &Trial, names: &[String]) {
    let cols: Vec<String> =
        (0..names.len()).map(|l| format!("{:>5.0}%/{:<5.0}", t.survival(l) * 100.0, t.mean_alive(l))).collect();
    let pop = t.runs.iter().map(|r| r.mean_population).sum::<f64>() / t.runs.len() as f64;
    println!("  {:>5} {:>6.2} {:>6.0} {}", t.index, t.score(), pop, cols.join(""));
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
