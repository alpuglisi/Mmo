//! Era standings by the era's mean share (playtest 5, M12): the sim's era
//! record ranks the final snapshot alone, so a lineage founded near an
//! era's end could place second for it. Here every daily record adds each
//! player's day score (the ranked season's: the mean of raw and weighted
//! share, scaled by the founding ramp) to the era it belongs to; when the
//! era closes its means are kept, and the era's ranks and alerts use them.

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
}

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
            self.sums.clear();
            self.days = 0;
            self.era = era;
        }
        self.days += 1;
        for e in &record.legacy {
            *self.sums.entry(e.player).or_default() += (e.share + e.weighted_share) / 2.0;
        }
    }

    /// Close `era`: keep its means (when its days were counted here) and
    /// return them.
    pub fn close(&mut self, era: u64) -> Option<&BTreeMap<PlayerId, f64>> {
        if self.era == era && self.days > 0 {
            let days = self.days as f64;
            let means = self.sums.iter().map(|(&p, &s)| (p, s / days)).collect();
            self.means.insert(era, means);
            self.sums.clear();
            self.days = 0;
            self.era = era + 1;
        }
        self.means.get(&era)
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
    /// held a fair share all era.
    #[test]
    fn eras_rank_by_the_mean_not_the_last_day() {
        let mut s = EraScores::default();
        // Era 0 of 4 days: records of days 1..=4.
        for d in 1..=3 {
            s.add(&rec(d, &[(1, 0.3)]), 4);
        }
        s.add(&rec(4, &[(1, 0.2), (2, 0.4)]), 4);
        let m = s.close(0).unwrap().clone();
        assert!((m[&1] - 0.275).abs() < 1e-9 && (m[&2] - 0.1).abs() < 1e-9, "{m:?}");
        // The next era starts afresh.
        s.add(&rec(5, &[(2, 0.5)]), 4);
        assert_eq!(s.close(1).unwrap().get(&1), None);
        assert_eq!(s.means(0), Some(&m));
    }
}
