//! Big clutches (playtest 8, `Balance::clutch_split`): a warm incubated-egg
//! grazer founded into grown worlds with a clutch of 4 and of 8, run for
//! 180 days. Reports eggs hatched against chilled, and the lineage at the
//! end, with the rule off (a big clutch cools as a whole) and on (the
//! parent warms 4, the rest chill one by one).
//!
//! `cargo run --release -p evo-sim --example bigclutch [SEEDS]`

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::{presets, Gene, WorldConfig};

fn main() {
    let seeds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let design = |litter: u32| {
        FounderDesign::new("Clutch Grazers", Thermo::Warm, BirthType::IncubatedEggs, litter)
            .with(Gene::BodySize, 0.35)
            .with(Gene::Fur, 0.5)
            .with(Gene::FatStores, 0.5)
            .with(Gene::GroupTolerance, 0.8)
            .founders(30)
    };
    let mut sum = std::collections::BTreeMap::<(u32, u32), [u64; 4]>::new();
    for seed in 1..=seeds {
        let mut w = presets::standard_world(WorldConfig { seed, width: 96, height: 96, ..Default::default() }, 1);
        w.run_days(20);
        for litter in [4u32, 8] {
            for split in [0.0f32, 1.0] {
                let mut c = w.clone();
                c.config.balance.clutch_split = split;
                let ids = c.found_lineage_unchecked(9, &design(litter));
                let l = c.organism(ids[0]).unwrap().lineage;
                c.run_days(180);
                let e = c.counters.eggs.get(&l).copied().unwrap_or_default();
                let alive = c.organisms.iter().filter(|o| o.alive && o.lineage == l).count() as u64;
                println!(
                    "seed {seed} litter {litter} clutch_split {split}: laid {} hatched {} chilled {} · {alive} alive",
                    e.laid, e.hatched, e.chilled
                );
                let s = sum.entry((litter, split as u32)).or_default();
                for (s, v) in s.iter_mut().zip([e.laid, e.hatched, e.chilled, alive]) {
                    *s += v;
                }
            }
        }
    }
    for ((litter, split), s) in &sum {
        println!("litter {litter} clutch_split {split}: laid {} hatched {} chilled {} alive {}", s[0], s[1], s[2], s[3]);
    }
}
