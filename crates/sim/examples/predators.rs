//! Predator boom and bust: `cargo run --release --example predators -- [days] [seeds..] [--set NAME=V] [--migrants]`.
//!
//! Runs the standard world (96×96) on each seed and follows the pack
//! hunters (lineage 4) against their prey: every 10 days the head counts of
//! all lineages, the hunters' mean hunger and the prey biomass (kg, prey
//! they could take) per hunter; then, pooled over the seeds, the hunters'
//! kills per prey lineage by how common that prey was (prey switching:
//! does a rare prey's per-capita risk fall?), hunger while chasing
//! (satiation), births per hunter by prey biomass per hunter (numerical
//! response), pregnant hunters' hunger and hunter deaths by cause. Each
//! seed ends with the hunters' peak and mean head count and the lineages
//! lost; the last line sums them over the seeds. Used to diagnose the
//! pack-hunter booms that wipe out rare prey (README, Balance).

use std::collections::BTreeMap;

use evo_sim::organism::{Action, DeathCause};
use evo_sim::{presets, Balance, WorldConfig};

const HUNTERS: u32 = 4;

fn main() {
    let mut balance = Balance::default();
    let mut pos: Vec<String> = Vec::new();
    let mut migrants = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--migrants" {
            migrants = true;
        } else if a == "--set" {
            let s = args.next().expect("--set NAME=V");
            let (n, v) = s.split_once('=').expect("--set NAME=V");
            assert!(balance.set(n, v.parse().unwrap()), "unknown parameter {n}");
        } else {
            pos.push(a);
        }
    }
    let days: u32 = pos.first().and_then(|s| s.parse().ok()).unwrap_or(120);
    let seeds: Vec<u64> =
        if pos.len() > 1 { pos[1..].iter().map(|s| s.parse().unwrap()).collect() } else { (1..=6).collect() };

    // Pooled tallies. Prey share bins: the prey lineage's share of all
    // other (non-hunter) organisms alive that day (in 5% steps): (lineage, bin) ->
    // (kills, prey-days).
    let mut switching: BTreeMap<(u32, u32), (u64, u64, f64)> = BTreeMap::new();
    // Hunger (10-point bins) of hunters chasing, and of all hunters.
    let mut chase_hunger: BTreeMap<u32, u64> = BTreeMap::new();
    let mut all_hunger: BTreeMap<u32, u64> = BTreeMap::new();
    // Hunger (20-point bins) of pregnant hunters, by prey kg per hunter (log2).
    let mut preg_hunger: BTreeMap<(i32, u32), u64> = BTreeMap::new();
    let mut ratio_bin = 0i32;
    // Prey kg per hunter (log2 bins) -> (hunter-days, births, deaths, starved).
    let mut numerical: BTreeMap<i32, (u64, u64, u64, u64)> = BTreeMap::new();
    let mut causes: BTreeMap<String, u64> = BTreeMap::new();
    let mut names = Vec::new();
    let (mut prey_lost, mut hunters_lost, mut ratio_sum) = (0, 0, 0.0f32);
    for &seed in &seeds {
        let config = WorldConfig { seed, width: 96, height: 96, balance, ..Default::default() };
        let mut w = if migrants { presets::extended_world(config, 1) } else { presets::standard_world(config, 1) };
        names = w.lineages.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        let n = names.len();
        let tpd = w.config.ticks_per_day;
        let mut kills_prev = w.counters.kills_by_lineage.clone();
        let mut born_prev = w.counters.born.get(&HUNTERS).copied().unwrap_or(0);
        let mut peak = 0usize;
        let mut sum = 0usize;
        let mut extinct = Vec::new();
        let mut period = vec![0u64; n];
        print!("seed {seed:>4}:");
        for day in 0..days {
            let mut dead_hunters = 0u64;
            let mut starved = 0u64;
            for _ in 0..tpd {
                w.step();
                for o in w.organisms.iter().filter(|o| o.alive && o.lineage == HUNTERS && o.matured) {
                    let b = (o.needs.hunger / 10.0).floor() as u32 * 10;
                    *all_hunger.entry(b).or_default() += 1;
                    if o.action == Action::Hunt {
                        *chase_hunger.entry(b).or_default() += 1;
                    }
                    if o.pregnancy.is_some() {
                        *preg_hunger.entry((ratio_bin, (o.needs.hunger / 20.0).floor() as u32 * 20)).or_default() += 1;
                    }
                }
            }
            for o in w.organisms.iter().filter(|o| !o.alive && o.lineage == HUNTERS) {
                if let Some(c) = o.death {
                    *causes.entry(format!("{c:?}")).or_default() += 1;
                    dead_hunters += 1;
                    if c == DeathCause::Starvation {
                        starved += 1;
                    }
                }
            }
            // Prey a grown hunter could take (lone hunter's limit times a
            // pack's 4×, roughly): everything lighter than 2.5 hunters, not
            // a hunter itself.
            let hunter_mass = w
                .organisms
                .iter()
                .filter(|o| o.alive && o.lineage == HUNTERS && o.matured)
                .map(|o| o.mass())
                .fold(0.0f32, f32::max)
                .max(1.0);
            let mut alive = vec![0u64; n];
            let mut kg = vec![0f32; n];
            for o in w.organisms.iter().filter(|o| o.alive) {
                alive[o.lineage as usize] += 1;
                if o.lineage != HUNTERS && o.mass() < 2.5 * hunter_mass {
                    kg[o.lineage as usize] += o.mass();
                }
            }
            let hunters = alive[HUNTERS as usize];
            let total_prey: u64 = (0..n).filter(|&l| l != HUNTERS as usize).map(|l| alive[l]).sum();
            let kills = &w.counters.kills_by_lineage;
            for l in 0..n as u32 {
                if l == HUNTERS || alive[l as usize] == 0 {
                    continue;
                }
                let k = kills.get(&(l, HUNTERS)).copied().unwrap_or(0)
                    - kills_prev.get(&(l, HUNTERS)).copied().unwrap_or(0);
                let share = alive[l as usize] as f32 / total_prey.max(1) as f32;
                let bin = (share * 20.0).floor() as u32 * 5;
                let e = switching.entry((l, bin)).or_default();
                e.0 += k;
                e.1 += alive[l as usize];
                e.2 += alive[l as usize] as f64 * hunters as f64;
                period[l as usize] += k;
            }
            kills_prev = kills.clone();
            let born = w.counters.born.get(&HUNTERS).copied().unwrap_or(0);
            if hunters > 0 {
                let per = kg.iter().sum::<f32>() / hunters as f32;
                let bin = per.max(1.0).log2().floor() as i32;
                ratio_bin = bin;
                let e = numerical.entry(bin).or_default();
                e.0 += hunters;
                e.1 += born - born_prev;
                e.2 += dead_hunters;
                e.3 += starved;
            }
            born_prev = born;
            peak = peak.max(hunters as usize);
            sum += hunters as usize;
            for (l, &count) in alive.iter().enumerate() {
                if count == 0 && !extinct.contains(&l) {
                    extinct.push(l);
                }
            }
            if day % 10 == 9 {
                let hunger: f32 =
                    w.organisms.iter().filter(|o| o.alive && o.lineage == HUNTERS).map(|o| o.needs.hunger).sum::<f32>()
                        / hunters.max(1) as f32;
                print!(
                    "\n  d{:<3} hunters {:>3} hunger {:>2.0} alive {:?} kills {:?}",
                    day + 1,
                    hunters,
                    hunger,
                    alive,
                    period
                );
                period = vec![0u64; n];
            }
        }
        let mean = sum as f32 / days as f32;
        ratio_sum += peak as f32 / mean.max(1e-3);
        hunters_lost += extinct.iter().filter(|&&l| l == HUNTERS as usize).count();
        prey_lost += extinct.iter().filter(|&&l| l != HUNTERS as usize).count();
        println!(
            "\n  hunters peak {peak} mean {mean:.1} peak/mean {:.2}; extinct: {:?}",
            peak as f32 / mean.max(1e-3),
            extinct.iter().map(|&l| names[l].as_str()).collect::<Vec<_>>()
        );
    }
    println!("\n== Hunter kills per 10k prey-days per hunter, by the prey's share of all prey alive");
    let mut last = u32::MAX;
    for ((l, bin), (k, d, hd)) in &switching {
        if *k == 0 && *d < 200 {
            continue;
        }
        if *l != last {
            print!("\n  {:<20}", names[*l as usize]);
            last = *l;
        }
        print!(" {bin}%:{:.2}({k}/{d})", 1e4 * *k as f64 / hd.max(1.0));
    }
    println!("\n\n== Hunter hunger (adults): chasing / all");
    for (b, a) in &all_hunger {
        let c = chase_hunger.get(b).copied().unwrap_or(0);
        println!("  {b:>3}: {c:>8} / {a:>8}  ({:.1}%)", 100.0 * c as f64 / *a as f64);
    }
    println!("\n== By prey kg per hunter (2^k): hunter-days, births and deaths per 100 hunter-days");
    for (b, (hd, born, dead, st)) in &numerical {
        let r = |x: u64| 100.0 * x as f64 / (*hd).max(1) as f64;
        println!("  2^{b:<3} days {hd:>7}  born {:>5.2}  dead {:>5.2}  starved {:>5.2}", r(*born), r(*dead), r(*st));
    }
    println!("\n== Pregnant hunters' hunger (ticks) by prey kg per hunter (2^k)");
    let mut last = i32::MIN;
    for ((r, b), c) in &preg_hunger {
        if *r != last {
            print!("\n  2^{r:<3}");
            last = *r;
        }
        print!(" {b}:{c}");
    }
    println!("\n\n== Hunter deaths: {causes:?}");
    println!(
        "\n== {} seeds: other lineages lost {prey_lost}, pack hunters lost {hunters_lost}, mean hunter peak/mean {:.2}",
        seeds.len(),
        ratio_sum / seeds.len().max(1) as f32
    );
}
