//! `evo-sim genetics`: statistical validation of the population genetics over
//! many generations and many replicate seeds.
//!
//! Two levels:
//!
//! - **genome** ([`wf`]): Wright–Fisher populations of fixed size `N` with
//!   random mating, built from the real genome code paths
//!   (`Genome::inherit`, `Genome::mutate_morph`, dominance and expression,
//!   load loci, origin tags), isolated from the ecology so population-genetics
//!   theory applies exactly. Strict checks with confidence intervals.
//! - **ecology** ([`eco`]): full `World` runs over many in-game years,
//!   tracking per-lineage heterozygosity, morphology, mutations, divergence
//!   and fertility. Soft checks (direction and magnitude).
//!
//! Every replicate draws from its own stream, `rng_for(seed, stream, check,
//! replicate)`, so results do not depend on the thread count. Nothing here
//! changes a world: the ecology level only reads organisms between steps.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

pub mod eco;
pub mod wf;

/// How a check is judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Strict check passed.
    Pass,
    /// Strict check failed.
    Fail,
    /// Soft (ecology) check in the expected direction and magnitude.
    SoftPass,
    /// Soft check outside expectations: reported, never fails the run.
    SoftFail,
    /// A measurement reported for information.
    Info,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            Verdict::SoftPass => "pass (soft)",
            Verdict::SoftFail => "WARN (soft)",
            Verdict::Info => "info",
        }
    }

    pub fn strict(pass: bool) -> Verdict {
        if pass {
            Verdict::Pass
        } else {
            Verdict::Fail
        }
    }

    pub fn soft(pass: bool) -> Verdict {
        if pass {
            Verdict::SoftPass
        } else {
            Verdict::SoftFail
        }
    }
}

/// One row of the report.
#[derive(Clone, Debug)]
pub struct Check {
    pub level: &'static str,
    pub group: &'static str,
    pub name: String,
    pub expected: String,
    pub observed: String,
    /// Confidence interval of the observed value (text), or "-".
    pub ci: String,
    pub verdict: Verdict,
    /// Model, replicates, notes.
    pub detail: String,
}

/// Number formatting that keeps small and large values readable.
pub fn num(x: f64) -> String {
    if !x.is_finite() {
        return format!("{x}");
    }
    let a = x.abs();
    if a == 0.0 {
        "0".into()
    } else if a >= 1000.0 {
        format!("{x:.0}")
    } else if a >= 10.0 {
        format!("{x:.1}")
    } else if a >= 0.01 {
        format!("{x:.4}")
    } else {
        format!("{x:.3e}")
    }
}

/// z for a two-sided 99.9% interval: strict checks pass when the expectation
/// lies within the observed mean ± `Z_STRICT` standard errors (plus any model
/// tolerance stated in the check).
pub const Z_STRICT: f64 = 3.291;

/// Mean and standard error of the mean.
pub fn mean_se(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    if xs.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let m = xs.iter().sum::<f64>() / n;
    if xs.len() < 2 {
        return (m, f64::NAN);
    }
    let v = xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0);
    (m, (v / n).sqrt())
}

/// Wilson score interval for `k` successes in `n` trials.
pub fn wilson(k: u64, n: u64, z: f64) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let (k, n) = (k as f64, n as f64);
    let p = k / n;
    let d = 1.0 + z * z / n;
    let c = (p + z * z / (2.0 * n)) / d;
    let h = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / d;
    (c - h, c + h)
}

/// A check of a mean against an expectation: passes when
/// `|mean - expected| <= Z_STRICT * se + tol`.
pub fn mean_check(
    group: &'static str,
    name: impl Into<String>,
    expected: f64,
    mean: f64,
    se: f64,
    tol: f64,
    detail: impl Into<String>,
) -> Check {
    let half = Z_STRICT * se;
    let pass = (mean - expected).abs() <= half + tol;
    Check {
        level: "genome",
        group,
        name: name.into(),
        expected: num(expected),
        observed: num(mean),
        ci: format!("[{}, {}]", num(mean - half), num(mean + half)),
        verdict: Verdict::strict(pass),
        detail: detail.into(),
    }
}

/// A binomial proportion check (Wilson interval at [`Z_STRICT`], widened by
/// `tol`).
pub fn prop_check(
    group: &'static str,
    name: impl Into<String>,
    expected: f64,
    k: u64,
    n: u64,
    tol: f64,
    detail: impl Into<String>,
) -> Check {
    let (lo, hi) = wilson(k, n, Z_STRICT);
    let p = k as f64 / n.max(1) as f64;
    let pass = expected >= lo - tol && expected <= hi + tol;
    Check {
        level: "genome",
        group,
        name: name.into(),
        expected: num(expected),
        observed: format!("{} ({k}/{n})", num(p)),
        ci: format!("[{}, {}]", num(lo), num(hi)),
        verdict: Verdict::strict(pass),
        detail: detail.into(),
    }
}

/// An exact count that must be zero (or equal to the expectation).
pub fn exact_check(
    group: &'static str,
    name: impl Into<String>,
    expected: u64,
    got: u64,
    detail: impl Into<String>,
) -> Check {
    Check {
        level: "genome",
        group,
        name: name.into(),
        expected: expected.to_string(),
        observed: got.to_string(),
        ci: "-".into(),
        verdict: Verdict::strict(expected == got),
        detail: detail.into(),
    }
}

/// Run sizes. `quick` is the CI subset (`#[test]`s); `long` the full run.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub name: &'static str,
    /// Neutral-drift replicates.
    pub drift_reps: usize,
    /// Mutation–drift equilibrium replicates.
    pub equilibrium_reps: usize,
    /// Generations sampled after burn-in in the equilibrium runs.
    pub equilibrium_gens: usize,
    /// Single-generation crosses for the mutation-rate checks.
    pub mutation_children: usize,
    /// F2 offspring per dominance cross.
    pub f2_children: usize,
    /// Families for the midparent / segregation-variance checks.
    pub families: usize,
    /// Replicates of the recessive-load runs.
    pub load_reps: usize,
    /// Population size of the recessive-load runs.
    pub load_n: usize,
    /// Replicates of the legacy-tag runs.
    pub legacy_reps: usize,
    /// Offspring for the linkage (phase-known) test.
    pub linkage_children: usize,
}

impl Profile {
    pub const QUICK: Profile = Profile {
        name: "quick",
        drift_reps: 60,
        equilibrium_reps: 6,
        equilibrium_gens: 400,
        mutation_children: 40_000,
        f2_children: 4_000,
        families: 1_500,
        load_reps: 3,
        load_n: 600,
        legacy_reps: 60,
        linkage_children: 12_000,
    };
    pub const LONG: Profile = Profile {
        name: "long",
        drift_reps: 2_000,
        equilibrium_reps: 64,
        equilibrium_gens: 2_000,
        mutation_children: 1_000_000,
        f2_children: 100_000,
        families: 40_000,
        load_reps: 40,
        load_n: 2_000,
        legacy_reps: 1_000,
        linkage_children: 400_000,
    };
}

/// Options of `evo-sim genetics`.
#[derive(Clone, Debug)]
pub struct Opts {
    pub profile: Profile,
    pub genome: bool,
    pub ecology: bool,
    pub out: PathBuf,
    pub seed: u64,
    /// Genome level: only the check groups whose name contains this.
    pub only: Option<String>,
    pub eco: eco::EcoOpts,
}

const USAGE: &str = "\
evo-sim genetics — statistical validation of the population genetics

USAGE:
    evo-sim genetics [--long] [--level genome|ecology|all] [--out DIR] [--seed N]
                     [--years N] [--size N] [--seeds N] [--sample-days N] [--meat] [--food]
                     [--classic] [--niches]

    --long          Large replicate counts (minutes); default is the quick CI-sized run
    --level L       genome (Wright–Fisher on the real genome code), ecology (full worlds)
                    or all (default: genome for the quick run, all for --long)
    --out DIR       Write report.md and checks.csv (and ecology.csv) here
                    (default target/genetics)
    --seed N        Base seed (default 1)
    --years N       Ecology: in-game years per world (default 2 quick, 12 long)
    --size N        Ecology: map size (default 64 quick, 96 long)
    --seeds N       Ecology: worlds, run in parallel (default 1 quick, 4 long)
    --sample-days N Ecology: days between samples (default 10)
    --meat          Ecology: run with the carnivore-diet rules and predator brakes on
                    (`Balance::carnivore_diet`) and the predators in their niches
                    (`presets::niche_world`)
    --food          Ecology: the food values on (`Balance::food_values`)
    --classic       Ecology: the carnivore-diet rules, brakes and food values off, as
                    before snapshot version 28
    --niches        Ecology: the predators in their niches
    --only NAME     Genome level: only the check groups whose name contains NAME
                    (neutral drift, mutation-drift equilibrium, morphology step mutation,
                    mutation rates, dominance, additive inheritance, recessive load,
                    legacy tags, linkage)

The process exits with a failure status if any strict (genome-level) check fails.
";

pub fn parse(args: &[String]) -> Result<Opts, String> {
    let mut long = false;
    let mut level: Option<String> = None;
    let mut out = PathBuf::from("target/genetics");
    let mut seed = 1;
    let mut only = None;
    let (mut years, mut size, mut seeds, mut sample_days) = (None, None, None, 10);
    let mut balance = evo_sim::Balance::default();
    let mut niches = false;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{flag} needs a value"));
        let num = |v: String| v.parse::<u64>().map_err(|_| format!("{flag}: '{v}' is not a number"));
        match flag.as_str() {
            "--long" => long = true,
            "--quick" => long = false,
            "--level" => level = Some(value()?),
            "--out" => out = PathBuf::from(value()?),
            "--seed" => seed = num(value()?)?,
            "--only" => only = Some(value()?),
            "--years" => years = Some(num(value()?)? as u32),
            "--size" => size = Some(num(value()?)? as usize),
            "--seeds" => seeds = Some(num(value()?)?.max(1)),
            "--sample-days" => sample_days = num(value()?)?.max(1) as u32,
            "--meat" => {
                balance = balance.carnivore_diet();
                niches = true;
            }
            "--food" => balance = balance.food_values(),
            "--classic" => balance = balance.carnivore_diet_off().food_values_off(),
            "--niches" => niches = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let level = level.unwrap_or_else(|| if long { "all" } else { "genome" }.into());
    let (genome, ecology) = match level.as_str() {
        "genome" => (true, false),
        "ecology" => (false, true),
        "all" => (true, true),
        other => return Err(format!("--level: unknown level '{other}'")),
    };
    Ok(Opts {
        profile: if long { Profile::LONG } else { Profile::QUICK },
        genome,
        ecology,
        out,
        seed,
        only,
        eco: eco::EcoOpts {
            days: years.unwrap_or(if long { 12 } else { 2 }) as u64
                * evo_sim::WorldConfig::default().days_per_year as u64,
            years: years.unwrap_or(if long { 12 } else { 2 }),
            size: size.unwrap_or(if long { 96 } else { 64 }),
            seeds: seeds.unwrap_or(if long { 4 } else { 1 }),
            sample_days,
            seed,
            balance,
            niches,
        },
    })
}

/// Entry point of `evo-sim genetics`.
pub fn main(args: &[String]) -> ExitCode {
    let opts = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            if e.is_empty() {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let mut checks = Vec::new();
    let mut sections = Vec::new();
    let mut timings = Vec::new();
    let started = Instant::now();
    if opts.genome {
        let t = Instant::now();
        let r = wf::run_all(&opts.profile, opts.seed, opts.only.as_deref(), true);
        timings.push(("genome level", t.elapsed().as_secs_f64()));
        checks.extend(r.checks);
        sections.extend(r.sections);
    }
    let mut eco_csv = None;
    if opts.ecology {
        let t = Instant::now();
        let r = eco::run(&opts.eco, true);
        timings.push(("ecology level", t.elapsed().as_secs_f64()));
        checks.extend(r.checks);
        sections.extend(r.sections);
        eco_csv = Some(r.csv);
    }
    let total = started.elapsed().as_secs_f64();
    let md = report_markdown(&opts, &checks, &sections, &timings, total);
    if let Err(e) = std::fs::create_dir_all(&opts.out)
        .and_then(|_| std::fs::write(opts.out.join("report.md"), &md))
        .and_then(|_| std::fs::write(opts.out.join("checks.csv"), checks_csv(&checks)))
        .and_then(|_| match &eco_csv {
            Some(c) => std::fs::write(opts.out.join("ecology.csv"), c),
            None => Ok(()),
        })
    {
        eprintln!("error: writing the report to {}: {e}", opts.out.display());
        return ExitCode::FAILURE;
    }
    print_table(&checks);
    let failed = checks.iter().filter(|c| c.verdict == Verdict::Fail).count();
    println!(
        "\n{} checks: {} strict passed, {failed} failed, {} soft passed, {} soft warnings ({total:.1} s). Report: {}",
        checks.len(),
        checks.iter().filter(|c| c.verdict == Verdict::Pass).count(),
        checks.iter().filter(|c| c.verdict == Verdict::SoftPass).count(),
        checks.iter().filter(|c| c.verdict == Verdict::SoftFail).count(),
        opts.out.join("report.md").display()
    );
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn print_table(checks: &[Check]) {
    for c in checks {
        println!(
            "{:<11} {:<8} {:<26} {:<58} expected {:<14} observed {:<22} CI {}",
            c.verdict.label(),
            c.level,
            c.group,
            c.name,
            c.expected,
            c.observed,
            c.ci
        );
    }
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn checks_csv(checks: &[Check]) -> String {
    let mut s = String::from("level,group,check,expected,observed,ci,verdict,detail\n");
    for c in checks {
        let row = [c.level, c.group, &c.name, &c.expected, &c.observed, &c.ci, c.verdict.label(), &c.detail];
        let _ = writeln!(s, "{}", row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
    }
    s
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn checks_table(checks: &[Check]) -> String {
    let mut s = String::from(
        "| Level | Group | Check | Expected | Observed | CI (99.9%) | Result |\n|---|---|---|---|---|---|---|\n",
    );
    for c in checks {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} | {} |",
            c.level,
            md_cell(c.group),
            md_cell(&c.name),
            md_cell(&c.expected),
            md_cell(&c.observed),
            md_cell(&c.ci),
            if c.verdict == Verdict::Fail { "**FAIL**".to_string() } else { c.verdict.label().to_string() }
        );
    }
    s
}

fn report_markdown(
    opts: &Opts,
    checks: &[Check],
    sections: &[(String, String)],
    timings: &[(&str, f64)],
    total: f64,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Genetics validation report\n");
    let _ = writeln!(
        s,
        "Profile `{}`, base seed {}, levels: {}{}. Runtime {:.1} s ({}).\n",
        opts.profile.name,
        opts.seed,
        if opts.genome { "genome " } else { "" },
        if opts.ecology {
            format!(
                "ecology ({} worlds, {}×{}, {} years each)",
                opts.eco.seeds, opts.eco.size, opts.eco.size, opts.eco.years
            )
        } else {
            String::new()
        },
        total,
        timings.iter().map(|(n, t)| format!("{n} {t:.1} s")).collect::<Vec<_>>().join(", ")
    );
    let failed = checks.iter().filter(|c| c.verdict == Verdict::Fail).count();
    let _ = writeln!(
        s,
        "**{} strict checks passed, {failed} failed; {} soft checks passed, {} soft warnings; {} informational rows.**\n",
        checks.iter().filter(|c| c.verdict == Verdict::Pass).count(),
        checks.iter().filter(|c| c.verdict == Verdict::SoftPass).count(),
        checks.iter().filter(|c| c.verdict == Verdict::SoftFail).count(),
        checks.iter().filter(|c| c.verdict == Verdict::Info).count(),
    );
    let _ = writeln!(s, "## Results\n\n{}", checks_table(checks));
    let _ = writeln!(s, "## Check details\n");
    for c in checks {
        let _ = writeln!(s, "- **{}: {}** ({}): {}", c.group, c.name, c.verdict.label(), c.detail);
    }
    s.push('\n');
    for (title, body) in sections {
        let _ = writeln!(s, "## {title}\n\n{body}\n");
    }
    s
}

/// A crude ASCII sparkline plot of observed against expected series.
pub fn ascii_plot(title: &str, xs: &[f64], series: &[(&str, char, Vec<f64>)], height: usize) -> String {
    let width = xs.len();
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (_, _, ys) in series {
        for &y in ys.iter().filter(|y| y.is_finite()) {
            lo = lo.min(y);
            hi = hi.max(y);
        }
    }
    if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) {
        hi = lo + 1.0;
    }
    let mut grid = vec![vec![' '; width]; height];
    for (_, ch, ys) in series {
        for (x, &y) in ys.iter().enumerate().take(width) {
            if !y.is_finite() {
                continue;
            }
            let r = ((hi - y) / (hi - lo) * (height - 1) as f64).round() as usize;
            let cell = &mut grid[r.min(height - 1)][x];
            *cell = if *cell == ' ' || *cell == *ch { *ch } else { '*' };
        }
    }
    let mut s = format!("```text\n{title}\n");
    for (r, row) in grid.iter().enumerate() {
        let label = if r == 0 {
            num(hi)
        } else if r == height - 1 {
            num(lo)
        } else {
            String::new()
        };
        let _ = writeln!(s, "{label:>9} |{}", row.iter().collect::<String>());
    }
    let _ = writeln!(
        s,
        "{:>9} +{}\n{:>9}  {} … {}",
        "",
        "-".repeat(width),
        "",
        num(*xs.first().unwrap_or(&0.0)),
        num(*xs.last().unwrap_or(&0.0))
    );
    let legend: Vec<String> = series.iter().map(|(n, c, _)| format!("{c} {n}")).collect();
    let _ = writeln!(s, "{:>9}  {} ('*' overlap)\n```", "", legend.join(", "));
    s
}
