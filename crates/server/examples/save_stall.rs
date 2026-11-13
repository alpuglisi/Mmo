//! How long a daily save stalls the world thread, encoding on it (before)
//! versus capturing a copy and encoding on the background saver (now):
//! `cargo run --release -p evo-server --example save_stall -- [size] [scale] [days] [reps]`.
//!
//! Builds the standard world (founding populations × `scale`), runs it for
//! `days`, then, `reps` times and interleaved so machine load hits all
//! alike, times: a plain tick; a tick plus a save encoded on the world
//! thread; a tick plus a capture handed to the saver; and the ticks that
//! follow while the saver encodes (they share the cores with it). Prints
//! medians.

use std::time::Instant;

use evo_server::game::{Game, GameConfig, PersistEvent};
use evo_server::saver::Saver;
use evo_server::snapshot::{self, Capture};
use evo_sim::{presets, WorldConfig};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let nums: Vec<u64> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let size = nums.first().copied().unwrap_or(512) as usize;
    let scale = nums.get(1).copied().unwrap_or(5) as u32;
    let days = nums.get(2).copied().unwrap_or(2) as u32;
    let reps = nums.get(3).copied().unwrap_or(7) as usize;
    let world = presets::standard_world(WorldConfig { seed: 7, width: size, height: size, ..Default::default() }, scale);
    let mut game = Game::new(world, GameConfig::default());
    game.world.run_days(days);
    println!("{size}x{size}, scale {scale}, {days} days: {} agents", game.world.alive_count());

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut saver = Saver::spawn(tx);
    let (mut plain, mut inline, mut background, mut capture, mut during) = (vec![], vec![], vec![], vec![], vec![]);
    let mut bytes = 0;
    for _ in 0..reps {
        let t = Instant::now();
        game.world.step();
        plain.push(ms(t));

        // Before: encode on the world thread.
        let t = Instant::now();
        game.world.step();
        bytes = snapshot::encode(&game).len();
        inline.push(ms(t));

        // Now: capture, hand to the saver, carry on ticking.
        let t = Instant::now();
        game.world.step();
        let c = Instant::now();
        saver.submit(Capture::new(&game));
        capture.push(ms(c));
        background.push(ms(t));
        for _ in 0..3 {
            let t = Instant::now();
            game.world.step();
            during.push(ms(t));
        }
        // Let the saver finish before the next round.
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
    saver.finish();
    let mut saved = 0;
    while let Ok(ev) = rx.try_recv() {
        saved += matches!(ev, PersistEvent::Snapshot(_)) as usize;
    }
    println!("medians of {reps}: save {:.1} MB, {saved} written by the saver", bytes as f64 / 1e6);
    println!("  tick, no save:                          {:.1} ms", median(plain));
    println!("  tick + save encoded on the world thread: {:.1} ms", median(inline));
    println!("  tick + capture for the background saver: {:.1} ms (capture {:.1} ms)", median(background), median(capture));
    println!("  ticks while the saver encodes:          {:.1} ms", median(during));
}
