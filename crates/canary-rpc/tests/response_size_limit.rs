//! The 16 MiB response body cap (D-06, CA-01 section 6).
//!
//! A response at the cap is accepted, and anything larger is
//! [`RpcError::ResponseTooLarge`] on the first attempt: the declared
//! `Content-Length` is refused before any body bytes are read, and a body
//! with no declared length is abandoned as soon as the streamed total would
//! pass the cap, so an oversized payload is never fully buffered.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Instant;

use canary_rpc::{HttpRpcClient, RetryPolicy, RpcClient, RpcError, MAX_RESPONSE_BYTES};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// How long the raw server holds a connection open after the body it has
/// sent so far. Longer than any local read of 16 MiB, short enough that a
/// leaked thread exits well before the process does.
const STALL: std::time::Duration = std::time::Duration::from_secs(3);

/// Builds a `getNetwork` JSON-RPC success envelope of exactly `len` bytes,
/// padded through the `passphrase` field so the body is still valid JSON
/// and parses at any size.
fn network_body_of_len(len: usize) -> Vec<u8> {
    let prefix = br#"{"jsonrpc":"2.0","id":1,"result":{"passphrase":""#;
    let suffix = br#"","protocolVersion":28}}"#;
    let padding = len
        .checked_sub(prefix.len() + suffix.len())
        .expect("requested length is large enough for the envelope");
    let mut body = Vec::with_capacity(len);
    body.extend_from_slice(prefix);
    body.extend(std::iter::repeat_n(b'a', padding));
    body.extend_from_slice(suffix);
    assert_eq!(body.len(), len, "the helper must honour the requested size");
    body
}

fn too_large(err: &RpcError) -> bool {
    matches!(err, RpcError::ResponseTooLarge { .. })
}

/// Serves a raw HTTP/1.1 response on a loopback port, without reqwest or
/// wiremock framing, so a test can control whether a `Content-Length` is
/// declared and when the body stops arriving.
///
/// `stall_after` writes that many body bytes, then holds the connection
/// open for [`STALL`] before closing it. A client that waits for the end of
/// the body therefore cannot answer before the stall elapses, which is how
/// the tests tell "stopped at the cap" apart from "read everything".
///
/// Returns the endpoint URL and a handle that yields how many body bytes
/// the server wrote.
fn spawn_streaming_server(
    body_bytes: usize,
    content_length: Option<usize>,
    stall_after: Option<usize>,
) -> (String, std::thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));

    let handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept one connection");
        socket
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("set a write timeout");

        // Consume the request head first: writing before the client has
        // finished sending can fill both directions of the socket and make
        // the test depend on buffer sizes.
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            match socket.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => request.extend_from_slice(&buffer[..read]),
            }
        }

        let mut head = String::from(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n",
        );
        if let Some(len) = content_length {
            head.push_str(&format!("Content-Length: {len}\r\n"));
        }
        head.push_str("\r\n");
        if socket.write_all(head.as_bytes()).is_err() {
            return 0;
        }

        let chunk = vec![b'a'; 64 * 1024];
        let mut written = 0usize;
        while written < body_bytes {
            let take = (body_bytes - written).min(chunk.len());
            if socket.write_all(&chunk[..take]).is_err() {
                break;
            }
            written += take;
        }
        if stall_after.is_some_and(|threshold| written >= threshold) {
            std::thread::sleep(STALL);
        }
        let _ = socket.flush();
        written
    });

    (endpoint, handle)
}

#[tokio::test]
async fn a_body_exactly_at_the_cap_is_accepted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(network_body_of_len(MAX_RESPONSE_BYTES)),
        )
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let info = client
        .get_network()
        .await
        .expect("a body of exactly the cap is within the limit");
    assert_eq!(info.protocol_version, 28);
}

#[tokio::test]
async fn a_body_one_byte_over_the_cap_is_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(network_body_of_len(MAX_RESPONSE_BYTES + 1)),
        )
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_network().await.unwrap_err();
    match err {
        RpcError::ResponseTooLarge { method, limit } => {
            assert_eq!(method, "getNetwork");
            assert_eq!(limit, MAX_RESPONSE_BYTES);
        }
        other => panic!("expected ResponseTooLarge, got {other:?}"),
    }
}

#[tokio::test]
async fn the_cap_is_sixteen_mebibytes() {
    assert_eq!(MAX_RESPONSE_BYTES, 16 * 1024 * 1024);
}

#[tokio::test]
async fn an_oversized_response_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(network_body_of_len(MAX_RESPONSE_BYTES + 1)),
        )
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri()).with_retry_policy(RetryPolicy {
        max_attempts: 3,
        base_delay: std::time::Duration::from_millis(1),
    });
    assert!(too_large(&client.get_network().await.unwrap_err()));

    // The response is deterministic, so it is reported on the first attempt.
    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn an_oversized_declared_length_is_refused_without_reading_the_body() {
    // The server announces 20 MiB and then writes nothing. If the client
    // waited for the body (or for the connection to close) this call would
    // block until the client's timeout; returning promptly shows the
    // declared length alone was enough to reject it.
    let (endpoint, _server) = spawn_streaming_server(0, Some(20 * 1024 * 1024), None);
    let client = HttpRpcClient::new(endpoint).with_timeout(std::time::Duration::from_secs(5));

    let started = Instant::now();
    let err = client.get_network().await.unwrap_err();
    let elapsed = started.elapsed();

    assert!(too_large(&err), "expected ResponseTooLarge, got {err:?}");
    assert!(
        err.to_string().contains("getNetwork"),
        "the diagnostic names the method: {err}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "the body must not be read: took {elapsed:?}"
    );
}

#[tokio::test]
async fn a_streamed_body_without_a_declared_length_is_cut_off_at_the_cap() {
    // 17 MiB streamed with no `Content-Length`, i.e. the shape a chunked or
    // connection-close response has, and then the connection stalls instead
    // of ending. The client must stop once the running total passes 16 MiB:
    // an implementation that read to the end of the body would block on the
    // stall and answer with a Timeout/InvalidJson after the stall, not with
    // ResponseTooLarge immediately.
    let body_bytes = 17 * 1024 * 1024;
    let (endpoint, _server) = spawn_streaming_server(body_bytes, None, Some(body_bytes));
    let client = HttpRpcClient::new(endpoint).with_timeout(std::time::Duration::from_secs(30));

    let started = Instant::now();
    let err = client.get_network().await.unwrap_err();
    let elapsed = started.elapsed();

    assert!(too_large(&err), "expected ResponseTooLarge, got {err:?}");
    assert!(
        elapsed < STALL,
        "the body must be abandoned at the cap, not read to the end: took {elapsed:?}"
    );
}
