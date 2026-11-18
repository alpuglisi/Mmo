//! Foundings in a drought (playtest 6 H4: a small warm egg layer founded at
//! 52% drought lost all 20 founders in 10 days, 16 to thirst, 4 to heat).
//! Each grown world runs to day 450; on up to three days with drought
//! between 45% and 80% (15 days apart at least) PocketNomad6's copy of the
//! leading design is founded with 20 founders on clones of the world, with
//! `Balance::founding_drought` off and on, and run for 10 and 20 days.
//!
//! `cargo run --release -p evo-sim --example droughtfound [SEEDS]`

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::organism::DeathCause;
use evo_sim::{presets, Gene, World, WorldConfig};

fn main() {
    let seeds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(6);
    let design = FounderDesign::new("Pocket Thunder", Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(Gene::BodySize, 0.25)
        .with(Gene::Carnivory, 0.25)
        .with(Gene::GroupTolerance, 0.9)
        .with(Gene::Fur, 0.6)
        .with(Gene::FatStores, 0.6)
        .with(Gene::Hibernation, 0.5)
        .founders(20);
    let mut tot = [[0usize; 2]; 2];
    let mut n = 0;
    for seed in 1..=seeds {
        let size = if seed % 2 == 0 { 96 } else { 128 };
        let mut config = WorldConfig { seed: seed + 40, width: size, height: size, ..Default::default() };
        config.scoring.catastrophes = true;
        let mut w = presets::standard_world(config, 1);
        let tpd = w.config.ticks_per_day;
        // Playtest 6's world was crowded: Herdmaster6's warm egg-laying
        // herd held the lineage cap around the water.
        w.run_days(20);
        let herd = FounderDesign::new("Rolling Thunder", Thermo::Warm, BirthType::IncubatedEggs, 3)
            .with(Gene::BodySize, 0.25)
            .with(Gene::Carnivory, 0.25)
            .with(Gene::GroupTolerance, 0.95)
            .with(Gene::Toxins, 0.6)
            .with(Gene::Fur, 0.6)
            .with(Gene::FatStores, 0.6)
            .with(Gene::Hibernation, 0.5)
            .founders(50);
        w.found_lineage_unchecked(2, &herd);
        let (mut cases, mut last) = (0, 0u64);
        while w.day() < 450 && cases < 3 {
            for _ in 0..tpd {
                w.step();
            }
            let d = w.map.drought;
            if !(0.45..=0.8).contains(&d) || w.day() < last + 15 || w.day() < 120 {
                continue;
            }
            last = w.day();
            cases += 1;
            n += 20;
            let mut line = format!("seed {:>2} day {:>3} drought {:.2}", seed + 40, w.day(), d);
            for (arm, on) in [0.0f32, 1.0].into_iter().enumerate() {
                let (a10, a20, causes, d10) = trial(&w, &design, on);
                tot[arm][0] += a10;
                tot[arm][1] += a20;
                line += &format!(
                    " | {} {a10:>2} {a20:>2} (drought {d10:.2}; {causes})",
                    if on > 0.0 { "on" } else { "off" }
                );
            }
            println!("{line}");
        }
    }
    println!(
        "founders alive after 10 / 20 days of {n}: off {} / {}, on {} / {}",
        tot[0][0], tot[0][1], tot[1][0], tot[1][1]
    );
}

/// Found `design` on a clone of `w` with `founding_drought` at `on`; the
/// founders alive after 10 and 20 days, the death causes, the drought at
/// day 10.
fn trial(w: &World, design: &FounderDesign, on: f32) -> (usize, usize, String, f32) {
    let mut c = w.clone();
    c.config.balance.founding_drought = on;
    let ids = c.found_lineage_unchecked(60, design);
    if c.days_until_winter() <= 30 {
        c.provision_for_winter(&ids);
    }
    let lineage = c.lineages.len() as u32 - 1;
    let alive = |c: &World| ids.iter().filter(|&&i| c.organism(i).is_some()).count();
    c.run_days(10);
    let (a10, d10) = (alive(&c), c.map.drought);
    c.run_days(10);
    let causes = [DeathCause::Thirst, DeathCause::Heat, DeathCause::Starvation, DeathCause::Predation]
        .iter()
        .map(|&k| format!("{:?} {}", k, c.counters.deaths_by_lineage.get(&(lineage, k)).copied().unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(", ");
    (a10, alive(&c), causes, d10)
}
