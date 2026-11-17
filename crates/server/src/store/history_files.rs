//! The file store's daily history: `worlds/<id>/history/`, segment files of
//! up to [`SEGMENT_LINES`] lines (`0000000000.jsonl`, numbered in write
//! order). Records are appended to the newest segment; when it is full the
//! next record starts a new one and the older segments are compacted by the
//! retention policy ([`crate::history`]): replaced and dropped records
//! removed, aged records thinned, a segment rewritten (a temp file renamed
//! into place) only when it changed, and neighbours small enough merged so
//! the number of files stays bounded too. Reads walk the segments from the
//! newest and stop as soon as the query is answered, parsing only the lines
//! they need.
//!
//! A world's `history.jsonl` from before segments is split into segments
//! after any existing ones and removed on first use; a crash before the
//! removal splits it again, and the copies replace the first ones (the
//! newest record of a world and day wins).

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use evo_sim::PlayerId;
use serde::Deserialize;

use super::{DailyRecord, LegacyEntry, LineageEntry, StoreResult};
use crate::history::{self, AgeWalk, DailyQuery, Dedup, Keep, QueryWalk, Step};

/// A world's history file from before segments.
pub(super) const LEGACY_FILE: &str = "history.jsonl";
/// A world's directory of segments.
pub(super) const DIR: &str = "history";
/// Lines in a full segment.
pub const SEGMENT_LINES: usize = 32;

/// One world's segments (all access is under one lock per world).
pub(super) struct HistoryFiles {
    dir: PathBuf,
    /// Segment numbers, oldest first; the last is appended to.
    segments: Vec<u64>,
    /// Lines in the last segment.
    live_lines: usize,
}

fn segment_name(seq: u64) -> String {
    format!("{seq:010}.jsonl")
}

fn parse_segment_name(name: &str) -> Option<u64> {
    let stem = name.strip_suffix(".jsonl")?;
    if stem.len() == 10 && stem.bytes().all(|b| b.is_ascii_digit()) {
        stem.parse().ok()
    } else {
        None
    }
}

/// The world and day of a line, without building the rest.
#[derive(Deserialize)]
struct Key {
    #[serde(default)]
    world_id: String,
    day: u64,
}

fn key(line: &str) -> Option<Key> {
    match serde_json::from_str(line) {
        Ok(k) => Some(k),
        Err(e) => {
            tracing::warn!("skipping an unreadable daily history line: {e}");
            None
        }
    }
}

/// A record read without its gene means (serde skips them).
#[derive(Deserialize)]
struct LeanRecord {
    #[serde(default)]
    world_id: String,
    day: u64,
    year: u64,
    season: String,
    breeding_adults: usize,
    legacy: Vec<LegacyEntry>,
    lineages: Vec<LeanLineage>,
}

#[derive(Deserialize)]
struct LeanLineage {
    lineage: u32,
    name: String,
    owner: PlayerId,
    alive: usize,
    adults: usize,
    #[serde(default)]
    born: u64,
    #[serde(default)]
    deaths: BTreeMap<String, u64>,
}

fn parse_record(line: &str, genes: bool) -> StoreResult<DailyRecord> {
    if genes {
        return Ok(serde_json::from_str(line)?);
    }
    let r: LeanRecord = serde_json::from_str(line)?;
    Ok(DailyRecord {
        world_id: r.world_id,
        day: r.day,
        year: r.year,
        season: r.season,
        breeding_adults: r.breeding_adults,
        legacy: r.legacy,
        lineages: r
            .lineages
            .into_iter()
            .map(|l| LineageEntry {
                lineage: l.lineage,
                name: l.name,
                owner: l.owner,
                alive: l.alive,
                adults: l.adults,
                born: l.born,
                deaths: l.deaths,
                genes: BTreeMap::new(),
                morph: None,
            })
            .collect(),
    })
}

fn non_empty(text: &str) -> impl DoubleEndedIterator<Item = &str> {
    text.lines().filter(|l| !l.trim().is_empty())
}

/// Write `bytes` to `dir/name` through a temp file renamed into place.
fn write_atomically(dir: &Path, name: &str, bytes: &[u8]) -> StoreResult<()> {
    let tmp = dir.join(format!("{name}.tmp"));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, dir.join(name))?;
    Ok(())
}

/// A segment's state during compaction: its lines when they changed or
/// were read for a merge (none: the file on disk is current), and the
/// newer segments merged into it, deleted once it is written.
struct Pending {
    seq: u64,
    count: usize,
    lines: Option<Vec<String>>,
    absorbed: Vec<u64>,
}

impl HistoryFiles {
    /// A world's history in `world_dir`, splitting a legacy file into
    /// segments first.
    pub(super) fn open(world_dir: &Path) -> StoreResult<Self> {
        let dir = world_dir.join(DIR);
        let mut h = Self { segments: list(&dir)?, dir, live_lines: 0 };
        let legacy = world_dir.join(LEGACY_FILE);
        let migrated = legacy.exists();
        if migrated {
            h.split_legacy(&legacy)?;
            h.segments = list(&h.dir)?;
        }
        if let Some(&live) = h.segments.last() {
            let path = h.path(live);
            let bytes = fs::read(&path)?;
            // A torn last line (a crash mid-append) ends here: later lines
            // start on their own, and readers skip it.
            if bytes.last().is_some_and(|&b| b != b'\n') {
                fs::OpenOptions::new().append(true).open(&path)?.write_all(b"\n")?;
            }
            h.live_lines = bytes.split(|&b| b == b'\n').filter(|l| !l.trim_ascii().is_empty()).count();
        }
        if migrated {
            h.compact()?;
        }
        Ok(h)
    }

    fn path(&self, seq: u64) -> PathBuf {
        self.dir.join(segment_name(seq))
    }

    /// Split `legacy` into segments after the existing ones, then remove it.
    fn split_legacy(&mut self, legacy: &Path) -> StoreResult<()> {
        fs::create_dir_all(&self.dir)?;
        let mut next = self.segments.last().map_or(0, |s| s + 1);
        let (mut buf, mut count, mut total) = (Vec::new(), 0usize, 0usize);
        for line in BufReader::new(fs::File::open(legacy)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            buf.extend_from_slice(line.as_bytes());
            buf.push(b'\n');
            count += 1;
            total += 1;
            if count == SEGMENT_LINES {
                write_atomically(&self.dir, &segment_name(next), &buf)?;
                next += 1;
                buf.clear();
                count = 0;
            }
        }
        if count > 0 {
            write_atomically(&self.dir, &segment_name(next), &buf)?;
        }
        fs::remove_file(legacy)?;
        tracing::info!("split {} ({total} daily records) into segments in {}", legacy.display(), self.dir.display());
        Ok(())
    }

    /// Append a record; starting a new segment compacts the older ones.
    pub(super) fn append(&mut self, record: &DailyRecord) -> StoreResult<()> {
        fs::create_dir_all(&self.dir)?;
        let started = self.segments.is_empty() || self.live_lines >= SEGMENT_LINES;
        if started {
            self.segments.push(self.segments.last().map_or(0, |s| s + 1));
            self.live_lines = 0;
        }
        let live = *self.segments.last().expect("a segment");
        let mut line = serde_json::to_vec(record)?;
        line.push(b'\n');
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.path(live))?;
        f.write_all(&line)?;
        f.flush()?;
        self.live_lines += 1;
        if started && self.segments.len() > 1 {
            self.compact()?;
        }
        Ok(())
    }

    /// The records `q` reads, oldest first.
    pub(super) fn read(&self, q: &DailyQuery) -> StoreResult<Vec<DailyRecord>> {
        let mut walk = QueryWalk::new(*q);
        let mut dedup = Dedup::default();
        let mut out = Vec::new();
        'segments: for &seq in self.segments.iter().rev() {
            if walk.done() {
                break;
            }
            let text = fs::read_to_string(self.path(seq))?;
            for line in non_empty(&text).rev() {
                let Some(k) = key(line) else { continue };
                if !dedup.first(&k.world_id, k.day) {
                    continue;
                }
                match walk.step(k.day) {
                    Step::Take => out.push(parse_record(line, q.genes)?),
                    Step::Skip => {}
                    Step::Stop => break 'segments,
                }
            }
        }
        out.reverse();
        Ok(out)
    }

    /// Apply the retention policy to every segment but the newest.
    pub(super) fn compact(&mut self) -> StoreResult<()> {
        let Some((&live, sealed)) = self.segments.split_last() else { return Ok(()) };
        let mut ages = AgeWalk::default();
        let mut dedup = Dedup::default();
        // The newest segment only starts the ages and hides what it replaced.
        for k in non_empty(&fs::read_to_string(self.path(live))?).rev().filter_map(key) {
            if dedup.first(&k.world_id, k.day) {
                ages.next(k.day);
            }
        }
        let mut kept = vec![live];
        let mut pending: Option<Pending> = None;
        for &seq in sealed.iter().rev() {
            let text = fs::read_to_string(self.path(seq))?;
            let lines: Vec<&str> = non_empty(&text).collect();
            // What each line keeps (none: replaced or unreadable), newest first.
            let mut keeps: Vec<Option<Keep>> = lines
                .iter()
                .rev()
                .map(|line| {
                    let k = key(line)?;
                    if !dedup.first(&k.world_id, k.day) {
                        return None;
                    }
                    let keep = history::keep(k.day, ages.next(k.day));
                    (keep != Keep::Drop).then_some(keep)
                })
                .collect();
            keeps.reverse();
            let cur = if keeps.iter().all(|k| *k == Some(Keep::Full)) {
                Pending { seq, count: lines.len(), lines: None, absorbed: Vec::new() }
            } else {
                let mut changed = false;
                let mut out = Vec::with_capacity(lines.len());
                for (line, keep) in lines.iter().zip(&keeps) {
                    match *keep {
                        None => changed = true,
                        Some(Keep::Full) => out.push(line.to_string()),
                        Some(keep) => {
                            let mut r: DailyRecord = serde_json::from_str(line)?;
                            if history::thin(&mut r, keep) {
                                changed = true;
                                out.push(serde_json::to_string(&r)?);
                            } else {
                                out.push(line.to_string());
                            }
                        }
                    }
                }
                Pending { seq, count: out.len(), lines: changed.then_some(out), absorbed: Vec::new() }
            };
            if cur.count == 0 {
                fs::remove_file(self.path(seq))?;
                continue;
            }
            pending = Some(match pending.take() {
                Some(newer) if newer.count + cur.count <= SEGMENT_LINES => {
                    let mut lines = self.lines_of(&cur)?;
                    lines.extend(self.lines_of(&newer)?);
                    let mut absorbed = newer.absorbed;
                    absorbed.push(newer.seq);
                    Pending { seq, count: lines.len(), lines: Some(lines), absorbed }
                }
                Some(newer) => {
                    kept.push(newer.seq);
                    self.flush(newer)?;
                    cur
                }
                None => cur,
            });
        }
        if let Some(p) = pending {
            kept.push(p.seq);
            self.flush(p)?;
        }
        kept.sort_unstable();
        self.segments = kept;
        Ok(())
    }

    fn lines_of(&self, p: &Pending) -> StoreResult<Vec<String>> {
        match &p.lines {
            Some(lines) => Ok(lines.clone()),
            None => Ok(non_empty(&fs::read_to_string(self.path(p.seq))?).map(str::to_string).collect()),
        }
    }

    fn flush(&self, p: Pending) -> StoreResult<()> {
        if let Some(lines) = p.lines {
            let mut bytes = Vec::new();
            for l in lines {
                bytes.extend_from_slice(l.as_bytes());
                bytes.push(b'\n');
            }
            write_atomically(&self.dir, &segment_name(p.seq), &bytes)?;
        }
        for seq in p.absorbed {
            fs::remove_file(self.path(seq))?;
        }
        Ok(())
    }

    /// Bytes on disk and segment files (for tests and reports).
    pub fn usage(&self) -> StoreResult<(u64, usize)> {
        let mut bytes = 0;
        for &seq in &self.segments {
            bytes += fs::metadata(self.path(seq))?.len();
        }
        Ok((bytes, self.segments.len()))
    }
}

/// The segment numbers in `dir`, oldest first (none if it doesn't exist).
fn list(dir: &Path) -> StoreResult<Vec<u64>> {
    let mut out = Vec::new();
    match fs::read_dir(dir) {
        Ok(entries) => {
            for e in entries {
                if let Some(seq) = e?.file_name().to_str().and_then(parse_segment_name) {
                    out.push(seq);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    out.sort_unstable();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("evo-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn rec(world: &str, day: u64) -> DailyRecord {
        let lineage = |k: u32, alive: usize| LineageEntry {
            lineage: k,
            name: format!("L{k}"),
            owner: k + 1,
            alive,
            adults: alive,
            born: 1,
            deaths: BTreeMap::new(),
            genes: (0..20).map(|g| (format!("gene_{g}"), 0.25)).collect(),
            morph: None,
        };
        DailyRecord {
            world_id: world.into(),
            day,
            year: day / 120,
            season: "spring".into(),
            breeding_adults: day as usize,
            legacy: vec![LegacyEntry { player: 1, share: 0.5, weighted_share: 0.5 }],
            lineages: vec![lineage(0, 10), lineage(1, 3)],
        }
    }

    fn days(records: &[DailyRecord]) -> Vec<u64> {
        records.iter().map(|r| r.day).collect()
    }

    #[test]
    fn segments_follow_the_policy_and_stay_bounded() {
        let dir = temp("bounded");
        let mut h = HistoryFiles::open(&dir).unwrap();
        let total = 6 * history::WINDOW_DAYS;
        let mut peak_files = 0;
        for d in 0..total {
            h.append(&rec("w", d)).unwrap();
            peak_files = peak_files.max(h.segments.len());
        }
        let stored = h.read(&DailyQuery::recent(usize::MAX)).unwrap();
        // The newest segment is as appended; the older ones were compacted
        // when its first record came, so they hold what compacting the
        // records up to that one in memory keeps.
        let live = h.live_lines;
        assert!(live <= SEGMENT_LINES);
        let sealed = stored.len() - live;
        let first_live = total - live as u64;
        assert_eq!(days(&stored[sealed..]), (first_live..total).collect::<Vec<_>>());
        let mut expected: Vec<DailyRecord> = (0..=first_live).map(|d| rec("w", d)).collect();
        history::compact(&mut expected);
        expected.pop();
        assert_eq!(&stored[..sealed], &expected[..]);
        let bound = history::WINDOW_DAYS as usize + 3 * (history::WINDOW_DAYS / history::ARCHIVE_STRIDE) as usize;
        assert!(stored.len() <= bound + SEGMENT_LINES, "{} records", stored.len());
        assert!(peak_files <= bound / SEGMENT_LINES + 8, "{peak_files} segment files");
        assert!(stored.windows(2).all(|w| w[0].day < w[1].day));
        // Thinned as their ages say.
        for r in &stored {
            let age = total - 1 - r.day;
            let genes = r.lineages.iter().any(|l| !l.genes.is_empty());
            if age < history::FULL_DAYS {
                assert!(genes, "day {}", r.day);
            }
            if age >= history::FULL_DAYS + SEGMENT_LINES as u64 && !r.day.is_multiple_of(history::GENE_STRIDE) {
                assert!(!genes, "day {}", r.day);
            }
        }

        // Ranges read the newest segments only, stopping early.
        let q = |from, to, limit| DailyQuery { from_day: from, to_day: to, limit, genes: true };
        assert_eq!(days(&h.read(&q(None, None, 3)).unwrap()), [total - 3, total - 2, total - 1]);
        let range = h.read(&q(Some(total - 50), Some(total - 48), 10)).unwrap();
        assert_eq!(days(&range), [total - 50, total - 49, total - 48]);
        let lean = h.read(&DailyQuery { genes: false, ..q(None, None, 1) }).unwrap();
        assert!(lean[0].lineages.iter().all(|l| l.genes.is_empty()) && lean[0].lineages.len() == 2);
        // Reopening finds the same segments.
        let again = HistoryFiles::open(&dir).unwrap();
        assert_eq!((again.segments.clone(), again.live_lines), (h.segments.clone(), h.live_lines));
        assert_eq!(again.read(&DailyQuery::recent(usize::MAX)).unwrap(), stored);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_legacy_file_is_split_and_compacted() {
        let dir = temp("legacy");
        let mut text = String::new();
        for d in 0..500 {
            text.push_str(&serde_json::to_string(&rec("w", d)).unwrap());
            text.push('\n');
        }
        // A replayed day.
        text.push_str(&serde_json::to_string(&DailyRecord { breeding_adults: 7, ..rec("w", 499) }).unwrap());
        text.push('\n');
        fs::write(dir.join(LEGACY_FILE), &text).unwrap();
        let h = HistoryFiles::open(&dir).unwrap();
        assert!(!dir.join(LEGACY_FILE).exists());
        let all = h.read(&DailyQuery::recent(usize::MAX)).unwrap();
        let mut expected: Vec<DailyRecord> = (0..500).map(|d| rec("w", d)).collect();
        expected[499].breeding_adults = 7;
        history::compact(&mut expected);
        assert_eq!(all.last().unwrap().breeding_adults, 7);
        assert_eq!(days(&all), days(&expected));
        assert!(all.len() < 450, "{}", all.len());

        // A crash before the old file was removed: split again, the copies
        // replace the first records.
        fs::write(dir.join(LEGACY_FILE), &text).unwrap();
        let h = HistoryFiles::open(&dir).unwrap();
        assert_eq!(days(&h.read(&DailyQuery::recent(usize::MAX)).unwrap()), days(&expected));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_torn_line_is_skipped() {
        let dir = temp("torn");
        let mut h = HistoryFiles::open(&dir).unwrap();
        h.append(&rec("w", 0)).unwrap();
        let live = h.path(0);
        fs::OpenOptions::new().append(true).open(&live).unwrap().write_all(b"{\"world_id\":\"w\",\"da").unwrap();
        let mut h = HistoryFiles::open(&dir).unwrap();
        h.append(&rec("w", 1)).unwrap();
        assert_eq!(days(&h.read(&DailyQuery::recent(10)).unwrap()), [0, 1]);
        let _ = fs::remove_dir_all(&dir);
    }
}
