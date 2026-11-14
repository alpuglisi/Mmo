//! Ranked seasons: the design doc's "seasonal leaderboards: snapshot and
//! rank at the end of each era", on a fixed clock of its own.
//!
//! A ranked season lasts [`crate::game::GameConfig::season_days`] simulated
//! days (60 by default, `--season-days`). While it runs, a
//! [`SeasonTracker`] (saved with the game) adds up each player's standing in
//! every [`Category`] from the daily records and the game's events; when the
//! first daily record past its end arrives, the standings are frozen into a
//! [`SeasonRecord`] (final ranks per category, winners and a one-line
//! summary), which the store keeps under `(world id, season number)`.
//!
//! Restart safety: the tracker lives in the saved game, so a world restored
//! from an older save replays the days since and rebuilds exactly the
//! standings it had (a day is counted at most once: `last_day`), and a
//! replayed rollover writes the same key again, replacing the record instead
//! of adding one. The last finished record is also kept in the saved game and
//! written again on startup, so a rollover whose record was lost (a crash
//! after the save that followed it, a failed write) is not lost for good.
//!
//! These are not the weather seasons of the calendar (`crate::seasons`
//! groups the daily history by those); "season" in the `/api/seasons` routes
//! always means a ranked season.

use std::collections::{BTreeMap, HashMap};

use evo_sim::{PlayerId, WILD};
use serde::{Deserialize, Serialize};

use crate::store::DailyRecord;

/// Default length of a ranked season, in simulated days (half a default
/// 120-day year; an era is 3 years).
pub const DEFAULT_SEASON_DAYS: u64 = 60;
/// Rows per category returned when a request names no limit.
pub const DEFAULT_ROWS: usize = 10;
/// Most rows per category in one response.
pub const MAX_ROWS: usize = 200;

/// What a ranked season ranks. The design doc names the legacy share as
/// the score and species founded, lineage longevity, niche dominance and
/// surviving catastrophes as prestige; the stud marketplace is its trading
/// lever. (It has no tournaments: the calibration "tournaments" are an
/// offline balance tool, not play.)
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Mean legacy share over the season's daily snapshots (a day without
    /// a share counts as 0); ties go to the higher final share. The season's
    /// champion.
    Legacy,
    /// Most living members the player's lineage had on a recorded day.
    Lineage,
    /// Species founded (credited speciation events) during the season.
    Species,
    /// Renown earned from stud fees (escrow paid out) during the season.
    Market,
    /// Catastrophes survived (a lineage alive at the start and the end)
    /// during the season.
    Survival,
}

impl Category {
    pub const ALL: [Category; 5] =
        [Category::Legacy, Category::Lineage, Category::Species, Category::Market, Category::Survival];

    pub fn name(self) -> &'static str {
        match self {
            Category::Legacy => "legacy",
            Category::Lineage => "lineage",
            Category::Species => "species",
            Category::Market => "market",
            Category::Survival => "survival",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == s)
    }

    /// The value as the summary shows it.
    fn describe(self, value: f64) -> String {
        let n = value.round() as u64;
        let s = |word: &str| if n == 1 { word.to_string() } else { format!("{word}s") };
        match self {
            Category::Legacy => format!("{:.1}% mean share", value * 100.0),
            Category::Lineage => format!("{n} {}", s("organism")),
            Category::Species => format!("{n} species"),
            Category::Market => format!("{n} renown"),
            Category::Survival => format!("{n} {}", s("catastrophe")),
        }
    }

    fn title(self) -> &'static str {
        match self {
            Category::Legacy => "legacy",
            Category::Lineage => "largest lineage",
            Category::Species => "species founded",
            Category::Market => "stud market",
            Category::Survival => "survival",
        }
    }
}

/// One player's final place in one category. Ranks are competition ranks
/// (1, 2, 2, 4): equal values share a place. Only players with something to
/// show (a value above zero) are ranked.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Placement {
    pub player: PlayerId,
    pub name: String,
    pub rank: u32,
    pub value: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Standing {
    pub category: Category,
    /// Best first.
    pub rows: Vec<Placement>,
}

/// A category's winner (rank 1; several when tied).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Winner {
    pub category: Category,
    pub player: PlayerId,
    pub name: String,
    pub value: f64,
}

/// Everything about a finished season except the full standings: what the
/// season list shows.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SeasonHead {
    pub world_id: String,
    /// 1 for a world's first season.
    pub season: u32,
    /// Daily snapshots counted: `first_day..=last_day`.
    pub first_day: u64,
    pub last_day: u64,
    /// Length the season was scheduled for, in days.
    pub length: u64,
    /// Daily snapshots recorded (fewer than `length` when the season was
    /// joined late, e.g. by a saved game from before ranked seasons).
    pub days: u32,
    /// World tick and wall-clock time (Unix seconds) of the rollover.
    pub ended_tick: u64,
    pub ended_unix: u64,
    /// Players ranked in any category.
    pub players: usize,
    pub winners: Vec<Winner>,
    pub summary: String,
}

/// A finished season as stored: the head and every category's standings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SeasonRecord {
    #[serde(flatten)]
    pub head: SeasonHead,
    pub standings: Vec<Standing>,
}

impl SeasonRecord {
    pub fn key(&self) -> (&str, u32) {
        (&self.head.world_id, self.head.season)
    }

    /// The player's places, if ranked in any category.
    pub fn player_season(&self, player: PlayerId) -> Option<PlayerSeason> {
        let placements: Vec<PlayerPlacement> = self
            .standings
            .iter()
            .filter_map(|s| {
                s.rows.iter().find(|r| r.player == player).map(|r| PlayerPlacement {
                    category: s.category,
                    rank: r.rank,
                    value: r.value,
                    of: s.rows.len(),
                })
            })
            .collect();
        (!placements.is_empty()).then(|| PlayerSeason {
            world_id: self.head.world_id.clone(),
            season: self.head.season,
            first_day: self.head.first_day,
            last_day: self.head.last_day,
            ended_unix: self.head.ended_unix,
            placements,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlayerPlacement {
    pub category: Category,
    pub rank: u32,
    pub value: f64,
    /// Players ranked in the category.
    pub of: usize,
}

/// One season of a player's history.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlayerSeason {
    pub world_id: String,
    pub season: u32,
    pub first_day: u64,
    pub last_day: u64,
    pub ended_unix: u64,
    /// In [`Category::ALL`] order.
    pub placements: Vec<PlayerPlacement>,
}

/// `GET /api/seasons`: finished seasons, newest first.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SeasonList {
    /// Finished seasons stored, all worlds.
    pub total: usize,
    pub offset: usize,
    pub seasons: Vec<SeasonHead>,
}

/// One page of a category's standings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StandingPage {
    pub category: Category,
    /// Ranked players in the category.
    pub total: usize,
    /// `offset..offset + limit` of the rows, plus the asked-for player's
    /// row if it falls outside them.
    pub rows: Vec<Placement>,
}

/// `GET /api/seasons/:n`: a finished season with paged standings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SeasonPage {
    #[serde(flatten)]
    pub head: SeasonHead,
    pub offset: usize,
    pub limit: usize,
    pub standings: Vec<StandingPage>,
}

/// `GET /api/seasons/current`: the season under way, standings so far.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CurrentSeason {
    pub world_id: String,
    pub season: u32,
    pub first_day: u64,
    /// Last day the season will count.
    pub last_day: u64,
    pub length: u64,
    /// Daily snapshots counted so far.
    pub days: u32,
    pub days_left: u64,
    pub offset: usize,
    pub limit: usize,
    pub standings: Vec<StandingPage>,
}

/// `GET /api/seasons/player/:player`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlayerHistory {
    pub player: PlayerId,
    /// Newest first; only seasons the player was ranked in.
    pub seasons: Vec<PlayerSeason>,
}

/// The standings of the season under way (saved with the game).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SeasonTracker {
    pub season: u32,
    pub first_day: u64,
    /// First day of the next season.
    pub end_day: u64,
    /// Daily snapshots counted.
    pub days: u32,
    /// The last day counted: a day is never counted twice.
    pub last_day: Option<u64>,
    legacy_sum: BTreeMap<PlayerId, f64>,
    final_share: BTreeMap<PlayerId, f64>,
    peak_alive: BTreeMap<PlayerId, u64>,
    species: BTreeMap<PlayerId, u64>,
    sales: BTreeMap<PlayerId, u64>,
    survived: BTreeMap<PlayerId, u64>,
    /// Bumped by every change to the standings (not saved; see
    /// [`Generation`]).
    #[serde(skip)]
    generation: Generation,
}

/// A change counter that is not part of the value it counts changes of: it
/// compares equal always and is not saved. A restored tracker starts again
/// from 0, which is fine for its one use, telling the world thread's
/// standings cache (`crate::standings`) that the standings changed since it
/// last looked (the cache starts empty with the process too).
#[derive(Clone, Copy, Debug, Default)]
pub struct Generation(pub u64);

impl PartialEq for Generation {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Generation {
    fn bump(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}

impl SeasonTracker {
    pub fn new(season: u32, first_day: u64, length: u64) -> Self {
        Self { season, first_day, end_day: first_day + length.max(1), ..Default::default() }
    }

    /// Changes to the standings so far (see [`Generation`]); with the
    /// season number, what identifies the standings' current state.
    pub fn generation(&self) -> u64 {
        self.generation.0
    }

    pub fn length(&self) -> u64 {
        self.end_day.saturating_sub(self.first_day)
    }

    /// Whether the daily record of `day` belongs to a later season.
    pub fn is_over_by(&self, day: u64) -> bool {
        day >= self.end_day
    }

    /// The season after this one, of `length` days, starting where this one
    /// ends, or at `day` if that is later still (no records in between).
    pub fn next(&self, day: u64, length: u64) -> Self {
        let length = length.max(1);
        let first = if day >= self.end_day + length { day } else { self.end_day };
        // The generation carries on, so it alone also tells seasons apart.
        let mut generation = self.generation;
        generation.bump();
        Self { generation, ..Self::new(self.season + 1, first, length) }
    }

    /// Count a daily record (once; an earlier or repeated day is ignored).
    pub fn record_day(&mut self, r: &DailyRecord) {
        if self.last_day.is_some_and(|d| r.day <= d) || r.day < self.first_day {
            return;
        }
        self.last_day = Some(r.day);
        self.days += 1;
        self.generation.bump();
        for v in self.final_share.values_mut() {
            *v = 0.0;
        }
        for e in r.legacy.iter().filter(|e| e.player != WILD) {
            *self.legacy_sum.entry(e.player).or_default() += e.share;
            self.final_share.insert(e.player, e.share);
        }
        let mut alive: BTreeMap<PlayerId, u64> = BTreeMap::new();
        for l in r.lineages.iter().filter(|l| l.owner != WILD) {
            *alive.entry(l.owner).or_default() += l.alive as u64;
        }
        for (p, n) in alive {
            let peak = self.peak_alive.entry(p).or_default();
            *peak = (*peak).max(n);
        }
    }

    pub fn add_species(&mut self, player: PlayerId) {
        if player != WILD {
            *self.species.entry(player).or_default() += 1;
            self.generation.bump();
        }
    }

    pub fn add_sale(&mut self, player: PlayerId, renown: u64) {
        if player != WILD && renown > 0 {
            *self.sales.entry(player).or_default() += renown;
            self.generation.bump();
        }
    }

    pub fn add_survival(&mut self, player: PlayerId) {
        if player != WILD {
            *self.survived.entry(player).or_default() += 1;
            self.generation.bump();
        }
    }

    /// Standings so far, every category in [`Category::ALL`] order.
    pub fn standings(&self, name_of: impl Fn(PlayerId) -> String) -> Vec<Standing> {
        let days = self.days.max(1) as f64;
        let count = |m: &BTreeMap<PlayerId, u64>| m.iter().map(|(&p, &v)| (p, v as f64, 0.0)).collect::<Vec<_>>();
        Category::ALL
            .into_iter()
            .map(|category| {
                let values: Vec<(PlayerId, f64, f64)> = match category {
                    Category::Legacy => self
                        .legacy_sum
                        .iter()
                        .map(|(&p, &s)| (p, s / days, self.final_share.get(&p).copied().unwrap_or(0.0)))
                        .collect(),
                    Category::Lineage => count(&self.peak_alive),
                    Category::Species => count(&self.species),
                    Category::Market => count(&self.sales),
                    Category::Survival => count(&self.survived),
                };
                Standing { category, rows: rank(values, &name_of) }
            })
            .collect()
    }

    /// Freeze the season into its record.
    pub fn finish(
        &self,
        world_id: &str,
        ended_tick: u64,
        ended_unix: u64,
        name_of: impl Fn(PlayerId) -> String,
    ) -> SeasonRecord {
        let standings = self.standings(name_of);
        let winners: Vec<Winner> = standings
            .iter()
            .flat_map(|s| {
                s.rows.iter().take_while(|r| r.rank == 1).map(|r| Winner {
                    category: s.category,
                    player: r.player,
                    name: r.name.clone(),
                    value: r.value,
                })
            })
            .collect();
        let mut players: Vec<PlayerId> = standings.iter().flat_map(|s| s.rows.iter().map(|r| r.player)).collect();
        players.sort_unstable();
        players.dedup();
        let last_day = self.end_day.saturating_sub(1).max(self.first_day);
        let head = SeasonHead {
            world_id: world_id.to_string(),
            season: self.season,
            first_day: self.first_day,
            last_day,
            length: self.length(),
            days: self.days,
            ended_tick,
            ended_unix,
            players: players.len(),
            summary: summary(self.season, self.first_day, last_day, &winners),
            winners,
        };
        SeasonRecord { head, standings }
    }
}

/// What the standings of the season under way were computed from
/// (`Game::live_season_key`): when it changes, they are recomputed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveKey {
    pub season: u32,
    /// [`SeasonTracker::generation`].
    pub generation: u64,
    pub days_left: u64,
    /// Bumped when a player's name changes (names are in the rows).
    pub names: u64,
}

/// The season under way with its full standings, computed once per change
/// ([`LiveKey`]) on the world thread and shared (`Arc`) by every request
/// and push that pages it (`crate::standings`).
#[derive(Clone, Debug, Default)]
pub struct LiveSeason {
    pub key: LiveKey,
    pub world_id: String,
    pub season: u32,
    pub first_day: u64,
    pub last_day: u64,
    pub length: u64,
    pub days: u32,
    pub days_left: u64,
    /// Every category in [`Category::ALL`] order, all rows.
    pub standings: Vec<Standing>,
    /// Per category: player → row index, for a player's own row.
    index: Vec<HashMap<PlayerId, usize>>,
}

impl LiveSeason {
    pub fn new(key: LiveKey, world_id: &str, t: &SeasonTracker, standings: Vec<Standing>) -> Self {
        let index = standings.iter().map(|s| s.rows.iter().enumerate().map(|(k, r)| (r.player, k)).collect()).collect();
        LiveSeason {
            key,
            world_id: world_id.to_string(),
            season: t.season,
            first_day: t.first_day,
            last_day: t.end_day.saturating_sub(1),
            length: t.length(),
            days: t.days,
            days_left: key.days_left,
            standings,
            index,
        }
    }

    /// Rows `offset..offset + limit` of each category plus `player`'s row:
    /// what [`page`] gives, without scanning for the player.
    pub fn page(&self, offset: usize, limit: usize, player: Option<PlayerId>) -> CurrentSeason {
        let standings = self
            .standings
            .iter()
            .zip(&self.index)
            .map(|(s, index)| {
                let mut rows: Vec<Placement> = s.rows.iter().skip(offset).take(limit).cloned().collect();
                if let Some(&k) = player.and_then(|p| index.get(&p)) {
                    if k < offset || k >= offset.saturating_add(limit) {
                        rows.push(s.rows[k].clone());
                    }
                }
                StandingPage { category: s.category, total: s.rows.len(), rows }
            })
            .collect();
        CurrentSeason {
            world_id: self.world_id.clone(),
            season: self.season,
            first_day: self.first_day,
            last_day: self.last_day,
            length: self.length,
            days: self.days,
            days_left: self.days_left,
            offset,
            limit,
            standings,
        }
    }
}

/// Rank `(player, value, tiebreak)` best first: competition ranks over
/// equal (value, tiebreak), players with nothing (value 0) left out.
fn rank(mut values: Vec<(PlayerId, f64, f64)>, name_of: &impl Fn(PlayerId) -> String) -> Vec<Placement> {
    values.retain(|v| v.1 > 0.0 && v.1.is_finite());
    values.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.2.total_cmp(&a.2)).then(a.0.cmp(&b.0)));
    let mut rows: Vec<Placement> = Vec::with_capacity(values.len());
    for (k, &(player, value, tie)) in values.iter().enumerate() {
        let rank = match (k.checked_sub(1).map(|j| values[j]), rows.last()) {
            (Some((_, v, t)), Some(prev)) if v == value && t == tie => prev.rank,
            _ => k as u32 + 1,
        };
        rows.push(Placement { player, name: name_of(player), rank, value });
    }
    rows
}

/// "Season 2 (days 61–120): Ana won the legacy crown with 12.3% mean share.
/// Largest lineage: Bo (412 organisms). …" (days shown 1-based, as the
/// client does).
fn summary(season: u32, first_day: u64, last_day: u64, winners: &[Winner]) -> String {
    let mut text = format!("Season {season} (days {}–{}): ", first_day + 1, last_day + 1);
    let names = |c: Category| -> Option<(String, f64)> {
        let w: Vec<&Winner> = winners.iter().filter(|w| w.category == c).collect();
        let first = w.first()?;
        let who = match w.len() {
            1 => first.name.clone(),
            2 => format!("{} and {}", w[0].name, w[1].name),
            n => format!("{} and {} others", first.name, n - 1),
        };
        Some((who, first.value))
    };
    match names(Category::Legacy) {
        Some((who, v)) => {
            text += &format!("{who} won the legacy crown with {}.", Category::Legacy.describe(v));
        }
        None => text += "no player alleles were among the breeding adults.",
    }
    for c in &Category::ALL[1..] {
        if let Some((who, v)) = names(*c) {
            let title = c.title();
            let mut t = title.chars();
            let title = t.next().map(|f| f.to_uppercase().chain(t).collect::<String>()).unwrap_or_default();
            text += &format!(" {title}: {who} ({}).", c.describe(v));
        }
    }
    text
}

/// Rows `offset..offset + limit` of each standing, plus `player`'s row when
/// it falls outside them.
pub fn page(standings: &[Standing], offset: usize, limit: usize, player: Option<PlayerId>) -> Vec<StandingPage> {
    standings
        .iter()
        .map(|s| {
            let mut rows: Vec<Placement> = s.rows.iter().skip(offset).take(limit).cloned().collect();
            if let Some(p) = player {
                if !rows.iter().any(|r| r.player == p) {
                    if let Some(r) = s.rows.iter().find(|r| r.player == p) {
                        rows.push(r.clone());
                    }
                }
            }
            StandingPage { category: s.category, total: s.rows.len(), rows }
        })
        .collect()
}

/// The paging a request asked for: offset, and a limit within
/// `1..=MAX_ROWS` (default [`DEFAULT_ROWS`]).
pub fn paging(offset: Option<usize>, limit: Option<usize>) -> (usize, usize) {
    (offset.unwrap_or(0), limit.unwrap_or(DEFAULT_ROWS).clamp(1, MAX_ROWS))
}

/// `records` (write order, oldest first) as a list page, newest first.
pub fn list_page(records: &[SeasonRecord], offset: usize, limit: usize) -> (Vec<SeasonHead>, usize) {
    let heads = records.iter().rev().skip(offset).take(limit).map(|r| r.head.clone()).collect();
    (heads, records.len())
}

/// A player's seasons in `records` (write order), newest first.
pub fn player_history(records: &[SeasonRecord], player: PlayerId, limit: usize) -> Vec<PlayerSeason> {
    records.iter().rev().filter_map(|r| r.player_season(player)).take(limit).collect()
}

/// Insert or replace `record` in `records` (write order): a replaced
/// record keeps its place.
pub fn upsert(records: &mut Vec<SeasonRecord>, record: &SeasonRecord) {
    match records.iter_mut().find(|r| r.key() == record.key()) {
        Some(r) => *r = record.clone(),
        None => records.push(record.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{LegacyEntry, LineageEntry};

    fn day(d: u64, shares: &[(PlayerId, f64)], alive: &[(PlayerId, usize)]) -> DailyRecord {
        DailyRecord {
            world_id: "w".into(),
            day: d,
            year: 0,
            season: "spring".into(),
            breeding_adults: 100,
            legacy: shares
                .iter()
                .map(|&(player, share)| LegacyEntry { player, share, weighted_share: share })
                .collect(),
            lineages: alive
                .iter()
                .enumerate()
                .map(|(k, &(owner, alive))| LineageEntry {
                    lineage: k as u32,
                    name: format!("l{k}"),
                    owner,
                    alive,
                    adults: alive / 2,
                    born: 0,
                    deaths: Default::default(),
                    genes: Default::default(),
                })
                .collect(),
        }
    }

    fn name(p: PlayerId) -> String {
        format!("p{p}")
    }

    #[test]
    fn ranks_every_category_and_names_the_winners() {
        let mut t = SeasonTracker::new(1, 0, 3);
        t.record_day(&day(0, &[(WILD, 0.5), (1, 0.25), (2, 0.25)], &[(WILD, 500), (1, 40), (2, 30)]));
        t.record_day(&day(1, &[(WILD, 0.375), (1, 0.5), (2, 0.125)], &[(1, 50), (2, 60)]));
        // Player 3 appears on the last day only: a missing day counts 0.
        t.record_day(&day(2, &[(1, 0.125), (2, 0.25), (3, 0.625)], &[(1, 45), (2, 10), (3, 5)]));
        t.add_species(2);
        t.add_species(WILD);
        t.add_sale(1, 30);
        t.add_sale(2, 30);
        t.add_survival(3);
        let r = t.finish("w", 144, 1000, name);
        assert_eq!(r.head.days, 3);
        assert_eq!((r.head.first_day, r.head.last_day, r.head.length), (0, 2, 3));
        let legacy = &r.standings[0];
        assert_eq!(legacy.category, Category::Legacy);
        // 1: 0.875/3, 2: 0.625/3, 3: 0.625/3 (tie on the mean; 3 has the higher final share).
        assert_eq!(legacy.rows.iter().map(|r| (r.player, r.rank)).collect::<Vec<_>>(), vec![(1, 1), (3, 2), (2, 3)]);
        assert!((legacy.rows[0].value - 0.875 / 3.0).abs() < 1e-12);
        let lineage = &r.standings[1];
        assert_eq!(
            lineage.rows.iter().map(|r| (r.player, r.value)).collect::<Vec<_>>(),
            vec![(2, 60.0), (1, 50.0), (3, 5.0)]
        );
        assert_eq!(r.standings[2].rows.len(), 1, "the wild founds nothing");
        // A tie shares the place.
        let market = &r.standings[3];
        assert_eq!(market.rows.iter().map(|r| r.rank).collect::<Vec<_>>(), vec![1, 1]);
        let winners: Vec<(Category, PlayerId)> = r.head.winners.iter().map(|w| (w.category, w.player)).collect();
        assert_eq!(
            winners,
            vec![
                (Category::Legacy, 1),
                (Category::Lineage, 2),
                (Category::Species, 2),
                (Category::Market, 1),
                (Category::Market, 2),
                (Category::Survival, 3)
            ]
        );
        assert_eq!(r.head.players, 3);
        assert!(r.head.summary.starts_with("Season 1 (days 1–3): p1 won the legacy crown with 29.2% mean share."));
        assert!(r.head.summary.contains("Stud market: p1 and p2 (30 renown)."), "{}", r.head.summary);
        // The player view.
        let mine = r.player_season(3).unwrap();
        assert_eq!(
            mine.placements.iter().map(|p| (p.category, p.rank, p.of)).collect::<Vec<_>>(),
            vec![(Category::Legacy, 2, 3), (Category::Lineage, 3, 3), (Category::Survival, 1, 1)]
        );
        assert!(r.player_season(9).is_none());
    }

    /// Every score change bumps the generation (what the standings cache
    /// keys on); ignored days and the wild don't; the next season carries on
    /// from it. It is not saved and not part of equality.
    #[test]
    fn score_changes_and_rollovers_bump_the_generation() {
        let mut t = SeasonTracker::new(1, 0, 3);
        let mut seen = vec![t.generation()];
        let mut changed = |t: &SeasonTracker, expect: bool| {
            assert_eq!(t.generation() != *seen.last().unwrap(), expect, "generation {}", t.generation());
            seen.push(t.generation());
        };
        t.record_day(&day(0, &[(1, 0.5)], &[(1, 4)]));
        changed(&t, true);
        t.record_day(&day(0, &[(1, 0.9)], &[]));
        changed(&t, false);
        t.add_species(1);
        changed(&t, true);
        t.add_species(WILD);
        changed(&t, false);
        t.add_sale(2, 10);
        changed(&t, true);
        t.add_sale(2, 0);
        changed(&t, false);
        t.add_survival(3);
        changed(&t, true);
        let n = t.next(3, 3);
        changed(&n, true);
        let restored: SeasonTracker = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(restored.generation(), 0);
        assert_eq!(restored, t);
    }

    /// A live season's pages are [`page`]'s, own row included.
    #[test]
    fn live_pages_match_page() {
        let mut t = SeasonTracker::new(1, 0, 2);
        let shares: Vec<(PlayerId, f64)> = (1..=30).map(|p| (p, p as f64 / 1000.0)).collect();
        t.record_day(&day(0, &shares, &[(4, 9), (5, 3)]));
        t.add_sale(7, 5);
        let s = t.standings(name);
        let key = LiveKey { season: 1, generation: t.generation(), days_left: 1, names: 0 };
        let live = LiveSeason::new(key, "w", &t, s.clone());
        for (offset, limit, player) in
            [(0, 5, Some(1)), (5, 5, Some(28)), (0, 5, Some(30)), (0, 200, None), (40, 5, Some(2))]
        {
            let got = live.page(offset, limit, player);
            assert_eq!(got.standings, page(&s, offset, limit, player), "{offset} {limit} {player:?}");
            assert_eq!((got.season, got.days, got.days_left, got.last_day, got.offset), (1, 1, 1, 1, offset));
        }
    }

    #[test]
    fn a_day_counts_once() {
        let mut t = SeasonTracker::new(1, 10, 5);
        let d = day(11, &[(1, 0.5)], &[(1, 10)]);
        t.record_day(&d);
        t.record_day(&d);
        t.record_day(&day(9, &[(1, 0.9)], &[]));
        assert_eq!(t.days, 1);
        assert!((t.standings(name)[0].rows[0].value - 0.5).abs() < 1e-12);
    }

    #[test]
    fn seasons_chain_and_skip_gaps() {
        let t = SeasonTracker::new(1, 0, 10);
        assert!(!t.is_over_by(9) && t.is_over_by(10));
        let n = t.next(10, 10);
        assert_eq!((n.season, n.first_day, n.end_day), (2, 10, 20));
        // A changed length applies from the next season.
        let n = t.next(10, 4);
        assert_eq!((n.first_day, n.end_day), (10, 14));
        // Long past the end (no records in between): the next starts now.
        let n = t.next(35, 10);
        assert_eq!((n.season, n.first_day, n.end_day), (2, 35, 45));
    }

    #[test]
    fn an_empty_season_has_a_summary() {
        let r = SeasonTracker::new(4, 30, 10).finish("w", 0, 0, name);
        assert_eq!(r.head.summary, "Season 4 (days 31–40): no player alleles were among the breeding adults.");
        assert!(r.standings.iter().all(|s| s.rows.is_empty()) && r.head.winners.is_empty());
    }

    #[test]
    fn pages_keep_the_asked_for_player() {
        let mut t = SeasonTracker::new(1, 0, 2);
        let shares: Vec<(PlayerId, f64)> = (1..=30).map(|p| (p, p as f64 / 1000.0)).collect();
        t.record_day(&day(0, &shares, &[]));
        let s = t.standings(name);
        let p = page(&s, 0, 5, Some(1));
        assert_eq!(p[0].total, 30);
        assert_eq!(p[0].rows.iter().map(|r| r.player).collect::<Vec<_>>(), vec![30, 29, 28, 27, 26, 1]);
        let p = page(&s, 5, 5, Some(28));
        assert_eq!(p[0].rows.iter().map(|r| r.rank).collect::<Vec<_>>(), vec![6, 7, 8, 9, 10, 3]);
        assert_eq!(paging(None, Some(10_000)), (0, MAX_ROWS));
        assert_eq!(paging(Some(3), Some(0)), (3, 1));
    }

    #[test]
    fn records_round_trip_as_json_and_upsert_in_place() {
        let mut t = SeasonTracker::new(1, 0, 2);
        t.record_day(&day(0, &[(1, 0.5)], &[(1, 3)]));
        let a = t.finish("w", 1, 2, name);
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<SeasonRecord>(&json).unwrap(), a);
        let tracker: SeasonTracker = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(tracker, t);
        let mut b = SeasonTracker::new(2, 2, 2).finish("w", 3, 4, name);
        let mut list = vec![a.clone()];
        upsert(&mut list, &b);
        let mut a2 = a.clone();
        a2.head.ended_unix = 99;
        upsert(&mut list, &a2);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].head.ended_unix, 99);
        let (heads, total) = list_page(&list, 0, 1);
        assert_eq!((heads[0].season, total), (2, 2));
        b.head.season = 3;
        assert_eq!(player_history(&[a, b], 1, 10).len(), 1);
    }
}
