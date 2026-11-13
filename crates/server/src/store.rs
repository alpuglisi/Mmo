//! Persistence behind the [`Store`] trait.
//!
//! The design doc puts accounts, lineage history and world snapshots in
//! PostgreSQL (sqlx): `PgStore` (cargo feature `postgres`, on by default;
//! `--database-url`) keeps them in one table per record type (`accounts`,
//! `designs`, `daily_history`, `trades`, `snapshots`, `terrains`,
//! `balances`, `ledger`, `alert_prefs`, `seasons`, `season_placements`, `auctions`). [`FileStore`] (the default) keeps the same records as JSON
//! files in a data directory, and [`MemoryStore`] serves tests.
//!
//! What is persisted: accounts (name, player id, argon2 hash), founder
//! designs (re-founded into the new world on restart), one history record per
//! simulated day (legacy shares and lineage counts), directed-mating
//! agreements with their price, listing and escrow settlement, renown
//! balances and the ledger (`crate::economy`), how each marketplace auction
//! ended (open auctions and their bids live in the saved game), players' alert preferences
//! (which alert kinds they see and get toasts for), finished ranked seasons
//! (`crate::league`: final standings per category, winners, summary), and saved games (a world snapshot with the
//! game state around it, `crate::snapshot`, which includes players' alerts;
//! the newest [`KEEP_SNAPSHOTS`] are kept) and, once per world, its static
//! terrain, which saved games name by world id and content hash instead of
//! carrying it. Not persisted: login sessions (clients log in again after a
//! restart).
//!
//! Accounts and alert preferences are global (one account plays in every
//! world); everything else belongs to one configured world (`shard`, see
//! [`Store`] and `crate::worlds`), so each world has its own designs,
//! history, trades, saved games, terrains, balances, ledger, auction results and ranked seasons. Data
//! written before there were several worlds belongs to the default world
//! (`main`): `FileStore::open` moves its files, and the PostgreSQL migration
//! `0007_worlds.sql` fills the new `shard` columns with `'main'`.
//!
//! Daily records carry the id of the generated map that wrote them; a world
//! that replays days after a restart from an older saved game replaces its
//! earlier records of those days (one record per map and day).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use evo_sim::PlayerId;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::design::DesignSpec;
use crate::league::{self, PlayerSeason, SeasonHead, SeasonRecord};
use crate::protocol::{AlertPrefs, LedgerEntry};

#[cfg(feature = "postgres")]
mod pg;
#[cfg(feature = "postgres")]
pub use pg::PgStore;

#[derive(Debug)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "store: {}", self.0)
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError(e.to_string())
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError(e.to_string())
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AccountRecord {
    pub name: String,
    pub player: PlayerId,
    /// PHC string from argon2.
    pub password_hash: String,
    pub created_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DesignRecord {
    pub player: PlayerId,
    pub design: DesignSpec,
    pub founded_day: u64,
    pub founded_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LegacyEntry {
    pub player: PlayerId,
    pub share: f64,
    pub weighted_share: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LineageEntry {
    pub lineage: u32,
    pub name: String,
    pub owner: PlayerId,
    pub alive: usize,
    pub adults: usize,
    /// Births during the day (records before this field read as 0).
    #[serde(default)]
    pub born: u64,
    /// Deaths during the day by cause (`death_name`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub deaths: BTreeMap<String, u64>,
    /// Mean expressed value of every gene over living members, by wire
    /// name. Player lineages only (the genetics dashboard's drift charts).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub genes: BTreeMap<String, f32>,
}

/// One simulated day: the legacy leaderboard and every lineage's size.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DailyRecord {
    /// The world (one per generated map, kept across restarts) that wrote
    /// the record; empty in records from before world ids.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub world_id: String,
    pub day: u64,
    pub year: u64,
    pub season: String,
    pub breeding_adults: usize,
    pub legacy: Vec<LegacyEntry>,
    pub lineages: Vec<LineageEntry>,
}

/// A directed-mating agreement between two players that reached an end.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TradeRecord {
    pub offer: u64,
    pub from_player: PlayerId,
    pub to_player: PlayerId,
    pub mine: u64,
    pub partner: u64,
    pub price: Option<u64>,
    pub status: String,
    pub conceived: Option<bool>,
    pub day: u64,
    /// The marketplace listing it was bought from (records before the
    /// economy read as none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing: Option<u64>,
    /// What happened to the escrowed price: `paid` (to the partner's owner)
    /// or `refunded`; none when nothing was held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settlement: Option<String>,
}

/// A player's renown balance (upserted on every change).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BalanceRecord {
    pub player: PlayerId,
    pub balance: u64,
    /// Simulated day of the last change.
    pub day: u64,
}

/// A ledger entry of one world. `(world_id, entry.seq)` identifies it: a
/// world replaying days after a restart from an older saved game writes the
/// same sequence numbers again, replacing the earlier entries.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LedgerRecord {
    pub world_id: String,
    #[serde(flatten)]
    pub entry: LedgerEntry,
}

/// How a marketplace auction ended. `(world_id, listing)` identifies it: a
/// world replaying days after a restart from an older saved game may close
/// the same auction again, replacing the earlier record.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AuctionRecord {
    pub world_id: String,
    pub listing: u64,
    pub seller: PlayerId,
    pub organism: u64,
    pub reserve: u64,
    pub deadline_tick: u64,
    /// Bids placed.
    pub bids: u32,
    /// The highest bid when it closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_bid: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<PlayerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_organism: Option<u64>,
    /// `sold` (the winning bid became agreement `offer`), `unsold` (no bid),
    /// `failed` (the winner's organism could no longer mate with the lot:
    /// refunded), `ended` (the lot died: refunded) or `withdrawn` (before any
    /// bid).
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offer: Option<u64>,
    /// Simulated day it closed.
    pub day: u64,
}

fn same_auction(a: &AuctionRecord, b: &AuctionRecord) -> bool {
    a.world_id == b.world_id && a.listing == b.listing
}

/// A player's alert preferences (upserted when they change them).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AlertPrefsRecord {
    pub player: PlayerId,
    #[serde(flatten)]
    pub prefs: AlertPrefs,
}

/// Persistence for every world of the server. Accounts and alert
/// preferences are global; every other record belongs to one world, named
/// by `shard`: the configured world id (`crate::worlds`, `main` for a
/// single-world server). Records of one shard are never seen through
/// another. [`WorldStore`] binds a shard for code that serves one world.
#[async_trait]
pub trait Store: Send + Sync {
    async fn load_accounts(&self) -> StoreResult<Vec<AccountRecord>>;
    /// Fails if the name is taken.
    async fn insert_account(&self, account: &AccountRecord) -> StoreResult<()>;
    /// Insert or replace a player's alert preferences.
    async fn put_alert_prefs(&self, record: &AlertPrefsRecord) -> StoreResult<()>;
    /// A player's alert preferences, if they ever saved any.
    async fn load_alert_prefs(&self, player: PlayerId) -> StoreResult<Option<AlertPrefsRecord>>;

    async fn load_designs(&self, shard: &str) -> StoreResult<Vec<DesignRecord>>;
    /// Insert or replace the player's design in the world.
    async fn put_design(&self, shard: &str, design: &DesignRecord) -> StoreResult<()>;
    /// Record a day; replaces an earlier record of the same world and day
    /// (a day replayed after a restart), which keeps its place in order.
    async fn append_daily(&self, shard: &str, record: &DailyRecord) -> StoreResult<()>;
    /// The most recent `limit` daily records, oldest first.
    async fn recent_daily(&self, shard: &str, limit: usize) -> StoreResult<Vec<DailyRecord>>;
    async fn append_trade(&self, shard: &str, record: &TradeRecord) -> StoreResult<()>;
    /// Every trade record, oldest first.
    async fn load_trades(&self, shard: &str) -> StoreResult<Vec<TradeRecord>>;
    /// Save a snapshot taken at `tick` on `day` (opaque bytes) and drop all
    /// but the world's newest [`KEEP_SNAPSHOTS`]. Replaces a snapshot of the
    /// same tick.
    async fn put_snapshot(&self, shard: &str, tick: u64, day: u64, bytes: &[u8]) -> StoreResult<()>;
    /// Ticks of the world's stored snapshots, newest first.
    async fn snapshot_ticks(&self, shard: &str) -> StoreResult<Vec<u64>>;
    async fn load_snapshot(&self, shard: &str, tick: u64) -> StoreResult<Vec<u8>>;
    /// Delete every snapshot and stored terrain of the world (`--fresh`
    /// discards the saved world).
    async fn clear_snapshots(&self, shard: &str) -> StoreResult<()>;
    /// Store a world's static terrain (`crate::game::TerrainBlob`, opaque
    /// bytes) under its generated world id and content hash, unless the
    /// store has it already (the same key always holds the same bytes).
    /// Saved games name it; it is written before the first saved game that
    /// does.
    async fn put_terrain(&self, shard: &str, world_id: &str, hash: u64, bytes: &[u8]) -> StoreResult<()>;
    /// A stored terrain, `None` if the store doesn't have it.
    async fn load_terrain(&self, shard: &str, world_id: &str, hash: u64) -> StoreResult<Option<Vec<u8>>>;
    /// Insert or replace a player's balance in the world.
    async fn put_balance(&self, shard: &str, record: &BalanceRecord) -> StoreResult<()>;
    async fn load_balances(&self, shard: &str) -> StoreResult<Vec<BalanceRecord>>;
    /// Record a ledger entry; replaces an earlier entry with the same
    /// generated world id and sequence number, which keeps its place in
    /// order.
    async fn append_ledger(&self, shard: &str, record: &LedgerRecord) -> StoreResult<()>;
    /// The most recent `limit` ledger entries of the world (of one player,
    /// or of everyone), oldest first.
    async fn recent_ledger(
        &self,
        shard: &str,
        player: Option<PlayerId>,
        limit: usize,
    ) -> StoreResult<Vec<LedgerRecord>>;
    /// Record how one of the world's auctions ended; replaces an earlier
    /// record of the same generated map and listing (an auction closed again
    /// after a restart), which keeps its place in order.
    async fn append_auction(&self, shard: &str, record: &AuctionRecord) -> StoreResult<()>;
    /// The world's most recent `limit` auction records, oldest first.
    async fn recent_auctions(&self, shard: &str, limit: usize) -> StoreResult<Vec<AuctionRecord>>;
    /// Record a finished ranked season of the world; replaces an earlier
    /// record of the same generated map and season (a rollover replayed
    /// after a restart), which keeps its place in order.
    async fn put_season(&self, shard: &str, record: &SeasonRecord) -> StoreResult<()>;
    /// The world's finished seasons (of every map it had), newest first (in
    /// the order first written): `limit` from `offset`, and how many are
    /// stored.
    async fn list_seasons(&self, shard: &str, offset: usize, limit: usize) -> StoreResult<(Vec<SeasonHead>, usize)>;
    /// One finished season of the world's map `world_id`, with its
    /// standings.
    async fn load_season(&self, shard: &str, world_id: &str, season: u32) -> StoreResult<Option<SeasonRecord>>;
    /// The latest `limit` finished seasons of the world a player was ranked
    /// in, newest first.
    async fn player_seasons(&self, shard: &str, player: PlayerId, limit: usize) -> StoreResult<Vec<PlayerSeason>>;
}

/// One world's view of a [`Store`]: the per-world methods with the shard
/// bound. It derefs to the whole store, so the global records (accounts,
/// alert preferences) read and write through it too.
#[derive(Clone)]
pub struct WorldStore {
    store: Arc<dyn Store>,
    shard: Arc<str>,
}

impl std::ops::Deref for WorldStore {
    type Target = dyn Store;

    fn deref(&self) -> &(dyn Store + 'static) {
        &*self.store
    }
}

impl WorldStore {
    pub fn new(store: Arc<dyn Store>, shard: &str) -> Self {
        WorldStore { store, shard: shard.into() }
    }

    /// The world id this view is bound to.
    pub fn shard(&self) -> &str {
        &self.shard
    }

    /// The whole store (accounts, alert preferences, other worlds).
    pub fn global(&self) -> &Arc<dyn Store> {
        &self.store
    }

    pub async fn load_designs(&self) -> StoreResult<Vec<DesignRecord>> {
        self.store.load_designs(&self.shard).await
    }
    pub async fn put_design(&self, design: &DesignRecord) -> StoreResult<()> {
        self.store.put_design(&self.shard, design).await
    }
    pub async fn append_daily(&self, record: &DailyRecord) -> StoreResult<()> {
        self.store.append_daily(&self.shard, record).await
    }
    pub async fn recent_daily(&self, limit: usize) -> StoreResult<Vec<DailyRecord>> {
        self.store.recent_daily(&self.shard, limit).await
    }
    pub async fn append_trade(&self, record: &TradeRecord) -> StoreResult<()> {
        self.store.append_trade(&self.shard, record).await
    }
    pub async fn load_trades(&self) -> StoreResult<Vec<TradeRecord>> {
        self.store.load_trades(&self.shard).await
    }
    pub async fn put_snapshot(&self, tick: u64, day: u64, bytes: &[u8]) -> StoreResult<()> {
        self.store.put_snapshot(&self.shard, tick, day, bytes).await
    }
    pub async fn snapshot_ticks(&self) -> StoreResult<Vec<u64>> {
        self.store.snapshot_ticks(&self.shard).await
    }
    pub async fn load_snapshot(&self, tick: u64) -> StoreResult<Vec<u8>> {
        self.store.load_snapshot(&self.shard, tick).await
    }
    pub async fn clear_snapshots(&self) -> StoreResult<()> {
        self.store.clear_snapshots(&self.shard).await
    }
    pub async fn put_terrain(&self, world_id: &str, hash: u64, bytes: &[u8]) -> StoreResult<()> {
        self.store.put_terrain(&self.shard, world_id, hash, bytes).await
    }
    pub async fn load_terrain(&self, world_id: &str, hash: u64) -> StoreResult<Option<Vec<u8>>> {
        self.store.load_terrain(&self.shard, world_id, hash).await
    }
    pub async fn put_balance(&self, record: &BalanceRecord) -> StoreResult<()> {
        self.store.put_balance(&self.shard, record).await
    }
    pub async fn load_balances(&self) -> StoreResult<Vec<BalanceRecord>> {
        self.store.load_balances(&self.shard).await
    }
    pub async fn append_ledger(&self, record: &LedgerRecord) -> StoreResult<()> {
        self.store.append_ledger(&self.shard, record).await
    }
    pub async fn recent_ledger(&self, player: Option<PlayerId>, limit: usize) -> StoreResult<Vec<LedgerRecord>> {
        self.store.recent_ledger(&self.shard, player, limit).await
    }
    pub async fn append_auction(&self, record: &AuctionRecord) -> StoreResult<()> {
        self.store.append_auction(&self.shard, record).await
    }
    pub async fn recent_auctions(&self, limit: usize) -> StoreResult<Vec<AuctionRecord>> {
        self.store.recent_auctions(&self.shard, limit).await
    }
    pub async fn put_season(&self, record: &SeasonRecord) -> StoreResult<()> {
        self.store.put_season(&self.shard, record).await
    }
    pub async fn list_seasons(&self, offset: usize, limit: usize) -> StoreResult<(Vec<SeasonHead>, usize)> {
        self.store.list_seasons(&self.shard, offset, limit).await
    }
    pub async fn load_season(&self, world_id: &str, season: u32) -> StoreResult<Option<SeasonRecord>> {
        self.store.load_season(&self.shard, world_id, season).await
    }
    pub async fn player_seasons(&self, player: PlayerId, limit: usize) -> StoreResult<Vec<PlayerSeason>> {
        self.store.player_seasons(&self.shard, player, limit).await
    }
}

fn same_entry(a: &LedgerRecord, b: &LedgerRecord) -> bool {
    a.world_id == b.world_id && a.entry.seq == b.entry.seq
}

fn last_n<T>(mut v: Vec<T>, limit: usize) -> Vec<T> {
    v.split_off(v.len().saturating_sub(limit))
}

/// A shard id a store can use (it names a directory in the file store).
fn check_shard(shard: &str) -> StoreResult<()> {
    if crate::worlds::valid_world_id(shard) {
        Ok(())
    } else {
        Err(StoreError(format!("world id {shard:?} is not valid")))
    }
}

/// Snapshots a store keeps per world (older ones are fallbacks if the
/// newest is unreadable).
pub const KEEP_SNAPSHOTS: usize = 3;

/// In-memory store for tests and throwaway servers.
#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<MemInner>,
}

#[derive(Default)]
struct MemInner {
    accounts: Vec<AccountRecord>,
    alert_prefs: BTreeMap<PlayerId, AlertPrefsRecord>,
    worlds: BTreeMap<String, MemWorld>,
}

#[derive(Default)]
struct MemWorld {
    designs: Vec<DesignRecord>,
    daily: Vec<DailyRecord>,
    trades: Vec<TradeRecord>,
    /// Oldest first.
    snapshots: Vec<(u64, Vec<u8>)>,
    terrains: BTreeMap<(String, u64), Vec<u8>>,
    balances: BTreeMap<PlayerId, BalanceRecord>,
    ledger: Vec<LedgerRecord>,
    auctions: Vec<AuctionRecord>,
    /// Write order.
    seasons: Vec<SeasonRecord>,
}

impl MemInner {
    fn world(&mut self, shard: &str) -> StoreResult<&mut MemWorld> {
        check_shard(shard)?;
        Ok(self.worlds.entry(shard.to_string()).or_default())
    }
}

impl MemoryStore {
    /// The default world's trade records.
    pub async fn trades(&self) -> Vec<TradeRecord> {
        self.load_trades(crate::worlds::DEFAULT_WORLD).await.unwrap_or_default()
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn load_accounts(&self) -> StoreResult<Vec<AccountRecord>> {
        Ok(self.inner.lock().await.accounts.clone())
    }

    async fn insert_account(&self, account: &AccountRecord) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        if g.accounts.iter().any(|a| a.name == account.name) {
            return Err(StoreError(format!("account {:?} exists", account.name)));
        }
        g.accounts.push(account.clone());
        Ok(())
    }

    async fn put_alert_prefs(&self, record: &AlertPrefsRecord) -> StoreResult<()> {
        self.inner.lock().await.alert_prefs.insert(record.player, record.clone());
        Ok(())
    }

    async fn load_alert_prefs(&self, player: PlayerId) -> StoreResult<Option<AlertPrefsRecord>> {
        Ok(self.inner.lock().await.alert_prefs.get(&player).cloned())
    }

    async fn load_designs(&self, shard: &str) -> StoreResult<Vec<DesignRecord>> {
        Ok(self.inner.lock().await.world(shard)?.designs.clone())
    }

    async fn put_design(&self, shard: &str, design: &DesignRecord) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        let w = g.world(shard)?;
        w.designs.retain(|d| d.player != design.player);
        w.designs.push(design.clone());
        Ok(())
    }

    async fn append_daily(&self, shard: &str, record: &DailyRecord) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        let w = g.world(shard)?;
        match w.daily.iter_mut().find(|r| same_day(r, record)) {
            Some(r) => *r = record.clone(),
            None => w.daily.push(record.clone()),
        }
        Ok(())
    }

    async fn recent_daily(&self, shard: &str, limit: usize) -> StoreResult<Vec<DailyRecord>> {
        let mut g = self.inner.lock().await;
        let w = g.world(shard)?;
        Ok(w.daily[w.daily.len().saturating_sub(limit)..].to_vec())
    }

    async fn append_trade(&self, shard: &str, record: &TradeRecord) -> StoreResult<()> {
        self.inner.lock().await.world(shard)?.trades.push(record.clone());
        Ok(())
    }

    async fn load_trades(&self, shard: &str) -> StoreResult<Vec<TradeRecord>> {
        Ok(self.inner.lock().await.world(shard)?.trades.clone())
    }

    async fn put_snapshot(&self, shard: &str, tick: u64, _day: u64, bytes: &[u8]) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        let w = g.world(shard)?;
        w.snapshots.retain(|s| s.0 != tick);
        w.snapshots.push((tick, bytes.to_vec()));
        w.snapshots.sort_by_key(|s| s.0);
        let excess = w.snapshots.len().saturating_sub(KEEP_SNAPSHOTS);
        w.snapshots.drain(..excess);
        Ok(())
    }

    async fn snapshot_ticks(&self, shard: &str) -> StoreResult<Vec<u64>> {
        Ok(self.inner.lock().await.world(shard)?.snapshots.iter().rev().map(|s| s.0).collect())
    }

    async fn load_snapshot(&self, shard: &str, tick: u64) -> StoreResult<Vec<u8>> {
        let mut g = self.inner.lock().await;
        g.world(shard)?
            .snapshots
            .iter()
            .find(|s| s.0 == tick)
            .map(|s| s.1.clone())
            .ok_or_else(|| StoreError(format!("no snapshot at tick {tick}")))
    }

    async fn clear_snapshots(&self, shard: &str) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        let w = g.world(shard)?;
        w.snapshots.clear();
        w.terrains.clear();
        Ok(())
    }

    async fn put_terrain(&self, shard: &str, world_id: &str, hash: u64, bytes: &[u8]) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        g.world(shard)?.terrains.entry((world_id.to_string(), hash)).or_insert_with(|| bytes.to_vec());
        Ok(())
    }

    async fn load_terrain(&self, shard: &str, world_id: &str, hash: u64) -> StoreResult<Option<Vec<u8>>> {
        Ok(self.inner.lock().await.world(shard)?.terrains.get(&(world_id.to_string(), hash)).cloned())
    }

    async fn put_balance(&self, shard: &str, record: &BalanceRecord) -> StoreResult<()> {
        self.inner.lock().await.world(shard)?.balances.insert(record.player, record.clone());
        Ok(())
    }

    async fn load_balances(&self, shard: &str) -> StoreResult<Vec<BalanceRecord>> {
        Ok(self.inner.lock().await.world(shard)?.balances.values().cloned().collect())
    }

    async fn append_ledger(&self, shard: &str, record: &LedgerRecord) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        let w = g.world(shard)?;
        match w.ledger.iter_mut().find(|r| same_entry(r, record)) {
            Some(r) => *r = record.clone(),
            None => w.ledger.push(record.clone()),
        }
        Ok(())
    }

    async fn recent_ledger(
        &self,
        shard: &str,
        player: Option<PlayerId>,
        limit: usize,
    ) -> StoreResult<Vec<LedgerRecord>> {
        let mut g = self.inner.lock().await;
        let w = g.world(shard)?;
        let rows = w.ledger.iter().filter(|r| player.is_none_or(|p| r.entry.player == p)).cloned().collect();
        Ok(last_n(rows, limit))
    }

    async fn append_auction(&self, shard: &str, record: &AuctionRecord) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        let w = g.world(shard)?;
        match w.auctions.iter_mut().find(|r| same_auction(r, record)) {
            Some(r) => *r = record.clone(),
            None => w.auctions.push(record.clone()),
        }
        Ok(())
    }

    async fn recent_auctions(&self, shard: &str, limit: usize) -> StoreResult<Vec<AuctionRecord>> {
        Ok(last_n(self.inner.lock().await.world(shard)?.auctions.clone(), limit))
    }

    async fn put_season(&self, shard: &str, record: &SeasonRecord) -> StoreResult<()> {
        league::upsert(&mut self.inner.lock().await.world(shard)?.seasons, record);
        Ok(())
    }

    async fn list_seasons(&self, shard: &str, offset: usize, limit: usize) -> StoreResult<(Vec<SeasonHead>, usize)> {
        Ok(league::list_page(&self.inner.lock().await.world(shard)?.seasons, offset, limit))
    }

    async fn load_season(&self, shard: &str, world_id: &str, season: u32) -> StoreResult<Option<SeasonRecord>> {
        Ok(self.inner.lock().await.world(shard)?.seasons.iter().find(|r| r.key() == (world_id, season)).cloned())
    }

    async fn player_seasons(&self, shard: &str, player: PlayerId, limit: usize) -> StoreResult<Vec<PlayerSeason>> {
        Ok(league::player_history(&self.inner.lock().await.world(shard)?.seasons, player, limit))
    }
}

/// Records of the same world and day (records without a world id are never
/// the same day as another).
fn same_day(a: &DailyRecord, b: &DailyRecord) -> bool {
    !a.world_id.is_empty() && a.world_id == b.world_id && a.day == b.day
}

/// JSON files in one directory. Global tables at the top:
///
/// - `accounts.json`, `alert_prefs.json`: whole tables, rewritten
///   atomically (write to a temp file, then rename) on every change;
///
/// and each world's records in `worlds/<world id>/`:
///
/// - `designs.json`, `balances.json`: whole tables, as above;
/// - `history.jsonl`, `trades.jsonl`, `ledger.jsonl`, `seasons.jsonl`,
///   `auctions.jsonl`: append-only, one record per line (a replayed day,
///   ledger entry, ranked season or auction is appended again and replaces
///   the earlier line when read);
/// - `snapshots/world-<tick>.snap`: saved games, written to a temp file,
///   synced and renamed into place, the newest [`KEEP_SNAPSHOTS`] kept;
/// - `terrain/<generated world id>-<hash>.terrain`: each generated map's
///   static terrain, written the same way, once.
///
/// A data directory from before there were several worlds keeps the
/// per-world files at the top; [`FileStore::open`] moves them into
/// `worlds/main/` (the default world), one rename each, so an interrupted
/// move finishes on the next open.
pub struct FileStore {
    dir: PathBuf,
    /// Serializes writers and caches the small tables.
    tables: Mutex<Tables>,
}

struct Tables {
    accounts: Vec<AccountRecord>,
    alert_prefs: Vec<AlertPrefsRecord>,
    /// Each world's small tables, read on first use.
    worlds: BTreeMap<String, WorldTables>,
}

struct WorldTables {
    designs: Vec<DesignRecord>,
    balances: Vec<BalanceRecord>,
}

/// The per-world files and directories of a data directory, which a
/// single-world data directory keeps at its top.
pub const FILE_STORE_WORLD_FILES: &[&str] = &[
    "designs.json",
    "balances.json",
    "history.jsonl",
    "trades.jsonl",
    "ledger.jsonl",
    "seasons.jsonl",
    "auctions.jsonl",
    "snapshots",
    "terrain",
];

const WORLDS_DIR: &str = "worlds";

impl FileStore {
    pub async fn open(dir: impl AsRef<Path>) -> StoreResult<Self> {
        let dir = dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&dir).await?;
        migrate_single_world_layout(&dir).await?;
        let accounts = read_table(&dir.join("accounts.json")).await?;
        let alert_prefs = read_table(&dir.join("alert_prefs.json")).await?;
        Ok(Self { dir, tables: Mutex::new(Tables { accounts, alert_prefs, worlds: BTreeMap::new() }) })
    }

    /// The directory of a world's records.
    fn world_dir(&self, shard: &str) -> StoreResult<PathBuf> {
        check_shard(shard)?;
        Ok(self.dir.join(WORLDS_DIR).join(shard))
    }

    /// A world's small tables, read from disk on first use.
    async fn world_tables<'a>(&self, t: &'a mut Tables, shard: &str) -> StoreResult<&'a mut WorldTables> {
        let dir = self.world_dir(shard)?;
        if !t.worlds.contains_key(shard) {
            let designs = read_table(&dir.join("designs.json")).await?;
            let balances = read_table(&dir.join("balances.json")).await?;
            t.worlds.insert(shard.to_string(), WorldTables { designs, balances });
        }
        Ok(t.worlds.get_mut(shard).expect("inserted"))
    }

    /// Every line of a world's JSONL file (none if it doesn't exist).
    async fn read_lines<T: for<'de> Deserialize<'de>>(&self, shard: &str, name: &str) -> StoreResult<Vec<T>> {
        let path = self.world_dir(shard)?.join(name);
        let _guard = self.tables.lock().await;
        let text = match tokio::fs::read_to_string(path).await {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        text.lines().filter(|l| !l.trim().is_empty()).map(|l| Ok(serde_json::from_str(l)?)).collect()
    }

    async fn write_table<T: Serialize>(&self, dir: &Path, name: &str, rows: &[T]) -> StoreResult<()> {
        tokio::fs::create_dir_all(dir).await?;
        let path = dir.join(name);
        let tmp = dir.join(format!("{name}.tmp"));
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(rows)?).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    async fn append_line<T: Serialize>(&self, shard: &str, name: &str, row: &T) -> StoreResult<()> {
        let dir = self.world_dir(shard)?;
        let _guard = self.tables.lock().await;
        tokio::fs::create_dir_all(&dir).await?;
        let mut line = serde_json::to_vec(row)?;
        line.push(b'\n');
        let mut f = tokio::fs::OpenOptions::new().create(true).append(true).open(dir.join(name)).await?;
        f.write_all(&line).await?;
        f.flush().await?;
        Ok(())
    }
}

/// Move a single-world data directory's per-world files into the default
/// world's directory. Each file or directory moves with one rename, and one
/// already moved is skipped, so a move interrupted by a crash finishes on
/// the next open. A file present in both places (an older server run on the
/// directory after it moved) is an error: merging is left to a person.
async fn migrate_single_world_layout(dir: &Path) -> StoreResult<()> {
    let target = dir.join(WORLDS_DIR).join(crate::worlds::DEFAULT_WORLD);
    for name in FILE_STORE_WORLD_FILES {
        let old = dir.join(name);
        if !tokio::fs::try_exists(&old).await? {
            continue;
        }
        let new = target.join(name);
        if tokio::fs::try_exists(&new).await? {
            return Err(StoreError(format!(
                "{} and {} both exist: a server from before several worlds used the data directory after it \
                 was migrated; merge them by hand",
                old.display(),
                new.display()
            )));
        }
        tokio::fs::create_dir_all(&target).await?;
        tokio::fs::rename(&old, &new).await?;
        tracing::info!("moved {} to {} (the default world)", old.display(), new.display());
    }
    Ok(())
}

const SNAPSHOT_DIR: &str = "snapshots";
const TERRAIN_DIR: &str = "terrain";

/// The file of a stored terrain. World ids are hex; anything else that
/// could leave the directory is refused.
fn terrain_name(world_id: &str, hash: u64) -> StoreResult<String> {
    if world_id.is_empty() || !world_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(StoreError(format!("world id {world_id:?} can't name a terrain file")));
    }
    Ok(format!("{world_id}-{hash:016x}.terrain"))
}

/// Write `bytes` to `dir/name` durably: a synced temp file renamed into
/// place, then the directory synced (best effort: not every platform can
/// open a directory).
async fn write_durably(dir: &Path, name: &str, bytes: &[u8]) -> StoreResult<()> {
    tokio::fs::create_dir_all(dir).await?;
    let tmp = dir.join(format!("{name}.tmp"));
    let mut f = tokio::fs::File::create(&tmp).await?;
    f.write_all(bytes).await?;
    f.sync_all().await?;
    drop(f);
    tokio::fs::rename(&tmp, dir.join(name)).await?;
    if let Ok(d) = tokio::fs::File::open(dir).await {
        let _ = d.sync_all().await;
    }
    Ok(())
}

fn snapshot_name(tick: u64) -> String {
    format!("world-{tick:012}.snap")
}

fn snapshot_tick(name: &str) -> Option<u64> {
    name.strip_prefix("world-")?.strip_suffix(".snap")?.parse().ok()
}

async fn read_table<T: for<'de> Deserialize<'de>>(path: &Path) -> StoreResult<Vec<T>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

#[async_trait]
impl Store for FileStore {
    async fn load_accounts(&self) -> StoreResult<Vec<AccountRecord>> {
        Ok(self.tables.lock().await.accounts.clone())
    }

    async fn insert_account(&self, account: &AccountRecord) -> StoreResult<()> {
        let mut t = self.tables.lock().await;
        if t.accounts.iter().any(|a| a.name == account.name) {
            return Err(StoreError(format!("account {:?} exists", account.name)));
        }
        t.accounts.push(account.clone());
        if let Err(e) = self.write_table(&self.dir, "accounts.json", &t.accounts).await {
            t.accounts.pop();
            return Err(e);
        }
        Ok(())
    }

    async fn put_alert_prefs(&self, record: &AlertPrefsRecord) -> StoreResult<()> {
        let mut t = self.tables.lock().await;
        match t.alert_prefs.iter_mut().find(|r| r.player == record.player) {
            Some(r) => *r = record.clone(),
            None => t.alert_prefs.push(record.clone()),
        }
        self.write_table(&self.dir, "alert_prefs.json", &t.alert_prefs).await
    }

    async fn load_alert_prefs(&self, player: PlayerId) -> StoreResult<Option<AlertPrefsRecord>> {
        Ok(self.tables.lock().await.alert_prefs.iter().find(|r| r.player == player).cloned())
    }

    async fn load_designs(&self, shard: &str) -> StoreResult<Vec<DesignRecord>> {
        let mut t = self.tables.lock().await;
        Ok(self.world_tables(&mut t, shard).await?.designs.clone())
    }

    async fn put_design(&self, shard: &str, design: &DesignRecord) -> StoreResult<()> {
        let mut t = self.tables.lock().await;
        let w = self.world_tables(&mut t, shard).await?;
        w.designs.retain(|d| d.player != design.player);
        w.designs.push(design.clone());
        self.write_table(&self.world_dir(shard)?, "designs.json", &w.designs).await
    }

    async fn append_daily(&self, shard: &str, record: &DailyRecord) -> StoreResult<()> {
        self.append_line(shard, "history.jsonl", record).await
    }

    async fn recent_daily(&self, shard: &str, limit: usize) -> StoreResult<Vec<DailyRecord>> {
        let mut records: Vec<DailyRecord> = Vec::new();
        let mut index: std::collections::HashMap<(String, u64), usize> = std::collections::HashMap::new();
        for r in self.read_lines::<DailyRecord>(shard, "history.jsonl").await? {
            if r.world_id.is_empty() {
                records.push(r);
                continue;
            }
            match index.get(&(r.world_id.clone(), r.day)) {
                Some(&i) => records[i] = r,
                None => {
                    index.insert((r.world_id.clone(), r.day), records.len());
                    records.push(r);
                }
            }
        }
        Ok(last_n(records, limit))
    }

    async fn append_trade(&self, shard: &str, record: &TradeRecord) -> StoreResult<()> {
        self.append_line(shard, "trades.jsonl", record).await
    }

    async fn load_trades(&self, shard: &str) -> StoreResult<Vec<TradeRecord>> {
        self.read_lines(shard, "trades.jsonl").await
    }

    async fn put_snapshot(&self, shard: &str, tick: u64, _day: u64, bytes: &[u8]) -> StoreResult<()> {
        let dir = self.world_dir(shard)?.join(SNAPSHOT_DIR);
        write_durably(&dir, &snapshot_name(tick), bytes).await?;
        for old in self.snapshot_ticks(shard).await?.into_iter().skip(KEEP_SNAPSHOTS) {
            tokio::fs::remove_file(dir.join(snapshot_name(old))).await?;
        }
        Ok(())
    }

    async fn snapshot_ticks(&self, shard: &str) -> StoreResult<Vec<u64>> {
        let mut ticks = Vec::new();
        let mut entries = match tokio::fs::read_dir(self.world_dir(shard)?.join(SNAPSHOT_DIR)).await {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ticks),
            Err(e) => return Err(e.into()),
        };
        while let Some(e) = entries.next_entry().await? {
            if let Some(t) = e.file_name().to_str().and_then(snapshot_tick) {
                ticks.push(t);
            }
        }
        ticks.sort_unstable_by(|a, b| b.cmp(a));
        Ok(ticks)
    }

    async fn load_snapshot(&self, shard: &str, tick: u64) -> StoreResult<Vec<u8>> {
        Ok(tokio::fs::read(self.world_dir(shard)?.join(SNAPSHOT_DIR).join(snapshot_name(tick))).await?)
    }

    async fn clear_snapshots(&self, shard: &str) -> StoreResult<()> {
        let world = self.world_dir(shard)?;
        let dir = world.join(SNAPSHOT_DIR);
        for tick in self.snapshot_ticks(shard).await? {
            tokio::fs::remove_file(dir.join(snapshot_name(tick))).await?;
        }
        match tokio::fs::remove_dir_all(world.join(TERRAIN_DIR)).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    async fn put_terrain(&self, shard: &str, world_id: &str, hash: u64, bytes: &[u8]) -> StoreResult<()> {
        let name = terrain_name(world_id, hash)?;
        let dir = self.world_dir(shard)?.join(TERRAIN_DIR);
        if tokio::fs::try_exists(dir.join(&name)).await? {
            return Ok(());
        }
        write_durably(&dir, &name, bytes).await
    }

    async fn load_terrain(&self, shard: &str, world_id: &str, hash: u64) -> StoreResult<Option<Vec<u8>>> {
        let path = self.world_dir(shard)?.join(TERRAIN_DIR).join(terrain_name(world_id, hash)?);
        match tokio::fs::read(path).await {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn put_balance(&self, shard: &str, record: &BalanceRecord) -> StoreResult<()> {
        let mut t = self.tables.lock().await;
        let w = self.world_tables(&mut t, shard).await?;
        match w.balances.iter_mut().find(|b| b.player == record.player) {
            Some(b) => *b = record.clone(),
            None => w.balances.push(record.clone()),
        }
        self.write_table(&self.world_dir(shard)?, "balances.json", &w.balances).await
    }

    async fn load_balances(&self, shard: &str) -> StoreResult<Vec<BalanceRecord>> {
        let mut t = self.tables.lock().await;
        Ok(self.world_tables(&mut t, shard).await?.balances.clone())
    }

    async fn append_ledger(&self, shard: &str, record: &LedgerRecord) -> StoreResult<()> {
        self.append_line(shard, "ledger.jsonl", record).await
    }

    async fn recent_ledger(
        &self,
        shard: &str,
        player: Option<PlayerId>,
        limit: usize,
    ) -> StoreResult<Vec<LedgerRecord>> {
        // A replayed entry (same world and sequence number) replaces the
        // earlier line in its place.
        let mut rows: Vec<LedgerRecord> = Vec::new();
        let mut index: std::collections::HashMap<(String, u64), usize> = std::collections::HashMap::new();
        for r in self.read_lines::<LedgerRecord>(shard, "ledger.jsonl").await? {
            match index.get(&(r.world_id.clone(), r.entry.seq)) {
                Some(&i) => rows[i] = r,
                None => {
                    index.insert((r.world_id.clone(), r.entry.seq), rows.len());
                    rows.push(r);
                }
            }
        }
        rows.retain(|r| player.is_none_or(|p| r.entry.player == p));
        Ok(last_n(rows, limit))
    }

    async fn append_auction(&self, shard: &str, record: &AuctionRecord) -> StoreResult<()> {
        self.append_line(shard, "auctions.jsonl", record).await
    }

    async fn recent_auctions(&self, shard: &str, limit: usize) -> StoreResult<Vec<AuctionRecord>> {
        let mut rows: Vec<AuctionRecord> = Vec::new();
        let mut index: std::collections::HashMap<(String, u64), usize> = std::collections::HashMap::new();
        for r in self.read_lines::<AuctionRecord>(shard, "auctions.jsonl").await? {
            match index.get(&(r.world_id.clone(), r.listing)) {
                Some(&i) => rows[i] = r,
                None => {
                    index.insert((r.world_id.clone(), r.listing), rows.len());
                    rows.push(r);
                }
            }
        }
        Ok(last_n(rows, limit))
    }

    async fn put_season(&self, shard: &str, record: &SeasonRecord) -> StoreResult<()> {
        self.append_line(shard, SEASONS_FILE, record).await
    }

    async fn list_seasons(&self, shard: &str, offset: usize, limit: usize) -> StoreResult<(Vec<SeasonHead>, usize)> {
        Ok(league::list_page(&self.seasons(shard).await?, offset, limit))
    }

    async fn load_season(&self, shard: &str, world_id: &str, season: u32) -> StoreResult<Option<SeasonRecord>> {
        Ok(self.seasons(shard).await?.into_iter().find(|r| r.key() == (world_id, season)))
    }

    async fn player_seasons(&self, shard: &str, player: PlayerId, limit: usize) -> StoreResult<Vec<PlayerSeason>> {
        Ok(league::player_history(&self.seasons(shard).await?, player, limit))
    }
}

const SEASONS_FILE: &str = "seasons.jsonl";

impl FileStore {
    /// Every stored season of the world in write order, a rewritten one in
    /// its first place.
    async fn seasons(&self, shard: &str) -> StoreResult<Vec<SeasonRecord>> {
        let mut records: Vec<SeasonRecord> = Vec::new();
        let mut index: std::collections::HashMap<(String, u32), usize> = std::collections::HashMap::new();
        for r in self.read_lines::<SeasonRecord>(shard, SEASONS_FILE).await? {
            let key = (r.head.world_id.clone(), r.head.season);
            match index.get(&key) {
                Some(&i) => records[i] = r,
                None => {
                    index.insert(key, records.len());
                    records.push(r);
                }
            }
        }
        Ok(records)
    }
}
