//! A player's founding, followed day by day: `cargo run --release --example founding -- DESIGN [--day D] [--days N] [--seeds S..] [--size N] [--every N] [--set NAME=V]`.
//!
//! Runs the standard world (default 128×128, seed 7 as in playtest 1) to day
//! `--day` (default 30), founds DESIGN as a fifth player lineage there, and
//! prints every `--every` days (default 5) its head count, adults by sex,
//! pregnant females, dormant members, mean hunger and thirst, and deaths by
//! cause since the last line, with thirst deaths split by dormancy at death
//! (dormant / awake) and whether there was drinkable water within 6 tiles,
//! and what the very thirsty (thirst above 80, awake) are doing; at the
//! end, deaths by sex, stage (adult, pregnant, young) and cause.
//! DESIGN is one of the playtest-1 designs: `broker` (GeneBroker's
//! hibernating omnivore, 70 founders), `nomad` (PocketNomad's copy of it
//! with hibernation 0.8, 141 founders) or `apex` (ApexHunter's pack, 87
//! founders). Read only (*Playtest 1: ecology fixes* in the README).

use std::collections::BTreeMap;

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::organism::{DeathCause, Dormancy, Sex};
use evo_sim::{presets, Balance, Gene, WorldConfig};
use rayon::prelude::*;

fn design(name: &str) -> FounderDesign {
    use Gene::*;
    let broker = FounderDesign::new("Brokers", Thermo::Warm, BirthType::Live, 3)
        .with(BodySize, 0.49)
        .with(Carnivory, 0.45)
        .with(Fur, 0.6)
        .with(FatStores, 0.7)
        .with(Hibernation, 0.5)
        .with(Nose, 0.5)
        .with(Ears, 0.4)
        .with(Eyes, 0.4)
        .with(Vigilance, 0.5)
        .with(Vocal, 0.5)
        .with(Claws, 0.4)
        .with(Burrowing, 0.5)
        .with(Muscle, 0.3)
        .with(Caching, 0.4)
        .with(LegLength, 0.3)
        .with(Camouflage, 0.4)
        .with(Pickiness, 0.3)
        .with(Boldness, 0.35)
        .with(GroupTolerance, 0.9);
    match name {
        "broker" => broker.founders(70),
        "nomad" => broker.with(Hibernation, 0.8).founders(141),
        "apex" => FounderDesign::new("Apex Pack", Thermo::Warm, BirthType::Live, 4)
            .with(BodySize, 0.30)
            .with(Carnivory, 0.9)
            .with(PackInstinct, 0.95)
            .with(GroupTolerance, 0.85)
            .with(Aggression, 0.6)
            .with(Boldness, 0.65)
            .with(LegLength, 0.7)
            .with(Muscle, 0.6)
            .with(Claws, 0.5)
            .with(Fangs, 0.5)
            .with(Eyes, 0.6)
            .founders(87),
        other => panic!("unknown design {other} (broker, nomad, apex)"),
    }
}

fn main() {
    let mut balance = Balance::default();
    let (mut day0, mut days, mut size, mut every) = (30u32, 120u32, 128usize, 5u32);
    let mut seeds = vec![7u64];
    let mut name = String::from("broker");
    let mut args = std::env::args().skip(1).peekable();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--day" => day0 = args.next().unwrap().parse().unwrap(),
            "--days" => days = args.next().unwrap().parse().unwrap(),
            "--size" => size = args.next().unwrap().parse().unwrap(),
            "--every" => every = args.next().unwrap().parse().unwrap(),
            "--seeds" => {
                seeds.clear();
                while let Some(s) = args.peek().and_then(|s| s.parse().ok()) {
                    seeds.push(s);
                    args.next();
                }
            }
            "--set" => {
                let s = args.next().unwrap();
                let (n, v) = s.split_once('=').unwrap();
                assert!(balance.set(n, v.parse().unwrap()), "unknown parameter {n}");
            }
            other => name = other.to_string(),
        }
    }
    let d = design(&name);
    let logs: Vec<String> = seeds
        .par_iter()
        .map(|&seed| {
            let config = WorldConfig { seed, width: size, height: size, balance, ..Default::default() };
            let mut w = presets::standard_world(config, 1);
            w.run_days(day0);
            w.found_lineage_unchecked(5, &d);
            let l = w.lineages.len() as u32 - 1;
            let mut log = format!("seed {seed}: {} founded on day {day0} ({:?})\n", d.name, w.season());
            let mut deaths: BTreeMap<String, u32> = BTreeMap::new();
            let mut by_sex: BTreeMap<String, u32> = BTreeMap::new();
            for day in 1..=days {
                w.run_days(1);
                for o in w.organisms.iter().filter(|o| !o.alive && o.lineage == l) {
                    let mut k = format!("{:?}", o.death.unwrap_or(DeathCause::Attrition));
                    if o.death == Some(DeathCause::Thirst) {
                        k += if o.dormancy != Dormancy::Awake { "(dormant" } else { "(awake" };
                        let (x, y) = w.map.tile_coords(o.pos);
                        k += if w.map.water_dist[w.map.idx(x, y)] <= 6 { ",water near)" } else { ",no water)" };
                    }
                    *deaths.entry(k).or_default() += 1;
                    let key = format!(
                        "{:?} {}{} {:?}",
                        o.sex,
                        if o.matured { "adult" } else { "young" },
                        if o.pregnancy.is_some() { " pregnant" } else { "" },
                        o.death.unwrap_or(DeathCause::Attrition)
                    );
                    *by_sex.entry(key).or_default() += 1;
                }
                if day % every == 0 || day == days {
                    let m: Vec<_> = w.organisms.iter().filter(|o| o.alive && o.lineage == l).collect();
                    let n = m.len().max(1) as f32;
                    let ad = |s: Sex| m.iter().filter(|o| o.matured && o.sex == s).count();
                    let mut line = format!(
                        "seed {seed} day {:4} {:?}: {} alive (F{} M{}), {} pregnant, {} dormant, hunger {:.0}, thirst {:.0} |",
                        day0 + day,
                        w.season(),
                        m.len(),
                        ad(Sex::Female),
                        ad(Sex::Male),
                        m.iter().filter(|o| o.pregnancy.is_some()).count(),
                        m.iter().filter(|o| o.dormancy != Dormancy::Awake).count(),
                        m.iter().map(|o| o.needs.hunger).sum::<f32>() / n,
                        m.iter().map(|o| o.needs.thirst).sum::<f32>() / n,
                    );
                    let mut acts: BTreeMap<String, u32> = BTreeMap::new();
                    for o in m.iter().filter(|o| o.needs.thirst > 80.0 && o.dormancy == Dormancy::Awake) {
                        *acts.entry(format!("{:?}", o.action)).or_default() += 1;
                    }
                    if !acts.is_empty() {
                        line += &format!(" thirsty acting {acts:?} |");
                    }
                    let resting: Vec<_> = m
                        .iter()
                        .filter(|o| o.needs.thirst > 80.0 && o.dormancy == Dormancy::Awake && format!("{:?}", o.action) == "Rest")
                        .collect();
                    if !resting.is_empty() {
                        let e = resting.iter().map(|o| o.needs.energy).sum::<f32>() / resting.len() as f32;
                        let sh = resting.iter().filter(|o| o.sheltered).count();
                        line += &format!(" thirsty resting: energy {e:.0}, {sh} of {} sheltered |", resting.len());
                    }
                    for (k, v) in &deaths {
                        line += &format!(" {k}={v}");
                    }
                    deaths.clear();
                    log += &line;
                    log.push('\n');
                    if m.is_empty() {
                        break;
                    }
                }
            }
            let mut line = format!("seed {seed} deaths by sex, stage and cause:");
            for (k, v) in &by_sex {
                line += &format!(" [{k}]={v}");
            }
            log += &line;
            log.push('\n');
            log
        })
        .collect();
    for l in logs {
        print!("{l}");
    }
}
