//! The `DefiLlama` yields `/pools` body: `{"status":"success","data":[{pool, chain, project,
//! symbol, apy, tvlUsd, ...}, ...]}` (docs/KNOWLEDGE.md). Untrusted input: the body is
//! already capped by the adapter; here the entry count is capped, every number is read from
//! its raw token, and every selected entry is validated or refused by name.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde_json::value::RawValue;

use crate::market::{Market, RawEntry, Refusal, selected, validate};

/// The source's name, as stored and served.
pub const SOURCE: &str = "defillama";
/// Most entries read from one body; past it the whole body is refused `too_many_entries`.
/// The live body had 14,893 at M1.
pub const MAX_ENTRIES: usize = 100_000; // LIMIT(entry-count)

/// Why a whole body was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BodyError {
    #[error("the body is not the expected JSON")]
    Malformed,
    #[error("the source reported a status other than success")]
    NotSuccess,
    #[error("more than {MAX_ENTRIES} entries")]
    TooManyEntries,
}

impl BodyError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed_body",
            Self::NotSuccess => "source_not_success",
            Self::TooManyEntries => "too_many_entries",
        }
    }
}

/// One body, read: the markets that passed and a count per refusal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub entries: usize,
    pub selected: usize,
    pub markets: Vec<Market>,
    pub refusals: BTreeMap<Refusal, u32>,
}

impl Parsed {
    pub fn refused(&self) -> u32 {
        self.refusals.values().sum()
    }
}

#[derive(Deserialize)]
struct Body<'a> {
    #[serde(borrow)]
    status: Option<&'a RawValue>,
    #[serde(borrow)]
    data: Capped<Entry<'a>>,
}

#[derive(Deserialize)]
struct Entry<'a> {
    #[serde(borrow, default)]
    pool: Option<&'a RawValue>,
    #[serde(borrow, default)]
    chain: Option<&'a RawValue>,
    #[serde(borrow, default)]
    project: Option<&'a RawValue>,
    #[serde(borrow, default)]
    symbol: Option<&'a RawValue>,
    #[serde(borrow, default)]
    apy: Option<&'a RawValue>,
    #[serde(borrow, default, rename = "tvlUsd")]
    tvl_usd: Option<&'a RawValue>,
}

/// A sequence that stops with an error at [`MAX_ENTRIES`] instead of allocating past it.
struct Capped<T>(Vec<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Capped<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = Capped<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1024));
                while let Some(item) = seq.next_element()? {
                    if out.len() >= MAX_ENTRIES {
                        return Err(de::Error::custom(TOO_MANY));
                    }
                    out.push(item);
                }
                Ok(Capped(out))
            }
        }
        deserializer.deserialize_seq(V(PhantomData))
    }
}

const TOO_MANY: &str = "too many entries";

/// A JSON string field; `null`, absent or another type reads as `None` / `Some("")`.
fn text(raw: Option<&RawValue>) -> Option<Cow<'_, str>> {
    let raw = raw?;
    if raw.get() == "null" {
        return None;
    }
    Some(serde_json::from_str::<Cow<'_, str>>(raw.get()).unwrap_or(Cow::Borrowed("")))
}

fn token(raw: Option<&RawValue>) -> Option<&str> {
    raw.map(RawValue::get).filter(|t| *t != "null")
}

/// Reads one body.
pub fn parse(body: &[u8]) -> Result<Parsed, BodyError> {
    let body: Body<'_> = serde_json::from_slice(body).map_err(|e| {
        if e.to_string().starts_with(TOO_MANY) {
            BodyError::TooManyEntries
        } else {
            BodyError::Malformed
        }
    })?;
    if text(body.status).as_deref() != Some("success") {
        return Err(BodyError::NotSuccess);
    }
    let mut parsed = Parsed {
        entries: body.data.0.len(),
        ..Parsed::default()
    };
    let mut valid = Vec::new();
    for entry in &body.data.0 {
        let (pool, chain, project, symbol) = (
            text(entry.pool),
            text(entry.chain),
            text(entry.project),
            text(entry.symbol),
        );
        let raw = RawEntry {
            pool_id: pool.as_deref(),
            chain: chain.as_deref(),
            protocol: project.as_deref(),
            symbol: symbol.as_deref(),
            apy: token(entry.apy),
            tvl_usd: token(entry.tvl_usd),
        };
        if selected(&raw).is_none() {
            continue;
        }
        parsed.selected += 1;
        match validate(&raw) {
            Ok(m) => valid.push(m),
            Err(r) => *parsed.refusals.entry(r).or_default() += 1,
        }
    }
    let mut seen = BTreeSet::new();
    let mut duplicated = BTreeSet::new();
    for m in &valid {
        if !seen.insert(m.pool_id.as_str()) {
            duplicated.insert(m.pool_id.clone());
        }
    }
    for m in valid {
        if duplicated.contains(&m.pool_id) {
            *parsed.refusals.entry(Refusal::DuplicatePool).or_default() += 1; // LIMIT(duplicate-pool)
        } else {
            parsed.markets.push(m);
        }
    }
    parsed.markets.sort_by(|a, b| a.pool_id.cmp(&b.pool_id));
    Ok(parsed)
}
