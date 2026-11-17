//! Predators over the years: `cargo run --release --example predlong -- [years] [seeds..] [--every DAYS] [--set NAME=V] [--off] [--meat] [--meat-only] [--founding SPEC]`.
//!
//! Across machines: add `--shard i/n` (and optionally `--shard-out FILE`) on
//! machine i of n to run every n-th seed and write its results to
//! `predlong-shard-i-of-n.json`; then `predlong --merge FILES..` prints the
//! report of the unsharded run (README, *Running experiments across
//! machines*).
//!
//! Runs the standard world (96×96) on each seed (seeds in parallel; default
//! 3 years on seeds 31–34) and follows the red fox (lineage 3) and the pack
//! hunters (lineage 4). Every `--every` days (default 60), per predator: head
//! count and adults, births, mean hunger, deaths by cause since the last
//! line, adult females and males (pregnant, able to breed, mean breeding
//! drive), kills by prey lineage and the share of each action; then the
//! prey head counts. Each seed ends with the adults' time budget by sex and
//! state (pregnant; "mother" while she has unweaned young), sampled every 6
//! ticks with mean hunger and thirst, its deaths by sex, stage, cause and
//! pregnancy, and the day each predator died out. The last lines pool
//! deaths by cause and count the seeds each predator survived. `--off`
//! runs `Balance::morphology_off()`, `--meat` the carnivore-diet rules and
//! predator brakes (`Balance::carnivore_diet()`, on by default from
//! snapshot version 31) with the predators in their niches
//! (`presets::niche_world`; `--meat-only` keeps the standard designs,
//! `--niches` founds the niche designs without changing the rules),
//! `--food` the food values (`Balance::food_values`, also on by default)
//! and `--classic` the rules of before version 31 (both off). Each seed
//! also reports adult females and males of
//! each predator at the end, every prey lineage's lowest head count, and the
//! predators' mean expressed values of the genes the rules read at the start
//! and the end (with their mean drift over the surviving seeds), to see
//! whether selection acts on them. `--founding SPEC` founds the niche world
//! as `presets::NicheFounding::parse(SPEC)` says (implies `--meat`; *First-year
//! prey boom: founding*). Read only: the diagnosis of
//! *Predators over the years* (README, Balance).

use std::collections::BTreeMap;

use evo_sim::organism::DeathCause;
use evo_sim::{presets, Balance, Gene, WorldConfig};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

const PRED: [u32; 2] = [3, 4];

/// Genes whose predator means are reported at the start and the end (the
/// genes the carnivore-diet rules and the predator brakes read).
const GENES: [Gene; 9] = [
    Gene::Carnivory,
    Gene::BodySize,
    Gene::FatStores,
    Gene::LitterSize,
    Gene::Pickiness,
    Gene::Aggression,
    Gene::GroupTolerance,
    Gene::PackInstinct,
    Gene::Nose,
];

/// Mean expressed values of [`GENES`] over a lineage (None if none live).
type GeneMeans = Option<[f32; 9]>;

/// Mean expressed value of each of [`GENES`] over lineage `l`'s living
/// members (None if none live).
fn gene_means(w: &evo_sim::World, l: u32) -> GeneMeans {
    let alive: Vec<_> = w.organisms.iter().filter(|o| o.alive && o.lineage == l).collect();
    if alive.is_empty() {
        return None;
    }
    let mut m = [0.0; 9];
    for (k, &g) in GENES.iter().enumerate() {
        m[k] = alive.iter().map(|o| o.pheno[g]).sum::<f32>() / alive.len() as f32;
    }
    Some(m)
}

/// One seed's result.
#[derive(Serialize, Deserialize)]
struct SeedResult {
    seed: u64,
    log: String,
    /// The day each predator died out.
    extinct: [Option<u32>; 2],
    #[serde(with = "deaths_file")]
    deaths: BTreeMap<(u32, DeathCause), u64>,
    born: [u64; 2],
    alive: [usize; 2],
    /// Adult females and males at the end, per predator.
    sexes: [(usize, usize); 2],
    /// Lowest head count of each prey lineage (0, 1, 2, 5, 6, 7) over the run.
    prey_min: [usize; 6],
    /// Gene means per predator at the start and the end.
    genes: [(GeneMeans, GeneMeans); 2],
}

const PREY: [usize; 6] = [0, 1, 2, 5, 6, 7];
const PREY_NAMES: [&str; 6] = ["vole", "antelope", "newt", "herd", "frog", "omni"];

/// Deaths by `(lineage, cause)` in a results file: `[lineage, cause index
/// in DeathCause::ALL, count]` triples.
mod deaths_file {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(m: &BTreeMap<(u32, DeathCause), u64>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(
            m.iter().map(|(&(l, c), &n)| (l, DeathCause::ALL.iter().position(|&x| x == c).expect("listed cause"), n)),
        )
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<(u32, DeathCause), u64>, D::Error> {
        let v: Vec<(u32, usize, u64)> = Vec::deserialize(d)?;
        v.into_iter()
            .map(|(l, c, n)| {
                let cause = DeathCause::ALL.get(c).ok_or_else(|| serde::de::Error::custom("unknown death cause"))?;
                Ok(((l, *cause), n))
            })
            .collect()
    }
}

/// A shard's results file (the format of `evo-sim`'s partial results).
#[derive(Serialize, Deserialize)]
struct Partial {
    format: String,
    version: u32,
    /// The command line without the sharding flags, `predlong` first.
    argv: Vec<String>,
    shard: Option<String>,
    complete: bool,
    /// `predlong seed=N` → that seed's result.
    results: BTreeMap<String, SeedResult>,
}

struct Opts {
    balance: Balance,
    every: u32,
    /// The predators in their niches (`presets::niche_world_with`).
    niches: bool,
    years: u32,
    seeds: Vec<u64>,
    founding: presets::NicheFounding,
}

fn parse(argv: &[String]) -> Opts {
    let mut balance = Balance::default();
    let mut pos: Vec<String> = Vec::new();
    let mut every = 60u32;
    let mut niches = false;
    let mut founding = presets::NicheFounding::default();
    let mut args = argv.iter().cloned();
    while let Some(a) = args.next() {
        if a == "--set" {
            let s = args.next().expect("--set NAME=V");
            let (n, v) = s.split_once('=').expect("--set NAME=V");
            assert!(balance.set(n, v.parse().unwrap()), "unknown parameter {n}");
        } else if a == "--off" {
            balance = balance.morphology_off();
        } else if a == "--meat" {
            balance = balance.carnivore_diet();
            niches = true;
        } else if a == "--founding" {
            founding = presets::NicheFounding::parse(&args.next().expect("--founding SPEC")).expect("founding spec");
            balance = balance.carnivore_diet();
            niches = true;
        } else if a == "--meat-only" {
            balance = balance.carnivore_diet();
        } else if a == "--food" {
            balance = balance.food_values();
        } else if a == "--classic" {
            balance = balance.carnivore_diet_off().food_values_off();
        } else if a == "--niches" {
            niches = true;
        } else if a == "--every" {
            every = args.next().expect("--every DAYS").parse().unwrap();
        } else {
            pos.push(a);
        }
    }
    let years: u32 = pos.first().and_then(|s| s.parse().ok()).unwrap_or(3);
    let seeds: Vec<u64> =
        if pos.len() > 1 { pos[1..].iter().map(|s| s.parse().unwrap()).collect() } else { (31..=34).collect() };
    Opts { balance, every, niches, years, seeds, founding }
}

fn seed_key(seed: u64) -> String {
    format!("predlong seed={seed}")
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("--merge") {
        merge(&raw[1..]);
        return;
    }
    // `--shard i/n` (1-based) runs the seeds at positions j with j % n == i - 1.
    let mut shard: Option<(usize, usize)> = None;
    let mut shard_out = None;
    let mut plan = vec!["predlong".to_string()];
    let mut it = raw.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--shard" => {
                let v = it.next().expect("--shard i/n");
                let (i, n) = v.split_once('/').expect("--shard i/n");
                let (i, n): (usize, usize) = (i.parse().expect("--shard i/n"), n.parse().expect("--shard i/n"));
                assert!(n > 0 && (1..=n).contains(&i), "--shard i/n needs 1 <= i <= n");
                shard = Some((i - 1, n));
            }
            "--shard-out" => shard_out = Some(it.next().expect("--shard-out FILE")),
            _ => plan.push(a),
        }
    }
    let opts = parse(&plan[1..]);
    let mine: Vec<u64> =
        opts.seeds.iter().enumerate().filter(|(j, _)| shard.is_none_or(|(i, n)| j % n == i)).map(|(_, &s)| s).collect();
    let results: Vec<SeedResult> = mine.par_iter().map(|&seed| run_seed(&opts, seed)).collect();
    match shard {
        None => report(&opts, &results),
        Some((i, n)) => {
            let path = shard_out.unwrap_or_else(|| format!("predlong-shard-{}-of-{n}.json", i + 1));
            let p = Partial {
                format: "evo-sim-partial".into(),
                version: 1,
                argv: plan,
                shard: Some(format!("{}/{n}", i + 1)),
                complete: true,
                results: results.into_iter().map(|r| (seed_key(r.seed), r)).collect(),
            };
            std::fs::write(&path, serde_json::to_string(&p).expect("serializable")).expect("write the results file");
            eprintln!(
                "shard {}/{n}: seeds {mine:?} written to {path}; when every shard is done: \
                 cargo run --release --example predlong -- --merge <every shard's file>",
                i + 1
            );
        }
    }
}

/// `--merge FILES..`: the report of the unsharded run from shards' files.
fn merge(files: &[String]) {
    assert!(!files.is_empty(), "--merge needs the shards' results files");
    let mut argv: Option<Vec<String>> = None;
    let mut all: BTreeMap<String, SeedResult> = BTreeMap::new();
    for f in files {
        let text = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("could not read {f}: {e}"));
        let p: Partial = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{f}: not a results file: {e}"));
        assert!(
            p.format == "evo-sim-partial" && p.argv.first().map(String::as_str) == Some("predlong"),
            "{f}: not a predlong results file"
        );
        match &argv {
            None => argv = Some(p.argv.clone()),
            Some(a) => assert!(*a == p.argv, "{f} is from another command line ({:?} vs {a:?})", p.argv),
        }
        all.extend(p.results);
    }
    let argv = argv.expect("at least one file");
    let opts = parse(&argv[1..]);
    let missing: Vec<u64> = opts.seeds.iter().copied().filter(|&s| !all.contains_key(&seed_key(s))).collect();
    assert!(missing.is_empty(), "no results for seeds {missing:?}: merge every shard's file");
    let results: Vec<SeedResult> =
        opts.seeds.iter().map(|&s| all.remove(&seed_key(s)).expect("checked above")).collect();
    report(&opts, &results);
}

/// Runs one seed for `opts.years` years.
fn run_seed(opts: &Opts, seed: u64) -> SeedResult {
    let (balance, every, years) = (opts.balance, opts.every, opts.years);
    let config = WorldConfig { seed, width: 96, height: 96, balance, ..Default::default() };
    let mut w =
        if opts.niches { presets::niche_world_with(config, 1, &opts.founding) } else { presets::standard_world(config, 1) };
    let genes0 = [gene_means(&w, 3), gene_means(&w, 4)];
    let mut prey_min = [usize::MAX; 6];
    let tpd = w.config.ticks_per_day;
    let days = years * w.config.days_per_year;
    let mut log = String::new();
    let mut extinct = [None; 2];
    let mut last_deaths = w.counters.deaths_by_lineage.clone();
    let mut last_born = [0u64; 2];
    let mut last_kills = w.counters.kills_by_lineage.clone();
    let mut acts: [BTreeMap<String, u64>; 2] = Default::default();
    let mut sexdeaths: BTreeMap<String, u64> = BTreeMap::new();
    let mut sexacts: BTreeMap<String, (BTreeMap<String, u64>, f64, f64, u64)> = BTreeMap::new();
    for day in 1..=days {
        for t in 0..tpd {
            w.step();
            if t % 6 == 0 {
                let mothers: std::collections::HashSet<evo_sim::organism::OrgId> = w
                    .organisms
                    .iter()
                    .filter(|o| o.alive && !o.matured && o.growth < 0.7 && PRED.contains(&o.lineage))
                    .filter_map(|o| o.mother)
                    .collect();
                for o in w.organisms.iter().filter(|o| o.alive && PRED.contains(&o.lineage)) {
                    *acts[(o.lineage - 3) as usize].entry(format!("{:?}", o.action)).or_default() += 1;
                    if o.matured {
                        let k = format!(
                            "{} {:?}{}",
                            if o.lineage == 3 { "fox" } else { "pack" },
                            o.sex,
                            if o.pregnancy.is_some() { " pregnant" } else { "" }
                        ) + if mothers.contains(&o.id) { " mother" } else { "" };
                        let e = sexacts.entry(k).or_insert_with(|| (BTreeMap::new(), 0.0, 0.0, 0));
                        *e.0.entry(format!("{:?}", o.action)).or_default() += 1;
                        e.1 += o.needs.hunger as f64;
                        e.2 += o.needs.thirst as f64;
                        e.3 += 1;
                    }
                }
            }
        }
        for o in w.organisms.iter().filter(|o| !o.alive && PRED.contains(&o.lineage)) {
            let key = format!(
                "{} {} {:?} {:?}{}",
                if o.lineage == 3 { "fox" } else { "pack" },
                if o.matured { "adult" } else { "young" },
                o.sex,
                o.death,
                if o.pregnancy.is_some() { " pregnant" } else { "" }
            );
            *sexdeaths.entry(key).or_default() += 1;
        }
        let counts = w.lineage_counts();
        for (k, &l) in PREY.iter().enumerate() {
            prey_min[k] = prey_min[k].min(counts[l].alive);
        }
        for (k, &l) in PRED.iter().enumerate() {
            if extinct[k].is_none() && counts[l as usize].alive == 0 {
                extinct[k] = Some(day);
            }
        }
        if day % every == 0 || day == days {
            let mut line = format!("seed {seed} day {day:5} {:?}:", w.season());
            for (k, &l) in PRED.iter().enumerate() {
                let born = w.counters.born.get(&l).copied().unwrap_or(0);
                let hunger: Vec<f32> =
                    w.organisms.iter().filter(|o| o.alive && o.lineage == l).map(|o| o.needs.hunger).collect();
                let mh = if hunger.is_empty() { 0.0 } else { hunger.iter().sum::<f32>() / hunger.len() as f32 };
                let mut d = String::new();
                for c in DeathCause::ALL {
                    let n = w.counters.deaths_by_lineage.get(&(l, c)).copied().unwrap_or(0)
                        - last_deaths.get(&(l, c)).copied().unwrap_or(0);
                    if n > 0 {
                        d += &format!(" {c:?}={n}");
                    }
                }
                line += &format!(
                    " | {} {}/{}a +{} h{:.0}{}",
                    if l == 3 { "fox" } else { "pack" },
                    counts[l as usize].alive,
                    counts[l as usize].adults,
                    born - last_born[k],
                    mh,
                    d
                );
                last_born[k] = born;
                let tick = w.tick();
                let ad: Vec<&evo_sim::organism::Organism> =
                    w.organisms.iter().filter(|o| o.alive && o.lineage == l && o.matured).collect();
                let fem = ad.iter().filter(|o| o.sex == evo_sim::organism::Sex::Female).count();
                let preg = ad.iter().filter(|o| o.pregnancy.is_some()).count();
                let cb = ad.iter().filter(|o| o.can_breed(tick)).count();
                let drive = ad.iter().map(|o| o.needs.breeding).sum::<f32>() / ad.len().max(1) as f32;
                line += &format!(" [F{fem} M{} preg{preg} canbreed{cb} drive{drive:.0}]", ad.len() - fem);
                let mut kills = String::new();
                for v in 0..w.lineages.len() as u32 {
                    let n = w.counters.kills_by_lineage.get(&(v, l)).copied().unwrap_or(0)
                        - last_kills.get(&(v, l)).copied().unwrap_or(0);
                    if n > 0 {
                        kills += &format!(" {v}:{n}");
                    }
                }
                let tot: u64 = acts[k].values().sum();
                let mut a: Vec<_> = acts[k].iter().collect();
                a.sort_by(|x, y| y.1.cmp(x.1));
                let a: Vec<String> = a
                    .iter()
                    .take(5)
                    .map(|(n, c)| format!("{n} {:.0}%", 100.0 * **c as f64 / tot.max(1) as f64))
                    .collect();
                line += &format!(" kills[{kills} ] acts[{}]", a.join(" "));
                acts[k].clear();
            }
            last_kills = w.counters.kills_by_lineage.clone();
            line += &format!(
                " || prey vole {} ant {} newt {} herd {} frog {} omni {}",
                counts[0].alive, counts[1].alive, counts[2].alive, counts[5].alive, counts[6].alive, counts[7].alive
            );
            log += &line;
            log.push('\n');
            last_deaths = w.counters.deaths_by_lineage.clone();
        }
    }
    for (k, (a, h, t, n)) in &sexacts {
        let mut v: Vec<_> = a.iter().collect();
        v.sort_by(|x, y| y.1.cmp(x.1));
        let v: Vec<String> =
            v.iter().take(7).map(|(x, c)| format!("{x} {:.0}%", 100.0 * **c as f64 / *n as f64)).collect();
        log += &format!(
            "seed {seed} budget {k}: samples {n} hunger {:.0} thirst {:.0} [{}]\n",
            h / *n as f64,
            t / *n as f64,
            v.join(" ")
        );
    }
    for (k, n) in &sexdeaths {
        log += &format!("seed {seed} deaths {k}: {n}\n");
    }
    let counts = w.lineage_counts();
    let born = [3u32, 4].map(|l| w.counters.born.get(&l).copied().unwrap_or(0));
    let sexes = [3u32, 4].map(|l| {
        let ad = w.organisms.iter().filter(|o| o.alive && o.lineage == l && o.matured);
        let f = ad.clone().filter(|o| o.sex == evo_sim::organism::Sex::Female).count();
        (f, ad.count() - f)
    });
    SeedResult {
        seed,
        log,
        extinct,
        deaths: w.counters.deaths_by_lineage.clone(),
        born,
        alive: [counts[3].alive, counts[4].alive],
        sexes,
        prey_min,
        genes: [(genes0[0], gene_means(&w, 3)), (genes0[1], gene_means(&w, 4))],
    }
}

/// The per-seed lines and the pooled summary.
fn report(opts: &Opts, results: &[SeedResult]) {
    let years = opts.years;
    let mut pooled: BTreeMap<(u32, DeathCause), u64> = BTreeMap::new();
    let mut survived = [0; 2];
    let mut viable = [0; 2];
    let mut prey_lost = 0;
    // Gene drift: summed (end − start) per predator over seeds where it lives.
    let mut drift = [([0.0f32; 9], 0usize); 2];
    for r in results {
        let (seed, extinct, alive, born) = (r.seed, r.extinct, r.alive, r.born);
        print!("{}", r.log);
        println!(
            "seed {seed}: fox {} (born {}, F{} M{}), pack {} (born {}, F{} M{})",
            extinct[0].map_or(format!("alive {}", alive[0]), |d| format!("extinct day {d}")),
            born[0],
            r.sexes[0].0,
            r.sexes[0].1,
            extinct[1].map_or(format!("alive {}", alive[1]), |d| format!("extinct day {d}")),
            born[1],
            r.sexes[1].0,
            r.sexes[1].1,
        );
        let lows: Vec<String> = PREY_NAMES.iter().zip(r.prey_min).map(|(n, m)| format!("{n} {m}")).collect();
        println!("seed {seed}: prey lowest {}", lows.join(", "));
        prey_lost += usize::from(r.prey_min.contains(&0));
        for k in 0..2 {
            if extinct[k].is_none() {
                survived[k] += 1;
            }
            if r.sexes[k].0 > 0 && r.sexes[k].1 > 0 {
                viable[k] += 1;
            }
            if let (Some(a), Some(b)) = r.genes[k] {
                let line: Vec<String> = GENES
                    .iter()
                    .zip(a.iter().zip(b))
                    .map(|(g, (x, y))| format!("{} {x:.3}→{y:.3}", g.label()))
                    .collect();
                println!("seed {seed}: {} genes {}", if k == 0 { "fox" } else { "pack" }, line.join(", "));
                for (d, (x, y)) in drift[k].0.iter_mut().zip(a.iter().zip(b)) {
                    *d += y - x;
                }
                drift[k].1 += 1;
            }
        }
        println!();
        for (&key, &n) in &r.deaths {
            if PRED.contains(&key.0) {
                *pooled.entry(key).or_default() += n;
            }
        }
    }
    for (&(l, c), n) in &pooled {
        println!("pooled deaths {} {c:?}: {n}", if l == 3 { "fox" } else { "pack" });
    }
    for (k, (d, n)) in drift.iter().enumerate() {
        if *n > 0 {
            let line: Vec<String> =
                GENES.iter().zip(d).map(|(g, x)| format!("{} {:+.3}", g.label(), x / *n as f32)).collect();
            println!("mean gene drift {} ({n} seeds): {}", if k == 0 { "fox" } else { "pack" }, line.join(", "));
        }
    }
    println!(
        "survived {years} years: fox {}/{} (viable {}), pack hunters {}/{} (viable {}); seeds with a prey lost: {}",
        survived[0],
        results.len(),
        viable[0],
        survived[1],
        results.len(),
        viable[1],
        prey_lost
    );
}
