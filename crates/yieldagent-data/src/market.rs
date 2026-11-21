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
/// The stablecoin M1 tracks; an entry of an allowlisted project whose symbol ends in it is
/// read, and refused by name unless its pool is pinned in [`ALLOWLIST`].
pub const ASSET: &str = "USDC";
/// Native USDC on Base: a pinned pool's `underlyingTokens` must be exactly this one token.
pub const USDC_BASE: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
/// The pools M1 tracks, by the source's pool id and the protocol it is pinned to. A symbol
/// is not identity: anyone can name a vault `...USDC`, so only these pools are kept.
pub const ALLOWLIST: [(&str, &str); 6] = [
    ("7e0661bf-8cf3-45e6-9424-31916d4c7b84", "aave-v3"),
    ("0c8567f8-ba5b-41ad-80de-00a71895eb19", "compound-v3"),
    ("28470287-2c5b-5744-bc13-b9f1c0190b7b", "morpho-blue"),
    ("a5fab52f-73fa-4f44-9c09-9af1eb20996c", "morpho-blue"),
    ("ba68527f-8ec2-4c55-827a-8f4673ae047c", "morpho-blue"),
    ("81ae8812-f04f-4f6e-9d71-ee5778f3a178", "morpho-blue"),
];
/// Longest raw `underlyingTokens` token read; a longer one is refused unread.
pub const MAX_UNDERLYING_LEN: usize = 128;
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
    PoolNotAllowlisted,
    UnderlyingNotUsdc,
    ApyBaseMissing,
    ApyBaseNotNumber,
    ApyBaseInexact,
    ApyBaseNegative,
    ApyBaseImplausible,
    ApyRewardNotNumber,
    ApyRewardInexact,
    ApyRewardNegative,
    ApyRewardImplausible,
    DuplicatePool,
}

impl Refusal {
    pub const ALL: [Self; 28] = [
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
        Self::PoolNotAllowlisted,
        Self::UnderlyingNotUsdc,
        Self::ApyBaseMissing,
        Self::ApyBaseNotNumber,
        Self::ApyBaseInexact,
        Self::ApyBaseNegative,
        Self::ApyBaseImplausible,
        Self::ApyRewardNotNumber,
        Self::ApyRewardInexact,
        Self::ApyRewardNegative,
        Self::ApyRewardImplausible,
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
            Self::PoolNotAllowlisted => "pool_not_allowlisted",
            Self::UnderlyingNotUsdc => "underlying_not_usdc",
            Self::ApyBaseMissing => "apy_base_missing",
            Self::ApyBaseNotNumber => "apy_base_not_number",
            Self::ApyBaseInexact => "apy_base_inexact",
            Self::ApyBaseNegative => "apy_base_negative",
            Self::ApyBaseImplausible => "apy_base_implausible",
            Self::ApyRewardNotNumber => "apy_reward_not_number",
            Self::ApyRewardInexact => "apy_reward_inexact",
            Self::ApyRewardNegative => "apy_reward_negative",
            Self::ApyRewardImplausible => "apy_reward_implausible",
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
    /// Percent per year, exact: the source's total, base plus reward.
    pub apy: Decimal,
    /// The lending rate alone, percent per year: what the spike check reads and plans use.
    pub apy_base: Decimal,
    /// Incentives on top (token rewards), when the source reports them; never planned from.
    pub apy_reward: Option<Decimal>,
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
    pub apy_base: Option<&'a str>,
    pub apy_reward: Option<&'a str>,
    pub tvl_usd: Option<&'a str>,
    /// The raw JSON token of `underlyingTokens`.
    pub underlying: Option<&'a str>,
}

/// Is this entry read at all? A pinned pool id always is; so is a Base entry of an
/// allowlisted project whose symbol ends in [`ASSET`], to be refused by name unless pinned.
/// Everything else in the source is ignored, not refused.
pub fn selected(raw: &RawEntry<'_>) -> bool {
    if raw
        .pool_id
        .is_some_and(|id| ALLOWLIST.iter().any(|(p, _)| *p == id))
    {
        return true;
    }
    raw.chain == Some(CHAIN)
        && PROTOCOLS.iter().any(|p| raw.protocol == Some(*p))
        && raw
            .symbol
            .is_some_and(|s| s.to_ascii_uppercase().ends_with(ASSET))
}

/// The protocol a pool is pinned to, if it is pinned on [`CHAIN`] under that protocol.
fn pinned(pool_id: &str, chain: &str, protocol: &str) -> Option<&'static str> {
    if chain != CHAIN {
        return None;
    }
    ALLOWLIST
        .iter()
        .find(|(id, p)| *id == pool_id && *p == protocol) // LIMIT(pool-allowlist)
        .map(|(_, p)| *p)
}

/// `underlyingTokens` is exactly `[USDC_BASE]`, compared without case.
fn underlying_is_usdc(token: Option<&str>) -> bool {
    let Some(token) = token.filter(|t| t.len() <= MAX_UNDERLYING_LEN) else {
        return false;
    };
    serde_json::from_str::<[std::borrow::Cow<'_, str>; 1]>(token)
        .is_ok_and(|[t]| t.eq_ignore_ascii_case(USDC_BASE)) // LIMIT(underlying-usdc)
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
    let chain = raw.chain.ok_or(Refusal::ChainMissing)?;
    let protocol = raw.protocol.ok_or(Refusal::ProtocolMissing)?;
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
    let protocol = pinned(pool_id, chain, protocol).ok_or(Refusal::PoolNotAllowlisted)?;
    if !underlying_is_usdc(raw.underlying) {
        return Err(Refusal::UnderlyingNotUsdc);
    }
    let apy_base = number(
        raw.apy_base,
        [
            Refusal::ApyBaseMissing,
            Refusal::ApyBaseNotNumber,
            Refusal::ApyBaseInexact,
            Refusal::ApyBaseNegative,
            Refusal::ApyBaseImplausible,
        ],
        MAX_APY_PERCENT,
    )?;
    let apy_reward = raw
        .apy_reward
        .map(|r| {
            number(
                Some(r),
                [
                    Refusal::ApyRewardNotNumber,
                    Refusal::ApyRewardNotNumber,
                    Refusal::ApyRewardInexact,
                    Refusal::ApyRewardNegative,
                    Refusal::ApyRewardImplausible,
                ],
                MAX_APY_PERCENT,
            )
        })
        .transpose()?;
    Ok(Market {
        pool_id: pool_id.to_owned(),
        chain: CHAIN,
        protocol,
        symbol: symbol.to_owned(),
        apy,
        apy_base,
        apy_reward,
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
            apy_base: Some("4.21"),
            apy_reward: None,
            tvl_usd: Some("123456789"),
            underlying: Some("[\"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913\"]"),
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
        assert_eq!(
            validate(&RawEntry {
                pool_id: Some(&"a".repeat(MAX_ID_LEN)),
                ..good()
            }),
            Err(Refusal::PoolNotAllowlisted),
            "a valid id, if not a pinned one"
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
        let unpinned = RawEntry {
            pool_id: Some("aaaaaaaa-0000-0000-0000-000000000001"),
            ..good()
        };
        assert!(selected(&good()));
        assert!(
            selected(&RawEntry {
                chain: Some("Ethereum"),
                ..good()
            }),
            "a pinned pool is always read, to be refused by name if it moved"
        );
        assert!(selected(&unpinned));
        assert!(selected(&RawEntry {
            symbol: Some("steakUSDC"),
            ..unpinned
        }));
        for other in [
            RawEntry {
                chain: Some("base"),
                ..unpinned
            },
            RawEntry {
                chain: Some("Ethereum"),
                ..unpinned
            },
            RawEntry {
                protocol: Some("aave-v3x"),
                ..unpinned
            },
            RawEntry {
                symbol: Some("WETH"),
                ..unpinned
            },
        ] {
            assert!(!selected(&other), "{other:?}");
        }
    }

    #[test]
    fn only_a_pinned_pool_of_exactly_usdc_is_kept() {
        assert_eq!(validate(&good()).map(|m| m.protocol), Ok("aave-v3"));
        for (raw, want) in [
            (
                RawEntry {
                    pool_id: Some("aaaaaaaa-0000-0000-0000-000000000001"),
                    ..good()
                },
                Refusal::PoolNotAllowlisted,
            ),
            (
                RawEntry {
                    protocol: Some("morpho-blue"),
                    ..good()
                },
                Refusal::PoolNotAllowlisted,
            ),
            (
                RawEntry {
                    chain: Some("Ethereum"),
                    ..good()
                },
                Refusal::PoolNotAllowlisted,
            ),
            (
                RawEntry {
                    underlying: Some("[\"0x660975730059246A68521a3e2FBD4740173100f5\"]"),
                    ..good()
                },
                Refusal::UnderlyingNotUsdc,
            ),
            (
                RawEntry {
                    underlying: None,
                    ..good()
                },
                Refusal::UnderlyingNotUsdc,
            ),
            (
                RawEntry {
                    apy_base: None,
                    ..good()
                },
                Refusal::ApyBaseMissing,
            ),
            (
                RawEntry {
                    apy_base: Some("100.01"),
                    ..good()
                },
                Refusal::ApyBaseImplausible,
            ),
            (
                RawEntry {
                    apy_reward: Some("-1"),
                    ..good()
                },
                Refusal::ApyRewardNegative,
            ),
            (
                RawEntry {
                    apy_reward: Some("\"1\""),
                    ..good()
                },
                Refusal::ApyRewardNotNumber,
            ),
        ] {
            assert_eq!(validate(&raw), Err(want), "{raw:?}");
        }
        let with_reward = validate(&RawEntry {
            apy: Some("5"),
            apy_base: Some("4"),
            apy_reward: Some("1"),
            ..good()
        })
        .unwrap();
        assert_eq!(
            (
                with_reward.apy_base.to_string(),
                with_reward.apy_reward.map(|r| r.to_string())
            ),
            ("4".to_owned(), Some("1".to_owned()))
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
