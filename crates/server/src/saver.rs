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
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::game::{PersistEvent, SnapshotBlob};
use crate::metrics::Metrics;
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
        Self::with_metrics(persist, None)
    }

    /// [`Saver::spawn`], recording each save's encoding time and size in
    /// `metrics`.
    pub fn with_metrics(persist: mpsc::UnboundedSender<PersistEvent>, metrics: Option<Arc<Mutex<Metrics>>>) -> Saver {
        let shared = Arc::new((Mutex::new(Slot::default()), Condvar::new()));
        let theirs = shared.clone();
        let thread = std::thread::Builder::new()
            .name("saver".into())
            .spawn(move || run(&theirs, &persist, metrics.as_deref()))
            .expect("spawn saver thread");
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

fn run(
    shared: &(Mutex<Slot>, Condvar),
    persist: &mpsc::UnboundedSender<PersistEvent>,
    metrics: Option<&Mutex<Metrics>>,
) {
    let (lock, cv) = shared;
    loop {
        if let Some(m) = metrics {
            // Nothing left to save once closed and empty: stop waiting.
            wait_for_store(m, || {
                let slot = lock.lock().unwrap();
                slot.closed && slot.pending.is_none()
            });
        }
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
        // A panic while encoding loses this save, not every later one.
        let encoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture.encode()));
        drop(capture);
        let Ok(bytes) = encoded else {
            tracing::error!("encoding the save of tick {tick} (day {day}) panicked; it is skipped");
            continue;
        };
        if let Some(m) = metrics {
            let mut m = m.lock().unwrap();
            m.save_encode_ms = (started.elapsed().as_secs_f64() * 1000.0) as f32;
            m.save_bytes = bytes.len() as u64;
        }
        tracing::info!(
            "encoded the save of tick {tick} (day {day}): {:.1} MB in {:.0} ms (off the world thread)",
            bytes.len() as f64 / 1e6,
            started.elapsed().as_secs_f64() * 1000.0
        );
        if let Some(m) = metrics {
            m.lock().unwrap().snapshots_pending += 1;
        }
        let _ = persist.send(PersistEvent::Snapshot(SnapshotBlob { tick, day, bytes }));
    }
}

/// Longest the saver waits for the store before sending a save anyway.
const STORE_WAIT: Duration = Duration::from_secs(120);

/// Wait until the store task has written the saves already sent (it
/// counts them down in `Metrics::snapshots_pending`), so a slow store holds
/// at most one encoded save in the persist channel: captures that arrive
/// meanwhile replace each other in the slot instead of piling up as
/// encoded blobs in memory. Gives up after [`STORE_WAIT`], or when `done`.
fn wait_for_store(m: &Mutex<Metrics>, done: impl Fn() -> bool) {
    let started = Instant::now();
    let mut warned = false;
    while m.lock().unwrap().snapshots_pending > 0 {
        if done() {
            return;
        }
        if started.elapsed() >= STORE_WAIT {
            tracing::error!("the store has not written the last save in {STORE_WAIT:?}; sending the next one anyway");
            return;
        }
        if !warned && started.elapsed() >= Duration::from_secs(5) {
            tracing::warn!("the store is slow: waiting for it to write the last save before encoding the next");
            warned = true;
        }
        std::thread::sleep(Duration::from_millis(10));
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
    fn a_slow_store_holds_back_saves_and_only_the_newest_is_encoded() {
        // The store task hasn't written the previous save yet.
        let metrics = Arc::new(Mutex::new(Metrics { snapshots_pending: 1, ..Default::default() }));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut saver = Saver::with_metrics(tx, Some(metrics.clone()));
        let mut g = game();
        saver.submit(Capture::new(&g));
        g.world.step();
        saver.submit(Capture::new(&g));
        std::thread::sleep(std::time::Duration::from_millis(200));
        // Nothing encoded or sent while the store is behind; the newer
        // capture replaced the older one in the slot.
        assert!(rx.try_recv().is_err());
        assert_eq!(saver.superseded(), 1);
        // The store catches up: the newest save goes out, and counts as
        // pending until the store task writes it.
        metrics.lock().unwrap().snapshots_pending = 0;
        saver.finish();
        let saves = drain(&mut rx);
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].tick, g.world.tick());
        assert_eq!(metrics.lock().unwrap().snapshots_pending, 1);
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
