//! The live HTTP adapter: one GET per fetch, HTTP/1.1, rustls with the webpki roots. The
//! URL is operator configuration, never request input; plain `http` is accepted only for a
//! loopback IP (the test fakes). Every await sits under one deadline; the body is capped.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Empty, LengthLimitError, Limited};
use hyper::body::Bytes;
use hyper::header::{ACCEPT, ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, HOST, USER_AGENT};
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::port::{FetchError, FetchFuture, PoolSource};

/// Why a source URL was refused at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    #[error("the URL must start with https:// (http:// only for a loopback IP)")]
    Scheme,
    #[error("plain http is accepted only for a loopback IP")]
    InsecureScheme,
    #[error("the URL's host is missing or not a plain host name")]
    Host,
    #[error("the URL's port is not a number")]
    Port,
    #[error("the URL's path has characters outside printable ASCII, or a fragment")]
    Path,
}

/// A parsed source URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Target {
    pub fn parse(url: &str) -> Result<Self, UrlError> {
        let (https, rest) = if let Some(rest) = url.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = url.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(UrlError::Scheme);
        };
        let (authority, path) = rest
            .find('/')
            .map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().map_err(|_| UrlError::Port)?),
            None => (authority, if https { 443 } else { 80 }),
        };
        // No userinfo, no IPv6 literal, no percent-encoding: a plain name or IPv4.
        if host.is_empty()
            || host.len() > 253
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return Err(UrlError::Host);
        }
        if !path.bytes().all(|b| b.is_ascii_graphic()) || path.contains('#') {
            return Err(UrlError::Path);
        }
        if !https && !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
            return Err(UrlError::InsecureScheme); // LIMIT(https-only)
        }
        Ok(Self {
            https,
            host: host.to_ascii_lowercase(),
            port,
            path: path.to_owned(),
        })
    }

    fn authority(&self) -> String {
        match (self.https, self.port) {
            (true, 443) | (false, 80) => self.host.clone(),
            _ => format!("{}:{}", self.host, self.port),
        }
    }
}

/// The live adapter.
#[derive(Clone)]
pub struct LiveSource {
    name: &'static str,
    target: Target,
    deadline: Duration,
    max_body: usize,
    tls: TlsConnector,
}

impl std::fmt::Debug for LiveSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveSource")
            .field("name", &self.name)
            .field("target", &self.target)
            .field("deadline", &self.deadline)
            .field("max_body", &self.max_body)
            .finish_non_exhaustive()
    }
}

impl LiveSource {
    /// `deadline` bounds the whole fetch (connect, TLS, head, body); `max_body` the body.
    pub fn new(name: &'static str, target: Target, deadline: Duration, max_body: usize) -> Self {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_or_else(
            |_| unreachable_config(),
            |b| b.with_root_certificates(roots).with_no_client_auth(),
        );
        Self {
            name,
            target,
            deadline,
            max_body,
            tls: TlsConnector::from(Arc::new(config)),
        }
    }

    async fn get(&self) -> Result<Bytes, FetchError> {
        let tcp = TcpStream::connect((self.target.host.as_str(), self.target.port))
            .await
            .map_err(|_| FetchError::Connect)?;
        if self.target.https {
            let name =
                ServerName::try_from(self.target.host.clone()).map_err(|_| FetchError::Tls)?;
            let tls = self
                .tls
                .connect(name, tcp)
                .await
                .map_err(|_| FetchError::Tls)?;
            self.exchange(TokioIo::new(tls)).await
        } else {
            self.exchange(TokioIo::new(tcp)).await
        }
    }

    async fn exchange<I>(&self, io: I) -> Result<Bytes, FetchError>
    where
        I: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
    {
        let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(io)
            .await
            .map_err(|_| FetchError::Protocol)?;
        let _conn = AbortOnDrop(tokio::spawn(conn));
        let request = Request::get(self.target.path.as_str())
            .header(HOST, self.target.authority())
            .header(ACCEPT, "application/json")
            .header(ACCEPT_ENCODING, "identity")
            .header(
                USER_AGENT,
                concat!("yieldagent/", env!("CARGO_PKG_VERSION"), " (read-only)"),
            )
            .body(Empty::new())
            .map_err(|_| FetchError::Protocol)?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| FetchError::Protocol)?;
        if response.status() != StatusCode::OK {
            return Err(FetchError::Status(response.status().as_u16()));
        }
        if response
            .headers()
            .get(CONTENT_ENCODING)
            .is_some_and(|v| v.as_bytes() != b"identity")
        {
            return Err(FetchError::Encoding);
        }
        let declared = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        if declared.is_some_and(|n| n > self.max_body as u64) {
            return Err(FetchError::TooLarge);
        }
        let body = Limited::new(response.into_body(), self.max_body) // LIMIT(source-body)
            .collect()
            .await
            .map_err(|e| {
                if e.downcast_ref::<LengthLimitError>().is_some() {
                    FetchError::TooLarge
                } else {
                    FetchError::Protocol
                }
            })?;
        Ok(body.to_bytes())
    }
}

#[expect(
    clippy::panic,
    reason = "ring's provider always supports the safe default versions"
)]
fn unreachable_config() -> rustls::ClientConfig {
    panic!("rustls: the ring provider rejected the safe default protocol versions")
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl PoolSource for LiveSource {
    fn name(&self) -> &'static str {
        self.name
    }

    fn fetch(&self) -> FetchFuture<'_> {
        Box::pin(async move {
            tokio::time::timeout(self.deadline, self.get()) // LIMIT(fetch-deadline)
                .await
                .unwrap_or(Err(FetchError::Timeout))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_https_or_loopback_http_and_plain() {
        let t = Target::parse("https://yields.llama.fi/pools").unwrap();
        assert_eq!(
            (t.https, t.host.as_str(), t.port, t.path.as_str()),
            (true, "yields.llama.fi", 443, "/pools")
        );
        assert_eq!(t.authority(), "yields.llama.fi");
        let t = Target::parse("http://127.0.0.1:63099/pools").unwrap();
        assert_eq!(t.authority(), "127.0.0.1:63099");
        for (bad, want) in [
            ("ftp://x/", UrlError::Scheme),
            ("yields.llama.fi/pools", UrlError::Scheme),
            ("http://yields.llama.fi/pools", UrlError::InsecureScheme),
            ("http://10.0.0.1/pools", UrlError::InsecureScheme),
            ("http://localhost/pools", UrlError::InsecureScheme),
            ("https://user@host/pools", UrlError::Host),
            ("https:///pools", UrlError::Host),
            ("https://[::1]/pools", UrlError::Port),
            ("https://host:99999/", UrlError::Port),
            ("https://host/a b", UrlError::Path),
            ("https://host/a#b", UrlError::Path),
            ("https://host/a\r\nX: y", UrlError::Path),
        ] {
            assert_eq!(Target::parse(bad), Err(want), "{bad}");
        }
    }
}
