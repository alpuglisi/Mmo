//! Retention of the daily history: how long each world's daily records keep
//! full resolution, and which ranges a history read returns.
//!
//! A daily record holds a row per lineage, with the mean of every gene for
//! player lineages: ~700 KB a day at 500 players, ~1 GB a real day at the
//! 1 s clock if every record were kept whole. Every store applies the same
//! policy, by a record's age (days before the newest record):
//!
//! - younger than [`FULL_DAYS`]: the whole record;
//! - younger than [`WINDOW_DAYS`] (the dashboard's window): every day, but
//!   gene means only on days divisible by [`GENE_STRIDE`] and only lineages
//!   with something to report (members, births or deaths that day);
//! - older: one day in `ARCHIVE_STRIDE × 2^k` (day divisible by it, `k` the
//!   number of times the age doubled past the window), without gene means.
//!
//! So a world keeps 30 whole records, ~42 with gene means and ~290 lean
//! ones in its window, and ~46 lean records per doubling of its age past a
//! year: bounded whatever its age (a lean record is a seventh to a tenth of
//! a whole one). The policy is monotone (a record only loses detail as it
//! ages, and a day an archive stride keeps is kept by every smaller stride),
//! so thinning again is a no-op and stores can compact at any cadence.
//!
//! Ages walk back from the newest record: a day number going up while
//! walking back (an older world, before a `--fresh` start) counts as one
//! day, so an older world's records age with the newest world's.

use std::collections::{HashSet, VecDeque};

use crate::store::DailyRecord;

/// Days kept whole.
pub const FULL_DAYS: u64 = 30;
/// Days kept every day (the game's dashboard window, `game::HISTORY_DAYS`).
pub const WINDOW_DAYS: u64 = 365;
/// In the window, gene means are kept on days divisible by this.
pub const GENE_STRIDE: u64 = 8;
/// Past the window, days divisible by this (doubled for each doubling of
/// the age past the window) are kept.
pub const ARCHIVE_STRIDE: u64 = 8;
/// The most records one history read returns.
pub const MAX_LIMIT: usize = WINDOW_DAYS as usize;

/// What of a record its age keeps, from most to least.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Keep {
    /// The whole record.
    Full = 0,
    /// Lineages with something to report, with their gene means.
    Genes = 1,
    /// Lineages with something to report, without gene means.
    Lean = 2,
    /// Nothing.
    Drop = 3,
}

impl Keep {
    /// The level stored with a record (PostgreSQL's `thinned`).
    pub fn level(self) -> i16 {
        self as i16
    }
}

/// The archive stride at `age` (at least [`WINDOW_DAYS`]).
fn archive_stride(age: u64) -> u64 {
    let doublings = (age / WINDOW_DAYS).max(1).ilog2();
    ARCHIVE_STRIDE.saturating_mul(1u64 << doublings.min(48))
}

/// What a record of `day`, `age` days before the newest, keeps.
pub fn keep(day: u64, age: u64) -> Keep {
    if age < FULL_DAYS {
        Keep::Full
    } else if age < WINDOW_DAYS {
        if day.is_multiple_of(GENE_STRIDE) {
            Keep::Genes
        } else {
            Keep::Lean
        }
    } else if day.is_multiple_of(archive_stride(age)) {
        Keep::Lean
    } else {
        Keep::Drop
    }
}

/// Thin `r` to `keep` (`Drop` thins as `Lean`; dropping is the caller's).
/// Returns whether anything was removed.
pub fn thin(r: &mut DailyRecord, keep: Keep) -> bool {
    if keep == Keep::Full {
        return false;
    }
    let before = r.lineages.len();
    r.lineages.retain(|l| l.alive > 0 || l.adults > 0 || l.born > 0 || !l.deaths.is_empty());
    let mut changed = r.lineages.len() != before;
    if keep >= Keep::Lean {
        for l in r.lineages.iter_mut().filter(|l| !l.genes.is_empty() || l.morph.is_some()) {
            l.genes.clear();
            l.morph = None;
            changed = true;
        }
    }
    changed
}

/// Ages of records met newest first: [`AgeWalk::next`] gives each record's
/// age in days before the newest.
#[derive(Clone, Debug, Default)]
pub struct AgeWalk {
    prev: Option<u64>,
    age: u64,
}

impl AgeWalk {
    /// The age of the next older record, of `day`.
    pub fn next(&mut self, day: u64) -> u64 {
        if let Some(p) = self.prev {
            self.age += if day < p { p - day } else { 1 };
        }
        self.prev = Some(day);
        self.age
    }
}

/// Records met newest first that a later (newer) record of the same world
/// and day replaced. Records without a world id are never duplicates.
#[derive(Default)]
pub struct Dedup(HashSet<(String, u64)>);

impl Dedup {
    /// Whether the record is the newest of its world and day (and remember
    /// it).
    pub fn first(&mut self, world_id: &str, day: u64) -> bool {
        world_id.is_empty() || self.0.insert((world_id.to_string(), day))
    }
}

/// A history read: the newest `limit` records with a day in
/// `from_day..=to_day`. Records are walked back from the newest; one above
/// `to_day` is skipped and the first below `from_day` ends the walk, so a
/// day range reads the newest world's days (an older world's are reached
/// only past it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DailyQuery {
    pub from_day: Option<u64>,
    pub to_day: Option<u64>,
    pub limit: usize,
    /// Whether gene means are returned (reading without them is cheaper).
    pub genes: bool,
}

impl DailyQuery {
    /// The newest `limit` records, whole.
    pub fn recent(limit: usize) -> Self {
        Self { from_day: None, to_day: None, limit, genes: true }
    }
}

/// What a read does with the next older record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Take,
    Skip,
    Stop,
}

/// A [`DailyQuery`] applied to records met newest first (after [`Dedup`]).
pub struct QueryWalk {
    q: DailyQuery,
    taken: usize,
}

impl QueryWalk {
    pub fn new(q: DailyQuery) -> Self {
        Self { q, taken: 0 }
    }

    pub fn step(&mut self, day: u64) -> Step {
        if self.taken >= self.q.limit || self.q.from_day.is_some_and(|f| day < f) {
            return Step::Stop;
        }
        if self.q.to_day.is_some_and(|t| day > t) {
            return Step::Skip;
        }
        self.taken += 1;
        Step::Take
    }

    pub fn done(&self) -> bool {
        self.taken >= self.q.limit
    }
}

/// The records `q` reads from `records` (oldest first, no duplicates),
/// oldest first.
pub fn select(records: &[DailyRecord], q: &DailyQuery) -> Vec<DailyRecord> {
    let mut walk = QueryWalk::new(*q);
    let mut out = Vec::new();
    for r in records.iter().rev() {
        match walk.step(r.day) {
            Step::Take => out.push(strip_genes(r.clone(), q.genes)),
            Step::Skip => {}
            Step::Stop => break,
        }
    }
    out.reverse();
    out
}

/// `r` without gene means (and body plans) unless `genes`.
pub fn strip_genes(mut r: DailyRecord, genes: bool) -> DailyRecord {
    if !genes {
        for l in &mut r.lineages {
            l.genes.clear();
            l.morph = None;
        }
    }
    r
}

/// Apply the policy to `records` (oldest first): drop replaced records
/// (the newest of a world and day stays, in its place) and what the
/// records' ages drop, and thin the rest. Returns whether anything changed.
pub fn compact(records: &mut Vec<DailyRecord>) -> bool {
    let mut ages = AgeWalk::default();
    let mut dedup = Dedup::default();
    let mut keep_flags = vec![false; records.len()];
    let mut changed = false;
    for (k, r) in records.iter_mut().enumerate().rev() {
        if !dedup.first(&r.world_id, r.day) {
            changed = true;
            continue;
        }
        let keep = keep(r.day, ages.next(r.day));
        if keep == Keep::Drop {
            changed = true;
            continue;
        }
        changed |= thin(r, keep);
        keep_flags[k] = true;
    }
    if changed {
        let mut flags = keep_flags.into_iter();
        records.retain(|_| flags.next().unwrap_or(false));
    }
    changed
}

/// [`compact`] for the game's window of records (one world, oldest first).
pub fn compact_window(records: &mut VecDeque<DailyRecord>) {
    let mut ages = AgeWalk::default();
    let mut keep_flags = vec![true; records.len()];
    let mut any_dropped = false;
    for (k, r) in records.iter_mut().enumerate().rev() {
        let keep = keep(r.day, ages.next(r.day));
        if keep == Keep::Full {
            continue;
        }
        if keep == Keep::Drop {
            keep_flags[k] = false;
            any_dropped = true;
        } else {
            thin(r, keep);
        }
    }
    if any_dropped {
        let mut flags = keep_flags.into_iter();
        records.retain(|_| flags.next().unwrap_or(true));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::store::{LegacyEntry, LineageEntry};

    fn rec(world: &str, day: u64) -> DailyRecord {
        let lineage = |k: u32, alive: usize| LineageEntry {
            lineage: k,
            name: format!("L{k}"),
            owner: k + 1,
            alive,
            adults: alive / 2,
            born: 0,
            deaths: BTreeMap::new(),
            genes: (0..3).map(|g| (format!("g{g}"), 0.5)).collect(),
            morph: None,
        };
        DailyRecord {
            world_id: world.into(),
            day,
            year: day / 120,
            season: ["spring", "summer", "autumn", "winter"][(day / 30 % 4) as usize].into(),
            breeding_adults: 10,
            legacy: vec![LegacyEntry { player: 1, share: 0.5, weighted_share: 0.5 }],
            lineages: vec![lineage(0, 10), lineage(1, 0)],
        }
    }

    #[test]
    fn keep_follows_the_age_tiers() {
        assert_eq!(keep(1001, 0), Keep::Full);
        assert_eq!(keep(1001, FULL_DAYS - 1), Keep::Full);
        assert_eq!(keep(1000, FULL_DAYS), Keep::Genes);
        assert_eq!(keep(1001, FULL_DAYS), Keep::Lean);
        assert_eq!(keep(1001, WINDOW_DAYS - 1), Keep::Lean);
        // Past the window: stride 8, then 16 past two windows, 32 past four.
        assert_eq!(keep(1000, WINDOW_DAYS), Keep::Lean);
        assert_eq!(keep(1004, WINDOW_DAYS), Keep::Drop);
        assert_eq!(keep(1000, 2 * WINDOW_DAYS - 1), Keep::Lean);
        assert_eq!(keep(1000, 2 * WINDOW_DAYS), Keep::Drop);
        assert_eq!(keep(1024, 4 * WINDOW_DAYS - 1), Keep::Lean);
        assert_eq!(keep(1040, 4 * WINDOW_DAYS), Keep::Drop);
        assert_eq!(keep(1024, 4 * WINDOW_DAYS), Keep::Lean);
        // Monotone: an older age never keeps more.
        for day in 0..200 {
            let mut last = Keep::Full;
            for age in (0..20 * WINDOW_DAYS).step_by(7) {
                let k = keep(day, age);
                assert!(k >= last, "day {day} age {age}: {k:?} after {last:?}");
                last = k;
            }
        }
        // Huge ages don't overflow.
        assert_eq!(keep(0, u64::MAX), Keep::Lean);
    }

    #[test]
    fn thinning_drops_genes_and_empty_lineages_once() {
        let mut r = rec("w", 40);
        assert!(!thin(&mut r, Keep::Full));
        assert!(thin(&mut r, Keep::Genes));
        assert_eq!(r.lineages.len(), 1);
        assert!(!r.lineages[0].genes.is_empty());
        assert!(thin(&mut r, Keep::Lean));
        assert!(r.lineages[0].genes.is_empty());
        assert!(!thin(&mut r, Keep::Lean), "thinning again is a no-op");
        assert_eq!((r.lineages[0].alive, r.legacy.len()), (10, 1));
    }

    #[test]
    fn ages_bridge_older_worlds_and_gaps() {
        let mut a = AgeWalk::default();
        // Newest world: days 5, 4, 2 (a gap); an older world ending at day 900.
        assert_eq!([5, 4, 2, 900, 899].map(|d| a.next(d)), [0, 1, 3, 4, 5]);
    }

    #[test]
    fn compaction_bounds_a_long_history() {
        let days = 20 * WINDOW_DAYS;
        let mut records: Vec<DailyRecord> = Vec::new();
        let mut peak = 0;
        for d in 0..days {
            records.push(rec("w", d));
            compact(&mut records);
            peak = peak.max(records.len());
        }
        // Window plus ~46 per doubling past it (log2(20) ≈ 4.3 doublings).
        let bound = WINDOW_DAYS as usize + 5 * (WINDOW_DAYS / ARCHIVE_STRIDE) as usize + 5;
        assert!(peak <= bound, "{peak} records > {bound}");
        assert!(records.len() < (days / 10) as usize, "{}", records.len());
        let full = records.iter().filter(|r| r.lineages.len() == 2).count();
        assert_eq!(full, FULL_DAYS as usize);
        let with_genes = records.iter().filter(|r| r.lineages.iter().any(|l| !l.genes.is_empty())).count();
        assert!(with_genes <= FULL_DAYS as usize + (WINDOW_DAYS / GENE_STRIDE) as usize + 1, "{with_genes}");
        // Every day of the window is there, oldest first.
        let newest = days - 1;
        let window: Vec<u64> = records.iter().map(|r| r.day).filter(|&d| newest - d < WINDOW_DAYS).collect();
        assert_eq!(window, (newest + 1 - WINDOW_DAYS..=newest).collect::<Vec<_>>());
        assert!(records.windows(2).all(|w| w[0].day < w[1].day));
        // Compacting again changes nothing, and compacting at any cadence
        // gives the same records.
        assert!(!compact(&mut records));
        let mut once: Vec<DailyRecord> = (0..days).map(|d| rec("w", d)).collect();
        compact(&mut once);
        assert_eq!(once, records);
    }

    #[test]
    fn compaction_keeps_the_newest_of_a_replayed_day() {
        let mut records = vec![rec("w", 1), rec("w", 2), rec("w", 3)];
        let mut replay = rec("w", 2);
        replay.breeding_adults = 99;
        records.push(replay);
        assert!(compact(&mut records));
        assert_eq!(records.iter().map(|r| (r.day, r.breeding_adults)).collect::<Vec<_>>(), [(1, 10), (3, 10), (2, 99)]);
        // Records without a world id are never replaced.
        let mut old = vec![rec("", 1), rec("", 1)];
        assert!(!compact(&mut old));
        assert_eq!(old.len(), 2);
    }

    #[test]
    fn queries_read_ranges_of_the_newest_world() {
        let mut records: Vec<DailyRecord> = (0..50).map(|d| rec("old", d)).collect();
        records.extend((0..20).map(|d| rec("new", d)));
        let days = |q: DailyQuery| select(&records, &q).iter().map(|r| r.day).collect::<Vec<_>>();
        assert_eq!(days(DailyQuery::recent(3)), [17, 18, 19]);
        let range = |from, to, limit| DailyQuery { from_day: from, to_day: to, limit, genes: true };
        assert_eq!(days(range(Some(5), Some(8), 100)), [5, 6, 7, 8]);
        assert_eq!(days(range(Some(5), Some(8), 2)), [7, 8]);
        assert_eq!(days(range(None, Some(1), 3)), [1, 0, 1], "past the newest world's day 0, the older world's day 1");
        assert_eq!(days(range(Some(30), None, 100)), Vec::<u64>::new(), "the newest world has no day 30");
        let lean = select(&records, &DailyQuery { genes: false, ..DailyQuery::recent(1) });
        assert!(lean[0].lineages.iter().all(|l| l.genes.is_empty()));
        assert_eq!(lean[0].lineages.len(), 2);
    }

    /// Prints what the policy stores for a 500-player world (record sizes
    /// as in the six-world soak: a row per player lineage with every gene's
    /// mean, plus wild and extinct lineages). `cargo test --release -p
    /// evo-server --lib history_sizes -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn history_sizes_at_500_players() {
        let lineage = |k: u32, alive: usize, genes: bool| LineageEntry {
            lineage: k,
            name: format!("Lineage {k:05}"),
            owner: if genes { k + 1 } else { 0 },
            alive,
            adults: alive * 2 / 3,
            born: (alive / 10) as u64,
            deaths: if alive > 0 {
                [("starved".to_string(), 3u64), ("predation".to_string(), 2)].into_iter().collect()
            } else {
                BTreeMap::new()
            },
            genes: if genes && alive > 0 {
                evo_sim::genome::Gene::ALL
                    .iter()
                    .enumerate()
                    .map(|(i, &g)| (crate::design::gene_key(g), 0.123_456_79 + i as f32 / 1000.0))
                    .collect()
            } else {
                BTreeMap::new()
            },
            morph: None,
        };
        let mut lineages: Vec<LineageEntry> = (0..500).map(|k| lineage(k, 120, true)).collect();
        lineages.extend((500..530).map(|k| lineage(k, 2000, false)));
        lineages.extend((530..730).map(|k| lineage(k, 0, true)));
        let full = DailyRecord {
            world_id: "0123456789abcdef".into(),
            day: 1000,
            year: 8,
            season: "summer".into(),
            breeding_adults: 40_000,
            legacy: (1..=500)
                .map(|p| LegacyEntry { player: p, share: 0.001_234_567_890_123, weighted_share: 0.000_987_654_321_012 })
                .collect(),
            lineages,
        };
        let size = |r: &DailyRecord| serde_json::to_string(r).unwrap().len() as u64 + 1;
        let mut genes = full.clone();
        thin(&mut genes, Keep::Genes);
        let mut lean = full.clone();
        thin(&mut lean, Keep::Lean);
        let (f, g, l) = (size(&full), size(&genes), size(&lean));
        println!("{} genes; record: whole {f} B, gene day {g} B, lean {l} B", evo_sim::genome::Gene::COUNT);
        for (label, days) in
            [("soak (262 days)", 262u64), ("1 real day", 1800), ("1 real month", 54_000), ("1 real year", 657_000)]
        {
            let mut ages = AgeWalk::default();
            let (mut bytes, mut records) = (0u64, 0u64);
            for day in (0..days).rev() {
                let k = keep(day, ages.next(day));
                bytes += match k {
                    Keep::Full => f,
                    Keep::Genes => g,
                    Keep::Lean => l,
                    Keep::Drop => 0,
                };
                records += u64::from(k != Keep::Drop);
            }
            println!(
                "{label}: {records} records, {:.1} MB (whole records every day: {:.1} MB)",
                bytes as f64 / 1e6,
                (days * f) as f64 / 1e6
            );
        }
    }

    #[test]
    fn the_game_window_thins_in_place() {
        let mut w: VecDeque<DailyRecord> = (0..WINDOW_DAYS + 40).map(|d| rec("w", d)).collect();
        compact_window(&mut w);
        let newest = WINDOW_DAYS + 39;
        assert_eq!(w.back().unwrap().lineages.len(), 2);
        let aged = w.iter().find(|r| r.day == newest - FULL_DAYS - 1).unwrap();
        assert_eq!(aged.lineages.len(), 1);
        assert!(w.iter().all(|r| newest - r.day < WINDOW_DAYS || r.day.is_multiple_of(ARCHIVE_STRIDE)));
    }
}
