//! `evo-sim`: run the prototype world headless, print reports, benchmark the
//! per-organism tick cost, and render map snapshots.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::process::ExitCode;
use std::time::Instant;

use evo_sim::group::GroupTrait;
use evo_sim::organism::DeathCause;
use evo_sim::{presets, render, Balance, Gene, PlayerId, World, WorldConfig, WILD};

mod sweep;

const USAGE: &str = "\
evo-sim — Evolution MMO simulation prototype

USAGE:
    evo-sim run    [--seed N] [--size N] [--days N] [--report N] [--snapshot PATH] [--threads N]
    evo-sim bench  [--seed N] [--size N] [--days N] [--scale N] [--threads N]
    evo-sim sweep  [--seed N] [--size N] [--days N] [--trials N] [--seeds N] [--rounds N] [--spread F] [--out PATH]
    evo-sim designs

COMMANDS:
    run      Found the wild ecosystem and four sample player lineages, run, report
    bench    Measure per-organism tick cost (µs) at a larger population
    sweep    Random-search the balance parameters, scoring lineage coexistence
    designs  List the sample founder designs and their point costs

OPTIONS:
    --seed N        World seed (default 1)
    --size N        Map width and height in tiles (default 128)
    --days N        Simulated days to run (default 120 for run, 10 for bench)
    --report N      Print a report every N days (default 10)
    --snapshot PATH Write a PNG of the final world (prefix; also writes one per report)
    --scale N       Founding population multiplier for bench (default 4)
    --threads N     Worker threads (default: all cores)
    --set NAME=V    Override a balance parameter (repeatable; for sweep, sets the start point)
    --trials N      Sweep: parameter sets per round (default 16)
    --seeds N       Sweep: worlds per parameter set (default 3)
    --rounds N      Sweep: rounds, each recentered on the best with half the spread (default 2)
    --spread F      Sweep: max log-factor per parameter in round 1 (default 0.5, i.e. ×0.61–×1.65)
    --out PATH      Sweep: write every trial to a CSV file

Balance parameters: hunger_rate thirst_rate starvation_damage veg_regrowth base_speed sprint
attack_reach stealth_effect perception_range attack_power carnivore_hunger_discount
pregnancy_cost meat_per_kg flee_weight ambush crowding toxin_damage larval_competition
adult_mortality burrow_shelter burrow_caution parent_defense
";

struct Args {
    command: String,
    seed: u64,
    size: usize,
    days: Option<u32>,
    report: u32,
    snapshot: Option<String>,
    scale: u32,
    threads: Option<usize>,
    balance: Balance,
    trials: usize,
    seeds: u64,
    rounds: u32,
    spread: f32,
    out: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let command = it.next().unwrap_or_else(|| "help".into());
    let mut a = Args {
        command,
        seed: 1,
        size: 128,
        days: None,
        report: 10,
        snapshot: None,
        scale: 4,
        threads: None,
        balance: Balance::default(),
        trials: 16,
        seeds: 3,
        rounds: 2,
        spread: 0.5,
        out: None,
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        let num = |v: String| v.parse::<u64>().map_err(|_| format!("{flag}: '{v}' is not a number"));
        match flag.as_str() {
            "--seed" => a.seed = num(value()?)?,
            "--size" => a.size = num(value()?)? as usize,
            "--days" => a.days = Some(num(value()?)? as u32),
            "--report" => a.report = (num(value()?)? as u32).max(1),
            "--snapshot" => a.snapshot = Some(value()?),
            "--scale" => a.scale = (num(value()?)? as u32).max(1),
            "--threads" => a.threads = Some(num(value()?)? as usize),
            "--trials" => a.trials = (num(value()?)? as usize).max(1),
            "--seeds" => a.seeds = num(value()?)?.max(1),
            "--rounds" => a.rounds = (num(value()?)? as u32).max(1),
            "--spread" => {
                let v = value()?;
                a.spread = v.parse().map_err(|_| format!("--spread: '{v}' is not a number"))?;
            }
            "--out" => a.out = Some(value()?),
            "--set" => {
                let v = value()?;
                let (name, x) = v.split_once('=').ok_or(format!("--set expects NAME=VALUE, got '{v}'"))?;
                let x: f32 = x.parse().map_err(|_| format!("--set {name}: '{x}' is not a number"))?;
                if !a.balance.set(name, x) {
                    return Err(format!("--set: unknown balance parameter '{name}'"));
                }
            }
            "-h" | "--help" => a.command = "help".into(),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(n) = args.threads {
        rayon::ThreadPoolBuilder::new().num_threads(n).build_global().expect("thread pool");
    }
    match args.command.as_str() {
        "run" => run(&args),
        "bench" => bench(&args),
        "sweep" => sweep::sweep(&sweep::SweepArgs {
            base_seed: args.seed,
            size: if args.size == 128 && !std::env::args().any(|a| a == "--size") { 96 } else { args.size },
            days: args.days.unwrap_or(120),
            trials: args.trials,
            seeds: args.seeds,
            rounds: args.rounds,
            spread: args.spread,
            center: args.balance,
            out: args.out.clone(),
        }),
        "designs" => designs(),
        "help" | "-h" | "--help" => print!("{USAGE}"),
        other => {
            eprintln!("error: unknown command '{other}'\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn player_names() -> BTreeMap<PlayerId, String> {
    let mut names: BTreeMap<PlayerId, String> = BTreeMap::new();
    names.insert(WILD, "Wild ecosystem".into());
    for (k, d) in presets::player_designs().iter().enumerate() {
        names.insert(k as PlayerId + 1, d.name.clone());
    }
    names
}

fn build_world(args: &Args, scale: u32) -> World {
    let config = WorldConfig { seed: args.seed, width: args.size, height: args.size, balance: args.balance, ..Default::default() };
    presets::standard_world(config, scale)
}

fn run(args: &Args) {
    let days = args.days.unwrap_or(120);
    let mut world = build_world(args, 1);
    let names = player_names();
    println!(
        "World {}×{} seed {} · land {:.0}% · {} founders · {} ticks/day, {} days/year",
        args.size,
        args.size,
        args.seed,
        world.map.land_fraction() * 100.0,
        world.alive_count(),
        world.config.ticks_per_day,
        world.config.days_per_year
    );
    let mut org_ticks: u64 = 0;
    let started = Instant::now();
    let mut window = Instant::now();
    let mut window_org_ticks = 0u64;
    for day in 1..=days {
        for _ in 0..world.config.ticks_per_day {
            let n = world.alive_count() as u64;
            world.step();
            org_ticks += n;
            window_org_ticks += n;
        }
        if day % args.report == 0 || day == days {
            let us = window.elapsed().as_secs_f64() * 1e6 / window_org_ticks.max(1) as f64;
            report(&world, &names, us);
            if let Some(prefix) = &args.snapshot {
                let path = format!("{prefix}-day{:04}.png", world.day());
                if let Err(e) = write_png(&world, &path) {
                    eprintln!("snapshot failed: {e}");
                }
            }
            window = Instant::now();
            window_org_ticks = 0;
        }
        if world.alive_count() == 0 {
            println!("All life has ended on day {}.", world.day());
            break;
        }
    }
    let secs = started.elapsed().as_secs_f64();
    println!(
        "\nRan {} days in {:.1}s · {:.2} µs per organism-tick · {} organism-ticks",
        world.day(),
        secs,
        secs * 1e6 / org_ticks.max(1) as f64,
        org_ticks
    );
    if let Some(prefix) = &args.snapshot {
        let path = format!("{prefix}-final.png");
        match write_png(&world, &path) {
            Ok(()) => println!("Wrote {path}"),
            Err(e) => eprintln!("snapshot failed: {e}"),
        }
    }
}

fn report(world: &World, names: &BTreeMap<PlayerId, String>, us_per_org_tick: f64) {
    let c = &world.counters;
    println!(
        "\n── Year {} day {} ({:?}) · {} alive · {} clutches · wind {:.2} · {:.2} µs/org-tick ──",
        world.year() + 1,
        world.day_of_year() + 1,
        world.season(),
        world.alive_count(),
        world.clutches.len(),
        world.weather.wind,
        us_per_org_tick
    );
    let legacy = world.latest_legacy();
    println!("  {:<22} {:>6} {:>7} {:>8} {:>9} {:>7}", "lineage", "alive", "adults", "legacy%", "weighted%", "max gen");
    let mut adults: BTreeMap<PlayerId, (usize, usize, u32)> = BTreeMap::new();
    for o in world.organisms.iter().filter(|o| o.alive) {
        let e = adults.entry(o.owner).or_default();
        e.0 += 1;
        if o.matured {
            e.1 += 1;
        }
        e.2 = e.2.max(o.generation);
    }
    for (owner, name) in names {
        let (alive, grown, gen) = adults.get(owner).copied().unwrap_or_default();
        let (share, weighted) = legacy.map_or((0.0, 0.0), |s| (s.share(*owner), s.weighted_share(*owner)));
        println!(
            "  {:<22} {:>6} {:>7} {:>7.1}% {:>8.1}% {:>7}",
            name,
            alive,
            grown,
            share * 100.0,
            weighted * 100.0,
            gen
        );
    }
    let deaths: Vec<String> = DeathCause::ALL
        .iter()
        .map(|d| format!("{} {}", d.label(), c.deaths.get(d).copied().unwrap_or(0)))
        .collect();
    println!(
        "  births {} · hatched {} · eggs lost {} · matings {} ({} cross-lineage) · kills {}/{} hunts",
        c.births,
        c.hatched,
        c.eggs_lost,
        c.matings,
        c.cross_lineage_matings,
        c.kills,
        c.kills + c.failed_hunts
    );
    println!("  deaths: {}", deaths.join(" · "));
    let counts = world.lineage_counts();
    for (l, lineage) in world.lineages.iter().enumerate() {
        let l32 = l as u32;
        let row: Vec<String> = DeathCause::ALL
            .iter()
            .filter_map(|d| c.deaths_by_lineage.get(&(l32, *d)).map(|n| format!("{} {n}", d.label())))
            .collect();
        println!(
            "    {:<20} alive {:>4} ({:>4} adults) · born {:>5} (hybrids {}) · died {} (juveniles {})",
            lineage.name,
            counts[l].alive,
            counts[l].adults,
            c.born.get(&l32).copied().unwrap_or(0),
            c.hybrids.iter().filter(|((m, _), _)| *m == l32).map(|(_, n)| n).sum::<u64>(),
            if row.is_empty() { "0".into() } else { row.join(", ") },
            c.juvenile_deaths.get(&l32).copied().unwrap_or(0)
        );
    }
    if !c.hybrids.is_empty() {
        let pairs: Vec<String> = c.hybrids.iter().map(|((m, f), n)| format!("{m}×{f} {n}")).collect();
        println!("  hybrid offspring (mother×father lineage): {}", pairs.join(" · "));
    }
    let groups: Vec<String> = GroupTrait::ALL
        .iter()
        .map(|t| format!("{} {}", t.label(), world.group.active_regions(*t)))
        .collect();
    println!("  group traits active (regions): {}", groups.join(" · "));
    print_drift(world, names);
}

/// Mean of a few genes per lineage, to watch selection at work.
fn print_drift(world: &World, names: &BTreeMap<PlayerId, String>) {
    const WATCH: [Gene; 6] = [Gene::BodySize, Gene::LegLength, Gene::Carnivory, Gene::Camouflage, Gene::Boldness, Gene::LitterSize];
    let mut sums: BTreeMap<PlayerId, ([f32; 6], u32)> = BTreeMap::new();
    for o in world.organisms.iter().filter(|o| o.alive && o.owner != WILD) {
        let e = sums.entry(o.owner).or_default();
        for (k, g) in WATCH.iter().enumerate() {
            e.0[k] += o.pheno[*g];
        }
        e.1 += 1;
    }
    let header: Vec<String> = WATCH.iter().map(|g| format!("{:>9.9}", g.label())).collect();
    println!("  {:<22} {}", "mean genes", header.join(""));
    for (owner, (s, n)) in sums {
        let vals: Vec<String> = s.iter().map(|v| format!("{:>9.3}", v / n as f32)).collect();
        println!("  {:<22} {}", names.get(&owner).map_or("?", |s| s.as_str()), vals.join(""));
    }
}

fn bench(args: &Args) {
    let days = args.days.unwrap_or(10);
    let mut world = build_world(args, args.scale);
    println!(
        "Bench: {}×{} map, {} organisms, {} days, {} threads",
        args.size,
        args.size,
        world.alive_count(),
        days,
        rayon::current_num_threads()
    );
    // Warm up one day so caches and populations settle.
    world.run_days(1);
    let mut org_ticks = 0u64;
    let start = Instant::now();
    for _ in 0..days * world.config.ticks_per_day {
        org_ticks += world.alive_count() as u64;
        world.step();
    }
    let secs = start.elapsed().as_secs_f64();
    let ticks = (days * world.config.ticks_per_day) as f64;
    let us = secs * 1e6 / org_ticks.max(1) as f64;
    println!("  {:.0} organisms on average", org_ticks as f64 / ticks);
    println!("  {:.2} ms per tick", secs * 1e3 / ticks);
    println!("  {us:.2} µs per organism-tick (wall clock, all threads)");
    println!(
        "  ≈ {:.0}k organisms at 1 tick/second on this machine",
        1e6 / us / 1e3
    );
}

fn designs() {
    println!("Budget: {} points\n", evo_sim::design::DESIGN_BUDGET);
    for d in presets::player_designs() {
        println!(
            "{:<22} {:?}, {:?}, {} per birth, {} founders · {:.1} points · {}",
            d.name,
            d.thermo,
            d.birth,
            d.litter_size,
            d.founders,
            d.point_cost(),
            if d.validate().is_ok() { "valid" } else { "INVALID" }
        );
        let traits: Vec<String> = d.traits.iter().map(|(g, v)| format!("{} {:.2}", g.label(), v)).collect();
        println!("    {}", traits.join(", "));
    }
}

fn write_png(world: &World, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (w, h, rgb) = render::render_rgb(world, 4);
    let file = BufWriter::new(File::create(path)?);
    let mut enc = png::Encoder::new(file, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgb)?;
    Ok(())
}
