//! Property targets for every parser of source data: the number token, the body, the
//! entry validation and the source URL. Each must never panic, and must keep its contract
//! on arbitrary input.

use std::str::FromStr;

use proptest::prelude::*;
use yieldagent_data::Decimal;
use yieldagent_data::decimal::parse_exact;
use yieldagent_data::defillama::parse;
use yieldagent_data::live::Target;
use yieldagent_data::market::{MAX_APY_PERCENT, MAX_TVL_USD, RawEntry, validate};

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2_000, ..ProptestConfig::default() })]

    #[test]
    fn number_tokens_never_panic(s in "\\PC{0,64}") {
        let _ = parse_exact(&s);
    }

    #[test]
    fn decimals_round_trip_exactly(m in any::<i64>(), scale in 0u32..=18) {
        let d = Decimal::new(m, scale);
        prop_assert_eq!(parse_exact(&d.to_string()), Ok(d.normalize()));
    }

    #[test]
    fn exponent_forms_equal_their_plain_forms(m in 0u64..1_000_000_000, e in -10i32..=10) {
        let token = format!("{m}e{e}");
        if let Ok(v) = parse_exact(&token) {
            let plain = if e >= 0 {
                Decimal::from(m) * Decimal::from(10u64.pow(e.unsigned_abs()))
            } else {
                Decimal::from(m) / Decimal::from(10u64.pow(e.unsigned_abs()))
            };
            prop_assert_eq!(v, plain.normalize());
        }
    }

    #[test]
    fn bodies_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = parse(&bytes);
    }

    #[test]
    fn json_shaped_bodies_never_panic_and_keep_only_valid_markets(
        entries in proptest::collection::vec(
            (
                prop_oneof![Just("Base".to_owned()), "\\PC{0,8}"],
                prop_oneof![Just("aave-v3".to_owned()), Just("morpho-blue".to_owned()), "\\PC{0,8}"],
                prop_oneof![Just("USDC".to_owned()), "\\PC{0,12}"],
                "\\PC{0,70}",
                prop_oneof![Just("null".to_owned()), "-?(0|[1-9][0-9]{0,7})(\\.[0-9]{1,30})?(e-?[0-9]{1,2})?", Just("\"5\"".to_owned()), Just("true".to_owned())],
                prop_oneof![Just("null".to_owned()), "-?(0|[1-9][0-9]{0,13})(\\.[0-9]{1,6})?", Just("{}".to_owned())],
            ),
            0..24,
        )
    ) {
        let data: Vec<String> = entries.iter().map(|(chain, project, symbol, pool, apy, tvl)| {
            format!(
                "{{\"chain\":{},\"project\":{},\"symbol\":{},\"pool\":{},\"apy\":{apy},\"tvlUsd\":{tvl}}}",
                json_string(chain), json_string(project), json_string(symbol), json_string(pool)
            )
        }).collect();
        let body = format!("{{\"status\":\"success\",\"data\":[{}]}}", data.join(","));
        let p = parse(body.as_bytes()).unwrap();
        prop_assert_eq!(p.entries, entries.len());
        prop_assert_eq!(p.markets.len() + p.refused() as usize, p.selected);
        for m in &p.markets {
            prop_assert!(m.apy >= Decimal::ZERO && m.apy <= MAX_APY_PERCENT);
            prop_assert!(m.tvl_usd >= Decimal::ZERO && m.tvl_usd <= MAX_TVL_USD);
            prop_assert!(m.pool_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
            prop_assert!(!m.symbol.contains(['<', '>', '"', '&', '\'']));
        }
    }

    #[test]
    fn validation_never_panics(
        pool in proptest::option::of("\\PC{0,80}"),
        symbol in proptest::option::of("\\PC{0,40}"),
        apy in proptest::option::of("\\PC{0,50}"),
        tvl in proptest::option::of("\\PC{0,50}"),
        reward in proptest::option::of("\\PC{0,50}"),
        underlying in proptest::option::of("\\PC{0,200}"),
    ) {
        let raw = RawEntry {
            pool_id: pool.as_deref(),
            chain: Some("Base"),
            protocol: Some("aave-v3"),
            symbol: symbol.as_deref(),
            apy: apy.as_deref(),
            apy_base: apy.as_deref(),
            apy_reward: reward.as_deref(),
            tvl_usd: tvl.as_deref(),
            underlying: underlying.as_deref(),
        };
        if let Ok(m) = validate(&raw) {
            prop_assert!(m.apy <= MAX_APY_PERCENT && !m.apy.is_sign_negative() || m.apy.is_zero());
            prop_assert!(m.apy_reward.is_none_or(|r| r <= MAX_APY_PERCENT));
            prop_assert!(underlying.as_deref().is_some_and(|u| u.to_ascii_lowercase().contains("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913")));
            prop_assert_eq!(Some(m.apy), apy.as_deref().and_then(|a| Decimal::from_str(a).ok().map(|d| d.normalize())).or(Some(m.apy)));
        }
    }

    #[test]
    fn source_urls_never_panic_and_plain_http_stays_on_loopback(s in "(https?://)?\\PC{0,60}") {
        if let Ok(t) = Target::parse(&s) {
            prop_assert!(t.https || t.host.starts_with("127."));
            prop_assert!(t.path.starts_with('/'));
            prop_assert!(!t.host.contains(['@', ':', '/']));
        }
    }
}
