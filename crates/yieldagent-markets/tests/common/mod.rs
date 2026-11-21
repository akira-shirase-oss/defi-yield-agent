#![allow(
    dead_code,
    unreachable_pub,
    reason = "each test file uses a different part"
)]

use std::sync::Arc;
use std::time::Duration;

use yieldagent_data::recorded::{Recorded, Step};
use yieldagent_data::screen::NoChainReads;
use yieldagent_markets::worker::{Ports, Settings};

pub const RECORDED: &[u8] =
    include_bytes!("../../../yieldagent-data/tests/fixtures/defillama-pools.json");
pub const HOSTILE: &[u8] =
    include_bytes!("../../../yieldagent-data/tests/fixtures/defillama-hostile.json");
pub const AAVE: &str = "7e0661bf-8cf3-45e6-9424-31916d4c7b84";
pub const COMPOUND: &str = "0c8567f8-ba5b-41ad-80de-00a71895eb19";
pub const USDC: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";

/// A body with one Base USDC market per `(pool, apy)`; the APY is all base, no reward.
pub fn body(markets: &[(&str, &str)]) -> Vec<u8> {
    let with_reward: Vec<(&str, &str, &str, &str)> =
        markets.iter().map(|(p, a)| (*p, *a, *a, "null")).collect();
    body_with_reward(&with_reward)
}

/// A body with one Base USDC market per `(pool, apy, apy_base, apy_reward)`, `apy` being
/// the source's own total.
pub fn body_with_reward(markets: &[(&str, &str, &str, &str)]) -> Vec<u8> {
    let data: Vec<String> = markets
        .iter()
        .map(|(pool, total, base, reward)| {
            let project = if *pool == COMPOUND { "compound-v3" } else { "aave-v3" };
            format!("{{\"pool\":\"{pool}\",\"chain\":\"Base\",\"project\":\"{project}\",\"symbol\":\"USDC\",\"apy\":{total},\"apyBase\":{base},\"apyReward\":{reward},\"tvlUsd\":1000000,\"underlyingTokens\":[\"{USDC}\"]}}")
        })
        .collect();
    format!("{{\"status\":\"success\",\"data\":[{}]}}", data.join(",")).into_bytes()
}

pub fn ports(steps: Vec<Step>) -> (Arc<Recorded>, Ports) {
    let source = Arc::new(Recorded::new("defillama", steps));
    let ports = Ports {
        source: source.clone(),
        chain: Arc::new(NoChainReads),
    };
    (source, ports)
}

pub fn settings() -> Settings {
    Settings {
        interval: Duration::from_secs(1),
        fetch_deadline: Duration::from_millis(300),
        write_deadline: Duration::from_secs(5),
        write_attempts: 3,
        retry_backoff: Duration::from_millis(10),
    }
}

pub async fn count(pool: &sqlx::PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(pool)
        .await
        .unwrap_or(-1)
}
