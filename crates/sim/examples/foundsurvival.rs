//! Founders alive 20 days after founding into a grown world, for three
//! playtest designs, under five home searches: the old one, with
//! `Balance::founding_conditions` (playtest 3), with
//! `Balance::founding_safety` too (playtest 4), with
//! `Balance::founding_drought` too (playtest 6), and with
//! `Balance::founding_rivals` too (playtest 7). Founding days: spring
//! (45), late autumn into winter (100), summer (160), the next autumn
//! (220), late winter (230), each world's worst drought between days 120
//! and 220 (a re-founding in a drought), and the first day from 120 on
//! with 40–60% of the land in drought (playtest 6's 52%).
//!
//! `cargo run --release -p evo-sim --example foundsurvival [SEED:DAY]`
//! (one case, with the founders' death causes; DAY 0 is the worst drought,
//! DAY 1 the 40–60% one).

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::{presets, Gene, World, WorldConfig};

const ARMS: [(&str, f32, f32, f32, f32); 5] = [
    ("old", 0.0, 0.0, 0.0, 0.0),
    ("pt3", 1.0, 0.0, 0.0, 0.0),
    ("pt4", 1.0, 1.0, 0.0, 0.0),
    ("pt6", 1.0, 1.0, 1.0, 0.0),
    ("pt7", 1.0, 1.0, 1.0, 1.0),
];

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
    // Day 0 stands for the worst drought, day 1 for the first at 40–60%.
    let days = [45u64, 100, 160, 0, 220, 230, 1];
    let mut tot: Vec<(u64, [usize; 5], usize)> = days.iter().map(|&d| (d, [0; 5], 0)).collect();
    for seed in 1..=8u64 {
        if only.is_some_and(|o| o.0 != seed) {
            continue;
        }
        let size = if seed % 2 == 0 { 96 } else { 128 };
        let mut w = presets::standard_world(WorldConfig { seed, width: size, height: size, ..Default::default() }, 1);
        let tpd = w.config.ticks_per_day;
        let mut drought: Option<(f32, World)> = None;
        let mut dry: Option<World> = None;
        for (di, &day) in days.iter().enumerate() {
            let at = if day == 0 {
                while w.day() < 220 {
                    for _ in 0..2 * tpd {
                        w.step();
                    }
                    if w.day() >= 120 && drought.as_ref().is_none_or(|d| w.map.drought > d.0) {
                        drought = Some((w.map.drought, w.clone()));
                    }
                    if w.day() >= 120 && dry.is_none() && (0.4..=0.6).contains(&w.map.drought) {
                        dry = Some(w.clone());
                    }
                }
                drought.as_ref().unwrap().1.clone()
            } else if day == 1 {
                while dry.is_none() && w.day() < 480 {
                    for _ in 0..tpd {
                        w.step();
                    }
                    if (0.4..=0.6).contains(&w.map.drought) {
                        dry = Some(w.clone());
                    }
                }
                match &dry {
                    Some(d) => d.clone(),
                    None => continue,
                }
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
                    format!("seed {seed} day {:>3}{} {:<10}", at.day(), ["d", "40", " "][day.min(2) as usize], d.name);
                for (a, &(arm, cond, safety, dry, rivals)) in ARMS.iter().enumerate() {
                    let mut c = at.clone();
                    c.config.balance.founding_conditions = cond;
                    c.config.balance.founding_safety = safety;
                    c.config.balance.founding_drought = dry;
                    c.config.balance.founding_rivals = rivals;
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
    let mut all = ([0usize; 5], 0usize);
    for (day, alive, n) in &tot {
        let name = match day {
            0 => "drought".to_string(),
            1 => "dry 40-60".to_string(),
            _ => format!("day {day}"),
        };
        println!(
            "{name:>9}: old {}/{n} pt3 {}/{n} pt4 {}/{n} pt6 {}/{n} pt7 {}/{n}",
            alive[0], alive[1], alive[2], alive[3], alive[4]
        );
        for (sum, n) in all.0.iter_mut().zip(alive) {
            *sum += n;
        }
        all.1 += n;
    }
    println!(
        "TOTAL old {}/{} pt3 {}/{} pt4 {}/{} pt6 {}/{} pt7 {}/{}",
        all.0[0], all.1, all.0[1], all.1, all.0[2], all.1, all.0[3], all.1, all.0[4], all.1
    );
}
