//! The background saver: encodes saved games off the world thread.
//!
//! The world thread hands over a [`Capture`] (a clone of the game, cheap)
//! and goes back to ticking; a dedicated `saver` thread encodes it (bincode and
//! LZ4, the expensive part) and sends the bytes to the store task as a
//! [`PersistEvent::Snapshot`].
//!
//! Ordering: one thread encodes one capture at a time and sends them in
//! the order they were taken, and the store task writes in the order it
//! receives, so an older save never lands after a newer one. If a capture
//! arrives while an earlier one is still waiting (the saver is behind),
//! the newer replaces it: the older would be superseded anyway, and this
//! bounds the memory held to one capture in flight plus one waiting.
//! [`Saver::finish`] (also on drop) encodes whatever is still waiting and
//! joins the thread, so shutdown never loses the last save.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::game::{PersistEvent, SnapshotBlob};
use crate::snapshot::Capture;

#[derive(Default)]
struct Slot {
    pending: Option<Capture>,
    closed: bool,
    /// Captures replaced before they were encoded.
    superseded: u64,
}

pub struct Saver {
    shared: Arc<(Mutex<Slot>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Saver {
    /// Start the saver thread; it sends each encoded save on `persist`.
    pub fn spawn(persist: mpsc::UnboundedSender<PersistEvent>) -> Saver {
        let shared = Arc::new((Mutex::new(Slot::default()), Condvar::new()));
        let theirs = shared.clone();
        let thread = std::thread::Builder::new().name("saver".into()).spawn(move || run(&theirs, &persist)).expect("spawn saver thread");
        Saver { shared, thread: Some(thread) }
    }

    /// Queue a capture for encoding, replacing one still waiting. Returns
    /// at once.
    pub fn submit(&self, capture: Capture) {
        let (lock, cv) = &*self.shared;
        let mut slot = lock.lock().unwrap();
        if let Some(old) = slot.pending.replace(capture) {
            slot.superseded += 1;
            tracing::warn!("the saver is behind: the save of tick {} is superseded before it was encoded", old.tick());
        }
        cv.notify_one();
    }

    /// Captures replaced before they were encoded, so far.
    pub fn superseded(&self) -> u64 {
        self.shared.0.lock().unwrap().superseded
    }

    /// Encode whatever is still waiting, send it and stop the thread (later
    /// submits are dropped). Also runs on drop.
    pub fn finish(&mut self) {
        let Some(thread) = self.thread.take() else { return };
        {
            let (lock, cv) = &*self.shared;
            lock.lock().unwrap().closed = true;
            cv.notify_one();
        }
        if thread.join().is_err() {
            tracing::error!("the saver thread panicked");
        }
    }
}

impl Drop for Saver {
    fn drop(&mut self) {
        self.finish();
    }
}

fn run(shared: &(Mutex<Slot>, Condvar), persist: &mpsc::UnboundedSender<PersistEvent>) {
    let (lock, cv) = shared;
    loop {
        let capture = {
            let mut slot = lock.lock().unwrap();
            loop {
                if let Some(c) = slot.pending.take() {
                    break c;
                }
                if slot.closed {
                    return;
                }
                slot = cv.wait(slot).unwrap();
            }
        };
        let started = Instant::now();
        let (tick, day) = (capture.tick(), capture.day());
        let bytes = capture.encode();
        drop(capture);
        tracing::info!(
            "encoded the save of tick {tick} (day {day}): {:.1} MB in {:.0} ms (off the world thread)",
            bytes.len() as f64 / 1e6,
            started.elapsed().as_secs_f64() * 1000.0
        );
        let _ = persist.send(PersistEvent::Snapshot(SnapshotBlob { tick, day, bytes }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{build_world, Game, GameConfig, WorldSettings};

    fn game() -> Game {
        let s = WorldSettings { width: 32, height: 32, ..Default::default() };
        Game::new(build_world(&s), GameConfig::default())
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<PersistEvent>) -> Vec<SnapshotBlob> {
        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            match ev {
                PersistEvent::Snapshot(s) => out.push(s),
                other => panic!("unexpected {other:?}"),
            }
        }
        out
    }

    #[test]
    fn saves_arrive_in_order_and_finish_waits_for_the_last() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut saver = Saver::spawn(tx);
        let mut g = game();
        let mut last = Vec::new();
        for _ in 0..20 {
            g.world.step();
            last = crate::snapshot::encode(&g);
            saver.submit(Capture::new(&g));
        }
        let end = g.world.tick();
        let superseded = saver.superseded();
        saver.finish();
        let saves = drain(&mut rx);
        // Strictly increasing ticks: no older save after a newer one.
        assert!(saves.windows(2).all(|w| w[0].tick < w[1].tick), "{saves:?}");
        assert_eq!(saves.len() as u64 + superseded, 20);
        // The last capture is always written, byte for byte what encoding
        // on the world thread would have given.
        let newest = saves.last().unwrap();
        assert_eq!(newest.tick, end);
        assert!(newest.bytes == last);
        // Channel closed: the saver dropped its sender.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_waiting_capture_is_replaced_by_a_newer_one() {
        // Two captures back to back: either the thread took the first before
        // the second came (two saves, in order) or the second replaced it
        // (one save, counted as superseded). Never the older one last.
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut saver = Saver::spawn(tx);
        let mut g = game();
        {
            // Hold the slot so the first capture is still waiting.
            let mut slot = saver.shared.0.lock().unwrap();
            slot.pending = Some(Capture::new(&g));
        }
        g.world.step();
        saver.submit(Capture::new(&g));
        let superseded = saver.superseded();
        saver.finish();
        let saves = drain(&mut rx);
        assert_eq!(saves.len() as u64 + superseded, 2);
        assert_eq!(saves.last().unwrap().tick, g.world.tick());
        assert!(saves.windows(2).all(|w| w[0].tick < w[1].tick));
    }

    #[test]
    fn dropping_the_saver_still_writes_the_last_save() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let g = game();
        {
            let saver = Saver::spawn(tx);
            saver.submit(Capture::new(&g));
        }
        let saves = drain(&mut rx);
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].tick, g.world.tick());
    }
}
