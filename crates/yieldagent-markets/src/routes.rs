//! The read API. Market data is public, so the reads need no account; they run behind the
//! server's in-flight gate. JSON under `/v1/markets`; `/markets` is a plain, unstyled HTML
//! list of the same data (no visual design: that is the maintainer's).

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use sqlx::PgPool;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use yieldagent_data::screen::{Freshness, freshness, source_state};
use yieldagent_http::stack::error;

use crate::board::{Board, View};
use crate::store::{self, MAX_HISTORY, Stored};

/// Default history page.
pub const DEFAULT_HISTORY: u32 = 100;

#[derive(Debug, Clone)]
pub struct Markets {
    pub board: Arc<Board>,
    pub pool: PgPool,
    /// Case 5's freshness bound.
    pub max_age: Duration,
}

/// `/markets` and `/markets/{pool_id}/history`, to nest under `/v1`.
pub fn api(state: Markets) -> Router {
    Router::new()
        .route("/markets", get(list))
        .route("/markets/{pool_id}/history", get(history))
        .with_state(state)
}

/// `/markets`, the plain HTML list.
pub fn page(state: Markets) -> Router {
    Router::new().route("/markets", get(html)).with_state(state)
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

/// Why a market may not be planned from (Case 5). M1 has no planner and no chain reads, so
/// every market carries at least `cross_check_unavailable`.
fn suspect(m: &Stored, now: OffsetDateTime, max_age: Duration) -> Vec<String> {
    let mut why = Vec::new();
    if freshness(m.observed_at, now, max_age) == Freshness::Stale {
        why.push("stale".to_owned());
    }
    why.extend(m.flags.iter().cloned());
    if m.cross_check != "agrees" {
        why.push(format!("cross_check_{}", m.cross_check));
    }
    why
}

fn market_json(m: &Stored, now: OffsetDateTime, max_age: Duration) -> Value {
    let why = suspect(m, now, max_age);
    json!({
        "source": m.source,
        "pool_id": m.pool_id,
        "chain": "Base",
        "protocol": m.protocol,
        "symbol": m.symbol,
        "apy": m.apy.to_string(),
        "tvl_usd": m.tvl_usd.to_string(),
        "observed_at": rfc3339(m.observed_at),
        "age_seconds": (now - m.observed_at).whole_seconds().max(0),
        "freshness": freshness(m.observed_at, now, max_age),
        "flags": m.flags,
        "cross_check": m.cross_check,
        "plannable": why.is_empty(),
        "suspect": why,
    })
}

fn board_json(view: &View, now: OffsetDateTime, max_age: Duration) -> Value {
    let sources: Vec<Value> = view
        .sources
        .iter()
        .map(|(name, s)| {
            json!({
                "name": name,
                "state": source_state(Some(s.last.ok), s.last_good_at, now, max_age),
                "last_round_at": rfc3339(s.last.observed_at),
                "last_good_at": s.last_good_at.map(rfc3339),
                "last_error": s.last.error_code,
                "entries": s.last.entries,
                "selected": s.last.selected,
                "accepted": s.last.accepted,
                "refusals": s.last.refusals,
            })
        })
        .collect();
    let markets: Vec<Value> = view
        .markets
        .iter()
        .map(|m| market_json(m, now, max_age))
        .collect();
    json!({
        "as_of": rfc3339(now),
        "max_age_seconds": max_age.whole_seconds(),
        "sources": sources,
        "markets": markets,
    })
}

async fn list(State(state): State<Markets>) -> Response {
    let view = state.board.view();
    axum::Json(board_json(&view, OffsetDateTime::now_utc(), state.max_age)).into_response()
}

/// The history query: `limit=1..=500`, `before=<unix seconds>`; nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryQuery {
    pub limit: u32,
    pub before: Option<OffsetDateTime>,
}

/// A refused query parameter: `(code, message)`.
pub type QueryRefusal = (&'static str, &'static str);

pub fn parse_history_query(raw: Option<&str>) -> Result<HistoryQuery, QueryRefusal> {
    let mut q = HistoryQuery {
        limit: DEFAULT_HISTORY,
        before: None,
    };
    let raw = raw.unwrap_or("");
    if raw.len() > 256 {
        return Err(("query_too_long", "the query string is too long"));
    }
    for pair in raw.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let digits =
            !value.is_empty() && value.len() <= 12 && value.bytes().all(|b| b.is_ascii_digit());
        match key {
            "limit" => {
                let n = value
                    .parse::<u32>()
                    .ok()
                    .filter(|n| digits && (1..=MAX_HISTORY).contains(n)); // LIMIT(history-limit)
                q.limit = n.ok_or(("invalid_limit", "limit must be 1..=500"))?;
            }
            "before" => {
                let t = value
                    .parse::<i64>()
                    .ok()
                    .filter(|_| digits)
                    .and_then(|s| OffsetDateTime::from_unix_timestamp(s).ok());
                q.before = Some(t.ok_or(("invalid_before", "before must be unix seconds"))?);
            }
            _ => return Err(("unknown_parameter", "only limit and before are accepted")),
        }
    }
    Ok(q)
}

fn pool_id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

async fn history(
    State(state): State<Markets>,
    Path(pool_id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Response {
    if !pool_id_ok(&pool_id) {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_pool_id",
            "a pool id is 1..=64 of [A-Za-z0-9-]",
        );
    }
    let q = match parse_history_query(raw.as_deref()) {
        Ok(q) => q,
        Err((code, message)) => return error(StatusCode::BAD_REQUEST, code, message),
    };
    match store::history(&state.pool, &pool_id, q.before, q.limit).await {
        Ok(rows) => {
            let now = OffsetDateTime::now_utc();
            let snapshots: Vec<Value> = rows
                .iter()
                .map(|m| {
                    let mut v = market_json(m, now, state.max_age);
                    if let Some(o) = v.as_object_mut() {
                        o.insert("round_id".into(), json!(m.round_id));
                    }
                    v
                })
                .collect();
            axum::Json(json!({ "pool_id": pool_id, "snapshots": snapshots })).into_response()
        }
        Err(e) => {
            tracing::warn!(error = %yieldagent_db::chain(&e), "market history read failed");
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                "the history is unavailable; retry shortly",
            )
        }
    }
}

/// Escapes text for an HTML text node or a quoted attribute.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"), // LIMIT(html-escape)
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The plain list: one `<li>` per source and per market, no styles, no scripts.
pub fn render(view: &View, now: OffsetDateTime, max_age: Duration) -> String {
    let e = escape;
    let mut page = String::from(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Markets</title></head><body>\n<h1>Markets</h1>\n<h2>Sources</h2>\n<ul>\n",
    );
    for (name, s) in &view.sources {
        let state =
            serde_json::to_value(source_state(Some(s.last.ok), s.last_good_at, now, max_age))
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
        let _ = writeln!(
            page,
            "<li>{}: {}; last round {}; last good {}; error {}; accepted {}, refused {}</li>",
            e(name),
            e(&state),
            e(&rfc3339(s.last.observed_at)),
            e(&s.last_good_at.map_or_else(|| "never".into(), rfc3339)),
            e(s.last.error_code.as_deref().unwrap_or("none")),
            s.last.accepted,
            s.last.refusals.values().sum::<u32>(),
        );
    }
    page.push_str("</ul>\n<h2>Markets</h2>\n<ul>\n");
    if view.markets.is_empty() {
        page.push_str("<li>No market data stored.</li>\n");
    }
    for m in &view.markets {
        let why = suspect(m, now, max_age);
        let _ = writeln!(
            page,
            "<li>{} {} {}: APY {}%, TVL {} USD, observed {} ({} s ago), pool {}; suspect: {}</li>",
            e(&m.source),
            e(&m.protocol),
            e(&m.symbol),
            e(&m.apy.to_string()),
            e(&m.tvl_usd.to_string()),
            e(&rfc3339(m.observed_at)),
            (now - m.observed_at).whole_seconds().max(0),
            e(&m.pool_id),
            e(&if why.is_empty() {
                "none".to_owned()
            } else {
                why.join(", ")
            }),
        );
    }
    page.push_str("</ul>\n</body></html>\n");
    page
}

async fn html(State(state): State<Markets>) -> Response {
    let view = state.board.view();
    let body = render(&view, OffsetDateTime::now_utc(), state.max_age);
    let mut response = body.into_response();
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
