//! The pool source port: one call fetches one body. Adapters: [`crate::live::LiveSource`]
//! (HTTP, deadline and body cap inside) and, for tests, `recorded::Recorded`.

use std::future::Future;
use std::pin::Pin;

pub use hyper::body::Bytes;

pub type FetchFuture<'a> = Pin<Box<dyn Future<Output = Result<Bytes, FetchError>> + Send + 'a>>;

/// A source of pool data.
pub trait PoolSource: Send + Sync {
    /// The source's stored name (`[a-z0-9-]{1,32}`).
    fn name(&self) -> &'static str;
    /// Fetches one body. Adapters bound it in time and size; callers add their own deadline.
    fn fetch(&self) -> FetchFuture<'_>;
}

/// Why a fetch failed. The code is stored on the round and served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    #[error("the fetch passed its deadline")]
    Timeout,
    #[error("could not connect")]
    Connect,
    #[error("the TLS handshake failed")]
    Tls,
    #[error("the source answered HTTP {0}")]
    Status(u16),
    #[error("the body passed its cap")]
    TooLarge,
    #[error("the body is compressed or otherwise encoded")]
    Encoding,
    #[error("the HTTP exchange failed")]
    Protocol,
}

impl FetchError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Connect => "connect_failed",
            Self::Tls => "tls_failed",
            Self::Status(_) => "http_status",
            Self::TooLarge => "body_too_large",
            Self::Encoding => "unsupported_encoding",
            Self::Protocol => "protocol_error",
        }
    }
}
