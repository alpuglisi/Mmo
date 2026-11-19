//! Late joiners (playtest 7 M10: a late founding landed inside a rival's
//! territory with four rivals within 35 tiles). Each grown world's sample
//! players claim a territory around their main group; then LateComer7's
//! cold flier (62 founders) is founded on days 200, 260 and 320 on clones
//! of the world, with `Balance::founding_rivals` off and on. Reports the
//! other players' animals within 35 tiles of the home, founders inside a
//! rival territory, and founders alive after 30 days.
//!
//! `cargo run --release -p evo-sim --example latejoin [SEEDS]`

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::math::Vec2;
use evo_sim::organism::DeathCause;
use evo_sim::{presets, Gene, World, WorldConfig};

const LATE: u32 = 50;

fn main() {
    let seeds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(6);
    let design = FounderDesign::new("Late Skylarks", Thermo::Cold, BirthType::Live, 3)
        .with(Gene::BodySize, 0.5)
        .with(Gene::Wings, 0.8)
        .with(Gene::Toxins, 0.6)
        .with(Gene::Coloration, 0.6)
        .with(Gene::Brumation, 0.7)
        .founders(62);
    // (rivals near, founders inside, alive after 30 days, eaten) per arm.
    let mut tot = [[0usize; 4]; 2];
    let mut n = 0;
    for seed in 1..=seeds {
        let size = if seed % 2 == 0 { 96 } else { 128 };
        let mut w = presets::standard_world(WorldConfig { seed, width: size, height: size, ..Default::default() }, 1);
        for day in [200u64, 260, 320] {
            while w.day() < day {
                w.run_days(1);
            }
            claim_all(&mut w);
            let mut line = format!("seed {seed} day {day}");
            n += design.founders as usize;
            for (arm, on) in [0.0f32, 1.0].into_iter().enumerate() {
                let r = trial(&w, &design, on);
                for (t, v) in tot[arm].iter_mut().zip(r) {
                    *t += v;
                }
                line += &format!(
                    " | {} rivals near {:>4}, inside {:>2}, alive {:>2}, eaten {:>2}",
                    if on > 0.0 { "on " } else { "off" },
                    r[0],
                    r[1],
                    r[2],
                    r[3]
                );
            }
            println!("{line}");
        }
    }
    for (arm, t) in tot.iter().enumerate() {
        println!(
            "{}: rival animals within 35 tiles of the homes {}, founders inside a rival territory {}, alive after 30 days {} of {n}, eaten {}",
            ["off", "on"][arm],
            t[0],
            t[1],
            t[2],
            t[3]
        );
    }
}

/// Every sample player claims what its main group can hold around its
/// middle (if it may claim now).
fn claim_all(w: &mut World) {
    let players: Vec<u32> = w.lineages.iter().map(|l| l.owner).filter(|&o| o != evo_sim::WILD).collect();
    for p in players {
        if w.territory(p, 0).is_some() {
            continue;
        }
        if let Some(c) = w.group_middle(p, 0) {
            let _ = w.claim_territory(p, 0, c, 30.0);
        }
    }
}

fn trial(w: &World, design: &FounderDesign, on: f32) -> [usize; 4] {
    let mut c = w.clone();
    c.config.balance.founding_rivals = on;
    let ids = c.found_lineage_unchecked(LATE, design);
    let lineage = c.lineages.len() as u32 - 1;
    let home = ids.iter().filter_map(|&i| c.organism(i)).fold(Vec2::ZERO, |a, o| a + o.pos) * (1.0 / ids.len() as f32);
    let rivals = c
        .organisms
        .iter()
        .filter(|o| o.alive && o.owner != evo_sim::WILD && o.owner != LATE && o.pos.dist(home) < 35.0)
        .count();
    let players: Vec<u32> = c.lineages.iter().map(|l| l.owner).filter(|&o| o != evo_sim::WILD && o != LATE).collect();
    let inside = ids
        .iter()
        .filter_map(|&i| c.organism(i))
        .filter(|o| players.iter().any(|&p| c.territory(p, 0).is_some_and(|t| t.contains(o.pos))))
        .count();
    c.run_days(30);
    let alive = ids.iter().filter(|&&i| c.organism(i).is_some()).count();
    let eaten = c.counters.deaths_by_lineage.get(&(lineage, DeathCause::Predation)).copied().unwrap_or(0) as usize;
    [rivals, inside, alive, eaten]
}
