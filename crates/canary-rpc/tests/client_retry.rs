//! Retry configuration on `HttpRpcClient` (E-097): the client must honor
//! the configured retry count and delay, and `0` retries must disable
//! retries entirely — failing on the first network error after exactly one
//! request.
//!
//! The policy under test is the one the CLI builds from
//! `--rpc-max-retries` / `--rpc-retry-delay-ms` via
//! [`RetryPolicy::from_retries`], so these tests pin both the mapping and
//! the behavior it produces.

use std::time::{Duration, Instant};

use canary_rpc::{HttpRpcClient, RetryPolicy, RpcClient, RpcError};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A server that answers every request with `status` (a retryable 5xx).
async fn always_failing_server(status: u16) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(status))
        .mount(&server)
        .await;
    server
}

async fn request_count(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .expect("the mock server records requests")
        .len()
}

#[test]
fn from_retries_maps_a_retry_count_onto_an_attempt_count() {
    // `--rpc-max-retries 0` must mean "one attempt, no retries"…
    let none = RetryPolicy::from_retries(0, Duration::from_millis(500));
    assert_eq!(none.max_attempts, 1);
    assert_eq!(none.base_delay, Duration::from_millis(500));

    // …and the default of 3 is three retries after the first attempt.
    let default = RetryPolicy::from_retries(3, Duration::from_millis(500));
    assert_eq!(default.max_attempts, 4);
    assert_eq!(default.base_delay, Duration::from_millis(500));

    // The conversion must saturate rather than overflow.
    let max = RetryPolicy::from_retries(u32::MAX, Duration::from_millis(1));
    assert_eq!(max.max_attempts, u32::MAX);
}

/// The acceptance case for `--rpc-max-retries 0`: the very first network
/// error is surfaced, with no second request ever sent.
#[tokio::test]
async fn zero_retries_fails_on_the_first_network_error_after_one_request() {
    let server = always_failing_server(503).await;
    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(0, Duration::from_millis(1)));

    let err = client
        .get_network()
        .await
        .expect_err("zero retries must fail on the first network error");
    assert!(
        matches!(err, RpcError::Transport { .. }),
        "unexpected error: {err}"
    );
    assert_eq!(
        request_count(&server).await,
        1,
        "zero retries must send exactly one request"
    );
}

/// Positive side: a configured retry count is honored — two retries after
/// the first attempt means exactly three requests, then the error.
#[tokio::test]
async fn configured_retries_are_honored_as_extra_requests() {
    let server = always_failing_server(503).await;
    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(2, Duration::from_millis(0)));

    let err = client
        .get_network()
        .await
        .expect_err("the endpoint never recovers");
    assert!(
        matches!(err, RpcError::Transport { .. }),
        "unexpected error: {err}"
    );
    assert_eq!(
        request_count(&server).await,
        3,
        "two retries after the first attempt means three requests"
    );
}

/// The configured delay is really waited: linear backoff with a 100ms base
/// sleeps 100ms after attempt 1 and 200ms after attempt 2, so the call must
/// take at least 300ms (the sleeps are awaited inside the call, so this
/// bounds from below — network speed cannot make it faster).
#[tokio::test]
async fn the_configured_base_delay_is_actually_waited_between_attempts() {
    let server = always_failing_server(503).await;
    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(2, Duration::from_millis(100)));

    let started = Instant::now();
    let _ = client
        .get_network()
        .await
        .expect_err("the endpoint never recovers");
    let elapsed = started.elapsed();

    assert_eq!(request_count(&server).await, 3);
    assert!(
        elapsed >= Duration::from_millis(300),
        "expected at least 300ms of linear backoff, took {elapsed:?}"
    );
}

/// Retrying must actually recover: a transient failure followed by a good
/// response is an overall success, and the success is not re-requested.
#[tokio::test]
async fn a_transient_failure_is_retried_until_the_endpoint_recovers() {
    let server = MockServer::start().await;
    // 503 on the first request only; the retry falls through to the
    // success mock below (same-priority mocks match in insertion order).
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .named("transient 503")
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "passphrase": "Test SDF Network ; September 2015",
                "protocolVersion": 28
            }
        })))
        .named("recovered")
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(3, Duration::from_millis(1)));
    let info = client.get_network().await.expect("retry succeeds");
    assert_eq!(info.protocol_version, 28);

    assert_eq!(request_count(&server).await, 2);
}

/// Deterministic failures are never retried, however many retries are
/// configured: one request, one JSON-RPC error.
#[tokio::test]
async fn deterministic_json_rpc_errors_are_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32601, "message": "Method not found" }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(3, Duration::from_millis(1)));

    let err = client
        .get_network()
        .await
        .expect_err("the endpoint answers with a JSON-RPC error");
    // -32601 is classified as the missing-capability diagnostic; whatever
    // the classification, a JSON-RPC `error` object is deterministic and
    // must come back on the first attempt.
    match &err {
        RpcError::MethodNotAvailable { method } => assert_eq!(method, "getNetwork"),
        other => panic!("expected MethodNotAvailable, got {other:?}"),
    }
    assert!(err.to_string().contains("-32601"), "{err}");
    assert_eq!(
        request_count(&server).await,
        1,
        "a deterministic error must not be retried"
    );
}
