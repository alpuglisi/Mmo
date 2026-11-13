//! Point-cost calibration harness. Prints one CSV row per player lineage per
//! world, for analysis elsewhere (see "Point costs" in the README).
//!
//! ```text
//! cargo run --release --example calibrate -- marginal   [options]
//! cargo run --release --example calibrate -- tournament [options]
//! cargo run --release --example calibrate -- costs
//! ```
//!
//! - `marginal`: the four sample player designs are the templates. Each world
//!   founds the wild ecosystem plus one variant of every template: the design
//!   unchanged (`control`), one costed trait raised by `--delta` (capped at
//!   1), `--extra-founders` more founders, the litter one larger or smaller,
//!   or body size lowered by half of `--delta` (`smaller`). Options rotate across worlds so every template meets every
//!   option once per seed, next to a different mix of rivals. Variants are
//!   founded without the budget check, so the measured gain is the value of
//!   the change itself; `trait_cost` should be proportional to it.
//! - `tournament`: four random designs per world that each spend the whole
//!   budget under the current costs. Tests whether any archetype or trait
//!   wins out of proportion to its price.
//! - `costs`: print the current cost table.
//!
//! Options: `--seeds N` (marginal; seeds `--seed0`..), `--worlds N`
//! (tournament), `--days N`, `--size N`, `--delta F`, `--extra-founders N`,
//! `--seed0 N`, `--only NAME,...` (marginal options to run), `--set NAME=V`
//! (override a `Balance` parameter, repeatable), `--deaths` (print each
//! player lineage's deaths by cause to stderr: `deaths,seed,slot,cause=n,...`).
//!
//! Columns: `mode,seed,world,slot,template,option,delta,points,share_final,
//! share_mean,alive,adults,design`. `share_final` is the mean legacy share
//! over the last 10 days, `share_mean` over every day after day 10.

use evo_sim::design::{
    trait_cost, BASE_FOUNDERS, COST_COLD_BLOODED, COST_PER_EXTRA_FOUNDER, COST_PER_EXTRA_OFFSPRING, COST_PER_SMALLER_BODY,
    DESIGN_BUDGET, MAX_LITTER,
};
use evo_sim::rng::rng_for;
use evo_sim::organism::DeathCause;
use evo_sim::{presets, Balance, BirthType, FounderDesign, Gene, PlayerId, Thermo, World, WorldConfig};
use rand::seq::SliceRandom;
use rand::Rng;

struct Opts {
    seeds: u64,
    worlds: u64,
    days: u32,
    size: usize,
    delta: f32,
    extra_founders: u32,
    seed0: u64,
    only: Option<Vec<String>>,
    balance: Balance,
    deaths: bool,
}

fn parse(args: &[String]) -> Opts {
    let mut o = Opts { seeds: 4, worlds: 20, days: 90, size: 96, delta: 0.3, extra_founders: 16, seed0: 1, only: None, balance: Balance::default(), deaths: false };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| panic!("{a} needs a value")).clone();
        match a.as_str() {
            "--seeds" => o.seeds = v().parse().unwrap(),
            "--worlds" => o.worlds = v().parse().unwrap(),
            "--days" => o.days = v().parse().unwrap(),
            "--size" => o.size = v().parse().unwrap(),
            "--delta" => o.delta = v().parse().unwrap(),
            "--extra-founders" => o.extra_founders = v().parse().unwrap(),
            "--seed0" => o.seed0 = v().parse().unwrap(),
            "--only" => o.only = Some(v().split(',').map(str::to_string).collect()),
            "--deaths" => o.deaths = true,
            "--set" => {
                let s = v();
                let (name, x) = s.split_once('=').unwrap_or_else(|| panic!("--set expects NAME=VALUE, got {s}"));
                assert!(o.balance.set(name, x.parse().unwrap()), "unknown balance parameter {name}");
            }
            _ => panic!("unknown option {a}"),
        }
    }
    o
}

fn costed_genes() -> Vec<Gene> {
    Gene::ALL.iter().copied().filter(|&g| trait_cost(g) > 0.0).collect()
}

fn describe(d: &FounderDesign) -> String {
    let mut s = format!("{:?}/{:?}/{}x/{}f", d.thermo, d.birth, d.litter_size, d.founders);
    for &(g, v) in &d.traits {
        s.push_str(&format!(" {}={:.2}", g.label().replace(' ', "_"), v));
    }
    s
}

/// Run a world with the given player designs and return each player's
/// `(share_final, share_mean, alive, adults)`.
fn run(seed: u64, o: &Opts, designs: &[FounderDesign]) -> Vec<(f64, f64, usize, usize)> {
    let config = WorldConfig { seed, width: o.size, height: o.size, balance: o.balance, ..Default::default() };
    let mut world: World = presets::wild_world(config, 1);
    let first = world.lineages.len();
    for (k, d) in designs.iter().enumerate() {
        world.found_lineage_unchecked(k as PlayerId + 1, d);
    }
    world.run_days(o.days);
    if o.deaths {
        for k in 0..designs.len() {
            let mut line = format!("deaths,{seed},{k}");
            for cause in DeathCause::ALL {
                let n = world.counters.deaths_by_lineage.get(&((first + k) as u32, cause)).copied().unwrap_or(0);
                line.push_str(&format!(",{}={n}", cause.label()));
            }
            eprintln!("{line}");
        }
    }
    let counts = world.lineage_counts();
    let tail = world.legacy.len().saturating_sub(10);
    (0..designs.len())
        .map(|k| {
            let p = k as PlayerId + 1;
            let mean = |from: usize| {
                let s = &world.legacy[from.min(world.legacy.len())..];
                s.iter().map(|l| l.share(p)).sum::<f64>() / s.len().max(1) as f64
            };
            let c = counts[first + k];
            (mean(tail), mean(10), c.alive, c.adults)
        })
        .collect()
}

#[derive(Clone)]
enum Variant {
    Control,
    Trait(Gene),
    Founders,
    LitterUp,
    LitterDown,
    /// Body size lowered by half of `--delta` (shrinking is free).
    Smaller,
}

impl Variant {
    fn name(&self) -> String {
        match self {
            Variant::Control => "control".into(),
            Variant::Trait(g) => g.label().replace(' ', "_"),
            Variant::Founders => "founders".into(),
            Variant::LitterUp => "litter+1".into(),
            Variant::LitterDown => "litter-1".into(),
            Variant::Smaller => "smaller".into(),
        }
    }

    /// The changed design and the change in the varied quantity.
    fn apply(&self, d: &FounderDesign, o: &Opts) -> (FounderDesign, f32) {
        let v = match self {
            Variant::Control => d.clone(),
            Variant::Trait(g) => d.clone().with(*g, (d.get(*g) + o.delta).min(1.0)),
            Variant::Founders => d.clone().founders(d.founders + o.extra_founders),
            Variant::LitterUp => FounderDesign { litter_size: (d.litter_size + 1).min(MAX_LITTER), ..d.clone() },
            Variant::LitterDown => FounderDesign { litter_size: d.litter_size.saturating_sub(1).max(1), ..d.clone() },
            Variant::Smaller => d.clone().with(Gene::BodySize, (d.get(Gene::BodySize) - 0.5 * o.delta).max(0.02)),
        };
        let delta = match self {
            Variant::Control => 0.0,
            Variant::Trait(g) => v.get(*g) - d.get(*g),
            Variant::Smaller => v.get(Gene::BodySize) - d.get(Gene::BodySize),
            Variant::Founders => o.extra_founders as f32,
            Variant::LitterUp | Variant::LitterDown => v.litter_size as f32 - d.litter_size as f32,
        };
        (v, delta)
    }
}

fn marginal(o: &Opts) {
    let templates = presets::player_designs();
    let mut variants = vec![Variant::Control, Variant::Founders, Variant::LitterUp, Variant::LitterDown, Variant::Smaller];
    variants.extend(costed_genes().into_iter().map(Variant::Trait));
    if let Some(only) = &o.only {
        variants.retain(|v| only.contains(&v.name()));
    }
    let n = variants.len();
    for s in 0..o.seeds {
        let seed = o.seed0 + s;
        for w in 0..n {
            let picks: Vec<(FounderDesign, String, f32)> = templates
                .iter()
                .enumerate()
                .map(|(t, d)| {
                    let v = &variants[(w + t * (n / 4 + 1) + s as usize * 3) % n];
                    let (design, delta) = v.apply(d, o);
                    (design, v.name(), delta)
                })
                .collect();
            let designs: Vec<FounderDesign> = picks.iter().map(|p| p.0.clone()).collect();
            let out = run(seed, o, &designs);
            for (t, ((design, name, delta), r)) in picks.iter().zip(out).enumerate() {
                println!(
                    "marginal,{seed},{w},{t},{},{name},{delta:.3},{:.1},{:.5},{:.5},{},{},{}",
                    templates[t].name,
                    design.point_cost(),
                    r.0,
                    r.1,
                    r.2,
                    r.3,
                    describe(design)
                );
            }
        }
    }
}

/// A random design that spends the whole budget under the current costs.
fn random_design(rng: &mut impl Rng, name: String) -> FounderDesign {
    use Gene::*;
    let thermo = if rng.gen_bool(0.5) { Thermo::Warm } else { Thermo::Cold };
    let birth = *[BirthType::Live, BirthType::IncubatedEggs, BirthType::AbandonedEggs].choose(rng).unwrap();
    let litter = rng.gen_range(1..=MAX_LITTER);
    let founders = rng.gen_range(10..=60);
    let mut d = FounderDesign::new(name, thermo, birth, litter).founders(founders);
    d = d.with(Carnivory, *[0.0, 0.1, 0.5, 0.85].choose(rng).unwrap());
    for g in [Boldness, Aggression, Wanderlust, GroupTolerance, ForagingRange, PackInstinct, Pickiness] {
        d = d.with(g, rng.gen_range(0.1..0.9));
    }
    d = d.with(BodySize, rng.gen_range(0.05..0.75));
    // Shed founders, then litter, until the fixed choices fit.
    while d.point_cost() > DESIGN_BUDGET * 0.7 && d.founders > BASE_FOUNDERS {
        d.founders -= 1;
    }
    while d.point_cost() > DESIGN_BUDGET * 0.7 && d.litter_size > 1 {
        d.litter_size -= 1;
    }
    // Move body size toward its default in case either side is priced.
    while d.point_cost() > DESIGN_BUDGET * 0.7 && (d.get(BodySize) - 0.3).abs() > 0.03 {
        let v = d.get(BodySize) - 0.05 * (d.get(BodySize) - 0.3).signum();
        d = d.with(BodySize, v);
    }
    let mut pool: Vec<Gene> = costed_genes().into_iter().filter(|&g| g != BodySize).collect();
    pool.shuffle(rng);
    pool.truncate(rng.gen_range(3..=8));
    let weights: Vec<f32> = pool.iter().map(|_| rng.gen_range(0.2..1.0)).collect();
    // Spend what's left in proportion to the weights, re-spending what traits
    // capped at 1 couldn't absorb.
    for _ in 0..4 {
        let left = DESIGN_BUDGET - d.point_cost();
        let open: Vec<usize> = (0..pool.len()).filter(|&i| d.get(pool[i]) < 1.0).collect();
        let total: f32 = open.iter().map(|&i| weights[i]).sum();
        if left < 0.5 || total == 0.0 {
            break;
        }
        for &i in &open {
            let g = pool[i];
            let add = left * weights[i] / total / trait_cost(g);
            let v = (d.get(g) + add).min(1.0);
            d = d.with(g, v);
        }
    }
    while d.point_cost() > DESIGN_BUDGET {
        let g = pool[rng.gen_range(0..pool.len())];
        let v = (d.get(g) - 0.01).max(0.0);
        d = d.with(g, v);
    }
    debug_assert!(d.validate().is_ok());
    d
}

fn tournament(o: &Opts) {
    for w in 0..o.worlds {
        let seed = o.seed0 + w;
        let mut rng = rng_for(seed, 0xca1, w, 0);
        let designs: Vec<FounderDesign> = (0..4).map(|k| random_design(&mut rng, format!("R{w}.{k}"))).collect();
        let out = run(seed, o, &designs);
        for (k, (d, r)) in designs.iter().zip(out).enumerate() {
            println!(
                "tournament,{seed},{w},{k},{},random,0,{:.1},{:.5},{:.5},{},{},{}",
                d.name,
                d.point_cost(),
                r.0,
                r.1,
                r.2,
                r.3,
                describe(d)
            );
        }
    }
}

fn costs() {
    println!(
        "budget {DESIGN_BUDGET}, base founders {BASE_FOUNDERS}, per extra founder {COST_PER_EXTRA_FOUNDER}, per extra offspring {COST_PER_EXTRA_OFFSPRING}, per unit smaller body {COST_PER_SMALLER_BODY}, cold-blooded {COST_COLD_BLOODED}"
    );
    for g in costed_genes() {
        println!("{:<14} {:5.1}", g.label(), trait_cost(g));
    }
}

fn main() {
    rayon::ThreadPoolBuilder::new().num_threads(1).build_global().unwrap();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(mode) = args.first() else {
        eprintln!("usage: calibrate marginal|tournament|costs [options]");
        return;
    };
    let o = parse(&args[1..]);
    match mode.as_str() {
        "marginal" => marginal(&o),
        "tournament" => tournament(&o),
        "costs" => costs(),
        _ => eprintln!("unknown mode {mode}"),
    }
}
