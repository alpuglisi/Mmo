//! Eggs and young under group orders (playtest 5, `Balance::brood_care`): a
//! warm egg-laying hibernator like Herdmaster5's, founded into grown worlds,
//! sent on a migration every 15 days for two years. Reports lifetime eggs
//! hatched against chilled, and the lineage at the end, with the rule off
//! and on.
//!
//! `cargo run --release -p evo-sim --example eggcare [SEEDS]`

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::math::Vec2;
use evo_sim::rng::rng_for;
use evo_sim::{presets, Gene, WorldConfig};
use rand::Rng;

fn main() {
    let seeds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let design = FounderDesign::new("Swarm Grazers", Thermo::Warm, BirthType::IncubatedEggs, 6)
        .with(Gene::BodySize, 0.45)
        .with(Gene::Hibernation, 0.6)
        .with(Gene::Estivation, 0.55)
        .with(Gene::Burrowing, 0.4)
        .with(Gene::Toxins, 0.45)
        .with(Gene::Fur, 0.6)
        .with(Gene::FatStores, 0.6)
        .with(Gene::Obedience, 0.85)
        .with(Gene::GroupTolerance, 0.9)
        .founders(50);
    let mut sum = [[0u64; 4]; 2];
    for seed in 1..=seeds {
        let mut w = presets::standard_world(WorldConfig { seed, width: 96, height: 96, ..Default::default() }, 1);
        w.run_days(30);
        for (k, care) in [0.0f32, 1.0].iter().enumerate() {
            let mut c = w.clone();
            c.config.balance.brood_care = *care;
            let ids = c.found_lineage_unchecked(9, &design);
            let l = c.organism(ids[0]).unwrap().lineage;
            let mut rng = rng_for(seed, 0xe66, 0, 0);
            for day in 0..240u64 {
                if day % 15 == 5 {
                    let to = Vec2::new(rng.gen_range(10.0..86.0), rng.gen_range(10.0..86.0));
                    let _ = c.command_migrate_group(9, 0, to);
                }
                c.run_days(1);
            }
            let e = c.counters.eggs.get(&l).copied().unwrap_or_default();
            let alive = c.organisms.iter().filter(|o| o.alive && o.lineage == l).count() as u64;
            println!(
                "seed {seed} brood_care {care}: laid {} hatched {} chilled {} raided {} · {alive} alive",
                e.laid, e.hatched, e.chilled, e.raided
            );
            for (s, v) in sum[k].iter_mut().zip([e.hatched, e.chilled, e.raided, alive]) {
                *s += v;
            }
        }
    }
    for (k, s) in sum.iter().enumerate() {
        println!("brood_care {k}: hatched {} chilled {} raided {} alive {}", s[0], s[1], s[2], s[3]);
    }
}
