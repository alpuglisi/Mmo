//! Playtest 2's predator and toxin findings, replayed through the server's
//! own game rules (`Game`) in a six-player world: is a designed carnivore
//! viable now, and do drab toxins still come free (*Playtest 2: predators,
//! toxins and drowning* in the README)?
//!
//! `cargo run --release -p evo-server --example predators -- [--seeds N]
//!  [--first-seed N] [--days D] [--size N] [--every TICKS]`
//!
//! Players, on the standard world with its wild species:
//!
//! - **Steady**, **Herd** and **Nomad** found the client's starter
//!   herbivore (40 founders; Herd with points on legs and vigilance, Nomad
//!   smaller and faster);
//! - **Hunter** founds playtest 2's "Apex Wolves" (ApexHunter2's v2: body
//!   0.4, carnivory 0.92, pack instinct 0.95, 42 founders), the design that
//!   starved;
//! - **Toxic** founds LateComer's winning recipe: an egg-laying, drab
//!   (coloration 0.1), toxic (0.8), burrowing, hibernating herbivore (60
//!   founders);
//! - **Late** founds the starter on day 40.
//!
//! Arms on the same seeds: **old rules** (the version 44 tunables off:
//! `drink_priority`, `toxic_learning`, `dry_sites`, `carnivore_energetics`,
//! `led_pack`), the Hunter's pack hunting on its own; **new rules**, the
//! same; and **new rules, deliberate**: every `--every` ticks (default 6)
//! with no order standing, the Hunter possesses a grown, healthy, hungry
//! hunter and orders it at the heaviest animal in sight it may attack
//! (players' first), passing up any that look poisonous to it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use evo_server::game::{build_world, Game, GameConfig, WorldSettings};
use evo_server::protocol::{ClientMsg, ServerMsg};
use evo_sim::{BirthType, FounderDesign, Gene, PlayerId, Thermo, WILD};

const STEADY: PlayerId = 1;
const HERD: PlayerId = 2;
const HUNTER: PlayerId = 3;
const TOXIC: PlayerId = 4;
const NOMAD: PlayerId = 5;
const LATE: PlayerId = 6;
const NAMES: [&str; 6] = ["Steady", "Herd", "Hunter", "Toxic", "Nomad", "Late"];
/// Hunger above which the Hunter sends an animal hunting.
const HUNGRY: f32 = 35.0;

struct Args {
    seeds: u64,
    first_seed: u64,
    days: u64,
    size: usize,
    every: u64,
}

fn args() -> Args {
    let mut a = Args { seeds: 4, first_seed: 1, days: 240, size: 128, every: 6 };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| panic!("{k} needs a value"));
        match k.as_str() {
            "--seeds" => a.seeds = v().parse().expect("--seeds"),
            "--first-seed" => a.first_seed = v().parse().expect("--first-seed"),
            "--days" => a.days = v().parse().expect("--days"),
            "--size" => a.size = v().parse().expect("--size"),
            "--every" => a.every = v().parse().expect("--every"),
            _ => panic!("unknown argument {k}"),
        }
    }
    a
}

/// The client's starting design (`docs/starter-design.md`).
fn starter(name: &str) -> FounderDesign {
    FounderDesign::new(name, Thermo::Warm, BirthType::Live, 2)
        .with(Gene::BodySize, 0.5)
        .with(Gene::GroupTolerance, 0.9)
        .with(Gene::Boldness, 0.3)
        .with(Gene::Fur, 0.6)
        .with(Gene::FatStores, 0.6)
        .with(Gene::Hibernation, 0.5)
        .founders(40)
}

/// ApexHunter2's v2, "Apex Wolves".
fn wolves(name: &str) -> FounderDesign {
    use Gene::*;
    FounderDesign::new(name, Thermo::Warm, BirthType::Live, 4)
        .with(BodySize, 0.4)
        .with(Carnivory, 0.92)
        .with(PackInstinct, 0.95)
        .with(GroupTolerance, 0.85)
        .with(LegLength, 0.7)
        .with(Muscle, 0.6)
        .with(Fangs, 0.8)
        .with(Claws, 0.6)
        .with(Eyes, 0.6)
        .with(Nose, 0.5)
        .with(Fur, 0.5)
        .with(FatStores, 0.7)
        .with(Torpor, 0.4)
        .with(Caching, 0.4)
        .with(Camouflage, 0.4)
        .with(Vigilance, 0.3)
        .with(Ears, 0.3)
        .with(SeasonalCoat, 0.4)
        .founders(42)
}

/// LateComer's recipe: egg-laying, drab, toxic, burrowing, hibernating.
fn toxic(name: &str) -> FounderDesign {
    use Gene::*;
    FounderDesign::new(name, Thermo::Warm, BirthType::IncubatedEggs, 4)
        .with(BodySize, 0.5)
        .with(Toxins, 0.8)
        .with(Coloration, 0.1)
        .with(Burrowing, 0.8)
        .with(Hibernation, 0.6)
        .with(Fur, 0.5)
        .with(FatStores, 0.6)
        .with(GroupTolerance, 0.8)
        .founders(60)
}

#[derive(Clone, Copy, PartialEq)]
enum Arm {
    Old,
    New,
    Deliberate,
}

const ARMS: [(Arm, &str); 3] = [
    (Arm::Old, "Old rules (the version 44 tunables off), the pack hunts on its own"),
    (Arm::New, "New rules, the pack hunts on its own"),
    (Arm::Deliberate, "New rules, deliberate hunting (attack orders)"),
];

#[derive(Default, Clone)]
struct Outcome {
    final_alive: BTreeMap<PlayerId, usize>,
    mean_share: BTreeMap<PlayerId, f64>,
    peak_alive: BTreeMap<PlayerId, usize>,
    /// The Hunter's kills by the victim's owner (0: the wild).
    kills: BTreeMap<PlayerId, u64>,
    /// Hunter deaths by cause.
    hunter_deaths: BTreeMap<String, u64>,
    /// Toxic's animals killed by any predator.
    toxic_eaten: u64,
    /// The Hunter's lesson about the Toxic lineage at the end.
    lesson: f32,
    orders: u32,
    warned: u32,
}

fn ok(out: &[(evo_server::game::Target, ServerMsg)]) -> Result<Option<serde_json::Value>, String> {
    match &out.first().map(|o| &o.1) {
        Some(ServerMsg::Result { ok: true, data, .. }) => Ok(data.clone()),
        Some(ServerMsg::Result { error: Some(e), .. }) => Err(e.code.clone()),
        _ => Err("no reply".into()),
    }
}

/// The Hunter's turn (`hunt.rs`): with no order standing, possess a grown,
/// healthy, hungry hunter and order it at the heaviest animal in sight it
/// may attack, players' first, passing up any that looks poisonous to it.
fn hunter_turn(g: &mut Game, o: &mut Outcome) {
    let busy = g.world.organisms.iter().any(|x| {
        x.alive && x.owner == HUNTER && x.possessed_by == Some(HUNTER) && g.world.attack_target(x.id).is_some()
    });
    if busy {
        return;
    }
    let mut hunters: Vec<(u64, evo_sim::math::Vec2, u32)> = g
        .world
        .organisms
        .iter()
        .filter(|x| {
            x.alive
                && x.owner == HUNTER
                && x.matured
                && x.health > 60.0
                && x.needs.thirst < 60.0
                && x.needs.hunger > HUNGRY
        })
        .map(|x| (x.id, x.pos, x.lineage))
        .collect();
    hunters.truncate(8);
    for (id, pos, lineage) in hunters {
        let mut targets: Vec<(u32, f32, u64)> = g
            .world
            .organisms
            .iter()
            .filter(|x| x.alive && x.owner != HUNTER && !x.sheltered && x.pos.dist(pos) <= 8.0)
            .filter(|x| g.world.toxic_lesson(lineage, x.lineage) < evo_sim::world::toxic::LOOKS_POISONOUS)
            .map(|x| (u32::from(x.owner == WILD), -x.mass() / (1.0 + 0.2 * x.pos.dist(pos)), x.id))
            .collect();
        targets.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        if targets.is_empty() {
            continue;
        }
        if ok(&g.command(HUNTER as u64, HUNTER, None, ClientMsg::Possess { organism: id })).is_err() {
            continue;
        }
        for &(_, _, t) in targets.iter().take(4) {
            if let Ok(d) =
                ok(&g.command(HUNTER as u64, HUNTER, None, ClientMsg::PossessAttack { organism: id, target: t }))
            {
                o.orders += 1;
                o.warned += u32::from(d.is_some_and(|d| d.get("warning").is_some()));
                return;
            }
        }
    }
}

fn run(seed: u64, arm: Arm, a: &Args) -> Outcome {
    let world = build_world(&WorldSettings { seed, width: a.size, height: a.size, ..Default::default() });
    let mut g = Game::new(world, GameConfig::default());
    if arm == Arm::Old {
        let b = &mut g.world.config.balance;
        (b.drink_priority, b.toxic_learning, b.dry_sites, b.carnivore_energetics, b.led_pack) =
            (0.0, 0.0, 0.0, 0.0, 0.0);
    }
    for (k, n) in NAMES.iter().enumerate() {
        g.set_name(k as PlayerId + 1, n);
    }
    let mut o = Outcome::default();
    g.found_at_any_price(STEADY, &starter("Steady")).expect("founded");
    let herd = starter("Herd").with(Gene::LegLength, 0.5).with(Gene::Vigilance, 0.6);
    g.found_at_any_price(HERD, &herd).expect("founded");
    g.found_at_any_price(HUNTER, &wolves("Hunter")).expect("founded");
    g.found_at_any_price(TOXIC, &toxic("Toxic")).expect("founded");
    let nomad = starter("Nomad").with(Gene::BodySize, 0.35).with(Gene::LegLength, 0.6);
    g.found_at_any_price(NOMAD, &nomad).expect("founded");
    let tpd = g.world.config.ticks_per_day as u64;
    let mut days = 0.0;
    for day in 0..a.days {
        if day == 40 {
            g.found_at_any_price(LATE, &starter("Late")).expect("founded");
        }
        for t in 0..tpd {
            if arm == Arm::Deliberate && t % a.every.max(1) == 0 {
                hunter_turn(&mut g, &mut o);
            }
            g.world.step();
            g.after_tick();
        }
        let alive: Vec<usize> = (1..=6).map(|p| g.world.owner_count(p)).collect();
        let total = alive.iter().sum::<usize>().max(1) as f64;
        for p in 1..=6u32 {
            let n = alive[p as usize - 1];
            *o.mean_share.entry(p).or_default() += n as f64 / total;
            let peak = o.peak_alive.entry(p).or_default();
            *peak = (*peak).max(n);
        }
        days += 1.0;
    }
    for v in o.mean_share.values_mut() {
        *v /= days;
    }
    for p in 1..=6 {
        o.final_alive.insert(p, g.world.owner_count(p));
    }
    let owner_of: Vec<PlayerId> = g.world.lineages.iter().map(|l| l.owner).collect();
    let owned = |l: u32, p: PlayerId| owner_of.get(l as usize) == Some(&p);
    for (&(prey, killer), &n) in &g.world.counters.kills_by_lineage {
        if owned(killer, HUNTER) {
            *o.kills.entry(owner_of.get(prey as usize).copied().unwrap_or(WILD)).or_default() += n;
        }
        if owned(prey, TOXIC) {
            o.toxic_eaten += n;
        }
    }
    for (&(l, cause), &n) in &g.world.counters.deaths_by_lineage {
        if owned(l, HUNTER) {
            *o.hunter_deaths.entry(format!("{cause:?}")).or_default() += n;
        }
    }
    let lineage = |p: PlayerId| (0..owner_of.len() as u32).find(|&l| owned(l, p));
    if let (Some(h), Some(t)) = (lineage(HUNTER), lineage(TOXIC)) {
        o.lesson = g.world.toxic_lesson(h, t);
    }
    o
}

fn main() {
    let a = args();
    let jobs: Vec<(u64, usize)> =
        (0..a.seeds).flat_map(|k| (0..ARMS.len()).map(move |arm| (a.first_seed + k, arm))).collect();
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).min(jobs.len()).max(1);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&(seed, arm)) = jobs.get(k) else { break };
                let o = run(seed, ARMS[arm].0, &a);
                println!(
                    "  seed {seed} arm {arm}: final {:?} kills {:?} hunter deaths {:?} lesson {:.2} orders {}",
                    o.final_alive, o.kills, o.hunter_deaths, o.lesson, o.orders
                );
                results.lock().unwrap().push((seed, arm, o));
            });
        }
    });
    let results = results.into_inner().unwrap();
    for (arm, (_, title)) in ARMS.iter().enumerate() {
        let os: Vec<&Outcome> = results.iter().filter(|r| r.1 == arm).map(|r| &r.2).collect();
        let n = os.len().max(1) as f64;
        let avg = |f: &dyn Fn(&Outcome) -> f64| os.iter().map(|o| f(o)).sum::<f64>() / n;
        println!("\n{title}, {} seeds, {} days:", os.len(), a.days);
        println!("  player   final alive  extinct  mean share  peak alive  killed by Hunter");
        for p in 1..=6u32 {
            println!(
                "  {:<8} {:>11.0}  {:>7}  {:>9.1}%  {:>10.0}  {:>15.0}",
                NAMES[p as usize - 1],
                avg(&|o| o.final_alive.get(&p).copied().unwrap_or(0) as f64),
                os.iter().filter(|o| o.final_alive.get(&p).copied().unwrap_or(0) == 0).count(),
                100.0 * avg(&|o| o.mean_share.get(&p).copied().unwrap_or(0.0)),
                avg(&|o| o.peak_alive.get(&p).copied().unwrap_or(0) as f64),
                avg(&|o| o.kills.get(&p).copied().unwrap_or(0) as f64),
            );
        }
        let mut deaths: BTreeMap<String, f64> = BTreeMap::new();
        for o in &os {
            for (k, v) in &o.hunter_deaths {
                *deaths.entry(k.clone()).or_default() += *v as f64 / n;
            }
        }
        let deaths: Vec<String> = deaths.iter().map(|(k, v)| format!("{k} {v:.0}")).collect();
        println!(
            "  Hunter's deaths/world: {}; wild kills {:.0}/world; Toxic's animals eaten {:.0}/world; Hunter's lesson about Toxic {:.2}; orders {:.0}/world ({:.0} warned)",
            deaths.join(", "),
            avg(&|o| o.kills.get(&WILD).copied().unwrap_or(0) as f64),
            avg(&|o| o.toxic_eaten as f64),
            avg(&|o| o.lesson as f64),
            avg(&|o| o.orders as f64),
            avg(&|o| o.warned as f64),
        );
    }
}
