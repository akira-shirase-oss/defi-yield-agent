//! Case 5's data checks, pure: an APY spike over its moving average, a snapshot past the
//! freshness bound, the chain cross-check. M1 has no proposals; these produce the flags a
//! later milestone's planner refuses to plan from, and the dashboard shows.

use rust_decimal::Decimal;
use serde::Serialize;
use time::{Duration, OffsetDateTime};

use crate::market::Market;

/// `END_GOAL` Case 5: an APY above this multiple of its moving average is suspect.
pub const SPIKE_MULTIPLIER: Decimal = Decimal::from_parts(3, 0, 0, false, 0);
/// Rounds in the moving average.
pub const AVERAGE_WINDOW: usize = 12;
/// Fewest prior rounds before a spike can be judged; with fewer, no flag.
pub const MIN_HISTORY: usize = 3;
/// `END_GOAL` Case 5: a snapshot older than this is suspect.
pub const DEFAULT_MAX_AGE: Duration = Duration::minutes(15);

/// A flag on one stored market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    /// The APY is more than [`SPIKE_MULTIPLIER`] × its moving average.
    ApySpike,
}

impl Flag {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ApySpike => "apy_spike",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        (code == "apy_spike").then_some(Self::ApySpike)
    }
}

/// The on-chain cross-check of a source's APY (Case 5). M1 reads no chain: the port is a
/// stub that always answers `Unavailable`, which is stored, so nothing later mistakes an
/// unchecked APY for a checked one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossCheck {
    Unavailable,
    Agrees,
    Disagrees,
}

impl CrossCheck {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Agrees => "agrees",
            Self::Disagrees => "disagrees",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "unavailable" => Some(Self::Unavailable),
            "agrees" => Some(Self::Agrees),
            "disagrees" => Some(Self::Disagrees),
            _ => None,
        }
    }
}

/// The chain-read port of Case 5. M1's only adapter is [`NoChainReads`].
pub trait ChainRates: Send + Sync {
    fn cross_check(&self, market: &Market) -> CrossCheck;
}

/// M1: no chain reads at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoChainReads;

impl ChainRates for NoChainReads {
    fn cross_check(&self, _: &Market) -> CrossCheck {
        CrossCheck::Unavailable
    }
}

/// Is `apy` a spike over `history` (oldest first, prior rounds only)? Exact decimals.
pub fn is_spike(apy: Decimal, history: &[Decimal]) -> bool {
    let window = &history[history.len().saturating_sub(AVERAGE_WINDOW)..];
    if window.len() < MIN_HISTORY {
        return false;
    }
    let sum: Decimal = window.iter().copied().sum();
    // apy > k × (sum / n)  ⇔  apy × n > k × sum, with no division to round.
    let n = Decimal::from(window.len());
    apy * n > SPIKE_MULTIPLIER * sum // LIMIT(apy-spike)
}

/// A snapshot's freshness at `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Fresh,
    /// Older than the bound: shown, never used.
    Stale,
}

pub fn freshness(observed_at: OffsetDateTime, now: OffsetDateTime, max_age: Duration) -> Freshness {
    if now - observed_at > max_age {
        Freshness::Stale // LIMIT(staleness)
    } else {
        Freshness::Fresh
    }
}

/// A source's state, from its last round and last good round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    /// Never read since the database was created.
    Unknown,
    /// The last round was good and within the freshness bound.
    Fresh,
    /// The last good round is older than the bound, whatever the last attempt was.
    Stale,
    /// The last round failed; the last good one (if any) is still within the bound.
    Down,
}

pub fn source_state(
    last_round_ok: Option<bool>,
    last_good_at: Option<OffsetDateTime>,
    now: OffsetDateTime,
    max_age: Duration,
) -> SourceState {
    match (last_round_ok, last_good_at) {
        (None, _) => SourceState::Unknown,
        (Some(_), Some(at)) if freshness(at, now, max_age) == Freshness::Stale => SourceState::Stale,
        (Some(true), Some(_)) => SourceState::Fresh,
        (Some(_), _) => SourceState::Down,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn a_spike_is_judged_against_the_moving_average_exactly() {
        let h = [d("4"), d("4"), d("4")];
        assert!(!is_spike(d("12"), &h), "exactly 3x is not over 3x");
        assert!(is_spike(d("12.0000001"), &h));
        assert!(!is_spike(d("1000"), &h[..2]), "too little history judges nothing");
        let mut long = vec![d("100"); 20];
        long.extend([d("1"); AVERAGE_WINDOW]);
        assert!(is_spike(d("3.5"), &long), "only the last window counts");
    }

    #[test]
    fn freshness_and_source_state_follow_the_bound() {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::days(1);
        let max = DEFAULT_MAX_AGE;
        assert_eq!(freshness(now - max, now, max), Freshness::Fresh);
        assert_eq!(freshness(now - max - Duration::SECOND, now, max), Freshness::Stale);
        assert_eq!(source_state(None, None, now, max), SourceState::Unknown);
        assert_eq!(source_state(Some(true), Some(now), now, max), SourceState::Fresh);
        assert_eq!(source_state(Some(false), Some(now), now, max), SourceState::Down);
        assert_eq!(source_state(Some(false), None, now, max), SourceState::Down);
        assert_eq!(source_state(Some(false), Some(now - Duration::hours(1)), now, max), SourceState::Stale);
        assert_eq!(source_state(Some(true), Some(now - Duration::hours(1)), now, max), SourceState::Stale);
    }

    #[test]
    fn the_m1_chain_port_never_claims_a_check() {
        let m = Market {
            pool_id: "p".into(),
            chain: "Base",
            protocol: "aave-v3",
            symbol: "USDC".into(),
            apy: d("4"),
            tvl_usd: d("1"),
        };
        assert_eq!(NoChainReads.cross_check(&m), CrossCheck::Unavailable);
    }
}
