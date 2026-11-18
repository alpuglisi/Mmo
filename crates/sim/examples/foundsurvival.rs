//! Founders alive 20 days after founding into a grown world, with the old
//! home search and with `Balance::founding_conditions` (playtest 3's fatal
//! founding spots), for three playtest designs at four times of the year.
//!
//! `cargo run --release -p evo-sim --example foundsurvival [SEED:DAY]`
//! (one case, with the founders' death causes).
use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::{presets, Gene, WorldConfig};

fn main() {
    let designs = [
        FounderDesign::new("Brokers", Thermo::Warm, BirthType::IncubatedEggs, 4)
            .with(Gene::BodySize, 0.5)
            .with(Gene::Hibernation, 0.7)
            .with(Gene::FatStores, 0.7)
            .with(Gene::Fur, 0.7)
            .with(Gene::Burrowing, 0.6)
            .with(Gene::Toxins, 0.4)
            .with(Gene::Climbing, 0.0)
            .founders(24),
        FounderDesign::new("Eggrunners", Thermo::Warm, BirthType::IncubatedEggs, 3)
            .with(Gene::BodySize, 0.45)
            .with(Gene::Hibernation, 0.6)
            .with(Gene::FatStores, 0.6)
            .with(Gene::Fur, 0.6)
            .founders(55),
        FounderDesign::new("Grazers", Thermo::Warm, BirthType::Live, 2).with(Gene::BodySize, 0.6).founders(30),
    ];
    let only: Option<(u64, u64)> = std::env::args().nth(1).map(|a| {
        let (x, y) = a.split_once(':').unwrap();
        (x.parse().unwrap(), y.parse().unwrap())
    });
    let days_to_found = [45u64, 100, 160, 220];
    let mut tot = [[0usize; 2]; 2];
    for seed in 1..=8u64 {
        if only.is_some_and(|o| o.0 != seed) {
            continue;
        }
        let size = if seed % 2 == 0 { 96 } else { 128 };
        let mut w = presets::standard_world(WorldConfig { seed, width: size, height: size, ..Default::default() }, 1);
        let tpd = w.config.ticks_per_day;
        for &day in &days_to_found {
            while w.day() < day {
                for _ in 0..tpd {
                    w.step();
                }
            }
            if only.is_some_and(|o| o.1 != day) {
                continue;
            }
            for (di, d) in designs.iter().enumerate() {
                let mut line = format!("seed {seed} day {day} {:<10}", d.name);
                for (k, on) in [0.0f32, 1.0].iter().enumerate() {
                    let mut c = w.clone();
                    c.config.balance.founding_conditions = *on;
                    let ids = c.found_lineage_unchecked(50 + di as u32, d);
                    if c.days_until_winter() <= 30 {
                        c.provision_for_winter(&ids);
                    }
                    let home = c.organism(ids[0]).unwrap().pos;
                    for _ in 0..20 * tpd {
                        c.step();
                    }
                    let alive = ids.iter().filter(|&&i| c.organism(i).is_some()).count();
                    if only.is_some() {
                        let l = c.lineages.len() as u32 - 1;
                        let deaths: Vec<_> = c
                            .counters
                            .deaths_by_lineage
                            .iter()
                            .filter(|((k, _), _)| *k == l)
                            .map(|((_, cause), n)| format!("{cause:?} {n}"))
                            .collect();
                        let t = c.map.ground_at(home);
                        println!(
                            "   {} {}: home temp(base) {:.1} drought {:.2} deaths {:?}",
                            d.name,
                            if k == 0 { "old" } else { "new" },
                            t.base_temp,
                            c.map.drought,
                            deaths
                        );
                    }
                    tot[k][0] += alive;
                    tot[k][1] += ids.len();
                    line += &format!(
                        " | {} {:>2}/{:<2} at ({:>3.0},{:>3.0})",
                        if k == 0 { "old" } else { "new" },
                        alive,
                        ids.len(),
                        home.x,
                        home.y
                    );
                }
                println!("{line}");
            }
        }
    }
    println!("TOTAL old {}/{} new {}/{}", tot[0][0], tot[0][1], tot[1][0], tot[1][1]);
}
