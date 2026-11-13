//! Seasonal leaderboards ("Scoring" in the design doc): the daily history
//! records grouped by season, each player ranked by mean legacy share over
//! the season's recorded days.

use std::collections::BTreeMap;

use evo_sim::PlayerId;

use crate::protocol::{SeasonBoard, SeasonRow};
use crate::store::DailyRecord;

#[derive(Default)]
struct Acc {
    share_sum: f64,
    weighted_sum: f64,
    final_share: f64,
    peak_share: f64,
    peak_alive: usize,
    days: usize,
}

/// Group `records` (oldest first) into seasons and rank each. A season is a
/// run of consecutive records with the same year and season whose days don't
/// go backwards (a server restart starts a new world at day 0, so its
/// records start a new run). Returns the newest `limit` seasons, newest
/// first; the newest is marked `current` when it is `current`.
pub fn season_boards(
    records: &[DailyRecord],
    current: Option<(u64, &str)>,
    name_of: impl Fn(PlayerId) -> String,
    limit: usize,
) -> Vec<SeasonBoard> {
    let mut runs: Vec<&[DailyRecord]> = Vec::new();
    let mut start = 0;
    for k in 1..=records.len() {
        let split = k == records.len() || {
            let (a, b) = (&records[k - 1], &records[k]);
            a.year != b.year || a.season != b.season || b.day < a.day
        };
        if split && k > start {
            runs.push(&records[start..k]);
            start = k;
        }
    }
    let n = runs.len();
    runs.iter()
        .enumerate()
        .rev()
        .take(limit)
        .map(|(k, run)| {
            let mut acc: BTreeMap<PlayerId, Acc> = BTreeMap::new();
            for r in run.iter() {
                let mut alive: BTreeMap<PlayerId, usize> = BTreeMap::new();
                for l in &r.lineages {
                    *alive.entry(l.owner).or_default() += l.alive;
                }
                for e in &r.legacy {
                    let a = acc.entry(e.player).or_default();
                    a.share_sum += e.share;
                    a.weighted_sum += e.weighted_share;
                    a.final_share = e.share;
                    a.peak_share = a.peak_share.max(e.share);
                    a.days += 1;
                }
                for (p, n) in alive {
                    let a = acc.entry(p).or_default();
                    a.peak_alive = a.peak_alive.max(n);
                }
            }
            let days = run.len();
            let mut rows: Vec<SeasonRow> = acc
                .into_iter()
                .map(|(player, a)| SeasonRow {
                    player,
                    name: name_of(player),
                    // Days without a share count as zero.
                    mean_share: a.share_sum / days as f64,
                    final_share: a.final_share,
                    peak_share: a.peak_share,
                    mean_weighted_share: a.weighted_sum / days as f64,
                    peak_alive: a.peak_alive,
                    days: a.days,
                })
                .collect();
            rows.sort_by(|a, b| b.mean_share.total_cmp(&a.mean_share).then(a.player.cmp(&b.player)));
            let (first, last) = (&run[0], &run[days - 1]);
            SeasonBoard {
                year: first.year,
                season: first.season.clone(),
                first_day: first.day,
                last_day: last.day,
                days,
                current: k + 1 == n && current.is_some_and(|(y, s)| y == first.year && s == first.season),
                rows,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{LegacyEntry, LineageEntry};

    fn rec(day: u64, year: u64, season: &str, shares: &[(PlayerId, f64, usize)]) -> DailyRecord {
        DailyRecord {
            day,
            year,
            season: season.into(),
            breeding_adults: 10,
            legacy: shares.iter().map(|&(player, share, _)| LegacyEntry { player, share, weighted_share: share / 2.0 }).collect(),
            lineages: shares
                .iter()
                .map(|&(owner, _, alive)| LineageEntry {
                    lineage: owner,
                    name: format!("L{owner}"),
                    owner,
                    alive,
                    adults: alive,
                    born: 0,
                    deaths: Default::default(),
                    genes: Default::default(),
                })
                .collect(),
        }
    }

    #[test]
    fn groups_by_season_and_ranks_by_mean_share() {
        let records = vec![
            rec(0, 0, "spring", &[(0, 0.8, 100), (1, 0.2, 10)]),
            rec(1, 0, "spring", &[(0, 0.6, 90), (1, 0.4, 30)]),
            rec(2, 0, "summer", &[(0, 0.3, 80), (1, 0.5, 40), (2, 0.2, 5)]),
            rec(3, 0, "summer", &[(0, 0.3, 70), (1, 0.6, 50)]),
            // Restart: a new world from day 0 is a new season run.
            rec(0, 0, "spring", &[(0, 1.0, 100)]),
        ];
        let name = |p: PlayerId| format!("p{p}");
        let boards = season_boards(&records, Some((0, "spring")), name, 10);
        assert_eq!(boards.len(), 3);
        assert!(boards[0].current && !boards[1].current);
        assert_eq!((boards[0].first_day, boards[0].days), (0, 1));
        let summer = &boards[1];
        assert_eq!((summer.season.as_str(), summer.first_day, summer.last_day, summer.days), ("summer", 2, 3, 2));
        assert_eq!(summer.rows[0].player, 1);
        assert!((summer.rows[0].mean_share - 0.55).abs() < 1e-9);
        assert!((summer.rows[0].final_share - 0.6).abs() < 1e-9);
        assert_eq!(summer.rows[0].peak_alive, 50);
        // Player 2 appears on one of two days: its mean counts the gap as 0.
        let p2 = summer.rows.iter().find(|r| r.player == 2).unwrap();
        assert!((p2.mean_share - 0.1).abs() < 1e-9 && p2.days == 1 && p2.name == "p2");
        let spring = &boards[2];
        assert_eq!(spring.rows[0].player, 0);
        assert!((spring.rows[0].mean_weighted_share - 0.35).abs() < 1e-9);
        assert_eq!(season_boards(&records, None, |_| String::new(), 1).len(), 1);
        assert!(season_boards(&[], None, |_| String::new(), 5).is_empty());
    }
}
