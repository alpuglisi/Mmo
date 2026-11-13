//! Persistence behind the [`Store`] trait.
//!
//! The design doc puts accounts, lineage history and world snapshots in
//! PostgreSQL (sqlx). No database is available yet, so [`FileStore`] keeps
//! the same records as JSON files in a data directory, and [`MemoryStore`]
//! serves tests. A `PgStore` would implement the same trait with one table
//! per record type (`accounts`, `designs`, `daily_history`, `trades`).
//!
//! What is persisted: accounts (name, player id, argon2 hash), founder
//! designs (re-founded into the new world on restart), one history record per
//! simulated day (legacy shares and lineage counts), and directed-mating
//! agreements with their price, and saved games (a world snapshot with the
//! game state around it, `crate::snapshot`; the newest [`KEEP_SNAPSHOTS`]
//! are kept). Not persisted: login sessions (clients log in again after a
//! restart).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use evo_sim::PlayerId;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::design::DesignSpec;

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
}

#[async_trait]
pub trait Store: Send + Sync {
    async fn load_accounts(&self) -> StoreResult<Vec<AccountRecord>>;
    /// Fails if the name is taken.
    async fn insert_account(&self, account: &AccountRecord) -> StoreResult<()>;
    async fn load_designs(&self) -> StoreResult<Vec<DesignRecord>>;
    /// Insert or replace the player's design.
    async fn put_design(&self, design: &DesignRecord) -> StoreResult<()>;
    async fn append_daily(&self, record: &DailyRecord) -> StoreResult<()>;
    /// The most recent `limit` daily records, oldest first.
    async fn recent_daily(&self, limit: usize) -> StoreResult<Vec<DailyRecord>>;
    async fn append_trade(&self, record: &TradeRecord) -> StoreResult<()>;
    /// Save a snapshot taken at `tick` (opaque bytes) and drop all but the
    /// newest [`KEEP_SNAPSHOTS`]. Replaces a snapshot of the same tick.
    async fn put_snapshot(&self, tick: u64, bytes: &[u8]) -> StoreResult<()>;
    /// Ticks of the stored snapshots, newest first.
    async fn snapshot_ticks(&self) -> StoreResult<Vec<u64>>;
    async fn load_snapshot(&self, tick: u64) -> StoreResult<Vec<u8>>;
}

/// Snapshots a store keeps (older ones are fallbacks if the newest is
/// unreadable).
pub const KEEP_SNAPSHOTS: usize = 3;

/// In-memory store for tests and throwaway servers.
#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<MemInner>,
}

#[derive(Default)]
struct MemInner {
    accounts: Vec<AccountRecord>,
    designs: Vec<DesignRecord>,
    daily: Vec<DailyRecord>,
    trades: Vec<TradeRecord>,
    /// Oldest first.
    snapshots: Vec<(u64, Vec<u8>)>,
}

impl MemoryStore {
    pub async fn trades(&self) -> Vec<TradeRecord> {
        self.inner.lock().await.trades.clone()
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

    async fn load_designs(&self) -> StoreResult<Vec<DesignRecord>> {
        Ok(self.inner.lock().await.designs.clone())
    }

    async fn put_design(&self, design: &DesignRecord) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        g.designs.retain(|d| d.player != design.player);
        g.designs.push(design.clone());
        Ok(())
    }

    async fn append_daily(&self, record: &DailyRecord) -> StoreResult<()> {
        self.inner.lock().await.daily.push(record.clone());
        Ok(())
    }

    async fn recent_daily(&self, limit: usize) -> StoreResult<Vec<DailyRecord>> {
        let g = self.inner.lock().await;
        Ok(g.daily[g.daily.len().saturating_sub(limit)..].to_vec())
    }

    async fn append_trade(&self, record: &TradeRecord) -> StoreResult<()> {
        self.inner.lock().await.trades.push(record.clone());
        Ok(())
    }

    async fn put_snapshot(&self, tick: u64, bytes: &[u8]) -> StoreResult<()> {
        let mut g = self.inner.lock().await;
        g.snapshots.retain(|s| s.0 != tick);
        g.snapshots.push((tick, bytes.to_vec()));
        g.snapshots.sort_by_key(|s| s.0);
        let excess = g.snapshots.len().saturating_sub(KEEP_SNAPSHOTS);
        g.snapshots.drain(..excess);
        Ok(())
    }

    async fn snapshot_ticks(&self) -> StoreResult<Vec<u64>> {
        Ok(self.inner.lock().await.snapshots.iter().rev().map(|s| s.0).collect())
    }

    async fn load_snapshot(&self, tick: u64) -> StoreResult<Vec<u8>> {
        let g = self.inner.lock().await;
        g.snapshots.iter().find(|s| s.0 == tick).map(|s| s.1.clone()).ok_or_else(|| StoreError(format!("no snapshot at tick {tick}")))
    }
}

/// JSON files in one directory:
///
/// - `accounts.json`, `designs.json`: whole tables, rewritten atomically
///   (write to a temp file, then rename) on every change;
/// - `history.jsonl`, `trades.jsonl`: append-only, one record per line;
/// - `snapshots/world-<tick>.snap`: saved games, written to a temp file,
///   synced and renamed into place, the newest [`KEEP_SNAPSHOTS`] kept.
pub struct FileStore {
    dir: PathBuf,
    /// Serializes writers and caches the small tables.
    tables: Mutex<Tables>,
}

struct Tables {
    accounts: Vec<AccountRecord>,
    designs: Vec<DesignRecord>,
}

impl FileStore {
    pub async fn open(dir: impl AsRef<Path>) -> StoreResult<Self> {
        let dir = dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&dir).await?;
        let accounts = read_table(&dir.join("accounts.json")).await?;
        let designs = read_table(&dir.join("designs.json")).await?;
        Ok(Self { dir, tables: Mutex::new(Tables { accounts, designs }) })
    }

    async fn write_table<T: Serialize>(&self, name: &str, rows: &[T]) -> StoreResult<()> {
        let path = self.dir.join(name);
        let tmp = self.dir.join(format!("{name}.tmp"));
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(rows)?).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    async fn append_line<T: Serialize>(&self, name: &str, row: &T) -> StoreResult<()> {
        let _guard = self.tables.lock().await;
        let mut line = serde_json::to_vec(row)?;
        line.push(b'\n');
        let mut f = tokio::fs::OpenOptions::new().create(true).append(true).open(self.dir.join(name)).await?;
        f.write_all(&line).await?;
        f.flush().await?;
        Ok(())
    }
}

const SNAPSHOT_DIR: &str = "snapshots";

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
        if let Err(e) = self.write_table("accounts.json", &t.accounts).await {
            t.accounts.pop();
            return Err(e);
        }
        Ok(())
    }

    async fn load_designs(&self) -> StoreResult<Vec<DesignRecord>> {
        Ok(self.tables.lock().await.designs.clone())
    }

    async fn put_design(&self, design: &DesignRecord) -> StoreResult<()> {
        let mut t = self.tables.lock().await;
        t.designs.retain(|d| d.player != design.player);
        t.designs.push(design.clone());
        self.write_table("designs.json", &t.designs).await
    }

    async fn append_daily(&self, record: &DailyRecord) -> StoreResult<()> {
        self.append_line("history.jsonl", record).await
    }

    async fn recent_daily(&self, limit: usize) -> StoreResult<Vec<DailyRecord>> {
        let _guard = self.tables.lock().await;
        let text = match tokio::fs::read_to_string(self.dir.join("history.jsonl")).await {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(limit)..].iter().map(|l| Ok(serde_json::from_str(l)?)).collect()
    }

    async fn append_trade(&self, record: &TradeRecord) -> StoreResult<()> {
        self.append_line("trades.jsonl", record).await
    }

    async fn put_snapshot(&self, tick: u64, bytes: &[u8]) -> StoreResult<()> {
        let dir = self.dir.join(SNAPSHOT_DIR);
        tokio::fs::create_dir_all(&dir).await?;
        let tmp = dir.join(format!("{}.tmp", snapshot_name(tick)));
        let mut f = tokio::fs::File::create(&tmp).await?;
        f.write_all(bytes).await?;
        f.sync_all().await?;
        drop(f);
        tokio::fs::rename(&tmp, dir.join(snapshot_name(tick))).await?;
        // Make the rename durable too (best effort: not every platform can
        // open a directory).
        if let Ok(d) = tokio::fs::File::open(&dir).await {
            let _ = d.sync_all().await;
        }
        for old in self.snapshot_ticks().await?.into_iter().skip(KEEP_SNAPSHOTS) {
            tokio::fs::remove_file(dir.join(snapshot_name(old))).await?;
        }
        Ok(())
    }

    async fn snapshot_ticks(&self) -> StoreResult<Vec<u64>> {
        let mut ticks = Vec::new();
        let mut entries = match tokio::fs::read_dir(self.dir.join(SNAPSHOT_DIR)).await {
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

    async fn load_snapshot(&self, tick: u64) -> StoreResult<Vec<u8>> {
        Ok(tokio::fs::read(self.dir.join(SNAPSHOT_DIR).join(snapshot_name(tick))).await?)
    }
}
