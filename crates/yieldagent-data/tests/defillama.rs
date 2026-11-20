//! The DefiLlama parser against the recorded body and a hostile one.

use std::str::FromStr;

use yieldagent_data::Decimal;
use yieldagent_data::defillama::{BodyError, MAX_ENTRIES, parse};
use yieldagent_data::market::Refusal;

const RECORDED: &[u8] = include_bytes!("fixtures/defillama-pools.json");
const HOSTILE: &[u8] = include_bytes!("fixtures/defillama-hostile.json");

#[test]
fn the_recorded_body_yields_only_base_usdc_on_the_allowlist() {
    let p = parse(RECORDED).unwrap();
    assert_eq!(p.entries, 50);
    assert_eq!(p.selected, 30);
    assert_eq!(p.markets.len(), 30, "refusals: {:?}", p.refusals);
    assert!(p.markets.iter().all(|m| m.chain == "Base"));
    let aave = p.markets.iter().find(|m| m.pool_id == "7e0661bf-8cf3-45e6-9424-31916d4c7b84").unwrap();
    assert_eq!((aave.protocol, aave.symbol.as_str()), ("aave-v3", "USDC"));
    assert_eq!(aave.apy, Decimal::from_str("5.35824").unwrap(), "exact, not a float");
}

#[test]
fn every_hostile_entry_is_refused_by_name_and_only_the_good_one_kept() {
    let p = parse(HOSTILE).unwrap();
    assert_eq!(p.entries, 17);
    assert_eq!(p.selected, 15, "Ethereum and evil-fork are not selected");
    let ids: Vec<_> = p.markets.iter().map(|m| m.pool_id.as_str()).collect();
    assert_eq!(ids, ["aaaaaaaa-0000-0000-0000-000000000001"]);
    let want = [
        (Refusal::ApyImplausible, 1),
        (Refusal::ApyNegative, 1),
        (Refusal::ApyNotNumber, 1),
        (Refusal::ApyMissing, 1),
        (Refusal::TvlImplausible, 1),
        (Refusal::TvlNegative, 1),
        (Refusal::ApyInexact, 1),
        (Refusal::PoolIdInvalid, 2),
        (Refusal::SymbolInvalid, 1),
        (Refusal::PoolIdMissing, 1),
        (Refusal::TvlMissing, 1),
        (Refusal::DuplicatePool, 2),
    ];
    for (r, n) in want {
        assert_eq!(p.refusals.get(&r).copied(), Some(n), "{r:?} in {:?}", p.refusals);
    }
    assert_eq!(p.refused(), 14);
}

#[test]
fn a_body_that_is_not_the_shape_is_refused_whole() {
    for (body, want) in [
        (&b""[..], BodyError::Malformed),
        (b"[]", BodyError::Malformed),
        (b"{\"status\":\"success\"}", BodyError::Malformed),
        (b"{\"status\":\"success\",\"data\":{}}", BodyError::Malformed),
        (b"{\"status\":\"error\",\"data\":[]}", BodyError::NotSuccess),
        (b"{\"data\":[]}", BodyError::NotSuccess),
        (b"<html>rate limited</html>", BodyError::Malformed),
    ] {
        assert_eq!(parse(body), Err(want), "{}", String::from_utf8_lossy(body));
    }
    for field in ["x", "apy", "pool"] {
        let deep = format!(
            "{{\"status\":\"success\",\"data\":[{{\"chain\":\"Base\",\"project\":\"aave-v3\",\"symbol\":\"USDC\",\"{field}\":{}{}}}]}}",
            "[".repeat(100_000),
            "]".repeat(100_000)
        );
        // Bounded nesting: an answer, never a stack overflow, and never a market.
        match parse(deep.as_bytes()) {
            Ok(p) => assert!(p.markets.is_empty(), "{field}"),
            Err(e) => assert_eq!(e, BodyError::Malformed, "{field}"),
        }
    }
}

#[test]
fn the_entry_count_is_capped() {
    let body = |n: usize| {
        let mut s = String::from("{\"status\":\"success\",\"data\":[");
        s.push_str(&vec!["{}"; n].join(","));
        s.push_str("]}");
        s
    };
    assert_eq!(parse(body(MAX_ENTRIES).as_bytes()).unwrap().entries, MAX_ENTRIES);
    assert_eq!(parse(body(MAX_ENTRIES + 1).as_bytes()), Err(BodyError::TooManyEntries));
}
