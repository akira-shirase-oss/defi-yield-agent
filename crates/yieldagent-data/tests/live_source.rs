//! The live adapter against loopback fakes (each fault the network can hand it), and the
//! real source when network permission is given (`YIELDAGENT_LIVE_NETWORK=1`).
#![expect(clippy::unwrap_used, clippy::expect_used, reason = "integration test helpers")]

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use yieldagent_data::defillama;
use yieldagent_data::live::{LiveSource, Target};
use yieldagent_data::port::{FetchError, PoolSource};

/// A guard far above any deadline under test: a missing deadline fails, it does not hang.
const GUARD: Duration = Duration::from_secs(20);
const DEADLINE: Duration = Duration::from_millis(300);
const MAX_BODY: usize = 64 * 1024;

#[derive(Clone, Copy)]
enum Fake {
    Answer(&'static str),
    Chunked(usize),
    Silent,
}

async fn serve(fake: Fake) -> LiveSource {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut head = vec![0u8; 4096];
                let _ = socket.read(&mut head).await;
                match fake {
                    Fake::Answer(raw) => {
                        let _ = socket.write_all(raw.as_bytes()).await;
                    }
                    Fake::Chunked(total) => {
                        let _ = socket
                            .write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n")
                            .await;
                        let chunk = vec![b'x'; 4096];
                        let mut sent = 0;
                        while sent < total {
                            let frame = format!("{:x}\r\n", chunk.len());
                            if socket.write_all(frame.as_bytes()).await.is_err()
                                || socket.write_all(&chunk).await.is_err()
                                || socket.write_all(b"\r\n").await.is_err()
                            {
                                return;
                            }
                            sent += chunk.len();
                        }
                        let _ = socket.write_all(b"0\r\n\r\n").await;
                    }
                    Fake::Silent => std::future::pending::<()>().await,
                }
                let _ = socket.shutdown().await;
            });
        }
    });
    let target = Target::parse(&format!("http://{addr}/pools")).unwrap();
    LiveSource::new("defillama", target, DEADLINE, MAX_BODY)
}

async fn fetch(source: &LiveSource) -> Result<Vec<u8>, FetchError> {
    tokio::time::timeout(GUARD, source.fetch())
        .await
        .expect("the adapter's own deadline must end the fetch")
}

#[tokio::test]
async fn a_good_answer_is_returned_whole() {
    let s = serve(Fake::Answer(
        "HTTP/1.1 200 OK\r\ncontent-length: 36\r\n\r\n{\"status\":\"success\",\"data\":[{}, {}]}",
    ))
    .await;
    let body = fetch(&s).await.unwrap();
    assert_eq!(defillama::parse(&body).unwrap().entries, 2);
}

#[tokio::test]
async fn a_body_past_the_cap_is_refused_declared_or_streamed() {
    let declared = serve(Fake::Answer("HTTP/1.1 200 OK\r\ncontent-length: 99999999\r\n\r\n{")).await;
    assert_eq!(fetch(&declared).await, Err(FetchError::TooLarge));
    let streamed = serve(Fake::Chunked(MAX_BODY * 4)).await;
    assert_eq!(fetch(&streamed).await, Err(FetchError::TooLarge));
    let under = serve(Fake::Chunked(MAX_BODY - 4096)).await;
    assert_eq!(fetch(&under).await.map(|b| b.len()), Ok(MAX_BODY - 4096));
}

#[tokio::test]
async fn a_silent_source_times_out_at_the_deadline() {
    let s = serve(Fake::Silent).await;
    assert_eq!(fetch(&s).await, Err(FetchError::Timeout));
}

#[tokio::test]
async fn errors_and_encodings_are_named() {
    let s = serve(Fake::Answer("HTTP/1.1 429 Too Many Requests\r\ncontent-length: 0\r\n\r\n")).await;
    assert_eq!(fetch(&s).await, Err(FetchError::Status(429)));
    let s = serve(Fake::Answer(
        "HTTP/1.1 200 OK\r\ncontent-encoding: gzip\r\ncontent-length: 2\r\n\r\n{}",
    ))
    .await;
    assert_eq!(fetch(&s).await, Err(FetchError::Encoding));
    let s = serve(Fake::Answer("garbage\r\n\r\n")).await;
    assert_eq!(fetch(&s).await, Err(FetchError::Protocol));
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = closed.local_addr().unwrap();
    drop(closed);
    let s = LiveSource::new("defillama", Target::parse(&format!("http://{addr}/")).unwrap(), DEADLINE, MAX_BODY);
    assert_eq!(fetch(&s).await, Err(FetchError::Connect));
}

#[tokio::test]
async fn the_live_source_parses_when_network_is_permitted() {
    if std::env::var("YIELDAGENT_LIVE_NETWORK").as_deref() != Ok("1") {
        println!("skipped: the live DefiLlama fetch needs YIELDAGENT_LIVE_NETWORK=1 (network permission)");
        return;
    }
    let target = Target::parse("https://yields.llama.fi/pools").unwrap();
    let s = LiveSource::new("defillama", target, Duration::from_secs(30), 64 * 1024 * 1024);
    let body = tokio::time::timeout(Duration::from_secs(60), s.fetch()).await.unwrap().unwrap();
    let p = defillama::parse(&body).unwrap();
    println!("live: {} entries, {} selected, {} kept, refusals {:?}", p.entries, p.selected, p.markets.len(), p.refusals);
    assert_ne!(p.markets.len(), 0);
}
