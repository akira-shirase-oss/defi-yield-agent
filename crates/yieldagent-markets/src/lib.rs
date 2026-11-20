//! M1, read-only: the market data worker ([`worker`]), its append-only snapshot store
//! ([`store`]), the in-memory board the reads are served from ([`board`]) and the read API
//! ([`routes`]): JSON under `/v1/markets` and a plain, unstyled HTML list at `/markets`.
//! Nothing here holds a key, builds a transaction or moves funds.

pub mod board;
pub mod routes;
pub mod store;
pub mod worker;

pub use board::Board;
