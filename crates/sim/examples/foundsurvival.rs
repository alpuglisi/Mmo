//! Founders alive 20 days after founding into a grown world, for three
//! playtest designs, under three home searches: the old one, with
//! `Balance::founding_conditions` (playtest 3), and with
//! `Balance::founding_safety` too (playtest 4). Founding days: spring
//! (45), late autumn into winter (100), summer (160), the next autumn
//! (220), late winter (230), and each world's worst drought between days
//! 120 and 220 (a re-founding in a drought).
//!
//! `cargo run --release -p evo-sim --example foundsurvival [SEED:DAY]`
//! (one case, with the founders' death causes; DAY 0 is the drought).

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::{presets, Gene, World, WorldConfig};

const ARMS: [(&str, f32, f32); 3] = [("old", 0.0, 0.0), ("pt3", 1.0, 0.0), ("pt4", 1.0, 1.0)];

fn main() {
    let only: Option<(u64, u64)> = std::env::args().nth(1).map(|a| {
        let (x, y) = a.split_once(':').unwrap();
        (x.parse().unwrap(), y.parse().unwrap())
    });
    let designs = [
        FounderDesign::new("Brokers", Thermo::Warm, BirthType::IncubatedEggs, 4)
            .with(Gene::BodySize, 0.5)
            .with(Gene::Hibernation, 0.7)
            .with(Gene::FatStores, 0.7)
            .with(Gene::Fur, 0.7)
            .with(Gene::Burrowing, 0.6)
            .with(Gene::Toxins, 0.4)
            .with(Gene::Swimming, 0.3)
            .founders(24),
        FounderDesign::new("Eggrunners", Thermo::Warm, BirthType::IncubatedEggs, 3)
            .with(Gene::BodySize, 0.45)
            .with(Gene::Hibernation, 0.6)
            .with(Gene::FatStores, 0.6)
            .with(Gene::Fur, 0.6)
            .founders(55),
        FounderDesign::new("Grazers", Thermo::Warm, BirthType::Live, 2).with(Gene::BodySize, 0.6).founders(30),
    ];
    // Day 0 stands for the worst drought.
    let days = [45u64, 100, 160, 0, 220, 230];
    let mut tot: Vec<(u64, [usize; 3], usize)> = days.iter().map(|&d| (d, [0; 3], 0)).collect();
    for seed in 1..=8u64 {
        if only.is_some_and(|o| o.0 != seed) {
            continue;
        }
        let size = if seed % 2 == 0 { 96 } else { 128 };
        let mut w = presets::standard_world(WorldConfig { seed, width: size, height: size, ..Default::default() }, 1);
        let tpd = w.config.ticks_per_day;
        let mut drought: Option<(f32, World)> = None;
        for (di, &day) in days.iter().enumerate() {
            let at = if day == 0 {
                while w.day() < 220 {
                    for _ in 0..2 * tpd {
                        w.step();
                    }
                    if w.day() >= 120 && drought.as_ref().is_none_or(|d| w.map.drought > d.0) {
                        drought = Some((w.map.drought, w.clone()));
                    }
                }
                drought.as_ref().unwrap().1.clone()
            } else {
                while w.day() < day {
                    for _ in 0..tpd {
                        w.step();
                    }
                }
                w.clone()
            };
            if only.is_some_and(|o| o.1 != day) {
                continue;
            }
            for (k, d) in designs.iter().enumerate() {
                let mut line =
                    format!("seed {seed} day {:>3}{} {:<10}", at.day(), if day == 0 { "d" } else { " " }, d.name);
                for (a, &(arm, cond, safety)) in ARMS.iter().enumerate() {
                    let mut c = at.clone();
                    c.config.balance.founding_conditions = cond;
                    c.config.balance.founding_safety = safety;
                    let ids = c.found_lineage_unchecked(50 + k as u32, d);
                    if c.days_until_winter() <= 30 {
                        c.provision_for_winter(&ids);
                    }
                    let home = c.organism(ids[0]).unwrap().pos;
                    for _ in 0..20 * tpd {
                        c.step();
                    }
                    let alive = ids.iter().filter(|&&i| c.organism(i).is_some()).count();
                    tot[di].1[a] += alive;
                    if a == 0 {
                        tot[di].2 += ids.len();
                    }
                    line += &format!(" | {arm} {alive:>2}/{:<2} at ({:>3.0},{:>3.0})", ids.len(), home.x, home.y);
                    if only.is_some() {
                        let l = c.lineages.len() as u32 - 1;
                        let deaths: Vec<_> = c
                            .counters
                            .deaths_by_lineage
                            .iter()
                            .filter(|((x, _), _)| *x == l)
                            .map(|((_, cause), n)| format!("{cause:?} {n}"))
                            .collect();
                        println!("   {} {arm}: drought {:.2} deaths {deaths:?}", d.name, c.map.drought);
                    }
                }
                println!("{line}");
            }
        }
    }
    let mut all = ([0usize; 3], 0usize);
    for (day, alive, n) in &tot {
        let name = if *day == 0 { "drought".to_string() } else { format!("day {day}") };
        println!("{name:>8}: old {}/{n} pt3 {}/{n} pt4 {}/{n}", alive[0], alive[1], alive[2]);
        for (sum, n) in all.0.iter_mut().zip(alive) {
            *sum += n;
        }
        all.1 += n;
    }
    println!("TOTAL old {}/{} pt3 {}/{} pt4 {}/{}", all.0[0], all.1, all.0[1], all.1, all.0[2], all.1);
}
