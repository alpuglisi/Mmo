//! Would saving the map's tiles as a delta against an earlier save pay?
//! `cargo run --release --features serde --example tile_delta -- [size] [scale] [days] [later days]`.
//!
//! Builds the standard world (founding populations × `scale`), runs it for
//! `days` and takes a snapshot (the keyframe). Then, for each of the next
//! `later days` days (the server saves once a day), it snapshots again and
//! sizes the tiles four ways: as snapshots store them (each tile's bits
//! XORed with its western neighbour's, byte planes, deflate), XORed with the
//! same tile in the keyframe instead, XORed with the keyframe and then with
//! the western neighbour, and the better of the two per field. Times the
//! snapshot and each encoding, and prints the per-field sizes of the last
//! day.

use std::io::Write;
use std::time::Instant;

use evo_sim::terrain::Tile;
use evo_sim::{presets, WorldConfig};

const FIELDS: usize = 12;
const NAMES: [&str; FIELDS] =
    ["temp", "water_depth", "current", "ice", "snow", "mud", "veg", "meat", "wind", "rain", "wetness", "silt"];

fn bits(t: &Tile) -> [u32; FIELDS] {
    let Tile { temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt } = *t;
    [temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt].map(f32::to_bits)
}

fn west(i: usize, width: usize) -> Option<usize> {
    if !i.is_multiple_of(width) {
        Some(i - 1)
    } else {
        i.checked_sub(width)
    }
}

fn spatial(v: &[[u32; FIELDS]], width: usize) -> Vec<[u32; FIELDS]> {
    (0..v.len())
        .map(|i| {
            let p = west(i, width).map_or([0; FIELDS], |p| v[p]);
            std::array::from_fn(|f| v[i][f] ^ p[f])
        })
        .collect()
}

/// Deflated byte planes of one field.
fn field_size(v: &[[u32; FIELDS]], f: usize) -> usize {
    let n = v.len();
    let mut planes = vec![0u8; n * 4];
    for (i, x) in v.iter().enumerate() {
        for (b, byte) in x[f].to_le_bytes().into_iter().enumerate() {
            planes[b * n + i] = byte;
        }
    }
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(&planes).unwrap();
    e.finish().unwrap().len()
}

fn sizes(v: &[[u32; FIELDS]]) -> [usize; FIELDS] {
    std::array::from_fn(|f| field_size(v, f))
}

fn mb(n: usize) -> f64 {
    n as f64 / 1e6
}

fn main() {
    let nums: Vec<u64> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let size = nums.first().copied().unwrap_or(512) as usize;
    let scale = nums.get(1).copied().unwrap_or(5) as u32;
    let days = nums.get(2).copied().unwrap_or(2) as u32;
    let later = nums.get(3).copied().unwrap_or(3) as u32;
    let mut w =
        presets::standard_world(WorldConfig { seed: 7, width: size, height: size, ..Default::default() }, scale);
    w.run_days(days);
    let key: Vec<[u32; FIELDS]> = w.map.tiles.iter().map(bits).collect();
    let snap = w.snapshot();
    println!("{size}x{size}, scale {scale}, day {days}: {} agents, snapshot {:.2} MB", w.alive_count(), mb(snap.len()));
    let mut last = None;
    for d in 1..=later {
        w.run_days(1);
        let t = Instant::now();
        let snap = w.snapshot();
        let snap_ms = t.elapsed().as_secs_f64() * 1e3;
        let now: Vec<[u32; FIELDS]> = w.map.tiles.iter().map(bits).collect();
        let t = Instant::now();
        let full = sizes(&spatial(&now, size));
        let full_ms = t.elapsed().as_secs_f64() * 1e3;
        let temporal: Vec<[u32; FIELDS]> =
            now.iter().zip(&key).map(|(a, b)| std::array::from_fn(|f| a[f] ^ b[f])).collect();
        let t = Instant::now();
        let delta = sizes(&temporal);
        let delta_ms = t.elapsed().as_secs_f64() * 1e3;
        let both = sizes(&spatial(&temporal, size));
        let best: usize = (0..FIELDS).map(|f| full[f].min(delta[f]).min(both[f])).sum();
        let (full_t, delta_t, both_t) = (full.iter().sum::<usize>(), delta.iter().sum(), both.iter().sum::<usize>());
        println!(
            "day {} ({} agents), {d} day(s) after the keyframe: snapshot {:.2} MB in {snap_ms:.0} ms; tiles as stored {:.3} MB ({full_ms:.0} ms), \
             vs keyframe {:.3} MB ({delta_ms:.0} ms), vs keyframe then west {:.3} MB, best per field {:.3} MB: saves {:.3} MB = {:.1}% of the snapshot",
            days + d,
            w.alive_count(),
            mb(snap.len()),
            mb(full_t),
            mb(delta_t),
            mb(both_t),
            mb(best),
            mb(full_t - best),
            100.0 * (full_t - best) as f64 / snap.len() as f64
        );
        last = Some((full, delta, both));
    }
    if let Some((full, delta, both)) = last {
        println!("per field (KB): as stored / vs keyframe / vs keyframe then west");
        for f in 0..FIELDS {
            println!(
                "  {:12} {:7.0} {:7.0} {:7.0}",
                NAMES[f],
                full[f] as f64 / 1e3,
                delta[f] as f64 / 1e3,
                both[f] as f64 / 1e3
            );
        }
    }
}
