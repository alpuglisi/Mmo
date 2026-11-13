//! Server health metrics for `GET /api/metrics` (and the load generator,
//! `examples/loadgen.rs`): world-thread timings over a rolling window,
//! connection and queue counters, save timings, outbound traffic and the
//! process's CPU time and resident memory.
//!
//! The world thread records into [`Metrics`] behind a mutex it takes a few
//! times per tick for a few hundred nanoseconds; the WebSocket writers
//! count bytes in [`NetStats`] atomics. Nothing here changes what the game
//! does.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Samples kept per timing window (about four minutes of ticks at 1 s).
pub const WINDOW: usize = 256;

/// A rolling window of durations in milliseconds.
#[derive(Clone, Debug, Default)]
pub struct Window {
    samples: VecDeque<f32>,
}

impl Window {
    pub fn push(&mut self, ms: f64) {
        if self.samples.len() == WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(ms as f32);
    }

    /// p50, p99 and max of the window (zeros when empty).
    pub fn summary(&self) -> Timing {
        if self.samples.is_empty() {
            return Timing::default();
        }
        let mut v: Vec<f32> = self.samples.iter().copied().collect();
        v.sort_by(f32::total_cmp);
        let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        Timing { n: v.len(), p50: at(0.5), p99: at(0.99), max: v[v.len() - 1] }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    /// Samples in the window.
    pub n: usize,
    pub p50: f32,
    pub p99: f32,
    pub max: f32,
}

/// What the world thread, the saver and the store task record.
#[derive(Debug, Default)]
pub struct Metrics {
    /// `World::step` plus the game's bookkeeping, per tick.
    pub tick: Window,
    /// Building and queueing the state messages of one push to every
    /// connection.
    pub states: Window,
    /// One possession sub-step and the states it sends.
    pub substep: Window,
    /// Applying the queued requests at a boundary.
    pub apply: Window,
    pub ticks: u64,
    pub state_pushes: u64,
    /// World-changing requests applied.
    pub applied: u64,
    /// Most requests queued at one boundary, since start.
    pub max_queued: usize,
    /// Connections closed because they couldn't take replies or events.
    pub dropped_conns: u64,
    /// State frames replaced by a newer one before a slow connection took
    /// them (`crate::push`).
    pub skipped_states: u64,
    pub conns: usize,
    pub saves: u64,
    pub save_capture_ms: f32,
    pub save_encode_ms: f32,
    pub save_write_ms: f32,
    pub save_bytes: u64,
    /// Saves sent to the store task and not yet written (the saver waits
    /// for 0 before encoding the next).
    pub snapshots_pending: usize,
}

/// Outbound WebSocket traffic, counted by the connection writers.
#[derive(Debug, Default)]
pub struct NetStats {
    pub bytes_out: AtomicU64,
    pub msgs_out: AtomicU64,
    pub state_bytes: AtomicU64,
    pub states_out: AtomicU64,
    pub ws_opened: AtomicU64,
    pub ws_closed: AtomicU64,
}

impl NetStats {
    pub fn sent(&self, bytes: usize, state: bool) {
        self.bytes_out.fetch_add(bytes as u64, Ordering::Relaxed);
        self.msgs_out.fetch_add(1, Ordering::Relaxed);
        if state {
            self.state_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
            self.states_out.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// `GET /api/metrics`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MetricsView {
    pub tick: u64,
    pub ticks: u64,
    pub conns: usize,
    pub tick_ms: Timing,
    pub states_ms: Timing,
    pub substep_ms: Timing,
    pub apply_ms: Timing,
    pub state_pushes: u64,
    pub applied: u64,
    pub max_queued: usize,
    pub dropped_conns: u64,
    pub skipped_states: u64,
    pub saves: u64,
    pub save_capture_ms: f32,
    pub save_encode_ms: f32,
    pub save_write_ms: f32,
    pub save_bytes: u64,
    pub snapshots_pending: usize,
    pub bytes_out: u64,
    pub msgs_out: u64,
    pub state_bytes: u64,
    pub states_out: u64,
    pub ws_opened: u64,
    pub ws_closed: u64,
    /// User plus system CPU time of the server process (Linux only).
    pub cpu_seconds: Option<f64>,
    /// Resident memory of the server process (Linux only).
    pub rss_bytes: Option<u64>,
}

impl MetricsView {
    pub fn gather(m: &Mutex<Metrics>, net: &NetStats, tick: u64) -> Self {
        let m = m.lock().unwrap();
        let (cpu_seconds, rss_bytes) = process_usage();
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        MetricsView {
            tick,
            ticks: m.ticks,
            conns: m.conns,
            tick_ms: m.tick.summary(),
            states_ms: m.states.summary(),
            substep_ms: m.substep.summary(),
            apply_ms: m.apply.summary(),
            state_pushes: m.state_pushes,
            applied: m.applied,
            max_queued: m.max_queued,
            dropped_conns: m.dropped_conns,
            skipped_states: m.skipped_states,
            saves: m.saves,
            save_capture_ms: m.save_capture_ms,
            save_encode_ms: m.save_encode_ms,
            save_write_ms: m.save_write_ms,
            save_bytes: m.save_bytes,
            snapshots_pending: m.snapshots_pending,
            bytes_out: get(&net.bytes_out),
            msgs_out: get(&net.msgs_out),
            state_bytes: get(&net.state_bytes),
            states_out: get(&net.states_out),
            ws_opened: get(&net.ws_opened),
            ws_closed: get(&net.ws_closed),
            cpu_seconds,
            rss_bytes,
        }
    }
}

/// CPU seconds and resident bytes of this process, from `/proc` (None
/// elsewhere).
pub fn process_usage() -> (Option<f64>, Option<u64>) {
    proc_usage("self")
}

/// CPU seconds and resident bytes of process `pid` (`"self"` for this one).
pub fn proc_usage(pid: &str) -> (Option<f64>, Option<u64>) {
    // Clock ticks per second: 100 on every Linux the server targets.
    const CLK_TCK: f64 = 100.0;
    let cpu = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok().and_then(|s| {
        // Fields after the parenthesized command name; utime and stime are
        // fields 14 and 15 (1-based), so 12 and 13 after the name.
        let rest = &s[s.rfind(')')? + 2..];
        let f: Vec<&str> = rest.split_whitespace().collect();
        let ticks = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
        Some(ticks as f64 / CLK_TCK)
    });
    let rss = std::fs::read_to_string(format!("/proc/{pid}/status")).ok().and_then(|s| {
        let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb * 1024)
    });
    (cpu, rss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_keeps_the_latest_samples_and_summarizes_them() {
        let mut w = Window::default();
        assert_eq!(w.summary(), Timing::default());
        for i in 0..(WINDOW + 100) {
            w.push(i as f64);
        }
        let s = w.summary();
        assert_eq!(s.n, WINDOW);
        assert_eq!(s.max, (WINDOW + 99) as f32);
        // Oldest kept is 100: the median of 100..=355 is about 228.
        assert!((s.p50 - 228.0).abs() <= 1.0, "{s:?}");
        assert!(s.p99 >= 350.0 && s.p99 <= s.max, "{s:?}");
    }

    #[test]
    fn process_usage_reads_proc_on_linux() {
        let (cpu, rss) = process_usage();
        if cfg!(target_os = "linux") {
            assert!(cpu.is_some_and(|c| c >= 0.0));
            assert!(rss.is_some_and(|r| r > 0));
        }
    }
}
