//! Who dies, how, when and to whom: `cargo run --release --example deaths -- <lineage,..> [days] [seeds..]`.
//!
//! Runs the standard world (96×96) on each seed and tallies every death of
//! the given lineages by cause, life stage (calf: growth < 0.7, sub-adult:
//! not yet breeding, adult), season, whether the mother was alive and near
//! (within 3 tiles), and the killer's lineage. Lineages in the standard
//! world: 0 vole, 1 antelope, 2 newt, 3 fox, 4 pack hunters, 5 herd grazers,
//! 6 toxic frogs, 7 burrowing omnivores. `--set NAME=V` overrides a balance
//! parameter.

use std::collections::BTreeMap;

use evo_sim::group::GroupTrait;
use evo_sim::{presets, Balance, WorldConfig};

fn main() {
    let mut balance = Balance::default();
    let mut pos: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--set" {
            let s = args.next().expect("--set NAME=V");
            let (n, v) = s.split_once('=').expect("--set NAME=V");
            assert!(balance.set(n, v.parse().unwrap()), "unknown parameter {n}");
        } else {
            pos.push(a);
        }
    }
    let lineages: Vec<u32> = pos.first().map_or(vec![1, 5], |s| s.split(',').map(|x| x.parse().unwrap()).collect());
    let days: u32 = pos.get(1).and_then(|s| s.parse().ok()).unwrap_or(120);
    let seeds: Vec<u64> =
        if pos.len() > 2 { pos[2..].iter().map(|s| s.parse().unwrap()).collect() } else { (31..=36).collect() };

    // (lineage, key) -> count
    let mut tally: BTreeMap<(u32, String), u64> = BTreeMap::new();
    fn add(t: &mut BTreeMap<(u32, String), u64>, l: u32, k: String) {
        *t.entry((l, k)).or_default() += 1;
    }
    let mut names = Vec::new();
    for &seed in &seeds {
        let mut w =
            presets::standard_world(WorldConfig { seed, width: 96, height: 96, balance, ..Default::default() }, 1);
        names = w.lineages.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        let tpd = w.config.ticks_per_day;
        for _ in 0..days {
            for _ in 0..tpd {
                w.step();
            }
            let season = format!("{:?}", w.season());
            for o in w.organisms.iter().filter(|o| !o.alive && lineages.contains(&o.lineage)) {
                let Some(cause) = o.death else { continue };
                let stage = if o.matured {
                    "adult"
                } else if o.growth < 0.7 {
                    "calf"
                } else {
                    "subadult"
                };
                let mother = o.mother.and_then(|m| w.organisms.iter().find(|x| x.id == m && x.alive));
                let care = match mother {
                    None => "orphan",
                    Some(m) if m.pos.dist(o.pos) <= 3.0 => "mother-near",
                    Some(_) => "mother-far",
                };
                let killer = o.killer.map_or("-".to_string(), |k| names[k as usize].clone());
                let l = o.lineage;
                add(&mut tally, l, format!("cause {:?}", cause));
                add(&mut tally, l, format!("stage {stage} {:?}", cause));
                add(&mut tally, l, format!("season {season} {:?}", cause));
                if !o.matured {
                    add(&mut tally, l, format!("young {stage} {care} {:?}", cause));
                }
                if o.killer.is_some() {
                    add(&mut tally, l, format!("killer {stage} {killer}"));
                }
                if o.killer.is_some() {
                    let herd = w.group.benefit(GroupTrait::HerdDefense, o.pos);
                    let freq = w.group.frequency(GroupTrait::HerdDefense, o.pos);
                    let r = |x: f32| (x * 5.0).round() / 5.0;
                    add(
                        &mut tally,
                        l,
                        format!(
                            "y-herd {stage} benefit {:.1} freq {:.1} kin>=3 {}",
                            r(herd),
                            r(freq),
                            o.kin_nearby >= 3
                        ),
                    );
                    add(&mut tally, l, format!("y-action {stage} {:?}", o.action));
                    add(&mut tally, l, format!("y-mass {stage} {killer} {:.1}kg", (o.mass() * 2.0).round() / 2.0));
                }
                add(
                    &mut tally,
                    l,
                    format!("z-hunger-at-death {:?} {}", cause, (o.needs.hunger / 20.0).floor() as u32 * 20),
                );
            }
        }
        let counts = w.lineage_counts();
        for &l in &lineages {
            let c = counts[l as usize];
            eprintln!(
                "seed {seed} {}: alive {} adults {} born {}",
                names[l as usize],
                c.alive,
                c.adults,
                w.counters.born.get(&l).copied().unwrap_or(0)
            );
            *tally.entry((l, "zz alive".into())).or_default() += c.alive as u64;
            *tally.entry((l, "zz born".into())).or_default() += w.counters.born.get(&l).copied().unwrap_or(0);
        }
    }
    let mut last = u32::MAX;
    for ((l, k), n) in &tally {
        if *l != last {
            println!("\n== {} ({} seeds)", names[*l as usize], seeds.len());
            last = *l;
        }
        println!("  {k:<48} {n:>6}");
    }
}
