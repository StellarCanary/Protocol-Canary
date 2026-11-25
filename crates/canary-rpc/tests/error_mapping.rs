//! JSON-RPC error classification tests (issue #480).
//!
//! - `-32601` ("Method not found") maps to [`RpcError::MethodNotAvailable`],
//!   preserving the method name in the diagnostic, is never retried, and is
//!   skipped by the runner rather than reported as a fatal endpoint error.
//! - Every other JSON-RPC code (`-32600`, `-32602`, ...) keeps mapping to
//!   the distinct [`RpcError::JsonRpcError`] variant and stays an error.
//! - Transport failures remain `Status::Error`: skipping is reserved for a
//!   method the endpoint does not implement, not for connection problems.
//!
//! Issue #726 adds the HTTP-status and body-shape half of the same mapping:
//! HTTP 429 (with or without a `Retry-After` header) is
//! [`RpcError::RateLimited`], and a `200 OK` carrying a non-JSON or truncated
//! body is [`RpcError::InvalidJson`] — never a panic, and never retried.

use std::time::Duration;

use canary_core::{
    ExecutionContext, GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType,
    ProtocolVersion, RunOptions, Status,
};
use canary_rpc::{
    DefaultRpcRunner, HttpRpcClient, RetryPolicy, RpcClient, RpcError, RpcFixture, RpcRunner,
};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Mounts an endpoint that answers every JSON-RPC request with the given
/// error object.
async fn json_rpc_error_server(code: i64, message: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": code, "message": message }
        })))
        .mount(&server)
        .await;
    server
}

/// A minimal `get-network` fixture with one trivially-true assertion.
fn get_network_fixture(id: &str) -> RpcFixture {
    let loaded = canary_fixtures::parse_fixture_str(
        &format!(
            "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"network\"\ndescription = \"test\"\nmethod = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n"
        ),
        std::path::Path::new("test.toml"),
    )
    .expect("fixture metadata parses");
    RpcFixture::from_loaded(&loaded).expect("fixture body parses")
}

fn context() -> ExecutionContext {
    ExecutionContext {
        protocol: ProtocolVersion(28),
        project: ProjectContext {
            root: ".".into(),
            name: "test".into(),
            project_type: ProjectType::Unknown,
            capabilities: vec![],
        },
        network: NetworkContext {
            name: NetworkName::Testnet,
            rpc_url: "https://example.invalid".into(),
            passphrase: "Test SDF Network ; September 2015".into(),
            observed_protocol: None,
        },
        fixtures: canary_core::FixtureStore::default(),
        git: GitContext::default(),
        cache: canary_core::CacheStore::new(
            std::env::temp_dir().join("canary-rpc-error-mapping-tests-cache"),
        ),
        options: RunOptions::default(),
    }
}

#[tokio::test]
async fn maps_minus_32601_to_method_not_available_preserving_the_method_name() {
    let server = json_rpc_error_server(-32601, "Method not found").await;
    let client = HttpRpcClient::new(server.uri());

    let err = client.get_latest_ledger().await.unwrap_err();
    match &err {
        RpcError::MethodNotAvailable { method } => assert_eq!(method, "getLatestLedger"),
        other => panic!("expected MethodNotAvailable, got {other:?}"),
    }

    // The diagnostic names both the method and the JSON-RPC code.
    let diagnostic = err.to_string();
    assert!(diagnostic.contains("getLatestLedger"), "{diagnostic}");
    assert!(diagnostic.contains("-32601"), "{diagnostic}");

    // Classification is protocol-level, not a transport/connection failure.
    assert!(!matches!(
        err,
        RpcError::Transport { .. } | RpcError::Timeout { .. } | RpcError::RateLimited { .. }
    ));
}

#[tokio::test]
async fn other_json_rpc_codes_map_to_json_rpc_error_not_method_not_available() {
    for (code, message) in [
        (-32600_i64, "Invalid Request"),
        (-32602, "Invalid params"),
        (-32000, "server error"),
    ] {
        let server = json_rpc_error_server(code, message).await;
        let client = HttpRpcClient::new(server.uri());

        let err = client.get_latest_ledger().await.unwrap_err();
        match &err {
            RpcError::JsonRpcError {
                method,
                code: got,
                message: got_message,
            } => {
                assert_eq!(method, "getLatestLedger");
                assert_eq!(*got, code);
                assert_eq!(got_message, message);
            }
            other => panic!("code {code} should map to JsonRpcError, got {other:?}"),
        }
        assert!(!matches!(err, RpcError::MethodNotAvailable { .. }));
    }
}

#[tokio::test]
async fn method_not_available_is_not_retried() {
    let server = json_rpc_error_server(-32601, "Method not found").await;
    let client = HttpRpcClient::new(server.uri());

    let err = client.get_latest_ledger().await.unwrap_err();
    assert!(matches!(err, RpcError::MethodNotAvailable { .. }));

    // The default policy retries transport failures three times; a method
    // the endpoint does not implement is deterministic, so exactly one
    // request must have been sent.
    let requests = server
        .received_requests()
        .await
        .expect("request recording enabled");
    assert_eq!(
        requests.len(),
        1,
        "-32601 must not be retried like a transient failure"
    );
}

#[tokio::test]
async fn runner_skips_the_check_when_the_method_is_not_available() {
    let server = json_rpc_error_server(-32601, "Method not found").await;
    let fixture = get_network_fixture("p28-rpc-error-mapping-skip");
    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));

    let result = runner.run(&fixture, &context()).await.expect("run");
    assert_eq!(result.status, Status::Skipped);
    assert!(
        result.summary.contains("method not available"),
        "{}",
        result.summary
    );
    assert!(result.summary.contains("getNetwork"), "{}", result.summary);
    let details = result.details.expect("the skip carries the diagnostic");
    assert!(details.contains("-32601"), "{details}");
}

#[tokio::test]
async fn runner_reports_an_error_for_other_json_rpc_codes_rather_than_skipping() {
    let server = json_rpc_error_server(-32602, "invalid params").await;
    let fixture = get_network_fixture("p28-rpc-error-mapping-error");
    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));

    let result = runner.run(&fixture, &context()).await.expect("run");
    assert_eq!(result.status, Status::Error);
    assert!(result.summary.starts_with("failed to call"));
    let details = result.details.expect("the error carries the diagnostic");
    assert!(details.contains("code -32602"), "{details}");
}

// ── HTTP 429 rate limiting and malformed bodies (issue #726) ────────────

/// Mounts an endpoint that answers every JSON-RPC request with the given
/// HTTP status and, when supplied, the `Retry-After` header a rate limiter
/// would send.
async fn status_server(status: u16, retry_after: Option<&str>) -> MockServer {
    let mut response = ResponseTemplate::new(status);
    if let Some(value) = retry_after {
        response = response.insert_header("Retry-After", value);
    }

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

/// A one-attempt policy. These tests assert the *mapping*; the retry loop
/// and its linear backoff are covered by `client_retry.rs`, so there is no
/// reason to sleep through the default 3-attempt schedule here.
fn single_attempt() -> RetryPolicy {
    RetryPolicy::from_retries(0, Duration::from_millis(1))
}

/// HTTP 429 is not a 5xx transport failure and not a JSON-RPC error object: it
/// is the endpoint refusing to do work right now, so it must surface as the
/// dedicated [`RpcError::RateLimited`] variant with the method name and the
/// number of attempts made. The `Retry-After` header does not change the
/// mapping — it is deliberately not honored (see `client_retry.rs`), so its
/// presence must not leak into the variant or the diagnostic.
#[tokio::test]
async fn maps_http_429_to_rate_limited_preserving_the_method_and_attempt_count() {
    let server = status_server(429, Some("120")).await;
    let client = HttpRpcClient::new(server.uri()).with_retry_policy(single_attempt());

    let err = client.get_latest_ledger().await.unwrap_err();
    match &err {
        RpcError::RateLimited { method, attempts } => {
            assert_eq!(method, "getLatestLedger");
            assert_eq!(*attempts, 1, "one attempt under a single-attempt policy");
        }
        other => panic!("429 must map to RateLimited, got {other:?}"),
    }

    // The rendered diagnostic is actionable on its own: it names the method,
    // the cause, and how many attempts were spent before giving up.
    let diagnostic = err.to_string();
    assert!(diagnostic.contains("getLatestLedger"), "{diagnostic}");
    assert!(diagnostic.contains("rate-limited"), "{diagnostic}");
    assert!(diagnostic.contains("1 attempt(s)"), "{diagnostic}");

    // It is a real `std::error::Error` (not just a Debug payload), so callers
    // can box it, chain it, and render it through `Display`.
    let as_error: &dyn std::error::Error = &err;
    assert_eq!(as_error.to_string(), diagnostic);
    assert!(
        std::error::Error::source(as_error).is_none(),
        "RateLimited carries its own detail and has no inner source"
    );

    // The mapping is terminal once the budget is spent: exactly one request.
    let requests = server
        .received_requests()
        .await
        .expect("request recording enabled");
    assert_eq!(requests.len(), 1);
}

/// A permanent 429 exhausts the configured attempt budget and reports every
/// attempt it made, so an operator can tell "the endpoint is busy" apart
/// from "we gave up after one try".
#[tokio::test]
async fn a_permanent_429_reports_the_exhausted_attempt_budget() {
    let server = status_server(429, Some("1")).await;
    let client = HttpRpcClient::new(server.uri())
        .with_retry_policy(RetryPolicy::from_retries(2, Duration::from_millis(1)));

    let err = client
        .get_network()
        .await
        .expect_err("the endpoint never stops rate-limiting");
    match &err {
        RpcError::RateLimited { method, attempts } => {
            assert_eq!(method, "getNetwork");
            assert_eq!(*attempts, 3, "one initial request plus two retries");
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
    assert!(err.to_string().contains("3 attempt(s)"), "{err}");

    let requests = server
        .received_requests()
        .await
        .expect("request recording enabled");
    assert_eq!(requests.len(), 3);
}

/// A `200 OK` whose body is not JSON at all — the classic case being an HTML
/// error page from a proxy in front of the RPC endpoint — is a parse error,
/// not a transport failure and not an invalid response shape. It must never
/// panic (the client parses, it does not unwrap) and must keep the serde
/// position detail that makes the bad body diagnosable.
#[tokio::test]
async fn maps_a_non_json_200_body_to_invalid_json_without_panicking() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html; charset=utf-8")
                .set_body_string("<html><body><h1>502 Bad Gateway</h1></body></html>"),
        )
        .mount(&server)
        .await;

    // Default policy on purpose: a parse error is deterministic, so the three
    // allowed attempts must collapse into a single request.
    let client = HttpRpcClient::new(server.uri());
    let err = client.get_latest_ledger().await.unwrap_err();

    match &err {
        RpcError::InvalidJson { method, message } => {
            assert_eq!(method, "getLatestLedger");
            assert!(
                message.contains("line"),
                "the serde detail keeps its position: {message}"
            );
        }
        other => panic!("a non-JSON body must map to InvalidJson, got {other:?}"),
    }

    let diagnostic = err.to_string();
    assert!(
        diagnostic.contains("invalid JSON response from getLatestLedger"),
        "{diagnostic}"
    );
    assert!(!matches!(
        err,
        RpcError::Transport { .. } | RpcError::Timeout { .. } | RpcError::RateLimited { .. }
    ));

    let requests = server
        .received_requests()
        .await
        .expect("request recording enabled");
    assert_eq!(requests.len(), 1, "a parse error must not be retried");
}

/// A body that starts out as JSON and then stops mid-object is the other half
/// of the parse-error mapping: it is still [`RpcError::InvalidJson`], never a
/// panic and never [`RpcError::InvalidResponse`] — the envelope is never even
/// inspected, because the document does not parse.
#[tokio::test]
async fn maps_a_truncated_json_200_body_to_invalid_json_rather_than_a_panic() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_string("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":"),
        )
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_network().await.unwrap_err();

    match &err {
        RpcError::InvalidJson { method, message } => {
            assert_eq!(method, "getNetwork");
            assert!(
                message.contains("EOF"),
                "a truncated document reports the premature end of input: {message}"
            );
        }
        other => panic!("a truncated body must map to InvalidJson, got {other:?}"),
    }
    assert!(!matches!(err, RpcError::InvalidResponse { .. }));
    assert!(err.to_string().contains("getNetwork"), "{err}");
}

/// The runner turns a rate-limited endpoint into an execution error rather
/// than a skip: `Status::Skipped` is reserved for a method the endpoint does
/// not implement (`-32601`), while a 429 means the check could not complete.
/// The rate-limit diagnostic rides along in `details` so the report explains
/// the failure without the operator re-running with `--verbose`.
#[tokio::test]
async fn runner_reports_rate_limiting_as_an_execution_error_with_the_diagnostic() {
    let server = status_server(429, Some("60")).await;
    let fixture = get_network_fixture("p28-rpc-error-mapping-rate-limited");
    let client = HttpRpcClient::new(server.uri()).with_retry_policy(single_attempt());
    let runner = DefaultRpcRunner::new(client);

    let result = runner.run(&fixture, &context()).await.expect("run");
    assert_eq!(
        result.status,
        Status::Error,
        "a rate-limited endpoint is an execution error, not a skip"
    );
    assert!(result.summary.starts_with("failed to call"));
    let details = result.details.expect("the error carries the diagnostic");
    assert!(details.contains("rate-limited"), "{details}");
    assert!(details.contains("getNetwork"), "{details}");
}

#[tokio::test]
async fn runner_keeps_transport_failures_as_errors_not_skips() {
    // No listener on port 0: the connection fails before any JSON-RPC
    // exchange, so this must stay an error — only -32601 skips.
    let fixture = get_network_fixture("p28-rpc-error-mapping-transport");
    let runner = DefaultRpcRunner::new(HttpRpcClient::new("http://127.0.0.1:0"));

    let result = runner.run(&fixture, &context()).await.expect("run");
    assert_eq!(result.status, Status::Error);
    assert!(result.summary.starts_with("failed to call"));
}
