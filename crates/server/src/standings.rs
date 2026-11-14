//! Caches for standings reads, so a crowd of clients asking for the same
//! leaderboards costs the store (and the CPU) once per change, not once per
//! request.
//!
//! Three kinds of standings are read:
//!
//! - **The ranked season under way** (`/api/seasons/current`, and the
//!   `season` push to WebSockets that sent `watch_season`): computed on the
//!   world thread only when the standings changed (`Game::live_season_key`:
//!   the tracker's generation, bumped by every counted day, species, sale
//!   and survival; the season number, so a rollover always changes it; the
//!   days left; and the player names), and published as an
//!   `Arc<league::LiveSeason>` next to the summary. Requests and pushes
//!   page that shared copy; they never reach the world thread.
//! - **Finished ranked seasons** (`/api/seasons`, `/api/seasons/:n`,
//!   `/api/seasons/player/:p`): a finished record only changes when this
//!   process writes it (a rollover, or a replayed one after a restart), and
//!   every write goes through [`crate::store::WorldStore::put_season`],
//!   which invalidates this world's cache under the same lock that loads
//!   take. So these are cached until written: no expiry, never stale.
//! - **Weather-season boards** (`/api/leaderboard/seasons`, from the whole
//!   daily history): cached per (daily-history generation, current year and
//!   season); [`crate::store::WorldStore::append_daily`] bumps the
//!   generation.
//!
//! Event-driven invalidation instead of a short expiry: the events are all
//! in this process and few (a season record per rollover, a daily record
//! per simulated day), so the cache is exact and a steady stream of readers
//! never reaches the store. An expiry would serve stale standings right
//! after a rollover (when players look) and still send a burst of misses to
//! the store every interval.
//!
//! The cache belongs to one world's [`crate::store::WorldStore`] (clones
//! share it; the HTTP routes and the world's persist task use clones of the
//! same one), so worlds never see each other's entries.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use evo_sim::PlayerId;

use crate::league::{PlayerSeason, SeasonHead, SeasonRecord};
use crate::protocol::SeasonBoard;

/// Entries kept per map before a map is emptied (they are small, and
/// refill on demand).
const MAX_ENTRIES: usize = 1024;

/// A page of the season list: heads and the total.
pub type ListPage = Arc<(Vec<SeasonHead>, usize)>;

/// What the weather-season boards were computed from: the daily-history
/// generation and the current (year, season).
pub type WeatherKey = (u64, u64, String);

#[derive(Default)]
struct Inner {
    /// `None`: the store has no such season (yet).
    records: HashMap<(String, u32), Option<Arc<SeasonRecord>>>,
    lists: HashMap<(usize, usize), ListPage>,
    players: HashMap<(PlayerId, usize), Arc<Vec<PlayerSeason>>>,
    daily_gen: u64,
    weather: Option<(WeatherKey, Arc<Vec<SeasonBoard>>)>,
}

/// One world's standings cache (see the module docs).
#[derive(Default)]
pub struct StandingsCache {
    inner: Mutex<Inner>,
    /// Held while filling a miss and while writing a season, so a load that
    /// started before a write can never put its stale result in after it,
    /// and a crowd missing the same entry loads it once.
    fill: tokio::sync::Mutex<()>,
    hits: AtomicU64,
    misses: AtomicU64,
}

fn bounded<K: std::hash::Hash + Eq, V>(map: &mut HashMap<K, V>, key: K, value: V) {
    if map.len() >= MAX_ENTRIES {
        map.clear();
    }
    map.insert(key, value);
}

impl StandingsCache {
    /// Reads answered from the cache, and reads that went to the store.
    pub fn counts(&self) -> (u64, u64) {
        (self.hits.load(Ordering::Relaxed), self.misses.load(Ordering::Relaxed))
    }

    /// The cached value, or `load`'s (stored with `put`). Misses load one at
    /// a time and check again first, so concurrent misses load once.
    async fn get_or_load<T, E, Fut>(
        &self,
        get: impl Fn(&Inner) -> Option<T>,
        load: impl FnOnce() -> Fut,
        put: impl FnOnce(&mut Inner, T),
    ) -> Result<T, E>
    where
        T: Clone,
        Fut: Future<Output = Result<T, E>>,
    {
        if let Some(v) = get(&self.inner.lock().unwrap()) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(v);
        }
        let _fill = self.fill.lock().await;
        if let Some(v) = get(&self.inner.lock().unwrap()) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(v);
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        let v = load().await?;
        put(&mut self.inner.lock().unwrap(), v.clone());
        Ok(v)
    }

    pub async fn record<E, Fut>(
        &self,
        map: &str,
        season: u32,
        load: impl FnOnce() -> Fut,
    ) -> Result<Option<Arc<SeasonRecord>>, E>
    where
        Fut: Future<Output = Result<Option<SeasonRecord>, E>>,
    {
        let key = (map.to_string(), season);
        self.get_or_load(
            |i| i.records.get(&key).cloned(),
            || async { load().await.map(|r| r.map(Arc::new)) },
            |i, v| bounded(&mut i.records, key.clone(), v),
        )
        .await
    }

    pub async fn list<E, Fut>(&self, offset: usize, limit: usize, load: impl FnOnce() -> Fut) -> Result<ListPage, E>
    where
        Fut: Future<Output = Result<(Vec<SeasonHead>, usize), E>>,
    {
        self.get_or_load(
            |i| i.lists.get(&(offset, limit)).cloned(),
            || async { load().await.map(Arc::new) },
            |i, v| bounded(&mut i.lists, (offset, limit), v),
        )
        .await
    }

    pub async fn player<E, Fut>(
        &self,
        player: PlayerId,
        limit: usize,
        load: impl FnOnce() -> Fut,
    ) -> Result<Arc<Vec<PlayerSeason>>, E>
    where
        Fut: Future<Output = Result<Vec<PlayerSeason>, E>>,
    {
        self.get_or_load(
            |i| i.players.get(&(player, limit)).cloned(),
            || async { load().await.map(Arc::new) },
            |i, v| bounded(&mut i.players, (player, limit), v),
        )
        .await
    }

    /// Write a season record with `write`, then drop what it makes stale:
    /// every list and player history, and the record's own entry (replaced
    /// by the record when the write succeeded; dropped when it failed, as
    /// the store may or may not hold it).
    pub async fn write_season<E, Fut>(&self, record: &SeasonRecord, write: impl FnOnce() -> Fut) -> Result<(), E>
    where
        Fut: Future<Output = Result<(), E>>,
    {
        let _fill = self.fill.lock().await;
        let result = write().await;
        let mut i = self.inner.lock().unwrap();
        i.lists.clear();
        i.players.clear();
        let key = (record.head.world_id.clone(), record.head.season);
        match &result {
            Ok(()) => bounded(&mut i.records, key, Some(Arc::new(record.clone()))),
            Err(_) => {
                i.records.remove(&key);
            }
        }
        result
    }

    /// The daily history grew: the weather-season boards are stale.
    pub fn daily_appended(&self) {
        let mut i = self.inner.lock().unwrap();
        i.daily_gen += 1;
        i.weather = None;
    }

    /// The weather-season boards for the current `(year, season)`, computed
    /// by `load` when the history or the season changed since.
    pub async fn weather<E, Fut>(
        &self,
        year: u64,
        season: &str,
        load: impl FnOnce() -> Fut,
    ) -> Result<Arc<Vec<SeasonBoard>>, E>
    where
        Fut: Future<Output = Result<Vec<SeasonBoard>, E>>,
    {
        let gen = self.inner.lock().unwrap().daily_gen;
        let key: WeatherKey = (gen, year, season.to_string());
        let get = |i: &Inner| match &i.weather {
            Some((k, v)) if *k == key && i.daily_gen == gen => Some(v.clone()),
            _ => None,
        };
        self.get_or_load(
            get,
            || async { load().await.map(Arc::new) },
            |i, v| {
                // A record appended while this loaded: don't keep it.
                if i.daily_gen == gen {
                    i.weather = Some((key.clone(), v));
                }
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::league::SeasonTracker;
    use crate::store::{DailyRecord, MemoryStore, Store, WorldStore};

    fn record(map: &str, season: u32, ended_unix: u64) -> SeasonRecord {
        SeasonTracker::new(season, 0, 2).finish(map, 0, ended_unix, |p| format!("p{p}"))
    }

    fn daily(day: u64) -> DailyRecord {
        DailyRecord {
            world_id: "m".into(),
            day,
            year: 0,
            season: "spring".into(),
            breeding_adults: 1,
            legacy: vec![],
            lineages: vec![],
        }
    }

    fn board() -> SeasonBoard {
        SeasonBoard {
            year: 0,
            season: "spring".into(),
            first_day: 0,
            last_day: 0,
            days: 1,
            current: true,
            rows: vec![],
        }
    }

    /// Reads are answered from the cache until a season write through the
    /// world's store (a rollover) invalidates them, the "not ended" answer
    /// included; another world's cache is not touched.
    #[tokio::test]
    async fn season_writes_invalidate_and_worlds_stay_apart() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
        let main = WorldStore::new(store.clone(), "main");
        let isle = WorldStore::new(store.clone(), "isle");
        let clone = main.clone();
        assert_eq!(main.list_seasons(0, 10).await.unwrap().1, 0);
        assert!(main.load_season("m", 1).await.unwrap().is_none());
        assert!(main.player_seasons(1, 10).await.unwrap().is_empty());
        assert_eq!(main.cache().counts(), (0, 3));
        // Repeated reads, from any clone, are hits.
        assert_eq!(clone.list_seasons(0, 10).await.unwrap().1, 0);
        assert!(clone.load_season("m", 1).await.unwrap().is_none());
        assert_eq!(main.cache().counts(), (2, 3));
        // The isle reads its own (same map id and season number).
        assert!(isle.load_season("m", 1).await.unwrap().is_none());
        assert_eq!(isle.cache().counts(), (0, 1));

        // Rollover in main (written by the persist task's clone).
        let one = record("m", 1, 10);
        clone.put_season(&one).await.unwrap();
        assert_eq!(main.load_season("m", 1).await.unwrap(), Some(one.clone()), "the write replaced the entry");
        let (heads, total) = main.list_seasons(0, 10).await.unwrap();
        assert_eq!((heads.len(), total), (1, 1));
        assert_eq!(main.player_seasons(1, 10).await.unwrap().len(), 0, "nobody ranked in an empty season");
        assert!(isle.load_season("m", 1).await.unwrap().is_none(), "not in the other world");
        assert_eq!(isle.list_seasons(0, 10).await.unwrap().1, 0);

        // A replayed rollover replaces the record: no stale copy is served.
        main.put_season(&record("m", 1, 99)).await.unwrap();
        assert_eq!(main.season_record("m", 1).await.unwrap().unwrap().head.ended_unix, 99);
        assert_eq!(main.list_seasons(0, 10).await.unwrap().0[0].ended_unix, 99);
        // The isle's own rollover, same key, its own record.
        isle.put_season(&record("m", 1, 7)).await.unwrap();
        assert_eq!(isle.load_season("m", 1).await.unwrap().unwrap().head.ended_unix, 7);
        assert_eq!(main.load_season("m", 1).await.unwrap().unwrap().head.ended_unix, 99);
        assert_eq!(main.list_seasons(0, 10).await.unwrap().1, 1);
    }

    /// A load that started before a write can't leave its stale answer in
    /// the cache after it: loads and writes take the same lock.
    #[tokio::test]
    async fn a_load_racing_a_write_is_not_kept() {
        let cache = Arc::new(StandingsCache::default());
        let (release, wait) = tokio::sync::oneshot::channel::<()>();
        let c = cache.clone();
        let load = tokio::spawn(async move {
            c.record("m", 1, || async {
                wait.await.unwrap();
                Ok::<_, ()>(None)
            })
            .await
        });
        tokio::task::yield_now().await;
        let c = cache.clone();
        let one = record("m", 1, 1);
        let r = one.clone();
        let write = tokio::spawn(async move { c.write_season(&r, || async { Ok::<_, ()>(()) }).await });
        tokio::task::yield_now().await;
        release.send(()).unwrap();
        assert_eq!(load.await.unwrap(), Ok(None));
        write.await.unwrap().unwrap();
        let got = cache.record("m", 1, || async { Err::<Option<SeasonRecord>, ()>(()) }).await.unwrap();
        assert_eq!(got.as_deref(), Some(&one));
        // A failed write drops the entry (the store may or may not have it).
        cache.write_season(&record("m", 1, 2), || async { Err::<(), ()>(()) }).await.unwrap_err();
        let reload = cache.record("m", 1, || async { Ok::<_, ()>(Some(record("m", 1, 3))) }).await.unwrap();
        assert_eq!(reload.unwrap().head.ended_unix, 3);
    }

    /// Weather boards are kept until the history grows or the weather
    /// season changes.
    #[tokio::test]
    async fn weather_boards_follow_daily_records_and_the_season() {
        let store = WorldStore::new(Arc::new(MemoryStore::default()), "main");
        let loads = AtomicU64::new(0);
        let boards = |n: usize| {
            let loads = &loads;
            move || async move {
                loads.fetch_add(1, Ordering::Relaxed);
                Ok::<_, ()>(vec![board(); n])
            }
        };
        let c = store.cache();
        assert_eq!(c.weather(0, "spring", boards(1)).await.unwrap().len(), 1);
        assert_eq!(c.weather(0, "spring", boards(2)).await.unwrap().len(), 1, "cached");
        store.append_daily(&daily(0)).await.unwrap();
        assert_eq!(c.weather(0, "spring", boards(2)).await.unwrap().len(), 2, "a new day");
        assert_eq!(c.weather(0, "summer", boards(3)).await.unwrap().len(), 3, "a new season");
        assert_eq!(c.weather(0, "summer", boards(4)).await.unwrap().len(), 3);
        assert_eq!(loads.load(Ordering::Relaxed), 3);
    }
}
