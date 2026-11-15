//! The process's memory allocator and its heap statistics.
//!
//! With the `jemalloc` feature (default) the whole process, server and
//! anything linking this library, allocates with jemalloc; without it, with
//! the system allocator (glibc malloc on Linux). [`heap_stats`] reports what
//! the allocator has handed out and what it holds besides, for
//! `GET /api/metrics`: resident memory that keeps growing while `allocated`
//! does not is the allocator keeping freed memory, not a leak.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// The allocator in use.
pub const ALLOCATOR: &str = if cfg!(feature = "jemalloc") {
    "jemalloc"
} else if cfg!(all(target_os = "linux", target_env = "gnu")) {
    "glibc"
} else {
    "system"
};

/// Heap statistics of the process's allocator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeapStats {
    /// Bytes in live allocations.
    pub allocated: u64,
    /// Bytes the allocator holds but has not handed out (free lists, dirty
    /// pages not yet returned, metadata): resident memory beyond
    /// `allocated` that is the allocator's, not the program's.
    pub retained: u64,
}

/// The allocator's statistics, where it reports them (jemalloc; glibc's
/// `mallinfo2`), else `None`. Read at most every [`STATS_EVERY`] (glibc
/// walks its free lists with each arena locked; `/api/metrics` is public).
pub fn heap_stats() -> Option<HeapStats> {
    static LAST: Mutex<Option<(Instant, Option<HeapStats>)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    match *last {
        Some((at, stats)) if at.elapsed() < STATS_EVERY => stats,
        _ => {
            let stats = imp::heap_stats();
            *last = Some((Instant::now(), stats));
            stats
        }
    }
}

/// How long [`heap_stats`] reuses a reading.
pub const STATS_EVERY: Duration = Duration::from_secs(2);

#[cfg(feature = "jemalloc")]
mod imp {
    use super::HeapStats;
    use tikv_jemalloc_ctl::{epoch, stats};

    pub fn heap_stats() -> Option<HeapStats> {
        // The statistics are cached; advancing the epoch refreshes them.
        epoch::advance().ok()?;
        let allocated = stats::allocated::read().ok()? as u64;
        let resident = stats::resident::read().ok()? as u64;
        Some(HeapStats { allocated, retained: resident.saturating_sub(allocated) })
    }
}

#[cfg(all(not(feature = "jemalloc"), target_os = "linux", target_env = "gnu"))]
mod imp {
    use super::HeapStats;

    /// glibc's `struct mallinfo2` (glibc 2.33 and later).
    #[repr(C)]
    #[allow(dead_code)] // only some fields are read
    struct MallInfo2 {
        arena: usize,
        ordblks: usize,
        smblks: usize,
        hblks: usize,
        hblkhd: usize,
        usmblks: usize,
        fsmblks: usize,
        uordblks: usize,
        fordblks: usize,
        keepcost: usize,
    }

    extern "C" {
        fn mallinfo2() -> MallInfo2;
    }

    pub fn heap_stats() -> Option<HeapStats> {
        // SAFETY: mallinfo2 takes no arguments and returns a plain struct;
        // it locks each arena while it walks the free lists.
        let m = unsafe { mallinfo2() };
        Some(HeapStats { allocated: (m.uordblks + m.hblkhd) as u64, retained: m.fordblks as u64 })
    }
}

#[cfg(not(any(feature = "jemalloc", all(target_os = "linux", target_env = "gnu"))))]
mod imp {
    pub fn heap_stats() -> Option<super::HeapStats> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_stats_see_a_live_allocation() {
        let Some(before) = imp::heap_stats() else { return };
        let block = std::hint::black_box(vec![1u8; 64 << 20]);
        let during = imp::heap_stats().unwrap();
        // Other tests allocate concurrently; 64 MB dwarfs them.
        assert!(during.allocated >= before.allocated + (32 << 20), "{ALLOCATOR}: {before:?} -> {during:?}");
        drop(std::hint::black_box(block));
    }
}
