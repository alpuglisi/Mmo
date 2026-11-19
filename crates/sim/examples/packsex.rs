//! A player pack's sex ratio (playtest 12: 31 of 89 adults female for
//! hundreds of days, births held to ~2 a day). Founds playtest 4's "Apex
//! Stalkers" into grown worlds and tallies, by sex, adults alive every 30
//! days and every death by cause and age class, and the newborns.
//!
//! `cargo run --release -p evo-sim --example packsex [SEEDS] [DAYS]` (env `NURSING=0`:
//! `Balance::pack_nursing` off)

use std::collections::{BTreeMap, BTreeSet};

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::organism::Sex;
use evo_sim::{presets, Gene, WorldConfig};

fn stalkers() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Apex Stalkers", Thermo::Warm, BirthType::Live, 3)
        .with(BodySize, 0.4)
        .with(Carnivory, 1.0)
        .with(Fangs, 0.7)
        .with(Muscle, 0.5)
        .with(Nose, 0.7)
        .with(Claws, 0.4)
        .with(PackInstinct, 0.85)
        .with(Aggression, 0.8)
        .with(GroupTolerance, 0.7)
        .with(FatStores, 0.7)
        .with(Fur, 0.6)
        .with(Eyes, 0.4)
        .with(Ears, 0.3)
        .with(Camouflage, 0.4)
        .with(LegLength, 0.3)
        .with(Caching, 0.4)
        .with(Swimming, 0.3)
        .founders(45)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let seeds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let days: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(240);
    let mut deaths: BTreeMap<(String, &str, &str), u64> = BTreeMap::new();
    let mut born = [0u64; 2];
    for seed in 1..=seeds {
        let mut w = presets::standard_world(WorldConfig { seed, width: 128, height: 128, ..Default::default() }, 1);
        w.run_days(20);
        // `NURSING=0` turns `Balance::pack_nursing` off.
        if let Some(v) = std::env::var("NURSING").ok().and_then(|v| v.parse().ok()) {
            w.config.balance.pack_nursing = v;
        }
        w.found_lineage_unchecked(9, &stalkers());
        let mut seen: BTreeSet<u64> = w.organisms.iter().filter(|o| o.owner == 9).map(|o| o.id).collect();
        let mut dead: BTreeSet<u64> = BTreeSet::new();
        let tpd = w.config.ticks_per_day as u64;
        for t in 0..days * tpd {
            w.step();
            for o in w.organisms.iter().filter(|o| o.owner == 9) {
                if seen.insert(o.id) {
                    born[usize::from(o.sex == Sex::Female)] += 1;
                }
                if !o.alive && dead.insert(o.id) {
                    let sex = if o.sex == Sex::Female { "F" } else { "M" };
                    let age = match (o.matured, o.pregnancy.is_some(), o.offspring_raised > 0) {
                        (false, _, _) => "young",
                        (true, true, _) => "adult pregnant",
                        (true, false, true) => "adult parent",
                        _ => "adult",
                    };
                    deaths.entry((format!("{:?}", o.death), sex, age)).and_modify(|n| *n += 1).or_insert(1);
                }
            }
            if (t + 1) % (30 * tpd) == 0 {
                let (mut m, mut f, mut preg) = (0, 0, 0);
                for o in w.organisms.iter().filter(|o| o.alive && o.owner == 9 && o.matured) {
                    if o.sex == Sex::Female {
                        f += 1;
                        preg += usize::from(o.pregnancy.is_some());
                    } else {
                        m += 1;
                    }
                }
                println!("seed {seed} day {}: adults {m} M, {f} F ({preg} pregnant)", (t + 1) / tpd);
            }
        }
    }
    println!("born: {} M, {} F", born[0], born[1]);
    for ((cause, sex, age), n) in &deaths {
        println!("died {cause} {sex} {age}: {n}");
    }
}
