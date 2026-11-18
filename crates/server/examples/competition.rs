//! A scripted four-player competition through the server's own game rules
//! (`Game`: founding, the market, escrow, renown, the shop, seasons), under
//! the old rules (playtest 1) and the new ones, to see whether trading pays
//! and whether founding big or sitting at the cap still wins.
//!
//! `cargo run --release -p evo-server --example competition -- [--seeds N]
//!  [--first-seed N] [--days D] [--size N] [--late-day D]`
//!
//! Players (all herbivores of one body plan, so any two can breed):
//!
//! - **Swarm** founds as many founders as the budget buys on day 0 (old
//!   rules: 130, playtest 1's swarms; new: what the rising founder price
//!   allows) and does nothing else.
//! - **Steady** founds a normal design (40 founders, points on traits).
//! - **Broker**, the same design, sires into rivals' dams from day 30: every
//!   day, while it has renown and slots, it offers its males to Steady's and
//!   Late's females for 20 renown, and buys extra mating slots. Under the
//!   old rules the dam owners decline (accepting only dilutes them and
//!   renown buys nothing: playtest 1's zero trades); under the new ones they
//!   accept and spend what they earn on reinforcements while small.
//! - **Late** joins on `--late-day` (default 150, mid-season) with as many
//!   founders as the budget buys.
//!
//! Three arms on the same seeds. Old rules: founders at a flat 0.7 points,
//! the hard lineage cap (`lineage_crowding` 0), no market for the dam
//! owners. New, no trade: today's rules, but the dam owners still decline
//! (what the soft cap and founder price do alone). New: today's rules with
//! the market. The legacy title of each
//! 60-day ranked season is computed from the daily records both ways: the
//! old raw-share mean and the new mean of raw and weighted share.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use evo_server::game::{build_world, Game, GameConfig, WorldSettings};
use evo_server::protocol::{ClientMsg, LedgerKind, ServerMsg};
use evo_server::store::DailyRecord;
use evo_sim::design::{founder_cost, DESIGN_BUDGET};
use evo_sim::organism::Sex;
use evo_sim::{BirthType, FounderDesign, Gene, PlayerId, Thermo};

const SWARM: PlayerId = 1;
const STEADY: PlayerId = 2;
const BROKER: PlayerId = 3;
const LATE: PlayerId = 4;
const NAMES: [&str; 4] = ["Swarm", "Steady", "Broker", "Late"];
const PRICE: u64 = 20;
const SEASON: u64 = 60;

struct Args {
    seeds: u64,
    first_seed: u64,
    days: u64,
    size: usize,
    late_day: u64,
}

fn args() -> Args {
    let mut a = Args { seeds: 4, first_seed: 1, days: 360, size: 128, late_day: 150 };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| panic!("{k} needs a value"));
        match k.as_str() {
            "--seeds" => a.seeds = v().parse().expect("--seeds"),
            "--first-seed" => a.first_seed = v().parse().expect("--first-seed"),
            "--days" => a.days = v().parse().expect("--days"),
            "--size" => a.size = v().parse().expect("--size"),
            "--late-day" => a.late_day = v().parse().expect("--late-day"),
            _ => panic!("unknown argument {k}"),
        }
    }
    a
}

/// The client's starting design (`docs/starter-design.md`): a sturdy,
/// social, cautious herbivore.
fn starter(name: &str) -> FounderDesign {
    FounderDesign::new(name, Thermo::Warm, BirthType::Live, 2)
        .with(Gene::BodySize, 0.5)
        .with(Gene::GroupTolerance, 0.9)
        .with(Gene::Boldness, 0.3)
        .with(Gene::Fur, 0.6)
        .with(Gene::FatStores, 0.6)
        .with(Gene::Hibernation, 0.5)
}

/// Steady's and Broker's design: the starter with 40 founders and points on
/// senses, legs and vigilance.
fn normal(name: &str) -> FounderDesign {
    starter(name)
        .with(Gene::Eyes, 0.5)
        .with(Gene::Ears, 0.5)
        .with(Gene::LegLength, 0.5)
        .with(Gene::Vigilance, 0.6)
        .with(Gene::Vocal, 0.5)
        .founders(40)
}

/// The starter with as many founders as the budget buys: at the old flat
/// price (`old`) or today's.
fn swarm(name: &str, old: bool) -> FounderDesign {
    let base = starter(name);
    let traits = base.point_cost() - founder_cost(base.founders);
    let price = |n: u32| if old { n.saturating_sub(20) as f32 * 0.7 } else { founder_cost(n) };
    let mut n = 20;
    while traits + price(n + 1) <= DESIGN_BUDGET {
        n += 1;
    }
    base.founders(n)
}

#[derive(Default, Clone)]
struct Outcome {
    final_share: BTreeMap<PlayerId, f64>,
    mean_share: BTreeMap<PlayerId, f64>,
    titles_raw: BTreeMap<PlayerId, u32>,
    titles_new: BTreeMap<PlayerId, u32>,
    peak_alive: BTreeMap<PlayerId, usize>,
    founders: BTreeMap<PlayerId, u32>,
    trades: u32,
    fees_paid: u64,
    renown: BTreeMap<PlayerId, u64>,
    exported: f64,
    reinforced: u32,
    slots_bought: u32,
    /// Renown from royalties and from income, per player.
    royalty: BTreeMap<PlayerId, u64>,
    income: BTreeMap<PlayerId, u64>,
}

fn data(out: &[(evo_server::game::Target, ServerMsg)]) -> Option<serde_json::Value> {
    match &out.first()?.1 {
        ServerMsg::Result { ok: true, data, .. } => data.clone(),
        _ => None,
    }
}

fn adults(g: &Game, player: PlayerId, sex: Sex) -> Vec<u64> {
    g.world
        .organisms
        .iter()
        .filter(|o| o.alive && o.owner == player && o.matured && o.sex == sex && o.pregnancy.is_none())
        .map(|o| o.id)
        .collect()
}

/// Broker's day: offer males to the hosts' females, escorted at once.
fn broker_day(g: &mut Game, trade: bool, o: &mut Outcome, day: u64) {
    let males = adults(g, BROKER, Sex::Male);
    if males.is_empty() {
        return;
    }
    if trade
        && g.world.mating_slots_left(BROKER) == 0
        && g.balance(BROKER) > 200
        && data(&g.command(BROKER as u64, BROKER, None, ClientMsg::BuyMatingSlot)).is_some()
    {
        o.slots_bought += 1;
    }
    let mut k = day as usize;
    for host in [STEADY, LATE] {
        let Some(lineage) = g.lineage_of(host).filter(|_| !g.is_extinct(host)) else { continue };
        for _ in 0..2 {
            if g.world.mating_slots_left(BROKER) == 0 || g.balance(BROKER) < PRICE {
                return;
            }
            let m = males[k % males.len()];
            k += 1;
            // An offer to the lineage: the host picks an animal that is free
            // (no guessing which females are pregnant).
            let msg = ClientMsg::OfferToLineage { mine: m, lineage, price: Some(PRICE) };
            let Some(d) = data(&g.command(BROKER as u64, BROKER, None, msg)) else { continue };
            let Some(id) = d["offer"]["id"].as_u64() else { continue };
            // The dam's owner: under the old rules accepting only dilutes
            // their next generation and renown buys nothing.
            let reply = g.command(
                host as u64,
                host,
                None,
                ClientMsg::RespondMating { request: id, accept: trade, partner: None },
            );
            if !trade {
                continue;
            }
            if data(&reply).is_none()
                && !matches!(&reply.first().map(|r| &r.1), Some(ServerMsg::Result { ok: true, .. }))
            {
                // Nobody free: withdraw it.
                g.command(BROKER as u64, BROKER, None, ClientMsg::CancelMating { request: id });
                continue;
            }
            let Some(f) = g.offer_partner(id) else { continue };
            let pos = g.world.organism(m).map(|x| x.pos);
            if let (Some(pos), Some(fo)) = (pos, g.world.organisms.iter_mut().find(|x| x.id == f)) {
                fo.pos = pos;
            }
            let before = g.balance(host);
            let out = g.command(BROKER as u64, BROKER, None, ClientMsg::CompleteMating { request: id });
            if data(&out).is_some() {
                o.trades += 1;
                o.fees_paid += PRICE - (g.balance(host) - before);
            }
        }
    }
}

/// A host's day under the new rules: reinforcements while small.
fn host_day(g: &mut Game, host: PlayerId, o: &mut Outcome) {
    let shop = data(&g.command(host as u64, host, None, ClientMsg::RenownShop)).unwrap_or_default();
    let blocked = shop.get("reinforce_blocked").is_some();
    let price = shop["reinforce_price"].as_u64().unwrap_or(u64::MAX);
    if !blocked && g.balance(host) >= price && data(&g.command(host as u64, host, None, ClientMsg::Reinforce)).is_some()
    {
        o.reinforced += 1;
    }
}

fn titles(records: &[DailyRecord], seasoned: bool) -> BTreeMap<PlayerId, u32> {
    let mut sums: BTreeMap<u64, BTreeMap<PlayerId, f64>> = BTreeMap::new();
    for r in records {
        let season = r.day / SEASON;
        for e in r.legacy.iter().filter(|e| e.player != 0) {
            let v = if seasoned { (e.share + e.weighted_share) / 2.0 } else { e.share };
            *sums.entry(season).or_default().entry(e.player).or_default() += v;
        }
    }
    let mut out = BTreeMap::new();
    for s in sums.values() {
        if let Some((&p, _)) = s.iter().max_by(|a, b| a.1.total_cmp(b.1)) {
            *out.entry(p).or_default() += 1;
        }
    }
    out
}

/// Arms: 0 old rules, 1 new rules without trades, 2 new rules.
const ARMS: [&str; 3] = ["Old rules", "New rules, dam owners still decline", "New rules"];

fn run(seed: u64, arm: usize, a: &Args) -> Outcome {
    let (new_rules, trade) = (arm > 0, arm == 2);
    let world = build_world(&WorldSettings { seed, width: a.size, height: a.size, ..Default::default() });
    let mut g = Game::new(world, GameConfig::default());
    if !new_rules {
        g.world.config.balance.lineage_crowding = 0.0;
    }
    for (k, n) in NAMES.iter().enumerate() {
        g.set_name(k as PlayerId + 1, n);
    }
    let mut o = Outcome::default();
    let found = |g: &mut Game, p: PlayerId, d: FounderDesign, o: &mut Outcome| {
        o.founders.insert(p, d.founders);
        g.found_at_any_price(p, &d).expect("founded");
    };
    found(&mut g, SWARM, swarm("Swarm", !new_rules), &mut o);
    found(&mut g, STEADY, normal("Steady"), &mut o);
    found(&mut g, BROKER, normal("Broker"), &mut o);
    let tpd = g.world.config.ticks_per_day as u64;
    let mut records = Vec::new();
    let mut seen: BTreeMap<PlayerId, u64> = BTreeMap::new();
    for day in 0..a.days {
        if day == a.late_day {
            found(&mut g, LATE, swarm("Late", !new_rules), &mut o);
        }
        if day >= 30 {
            broker_day(&mut g, trade, &mut o, day);
            if trade {
                for host in [STEADY, LATE] {
                    if g.lineage_of(host).is_some() {
                        host_day(&mut g, host, &mut o);
                    }
                }
            }
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
        for p in 1..=4 {
            for e in g.ledger(p, 100) {
                if e.seq <= *seen.get(&p).unwrap_or(&0) {
                    continue;
                }
                match e.kind {
                    LedgerKind::Royalty => *o.royalty.entry(p).or_default() += e.delta as u64,
                    LedgerKind::Income => *o.income.entry(p).or_default() += e.delta as u64,
                    _ => {}
                }
            }
            if let Some(e) = g.ledger(p, 1).first() {
                seen.insert(p, e.seq);
            }
            let alive = g.world.owner_count(p);
            let peak = o.peak_alive.entry(p).or_default();
            *peak = (*peak).max(alive);
        }
    }
    let last = records.last().cloned();
    if let Some(r) = &last {
        for e in r.legacy.iter().filter(|e| e.player != 0) {
            o.final_share.insert(e.player, e.share);
        }
    }
    let n = records.len().max(1) as f64;
    for r in &records {
        for e in r.legacy.iter().filter(|e| e.player != 0) {
            *o.mean_share.entry(e.player).or_default() += e.share / n;
        }
    }
    o.titles_raw = titles(&records, false);
    o.titles_new = titles(&records, true);
    for p in 1..=4 {
        o.renown.insert(p, g.balance(p));
    }
    o.exported = g.lineage_stats(BROKER).score.map_or(0.0, |s| s.exported_share);
    o
}

fn main() {
    let a = args();
    for old in [true, false] {
        let s = swarm("x", old);
        println!(
            "{} rules: swarm founders {} ({:.1} points)",
            if old { "old" } else { "new" },
            s.founders,
            if old {
                s.point_cost() - founder_cost(s.founders) + (s.founders - 20) as f32 * 0.7
            } else {
                s.point_cost()
            }
        );
    }
    let jobs: Vec<(u64, usize)> = (0..a.seeds).flat_map(|k| (0..3).map(move |arm| (a.first_seed + k, arm))).collect();
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).min(jobs.len()).max(1);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&(seed, arm)) = jobs.get(k) else { break };
                let o = run(seed, arm, &a);
                println!(
                    "  seed {seed} {}: final {:?} titles(raw) {:?} titles(new) {:?} trades {} peak {:?}",
                    ["old", "new-no-trade", "new"][arm],
                    o.final_share
                        .iter()
                        .map(|(p, v)| format!("{}:{:.1}%", NAMES[*p as usize - 1], v * 100.0))
                        .collect::<Vec<_>>(),
                    o.titles_raw,
                    o.titles_new,
                    o.trades,
                    o.peak_alive
                );
                results.lock().unwrap().push((seed, arm, o));
            });
        }
    });
    let results = results.into_inner().unwrap();
    for (arm, title) in ARMS.iter().enumerate() {
        let os: Vec<&Outcome> = results.iter().filter(|r| r.1 == arm).map(|r| &r.2).collect();
        let n = os.len().max(1) as f64;
        println!("\n{title}, {} seeds, {} days:", os.len(), a.days);
        println!("  player   founders  final share  mean share  peak alive  renown  income  royalty  legacy titles (raw / new rule)");
        for p in 1..=4u32 {
            let avg = |f: &dyn Fn(&Outcome) -> f64| os.iter().map(|o| f(o)).sum::<f64>() / n;
            println!(
                "  {:<8} {:>8.0}  {:>10.1}%  {:>9.1}%  {:>10.0}  {:>6.0}  {:>6.0}  {:>7.0}  {:.0} / {:.0}",
                NAMES[p as usize - 1],
                avg(&|o| o.founders.get(&p).copied().unwrap_or(0) as f64),
                100.0 * avg(&|o| o.final_share.get(&p).copied().unwrap_or(0.0)),
                100.0 * avg(&|o| o.mean_share.get(&p).copied().unwrap_or(0.0)),
                avg(&|o| o.peak_alive.get(&p).copied().unwrap_or(0) as f64),
                avg(&|o| o.renown.get(&p).copied().unwrap_or(0) as f64),
                avg(&|o| o.income.get(&p).copied().unwrap_or(0) as f64),
                avg(&|o| o.royalty.get(&p).copied().unwrap_or(0) as f64),
                os.iter().map(|o| o.titles_raw.get(&p).copied().unwrap_or(0)).sum::<u32>() as f64,
                os.iter().map(|o| o.titles_new.get(&p).copied().unwrap_or(0)).sum::<u32>() as f64,
            );
        }
        println!(
            "  trades {:.1}/world, fees destroyed {:.0}/world, broker's alleles outside its lineage {:.2}% of all, reinforcements {:.1}/world, slots bought {:.1}/world",
            os.iter().map(|o| o.trades as f64).sum::<f64>() / n,
            os.iter().map(|o| o.fees_paid as f64).sum::<f64>() / n,
            100.0 * os.iter().map(|o| o.exported).sum::<f64>() / n,
            os.iter().map(|o| o.reinforced as f64).sum::<f64>() / n,
            os.iter().map(|o| o.slots_bought as f64).sum::<f64>() / n,
        );
    }
}
