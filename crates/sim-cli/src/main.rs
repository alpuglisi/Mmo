//! `evo-sim`: run the prototype world headless, print reports, benchmark the
//! per-organism tick cost, and render map snapshots.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::process::ExitCode;
use std::time::Instant;

use evo_sim::chronicle::CatastropheKind;
use evo_sim::group::GroupTrait;
use evo_sim::math::Vec2;
use evo_sim::organism::{Action, DeathCause, Dormancy, Infection};
use evo_sim::{presets, render, Balance, Gene, LodConfig, PlayerId, ScoringConfig, World, WorldConfig, WILD};

mod adaptive;
mod rankcheck;
mod stats;
mod sweep;

const USAGE: &str = "\
evo-sim — Evolution MMO simulation prototype

USAGE:
    evo-sim run    [--seed N] [--size N] [--days N] [--report N] [--snapshot PATH] [--threads N] [--lod] [--focus X,Y] [--sequential-apply]
                   [--catastrophes] [--catastrophe KIND@DAY] [--era-years N] [--migrants]
    evo-sim bench  [--seed N] [--size N] [--days N] [--scale N] [--threads N,..] [--lod] [--focus X,Y] [--sequential-apply] [--compare-apply]
    evo-sim sweep  [--seed N] [--size N] [--days N] [--trials N] [--seeds N] [--rounds N] [--spread F] [--out PATH] [--sequential-apply]
                   [--screen-days D,..] [--screen-seeds N] [--eta F] [--migrants]
    evo-sim compare [--seed N] [--size N] [--days N] [--seeds N] [--out PATH] [--migrants] [--set NAME=V ..] [--migrant-gene GENE=V ..]
                    [vs [defaults | --set NAME=V .. | --migrant-gene GENE=V ..]]
                    [--adaptive] [--max-seeds N] [--batch N] [--tolerance F]
    evo-sim rankcheck [--seed N] [--size N] [--days N] [--trials N] [--seeds N] [--spread F] [--screen-days D,..] [--eta F] [--out PATH]
                      [--from PATH]
    evo-sim designs

COMMANDS:
    run      Found the wild ecosystem and four sample player lineages, run, report
    bench    Measure per-organism tick cost (µs) at a larger population
    sweep    Random-search the balance parameters, scoring lineage coexistence
    compare  Run two balance configurations on the same seeds and report the paired
             score difference with a 95% confidence interval (is A better than B?)
    rankcheck  Run one sweep round's trials and report how well short-run (screen)
             ranks predict full-run ranks (Spearman correlation, top-1/eta kept); with
             4+ seeds also seed-split reliabilities (noise vs screen error) and
             simulated screened sweeps (chance the true best survives a cut)
    designs  List the sample player designs (and the migrating one) and their point costs

OPTIONS:
    --seed N        World seed (default 1)
    --size N        Map width and height in tiles (default 128)
    --days N        Simulated days to run (default 120 for run, 10 for bench)
    --report N      Print a report every N days (default 10)
    --snapshot PATH Write a PNG of the final world (prefix; also writes one per report)
    --scale N       Founding population multiplier for bench (default 4)
    --threads N     Worker threads (default: all cores). Bench takes a list (1,2,4)
                    and measures each, rotating the simulated days between them
    --set NAME=V    Override a balance parameter (repeatable; for sweep, sets the start point;
                    for compare, configures A, or B after `vs`)
    --trials N      Sweep: parameter sets per round (default 16)
    --seeds N       Sweep: worlds per parameter set on the full run (default 3);
                    compare: seeds per configuration (default 12)
    --screen-days D,..  Sweep: successive halving. Each round first runs every trial
                    for D days (a comma list gives several screens, shortest first),
                    keeps the best 1/eta plus the round center, and resumes only
                    those worlds to the full --days (default: no screens)
    --screen-seeds N    Sweep: seeds per trial in the screens (default --seeds)
    --eta F         Sweep: a screen keeps ceil(trials / F) trials (default 3)
                    rankcheck: the checkpoints to rank against --days (default 20,40,60,80)
    --rounds N      Sweep: rounds, each recentered on the best with half the spread (default 2)
    --spread F      Sweep: max log-factor per parameter in round 1 (default 0.5, i.e. ×0.61–×1.65)
    --out PATH      Sweep: write every trial to a CSV file (score, its SE and 95% CI,
                    the paired difference to the center); compare: per-seed scores;
                    rankcheck: every world's score at every checkpoint
    --from PATH     Rankcheck: re-analyze a CSV written by its --out instead of simulating
    --adaptive      Compare: start with --seeds seeds (default 12) and add batches until
                    the paired difference's CI excludes 0, lies within ±tolerance, or
                    --max-seeds is reached. Each look uses Pocock's sequential level
                    (5% overall), so the printed interval is wider than a one-shot CI
    --max-seeds N   Compare: seed cap for --adaptive (default 48; implies --adaptive)
    --batch N       Compare: seeds added per look (default --seeds)
    --tolerance F   Compare: equivalence margin in score points; stop when the whole
                    sequential CI lies within ±F (default 0.3; 0 turns it off)
    vs              Compare: the --set flags after it configure B; before it, A.
                    B defaults to the default balance (`vs defaults` says so)
    --lod           Level of detail: full agents only near focus points, statistical
                    cohorts elsewhere (run and bench)
    --focus X,Y     A focus point in tiles (repeatable; implies --lod). Without any,
                    --lod runs the whole world as cohorts
    --lod-set NAME=V  Override a level-of-detail parameter (repeatable)
    --migrants      Also found the migrating sample design (Migrating caribou, a fifth
                    player lineage) (run, sweep, compare). Sweep and compare report it
                    but score only the standard eight lineages, so scores keep their
                    12-point scale
    --migrant-gene GENE=V  Compare: found the migrating caribou with GENE set to V (implies
                    --migrants; spaces in gene names as underscores; repeatable; before `vs`
                    for A, after it for B), e.g. `--migrant-gene migration=0 vs defaults`
                    measures what migrating buys the herd (paired, same seeds)
    --sequential-apply  Run the apply phase fully sequentially, as before the
                    parallel apply (run, bench, sweep; worlds differ from parallel ones)
    --compare-apply Bench: measure parallel and sequential apply, rotating days
    --catastrophes  Random catastrophes (from year 2; off by default) (run)
    --catastrophe KIND@DAY  Force a catastrophe starting on DAY (0-based, repeatable):
                    severe_winter, megadrought, epidemic, flood_year
    --era-years N   Era length in years (default 3)

Balance parameters: hunger_rate thirst_rate starvation_damage veg_regrowth base_speed sprint
attack_reach stealth_effect perception_range attack_power carnivore_hunger_discount
generalist_tax cold_heat_burn size_burn
pregnancy_cost meat_per_kg flee_weight ambush crowding toxin_damage larval_competition larval_stage
adult_mortality burrow_shelter burrow_caution parent_defense herd_guard breeding_density escape
escape_window home_buffer injury_aversion fight_damage recessive_mutation recessive_hazard
disease_emergence disease_transmission disease_damage disease_recovery disease_chronic
disease_fertility disease_kin path_nodes water_response snowmelt_swell rain_growth
storm_exposure canopy_share dormancy_temp estivation_temp dormant_metabolism
dormancy_mismatch brumation_mismatch fat_capacity den_concealment migration_range
migration_risk torpor_metabolism cache_capacity shared_burrow sentry_share
sentry_vigilance sentry_cost communal_den group_navigation shared_cache molt_cost
coat_insulation breeding_window timed_conception breeding_recklessness courtship_caution
brood_recovery egg_overheat brood_warmth brood_cost threat_caution alert_guard pasture_search
search_image cold_gestation nest_predation tameness migration_onset possess_urgency possess_danger

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
    /// Bench: every thread count given to `--threads` (comma-separated).
    thread_list: Vec<usize>,
    compare_apply: bool,
    balance: Balance,
    lod: LodConfig,
    focus: Vec<Vec2>,
    sequential_apply: bool,
    migrants: bool,
    scoring: ScoringConfig,
    forced: Vec<(CatastropheKind, u64)>,
    trials: usize,
    /// `--seeds`; the default depends on the command (sweep 3, compare 12).
    seeds: Option<u64>,
    rounds: u32,
    spread: f32,
    out: Option<String>,
    screen_days: Vec<u32>,
    screen_seeds: Option<u64>,
    eta: f64,
    /// Compare: the configuration after `vs` (defaults unless `--set` follows).
    balance_b: Option<Balance>,
    /// Compare: `--migrant-gene` overrides of the migrating lineage, for A and B.
    migrant_genes: [Vec<(Gene, f32)>; 2],
    /// Compare: `--adaptive` (or `--max-seeds`) given.
    adaptive: bool,
    max_seeds: u64,
    batch: Option<u64>,
    tolerance: f64,
    /// Rankcheck: re-analyze this `--out` CSV.
    from: Option<String>,
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
        thread_list: Vec::new(),
        compare_apply: false,
        balance: Balance::default(),
        lod: LodConfig::default(),
        focus: Vec::new(),
        sequential_apply: false,
        migrants: false,
        scoring: ScoringConfig::default(),
        forced: Vec::new(),
        trials: 16,
        seeds: None,
        rounds: 2,
        spread: 0.5,
        out: None,
        screen_days: Vec::new(),
        screen_seeds: None,
        eta: 3.0,
        balance_b: None,
        migrant_genes: [Vec::new(), Vec::new()],
        adaptive: false,
        max_seeds: 48,
        batch: None,
        tolerance: 0.3,
        from: None,
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
            "--threads" => {
                let v = value()?;
                a.thread_list = v
                    .split(',')
                    .map(|t| num(t.trim().to_string()).map(|n| n.max(1) as usize))
                    .collect::<Result<_, _>>()?;
                a.threads = a.thread_list.iter().copied().max();
            }
            "--trials" => a.trials = (num(value()?)? as usize).max(1),
            "--seeds" => a.seeds = Some(num(value()?)?.max(1)),
            "--screen-days" => {
                let v = value()?;
                a.screen_days =
                    v.split(',').map(|t| num(t.trim().to_string()).map(|n| n as u32)).collect::<Result<_, _>>()?;
            }
            "--screen-seeds" => a.screen_seeds = Some(num(value()?)?.max(1)),
            "--eta" => {
                let v = value()?;
                a.eta = v.parse().map_err(|_| format!("--eta: '{v}' is not a number"))?;
                if a.eta.is_nan() || a.eta < 1.0 {
                    return Err(format!("--eta must be at least 1, got '{v}'"));
                }
            }
            // Compare: what follows `vs` configures B; `defaults` names the
            // unmodified balance on either side.
            "vs" if a.command == "compare" => {
                if a.balance_b.is_some() {
                    return Err("compare takes one 'vs'".into());
                }
                a.balance_b = Some(Balance::default());
            }
            "defaults" if a.command == "compare" => {}
            "--adaptive" => a.adaptive = true,
            "--from" => a.from = Some(value()?),
            "--max-seeds" => {
                a.max_seeds = num(value()?)?.max(1);
                a.adaptive = true;
            }
            "--batch" => a.batch = Some(num(value()?)?.max(1)),
            "--tolerance" => {
                let v = value()?;
                a.tolerance = v.parse().map_err(|_| format!("--tolerance: '{v}' is not a number"))?;
                if a.tolerance.is_nan() || a.tolerance < 0.0 {
                    return Err(format!("--tolerance must be at least 0, got '{v}'"));
                }
            }
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
                let target = a.balance_b.as_mut().unwrap_or(&mut a.balance);
                if !target.set(name, x) {
                    return Err(format!("--set: unknown balance parameter '{name}'"));
                }
            }
            "--migrant-gene" => {
                let v = value()?;
                let (name, x) = v.split_once('=').ok_or(format!("--migrant-gene expects GENE=VALUE, got '{v}'"))?;
                let x: f32 = x.parse().map_err(|_| format!("--migrant-gene {name}: '{x}' is not a number"))?;
                let gene = Gene::ALL.iter().copied().find(|g| g.label().replace(' ', "_") == name).ok_or(format!(
                    "--migrant-gene: unknown gene '{name}' (spaces as underscores, e.g. group_tolerance)"
                ))?;
                a.migrant_genes[usize::from(a.balance_b.is_some())].push((gene, x.clamp(0.0, 1.0)));
            }
            "--lod" => a.lod.enabled = true,
            "--sequential-apply" => a.sequential_apply = true,
            "--migrants" => a.migrants = true,
            "--compare-apply" => a.compare_apply = true,
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
            "--catastrophes" => a.scoring.catastrophes = true,
            "--catastrophe" => {
                let v = value()?;
                let (kind, day) = v.split_once('@').ok_or(format!("--catastrophe expects KIND@DAY, got '{v}'"))?;
                let kind = CatastropheKind::from_label(kind).ok_or(format!("--catastrophe: unknown kind '{kind}'"))?;
                let day = day.parse::<u64>().map_err(|_| format!("--catastrophe: '{day}' is not a day"))?;
                a.forced.push((kind, day));
            }
            "--era-years" => a.scoring.era_years = (num(value()?)? as u32).max(1),
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
    let size_given = std::env::args().any(|a| a == "--size");
    let trials_given = std::env::args().any(|a| a == "--trials");
    let spread_given = std::env::args().any(|a| a == "--spread");
    let size = if args.size == 128 && !size_given { 96 } else { args.size };
    match args.command.as_str() {
        "run" => run(&args),
        "bench" => bench(&args),
        "sweep" => sweep::sweep(&sweep::SweepArgs {
            base_seed: args.seed,
            size,
            days: args.days.unwrap_or(120),
            trials: args.trials,
            seeds: args.seeds.unwrap_or(3),
            rounds: args.rounds,
            spread: args.spread,
            center: args.balance,
            out: args.out.clone(),
            parallel_apply: !args.sequential_apply,
            screen_days: args.screen_days.clone(),
            screen_seeds: args.screen_seeds,
            eta: args.eta,
            migrants: args.migrants,
        }),
        "compare" => {
            let seeds = args.seeds.unwrap_or(12);
            sweep::compare(&sweep::CompareArgs {
                base_seed: args.seed,
                size,
                days: args.days.unwrap_or(120),
                seeds,
                a: args.balance,
                b: args.balance_b.unwrap_or_default(),
                out: args.out.clone(),
                parallel_apply: !args.sequential_apply,
                migrants: args.migrants,
                migrant_genes: args.migrant_genes.clone(),
                adaptive: args.adaptive.then(|| adaptive::AdaptiveArgs {
                    initial: seeds,
                    batch: args.batch.unwrap_or(seeds),
                    max_seeds: args.max_seeds,
                    tolerance: args.tolerance,
                }),
            });
        }
        "rankcheck" => {
            if let Err(e) = rankcheck::rankcheck(&rankcheck::RankCheckArgs {
                base_seed: args.seed,
                size,
                days: args.days.unwrap_or(120),
                trials: if trials_given { args.trials } else { 24 },
                seeds: args.seeds.unwrap_or(4),
                spread: if spread_given { args.spread } else { 0.25 },
                center: args.balance,
                checkpoints: if args.screen_days.is_empty() { vec![20, 40, 60, 80] } else { args.screen_days.clone() },
                eta: args.eta,
                parallel_apply: !args.sequential_apply,
                out: args.out.clone(),
                from: args.from.clone(),
            }) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
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
    names.insert(presets::MIGRANT_OWNER, presets::migrant_design().name);
    names
}

fn build_world(args: &Args, scale: u32) -> World {
    let config = WorldConfig {
        seed: args.seed,
        width: args.size,
        height: args.size,
        balance: args.balance,
        lod: args.lod,
        parallel_apply: !args.sequential_apply,
        scoring: args.scoring,
        ..Default::default()
    };
    let mut world =
        if args.migrants { presets::extended_world(config, scale) } else { presets::standard_world(config, scale) };
    for &(kind, day) in &args.forced {
        world.schedule_catastrophe(kind, day, None);
    }
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
    let deaths: Vec<String> =
        DeathCause::ALL.iter().map(|d| format!("{} {}", d.label(), c.deaths.get(d).copied().unwrap_or(0))).collect();
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
    let groups: Vec<String> =
        GroupTrait::ALL.iter().map(|t| format!("{} {}", t.label(), world.group.active_regions(*t))).collect();
    println!("  group traits active (regions): {}", groups.join(" · "));
    print_drift(world, names);
    print_chronicle(world, names);
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

/// Species, catastrophes, eras and achievements (`World::chronicle`).
fn print_chronicle(world: &World, names: &BTreeMap<PlayerId, String>) {
    let ch = &world.chronicle;
    let t = &ch.species;
    let name = |p: PlayerId| {
        names.get(&p).cloned().unwrap_or_else(|| if p == WILD { "wild".into() } else { format!("player {p}") })
    };
    let alive: Vec<String> = t
        .alive()
        .map(|s| format!("{} ~{:.0} ({} {:.0}%)", s.name, s.adults, name(s.dominant), s.dominant_share * 100.0))
        .collect();
    let extinct = t.species.iter().filter(|s| !s.is_alive()).count();
    let splits = t.species.iter().filter(|s| s.origin == evo_sim::species::SpeciesOrigin::Split).count();
    println!(
        "  species: {} alive, {} extinct, {} splits (detected day {}, {} samples, {} unclassified): {}",
        alive.len(),
        extinct,
        splits,
        t.last_detection.map_or("-".into(), |d| (d + 1).to_string()),
        t.last_samples,
        t.unclassified,
        alive.join(" · ")
    );
    for c in &ch.catastrophes {
        if !c.started {
            continue;
        }
        let after = c.population_after.map_or("ongoing".to_string(), |p| format!("{p} after"));
        let survivors: Vec<String> = c.survivors.iter().map(|&p| name(p)).collect();
        println!(
            "  catastrophe #{} {} days {}–{}: population {} before, {}{}",
            c.id,
            c.kind.label(),
            c.start_day + 1,
            c.end_day,
            c.population_before,
            after,
            if c.ended {
                format!(" · survived by {}", if survivors.is_empty() { "none".into() } else { survivors.join(", ") })
            } else {
                String::new()
            }
        );
    }
    let era_days = world.config.era_days();
    let pending = ch.catastrophes.iter().filter(|c| !c.started).count();
    println!(
        "  era {}: day {} of {}{} · catastrophes {}",
        world.era() + 1,
        world.day() % era_days + 1,
        era_days,
        if pending > 0 { format!(" · {pending} scheduled") } else { String::new() },
        if world.config.scoring.catastrophes { "random from year 2" } else { "off (forced only)" }
    );
    for e in &ch.eras {
        let top: Vec<String> =
            e.ranking.iter().take(3).map(|r| format!("{} {:.1}%", name(r.player), r.share * 100.0)).collect();
        println!(
            "  era {} (days {}–{}): {} · {} species",
            e.era + 1,
            e.first_day + 1,
            e.last_day + 1,
            top.join(", "),
            e.species_alive
        );
    }
    let counts = ch.achievement_counts();
    if !counts.is_empty() {
        let rows: Vec<String> = counts
            .iter()
            .map(|(&p, k)| {
                format!("{} {}", name(p), k.iter().map(|(l, n)| format!("{l} {n}")).collect::<Vec<_>>().join(", "))
            })
            .collect();
        println!("  achievements: {}", rows.join(" · "));
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
    const WATCH: [Gene; 6] =
        [Gene::BodySize, Gene::LegLength, Gene::Carnivory, Gene::Camouflage, Gene::Boldness, Gene::LitterSize];
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
    // Every (threads, apply mode) setup to measure. Simulated days go to the
    // setups in rotation, so all of them see similar populations and the
    // same machine load; each reports the median day.
    let threads =
        if args.thread_list.is_empty() { vec![rayon::current_num_threads()] } else { args.thread_list.clone() };
    let modes = if args.compare_apply { vec![true, false] } else { vec![!args.sequential_apply] };
    let setups: Vec<(usize, bool)> = threads.iter().flat_map(|&t| modes.iter().map(move |&m| (t, m))).collect();
    let pools: Vec<rayon::ThreadPool> = setups
        .iter()
        .map(|&(t, _)| rayon::ThreadPoolBuilder::new().num_threads(t).build().expect("thread pool"))
        .collect();
    println!(
        "Bench: {}×{} map, {} organisms, {} days, threads {:?}, {} apply{}",
        args.size,
        args.size,
        world.population(),
        days,
        threads,
        match modes.as_slice() {
            [true] => "parallel",
            [false] => "sequential",
            _ => "parallel and sequential",
        },
        if args.lod.enabled {
            format!(", level of detail on ({} focus points)", args.focus.len())
        } else {
            String::new()
        }
    );
    if (days as usize) < setups.len() {
        println!("  (fewer days than setups: some setups go unmeasured; use --days {})", setups.len());
    }
    // Warm up one day so caches and populations settle.
    pools[0].install(|| world.run_days(1));
    struct Day {
        secs: f64,
        org_ticks: u64,
        pop_ticks: u64,
        searches: u64,
        phases: evo_sim::PhaseTimes,
    }
    let mut samples: Vec<Vec<Day>> = setups.iter().map(|_| Vec::new()).collect();
    let tpd = world.config.ticks_per_day;
    for d in 0..days as usize {
        let k = d % setups.len();
        world.config.parallel_apply = setups[k].1;
        world.phase_times = Default::default();
        let searches = world.counters.path_searches;
        let (mut org_ticks, mut pop_ticks) = (0u64, 0u64);
        let start = Instant::now();
        pools[k].install(|| {
            for _ in 0..tpd {
                org_ticks += world.alive_count() as u64;
                pop_ticks += world.population() as u64;
                world.step();
            }
        });
        let secs = start.elapsed().as_secs_f64();
        let searches = world.counters.path_searches - searches;
        samples[k].push(Day { secs, org_ticks, pop_ticks, searches, phases: world.phase_times });
    }
    let ticks = tpd as f64;
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        let n = v.len();
        if n % 2 == 1 {
            v[n / 2]
        } else {
            0.5 * (v[n / 2 - 1] + v[n / 2])
        }
    };
    for (k, &(t, parallel)) in setups.iter().enumerate() {
        let s = &samples[k];
        if s.is_empty() {
            continue;
        }
        let per_tick = |f: &dyn Fn(&Day) -> f64| median(s.iter().map(|d| f(d) * 1e3 / ticks).collect());
        let ms = per_tick(&|d| d.secs);
        let min = s.iter().map(|d| d.secs * 1e3 / ticks).fold(f64::MAX, f64::min);
        let agents = s.iter().map(|d| d.org_ticks).sum::<u64>() as f64 / (ticks * s.len() as f64);
        let pop = s.iter().map(|d| d.pop_ticks).sum::<u64>() as f64 / (ticks * s.len() as f64);
        let us = median(s.iter().map(|d| d.secs * 1e6 / d.org_ticks.max(1) as f64).collect());
        println!(
            "\n  {t} threads, {} apply ({} days measured)",
            if parallel { "parallel" } else { "sequential" },
            s.len()
        );
        println!("  {pop:.0} organisms on average");
        if args.lod.enabled {
            println!("  {agents:.0} of them agents, the rest in cohorts");
            let us_pop = median(s.iter().map(|d| d.secs * 1e6 / d.pop_ticks.max(1) as f64).collect());
            println!("  {us_pop:.2} µs per organism-tick counting cohort members");
        }
        println!("  {ms:.2} ms per tick (median day; fastest {min:.2})");
        println!(
            "  {:.2} path searches per tick",
            s.iter().map(|d| d.searches).sum::<u64>() as f64 / (ticks * s.len() as f64)
        );
        println!(
            "  phases (ms per tick, median day): environment {:.2}, daily {:.2}, grid {:.2}, decide {:.2}, apply {:.2}",
            per_tick(&|d| d.phases.environment),
            per_tick(&|d| d.phases.daily),
            per_tick(&|d| d.phases.grid),
            per_tick(&|d| d.phases.decide),
            per_tick(&|d| d.phases.apply)
        );
        if parallel {
            let engaged = s.iter().map(|d| d.phases.engaged).sum::<u64>() as f64;
            let applied = s.iter().map(|d| d.phases.applied).sum::<u64>().max(1) as f64;
            println!(
                "  parallel apply: {:.2} ms per tick in its sequential sweeps, {:.1}% of turns engaged in a chase",
                per_tick(&|d| d.phases.apply_serial),
                100.0 * engaged / applied
            );
        }
        if agents > 0.0 {
            println!("  {us:.2} µs per organism-tick (agents only; wall clock, all threads)");
            println!("  ≈ {:.0}k organisms at 1 tick/second at this density", 1e6 / us / 1e3);
        }
    }
}

fn designs() {
    println!("Budget: {} points\n", evo_sim::design::DESIGN_BUDGET);
    for d in presets::player_designs().into_iter().chain([presets::migrant_design()]) {
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
