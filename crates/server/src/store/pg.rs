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
use crate::league::{Category, PlayerPlacement, PlayerSeason, SeasonHead, SeasonRecord};
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
    (4, "terrain", include_str!("../../migrations/0004_terrain.sql")),
    (5, "seasons", include_str!("../../migrations/0005_seasons.sql")),
    // 6 is the marketplace auctions' migration (another branch).
    (7, "worlds", include_str!("../../migrations/0007_worlds.sql")),
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

/// A terrain content hash as stored: 16 lowercase hex digits.
fn hash_text(hash: u64) -> String {
    format!("{hash:016x}")
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

    /// Apply the embedded migrations up to `version` only (tests of a later
    /// migration build the schema it starts from).
    #[doc(hidden)]
    pub async fn migrate_to(pool: &PgPool, version: i64) -> StoreResult<()> {
        let mut m = Migrator::new(Embedded).await?;
        m.migrations = m.migrations.iter().filter(|x| x.version <= version).cloned().collect::<Vec<_>>().into();
        m.run(pool).await?;
        Ok(())
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

    async fn load_designs(&self, shard: &str) -> StoreResult<Vec<DesignRecord>> {
        let rows = sqlx::query(
            "SELECT player, design::text AS design, founded_day, founded_unix FROM designs WHERE shard = $1 \
             ORDER BY player",
        )
        .bind(shard)
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

    async fn put_design(&self, shard: &str, design: &DesignRecord) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO designs (shard, player, design, founded_day, founded_unix) \
             VALUES ($5, $1, $2::jsonb, $3, $4) \
             ON CONFLICT (shard, player) DO UPDATE SET design = EXCLUDED.design, founded_day = EXCLUDED.founded_day, \
             founded_unix = EXCLUDED.founded_unix",
        )
        .bind(i64::from(design.player))
        .bind(serde_json::to_string(&design.design)?)
        .bind(int(design.founded_day))
        .bind(int(design.founded_unix))
        .bind(shard)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn append_daily(&self, shard: &str, record: &DailyRecord) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO daily_history (shard, world_id, day, year, season, breeding_adults, legacy, lineages) \
             VALUES ($8, $1, $2, $3, $4, $5, $6::jsonb, $7::jsonb) \
             ON CONFLICT (shard, world_id, day) DO UPDATE SET year = EXCLUDED.year, season = EXCLUDED.season, \
             breeding_adults = EXCLUDED.breeding_adults, legacy = EXCLUDED.legacy, lineages = EXCLUDED.lineages",
        )
        .bind(&record.world_id)
        .bind(int(record.day))
        .bind(int(record.year))
        .bind(&record.season)
        .bind(int(record.breeding_adults as u64))
        .bind(serde_json::to_string(&record.legacy)?)
        .bind(serde_json::to_string(&record.lineages)?)
        .bind(shard)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn recent_daily(&self, shard: &str, limit: usize) -> StoreResult<Vec<DailyRecord>> {
        let rows = sqlx::query(
            "SELECT world_id, day, year, season, breeding_adults, legacy::text AS legacy, lineages::text AS lineages \
             FROM (SELECT * FROM daily_history WHERE shard = $2 ORDER BY seq DESC LIMIT $1) recent ORDER BY seq",
        )
        .bind(int(limit as u64))
        .bind(shard)
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

    async fn append_trade(&self, shard: &str, record: &TradeRecord) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO trades (shard, offer, from_player, to_player, mine, partner, price, status, conceived, day, listing, settlement) \
             VALUES ($12, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
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
        .bind(shard)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_trades(&self, shard: &str) -> StoreResult<Vec<TradeRecord>> {
        let rows = sqlx::query("SELECT offer, from_player, to_player, mine, partner, price, status, conceived, day, listing, settlement FROM trades WHERE shard = $1 ORDER BY id")
            .bind(shard)
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

    async fn put_snapshot(&self, shard: &str, tick: u64, day: u64, bytes: &[u8]) -> StoreResult<()> {
        let header = crate::snapshot::header(bytes).ok();
        let version = header.map(|h| h.version as i32);
        let terrain = header.and_then(|h| h.terrain).map(hash_text);
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO snapshots (shard, tick, day, size_bytes, format_version, terrain_hash, data) \
             VALUES ($7, $1, $2, $3, $4, $5, $6) \
             ON CONFLICT (shard, tick) DO UPDATE SET day = EXCLUDED.day, size_bytes = EXCLUDED.size_bytes, \
             format_version = EXCLUDED.format_version, terrain_hash = EXCLUDED.terrain_hash, saved_at = now(), \
             data = EXCLUDED.data",
        )
        .bind(int(tick))
        .bind(int(day))
        .bind(int(bytes.len() as u64))
        .bind(version)
        .bind(terrain)
        .bind(bytes)
        .bind(shard)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM snapshots WHERE shard = $2 AND tick NOT IN \
             (SELECT tick FROM snapshots WHERE shard = $2 ORDER BY tick DESC LIMIT $1)",
        )
        .bind(int(KEEP_SNAPSHOTS as u64))
        .bind(shard)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn snapshot_ticks(&self, shard: &str) -> StoreResult<Vec<u64>> {
        let rows = sqlx::query("SELECT tick FROM snapshots WHERE shard = $1 ORDER BY tick DESC")
            .bind(shard)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| get_u64(r, "tick")).collect()
    }

    async fn load_snapshot(&self, shard: &str, tick: u64) -> StoreResult<Vec<u8>> {
        let row = sqlx::query("SELECT data FROM snapshots WHERE shard = $2 AND tick = $1")
            .bind(int(tick))
            .bind(shard)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(r) => Ok(r.try_get("data")?),
            None => Err(StoreError(format!("no snapshot at tick {tick}"))),
        }
    }

    async fn clear_snapshots(&self, shard: &str) -> StoreResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM snapshots WHERE shard = $1").bind(shard).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM terrains WHERE shard = $1").bind(shard).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn put_terrain(&self, shard: &str, world_id: &str, hash: u64, bytes: &[u8]) -> StoreResult<()> {
        // Usually stored already (once per world): don't send megabytes to
        // find out.
        let known = sqlx::query("SELECT 1 FROM terrains WHERE shard = $3 AND world_id = $1 AND hash = $2")
            .bind(world_id)
            .bind(hash_text(hash))
            .bind(shard)
            .fetch_optional(&self.pool)
            .await?;
        if known.is_some() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO terrains (shard, world_id, hash, size_bytes, data) VALUES ($5, $1, $2, $3, $4) \
             ON CONFLICT (shard, world_id, hash) DO NOTHING",
        )
        .bind(world_id)
        .bind(hash_text(hash))
        .bind(int(bytes.len() as u64))
        .bind(bytes)
        .bind(shard)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_terrain(&self, shard: &str, world_id: &str, hash: u64) -> StoreResult<Option<Vec<u8>>> {
        let row = sqlx::query("SELECT data FROM terrains WHERE shard = $3 AND world_id = $1 AND hash = $2")
            .bind(world_id)
            .bind(hash_text(hash))
            .bind(shard)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.try_get("data")).transpose()?)
    }

    async fn put_balance(&self, shard: &str, record: &BalanceRecord) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO balances (shard, player, balance, updated_day) VALUES ($4, $1, $2, $3) \
             ON CONFLICT (shard, player) DO UPDATE SET balance = EXCLUDED.balance, updated_day = EXCLUDED.updated_day, updated_at = now()",
        )
        .bind(i64::from(record.player))
        .bind(int(record.balance))
        .bind(int(record.day))
        .bind(shard)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_balances(&self, shard: &str) -> StoreResult<Vec<BalanceRecord>> {
        let rows = sqlx::query("SELECT player, balance, updated_day FROM balances WHERE shard = $1 ORDER BY player")
            .bind(shard)
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

    async fn append_ledger(&self, shard: &str, record: &LedgerRecord) -> StoreResult<()> {
        let e = &record.entry;
        sqlx::query(
            "INSERT INTO ledger (shard, world_id, seq, player, tick, day, delta, balance, kind, text, offer, listing) \
             VALUES ($12, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (shard, world_id, seq) DO UPDATE SET player = EXCLUDED.player, tick = EXCLUDED.tick, day = EXCLUDED.day, \
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
        .bind(shard)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn recent_ledger(
        &self,
        shard: &str,
        player: Option<PlayerId>,
        limit: usize,
    ) -> StoreResult<Vec<LedgerRecord>> {
        let rows = sqlx::query(
            "SELECT world_id, seq, player, tick, day, delta, balance, kind, text, offer, listing \
             FROM (SELECT * FROM ledger WHERE shard = $3 AND ($1::BIGINT IS NULL OR player = $1) \
             ORDER BY id DESC LIMIT $2) recent ORDER BY id",
        )
        .bind(player.map(i64::from))
        .bind(int(limit as u64))
        .bind(shard)
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

    async fn put_season(&self, shard: &str, record: &SeasonRecord) -> StoreResult<()> {
        let h = &record.head;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO seasons (shard, world_id, season, first_day, last_day, ended_unix, head, standings) \
             VALUES ($8, $1, $2, $3, $4, $5, $6::jsonb, $7::jsonb) \
             ON CONFLICT (shard, world_id, season) DO UPDATE SET first_day = EXCLUDED.first_day, \
             last_day = EXCLUDED.last_day, ended_unix = EXCLUDED.ended_unix, head = EXCLUDED.head, \
             standings = EXCLUDED.standings",
        )
        .bind(&h.world_id)
        .bind(i64::from(h.season))
        .bind(int(h.first_day))
        .bind(int(h.last_day))
        .bind(int(h.ended_unix))
        .bind(serde_json::to_string(h)?)
        .bind(serde_json::to_string(&record.standings)?)
        .bind(shard)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM season_placements WHERE shard = $3 AND world_id = $1 AND season = $2")
            .bind(&h.world_id)
            .bind(i64::from(h.season))
            .bind(shard)
            .execute(&mut *tx)
            .await?;
        let (mut players, mut categories, mut ranks, mut values, mut ranked) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for st in &record.standings {
            for r in &st.rows {
                players.push(i64::from(r.player));
                categories.push(st.category.name().to_string());
                ranks.push(i64::from(r.rank));
                values.push(r.value);
                ranked.push(st.rows.len() as i64);
            }
        }
        sqlx::query(
            "INSERT INTO season_placements (shard, world_id, season, player, category, rank, value, ranked) \
             SELECT $8, $1, $2, * FROM UNNEST($3::bigint[], $4::text[], $5::bigint[], $6::float8[], $7::bigint[])",
        )
        .bind(&h.world_id)
        .bind(i64::from(h.season))
        .bind(players)
        .bind(categories)
        .bind(ranks)
        .bind(values)
        .bind(ranked)
        .bind(shard)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn list_seasons(&self, shard: &str, offset: usize, limit: usize) -> StoreResult<(Vec<SeasonHead>, usize)> {
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM seasons WHERE shard = $1")
            .bind(shard)
            .fetch_one(&self.pool)
            .await?;
        let rows =
            sqlx::query("SELECT head::text AS head FROM seasons WHERE shard = $3 ORDER BY seq DESC OFFSET $1 LIMIT $2")
                .bind(int(offset as u64))
                .bind(int(limit as u64))
                .bind(shard)
                .fetch_all(&self.pool)
                .await?;
        let heads = rows
            .iter()
            .map(|r| Ok(serde_json::from_str(r.try_get::<&str, _>("head")?)?))
            .collect::<StoreResult<Vec<SeasonHead>>>()?;
        Ok((heads, total as usize))
    }

    async fn load_season(&self, shard: &str, world_id: &str, season: u32) -> StoreResult<Option<SeasonRecord>> {
        let row = sqlx::query(
            "SELECT head::text AS head, standings::text AS standings FROM seasons \
             WHERE shard = $3 AND world_id = $1 AND season = $2",
        )
        .bind(world_id)
        .bind(i64::from(season))
        .bind(shard)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| {
            Ok(SeasonRecord {
                head: serde_json::from_str(r.try_get::<&str, _>("head")?)?,
                standings: serde_json::from_str(r.try_get::<&str, _>("standings")?)?,
            })
        })
        .transpose()
    }

    async fn player_seasons(&self, shard: &str, player: PlayerId, limit: usize) -> StoreResult<Vec<PlayerSeason>> {
        let rows = sqlx::query(
            "WITH mine AS ( \
                 SELECT s.seq FROM seasons s JOIN season_placements p USING (shard, world_id, season) \
                 WHERE s.shard = $3 AND p.player = $1 GROUP BY s.seq ORDER BY s.seq DESC LIMIT $2) \
             SELECT s.seq, s.world_id, s.season, s.first_day, s.last_day, s.ended_unix, \
                    p.category, p.rank, p.value, p.ranked \
             FROM seasons s JOIN season_placements p USING (shard, world_id, season) \
             WHERE s.shard = $3 AND p.player = $1 AND s.seq IN (SELECT seq FROM mine) ORDER BY s.seq DESC",
        )
        .bind(i64::from(player))
        .bind(int(limit as u64))
        .bind(shard)
        .fetch_all(&self.pool)
        .await?;
        let mut out: Vec<(i64, PlayerSeason)> = Vec::new();
        for r in &rows {
            let seq: i64 = r.try_get("seq")?;
            if out.last().is_none_or(|(s, _)| *s != seq) {
                out.push((
                    seq,
                    PlayerSeason {
                        world_id: r.try_get("world_id")?,
                        season: get_u64(r, "season")? as u32,
                        first_day: get_u64(r, "first_day")?,
                        last_day: get_u64(r, "last_day")?,
                        ended_unix: get_u64(r, "ended_unix")?,
                        placements: Vec::new(),
                    },
                ));
            }
            // A category this build doesn't know is left out.
            let Some(category) = Category::parse(r.try_get::<&str, _>("category")?) else { continue };
            if let Some((_, s)) = out.last_mut() {
                s.placements.push(PlayerPlacement {
                    category,
                    rank: get_u64(r, "rank")? as u32,
                    value: r.try_get("value")?,
                    of: get_u64(r, "ranked")? as usize,
                });
            }
        }
        Ok(out
            .into_iter()
            .map(|(_, mut s)| {
                s.placements.sort_by_key(|p| p.category);
                s
            })
            .filter(|s| !s.placements.is_empty())
            .collect())
    }
}
