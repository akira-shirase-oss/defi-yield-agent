//! The read API against PostgreSQL: the JSON a dashboard reads, staleness judged at read
//! time, the history's bounded and named query refusals, and the plain HTML list escaped.
#![expect(clippy::unwrap_used, reason = "integration test helpers")]

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{AAVE, RECORDED, body, ports, settings};
use http_body_util::BodyExt;
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use yieldagent_data::recorded::Step;
use yieldagent_data::screen::DEFAULT_MAX_AGE;
use yieldagent_db::testing::{Role, TestDb};
use yieldagent_markets::Board;
use yieldagent_markets::board::{SourceView, View};
use yieldagent_markets::routes::{self, Markets, render};
use yieldagent_markets::store::{self, LastRound, Stored};
use yieldagent_markets::worker::{fetch_round, store_round};

fn app(state: Markets) -> Router {
    Router::new().nest("/v1", routes::api(state.clone())).merge(routes::page(state))
}

async fn get(app: &Router, uri: &str) -> (StatusCode, String, Option<String>) {
    let r = app.clone().oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    let status = r.status();
    let csp = r.headers().get("content-security-policy").map(|v| v.to_str().unwrap().to_owned());
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap(), csp)
}

async fn json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let (s, b, _) = get(app, uri).await;
    (s, serde_json::from_str(&b).unwrap())
}

#[tokio::test]
async fn the_board_serves_exact_apys_and_marks_every_market_unplannable_in_m1() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Arc::new(Board::default());
    let (_, p) = ports(vec![Step::Body(RECORDED.to_vec())]);
    let round = fetch_round(&p, &board, &settings()).await;
    store_round(&pool, &board, &round, &settings()).await.unwrap();
    let app = app(Markets { board, pool: pool.clone(), max_age: DEFAULT_MAX_AGE });
    let (status, v) = json(&app, "/v1/markets").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["sources"][0]["name"], "defillama");
    assert_eq!(v["sources"][0]["state"], "fresh");
    assert_eq!(v["markets"].as_array().unwrap().len(), 30);
    let aave = v["markets"].as_array().unwrap().iter().find(|m| m["pool_id"] == AAVE).unwrap();
    assert_eq!(aave["apy"], "5.35824", "a string, exact: never a float");
    assert_eq!(aave["freshness"], "fresh");
    assert_eq!(aave["plannable"], false);
    assert_eq!(aave["suspect"], serde_json::json!(["cross_check_unavailable"]));
    db.cleanup().await;
}

#[tokio::test]
async fn an_old_snapshot_is_served_stale_and_its_source_stale_never_refreshed() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Arc::new(Board::default());
    let (_, p) = ports(vec![Step::Body(body(&[(AAVE, "4")]))]);
    let mut round = fetch_round(&p, &board, &settings()).await;
    round.started_at -= Duration::hours(1);
    round.observed_at -= Duration::hours(1);
    store::record(&pool, &round).await.unwrap();
    board.load(&pool).await.unwrap();
    let app = app(Markets { board, pool: pool.clone(), max_age: DEFAULT_MAX_AGE });
    let (_, v) = json(&app, "/v1/markets").await;
    assert_eq!(v["sources"][0]["state"], "stale");
    let m = &v["markets"][0];
    assert_eq!((m["freshness"].as_str(), m["apy"].as_str()), (Some("stale"), Some("4")));
    assert_eq!(m["suspect"], serde_json::json!(["stale", "cross_check_unavailable"]));
    assert!(m["age_seconds"].as_i64().unwrap() >= 3600);
    db.cleanup().await;
}

#[tokio::test]
async fn history_is_newest_first_bounded_and_refuses_bad_queries_by_name() {
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let board = Arc::new(Board::default());
    let (source, p) = ports(vec![]);
    for apy in ["4", "5", "6"] {
        source.script([Step::Body(body(&[(AAVE, apy)]))]);
        let round = fetch_round(&p, &board, &settings()).await;
        store_round(&pool, &board, &round, &settings()).await.unwrap();
    }
    let app = app(Markets { board, pool: pool.clone(), max_age: DEFAULT_MAX_AGE });
    let (status, v) = json(&app, &format!("/v1/markets/{AAVE}/history?limit=2")).await;
    assert_eq!(status, StatusCode::OK);
    let apys: Vec<_> = v["snapshots"].as_array().unwrap().iter().map(|s| s["apy"].as_str().unwrap()).collect();
    assert_eq!(apys, ["6", "5"]);
    let before = OffsetDateTime::now_utc().unix_timestamp() + 60;
    let (_, v) = json(&app, &format!("/v1/markets/{AAVE}/history?before={before}")).await;
    assert_eq!(v["snapshots"].as_array().unwrap().len(), 3);
    for (uri, code) in [
        (format!("/v1/markets/{AAVE}/history?limit=501"), "invalid_limit"),
        (format!("/v1/markets/{AAVE}/history?limit=0"), "invalid_limit"),
        (format!("/v1/markets/{AAVE}/history?limit=-1"), "invalid_limit"),
        (format!("/v1/markets/{AAVE}/history?limit=99999999999999999999"), "invalid_limit"),
        (format!("/v1/markets/{AAVE}/history?before=yesterday"), "invalid_before"),
        (format!("/v1/markets/{AAVE}/history?limit=1;DROP%20TABLE"), "invalid_limit"),
        (format!("/v1/markets/{AAVE}/history?pool_id=x"), "unknown_parameter"),
        (format!("/v1/markets/{AAVE}/history?{}", "a=1&".repeat(100)), "query_too_long"),
        ("/v1/markets/x'%20OR%20'1'='1/history".to_owned(), "invalid_pool_id"),
        (format!("/v1/markets/{}/history", "a".repeat(65)), "invalid_pool_id"),
    ] {
        let (status, v) = json(&app, &uri).await;
        assert_eq!((status, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some(code)), "{uri}");
    }
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM market_snapshots").fetch_one(&pool).await.unwrap();
    assert_eq!(stored, 3, "nothing a query sends changes the store");
    db.cleanup().await;
}

#[tokio::test]
async fn the_html_list_is_plain_and_escapes_everything_it_shows() {
    let now = OffsetDateTime::now_utc();
    let evil = "<script>alert(1)</script>\"'&";
    let mut view = View::default();
    view.sources.insert(
        evil.to_owned(),
        SourceView {
            last: LastRound {
                source: evil.to_owned(),
                observed_at: now,
                ok: false,
                error_code: Some(evil.to_owned()),
                entries: 0,
                selected: 0,
                accepted: 0,
                refusals: std::collections::BTreeMap::new(),
            },
            last_good_at: None,
        },
    );
    view.markets.push(Stored {
        round_id: uuid::Uuid::nil(),
        source: evil.to_owned(),
        pool_id: evil.to_owned(),
        protocol: evil.to_owned(),
        symbol: evil.to_owned(),
        apy: "4.5".parse().unwrap(),
        tvl_usd: "1".parse().unwrap(),
        observed_at: now,
        flags: vec![evil.to_owned()],
        cross_check: "unavailable".to_owned(),
    });
    let page = render(&view, now, DEFAULT_MAX_AGE);
    assert!(!page.contains("<script"), "{page}");
    assert!(!page.contains("\"'&<"), "{page}");
    assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;&quot;&#39;&amp;"));
    assert!(!page.contains("style") && !page.contains("class="), "no visual design");
    let db = TestDb::new().await;
    let pool = db.connect_as(Role::Runtime).await;
    let app = app(Markets { board: Arc::new(Board::default()), pool, max_age: DEFAULT_MAX_AGE });
    let (status, page, csp) = get(&app, "/markets").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("No market data stored."));
    assert_eq!(csp.as_deref(), Some("default-src 'none'"));
    db.cleanup().await;
}
