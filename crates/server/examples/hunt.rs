//! A predator player against herbivore players, through the server's own
//! game rules (`Game`: founding, possession and `possess_attack`), to see
//! whether deliberate hunting pays the predator without wiping out the
//! other players' species, and whether a late joiner's founding site can be
//! camped (*Predators against players* in the README).
//!
//! `cargo run --release -p evo-server --example hunt -- [--seeds N]
//!  [--first-seed N] [--days D] [--size N] [--late-day D] [--every TICKS]
//!  [--grace DAYS]` (`--grace`: `Balance::attack_grace` where it is on)
//!
//! Players, on the standard world with its wild species:
//!
//! - **Steady** and **Herd** found the client's starter herbivore (40
//!   founders each, one of them with points on legs and vigilance).
//! - **Hunter** founds a pack predator (40 founders: carnivory 0.9, pack
//!   instinct 0.95).
//! - **Late** founds the starter herbivore on `--late-day` (default 120).
//!
//! Arms on the same seeds: **passive** (the Hunter's animals hunt on their
//! own, today's rules), **deliberate** (every `--every` ticks, default 6,
//! with no order standing, the Hunter possesses a grown, healthy, hungry
//! hunter and orders it at the heaviest organism in sight it may attack:
//! Late's from the day Late founds, else the other players', else the
//! wild's; its pack-mates join) and **camping** (the same, and for 20 days
//! from Late's founding the Hunter migrates its pack to Late's founding
//! site: the spawn-camping case), with the attack grace on (orders at Late
//! refused for its first 10 days) and off.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use evo_server::game::{build_world, Game, GameConfig, WorldSettings};
use evo_server::protocol::{AlertKind, ClientMsg, ServerMsg};
use evo_sim::{BirthType, FounderDesign, Gene, PlayerId, Thermo, WILD};

const STEADY: PlayerId = 1;
const HERD: PlayerId = 2;
const HUNTER: PlayerId = 3;
const LATE: PlayerId = 4;
const NAMES: [&str; 4] = ["Steady", "Herd", "Hunter", "Late"];
/// Hunger above which the Hunter sends an animal hunting.
const HUNGRY: f32 = 35.0;

struct Args {
    seeds: u64,
    first_seed: u64,
    days: u64,
    size: usize,
    late_day: u64,
    every: u64,
    /// `Balance::attack_grace` in the arms with the grace on (None: the default).
    grace: Option<f32>,
}

fn args() -> Args {
    let mut a = Args { seeds: 6, first_seed: 1, days: 240, size: 128, late_day: 120, every: 6, grace: None };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| panic!("{k} needs a value"));
        match k.as_str() {
            "--seeds" => a.seeds = v().parse().expect("--seeds"),
            "--first-seed" => a.first_seed = v().parse().expect("--first-seed"),
            "--days" => a.days = v().parse().expect("--days"),
            "--size" => a.size = v().parse().expect("--size"),
            "--late-day" => a.late_day = v().parse().expect("--late-day"),
            "--every" => a.every = v().parse().expect("--every"),
            "--grace" => a.grace = Some(v().parse().expect("--grace")),
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

/// The predator player's pack (playtest 1's ApexHunter, 40 founders).
fn pack(name: &str) -> FounderDesign {
    FounderDesign::new(name, Thermo::Warm, BirthType::Live, 4)
        .with(Gene::BodySize, 0.30)
        .with(Gene::Carnivory, 0.9)
        .with(Gene::PackInstinct, 0.95)
        .with(Gene::GroupTolerance, 0.85)
        .with(Gene::Aggression, 0.6)
        .with(Gene::Boldness, 0.65)
        .with(Gene::LegLength, 0.7)
        .with(Gene::Muscle, 0.6)
        .with(Gene::Claws, 0.5)
        .with(Gene::Fangs, 0.5)
        .with(Gene::Eyes, 0.6)
        .founders(40)
}

#[derive(Clone, Copy, PartialEq)]
enum Arm {
    Passive,
    Deliberate,
    Camping { grace: bool },
}

const ARMS: [(Arm, &str); 4] = [
    (Arm::Passive, "Passive (the Hunter's animals hunt on their own)"),
    (Arm::Deliberate, "Deliberate (attack orders)"),
    (Arm::Camping { grace: true }, "Camping Late's founding site, grace on"),
    (Arm::Camping { grace: false }, "Camping Late's founding site, grace off"),
];

#[derive(Default, Clone)]
struct Outcome {
    final_alive: BTreeMap<PlayerId, usize>,
    mean_share: BTreeMap<PlayerId, f64>,
    peak_alive: BTreeMap<PlayerId, usize>,
    /// The Hunter's kills by the victim's owner (0: the wild).
    kills: BTreeMap<PlayerId, u64>,
    /// Late's head count 10 days after founding (None if it never founded).
    late_day10: Option<usize>,
    orders: u32,
    refused: BTreeMap<String, u32>,
    alerts: u32,
}

fn ok(out: &[(evo_server::game::Target, ServerMsg)]) -> Result<(), String> {
    match &out.first().map(|o| &o.1) {
        Some(ServerMsg::Result { ok: true, .. }) => Ok(()),
        Some(ServerMsg::Result { error: Some(e), .. }) => Err(e.code.clone()),
        _ => Err("no reply".into()),
    }
}

/// The Hunter's turn: if no attack order stands, possess a grown, healthy,
/// hungry hunter and order it at the heaviest organism in sight it may
/// attack (nearer ones first among equals), Late's first when
/// `prefer_late`, then the other players', then the wild's.
fn hunter_turn(g: &mut Game, prefer_late: bool, o: &mut Outcome) {
    let busy = g.world.organisms.iter().any(|x| {
        x.alive && x.owner == HUNTER && x.possessed_by == Some(HUNTER) && g.world.attack_target(x.id).is_some()
    });
    if busy {
        return;
    }
    let mut hunters: Vec<(u64, evo_sim::math::Vec2)> = g
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
        .map(|x| (x.id, x.pos))
        .collect();
    hunters.truncate(8);
    let rank = |owner: PlayerId| match owner {
        LATE if prefer_late => 0,
        WILD => 2,
        _ => 1,
    };
    for (id, pos) in hunters {
        // The most meat first (the pack shares the kill), nearer ones
        // first among equals; refused ones (too big) are skipped.
        let mut targets: Vec<(u32, f32, u64)> = g
            .world
            .organisms
            .iter()
            .filter(|x| x.alive && x.owner != HUNTER && !x.sheltered && x.pos.dist(pos) <= 8.0)
            .map(|x| (rank(x.owner), -x.mass() / (1.0 + 0.2 * x.pos.dist(pos)), x.id))
            .collect();
        targets.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        if targets.is_empty() {
            continue;
        }
        if ok(&g.command(HUNTER as u64, HUNTER, None, ClientMsg::Possess { organism: id })).is_err() {
            continue;
        }
        for &(_, _, t) in targets.iter().take(4) {
            let out = g.command(HUNTER as u64, HUNTER, None, ClientMsg::PossessAttack { organism: id, target: t });
            match ok(&out) {
                Ok(()) => {
                    o.orders += 1;
                    o.alerts += out
                        .iter()
                        .filter(|(to, m)| {
                            matches!(to, evo_server::game::Target::Player(p) if *p != HUNTER)
                                && matches!(m, ServerMsg::Alert { alert } if alert.kind == AlertKind::Attack)
                        })
                        .count() as u32;
                    return;
                }
                Err(code) => *o.refused.entry(code).or_default() += 1,
            }
        }
    }
}

fn run(seed: u64, arm: Arm, a: &Args) -> Outcome {
    let world = build_world(&WorldSettings { seed, width: a.size, height: a.size, ..Default::default() });
    let mut g = Game::new(world, GameConfig::default());
    if let Some(d) = a.grace {
        g.world.config.balance.attack_grace = d;
    }
    if arm == (Arm::Camping { grace: false }) {
        g.world.config.balance.attack_grace = 0.0;
    }
    for (k, n) in NAMES.iter().enumerate() {
        g.set_name(k as PlayerId + 1, n);
    }
    let mut o = Outcome::default();
    g.found_at_any_price(STEADY, &starter("Steady")).expect("founded");
    let herd = starter("Herd").with(Gene::LegLength, 0.5).with(Gene::Vigilance, 0.6);
    g.found_at_any_price(HERD, &herd).expect("founded");
    g.found_at_any_price(HUNTER, &pack("Hunter")).expect("founded");
    let tpd = g.world.config.ticks_per_day as u64;
    let mut days = 0.0;
    for day in 0..a.days {
        if day == a.late_day {
            g.found_at_any_price(LATE, &starter("Late")).expect("founded");
        }
        // The camper takes its pack to Late's founding site and keeps it there.
        if matches!(arm, Arm::Camping { .. }) && day >= a.late_day && day < a.late_day + 20 {
            let late: Vec<_> = g.world.organisms.iter().filter(|x| x.alive && x.owner == LATE).map(|x| x.pos).collect();
            if let Some(&first) = late.first() {
                let c = late.iter().fold(first * 0.0, |s, &p| s + p) * (1.0 / late.len() as f32);
                g.command(HUNTER as u64, HUNTER, None, ClientMsg::Migrate { x: c.x, y: c.y, group: None });
            }
        }
        if day == a.late_day + 10 {
            o.late_day10 = Some(g.world.owner_count(LATE));
        }
        for t in 0..tpd {
            if arm != Arm::Passive && t % a.every.max(1) == 0 {
                hunter_turn(&mut g, day >= a.late_day, &mut o);
            }
            g.world.step();
            g.after_tick();
        }
        let alive: Vec<usize> = (1..=4).map(|p| g.world.owner_count(p)).collect();
        let total = alive.iter().sum::<usize>().max(1) as f64;
        for p in 1..=4u32 {
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
    for p in 1..=4 {
        o.final_alive.insert(p, g.world.owner_count(p));
    }
    let owner_of: Vec<PlayerId> = g.world.lineages.iter().map(|l| l.owner).collect();
    for (&(prey, killer), &n) in &g.world.counters.kills_by_lineage {
        if owner_of.get(killer as usize) == Some(&HUNTER) {
            *o.kills.entry(owner_of.get(prey as usize).copied().unwrap_or(WILD)).or_default() += n;
        }
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
                    "  seed {seed} arm {arm}: final {:?} kills {:?} late@10d {:?} orders {} refused {:?}",
                    o.final_alive, o.kills, o.late_day10, o.orders, o.refused
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
        for p in 1..=4u32 {
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
        let mut refused: BTreeMap<String, u32> = BTreeMap::new();
        for o in &os {
            for (k, v) in &o.refused {
                *refused.entry(k.clone()).or_default() += v;
            }
        }
        println!(
            "  Hunter's wild kills {:.0}/world; attack orders {:.0}/world, refused {:?}; attack alerts to victims {:.0}/world; Late alive 10 days after founding {:.1}",
            avg(&|o| o.kills.get(&WILD).copied().unwrap_or(0) as f64),
            avg(&|o| o.orders as f64),
            refused,
            avg(&|o| o.alerts as f64),
            avg(&|o| o.late_day10.unwrap_or(0) as f64),
        );
    }
}
