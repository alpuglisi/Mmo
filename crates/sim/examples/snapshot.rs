//! Snapshot size and speed:
//! `cargo run --release --features serde --example snapshot -- [size] [scale] [days] [--lod]`.
//!
//! Builds the standard world (founding populations × `scale`), runs it for
//! `days`, then times `World::snapshot`, `World::restore` and `World::clone`
//! (all a server's world thread pays to save in the background; it shares
//! the static terrain) and prints the sizes (bincode before LZ4, and the
//! stored bytes), and those of the terrain, which is stored once per world
//! rather than in every snapshot (`Terrain::encode`).

use std::time::Instant;

use evo_sim::world::SnapshotHeader;
use evo_sim::{presets, World, WorldConfig};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lod = args.iter().any(|a| a == "--lod");
    let nums: Vec<u64> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let size = nums.first().copied().unwrap_or(256) as usize;
    let scale = nums.get(1).copied().unwrap_or(1) as u32;
    let days = nums.get(2).copied().unwrap_or(3) as u32;
    let mut config = WorldConfig { seed: 7, width: size, height: size, ..Default::default() };
    config.lod.enabled = lod;
    let mut w = presets::standard_world(config, scale);
    let t = Instant::now();
    w.run_days(days);
    println!(
        "{size}x{size}, scale {scale}, {days} days ({:.1} s): {} agents ({} in the vector), {} cohort members",
        t.elapsed().as_secs_f64(),
        w.alive_count(),
        w.organisms.len(),
        w.abstract_count()
    );
    let reps = 5;
    let t = Instant::now();
    let mut bytes = Vec::new();
    for _ in 0..reps {
        bytes = w.snapshot();
    }
    let snap_ms = t.elapsed().as_secs_f64() * 1000.0 / reps as f64;
    let h = SnapshotHeader::read(&bytes).unwrap();
    let t = Instant::now();
    for _ in 0..reps {
        let r = World::restore(&bytes, Some(w.map.terrain().clone())).unwrap();
        assert_eq!(r.tick(), w.tick());
    }
    let restore_ms = t.elapsed().as_secs_f64() * 1000.0 / reps as f64;
    let t = Instant::now();
    let terrain = w.map.terrain().encode();
    let terrain_ms = t.elapsed().as_secs_f64() * 1000.0;
    // What a server's tick loop pays to save off-thread: a clone of the
    // world (encoding the clone happens on another thread).
    let t = Instant::now();
    for _ in 0..reps {
        let c = w.clone();
        assert_eq!(c.tick(), w.tick());
    }
    let clone_ms = t.elapsed().as_secs_f64() * 1000.0 / reps as f64;
    println!(
        "raw {:.2} MB, stored {:.2} MB (LZ4 {:.0}%), {:.0} B/agent stored; snapshot {snap_ms:.0} ms, restore {restore_ms:.0} ms, clone {clone_ms:.1} ms",
        h.raw_len as f64 / 1e6,
        bytes.len() as f64 / 1e6,
        100.0 * bytes.len() as f64 / h.raw_len as f64,
        bytes.len() as f64 / w.organisms.len().max(1) as f64
    );
    println!("terrain (stored once per world): {:.2} MB, encoded in {terrain_ms:.0} ms", terrain.len() as f64 / 1e6);
}
