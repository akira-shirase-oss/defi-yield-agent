//! The snapshot store: one transaction per round, so a crash leaves all of a round or none;
//! the round id is chosen before the fetch, so a retried write of the same round is a no-op.

use std::collections::BTreeMap;

use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;
use yieldagent_data::Decimal;
use yieldagent_data::market::Market;
use yieldagent_data::screen::{CrossCheck, Flag};
use yieldagent_db::DbError;

/// Most snapshot rows one history read returns.
pub const MAX_HISTORY: u32 = 500; // LIMIT(history-rows)

/// One stored market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub market: Market,
    pub flags: Vec<Flag>,
    pub cross_check: CrossCheck,
}

/// What one round found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok(Vec<Snapshot>),
    /// The source failed or yielded nothing usable; `code` names why. No snapshot.
    Down(&'static str),
}

/// One fetch, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    pub id: Uuid,
    pub source: &'static str,
    pub started_at: OffsetDateTime,
    pub observed_at: OffsetDateTime,
    pub entries: u32,
    pub selected: u32,
    pub refusals: BTreeMap<&'static str, u32>,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    Inserted,
    /// This round id was already stored: nothing changed.
    AlreadyStored,
}

fn count(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// Writes a round and its snapshots in one transaction.
pub async fn record(pool: &PgPool, round: &Round) -> Result<Written, DbError> {
    let (outcome, code, snapshots): (&str, Option<&str>, &[Snapshot]) = match &round.outcome {
        Outcome::Ok(s) => ("ok", None, s),
        Outcome::Down(code) => ("down", Some(code), &[]),
    };
    let refusals: BTreeMap<&str, u32> = round.refusals.clone();
    let mut tx = pool.begin().await?;
    let inserted = sqlx::query(
        "INSERT INTO market_rounds (round_id, source, started_at, observed_at, outcome, error_code,
                                    entries, selected, accepted, refusals)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (round_id) DO NOTHING", // LIMIT(idempotent-round)
    )
    .bind(round.id)
    .bind(round.source)
    .bind(round.started_at)
    .bind(round.observed_at)
    .bind(outcome)
    .bind(code)
    .bind(i32::try_from(round.entries).unwrap_or(i32::MAX))
    .bind(i32::try_from(round.selected).unwrap_or(i32::MAX))
    .bind(count(snapshots.len()))
    .bind(sqlx::types::Json(refusals))
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        tx.rollback().await?;
        return Ok(Written::AlreadyStored);
    }
    if !snapshots.is_empty() {
        let mut ids = Vec::with_capacity(snapshots.len());
        let mut protocols = Vec::with_capacity(snapshots.len());
        let mut symbols = Vec::with_capacity(snapshots.len());
        let mut apys = Vec::with_capacity(snapshots.len());
        let mut tvls = Vec::with_capacity(snapshots.len());
        let mut flags = Vec::with_capacity(snapshots.len());
        let mut checks = Vec::with_capacity(snapshots.len());
        for s in snapshots {
            ids.push(s.market.pool_id.as_str());
            protocols.push(s.market.protocol);
            symbols.push(s.market.symbol.as_str());
            apys.push(s.market.apy.to_string());
            tvls.push(s.market.tvl_usd.to_string());
            flags.push(
                s.flags
                    .iter()
                    .map(|f| f.code())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            checks.push(s.cross_check.code());
        }
        sqlx::query(
            "INSERT INTO market_snapshots (round_id, pool_id, source, chain, protocol, symbol, apy,
                                           tvl_usd, observed_at, flags, cross_check)
             SELECT $1, u.pool_id, $2, 'Base', u.protocol, u.symbol, u.apy::numeric, u.tvl::numeric, $3,
                    CASE WHEN u.flags = '' THEN '{}'::text[] ELSE string_to_array(u.flags, ',') END,
                    u.cross_check
               FROM unnest($4::text[], $5::text[], $6::text[], $7::text[], $8::text[], $9::text[], $10::text[])
                    AS u (pool_id, protocol, symbol, apy, tvl, flags, cross_check)",
        )
        .bind(round.id)
        .bind(round.source)
        .bind(round.observed_at)
        .bind(&ids)
        .bind(&protocols)
        .bind(&symbols)
        .bind(&apys)
        .bind(&tvls)
        .bind(&flags)
        .bind(&checks)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Written::Inserted)
}

/// A source's last round, for the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastRound {
    pub source: String,
    pub observed_at: OffsetDateTime,
    pub ok: bool,
    pub error_code: Option<String>,
    pub entries: u32,
    pub selected: u32,
    pub accepted: u32,
    pub refusals: BTreeMap<String, u32>,
}

/// A stored snapshot, read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub round_id: Uuid,
    pub source: String,
    pub pool_id: String,
    pub protocol: String,
    pub symbol: String,
    pub apy: Decimal,
    pub tvl_usd: Decimal,
    pub observed_at: OffsetDateTime,
    pub flags: Vec<String>,
    pub cross_check: String,
}

type RoundRow = (String, OffsetDateTime, String, Option<String>, i32, i32, i32, Value);
type StoredRow = (Uuid, String, String, String, String, String, String, OffsetDateTime, Vec<String>, String);

fn stored(row: StoredRow) -> Result<Stored, DbError> {
    let (round_id, source, pool_id, protocol, symbol, apy, tvl, observed_at, flags, cross_check) = row;
    let decimal = |s: &str| {
        s.parse::<Decimal>()
            .map(|d| d.normalize())
            .map_err(|_| DbError::Invalid(format!("stored numeric {s:?} is not a decimal")))
    };
    Ok(Stored {
        round_id,
        source,
        pool_id,
        protocol,
        symbol,
        apy: decimal(&apy)?,
        tvl_usd: decimal(&tvl)?,
        observed_at,
        flags,
        cross_check,
    })
}

macro_rules! stored_columns {
    () => {
        "round_id, source, pool_id, protocol, symbol, apy::text, tvl_usd::text, observed_at, flags, cross_check"
    };
}

fn u32_of(n: i32) -> u32 {
    u32::try_from(n).unwrap_or(0)
}

/// The last round of each source, and the snapshots of each source's last good round.
/// The latest state: each source's last round, its last good round, and those snapshots.
pub type Latest = (Vec<LastRound>, Vec<(String, OffsetDateTime)>, Vec<Stored>);

pub async fn latest(pool: &PgPool) -> Result<Latest, DbError> {
    let rounds: Vec<RoundRow> = sqlx::query_as(
        "SELECT DISTINCT ON (source) source, observed_at, outcome, error_code, entries, selected, accepted, refusals
           FROM market_rounds ORDER BY source, observed_at DESC, round_id DESC",
    )
    .fetch_all(pool)
    .await?;
    let last = rounds
        .into_iter()
        .map(|(source, observed_at, outcome, error_code, entries, selected, accepted, refusals)| LastRound {
            source,
            observed_at,
            ok: outcome == "ok",
            error_code,
            entries: u32_of(entries),
            selected: u32_of(selected),
            accepted: u32_of(accepted),
            refusals: serde_json::from_value(refusals).unwrap_or_default(),
        })
        .collect();
    let good: Vec<(Uuid, String, OffsetDateTime)> = sqlx::query_as(
        "SELECT DISTINCT ON (source) round_id, source, observed_at
           FROM market_rounds WHERE outcome = 'ok' ORDER BY source, observed_at DESC, round_id DESC",
    )
    .fetch_all(pool)
    .await?;
    let ids: Vec<Uuid> = good.iter().map(|g| g.0).collect();
    let rows: Vec<StoredRow> = sqlx::query_as(concat!(
        "SELECT ",
        stored_columns!(),
        " FROM market_snapshots WHERE round_id = ANY($1) ORDER BY source, pool_id"
    ))
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let snapshots = rows.into_iter().map(stored).collect::<Result<_, _>>()?;
    Ok((last, good.into_iter().map(|g| (g.1, g.2)).collect(), snapshots))
}

/// The last `window` APYs of every pool seen in the last day, oldest first (the spike
/// check's moving average after a restart).
pub async fn recent_apys(pool: &PgPool, window: usize) -> Result<BTreeMap<String, Vec<Decimal>>, DbError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT pool_id, apy::text FROM (
             SELECT pool_id, apy, observed_at,
                    row_number() OVER (PARTITION BY pool_id ORDER BY observed_at DESC, round_id DESC) AS n
               FROM market_snapshots WHERE observed_at > now() - interval '1 day') t
          WHERE n <= $1 ORDER BY pool_id, observed_at",
    )
    .bind(i64::try_from(window).unwrap_or(i64::MAX))
    .fetch_all(pool)
    .await?;
    let mut out: BTreeMap<String, Vec<Decimal>> = BTreeMap::new();
    for (id, apy) in rows {
        if let Ok(d) = apy.parse::<Decimal>() {
            out.entry(id).or_default().push(d.normalize());
        }
    }
    Ok(out)
}

/// One pool's snapshots, newest first, older than `before` when given.
pub async fn history(
    pool: &PgPool,
    pool_id: &str,
    before: Option<OffsetDateTime>,
    limit: u32,
) -> Result<Vec<Stored>, DbError> {
    let rows: Vec<StoredRow> = sqlx::query_as(concat!(
        "SELECT ",
        stored_columns!(),
        " FROM market_snapshots
          WHERE pool_id = $1 AND ($2::timestamptz IS NULL OR observed_at < $2)
          ORDER BY observed_at DESC, round_id DESC LIMIT $3"
    ))
    .bind(pool_id)
    .bind(before)
    .bind(i64::from(limit.min(MAX_HISTORY)))
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(stored).collect()
}
