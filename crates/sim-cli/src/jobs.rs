//! Splitting an experiment across machines: `--shard i/n`, `--cache FILE`
//! and `evo-sim merge`.
//!
//! Every experiment (sweep, compare, compare --adaptive, rankcheck,
//! genetics) runs its worlds as batches of independent jobs through
//! [`run`]. A job has a key that names everything its result depends on (the
//! balance, seed, map size, days, designs …), so a result computed on one
//! machine can stand in for the same job on another.
//!
//! - Without `--shard` a batch runs entirely here, every job in parallel
//!   across all cores (results already known from `--cache` files are reused).
//! - With `--shard i/n` this process runs only the jobs at positions `j` of
//!   each batch with `j % n == i - 1`. If the batch is then complete (the
//!   other jobs came from `--cache`), the command carries on; otherwise it
//!   writes everything it knows to a partial-results file (JSON) and stops.
//! - `evo-sim merge FILES..` reads partial-results files of the same command
//!   line and replays that command with every job's result taken from them:
//!   the report is the one an unsharded run prints. A command with several
//!   dependent stages (a sweep's rounds and screens, an adaptive compare
//!   look by look) cannot finish from the first stage's results: merge then
//!   writes the merged results and prints the command for the next stage
//!   (the same shards with `--cache merged.json`).
//!
//! Every machine must run the same build: a key does not capture the rules.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rayon::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Exit status of `evo-sim merge` when the results only cover a first
/// stage of a multi-stage command (the next stage's command is printed).
pub const EXIT_NEXT_STAGE: u8 = 3;

/// This process's slice of every batch: positions `j` with
/// `j % count == index`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shard {
    /// 0-based (`--shard 1/4` is index 0).
    pub index: usize,
    pub count: usize,
}

impl Shard {
    /// Parses `i/n` with `1 <= i <= n`.
    pub fn parse(s: &str) -> Result<Shard, String> {
        let bad = || format!("--shard: expected i/n with 1 <= i <= n (e.g. 2/4), got '{s}'");
        let (i, n) = s.split_once('/').ok_or_else(bad)?;
        let (i, n): (usize, usize) = (i.trim().parse().map_err(|_| bad())?, n.trim().parse().map_err(|_| bad())?);
        if n == 0 || i == 0 || i > n {
            return Err(bad());
        }
        Ok(Shard { index: i - 1, count: n })
    }

    pub fn owns(&self, position: usize) -> bool {
        position % self.count == self.index
    }

    pub fn label(&self) -> String {
        format!("{}/{}", self.index + 1, self.count)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    /// Run every job here.
    Local,
    /// Run this slice of every batch.
    Shard(Shard),
    /// `evo-sim merge`: every result must be known; nothing is simulated.
    Replay,
}

struct Ctx {
    mode: Mode,
    /// The command line being run, without the sharding flags.
    argv: Vec<String>,
    /// Where a shard writes its partial results (merge: the merged results).
    out: PathBuf,
    /// Adaptive compare under `--shard`: run look by look (one stage per
    /// look) instead of every seed up to the cap in one stage.
    stagewise: bool,
    /// Every result known: loaded from `--cache` / merge inputs, or computed.
    known: Mutex<BTreeMap<String, Value>>,
}

static CTX: OnceLock<Ctx> = OnceLock::new();

fn ctx() -> &'static Ctx {
    CTX.get_or_init(|| Ctx {
        mode: Mode::Local,
        argv: Vec::new(),
        out: PathBuf::new(),
        stagewise: false,
        known: Mutex::new(BTreeMap::new()),
    })
}

/// A partial-results file.
#[derive(Serialize, Deserialize)]
pub struct Partial {
    /// Always `evo-sim-partial`.
    pub format: String,
    pub version: u32,
    /// The command line (without the sharding flags) that produced it.
    pub argv: Vec<String>,
    /// `i/n` for a shard's file, absent for a merged one.
    pub shard: Option<String>,
    /// Whether the command ran to its end (the shard's last batch done).
    pub complete: bool,
    /// Job key → result.
    pub results: BTreeMap<String, Value>,
}

const FORMAT: &str = "evo-sim-partial";

impl Partial {
    pub fn read(path: &Path) -> Result<Partial, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let p: Partial =
            serde_json::from_str(&text).map_err(|e| format!("{}: not a partial-results file: {e}", path.display()))?;
        if p.format != FORMAT || p.version != 1 {
            return Err(format!("{}: not an evo-sim partial-results file (version 1)", path.display()));
        }
        Ok(p)
    }

    fn write(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
    }
}

/// Removes the sharding flags (`--shard i/n`, `--cache FILE` (repeatable),
/// `--shard-out FILE`, `--stagewise`) from `argv` (the arguments after the
/// program name), sets up this process accordingly and returns the rest.
pub fn init(argv: Vec<String>) -> Result<Vec<String>, String> {
    let mut rest = Vec::new();
    let mut shard = None;
    let mut caches = Vec::new();
    let mut out = None;
    let mut stagewise = false;
    let mut it = argv.into_iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--shard" => shard = Some(Shard::parse(&value()?)?),
            "--cache" => caches.push(PathBuf::from(value()?)),
            "--shard-out" => out = Some(PathBuf::from(value()?)),
            "--stagewise" => stagewise = true,
            _ => rest.push(a),
        }
    }
    let mut known = BTreeMap::new();
    for path in &caches {
        let p = Partial::read(path)?;
        absorb(&mut known, p.results, path)?;
    }
    let out = out.unwrap_or_else(|| match shard {
        Some(s) => PathBuf::from(format!("evo-sim-shard-{}-of-{}.json", s.index + 1, s.count)),
        None => PathBuf::new(),
    });
    let mode = shard.map_or(Mode::Local, Mode::Shard);
    install(Ctx { mode, argv: rest.clone(), out, stagewise, known: Mutex::new(known) });
    Ok(rest)
}

/// Adds `results` to `known`; the same key with a different result is an
/// error (the files come from different builds or command lines).
fn absorb(known: &mut BTreeMap<String, Value>, results: BTreeMap<String, Value>, from: &Path) -> Result<(), String> {
    for (k, v) in results {
        match known.get(&k) {
            Some(old) if *old != v => {
                return Err(format!(
                    "{}: a different result for a job another file also has ({k}); were they made by the same build?",
                    from.display()
                ))
            }
            _ => {
                known.insert(k, v);
            }
        }
    }
    Ok(())
}

fn install(c: Ctx) {
    if CTX.set(c).is_err() {
        panic!("jobs::init called twice");
    }
}

/// `evo-sim merge [--cache-out FILE] FILES..`: loads the partial-results
/// files and returns the command line to replay (every job's result must
/// then come from them).
pub fn init_merge(args: &[String]) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    let mut out = PathBuf::from("evo-sim-merged.json");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cache-out" => out = PathBuf::from(it.next().ok_or("--cache-out needs a value")?),
            f => files.push(PathBuf::from(f)),
        }
    }
    if files.is_empty() {
        return Err("merge: no partial-results files given".into());
    }
    let mut argv: Option<(Vec<String>, PathBuf)> = None;
    let mut known = BTreeMap::new();
    let mut shards: Vec<String> = Vec::new();
    for path in &files {
        let p = Partial::read(path)?;
        match &argv {
            None => argv = Some((p.argv.clone(), path.clone())),
            Some((a, first)) if *a != p.argv => {
                return Err(format!(
                    "{} is from `evo-sim {}` but {} from `evo-sim {}`: merge files of one command line",
                    path.display(),
                    quote(&p.argv),
                    first.display(),
                    quote(a)
                ))
            }
            _ => {}
        }
        if let Some(s) = p.shard {
            shards.push(s);
        }
        absorb(&mut known, p.results, path)?;
    }
    let (argv, _) = argv.expect("at least one file");
    if argv.first().map(String::as_str) == Some("predlong") {
        return Err(
            "these are the predlong example's results: cargo run --release --example predlong -- --merge FILES.."
                .into(),
        );
    }
    eprintln!(
        "merge: {} results from {} files{} · replaying `evo-sim {}`",
        known.len(),
        files.len(),
        if shards.is_empty() { String::new() } else { format!(" (shards {})", shards.join(", ")) },
        quote(&argv)
    );
    install(Ctx { mode: Mode::Replay, argv: argv.clone(), out, stagewise: false, known: Mutex::new(known) });
    Ok(argv)
}

/// A command line for printing (arguments with spaces quoted).
pub fn quote(argv: &[String]) -> String {
    argv.iter()
        .map(|a| if a.is_empty() || a.contains([' ', '"', '\'']) { format!("'{a}'") } else { a.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether this process runs one shard of the experiment.
pub fn sharded() -> bool {
    matches!(ctx().mode, Mode::Shard(_))
}

/// Adaptive compare under `--shard`: look by look (`--stagewise`).
pub fn stagewise() -> bool {
    ctx().stagewise
}

fn snapshot(complete: bool) -> Partial {
    let c = ctx();
    Partial {
        format: FORMAT.into(),
        version: 1,
        argv: c.argv.clone(),
        shard: match c.mode {
            Mode::Shard(s) => Some(s.label()),
            _ => None,
        },
        complete,
        results: c.known.lock().expect("results lock").clone(),
    }
}

/// Runs a batch of independent jobs: those with a known result are taken
/// from it, the rest run here in parallel (under `--shard`, only this
/// shard's positions). Returns `(result, extra)` per job in order; `extra`
/// is the job's local by-product (not saved), absent for a job whose result
/// came from elsewhere.
///
/// When the batch cannot be completed here (other shards' jobs, or a merge
/// that lacks a later stage) this writes the results file and ends the
/// process.
pub fn run_with<J, T, X>(
    jobs: Vec<J>,
    key: impl Fn(&J) -> String,
    work: impl Fn(J) -> (T, X) + Sync + Send,
) -> Vec<(T, Option<X>)>
where
    J: Send,
    T: Serialize + DeserializeOwned + Send,
    X: Send,
{
    let c = ctx();
    let keys: Vec<String> = jobs.iter().map(&key).collect();
    let mut out: Vec<Option<(T, Option<X>)>> = Vec::with_capacity(jobs.len());
    let mut todo = Vec::new();
    let mut missing = 0usize;
    {
        let known = c.known.lock().expect("results lock");
        for (j, (job, k)) in jobs.into_iter().zip(&keys).enumerate() {
            let cached = known.get(k).map(|v| {
                serde_json::from_value::<T>(v.clone())
                    .unwrap_or_else(|e| panic!("cached result of {k} unreadable: {e}"))
            });
            match cached {
                Some(t) => out.push(Some((t, None))),
                None => {
                    out.push(None);
                    match c.mode {
                        Mode::Replay => missing += 1,
                        Mode::Shard(s) if !s.owns(j) => missing += 1,
                        _ => todo.push((j, job)),
                    }
                }
            }
        }
    }
    if missing > 0 && c.mode == Mode::Replay {
        let merged = snapshot(false);
        if let Err(e) = merged.write(&c.out) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        eprintln!(
            "\nmerge: this command has another stage that needs results the files do not have ({missing} of a \
             batch of {}). Wrote the {} merged results to {}.\nRun the next stage on each machine i of n:\n    \
             evo-sim {} --shard i/n --cache {}\nthen merge their files again (they include the merged results).",
            keys.len(),
            merged.results.len(),
            c.out.display(),
            quote(&c.argv),
            c.out.display()
        );
        std::process::exit(EXIT_NEXT_STAGE as i32);
    }
    let done: Vec<(usize, T, X)> = todo
        .into_par_iter()
        .map(|(j, job)| {
            let (t, x) = work(job);
            (j, t, x)
        })
        .collect();
    let ran = done.len();
    {
        let mut known = c.known.lock().expect("results lock");
        for (j, t, _) in &done {
            let v = serde_json::to_value(t).expect("serializable result");
            // A result must read back (JSON has no NaN: such fields need `nan`).
            if let Err(e) = serde_json::from_value::<T>(v.clone()) {
                panic!("the result of {} does not survive its results file: {e}", keys[*j]);
            }
            known.insert(keys[*j].clone(), v);
        }
    }
    for (j, t, x) in done {
        out[j] = Some((t, Some(x)));
    }
    if missing > 0 {
        let Mode::Shard(s) = c.mode else { unreachable!("only shards and merges miss results") };
        let partial = snapshot(false);
        if let Err(e) = partial.write(&c.out) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        eprintln!(
            "\nshard {}: ran {} of this batch's {} jobs; the other shards run the rest. Wrote {} results to {}.\n\
             When every shard is done: evo-sim merge <every shard's file>",
            s.label(),
            ran,
            keys.len(),
            partial.results.len(),
            c.out.display()
        );
        std::process::exit(0);
    }
    out.into_iter().map(|o| o.expect("every job done")).collect()
}

/// [`run_with`] for jobs without a local by-product.
pub fn run<J, T>(jobs: Vec<J>, key: impl Fn(&J) -> String, work: impl Fn(J) -> T + Sync + Send) -> Vec<T>
where
    J: Send,
    T: Serialize + DeserializeOwned + Send,
{
    run_with(jobs, key, |j| (work(j), ())).into_iter().map(|(t, _)| t).collect()
}

/// At the end of a command: a shard writes its results file (the command
/// ran to its end because every batch was complete).
pub fn finish() -> Result<(), String> {
    let c = ctx();
    if let Mode::Shard(s) = c.mode {
        let partial = snapshot(true);
        partial.write(&c.out)?;
        eprintln!(
            "\nshard {}: done; wrote {} results to {}. Merge every shard's file: evo-sim merge FILES..",
            s.label(),
            partial.results.len(),
            c.out.display()
        );
    }
    Ok(())
}

/// A short stable hash of a value's `Debug` text (FNV-1a, 64 bits), for job
/// keys: a `Balance` or a design prints every field, floats exactly.
pub fn digest(v: &impl std::fmt::Debug) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("{v:?}").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// `#[serde(with = "crate::jobs::nan")]` for an `f64` that may be NaN or
/// infinite (JSON numbers cannot be): such values are written as strings.
pub mod nan {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(x: &f64, s: S) -> Result<S::Ok, S::Error> {
        if x.is_finite() {
            s.serialize_f64(*x)
        } else {
            s.serialize_str(&x.to_string())
        }
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Num {
        Number(f64),
        Text(String),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        match Num::deserialize(d)? {
            Num::Number(x) => Ok(x),
            Num::Text(t) => t.parse().map_err(serde::de::Error::custom),
        }
    }
}

/// [`nan`] for a `Vec<f64>`.
pub mod nan_vec {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct F(#[serde(with = "super::nan")] f64);

    pub fn serialize<S: Serializer>(v: &[f64], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(v.iter().map(|&x| F(x)))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f64>, D::Error> {
        Ok(Vec::<F>::deserialize(d)?.into_iter().map(|F(x)| x).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shards_parse_and_partition() {
        assert_eq!(Shard::parse("1/1"), Ok(Shard { index: 0, count: 1 }));
        assert_eq!(Shard::parse("3/4"), Ok(Shard { index: 2, count: 4 }));
        for bad in ["0/4", "5/4", "1/0", "x/2", "2", ""] {
            assert!(Shard::parse(bad).is_err(), "{bad}");
        }
        // Every position belongs to exactly one shard.
        let shards: Vec<Shard> = (1..=3).map(|i| Shard::parse(&format!("{i}/3")).unwrap()).collect();
        for j in 0..20 {
            assert_eq!(shards.iter().filter(|s| s.owns(j)).count(), 1);
        }
        assert_eq!(shards[1].label(), "2/3");
    }

    #[test]
    fn digests_tell_values_apart() {
        assert_eq!(digest(&1.5f32), digest(&1.5f32));
        assert_ne!(digest(&1.5f32), digest(&1.5000001f32));
        assert_eq!(digest(&"x").len(), 16);
    }

    #[test]
    fn local_batches_run_every_job_in_order() {
        let out = run((0..50u64).collect(), |j| format!("test-square {j}"), |j| j * j);
        assert_eq!(out, (0..50u64).map(|j| j * j).collect::<Vec<_>>());
        // A second batch with the same keys takes the known results.
        let again = run((0..50u64).collect(), |j| format!("test-square {j}"), |_| 0u64);
        assert_eq!(again, out);
    }

    #[test]
    fn non_finite_floats_survive_the_file() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct R {
            #[serde(with = "nan")]
            a: f64,
            #[serde(with = "nan_vec")]
            b: Vec<f64>,
        }
        let r = R { a: f64::INFINITY, b: vec![1.0 / 3.0, f64::NEG_INFINITY, 0.1 + 0.2] };
        let back: R = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        let n: R = serde_json::from_str(r#"{"a":"NaN","b":[]}"#).unwrap();
        assert!(n.a.is_nan());
    }

    #[test]
    fn conflicting_results_are_refused() {
        let mut known = BTreeMap::new();
        let a: BTreeMap<String, Value> = [("k".to_string(), Value::from(1))].into();
        let b: BTreeMap<String, Value> = [("k".to_string(), Value::from(2))].into();
        absorb(&mut known, a.clone(), Path::new("a")).unwrap();
        absorb(&mut known, a, Path::new("a")).unwrap();
        assert!(absorb(&mut known, b, Path::new("b")).is_err());
    }
}
