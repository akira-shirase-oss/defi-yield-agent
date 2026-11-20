//! Market data for M1, read-only: the pool source port and its adapters, the `DefiLlama`
//! parser, exact decimals, the validation every field passes, and Case 5's data checks.
//! No database, no wallet, no keys, no transactions: this crate cannot sign or send.

pub mod decimal;
pub mod defillama;
pub mod live;
pub mod market;
pub mod port;
#[cfg(feature = "test-support")]
pub mod recorded;
pub mod screen;

pub use rust_decimal::Decimal;
