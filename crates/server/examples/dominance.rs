//! The playtests' dominant designs, replayed: do playtest 2's egg-laying,
//! toxic-but-drab hibernating burrower (founded late in winter with
//! reinforcements) and playtest 3's cold-blooded, brumating, toxic flier
//! still run away under the new rules, and are they still viable?
//!
//! `cargo run --release -p evo-server --example dominance -- [--seeds N]
//!  [--first-seed N] [--days D] [--size N] [--late-year Y] [--flier-day D]
//!  [--baseline playtest2] [--set NAME=V ..] [--new-only]`
//!
//! Ten players through the server's own rules (`Game`: founding with the
//! winter provision, the renown shop, seasons) on the standard world with
//! its wild species:
//!
//! - **Steady** founds the client's starter herbivore with 40 founders and
//!   points on senses and legs (day 0).
//! - **Big** founds a big hibernating grazer (body size 0.75, playtest 2's
//!   Herdmaster) with 40 founders (day 0).
//! - **Broker** founds a live-bearing grazer of antelope size (playtest 2's
//!   early leader, day 0).
//! - **Eggs** founds LateComer's design on the first day of winter of year
//!   `--late-year` (default 1, the second year), and buys reinforcements
//!   whenever the shop sells them, as LateComer did.
//! - **Flier** founds PocketNomad5's re-founded flier (playtest 5's
//!   leader; 10 founders) on `--flier-day` (default 165).
//! - **Cold** founds LateComer4's design (playtest 4's winner and
//!   LateComer5's: cold-blooded, live-bearing, brumating, toxic flier, 62
//!   founders) on `--cold-day` (default 313, as LateComer5 did).
//! - **Herd** founds Herdmaster6's warm, egg-incubating, hibernating, toxic,
//!   bright, very sociable herd (playtest 6's runaway; 50 founders) on
//!   `--herd-day` (default 28).
//! - **Sleepers** founds PocketNomad7's copy of the leading class (warm,
//!   small, incubated eggs, toxic, bright, hibernating and estivating; 20
//!   founders, a re-found) on day 234; **LoopHerd** Rulebender7's copy of
//!   Herdmaster6's herd (estivation 0.35, 40 founders) on day 34.
//! - **Loophole** founds Rulebender5's small egg-incubating cold flier as a
//!   half-size re-found (22 founders) on `--loophole-day` (default 334).
//!
//! Two arms on the same seeds. **Playtest 7 rules**: the version 64
//! tunables off (`honest_signals`, `wild_isolation`), or with `--baseline playtest2`
//! playtest 2's (the balance rules since version 43 off, the old soft cap),
//! each founded at any price. **New rules**:
//! today's defaults, each design fitted to today's prices by dropping
//! founders.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use evo_server::game::{build_world, Game, GameConfig, WorldSettings};
use evo_server::protocol::{ClientMsg, ServerMsg};
use evo_sim::design::DESIGN_BUDGET;
use evo_sim::terrain::Season;
use evo_sim::{BirthType, FounderDesign, Gene, PlayerId, Thermo};

const STEADY: PlayerId = 1;
const BIG: PlayerId = 2;
const BROKER: PlayerId = 3;
const EGGS: PlayerId = 4;
const FLIER: PlayerId = 5;
const COLD: PlayerId = 6;
const LOOPHOLE: PlayerId = 7;
const HERD: PlayerId = 8;
const SLEEPERS: PlayerId = 9;
const LOOPHERD: PlayerId = 10;
const PLAYERS: u32 = 10;
const NAMES: [&str; 10] =
    ["Steady", "Big", "Broker", "Eggs", "Flier", "Cold", "Loophole", "Herd", "Sleepers", "LoopHerd"];
const SEASON: u64 = 60;
/// PocketNomad7's and Rulebender7's founding days (playtest 7).
const SLEEPERS_DAY: u64 = 234;
const LOOPHERD_DAY: u64 = 34;

struct Args {
    seeds: u64,
    first_seed: u64,
    days: u64,
    size: usize,
    late_year: u64,
    flier_day: u64,
    cold_day: u64,
    loophole_day: u64,
    herd_day: u64,
    playtest2_baseline: bool,
    /// `--set NAME=V`: balance overrides for the new-rules arm.
    sets: Vec<(String, f32)>,
    /// `--new-only`: run only the new-rules arm.
    new_only: bool,
}

fn args() -> Args {
    let mut a = Args {
        seeds: 4,
        first_seed: 1,
        days: 840,
        size: 128,
        late_year: 1,
        flier_day: 165,
        cold_day: 313,
        loophole_day: 334,
        herd_day: 28,
        playtest2_baseline: false,
        sets: Vec::new(),
        new_only: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| panic!("{k} needs a value"));
        match k.as_str() {
            "--seeds" => a.seeds = v().parse().expect("--seeds"),
            "--first-seed" => a.first_seed = v().parse().expect("--first-seed"),
            "--days" => a.days = v().parse().expect("--days"),
            "--size" => a.size = v().parse().expect("--size"),
            "--late-year" => a.late_year = v().parse().expect("--late-year"),
            "--flier-day" => a.flier_day = v().parse().expect("--flier-day"),
            "--cold-day" => a.cold_day = v().parse().expect("--cold-day"),
            "--loophole-day" => a.loophole_day = v().parse().expect("--loophole-day"),
            "--herd-day" => a.herd_day = v().parse().expect("--herd-day"),
            "--baseline" => a.playtest2_baseline = v() == "playtest2",
            "--set" => {
                let kv = v();
                let (k, x) = kv.split_once('=').expect("--set NAME=V");
                a.sets.push((k.to_string(), x.parse().expect("--set value")));
            }
            "--new-only" => a.new_only = true,
            _ => panic!("unknown argument {k}"),
        }
    }
    a
}

/// LateComer's "Late Eggrunners" (`docs/playtest-2-logs/LateComer.md`).
fn eggrunner() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Eggs", Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(BodySize, 0.5)
        .with(LegLength, 0.7)
        .with(Hooves, 0.5)
        .with(Toxins, 0.7)
        .with(Coloration, 0.4)
        .with(Fur, 0.6)
        .with(FatStores, 0.6)
        .with(Hibernation, 0.6)
        .with(Burrowing, 0.4)
        .with(Vigilance, 0.5)
        .with(Ears, 0.4)
        .with(Vocal, 0.4)
        .with(Camouflage, 0.4)
        .with(Swimming, 0.3)
        .with(Caching, 0.4)
        .with(Muscle, 0.3)
        .with(Nose, 0.3)
        .with(Eyes, 0.3)
        .with(GroupTolerance, 0.9)
        .with(Boldness, 0.3)
        .with(Pickiness, 0.2)
        .with(ForagingRange, 0.7)
        .with(Aggression, 0.3)
        .founders(55)
}

/// The client's starter with senses and legs, 40 founders.
fn steady() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Steady", Thermo::Warm, BirthType::Live, 2)
        .with(BodySize, 0.5)
        .with(GroupTolerance, 0.9)
        .with(Boldness, 0.3)
        .with(Fur, 0.6)
        .with(FatStores, 0.6)
        .with(Hibernation, 0.5)
        .with(Eyes, 0.5)
        .with(Ears, 0.5)
        .with(LegLength, 0.5)
        .with(Vigilance, 0.6)
        .with(Vocal, 0.5)
        .founders(40)
}

/// A big hibernating grazer (playtest 2's Herdmaster: body 0.75).
fn big() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Big", Thermo::Warm, BirthType::Live, 2)
        .with(BodySize, 0.75)
        .with(GroupTolerance, 0.9)
        .with(Boldness, 0.3)
        .with(Fur, 0.6)
        .with(FatStores, 0.6)
        .with(Hibernation, 0.6)
        .with(LegLength, 0.5)
        .with(Hooves, 0.5)
        .with(Swimming, 0.3)
        .with(Vigilance, 0.5)
        .founders(40)
}

/// A live-bearing grazer of antelope size (playtest 2's GeneBroker, the
/// early leader: body size 0.66).
fn broker() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Broker", Thermo::Warm, BirthType::Live, 2)
        .with(BodySize, 0.66)
        .with(GroupTolerance, 0.8)
        .with(Boldness, 0.3)
        .with(Fur, 0.5)
        .with(FatStores, 0.5)
        .with(LegLength, 0.6)
        .with(Hooves, 0.5)
        .with(Eyes, 0.4)
        .with(Ears, 0.4)
        .with(Vigilance, 0.5)
        .founders(40)
}

/// PocketNomad5's re-founded flier (`docs/playtest-5-logs/PocketNomad5.md`;
/// playtest 3's PocketNomad3 design was the same class): cold-blooded,
/// live-bearing, brumating, very toxic and bright, scaled, 3 kg; founded
/// as a re-found (10 of its 20 founders).
fn flier() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Flier", Thermo::Cold, BirthType::Live, 3)
        .with(BodySize, 0.45)
        .with(Wings, 0.8)
        .with(Toxins, 0.8)
        .with(Coloration, 0.8)
        .with(Brumation, 0.6)
        .with(Scales, 0.4)
        .founders(10)
}

/// LateComer4's winner (`docs/playtest-4-logs/LateComer4.md`): a
/// cold-blooded, live-bearing, brumating, toxic, honestly coloured flier of
/// 4 kg, 62 founders, founded on day 320.
fn cold_flier() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Cold", Thermo::Cold, BirthType::Live, 3)
        .with(BodySize, 0.5)
        .with(Wings, 0.8)
        .with(Toxins, 0.6)
        .with(Coloration, 0.6)
        .with(Brumation, 0.7)
        .founders(62)
}

/// Herdmaster6's "Rolling Thunder Herd" (`docs/playtest-6-logs/Herdmaster6.md`,
/// playtest 6's runaway): a small warm, egg-incubating, hibernating, toxic,
/// bright, very sociable herd of 50, founded on day 28.
fn herd() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Herd", Thermo::Warm, BirthType::IncubatedEggs, 6)
        .with(BodySize, 0.3)
        .with(FatStores, 0.6)
        .with(Fur, 0.6)
        .with(Hibernation, 0.55)
        .with(GroupTolerance, 0.95)
        .with(Obedience, 0.9)
        .with(Toxins, 0.6)
        .with(Coloration, 0.6)
        .with(Ears, 0.3)
        .with(Vigilance, 0.4)
        .with(LegLength, 0.3)
        .with(ForagingRange, 0.6)
        .with(Wanderlust, 0.3)
        .with(Pickiness, 0.3)
        .founders(50)
}

/// PocketNomad7's "Pocket Sleepers" (`docs/playtest-7-logs/PocketNomad7.md`,
/// a copy of the leading class from the starter, so its litter of 3): warm,
/// incubated eggs, small, toxic, bright, hibernating and estivating, very
/// sociable; a re-found (20 of 40 founders) on day 234.
fn sleepers() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Sleepers", Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(BodySize, 0.25)
        .with(Toxins, 0.7)
        .with(Coloration, 0.8)
        .with(Hibernation, 0.6)
        .with(Estivation, 0.4)
        .with(GroupTolerance, 0.95)
        .founders(20)
}

/// Rulebender7's "Loophole Herd" (`docs/playtest-7-logs/Rulebender7.md`):
/// Herdmaster6's design with estivation 0.35 and sociability 0.9, 40
/// founders, founded on day 34.
fn loopherd() -> FounderDesign {
    let mut d = herd().with(Gene::Estivation, 0.35).with(Gene::GroupTolerance, 0.9).founders(40);
    // Its own lineage name (the server refuses another player's).
    d.name = "Rolling Copies".into();
    d
}

/// Rulebender5's "Loophole Fliers" (`docs/playtest-5-logs/Rulebender5.md`):
/// a small cold-blooded, egg-incubating, brumating, toxic, bright flier,
/// re-founded at half size (22 of 44 founders) on day 334.
fn loophole() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Loophole", Thermo::Cold, BirthType::IncubatedEggs, 3)
        .with(BodySize, 0.25)
        .with(Wings, 0.8)
        .with(Toxins, 0.6)
        .with(Coloration, 0.6)
        .with(Brumation, 0.5)
        .founders(22)
}

/// `d` with as many of its founders as fit today's prices.
fn fitted(mut d: FounderDesign) -> FounderDesign {
    while d.point_cost() > DESIGN_BUDGET + 1e-3 && d.founders > 2 {
        d.founders -= 1;
    }
    d
}

#[derive(Default, Clone)]
struct Outcome {
    founders: BTreeMap<PlayerId, u32>,
    final_share: BTreeMap<PlayerId, f64>,
    peak_share: BTreeMap<PlayerId, f64>,
    /// Mean share over the days after Eggs founded.
    mean_share: BTreeMap<PlayerId, f64>,
    peak_alive: BTreeMap<PlayerId, usize>,
    final_alive: BTreeMap<PlayerId, usize>,
    titles: BTreeMap<PlayerId, u32>,
    seasons: u32,
    reinforced: u32,
    /// Eggs: wild alleles in its lineage at the end, and the largest fall
    /// of its adults over 60 days (the boom cohort ageing out).
    eggs_wild: f64,
    eggs_crash: f64,
    founded_day: u64,
    /// Players whose lineage reached 95% of the hard cap.
    capped: BTreeMap<PlayerId, bool>,
}

fn data(out: &[(evo_server::game::Target, ServerMsg)]) -> Option<serde_json::Value> {
    match &out.first()?.1 {
        ServerMsg::Result { ok: true, data, .. } => data.clone(),
        _ => None,
    }
}

fn run(seed: u64, new_rules: bool, a: &Args) -> Outcome {
    let world = build_world(&WorldSettings { seed, width: a.size, height: a.size, ..Default::default() });
    let mut g = Game::new(world, GameConfig::default());
    if new_rules {
        for (k, x) in &a.sets {
            assert!(g.world.config.balance.set(k, *x), "unknown balance field {k}");
        }
    }
    if !new_rules {
        // Default: playtest 7's rules (the version 64 tunables off: hunger
        // and the cap lower only the learned lesson, a bite's aversion to
        // bright prey lasts for life, player and wild lineages fertile by
        // body plan alone).
        let b = &mut g.world.config.balance;
        (b.honest_signals, b.wild_isolation) = (0.0, 0.0);
        if a.playtest2_baseline {
            (b.toxin_upkeep, b.hungry_hunters, b.toxin_source, b.speciation_credit, b.clutch_cover) =
                (0.0, 0.0, 0.0, 0.0, 0.0);
            (b.thermo_hybrid, b.flight_upkeep, b.avoid_cap, b.cold_heat, b.species_credit) = (0.0, 0.0, 0.0, 0.0, 0.0);
            (b.winter_rest, b.toxic_avoidance, b.egg_attendance, b.player_species, b.raised_cap) =
                (0.0, 0.0, 0.0, 0.0, 0.0);
            (b.eye_hybrid, b.winter_toll, b.wake_spread, b.dormancy_toll) = (0.0, 0.0, 0.0, 0.08);
            // `--baseline playtest2`: the rules of playtest 2.
            (b.egg_hybrid, b.age_spread, b.dormancy_toll, b.emergence_lag) = (0.0, 0.0, 0.0, 0.0);
            (b.lineage_crowding, b.crowding_ceiling, b.crowding_hunger) = (4.0, 2.0, 0.3);
        }
    }
    for (k, n) in NAMES.iter().enumerate() {
        g.set_name(k as PlayerId + 1, n);
    }
    let mut o = Outcome::default();
    let found = |g: &mut Game, p: PlayerId, d: FounderDesign, o: &mut Outcome| {
        let d = if new_rules { fitted(d) } else { d };
        o.founders.insert(p, d.founders);
        if new_rules {
            g.found(p, &d).expect("founded");
        } else {
            g.found_at_any_price(p, &d).expect("founded");
        }
    };
    found(&mut g, STEADY, steady(), &mut o);
    found(&mut g, BIG, big(), &mut o);
    found(&mut g, BROKER, broker(), &mut o);
    let tpd = g.world.config.ticks_per_day as u64;
    let year = 4 * g.world.config.days_per_season() as u64;
    let mut founded: Option<u64> = None;
    let mut records = Vec::new();
    let mut adults_by_day: Vec<usize> = Vec::new();
    for day in 0..a.days {
        if day == a.flier_day {
            found(&mut g, FLIER, flier(), &mut o);
        }
        if day == a.cold_day {
            found(&mut g, COLD, cold_flier(), &mut o);
        }
        if day == a.loophole_day {
            found(&mut g, LOOPHOLE, loophole(), &mut o);
        }
        if day == a.herd_day {
            found(&mut g, HERD, herd(), &mut o);
        }
        if day == SLEEPERS_DAY {
            found(&mut g, SLEEPERS, sleepers(), &mut o);
        }
        if day == LOOPHERD_DAY {
            found(&mut g, LOOPHERD, loopherd(), &mut o);
        }
        if founded.is_none() && day >= a.late_year * year && g.world.season() == Season::Winter {
            found(&mut g, EGGS, eggrunner(), &mut o);
            founded = Some(day);
        }
        if founded.is_some() {
            let shop = data(&g.command(EGGS as u64, EGGS, None, ClientMsg::RenownShop)).unwrap_or_default();
            let price = shop["reinforce_price"].as_u64().unwrap_or(u64::MAX);
            if shop.get("reinforce_blocked").is_none()
                && g.balance(EGGS) >= price
                && data(&g.command(EGGS as u64, EGGS, None, ClientMsg::Reinforce)).is_some()
            {
                o.reinforced += 1;
            }
            adults_by_day.push(g.world.organisms.iter().filter(|x| x.alive && x.owner == EGGS && x.matured).count());
        }
        for _ in 0..tpd {
            g.world.step();
            g.after_tick();
        }
        for ev in g.take_persist() {
            if let evo_server::game::PersistEvent::Daily(r) = ev {
                records.push(r);
            }
        }
        for p in 1..=PLAYERS {
            let alive = g.world.owner_count(p);
            let peak = o.peak_alive.entry(p).or_default();
            *peak = (*peak).max(alive);
            o.final_alive.insert(p, alive);
        }
    }
    let ceiling = g.world.player_ceiling();
    for (&p, &peak) in &o.peak_alive {
        o.capped.insert(p, peak as f64 >= 0.95 * ceiling as f64);
    }
    let start = founded.unwrap_or(a.days);
    o.founded_day = start;
    if let Some(r) = records.last() {
        for e in r.legacy.iter().filter(|e| e.player != 0) {
            o.final_share.insert(e.player, e.share);
        }
    }
    let after: Vec<_> = records.iter().filter(|r| r.day >= start).collect();
    let n = after.len().max(1) as f64;
    for r in &after {
        for e in r.legacy.iter().filter(|e| e.player != 0) {
            *o.mean_share.entry(e.player).or_default() += e.share / n;
            let p = o.peak_share.entry(e.player).or_default();
            *p = p.max(e.share);
        }
    }
    // Legacy titles of the whole seasons after Eggs founded (raw share).
    let mut sums: BTreeMap<u64, BTreeMap<PlayerId, f64>> = BTreeMap::new();
    for r in &after {
        if r.day / SEASON > start / SEASON {
            for e in r.legacy.iter().filter(|e| e.player != 0) {
                *sums.entry(r.day / SEASON).or_default().entry(e.player).or_default() += e.share;
            }
        }
    }
    for s in sums.values() {
        o.seasons += 1;
        if let Some((&p, _)) = s.iter().max_by(|a, b| a.1.total_cmp(b.1)) {
            *o.titles.entry(p).or_default() += 1;
        }
    }
    o.eggs_wild = g
        .lineage_stats(EGGS)
        .score
        .and_then(|s| s.carried.iter().find(|c| c.player == 0).map(|c| c.share))
        .unwrap_or(0.0);
    let mut crash: f64 = 0.0;
    for i in 60..adults_by_day.len() {
        let peak = adults_by_day[i - 60..i].iter().copied().max().unwrap_or(0) as f64;
        if peak >= 200.0 {
            crash = crash.max(1.0 - adults_by_day[i] as f64 / peak);
        }
    }
    o.eggs_crash = crash;
    o
}

fn main() {
    let a = args();
    for d in [steady(), big(), broker(), eggrunner(), flier(), cold_flier(), loophole(), herd(), sleepers(), loopherd()]
    {
        let f = fitted(d.clone());
        println!(
            "{}: {:.1} points at today's prices, {} of {} founders fit",
            d.name,
            d.point_cost(),
            f.founders,
            d.founders
        );
    }
    let arms: &[bool] = if a.new_only { &[true] } else { &[false, true] };
    let jobs: Vec<(u64, bool)> = (0..a.seeds).flat_map(|k| arms.iter().map(move |&n| (a.first_seed + k, n))).collect();
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).min(jobs.len()).max(1);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&(seed, new)) = jobs.get(k) else { break };
                let o = run(seed, new, &a);
                println!(
                    "  seed {seed} {}: founded day {} final {:?} peak share {:?} peak alive {:?} final alive {:?} titles {:?} reinforced {}",
                    if new { "new" } else { "old" },
                    o.founded_day,
                    o.final_share.iter().map(|(p, v)| format!("{}:{:.1}%", NAMES[*p as usize - 1], v * 100.0)).collect::<Vec<_>>(),
                    o.peak_share.iter().map(|(p, v)| format!("{}:{:.1}%", NAMES[*p as usize - 1], v * 100.0)).collect::<Vec<_>>(),
                    o.peak_alive,
                    o.final_alive,
                    o.titles,
                    o.reinforced
                );
                results.lock().unwrap().push((seed, new, o));
            });
        }
    });
    let results = results.into_inner().unwrap();
    let old_title = if a.playtest2_baseline { "Playtest 2 rules" } else { "Playtest 7 rules" };
    for (new, title) in [(false, old_title), (true, "New rules")] {
        if a.new_only && !new {
            continue;
        }
        let os: Vec<&Outcome> = results.iter().filter(|r| r.1 == new).map(|r| &r.2).collect();
        let n = os.len().max(1) as f64;
        let seasons: u32 = os.iter().map(|o| o.seasons).sum();
        println!(
            "\n{title}, {} seeds, {} days (Eggs founded on day {:.0} on average):",
            os.len(),
            a.days,
            os.iter().map(|o| o.founded_day as f64).sum::<f64>() / n
        );
        println!(
            "  player   founders  final share  peak share  mean share*  peak alive  final alive  extinct  capped  titles*"
        );
        for p in 1..=PLAYERS {
            let avg = |f: &dyn Fn(&Outcome) -> f64| os.iter().map(|o| f(o)).sum::<f64>() / n;
            println!(
                "  {:<8} {:>8.0}  {:>10.1}%  {:>9.1}%  {:>10.1}%  {:>10.0}  {:>11.0}  {:>7}  {:>6}  {}/{}",
                NAMES[p as usize - 1],
                avg(&|o| o.founders.get(&p).copied().unwrap_or(0) as f64),
                100.0 * avg(&|o| o.final_share.get(&p).copied().unwrap_or(0.0)),
                100.0 * avg(&|o| o.peak_share.get(&p).copied().unwrap_or(0.0)),
                100.0 * avg(&|o| o.mean_share.get(&p).copied().unwrap_or(0.0)),
                avg(&|o| o.peak_alive.get(&p).copied().unwrap_or(0) as f64),
                avg(&|o| o.final_alive.get(&p).copied().unwrap_or(0) as f64),
                os.iter().filter(|o| o.final_alive.get(&p).copied().unwrap_or(0) == 0).count(),
                os.iter().filter(|o| o.capped.get(&p).copied().unwrap_or(false)).count(),
                os.iter().map(|o| o.titles.get(&p).copied().unwrap_or(0)).sum::<u32>(),
                seasons,
            );
        }
        println!(
            "  Eggs: reinforcements {:.1}/world, wild alleles in its lineage {:.1}%, worst 60-day fall of its adults {:.0}%",
            os.iter().map(|o| o.reinforced as f64).sum::<f64>() / n,
            100.0 * os.iter().map(|o| o.eggs_wild).sum::<f64>() / n,
            100.0 * os.iter().map(|o| o.eggs_crash).sum::<f64>() / n,
        );
        let fliers: u32 = os
            .iter()
            .map(|o| [FLIER, COLD, LOOPHOLE].iter().map(|p| o.titles.get(p).copied().unwrap_or(0)).sum::<u32>())
            .sum();
        println!("  cold fliers as a class (Flier + Cold + Loophole): {fliers}/{seasons} titles");
        let eggers: u32 =
            os.iter().map(|o| [EGGS, HERD].iter().map(|p| o.titles.get(p).copied().unwrap_or(0)).sum::<u32>()).sum();
        println!("  warm egg-laying hibernators as a class (Eggs + Herd): {eggers}/{seasons} titles");
        println!(
            "  (* from Eggs' founding on: mean share over those days; legacy titles of the whole seasons after it)"
        );
    }
}
