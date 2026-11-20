//! The worker and the store against PostgreSQL: whole rounds, idempotent writes, down and
//! stale sources without invented data, Case 5's spike flag, deadlines, crash recovery.
#![expect(clippy::unwrap_used, reason = "integration test helpers")]

mod common;

use std::time::Duration;

use common::{AAVE, HOSTILE, RECORDED, body, count, ports, settings};
use time::OffsetDateTime;
use yieldagent_data::Decimal;
use yieldagent_data::market::Market;
use yieldagent_data::port::FetchError;
use yieldagent_data::recorded::Step;
use yieldagent_data::screen::CrossCheck;
use yieldagent_db::testing::{Role, TestDb};
use yieldagent_markets::Board;
use yieldagent_markets::store::{self, Outcome, Round, Snapshot, Written};
use yieldagent_markets::worker::{RoundError, Settings, fetch_round, store_round};

const GUARD: Duration = Duration::from_secs(20);
const ROUNDS: &str = "SELECT count(*) FROM market_rounds";
const SNAPSHOTS: &str = "SELECT count(*) FROM market_snapshots";

#[tokio::test]
async fn a_round_is_stored_whole_once_and_a_retry_changes_nothing() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Board::default();
    let (_, ports) = ports(vec![Step::Body(RECORDED.to_vec())]);
    let round = fetch_round(&ports, &board, &settings()).await;
    assert_eq!(store_round(&pool, &board, &round, &settings()).await.unwrap(), Written::Inserted);
    assert_eq!((count(&pool, ROUNDS).await, count(&pool, SNAPSHOTS).await), (1, 30));
    // The same round again (a retry after a lost commit acknowledgement): nothing changes.
    assert_eq!(store::record(&pool, &round).await.unwrap(), Written::AlreadyStored);
    assert_eq!((count(&pool, ROUNDS).await, count(&pool, SNAPSHOTS).await), (1, 30));
    let apy: String = sqlx::query_scalar("SELECT apy::text FROM market_snapshots WHERE pool_id = $1")
        .bind(AAVE)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(apy, "5.35824", "stored exactly, as the source sent it");
    db.cleanup().await;
}

#[tokio::test]
async fn hostile_entries_are_refused_by_name_and_the_counts_stored() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Board::default();
    let (_, ports) = ports(vec![Step::Body(HOSTILE.to_vec())]);
    let round = fetch_round(&ports, &board, &settings()).await;
    store_round(&pool, &board, &round, &settings()).await.unwrap();
    let (accepted, refusals): (i32, serde_json::Value) =
        sqlx::query_as("SELECT accepted, refusals FROM market_rounds").fetch_one(&pool).await.unwrap();
    assert_eq!(accepted, 1);
    assert_eq!(refusals["apy_implausible"], 1);
    assert_eq!(refusals["apy_negative"], 1);
    assert_eq!(refusals["duplicate_pool"], 2);
    assert_eq!(refusals["pool_id_invalid"], 2);
    let ids: Vec<String> = sqlx::query_scalar("SELECT pool_id FROM market_snapshots").fetch_all(&pool).await.unwrap();
    assert_eq!(ids, ["aaaaaaaa-0000-0000-0000-000000000001"]);
    db.cleanup().await;
}

#[tokio::test]
async fn a_down_source_stores_a_down_round_and_no_snapshot_and_keeps_the_last_good() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Board::default();
    let (source, ports) = ports(vec![Step::Body(RECORDED.to_vec())]);
    let good = fetch_round(&ports, &board, &settings()).await;
    store_round(&pool, &board, &good, &settings()).await.unwrap();
    for (step, code) in [
        (Step::Fail(FetchError::Status(503)), "http_status"),
        (Step::Body(b"<html>busy</html>".to_vec()), "malformed_body"),
        (Step::Body(body(&[])), "no_markets"),
        (Step::Body(body(&[("bad pool", "4")])), "no_markets"),
    ] {
        source.script([step]);
        let round = fetch_round(&ports, &board, &settings()).await;
        assert_eq!(round.outcome, Outcome::Down(code));
        store_round(&pool, &board, &round, &settings()).await.unwrap();
        let stored: (String, Option<String>, i32) = sqlx::query_as(
            "SELECT outcome, error_code, accepted FROM market_rounds WHERE round_id = $1",
        )
        .bind(round.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, ("down".to_owned(), Some(code.to_owned()), 0));
        assert_eq!(count(&pool, SNAPSHOTS).await, 30, "{code}: no snapshot invented");
        let view = board.view();
        let s = &view.sources["defillama"];
        assert!(!s.last.ok);
        assert_eq!(s.last_good_at, Some(good.observed_at));
        assert!(view.markets.iter().all(|m| m.round_id == good.id), "{code}: the board shows the last good round");
    }
    db.cleanup().await;
}

#[tokio::test]
async fn an_apy_spike_over_the_moving_average_is_flagged_and_stored_flagged() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Board::default();
    let (source, ports) = ports(vec![]);
    for apy in ["4", "4", "4", "12", "4", "4", "40"] {
        source.script([Step::Body(body(&[(AAVE, apy)]))]);
        let round = fetch_round(&ports, &board, &settings()).await;
        store_round(&pool, &board, &round, &settings()).await.unwrap();
    }
    let flags: Vec<(String, Vec<String>)> = sqlx::query_as(
        "SELECT apy::text, flags FROM market_snapshots ORDER BY observed_at, round_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let flagged: Vec<&str> = flags.iter().filter(|(_, f)| f == &["apy_spike"]).map(|(a, _)| a.as_str()).collect();
    assert_eq!(flagged, ["40"], "12 is exactly 3x the average of 4s, not over it; 40 is over 3 x 32/6");
    db.cleanup().await;
}

#[tokio::test]
async fn a_spike_is_judged_after_a_restart_from_the_stored_history() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Board::default();
    let (source, ports) = ports(vec![]);
    for apy in ["4", "4", "4"] {
        source.script([Step::Body(body(&[(AAVE, apy)]))]);
        let round = fetch_round(&ports, &board, &settings()).await;
        store_round(&pool, &board, &round, &settings()).await.unwrap();
    }
    let restarted = Board::default();
    restarted.load(&pool).await.unwrap();
    source.script([Step::Body(body(&[(AAVE, "13")]))]);
    let round = fetch_round(&ports, &restarted, &settings()).await;
    let Outcome::Ok(s) = &round.outcome else { panic!("{round:?}") };
    assert_eq!(s[0].flags.len(), 1, "the moving average survives a restart");
    db.cleanup().await;
}

#[tokio::test]
async fn a_hanging_source_is_cut_at_the_worker_deadline_and_stored_down() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Board::default();
    let (_, ports) = ports(vec![Step::Hang]);
    let round = tokio::time::timeout(GUARD, fetch_round(&ports, &board, &settings())).await.unwrap();
    assert_eq!(round.outcome, Outcome::Down("timeout"));
    store_round(&pool, &board, &round, &settings()).await.unwrap();
    assert_eq!((count(&pool, ROUNDS).await, count(&pool, SNAPSHOTS).await), (1, 0));
    db.cleanup().await;
}

fn market(apy: &str) -> Snapshot {
    Snapshot {
        market: Market {
            pool_id: AAVE.into(),
            chain: "Base",
            protocol: "aave-v3",
            symbol: "USDC".into(),
            apy: apy.parse::<Decimal>().unwrap(),
            tvl_usd: Decimal::ONE,
        },
        flags: vec![],
        cross_check: CrossCheck::Unavailable,
    }
}

fn round_of(snapshots: Vec<Snapshot>) -> Round {
    let now = OffsetDateTime::now_utc();
    Round {
        id: uuid::Uuid::now_v7(),
        source: "defillama",
        started_at: now,
        observed_at: now,
        entries: 1,
        selected: 1,
        refusals: std::collections::BTreeMap::new(),
        outcome: Outcome::Ok(snapshots),
    }
}

#[tokio::test]
async fn a_failed_snapshot_write_leaves_no_part_of_its_round_and_the_database_refuses_implausible_rows() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    // Past the validation (as a bug would be): the database's CHECK refuses it, and the
    // round row written before it in the same transaction is gone too.
    let mut bad = market("4");
    bad.market.apy = "101".parse().unwrap();
    let round = round_of(vec![market("4"), bad]);
    let err = store::record(&pool, &round).await.unwrap_err();
    assert!(yieldagent_db::chain(&err).contains("market_snapshots_apy_check"), "{}", yieldagent_db::chain(&err));
    assert_eq!((count(&pool, ROUNDS).await, count(&pool, SNAPSHOTS).await), (0, 0));
    db.cleanup().await;
}

#[tokio::test]
async fn stored_history_is_append_only_for_the_runtime_role() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    store::record(&pool, &round_of(vec![market("4")])).await.unwrap();
    for sql in [
        "UPDATE market_snapshots SET apy = 99",
        "DELETE FROM market_snapshots",
        "UPDATE market_rounds SET outcome = 'down', error_code = 'x', accepted = 0",
        "DELETE FROM market_rounds",
        "TRUNCATE market_snapshots",
    ] {
        let err = sqlx::query(sql).execute(&pool).await.unwrap_err();
        let text = yieldagent_db::chain(&yieldagent_db::DbError::from(err));
        assert!(text.contains("append-only") || text.contains("permission denied"), "{sql}: {text}");
    }
    let apy: String = sqlx::query_scalar("SELECT apy::text FROM market_snapshots").fetch_one(&pool).await.unwrap();
    assert_eq!((apy.as_str(), count(&pool, ROUNDS).await), ("4", 1));
    db.cleanup().await;
}

#[tokio::test]
async fn an_unreachable_database_ends_the_round_after_its_bounded_attempts() {
    let settings = Settings { write_deadline: Duration::from_millis(200), ..settings() };
    let dead = yieldagent_db::connect_lazy(
        "postgres://yieldagent_app:x@127.0.0.1:1/none",
        &yieldagent_db::PoolSettings { acquire_timeout: Duration::from_millis(100), ..Default::default() },
    )
    .unwrap();
    let board = Board::default();
    let r = tokio::time::timeout(GUARD, store_round(&dead, &board, &round_of(vec![market("4")]), &settings))
        .await
        .unwrap();
    assert!(matches!(r, Err(RoundError::NotStored { attempts: 3, .. })), "{r:?}");
    assert!(board.view().sources.is_empty(), "an unstored round never reaches the board");
}

#[tokio::test]
async fn the_board_rebuilt_after_a_crash_equals_the_board_before() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Board::default();
    let (source, ports) = ports(vec![Step::Body(RECORDED.to_vec())]);
    let round = fetch_round(&ports, &board, &settings()).await;
    store_round(&pool, &board, &round, &settings()).await.unwrap();
    source.script([Step::Fail(FetchError::Timeout)]);
    let down = fetch_round(&ports, &board, &settings()).await;
    store_round(&pool, &board, &down, &settings()).await.unwrap();
    let rebuilt = Board::default();
    rebuilt.load(&pool).await.unwrap();
    let (a, b) = (board.view(), rebuilt.view());
    assert_eq!(a.markets, b.markets);
    assert_eq!(a.sources["defillama"].last_good_at, b.sources["defillama"].last_good_at);
    assert_eq!(b.sources["defillama"].last.error_code.as_deref(), Some("timeout"));
    db.cleanup().await;
}
