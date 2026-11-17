//! Cost of founding a lineage (the home search, `World::found_lineage`):
//! `cargo run --release -p evo-sim --example founding -- [--size N] [--scale N] [--days N] [--seeds A..B] [--repeat N]`.
//!
//! Builds the server's world (the wild ecosystem scaled to a `--size`
//! square map, 128 by default; `--scale` multiplies the wild founders), runs it `--days` days (10) so the wild
//! population spreads, then founds each sample player design on a copy of
//! the world and reports the time per founding and a hash of the founders'
//! positions (the chosen homes), per seed and in total.

use std::time::Instant;

use evo_sim::{presets, World, WorldConfig};

fn main() {
    let mut size = 128usize;
    let mut days = 10u32;
    let mut scale = 1u32;
    let mut repeat = 1u32;
    let mut seeds = 1u64..6;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--size" => size = args.next().expect("--size N").parse().expect("--size N"),
            "--repeat" => repeat = args.next().expect("--repeat N").parse().expect("--repeat N"),
            "--scale" => scale = args.next().expect("--scale N").parse().expect("--scale N"),
            "--days" => days = args.next().expect("--days N").parse().expect("--days N"),
            "--seeds" => {
                let s = args.next().expect("--seeds A..B");
                let (a, b) = s.split_once("..").expect("--seeds A..B");
                seeds = a.parse().expect("--seeds A..B")..b.parse().expect("--seeds A..B");
            }
            other => panic!("unknown option {other}"),
        }
    }
    let designs = presets::player_designs();
    let (mut total, mut n, mut hash) = (0.0f64, 0u32, 0xcbf2_9ce4_8422_2325u64);
    for seed in seeds {
        let config = WorldConfig { seed, width: size, height: size, ..Default::default() };
        let area = (size * size) as f32 / (96.0 * 96.0);
        let mut world = World::new(config);
        for (owner, design) in presets::wild_designs() {
            let k = ((design.founders as f32 * area).round() as u32).max(4) * scale;
            world.found_lineage_unchecked(owner, &design.founders(k));
        }
        world.run_days(days);
        let (mut secs, mut seed_hash) = (0.0f64, 0xcbf2_9ce4_8422_2325u64);
        for (k, design) in designs.iter().enumerate() {
            let mut w = world.clone();
            let mut ids = Vec::new();
            for _ in 0..repeat {
                w = world.clone();
                // A clone has no spare room: growing the organism list
                // would copy every organism, which a running world rarely
                // does (and which is not the home search).
                w.organisms.reserve(design.founders as usize);
                let start = Instant::now();
                ids = w.found_lineage_unchecked(k as u32 + 1, design);
                secs += start.elapsed().as_secs_f64() / repeat as f64;
            }
            for id in ids {
                let o = w.organism(id).expect("founder alive");
                for v in [o.pos.x.to_bits() as u64, o.pos.y.to_bits() as u64] {
                    seed_hash = (seed_hash ^ v).wrapping_mul(0x100_0000_01b3);
                }
            }
        }
        println!(
            "seed {seed}: {} organisms, {:.2} ms per founding, homes {seed_hash:016x}",
            world.alive_count(),
            secs * 1e3 / designs.len() as f64
        );
        total += secs;
        n += designs.len() as u32;
        hash = (hash ^ seed_hash).wrapping_mul(0x100_0000_01b3);
    }
    println!("{:.2} ms per founding over {n} foundings, homes {hash:016x}", total * 1e3 / n as f64);
}
