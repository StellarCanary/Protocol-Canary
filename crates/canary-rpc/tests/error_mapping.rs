//! JSON-RPC error classification tests (issue #480).
//!
//! - `-32601` ("Method not found") maps to [`RpcError::MethodNotAvailable`],
//!   preserving the method name in the diagnostic, is never retried, and is
//!   skipped by the runner rather than reported as a fatal endpoint error.
//! - Every other JSON-RPC code (`-32600`, `-32602`, ...) keeps mapping to
//!   the distinct [`RpcError::JsonRpcError`] variant and stays an error.
//! - Transport failures remain `Status::Error`: skipping is reserved for a
//!   method the endpoint does not implement, not for connection problems.

use canary_core::{
    ExecutionContext, GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType,
    ProtocolVersion, RunOptions, Status,
};
use canary_rpc::{DefaultRpcRunner, HttpRpcClient, RpcClient, RpcError, RpcFixture, RpcRunner};
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
