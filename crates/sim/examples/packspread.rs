//! Pack cohesion (playtest 9, `Balance::pack_cohesion`): a player's pure
//! carnivore pack (pack instinct 1.0, 36 founders) founded into grown
//! worlds. Reports, every few days, its head count and how far its hunters
//! are from their thickest cluster (median and 90th percentile, tiles),
//! with the rule off and on.
//!
//! `cargo run --release -p evo-sim --example packspread [SEEDS] [DAYS]`

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::math::Vec2;
use evo_sim::{presets, Gene, WorldConfig};

fn spread(w: &evo_sim::World, owner: u32) -> (usize, f32, f32) {
    let pos: Vec<Vec2> = w.organisms.iter().filter(|o| o.alive && o.owner == owner).map(|o| o.pos).collect();
    if pos.is_empty() {
        return (0, 0.0, 0.0);
    }
    // The thickest 12-tile cell and its neighbours.
    let cell = |p: Vec2| ((p.x / 12.0).floor() as i32, (p.y / 12.0).floor() as i32);
    let mut best = (0usize, Vec2::ZERO);
    for &p in &pos {
        let (cx, cy) = cell(p);
        let near: Vec<Vec2> =
            pos.iter().copied().filter(|&q| (cell(q).0 - cx).abs() <= 1 && (cell(q).1 - cy).abs() <= 1).collect();
        if near.len() > best.0 {
            let sum = near.iter().fold(Vec2::ZERO, |s, &q| s + q);
            best = (near.len(), sum * (1.0 / near.len() as f32));
        }
    }
    let mut d: Vec<f32> = pos.iter().map(|p| p.dist(best.1)).collect();
    d.sort_by(f32::total_cmp);
    (pos.len(), d[d.len() / 2], d[d.len() * 9 / 10])
}

fn main() {
    let mut args = std::env::args().skip(1);
    let seeds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let days: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(60);
    let design = FounderDesign::new("Apex Pack", Thermo::Warm, BirthType::Live, 2)
        .with(Gene::Carnivory, 0.95)
        .with(Gene::PackInstinct, 1.0)
        .with(Gene::BodySize, 0.4)
        .with(Gene::GroupTolerance, 0.8)
        .founders(36);
    let mut sum = [[0.0f32; 3]; 2];
    for seed in 1..=seeds {
        let mut w = presets::standard_world(WorldConfig { seed, width: 128, height: 128, ..Default::default() }, 1);
        w.run_days(20);
        for (k, rule) in [0.0f32, 1.0].iter().enumerate() {
            let mut c = w.clone();
            c.config.balance.pack_cohesion = *rule;
            c.found_lineage_unchecked(9, &design);
            for day in 1..=days {
                c.run_days(1);
                if day % 10 == 0 || day == 8 {
                    let (n, med, p90) = spread(&c, 9);
                    println!("seed {seed} pack_cohesion {rule} day {day}: {n} alive, median {med:.1}, p90 {p90:.1}");
                    if day == days {
                        for (s, v) in sum[k].iter_mut().zip([n as f32, med, p90]) {
                            *s += v / seeds as f32;
                        }
                    }
                }
            }
        }
    }
    for (k, s) in sum.iter().enumerate() {
        println!("pack_cohesion {k}: day {days}: {:.0} alive, median {:.1}, p90 {:.1} tiles", s[0], s[1], s[2]);
    }
}
