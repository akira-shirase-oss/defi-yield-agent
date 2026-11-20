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

/// A body with one Base USDC market per `(pool, apy)`.
pub fn body(markets: &[(&str, &str)]) -> Vec<u8> {
    let data: Vec<String> = markets
        .iter()
        .map(|(pool, apy)| {
            format!("{{\"pool\":\"{pool}\",\"chain\":\"Base\",\"project\":\"aave-v3\",\"symbol\":\"USDC\",\"apy\":{apy},\"tvlUsd\":1000000}}")
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
