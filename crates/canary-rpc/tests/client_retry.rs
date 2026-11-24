//! Retry configuration on `HttpRpcClient` (E-097): the client must honor
//! the configured retry count and delay, and `0` retries must disable
//! retries entirely — failing on the first network error after exactly one
//! request.
//!
//! The policy under test is the one the CLI builds from
//! `--rpc-max-retries` / `--rpc-retry-delay-ms` via
//! [`RetryPolicy::from_retries`], so these tests pin both the mapping and
//! the behavior it produces.
//!
//! Issue #501 extends the coverage to the adverse-condition paths: HTTP
//! 429 rate limiting (including this client's deliberate non-use of the
//! `Retry-After` header), per-request timeouts, retry exhaustion, and
//! non-retryable HTTP 4xx statuses. Note that this crate has no dedicated
//! `RpcError::MaxRetriesExceeded` variant: the retry-exhaustion result is
//! the last transient error with the attempt count attached
//! ([`RpcError::RateLimited`], [`RpcError::Timeout`]), or the bare
//! [`RpcError::Transport`] for 5xx/connection failures.

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

// ── HTTP 429 rate limiting (issue #501) ────────────────────────────────

/// A 429 is transient: the client retries it under the policy, so two
/// rate-limited answers followed by a good one are an overall success
/// after exactly three requests.
#[tokio::test]
async fn rate_limited_responses_are_retried_until_the_endpoint_recovers() {
    let server = MockServer::start().await;
    // 429 on the first two requests only; afterwards this mock stops
    // matching and the request falls through to the success mock.
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(2)
        .named("transient 429")
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
        .with_retry_policy(RetryPolicy::from_retries(2, Duration::from_millis(1)));
    let info = client.get_network().await.expect("retry succeeds");
    assert_eq!(info.protocol_version, 28);

    assert_eq!(
        request_count(&server).await,
        3,
        "two 429s plus the request that succeeded"
    );
}

/// Exhausting the attempt budget against a permanent 429 surfaces the
/// repository's retry-exhaustion result: `RpcError::RateLimited` with
/// `attempts` equal to the configured attempt budget. (There is no
/// dedicated `RpcError::MaxRetriesExceeded` variant — the final transient
/// error is returned with the attempt count attached.)
#[tokio::test]
async fn exhausted_rate_limit_retries_surface_rate_limited_with_the_attempt_count() {
    let server = always_failing_server(429).await;
    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(2, Duration::from_millis(1)));

    let err = client
        .get_network()
        .await
        .expect_err("the endpoint never stops rate-limiting");
    match &err {
        RpcError::RateLimited { method, attempts } => {
            assert_eq!(method, "getNetwork");
            assert_eq!(
                *attempts, 3,
                "three attempts: one initial request plus two retries"
            );
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
    assert!(err.to_string().contains("3 attempt(s)"), "{err}");
    assert_eq!(request_count(&server).await, 3);
}

/// Rate limiting is paced by the configured linear backoff exactly like
/// any other transient failure: a 100ms base with three attempts sleeps
/// 100ms then 200ms, so the call cannot complete in under 300ms. This is
/// a lower-bound assertion only — waiting is awaited inside the call, so
/// no amount of machine slowness can make it flaky.
#[tokio::test]
async fn rate_limited_retries_wait_the_configured_linear_backoff() {
    let server = always_failing_server(429).await;
    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(2, Duration::from_millis(100)));

    let started = Instant::now();
    let _ = client
        .get_network()
        .await
        .expect_err("the endpoint never stops rate-limiting");
    let elapsed = started.elapsed();

    assert_eq!(request_count(&server).await, 3);
    assert!(
        elapsed >= Duration::from_millis(300),
        "expected at least 300ms of linear backoff, took {elapsed:?}"
    );
}

/// The `Retry-After` header is deliberately not honored: the documented
/// timing model is the configured linear backoff and nothing else (see
/// `HttpRpcClient::with_retry_policy`). The 429 below demands a one-hour
/// pause via `Retry-After`; if the client ever started honoring it, this
/// call would take an hour instead of a couple of milliseconds — the
/// upper bound below fails loudly rather than hanging the suite.
#[tokio::test]
async fn retry_after_header_does_not_extend_the_configured_backoff() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "3600"))
        .up_to_n_times(1)
        .named("429 with an hour-long Retry-After")
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
        .with_retry_policy(RetryPolicy::from_retries(1, Duration::from_millis(1)));

    let started = Instant::now();
    let info = client.get_network().await.expect("retry succeeds");
    let elapsed = started.elapsed();

    assert_eq!(info.protocol_version, 28);
    assert_eq!(request_count(&server).await, 2);
    assert!(
        elapsed < Duration::from_secs(10),
        "the Retry-After header must not be waited out; the configured \
         linear backoff alone applies (took {elapsed:?})"
    );
}

// ── Request timeouts (issue #501) ──────────────────────────────────────

/// A response that misses the configured per-request deadline becomes
/// `RpcError::Timeout` and is retried like any transient failure: two
/// attempts mean two requests, and the surfaced error records the
/// attempt count.
#[tokio::test]
async fn a_timed_out_request_is_retried_and_reports_the_attempt_count() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        // Longer than the client's deadline below, so every attempt times out.
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(600)))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri())
        .with_timeout(Duration::from_millis(200))
        .with_retry_policy(RetryPolicy::from_retries(1, Duration::from_millis(1)));

    let err = client
        .get_network()
        .await
        .expect_err("the server never answers within the deadline");
    match &err {
        RpcError::Timeout { method, attempts } => {
            assert_eq!(method, "getNetwork");
            assert_eq!(*attempts, 2, "one initial attempt plus one retry");
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
    assert!(err.to_string().contains("2 attempt(s)"), "{err}");
    assert_eq!(
        request_count(&server).await,
        2,
        "each timed-out attempt sends exactly one request"
    );
}

/// A timeout does not condemn the whole call: when a later attempt
/// answers within the deadline, its result is returned as `Ok`.
#[tokio::test]
async fn a_timed_out_attempt_recovers_when_the_next_attempt_answers_in_time() {
    let server = MockServer::start().await;
    let requests_seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(move |_request: &wiremock::Request| {
            let nth = requests_seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if nth == 0 {
                // Never answered within the client's 200ms deadline.
                ResponseTemplate::new(200).set_delay(Duration::from_millis(600))
            } else {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "passphrase": "Test SDF Network ; September 2015",
                        "protocolVersion": 28
                    }
                }))
            }
        })
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri())
        .with_timeout(Duration::from_millis(200))
        .with_retry_policy(RetryPolicy::from_retries(1, Duration::from_millis(1)));

    let info = client
        .get_network()
        .await
        .expect("the second attempt answers in time");
    assert_eq!(info.protocol_version, 28);
    assert_eq!(request_count(&server).await, 2);
}

// ── Retry exhaustion (issue #501) ──────────────────────────────────────

/// Exhausted 5xx retries surface the transport error after exactly
/// `max_attempts` requests. `RpcError::Transport` carries no attempt
/// count, so the count is proven through the server's request log; the
/// 429 and timeout tests above pin the `attempts` field of the variants
/// that do have one.
#[tokio::test]
async fn exhausted_server_error_retries_stop_at_the_attempt_limit() {
    let server = always_failing_server(503).await;
    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(3, Duration::from_millis(1)));

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
        4,
        "one initial request plus three retries"
    );
}

// ── Non-retryable HTTP statuses (issue #501) ───────────────────────────

/// HTTP 400 is not transient: with a non-JSON body the call fails on the
/// first attempt as `InvalidJson`, and no retry budget changes that.
#[tokio::test]
async fn http_400_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(400).set_body_string("Bad Request"))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(3, Duration::from_millis(1)));

    let err = client
        .get_network()
        .await
        .expect_err("the endpoint rejects the request");
    match &err {
        RpcError::InvalidJson { method, .. } => assert_eq!(method, "getNetwork"),
        other => panic!("expected InvalidJson, got {other:?}"),
    }
    assert_eq!(
        request_count(&server).await,
        1,
        "a 400 must fail immediately, not be retried"
    );
}

/// A 400 that *does* carry a JSON-RPC `error` object is a deterministic
/// protocol rejection (`JsonRpcError`) — equally unretryable.
#[tokio::test]
async fn http_400_with_a_json_rpc_error_body_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32600, "message": "Invalid Request" }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(3, Duration::from_millis(1)));

    let err = client
        .get_network()
        .await
        .expect_err("the endpoint rejects the request");
    assert!(
        matches!(err, RpcError::JsonRpcError { code: -32600, .. }),
        "unexpected error: {err}"
    );
    assert_eq!(
        request_count(&server).await,
        1,
        "a JSON-RPC error must fail immediately, not be retried"
    );
}
