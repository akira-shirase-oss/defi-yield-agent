//! The DefiLlama parser against the recorded body and a hostile one.

use std::str::FromStr;

use yieldagent_data::Decimal;
use yieldagent_data::defillama::{BodyError, parse};
use yieldagent_data::market::Refusal;

const RECORDED: &[u8] = include_bytes!("fixtures/defillama-pools.json");
const HOSTILE: &[u8] = include_bytes!("fixtures/defillama-hostile.json");

#[test]
fn the_recorded_body_yields_only_base_usdc_on_the_allowlist() {
    let p = parse(RECORDED).unwrap();
    assert_eq!(p.entries, 50);
    assert_eq!(p.selected, 30);
    let mut ids: Vec<_> = p.markets.iter().map(|m| m.pool_id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, PINNED, "refusals: {:?}", p.refusals);
    let refused: Vec<_> = p.refusals.iter().map(|(r, n)| (r.code(), *n)).collect();
    assert_eq!(
        refused,
        [("pool_not_allowlisted", 24)],
        "syrupUSDC, the WETH-pool comet and every unpinned vault are skipped by name"
    );
    assert!(p.markets.iter().all(|m| m.chain == "Base"));
    let aave = p
        .markets
        .iter()
        .find(|m| m.pool_id == "7e0661bf-8cf3-45e6-9424-31916d4c7b84")
        .unwrap();
    assert_eq!((aave.protocol, aave.symbol.as_str()), ("aave-v3", "USDC"));
    assert_eq!(
        aave.apy,
        Decimal::from_str("5.35824").unwrap(),
        "exact, not a float"
    );
}

/// The pools `market::ALLOWLIST` pins, sorted.
const PINNED: [&str; 6] = [
    "0c8567f8-ba5b-41ad-80de-00a71895eb19",
    "28470287-2c5b-5744-bc13-b9f1c0190b7b",
    "7e0661bf-8cf3-45e6-9424-31916d4c7b84",
    "81ae8812-f04f-4f6e-9d71-ee5778f3a178",
    "a5fab52f-73fa-4f44-9c09-9af1eb20996c",
    "ba68527f-8ec2-4c55-827a-8f4673ae047c",
];
const AAVE: &str = "7e0661bf-8cf3-45e6-9424-31916d4c7b84";
const USDC: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";

fn aave(underlying: &str) -> String {
    format!(
        "{{\"pool\":\"{AAVE}\",\"chain\":\"Base\",\"project\":\"aave-v3\",\"symbol\":\"USDC\",\"apy\":5,\"apyBase\":5,\"tvlUsd\":1000000,\"underlyingTokens\":{underlying}}}"
    )
}

fn body_of(entries: &[String]) -> Vec<u8> {
    format!(
        "{{\"status\":\"success\",\"data\":[{}]}}",
        entries.join(",")
    )
    .into_bytes()
}

#[test]
fn a_pinned_pool_is_kept_only_while_its_underlying_token_is_exactly_usdc() {
    let usdc = format!("[\"{USDC}\"]");
    let lower = format!("[\"{}\"]", USDC.to_ascii_lowercase());
    let p = parse(&body_of(&[aave(&usdc)])).unwrap();
    assert_eq!(p.markets.len(), 1, "{:?}", p.refusals);
    assert_eq!(
        parse(&body_of(&[aave(&lower)])).unwrap().markets.len(),
        1,
        "addresses are case-free"
    );
    for bad in [
        "[\"0x660975730059246A68521a3e2FBD4740173100f5\"]".to_owned(),
        format!("[\"{USDC}\",\"{USDC}\"]"),
        "[]".to_owned(),
        "null".to_owned(),
        format!("\"{USDC}\""),
        "[5]".to_owned(),
    ] {
        let p = parse(&body_of(&[aave(&bad)])).unwrap();
        let refused: Vec<_> = p.refusals.iter().map(|(r, n)| (r.code(), *n)).collect();
        assert_eq!(refused, [("underlying_not_usdc", 1)], "{bad}");
        assert!(p.markets.is_empty(), "{bad}");
    }
    let wrong_protocol = aave(&usdc).replace("aave-v3", "morpho-blue");
    let p = parse(&body_of(&[wrong_protocol])).unwrap();
    let refused: Vec<_> = p.refusals.iter().map(|(r, n)| (r.code(), *n)).collect();
    assert_eq!(
        refused,
        [("pool_not_allowlisted", 1)],
        "a pinned id is pinned to its protocol"
    );
}

#[test]
fn a_body_past_the_market_cap_is_refused_whole() {
    let usdc = format!("[\"{USDC}\"]");
    let at_cap = vec![aave(&usdc); 64];
    let p = parse(&body_of(&at_cap)).unwrap();
    assert_eq!(
        (p.markets.len(), p.refused()),
        (0, 64),
        "every copy of a repeated pool is refused"
    );
    let past = vec![aave(&usdc); 65];
    let e = parse(&body_of(&past)).map(|p| p.markets.len()).unwrap_err();
    assert_eq!(e.code(), "too_many_markets");
}

#[test]
fn every_hostile_entry_is_refused_by_name_and_only_the_good_one_kept() {
    let p = parse(HOSTILE).unwrap();
    assert_eq!(p.entries, 17);
    assert_eq!(p.selected, 15, "Ethereum and evil-fork are not selected");
    let ids: Vec<_> = p.markets.iter().map(|m| m.pool_id.as_str()).collect();
    assert_eq!(ids, ["7e0661bf-8cf3-45e6-9424-31916d4c7b84"]);
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
        assert_eq!(
            p.refusals.get(&r).copied(),
            Some(n),
            "{r:?} in {:?}",
            p.refusals
        );
    }
    assert_eq!(p.refused(), 14);
}

#[test]
fn a_body_that_is_not_the_shape_is_refused_whole() {
    for (body, want) in [
        (&b""[..], BodyError::Malformed),
        (b"[]", BodyError::Malformed),
        (b"{\"status\":\"success\"}", BodyError::Malformed),
        (
            b"{\"status\":\"success\",\"data\":{}}",
            BodyError::Malformed,
        ),
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
    // Literal counts: the proof raises the constant; 100,001 entries must still be refused.
    assert_eq!(parse(body(100_000).as_bytes()).unwrap().entries, 100_000);
    assert_eq!(
        parse(body(100_001).as_bytes()),
        Err(BodyError::TooManyEntries)
    );
}
