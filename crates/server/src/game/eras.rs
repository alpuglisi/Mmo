//! Era standings by the era's mean share (playtest 5, M12): the sim's era
//! record ranks the final snapshot alone, so a lineage founded near an
//! era's end could place second for it. Here every daily record adds each
//! player's day score (the ranked season's: the mean of raw and weighted
//! share, scaled by the founding ramp) to the era it belongs to; when the
//! era closes its means are kept, and the era's ranks and alerts use them.
//!
//! Playtest 12: founding on an era's first day left a late joiner 4×
//! behind on the plain mean (6.3% against 25.8%) while it held half the
//! leader's share at the end. An era's score is now [`MEAN_WEIGHT`] of a
//! mean that weights each day by its place in the era (the last day counts
//! as many times as the era has days, the first once) and the rest the
//! mean of its last tenth: growth late in an era counts, a one-day spike
//! doesn't.

use std::collections::BTreeMap;

use evo_sim::PlayerId;
use serde::{Deserialize, Serialize};

use crate::store::DailyRecord;

/// The era under way's running scores and every finished era's means
/// (saved with the game; empty in older saves, whose eras keep the sim's
/// final-snapshot ranks).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EraScores {
    /// The era the running sums belong to.
    era: u64,
    /// Daily records counted in it.
    days: u32,
    sums: BTreeMap<PlayerId, f64>,
    /// Finished eras: each player's mean day score.
    means: BTreeMap<u64, BTreeMap<PlayerId, f64>>,
    /// Day scores weighted by their place in the era, and the weights'
    /// sum (absent in older saves: the plain mean).
    weighted: BTreeMap<PlayerId, f64>,
    weights: f64,
    /// Day scores over the era's last tenth, and the days counted.
    tail: BTreeMap<PlayerId, f64>,
    tail_days: u32,
}

/// The share of an era's score from its time-weighted mean; the rest is the
/// mean of its last tenth ([`EraScores`]).
pub const MEAN_WEIGHT: f64 = 0.75;

impl EraScores {
    /// The era a daily record belongs to: a record is taken as its day
    /// starts, so it scores the day before (the era's last record is taken
    /// as the next era starts, as the sim closes it).
    pub fn era_of(day: u64, era_days: u64) -> u64 {
        day.saturating_sub(1) / era_days.max(1)
    }

    /// Count `record` (its shares already scaled by the founding ramp) for
    /// its era; a record of a later era starts that era's sums afresh.
    pub fn add(&mut self, record: &DailyRecord, era_days: u64) {
        let era = Self::era_of(record.day, era_days);
        if era < self.era {
            return;
        }
        if era > self.era {
            self.clear();
            self.era = era;
        }
        self.days += 1;
        // The day's place in the era (1 on its first day).
        let place = (record.day.saturating_sub(1) % era_days.max(1) + 1) as f64;
        let in_tail = place > 0.9 * era_days.max(1) as f64;
        self.weights += place;
        self.tail_days += u32::from(in_tail);
        for e in &record.legacy {
            let score = (e.share + e.weighted_share) / 2.0;
            *self.sums.entry(e.player).or_default() += score;
            *self.weighted.entry(e.player).or_default() += place * score;
            if in_tail {
                *self.tail.entry(e.player).or_default() += score;
            }
        }
    }

    /// Close `era`: keep its means (when its days were counted here) and
    /// return them.
    pub fn close(&mut self, era: u64) -> Option<&BTreeMap<PlayerId, f64>> {
        if self.era == era && self.days > 0 {
            let days = self.days as f64;
            let means = self
                .sums
                .iter()
                .map(|(&p, &s)| {
                    if self.weights <= 0.0 {
                        return (p, s / days);
                    }
                    let weighted = self.weighted.get(&p).copied().unwrap_or(0.0) / self.weights;
                    let tail = if self.tail_days > 0 {
                        self.tail.get(&p).copied().unwrap_or(0.0) / self.tail_days as f64
                    } else {
                        weighted
                    };
                    (p, MEAN_WEIGHT * weighted + (1.0 - MEAN_WEIGHT) * tail)
                })
                .collect();
            self.means.insert(era, means);
            self.clear();
            self.era = era + 1;
        }
        self.means.get(&era)
    }

    fn clear(&mut self) {
        self.sums.clear();
        self.days = 0;
        self.weighted.clear();
        self.weights = 0.0;
        self.tail.clear();
        self.tail_days = 0;
    }

    /// A finished era's means, if it was scored here.
    pub fn means(&self, era: u64) -> Option<&BTreeMap<PlayerId, f64>> {
        self.means.get(&era)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LegacyEntry;

    fn rec(day: u64, shares: &[(PlayerId, f64)]) -> DailyRecord {
        DailyRecord {
            world_id: String::new(),
            day,
            year: 0,
            season: String::new(),
            breeding_adults: 0,
            legacy: shares
                .iter()
                .map(|&(player, share)| LegacyEntry { player, share, weighted_share: share })
                .collect(),
            lineages: Vec::new(),
        }
    }

    /// A late joiner with a big final share doesn't outrank a player who
    /// held a fair share all era; the later days count more, and the last
    /// tenth's mean a quarter.
    #[test]
    fn eras_rank_by_the_mean_not_the_last_day() {
        let mut s = EraScores::default();
        // Era 0 of 4 days: records of days 1..=4 (weights 1, 2, 3, 4; the
        // last tenth is day 4).
        for d in 1..=3 {
            s.add(&rec(d, &[(1, 0.3)]), 4);
        }
        s.add(&rec(4, &[(1, 0.2), (2, 0.4)]), 4);
        let m = s.close(0).unwrap().clone();
        let (one, two) = (0.75 * (0.3 * 6.0 + 0.2 * 4.0) / 10.0 + 0.25 * 0.2, 0.75 * 0.4 * 4.0 / 10.0 + 0.25 * 0.4);
        assert!((m[&1] - one).abs() < 1e-9 && (m[&2] - two).abs() < 1e-9, "{m:?}");
        assert!(m[&1] > m[&2]);
        // The next era starts afresh.
        s.add(&rec(5, &[(2, 0.5)]), 4);
        assert_eq!(s.close(1).unwrap().get(&1), None);
        assert_eq!(s.means(0), Some(&m));
    }

    /// Playtest 12: a late joiner growing through the era was 4× behind on
    /// the plain mean while holding half the leader's share at the end.
    #[test]
    fn late_growth_counts_more_than_early_share() {
        let mut s = EraScores::default();
        let n = 100u64;
        for d in 1..=n {
            let t = d as f64 / n as f64;
            // The leader holds 30% then sinks to 20%; the joiner grows 0 → 10%.
            s.add(&rec(d, &[(1, 0.3 - 0.1 * t), (2, 0.1 * t)]), n);
        }
        let m = s.close(0).unwrap().clone();
        let plain = (0.25, 0.05);
        assert!(m[&2] / m[&1] > plain.1 / plain.0 + 0.1, "{m:?}");
        assert!(m[&1] > m[&2]);
    }
}
