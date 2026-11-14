//! What obedience buys with real orders: `cargo run --release --example obedience -- orders [days] [seeds..]`,
//! and whether it drifts down without them: `... -- drift [days] [seeds..]`.
//!
//! **orders.** The standard world (96×96, wild ecosystem and the four sample
//! player lineages) with the herd grazers (player 2) founded at a low or a
//! high obedience (`LOW`, `HIGH`; every other allele is the same, since the
//! obedience locus draws from a stream of its own) and given scripted orders
//! by a player policy, every `EVERY` days:
//!
//! - `none`: no orders (obedience is then only a cost, in tameness);
//! - `rally`: rally the herd at the best grazing spot near it (the disc of
//!   the rally's radius within `SEARCH` tiles of the herd's centroid with
//!   the most plant food, halved without drinkable water in or next to it);
//! - `territory`: claim that spot as a territory, at the largest radius;
//! - `bad-rally`: rally the herd at the barest spot near it (bad orders);
//! - `pulse`: the `rally` spot every `PULSE_EVERY` days, cancelled a day
//!   later (a short gathering, then free).
//!
//! `POLICIES=none,pulse` runs a subset (`none` first).
//!
//! Each (seed, policy) runs both obedience levels on the same seed, so the
//! difference is paired. Per policy: the herd's living members at the end
//! and averaged over the days, the share of members inside the order's
//! circle (sampled daily), starvation and predation deaths, and mean
//! obedience at the end; then high − low per seed with a 95% t interval,
//! and against `none` (what obedience buys with the orders over what it
//! costs without them).
//!
//! **drift.** The standard world, no orders, with `Balance::tameness` at
//! its default and at 0 (obedience neutral, the control) on the same seeds:
//! each lineage's mean expressed obedience at the end, and the paired
//! difference (tameness − control), with a 95% t interval. Lineages extinct
//! in either arm are left out of that seed.

use rayon::prelude::*;

use evo_sim::genome::{Gene, PlayerId};
use evo_sim::math::Vec2;
use evo_sim::organism::DeathCause;
use evo_sim::{presets, Balance, World, WorldConfig};

const HERD: PlayerId = 2;
const LOW: f32 = 0.2;
const HIGH: f32 = 0.8;
/// Days between orders (a rally stands for `groups::RALLY_DAYS`, 3).
const EVERY: u32 = 2;
/// How far from the herd's centroid the policy looks for a spot.
const SEARCH: i32 = 16;
/// The `pulse` policy rallies every this many days and cancels the rally
/// a day later.
const PULSE_EVERY: u32 = 6;
const SIZE: usize = 96;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Policy {
    None,
    Rally,
    Territory,
    BadRally,
    Pulse,
}

impl Policy {
    const ALL: [Policy; 5] = [Policy::None, Policy::Rally, Policy::Territory, Policy::BadRally, Policy::Pulse];
    fn name(self) -> &'static str {
        match self {
            Policy::None => "none",
            Policy::Rally => "rally",
            Policy::Territory => "territory",
            Policy::BadRally => "bad-rally",
            Policy::Pulse => "pulse",
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
struct Outcome {
    end: f64,
    mean: f64,
    inside: f64,
    starved: f64,
    killed: f64,
    obedience: f64,
}

/// The standard world with the herd grazers founded at `obedience`.
fn world(seed: u64, balance: Balance, obedience: Option<f32>) -> World {
    let config = WorldConfig { seed, width: SIZE, height: SIZE, balance, ..Default::default() };
    let mut w = presets::wild_world(config, 1);
    for (k, design) in presets::player_designs().into_iter().enumerate() {
        let owner = k as PlayerId + 1;
        let design = match obedience {
            Some(v) if owner == HERD => design.with(Gene::Obedience, v),
            _ => design,
        };
        w.found_lineage(owner, &design).expect("preset designs fit the budget");
    }
    w
}

fn herd(w: &World) -> impl Iterator<Item = &evo_sim::organism::Organism> {
    w.organisms.iter().filter(|o| o.alive && o.owner == HERD)
}

/// The disc of `radius` within `SEARCH` tiles of `c` with the most (`best`)
/// or least plant food; a disc without drinkable water within it (or a tile
/// beyond) counts half when looking for the best.
fn spot(w: &World, c: Vec2, radius: f32, best: bool) -> Vec2 {
    let r = radius.ceil() as i32;
    let (mut pick, mut score) = (w.map.clamp(c), if best { f32::MIN } else { f32::MAX });
    for dy in (-SEARCH..=SEARCH).step_by(2) {
        for dx in (-SEARCH..=SEARCH).step_by(2) {
            let p = w.map.clamp(c + Vec2::new(dx as f32, dy as f32));
            let (mut veg, mut water, mut land) = (0.0f32, false, 0);
            for y in -r - 1..=r + 1 {
                for x in -r - 1..=r + 1 {
                    let q = p + Vec2::new(x as f32, y as f32);
                    if q.x < 0.0 || q.y < 0.0 || q.x >= w.map.width as f32 || q.y >= w.map.height as f32 {
                        continue;
                    }
                    let d = ((x * x + y * y) as f32).sqrt();
                    let t = w.map.tile_at(q);
                    water |= d <= radius + 1.0 && t.has_drinkable_water();
                    if d <= radius && t.water_depth == 0.0 {
                        veg += t.veg;
                        land += 1;
                    }
                }
            }
            if land == 0 {
                continue;
            }
            let s = if best && !water { veg * 0.5 } else { veg };
            if (best && s > score) || (!best && s < score) {
                (pick, score) = (p, s);
            }
        }
    }
    pick
}

fn run(seed: u64, days: u32, policy: Policy, obedience: f32) -> Outcome {
    let mut w = world(seed, Balance::default(), Some(obedience));
    let tpd = w.config.ticks_per_day;
    let (mut sum, mut inside, mut samples) = (0usize, 0usize, 0usize);
    for day in 0..days {
        let n = herd(&w).count();
        if policy == Policy::Pulse && day % PULSE_EVERY == 1 {
            let _ = w.cancel_rally(HERD, 0);
        }
        let due = if policy == Policy::Pulse { day % PULSE_EVERY == 0 } else { day % EVERY == 0 };
        if n > 0 && policy != Policy::None && due {
            let c = herd(&w).fold(Vec2::ZERO, |s, o| s + o.pos) * (1.0 / n as f32);
            match policy {
                Policy::Rally | Policy::BadRally | Policy::Pulse => {
                    let p = spot(&w, c, World::rally_radius(n), policy != Policy::BadRally);
                    w.rally_group(HERD, 0, p).unwrap();
                }
                Policy::Territory => {
                    let adults = herd(&w).filter(|o| o.matured).count();
                    let r = World::territory_radius_limit(adults);
                    let p = spot(&w, c, r, true);
                    w.claim_territory(HERD, 0, p, r).unwrap();
                }
                Policy::None => {}
            }
        }
        for _ in 0..tpd {
            w.step();
        }
        let circle = match policy {
            Policy::Territory => w.territory(HERD, 0).map(|t| (t.center, t.radius)),
            _ => w.rally(HERD, 0).map(|r| (r.center, r.radius)),
        };
        for o in herd(&w) {
            sum += 1;
            if let Some((c, r)) = circle {
                samples += 1;
                inside += usize::from(o.pos.dist(c) <= r);
            }
        }
    }
    let lineage = w.organisms.iter().find(|o| o.owner == HERD).map(|o| o.lineage).unwrap_or(1);
    let deaths = |c: DeathCause| w.counters.deaths_by_lineage.get(&(lineage, c)).copied().unwrap_or(0) as f64;
    let n = herd(&w).count();
    Outcome {
        end: n as f64,
        mean: sum as f64 / days as f64,
        inside: if samples > 0 { inside as f64 / samples as f64 } else { f64::NAN },
        starved: deaths(DeathCause::Starvation),
        killed: deaths(DeathCause::Predation),
        obedience: if n > 0 {
            herd(&w).map(|o| o.pheno[Gene::Obedience] as f64).sum::<f64>() / n as f64
        } else {
            f64::NAN
        },
    }
}

/// Two-sided 95% critical value of Student's t.
fn t95(df: usize) -> f64 {
    const T: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160, 2.145, 2.131,
        2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056, 2.052, 2.048, 2.045, 2.042,
    ];
    if df == 0 {
        f64::INFINITY
    } else {
        T.get(df - 1).copied().unwrap_or(1.96)
    }
}

/// Mean and 95% half-width.
fn ci(v: &[f64]) -> (f64, f64) {
    let n = v.len();
    let m = v.iter().sum::<f64>() / n.max(1) as f64;
    if n < 2 {
        return (m, f64::INFINITY);
    }
    let var = v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1) as f64;
    (m, t95(n - 1) * (var / n as f64).sqrt())
}

fn orders(days: u32, seeds: &[u64], policies: &[Policy]) {
    println!("Herd grazers (player 2) at obedience {LOW} vs {HIGH}; {SIZE}x{SIZE}, {days} days, seeds {seeds:?}");
    let jobs: Vec<(u64, Policy, f32)> =
        seeds.iter().flat_map(|&s| policies.iter().flat_map(move |&p| [LOW, HIGH].map(|o| (s, p, o)))).collect();
    let out: Vec<Outcome> = jobs.par_iter().map(|&(s, p, o)| run(s, days, p, o)).collect();
    let get = |s: u64, p: Policy, o: f32| out[jobs.iter().position(|&j| j == (s, p, o)).unwrap()];
    println!(
        "{:<10} {:>13} {:>13} {:>11} {:>11} {:>11} {:>11}",
        "policy", "end low/high", "mean low/high", "inside l/h", "starved l/h", "killed l/h", "obed. l/h"
    );
    let avg = |p: Policy, o: f32, f: fn(&Outcome) -> f64| {
        let v: Vec<f64> = seeds.iter().map(|&s| f(&get(s, p, o))).filter(|x| x.is_finite()).collect();
        v.iter().sum::<f64>() / v.len().max(1) as f64
    };
    for &p in policies {
        let pair =
            |f: fn(&Outcome) -> f64, prec: usize| format!("{:.prec$}/{:.prec$}", avg(p, LOW, f), avg(p, HIGH, f));
        println!(
            "{:<10} {:>13} {:>13} {:>11} {:>11} {:>11} {:>11}",
            p.name(),
            pair(|o| o.end, 1),
            pair(|o| o.mean, 1),
            if p == Policy::None { String::from("-") } else { pair(|o| o.inside, 2) },
            pair(|o| o.starved, 1),
            pair(|o| o.killed, 1),
            pair(|o| o.obedience, 2)
        );
    }
    println!("\nPaired high − low per seed (mean ± 95% CI over {} seeds):", seeds.len());
    for &p in policies {
        let d = |f: fn(&Outcome) -> f64| -> Vec<f64> {
            seeds.iter().map(|&s| f(&get(s, p, HIGH)) - f(&get(s, p, LOW))).collect()
        };
        let (e, eh) = ci(&d(|o| o.end));
        let (m, mh) = ci(&d(|o| o.mean));
        let (st, sth) = ci(&d(|o| o.starved));
        let (k, kh) = ci(&d(|o| o.killed));
        let ins = if p == Policy::None {
            String::from("    -")
        } else {
            let (i, ih) = ci(&d(|o| o.inside));
            format!("{i:+.2} ± {ih:.2}")
        };
        println!(
            "{:<10} end {e:+6.1} ± {eh:4.1}  mean {m:+6.1} ± {mh:4.1}  inside {ins}  starved {st:+5.1} ± {sth:4.1}  killed {k:+5.1} ± {kh:4.1}",
            p.name()
        );
    }
    println!("\nWhat obedience buys with orders over its cost without: (high − low | policy) − (high − low | none):");
    for &p in policies.iter().skip(1) {
        let dd = |f: fn(&Outcome) -> f64| -> Vec<f64> {
            seeds
                .iter()
                .map(|&s| {
                    (f(&get(s, p, HIGH)) - f(&get(s, p, LOW)))
                        - (f(&get(s, Policy::None, HIGH)) - f(&get(s, Policy::None, LOW)))
                })
                .collect()
        };
        let (e, eh) = ci(&dd(|o| o.end));
        let (m, mh) = ci(&dd(|o| o.mean));
        println!("{:<10} end {e:+6.1} ± {eh:4.1}  mean {m:+6.1} ± {mh:4.1}", p.name());
    }
    let orders_vs_none = |p: Policy, o: f32| -> (f64, f64) {
        ci(&seeds.iter().map(|&s| get(s, p, o).end - get(s, Policy::None, o).end).collect::<Vec<_>>())
    };
    println!("\nThe orders themselves: policy − none, end members (mean ± 95% CI):");
    for &p in policies.iter().skip(1) {
        let ((l, lh), (h, hh)) = (orders_vs_none(p, LOW), orders_vs_none(p, HIGH));
        println!("{:<10} at {LOW}: {l:+6.1} ± {lh:4.1}   at {HIGH}: {h:+6.1} ± {hh:4.1}", p.name());
    }
}

fn drift(days: u32, seeds: &[u64]) {
    let tame = Balance::default().tameness;
    println!(
        "Mean expressed obedience without orders; tameness {tame} vs 0; {SIZE}x{SIZE}, {days} days, seeds {seeds:?}"
    );
    let jobs: Vec<(u64, bool)> = seeds.iter().flat_map(|&s| [(s, true), (s, false)]).collect();
    let out: Vec<(Vec<String>, Vec<Option<f64>>)> = jobs
        .par_iter()
        .map(|&(seed, on)| {
            let mut balance = Balance::default();
            if !on {
                balance.tameness = 0.0;
            }
            let mut w = world(seed, balance, None);
            w.run_days(days);
            let names: Vec<String> = w.lineages.iter().map(|l| l.name.clone()).collect();
            let mut sum = vec![(0.0f64, 0usize); names.len()];
            for o in w.organisms.iter().filter(|o| o.alive) {
                if let Some(s) = sum.get_mut(o.lineage as usize) {
                    s.0 += o.pheno[Gene::Obedience] as f64;
                    s.1 += 1;
                }
            }
            let means = sum.iter().map(|&(s, n)| (n > 0).then(|| s / n as f64)).collect();
            (names, means)
        })
        .collect();
    let names = &out[0].0;
    println!("{:<22} {:>6} {:>12} {:>12} {:>18}", "lineage", "seeds", "tameness", "control", "diff (95% CI)");
    let (mut all_on, mut all_off) = (Vec::new(), Vec::new());
    for (l, name) in names.iter().enumerate() {
        let (mut on, mut off) = (Vec::new(), Vec::new());
        for (k, _) in seeds.iter().enumerate() {
            if let (Some(a), Some(b)) = (out[2 * k].1[l], out[2 * k + 1].1[l]) {
                on.push(a);
                off.push(b);
            }
        }
        all_on.extend(&on);
        all_off.extend(&off);
        let diff: Vec<f64> = on.iter().zip(&off).map(|(a, b)| a - b).collect();
        let (d, dh) = ci(&diff);
        println!(
            "{name:<22} {:>6} {:>12.3} {:>12.3} {:>+10.3} ± {dh:.3}",
            on.len(),
            on.iter().sum::<f64>() / on.len().max(1) as f64,
            off.iter().sum::<f64>() / off.len().max(1) as f64,
            d
        );
    }
    let diff: Vec<f64> = all_on.iter().zip(&all_off).map(|(a, b)| a - b).collect();
    let (d, dh) = ci(&diff);
    let (a, ah) = ci(&all_on.iter().map(|x| x - 0.5).collect::<Vec<_>>());
    println!(
        "all lineage-seeds ({}): tameness − control {d:+.3} ± {dh:.3}; tameness − 0.5 {a:+.3} ± {ah:.3}",
        diff.len()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("orders");
    let days: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(if mode == "drift" { 360 } else { 120 });
    let seeds: Vec<u64> =
        if args.len() > 2 { args[2..].iter().map(|s| s.parse().unwrap()).collect() } else { (101..=112).collect() };
    match mode {
        "orders" => {
            // `POLICIES=none,pulse` runs a subset (`none` first: the others
            // are compared with it).
            let policies: Vec<Policy> = match std::env::var("POLICIES") {
                Ok(v) => v
                    .split(',')
                    .map(|n| {
                        *Policy::ALL.iter().find(|p| p.name() == n).unwrap_or_else(|| panic!("unknown policy {n}"))
                    })
                    .collect(),
                Err(_) => Policy::ALL.to_vec(),
            };
            assert_eq!(policies.first(), Some(&Policy::None), "list none first");
            orders(days, &seeds, &policies)
        }
        "drift" => drift(days, &seeds),
        m => panic!("unknown mode {m}: orders or drift"),
    }
}
