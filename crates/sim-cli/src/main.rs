//! `evo-sim`: run the prototype world headless, print reports, benchmark the
//! per-organism tick cost, and render map snapshots.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::process::ExitCode;
use std::time::Instant;

use evo_sim::group::GroupTrait;
use evo_sim::organism::{Action, DeathCause, Dormancy, Infection};
use evo_sim::math::Vec2;
use evo_sim::{presets, render, Balance, Gene, LodConfig, PlayerId, World, WorldConfig, WILD};

mod sweep;

const USAGE: &str = "\
evo-sim — Evolution MMO simulation prototype

USAGE:
    evo-sim run    [--seed N] [--size N] [--days N] [--report N] [--snapshot PATH] [--threads N] [--lod] [--focus X,Y]
    evo-sim bench  [--seed N] [--size N] [--days N] [--scale N] [--threads N] [--lod] [--focus X,Y]
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
    --lod           Level of detail: full agents only near focus points, statistical
                    cohorts elsewhere (run and bench)
    --focus X,Y     A focus point in tiles (repeatable; implies --lod). Without any,
                    --lod runs the whole world as cohorts
    --lod-set NAME=V  Override a level-of-detail parameter (repeatable)

Balance parameters: hunger_rate thirst_rate starvation_damage veg_regrowth base_speed sprint
attack_reach stealth_effect perception_range attack_power carnivore_hunger_discount
pregnancy_cost meat_per_kg flee_weight ambush crowding toxin_damage larval_competition
adult_mortality burrow_shelter burrow_caution parent_defense breeding_density escape
escape_window home_buffer injury_aversion fight_damage recessive_mutation recessive_hazard
disease_emergence disease_transmission disease_damage disease_recovery disease_chronic
disease_fertility disease_kin path_nodes water_response snowmelt_swell rain_growth
storm_exposure canopy_share dormancy_temp estivation_temp dormant_metabolism
dormancy_mismatch brumation_mismatch fat_capacity den_concealment migration_range
migration_risk torpor_metabolism cache_capacity shared_burrow sentry_share
sentry_vigilance sentry_cost communal_den group_navigation shared_cache molt_cost
coat_insulation breeding_window timed_conception breeding_recklessness
possess_urgency possess_danger

Level-of-detail parameters: active_radius hysteresis pool_size graze_fraction
juvenile_mortality prey_half_density plant_diet carcass_use migration mate_search_days clump
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
    lod: LodConfig,
    focus: Vec<Vec2>,
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
        lod: LodConfig::default(),
        focus: Vec::new(),
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
            "--lod" => a.lod.enabled = true,
            "--focus" => {
                let v = value()?;
                let (x, y) = v.split_once(',').ok_or(format!("--focus expects X,Y, got '{v}'"))?;
                let parse = |t: &str| t.trim().parse::<f32>().map_err(|_| format!("--focus: '{t}' is not a number"));
                a.focus.push(Vec2::new(parse(x)?, parse(y)?));
                a.lod.enabled = true;
            }
            "--lod-set" => {
                let v = value()?;
                let (name, x) = v.split_once('=').ok_or(format!("--lod-set expects NAME=VALUE, got '{v}'"))?;
                let x: f32 = x.parse().map_err(|_| format!("--lod-set {name}: '{x}' is not a number"))?;
                if !a.lod.set(name, x) {
                    return Err(format!("--lod-set: unknown level-of-detail parameter '{name}'"));
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
    let config = WorldConfig {
        seed: args.seed,
        width: args.size,
        height: args.size,
        balance: args.balance,
        lod: args.lod,
        ..Default::default()
    };
    let mut world = presets::standard_world(config, scale);
    if args.lod.enabled {
        world.set_focus(1, args.focus.clone());
    }
    world
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
    if args.lod.enabled {
        println!(
            "Level of detail on: {} focus point(s), active radius {} tiles",
            args.focus.len(),
            args.lod.active_radius
        );
    }
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
        if world.population() == 0 {
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
        "\n── Year {} day {} ({:?}) · {} alive{} · {} clutches · wind {:.2} · drought {:.0}% · flood {:.0}% · {:.2} µs/org-tick ──",
        world.year() + 1,
        world.day_of_year() + 1,
        world.season(),
        world.population(),
        if world.config.lod.enabled {
            format!(" ({} agents, {} in cohorts)", world.alive_count(), world.abstract_count())
        } else {
            String::new()
        },
        world.clutches.len(),
        world.weather.wind,
        world.map.drought * 100.0,
        world.map.flood * 100.0,
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
    // Cohort members of abstract regions.
    let abstract_counts = world.abstract_counts();
    for (l, c) in abstract_counts.iter().enumerate() {
        let e = adults.entry(world.lineages[l].owner).or_default();
        e.0 += c.alive;
        e.1 += c.adults;
    }
    for (_, c) in world.cohorts() {
        let e = adults.entry(c.owner).or_default();
        e.2 = e.2.max(c.pool.iter().map(|p| p.generation).max().unwrap_or(0));
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
    let agents = world.lineage_counts();
    let counts = world.population_counts();
    // Per lineage: [acute, chronic, harmful alleles carried, homozygous].
    let mut genetic = vec![[0u32; 4]; world.lineages.len()];
    for o in world.organisms.iter().filter(|o| o.alive) {
        let g = &mut genetic[o.lineage as usize];
        g[0] += (o.infection == Infection::Acute) as u32;
        g[1] += (o.infection == Infection::Chronic) as u32;
        g[2] += o.genome.harmful_alleles();
        g[3] += (o.recessive_load > 0) as u32;
    }
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
        let g = genetic[l];
        println!(
            "      infections {} (now sick {}, carriers {}) · harmful recessives {:.2}/organism, {} affected",
            c.infections.get(&l32).copied().unwrap_or(0),
            g[0],
            g[1],
            g[2] as f32 / agents[l].alive.max(1) as f32,
            g[3]
        );
    }
    if !c.hybrids.is_empty() {
        let pairs: Vec<String> = c.hybrids.iter().map(|((m, f), n)| format!("{m}×{f} {n}")).collect();
        println!("  hybrid offspring (mother×father lineage): {}", pairs.join(" · "));
    }
    print_seasonal(world);
    let groups: Vec<String> = GroupTrait::ALL
        .iter()
        .map(|t| format!("{} {}", t.label(), world.group.active_regions(*t)))
        .collect();
    println!("  group traits active (regions): {}", groups.join(" · "));
    print_drift(world, names);
    if world.config.lod.enabled {
        let s = world.lod_stats();
        let active = (0..world.map.chunks_x * world.map.chunks_y).filter(|&r| world.region_is_active(r)).count();
        println!(
            "  level of detail: {active}/{} regions active · {} collapses, {} expansions · {} absorbed, {} expanded, {} emigrated as agents",
            world.map.chunks_x * world.map.chunks_y,
            s.collapses,
            s.expansions,
            s.absorbed,
            s.expanded,
            s.emigrated
        );
    }
}

/// Seasonal strategies at work, per lineage that uses any: dormant (winter or
/// summer), torpid, migrating, in winter coat, mean fat of adults and food
/// cached.
fn print_seasonal(world: &World) {
    #[derive(Default)]
    struct Row {
        winter: usize,
        summer: usize,
        torpid: usize,
        migrating: usize,
        coated: usize,
        adults: usize,
        fat: f32,
        cached: f32,
        shared: f32,
    }
    let mut rows: BTreeMap<u32, Row> = BTreeMap::new();
    for o in world.organisms.iter().filter(|o| o.alive) {
        let r = rows.entry(o.lineage).or_default();
        match o.dormancy {
            Dormancy::Winter => r.winter += 1,
            Dormancy::Summer => r.summer += 1,
            Dormancy::Torpor => r.torpid += 1,
            Dormancy::Awake => {}
        }
        if o.action == Action::Migrate {
            r.migrating += 1;
        }
        if o.coat.is_some_and(|c| c > 0.5) {
            r.coated += 1;
        }
        if o.matured {
            r.adults += 1;
            r.fat += o.fat;
        }
        r.cached += o.cache;
    }
    for (&l, r) in rows.iter_mut() {
        r.shared = world.shared_cache_food(l);
    }
    for (l, r) in rows {
        if r.winter + r.summer + r.torpid + r.migrating + r.coated == 0 && r.fat + r.cached + r.shared < 1.0 {
            continue;
        }
        let shared = if r.shared >= 0.5 { format!(" (shared {:.0})", r.shared) } else { String::new() };
        println!(
            "    {:<20} seasonal: hibernating {} · estivating {} · torpid {} · migrating {} · winter coat {} · fat {:.0}/adult · cached {:.0}{shared}",
            world.lineages[l as usize].name,
            r.winter,
            r.summer,
            r.torpid,
            r.migrating,
            r.coated,
            r.fat / r.adults.max(1) as f32,
            r.cached + r.shared
        );
    }
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
        "Bench: {}×{} map, {} organisms, {} days, {} threads{}",
        args.size,
        args.size,
        world.population(),
        days,
        rayon::current_num_threads(),
        if args.lod.enabled { format!(", level of detail on ({} focus points)", args.focus.len()) } else { String::new() }
    );
    // Warm up one day so caches and populations settle.
    world.run_days(1);
    let mut org_ticks = 0u64;
    let mut pop_ticks = 0u64;
    let searches = world.counters.path_searches;
    let start = Instant::now();
    for _ in 0..days * world.config.ticks_per_day {
        org_ticks += world.alive_count() as u64;
        pop_ticks += world.population() as u64;
        world.step();
    }
    let secs = start.elapsed().as_secs_f64();
    let ticks = (days * world.config.ticks_per_day) as f64;
    let us = secs * 1e6 / org_ticks.max(1) as f64;
    println!("  {:.0} organisms on average", pop_ticks as f64 / ticks);
    if args.lod.enabled {
        println!("  {:.0} of them agents, the rest in cohorts", org_ticks as f64 / ticks);
        println!("  {:.2} µs per organism-tick counting cohort members", secs * 1e6 / pop_ticks.max(1) as f64);
    }
    println!("  {:.2} ms per tick", secs * 1e3 / ticks);
    println!("  {:.2} path searches per tick", (world.counters.path_searches - searches) as f64 / ticks);
    if org_ticks > 0 {
        println!("  {us:.2} µs per organism-tick (agents only; wall clock, all threads)");
        println!("  ≈ {:.0}k organisms at 1 tick/second on this machine", 1e6 / us / 1e3);
    }
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
