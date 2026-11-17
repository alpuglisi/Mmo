//! Predators over the years: `cargo run --release --example predlong -- [years] [seeds..] [--every DAYS] [--set NAME=V] [--off] [--meat] [--meat-only]`.
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
//! predator brakes (`Balance::carnivore_diet()`, off by default) with the
//! predators in their niches (`presets::niche_world`; `--meat-only` keeps
//! the standard designs; after `--meat`, `--std-fox` and `--std-pack` keep
//! that predator's standard design), `--food` the food values
//! (`Balance::food_values`). Each seed also reports adult females and males of
//! each predator at the end, every prey lineage's lowest head count, and the
//! predators' mean expressed values of the genes the rules read at the start
//! and the end (with their mean drift over the surviving seeds), to see
//! whether selection acts on them. Read only: the diagnosis of
//! *Predators over the years* (README, Balance).

use std::collections::BTreeMap;

use evo_sim::organism::DeathCause;
use evo_sim::{presets, Balance, Gene, WorldConfig};
use rayon::prelude::*;

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
struct SeedResult {
    seed: u64,
    log: String,
    /// The day each predator died out.
    extinct: [Option<u32>; 2],
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

fn main() {
    let mut balance = Balance::default();
    let mut pos: Vec<String> = Vec::new();
    let mut every = 60u32;
    let (mut niche_fox, mut niche_pack) = (false, false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--set" {
            let s = args.next().expect("--set NAME=V");
            let (n, v) = s.split_once('=').expect("--set NAME=V");
            assert!(balance.set(n, v.parse().unwrap()), "unknown parameter {n}");
        } else if a == "--off" {
            balance = balance.morphology_off();
        } else if a == "--meat" {
            balance = balance.carnivore_diet();
            (niche_fox, niche_pack) = (true, true);
        } else if a == "--meat-only" {
            balance = balance.carnivore_diet();
        } else if a == "--food" {
            balance = balance.food_values();
        } else if a == "--std-fox" {
            niche_fox = false;
        } else if a == "--std-pack" {
            niche_pack = false;
        } else if a == "--every" {
            every = args.next().expect("--every DAYS").parse().unwrap();
        } else {
            pos.push(a);
        }
    }
    let years: u32 = pos.first().and_then(|s| s.parse().ok()).unwrap_or(3);
    let seeds: Vec<u64> =
        if pos.len() > 1 { pos[1..].iter().map(|s| s.parse().unwrap()).collect() } else { (31..=34).collect() };

    let results: Vec<SeedResult> = seeds
        .par_iter()
        .map(|&seed| {
            let config = WorldConfig { seed, width: 96, height: 96, balance, ..Default::default() };
            let fox = if niche_fox { presets::niche_wild_designs() } else { presets::wild_designs() };
            let pack = if niche_pack { presets::niche_player_designs() } else { presets::player_designs() };
            let mut w = presets::standard_world_from(config, 1, fox, pack);
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
                        counts[0].alive,
                        counts[1].alive,
                        counts[2].alive,
                        counts[5].alive,
                        counts[6].alive,
                        counts[7].alive
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
        })
        .collect();

    let mut pooled: BTreeMap<(u32, DeathCause), u64> = BTreeMap::new();
    let mut survived = [0; 2];
    let mut viable = [0; 2];
    let mut prey_lost = 0;
    // Gene drift: summed (end − start) per predator over seeds where it lives.
    let mut drift = [([0.0f32; 9], 0usize); 2];
    for r in &results {
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
