//! A validated market and the named reasons a source entry is refused. Every field of a
//! source entry passes [`validate`]; nothing the source sends reaches storage otherwise.

use rust_decimal::Decimal;
use serde::Serialize;

use crate::decimal::{DecimalError, parse_exact};

/// The one chain M1 reads (`END_GOAL`: Base only).
pub const CHAIN: &str = "Base";
/// The allowlisted lending markets, by the source's project name (`END_GOAL`: Aave v3,
/// Morpho Blue, Compound v3 on Base). An entry of any other project is not selected.
pub const PROTOCOLS: [&str; 3] = ["aave-v3", "compound-v3", "morpho-blue"];
/// The stablecoin M1 tracks; an entry whose symbol does not end in it is not selected.
pub const ASSET: &str = "USDC";
/// Highest plausible stablecoin lending APY, in percent. Above it the entry is refused
/// `apy_implausible`: no lending market pays it, and a source that says so is wrong.
pub const MAX_APY_PERCENT: Decimal = Decimal::from_parts(100, 0, 0, false, 0);
/// Largest plausible TVL in USD (a trillion); above it, `tvl_implausible`.
pub const MAX_TVL_USD: Decimal = Decimal::from_parts(3_567_587_328, 232, 0, false, 0);
/// Longest pool id and symbol kept.
pub const MAX_ID_LEN: usize = 64;
pub const MAX_SYMBOL_LEN: usize = 32;

/// Why one source entry was refused. The code is stable: it is stored and served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    PoolIdMissing,
    PoolIdInvalid,
    ProtocolMissing,
    ChainMissing,
    SymbolMissing,
    SymbolInvalid,
    ApyMissing,
    ApyNotNumber,
    ApyInexact,
    ApyNegative,
    ApyImplausible,
    TvlMissing,
    TvlNotNumber,
    TvlInexact,
    TvlNegative,
    TvlImplausible,
    DuplicatePool,
}

impl Refusal {
    pub const ALL: [Self; 17] = [
        Self::PoolIdMissing,
        Self::PoolIdInvalid,
        Self::ProtocolMissing,
        Self::ChainMissing,
        Self::SymbolMissing,
        Self::SymbolInvalid,
        Self::ApyMissing,
        Self::ApyNotNumber,
        Self::ApyInexact,
        Self::ApyNegative,
        Self::ApyImplausible,
        Self::TvlMissing,
        Self::TvlNotNumber,
        Self::TvlInexact,
        Self::TvlNegative,
        Self::TvlImplausible,
        Self::DuplicatePool,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::PoolIdMissing => "pool_id_missing",
            Self::PoolIdInvalid => "pool_id_invalid",
            Self::ProtocolMissing => "protocol_missing",
            Self::ChainMissing => "chain_missing",
            Self::SymbolMissing => "symbol_missing",
            Self::SymbolInvalid => "symbol_invalid",
            Self::ApyMissing => "apy_missing",
            Self::ApyNotNumber => "apy_not_number",
            Self::ApyInexact => "apy_inexact",
            Self::ApyNegative => "apy_negative",
            Self::ApyImplausible => "apy_implausible",
            Self::TvlMissing => "tvl_missing",
            Self::TvlNotNumber => "tvl_not_number",
            Self::TvlInexact => "tvl_inexact",
            Self::TvlNegative => "tvl_negative",
            Self::TvlImplausible => "tvl_implausible",
            Self::DuplicatePool => "duplicate_pool",
        }
    }
}

/// One market as stored: every field validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Market {
    pub pool_id: String,
    pub chain: &'static str,
    pub protocol: &'static str,
    pub symbol: String,
    /// Percent per year, exact.
    pub apy: Decimal,
    pub tvl_usd: Decimal,
}

/// The raw fields of one selected entry, as the source sent them. Numbers are the raw
/// JSON tokens (`None` when absent or `null`).
#[derive(Debug, Clone, Copy)]
pub struct RawEntry<'a> {
    pub pool_id: Option<&'a str>,
    pub chain: Option<&'a str>,
    pub protocol: Option<&'a str>,
    pub symbol: Option<&'a str>,
    pub apy: Option<&'a str>,
    pub tvl_usd: Option<&'a str>,
}

/// Is this entry one M1 tracks? Chain, protocol and asset are matched exactly against the
/// allowlist; everything else in the source is ignored, not refused.
pub fn selected(raw: &RawEntry<'_>) -> Option<&'static str> {
    if raw.chain != Some(CHAIN) {
        return None;
    }
    let protocol = PROTOCOLS.into_iter().find(|p| raw.protocol == Some(*p))?;
    raw.symbol
        .filter(|s| s.to_ascii_uppercase().ends_with(ASSET))
        .map(|_| protocol)
}

fn pool_id_ok(id: &str) -> bool {
    !id.is_empty()
        // LIMIT(id-length)
        && id.len() <= MAX_ID_LEN
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') // LIMIT(pool-id-charset)
}

fn symbol_ok(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol.len() <= MAX_SYMBOL_LEN
        && symbol
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+' | b'_')) // LIMIT(symbol-charset)
}

fn number(
    token: Option<&str>,
    [missing, not_number, inexact, negative, implausible]: [Refusal; 5],
    max: Decimal,
) -> Result<Decimal, Refusal> {
    let value = match parse_exact(token.ok_or(missing)?) {
        Ok(v) => v,
        Err(DecimalError::NotANumber) => return Err(not_number),
        Err(DecimalError::Inexact | DecimalError::TooLong) => return Err(inexact),
    };
    if value.is_sign_negative() && !value.is_zero() {
        return Err(negative); // LIMIT(non-negative)
    }
    if value > max {
        return Err(implausible); // LIMIT(plausible-ceiling)
    }
    Ok(value)
}

/// Validates every field of one selected entry; the first failing field names the refusal.
pub fn validate(raw: &RawEntry<'_>) -> Result<Market, Refusal> {
    let pool_id = raw.pool_id.ok_or(Refusal::PoolIdMissing)?;
    if !pool_id_ok(pool_id) {
        return Err(Refusal::PoolIdInvalid);
    }
    raw.chain.ok_or(Refusal::ChainMissing)?;
    let protocol = selected(raw).ok_or(Refusal::ProtocolMissing)?;
    let symbol = raw.symbol.ok_or(Refusal::SymbolMissing)?;
    if !symbol_ok(symbol) {
        return Err(Refusal::SymbolInvalid);
    }
    let apy = number(
        raw.apy,
        [
            Refusal::ApyMissing,
            Refusal::ApyNotNumber,
            Refusal::ApyInexact,
            Refusal::ApyNegative,
            Refusal::ApyImplausible,
        ],
        MAX_APY_PERCENT,
    )?;
    let tvl_usd = number(
        raw.tvl_usd,
        [
            Refusal::TvlMissing,
            Refusal::TvlNotNumber,
            Refusal::TvlInexact,
            Refusal::TvlNegative,
            Refusal::TvlImplausible,
        ],
        MAX_TVL_USD,
    )?;
    Ok(Market {
        pool_id: pool_id.to_owned(),
        chain: CHAIN,
        protocol,
        symbol: symbol.to_owned(),
        apy,
        tvl_usd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> RawEntry<'static> {
        RawEntry {
            pool_id: Some("7e0661bf-8cf3-45e6-9424-31916d4c7b84"),
            chain: Some("Base"),
            protocol: Some("aave-v3"),
            symbol: Some("USDC"),
            apy: Some("4.21"),
            tvl_usd: Some("123456789"),
        }
    }

    #[test]
    fn the_ceilings_are_the_documented_numbers() {
        assert_eq!(MAX_APY_PERCENT.to_string(), "100");
        assert_eq!(MAX_TVL_USD.to_string(), "1000000000000");
    }

    #[test]
    fn a_good_entry_validates_exactly() {
        let m = validate(&good()).unwrap();
        assert_eq!(m.apy.to_string(), "4.21");
        assert_eq!(m.protocol, "aave-v3");
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one table of cases")]
    fn every_field_has_its_named_refusal() {
        let cases: [(RawEntry<'static>, Refusal); 14] = [
            (
                RawEntry {
                    pool_id: None,
                    ..good()
                },
                Refusal::PoolIdMissing,
            ),
            (
                RawEntry {
                    pool_id: Some("a b"),
                    ..good()
                },
                Refusal::PoolIdInvalid,
            ),
            (
                RawEntry {
                    pool_id: Some("<script>"),
                    ..good()
                },
                Refusal::PoolIdInvalid,
            ),
            (
                RawEntry {
                    symbol: Some("x\"><USDC"),
                    ..good()
                },
                Refusal::SymbolInvalid,
            ),
            (
                RawEntry {
                    apy: None,
                    ..good()
                },
                Refusal::ApyMissing,
            ),
            (
                RawEntry {
                    apy: Some("\"4.2\""),
                    ..good()
                },
                Refusal::ApyNotNumber,
            ),
            (
                RawEntry {
                    apy: Some("1e-29"),
                    ..good()
                },
                Refusal::ApyInexact,
            ),
            (
                RawEntry {
                    apy: Some("-0.01"),
                    ..good()
                },
                Refusal::ApyNegative,
            ),
            (
                RawEntry {
                    apy: Some("448575.06523"),
                    ..good()
                },
                Refusal::ApyImplausible,
            ),
            (
                RawEntry {
                    tvl_usd: None,
                    ..good()
                },
                Refusal::TvlMissing,
            ),
            (
                RawEntry {
                    tvl_usd: Some("true"),
                    ..good()
                },
                Refusal::TvlNotNumber,
            ),
            (
                RawEntry {
                    tvl_usd: Some("1e29"),
                    ..good()
                },
                Refusal::TvlInexact,
            ),
            (
                RawEntry {
                    tvl_usd: Some("-1"),
                    ..good()
                },
                Refusal::TvlNegative,
            ),
            (
                RawEntry {
                    tvl_usd: Some("1000000000001"),
                    ..good()
                },
                Refusal::TvlImplausible,
            ),
        ];
        for (raw, want) in cases {
            assert_eq!(validate(&raw), Err(want), "{raw:?}");
        }
    }

    #[test]
    fn the_boundaries_are_inclusive() {
        assert!(
            validate(&RawEntry {
                apy: Some("100"),
                ..good()
            })
            .is_ok()
        );
        assert!(
            validate(&RawEntry {
                apy: Some("0"),
                ..good()
            })
            .is_ok()
        );
        assert!(
            validate(&RawEntry {
                apy: Some("-0"),
                ..good()
            })
            .is_ok()
        );
        assert!(
            validate(&RawEntry {
                tvl_usd: Some("1000000000000"),
                ..good()
            })
            .is_ok()
        );
        assert!(
            validate(&RawEntry {
                pool_id: Some(&"a".repeat(MAX_ID_LEN)),
                ..good()
            })
            .is_ok()
        );
        assert_eq!(
            validate(&RawEntry {
                pool_id: Some(&"a".repeat(MAX_ID_LEN + 1)),
                ..good()
            }),
            Err(Refusal::PoolIdInvalid)
        );
    }

    #[test]
    fn selection_matches_the_allowlist_exactly() {
        assert_eq!(selected(&good()), Some("aave-v3"));
        assert_eq!(
            selected(&RawEntry {
                chain: Some("base"),
                ..good()
            }),
            None
        );
        assert_eq!(
            selected(&RawEntry {
                chain: Some("Ethereum"),
                ..good()
            }),
            None
        );
        assert_eq!(
            selected(&RawEntry {
                protocol: Some("aave-v3x"),
                ..good()
            }),
            None
        );
        assert_eq!(
            selected(&RawEntry {
                symbol: Some("WETH"),
                ..good()
            }),
            None
        );
        assert_eq!(
            selected(&RawEntry {
                symbol: Some("steakUSDC"),
                ..good()
            }),
            Some("aave-v3")
        );
    }

    #[test]
    fn codes_are_unique_and_snake_case() {
        let mut seen = std::collections::BTreeSet::new();
        for r in Refusal::ALL {
            assert!(seen.insert(r.code()));
            assert!(
                r.code()
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_')
            );
            assert_eq!(serde_json::to_value(r).unwrap(), r.code());
        }
    }
}
