//! The market data worker: one round per interval. A round has its id before it fetches;
//! the fetch, every write attempt and the board update each run under a deadline; a failed
//! fetch stores a `down` round and no snapshot, so nothing is invented.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use yieldagent_data::defillama;
use yieldagent_data::port::{FetchError, PoolSource};
use yieldagent_data::screen::{ChainRates, Flag, is_spike};
use yieldagent_db::DbError;

use crate::board::Board;
use crate::store::{self, Outcome, Round, Snapshot, Written};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub interval: Duration,
    /// Bounds the whole fetch, whatever the adapter does.
    pub fetch_deadline: Duration,
    /// Bounds each write attempt.
    pub write_deadline: Duration,
    /// Write attempts per round (the same round id each time).
    pub write_attempts: u32,
    pub retry_backoff: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(300),
            fetch_deadline: Duration::from_secs(20),
            write_deadline: Duration::from_secs(5),
            write_attempts: 3,
            retry_backoff: Duration::from_millis(500),
        }
    }
}

/// Why a round was not stored.
#[derive(Debug, thiserror::Error)]
pub enum RoundError {
    #[error("the round could not be stored after {attempts} attempts")]
    NotStored {
        attempts: u32,
        #[source]
        last: Option<DbError>,
    },
}

/// The ports a round reads.
#[derive(Clone)]
pub struct Ports {
    pub source: Arc<dyn PoolSource>,
    pub chain: Arc<dyn ChainRates>,
}

/// The clock, at PostgreSQL's precision, so a round reads back exactly as it was built.
fn now() -> OffsetDateTime {
    let t = OffsetDateTime::now_utc();
    t.replace_nanosecond(t.nanosecond() / 1_000 * 1_000)
        .unwrap_or(t)
}

/// Builds a round from one fetch. Pure but for the fetch and the clock.
pub async fn fetch_round(ports: &Ports, board: &Board, settings: &Settings) -> Round {
    let id = Uuid::now_v7();
    let started_at = now();
    let fetched = tokio::time::timeout(settings.fetch_deadline, ports.source.fetch()) // LIMIT(worker-fetch-deadline)
        .await
        .unwrap_or(Err(FetchError::Timeout));
    let observed_at = now().max(started_at);
    let mut round = Round {
        id,
        source: ports.source.name(),
        started_at,
        observed_at,
        entries: 0,
        selected: 0,
        refusals: BTreeMap::new(),
        outcome: Outcome::Down("no_markets"),
    };
    let body = match fetched {
        Ok(body) => body,
        Err(e) => {
            round.outcome = Outcome::Down(e.code());
            return round;
        }
    };
    let parsed = match defillama::parse(&body) {
        Ok(p) => p,
        Err(e) => {
            round.outcome = Outcome::Down(e.code());
            return round;
        }
    };
    round.entries = u32::try_from(parsed.entries).unwrap_or(u32::MAX);
    round.selected = u32::try_from(parsed.selected).unwrap_or(u32::MAX);
    round.refusals = parsed
        .refusals
        .iter()
        .map(|(r, n)| (r.code(), *n))
        .collect();
    if parsed.markets.is_empty() {
        return round; // LIMIT(no-markets-is-down)
    }
    let view = board.view();
    let snapshots = parsed
        .markets
        .into_iter()
        .map(|market| {
            let history: Vec<_> = view
                .recent
                .get(&market.pool_id)
                .map(|h| h.iter().copied().collect())
                .unwrap_or_default();
            let mut flags = Vec::new();
            if is_spike(market.apy_base, &history) {
                flags.push(Flag::ApySpike);
            }
            let cross_check = ports.chain.cross_check(&market);
            Snapshot {
                market,
                flags,
                cross_check,
            }
        })
        .collect();
    round.outcome = Outcome::Ok(snapshots);
    round
}

/// Stores a round with bounded attempts, then applies it to the board.
pub async fn store_round(
    pool: &PgPool,
    board: &Board,
    round: &Round,
    settings: &Settings,
) -> Result<Written, RoundError> {
    let mut last = None;
    let attempts = settings.write_attempts.max(1);
    // LIMIT(write-attempts)
    for attempt in 1..=attempts {
        match tokio::time::timeout(settings.write_deadline, store::record(pool, round)).await {
            Ok(Ok(written)) => {
                board.apply(round);
                alert(round);
                return Ok(written);
            }
            Ok(Err(e)) => last = Some(e),
            Err(_) => last = None,
        }
        if attempt < attempts {
            tokio::time::sleep(settings.retry_backoff).await;
        }
    }
    Err(RoundError::NotStored { attempts, last })
}

/// Case 5's alert, stubbed as a structured log line: no payload, only names and codes.
fn alert(round: &Round) {
    match &round.outcome {
        Outcome::Down(code) => tracing::warn!(source = round.source, code, "market source down"),
        Outcome::Ok(snapshots) => {
            for s in snapshots.iter().filter(|s| !s.flags.is_empty()) {
                tracing::warn!(source = round.source, pool = %s.market.pool_id, code = "apy_spike", "suspect market data");
            }
        }
    }
}

/// Runs rounds until `shutdown`. A round that cannot be stored is logged and skipped: the
/// board keeps the last stored state, and its age makes it stale in time.
pub async fn run(
    pool: PgPool,
    board: Arc<Board>,
    ports: Ports,
    settings: Settings,
    shutdown: CancellationToken,
) {
    let mut ticks = tokio::time::interval(settings.interval);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return,
            _ = ticks.tick() => {}
        }
        let round = tokio::select! {
            () = shutdown.cancelled() => return,
            r = fetch_round(&ports, &board, &settings) => r,
        };
        tokio::select! {
            () = shutdown.cancelled() => return,
            r = store_round(&pool, &board, &round, &settings) => {
                if let Err(e) = r {
                    tracing::error!(source = round.source, error = %yieldagent_db::chain(&e), "market round not stored");
                }
            }
        }
    }
}
