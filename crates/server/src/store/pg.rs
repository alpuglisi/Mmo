//! [`PgStore`]: the [`Store`] in PostgreSQL (sqlx), one table per record
//! type. The schema is in `crates/server/migrations/`, embedded in the
//! binary and applied with sqlx's migrator on [`PgStore::connect`].
//!
//! Queries are plain runtime strings (`sqlx::query`), not the
//! compile-time-checked macros, so building needs no database.

use std::borrow::Cow;

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use sqlx::migrate::{Migration, MigrationSource, MigrationType, Migrator};
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::Row;

use super::{
    AccountRecord, AlertPrefsRecord, BalanceRecord, DailyRecord, DesignRecord, LedgerRecord, Store, StoreError,
    StoreResult, TradeRecord, KEEP_SNAPSHOTS,
};
use crate::protocol::{AlertKind, AlertPrefs, LedgerEntry, LedgerKind};
use evo_sim::PlayerId;

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        StoreError(e.to_string())
    }
}

impl From<sqlx::migrate::MigrateError> for StoreError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        StoreError(format!("migration: {e}"))
    }
}

/// The migrations in `crates/server/migrations/`, compiled in. Append new
/// files here; never edit an applied one (sqlx checks their checksums).
const MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "initial", include_str!("../../migrations/0001_initial.sql")),
    (2, "economy", include_str!("../../migrations/0002_economy.sql")),
    (3, "alert_prefs", include_str!("../../migrations/0003_alert_prefs.sql")),
];

#[derive(Debug)]
struct Embedded;

impl<'s> MigrationSource<'s> for Embedded {
    fn resolve(self) -> BoxFuture<'s, Result<Vec<Migration>, sqlx::error::BoxDynError>> {
        Box::pin(async {
            Ok(MIGRATIONS
                .iter()
                .map(|&(version, desc, sql)| {
                    Migration::new(version, Cow::Borrowed(desc), MigrationType::Simple, Cow::Borrowed(sql), false)
                })
                .collect())
        })
    }
}

pub struct PgStore {
    pool: PgPool,
}

/// Game counters are unsigned and far below `i64::MAX`.
fn int(v: u64) -> i64 {
    v as i64
}

fn get_u64(row: &PgRow, col: &str) -> StoreResult<u64> {
    Ok(row.try_get::<i64, _>(col)? as u64)
}

fn kind_names(kinds: &[AlertKind]) -> Vec<String> {
    kinds.iter().map(|k| k.name().to_string()).collect()
}

/// Alert kinds by wire name; names this build doesn't know are dropped.
fn kinds(row: &PgRow, col: &str) -> StoreResult<Vec<AlertKind>> {
    Ok(row.try_get::<Vec<String>, _>(col)?.iter().filter_map(|n| AlertKind::parse(n)).collect())
}

impl PgStore {
    /// Connect (e.g. `postgres://user:pass@host/db`) and bring the schema
    /// up to date.
    pub async fn connect(url: &str) -> StoreResult<Self> {
        let pool = PgPoolOptions::new().max_connections(4).connect(url).await?;
        Self::with_pool(pool).await
    }

    pub async fn with_pool(pool: PgPool) -> StoreResult<Self> {
        Migrator::new(Embedded).await?.run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl Store for PgStore {
    async fn load_accounts(&self) -> StoreResult<Vec<AccountRecord>> {
        let rows = sqlx::query("SELECT name, player, password_hash, created_unix FROM accounts ORDER BY player")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(AccountRecord {
                    name: r.try_get("name")?,
                    player: get_u64(r, "player")? as u32,
                    password_hash: r.try_get("password_hash")?,
                    created_unix: get_u64(r, "created_unix")?,
                })
            })
            .collect()
    }

    async fn insert_account(&self, account: &AccountRecord) -> StoreResult<()> {
        let done = sqlx::query(
            "INSERT INTO accounts (name, player, password_hash, created_unix) VALUES ($1, $2, $3, $4) \
             ON CONFLICT DO NOTHING",
        )
        .bind(&account.name)
        .bind(i64::from(account.player))
        .bind(&account.password_hash)
        .bind(int(account.created_unix))
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(StoreError(format!("account {:?} exists", account.name)));
        }
        Ok(())
    }

    async fn load_designs(&self) -> StoreResult<Vec<DesignRecord>> {
        let rows = sqlx::query(
            "SELECT player, design::text AS design, founded_day, founded_unix FROM designs ORDER BY player",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(DesignRecord {
                    player: get_u64(r, "player")? as u32,
                    design: serde_json::from_str(r.try_get::<&str, _>("design")?)?,
                    founded_day: get_u64(r, "founded_day")?,
                    founded_unix: get_u64(r, "founded_unix")?,
                })
            })
            .collect()
    }

    async fn put_design(&self, design: &DesignRecord) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO designs (player, design, founded_day, founded_unix) VALUES ($1, $2::jsonb, $3, $4) \
             ON CONFLICT (player) DO UPDATE SET design = EXCLUDED.design, founded_day = EXCLUDED.founded_day, \
             founded_unix = EXCLUDED.founded_unix",
        )
        .bind(i64::from(design.player))
        .bind(serde_json::to_string(&design.design)?)
        .bind(int(design.founded_day))
        .bind(int(design.founded_unix))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn append_daily(&self, record: &DailyRecord) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO daily_history (world_id, day, year, season, breeding_adults, legacy, lineages) \
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7::jsonb) \
             ON CONFLICT (world_id, day) DO UPDATE SET year = EXCLUDED.year, season = EXCLUDED.season, \
             breeding_adults = EXCLUDED.breeding_adults, legacy = EXCLUDED.legacy, lineages = EXCLUDED.lineages",
        )
        .bind(&record.world_id)
        .bind(int(record.day))
        .bind(int(record.year))
        .bind(&record.season)
        .bind(int(record.breeding_adults as u64))
        .bind(serde_json::to_string(&record.legacy)?)
        .bind(serde_json::to_string(&record.lineages)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn recent_daily(&self, limit: usize) -> StoreResult<Vec<DailyRecord>> {
        let rows = sqlx::query(
            "SELECT world_id, day, year, season, breeding_adults, legacy::text AS legacy, lineages::text AS lineages \
             FROM (SELECT * FROM daily_history ORDER BY seq DESC LIMIT $1) recent ORDER BY seq",
        )
        .bind(int(limit as u64))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(DailyRecord {
                    world_id: r.try_get("world_id")?,
                    day: get_u64(r, "day")?,
                    year: get_u64(r, "year")?,
                    season: r.try_get("season")?,
                    breeding_adults: get_u64(r, "breeding_adults")? as usize,
                    legacy: serde_json::from_str(r.try_get::<&str, _>("legacy")?)?,
                    lineages: serde_json::from_str(r.try_get::<&str, _>("lineages")?)?,
                })
            })
            .collect()
    }

    async fn append_trade(&self, record: &TradeRecord) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO trades (offer, from_player, to_player, mine, partner, price, status, conceived, day, listing, settlement) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(int(record.offer))
        .bind(i64::from(record.from_player))
        .bind(i64::from(record.to_player))
        .bind(int(record.mine))
        .bind(int(record.partner))
        .bind(record.price.map(int))
        .bind(&record.status)
        .bind(record.conceived)
        .bind(int(record.day))
        .bind(record.listing.map(int))
        .bind(&record.settlement)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_trades(&self) -> StoreResult<Vec<TradeRecord>> {
        let rows = sqlx::query("SELECT offer, from_player, to_player, mine, partner, price, status, conceived, day, listing, settlement FROM trades ORDER BY id")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(TradeRecord {
                    offer: get_u64(r, "offer")?,
                    from_player: get_u64(r, "from_player")? as u32,
                    to_player: get_u64(r, "to_player")? as u32,
                    mine: get_u64(r, "mine")?,
                    partner: get_u64(r, "partner")?,
                    price: r.try_get::<Option<i64>, _>("price")?.map(|p| p as u64),
                    status: r.try_get("status")?,
                    conceived: r.try_get("conceived")?,
                    day: get_u64(r, "day")?,
                    listing: r.try_get::<Option<i64>, _>("listing")?.map(|p| p as u64),
                    settlement: r.try_get("settlement")?,
                })
            })
            .collect()
    }

    async fn put_snapshot(&self, tick: u64, day: u64, bytes: &[u8]) -> StoreResult<()> {
        let version = crate::snapshot::header(bytes).ok().map(|h| h.version as i32);
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO snapshots (tick, day, size_bytes, format_version, data) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (tick) DO UPDATE SET day = EXCLUDED.day, size_bytes = EXCLUDED.size_bytes, \
             format_version = EXCLUDED.format_version, saved_at = now(), data = EXCLUDED.data",
        )
        .bind(int(tick))
        .bind(int(day))
        .bind(int(bytes.len() as u64))
        .bind(version)
        .bind(bytes)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM snapshots WHERE tick NOT IN (SELECT tick FROM snapshots ORDER BY tick DESC LIMIT $1)")
            .bind(int(KEEP_SNAPSHOTS as u64))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn snapshot_ticks(&self) -> StoreResult<Vec<u64>> {
        let rows = sqlx::query("SELECT tick FROM snapshots ORDER BY tick DESC").fetch_all(&self.pool).await?;
        rows.iter().map(|r| get_u64(r, "tick")).collect()
    }

    async fn load_snapshot(&self, tick: u64) -> StoreResult<Vec<u8>> {
        let row = sqlx::query("SELECT data FROM snapshots WHERE tick = $1")
            .bind(int(tick))
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(r) => Ok(r.try_get("data")?),
            None => Err(StoreError(format!("no snapshot at tick {tick}"))),
        }
    }

    async fn clear_snapshots(&self) -> StoreResult<()> {
        sqlx::query("DELETE FROM snapshots").execute(&self.pool).await?;
        Ok(())
    }
    async fn put_balance(&self, record: &BalanceRecord) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO balances (player, balance, updated_day) VALUES ($1, $2, $3) \
             ON CONFLICT (player) DO UPDATE SET balance = EXCLUDED.balance, updated_day = EXCLUDED.updated_day, updated_at = now()",
        )
        .bind(i64::from(record.player))
        .bind(int(record.balance))
        .bind(int(record.day))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_balances(&self) -> StoreResult<Vec<BalanceRecord>> {
        let rows = sqlx::query("SELECT player, balance, updated_day FROM balances ORDER BY player")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(BalanceRecord {
                    player: get_u64(r, "player")? as PlayerId,
                    balance: get_u64(r, "balance")?,
                    day: get_u64(r, "updated_day")?,
                })
            })
            .collect()
    }

    async fn append_ledger(&self, record: &LedgerRecord) -> StoreResult<()> {
        let e = &record.entry;
        sqlx::query(
            "INSERT INTO ledger (world_id, seq, player, tick, day, delta, balance, kind, text, offer, listing) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (world_id, seq) DO UPDATE SET player = EXCLUDED.player, tick = EXCLUDED.tick, day = EXCLUDED.day, \
             delta = EXCLUDED.delta, balance = EXCLUDED.balance, kind = EXCLUDED.kind, text = EXCLUDED.text, \
             offer = EXCLUDED.offer, listing = EXCLUDED.listing",
        )
        .bind(&record.world_id)
        .bind(int(e.seq))
        .bind(i64::from(e.player))
        .bind(int(e.tick))
        .bind(int(e.day))
        .bind(e.delta)
        .bind(int(e.balance))
        .bind(e.kind.name())
        .bind(&e.text)
        .bind(e.offer.map(int))
        .bind(e.listing.map(int))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn recent_ledger(&self, player: Option<PlayerId>, limit: usize) -> StoreResult<Vec<LedgerRecord>> {
        let rows = sqlx::query(
            "SELECT world_id, seq, player, tick, day, delta, balance, kind, text, offer, listing \
             FROM (SELECT * FROM ledger WHERE $1::BIGINT IS NULL OR player = $1 ORDER BY id DESC LIMIT $2) recent ORDER BY id",
        )
        .bind(player.map(i64::from))
        .bind(int(limit as u64))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                let kind: String = r.try_get("kind")?;
                Ok(LedgerRecord {
                    world_id: r.try_get("world_id")?,
                    entry: LedgerEntry {
                        seq: get_u64(r, "seq")?,
                        player: get_u64(r, "player")? as PlayerId,
                        tick: get_u64(r, "tick")?,
                        day: get_u64(r, "day")?,
                        delta: r.try_get("delta")?,
                        balance: get_u64(r, "balance")?,
                        kind: LedgerKind::parse(&kind)
                            .ok_or_else(|| StoreError(format!("unknown ledger kind {kind:?}")))?,
                        text: r.try_get("text")?,
                        offer: r.try_get::<Option<i64>, _>("offer")?.map(|v| v as u64),
                        listing: r.try_get::<Option<i64>, _>("listing")?.map(|v| v as u64),
                    },
                })
            })
            .collect()
    }

    async fn put_alert_prefs(&self, record: &AlertPrefsRecord) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO alert_prefs (player, hidden, toast) VALUES ($1, $2, $3) \
             ON CONFLICT (player) DO UPDATE SET hidden = EXCLUDED.hidden, toast = EXCLUDED.toast, updated_at = now()",
        )
        .bind(i64::from(record.player))
        .bind(kind_names(&record.prefs.hidden))
        .bind(kind_names(&record.prefs.toast))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_alert_prefs(&self, player: PlayerId) -> StoreResult<Option<AlertPrefsRecord>> {
        let row = sqlx::query("SELECT player, hidden, toast FROM alert_prefs WHERE player = $1")
            .bind(i64::from(player))
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| {
            Ok(AlertPrefsRecord {
                player: get_u64(&r, "player")? as PlayerId,
                prefs: AlertPrefs { hidden: kinds(&r, "hidden")?, toast: kinds(&r, "toast")? },
            })
        })
        .transpose()
    }
}
