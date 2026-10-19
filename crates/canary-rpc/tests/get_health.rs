//! Unit and mock-server integration tests for `getHealth` (`[E-016]`).

use std::path::Path;

use canary_core::{
    CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext, NetworkName,
    ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::{
    observe_health, DefaultRpcRunner, GetHealthResponse, HttpRpcClient, RpcClient, RpcError,
    RpcFixture, RpcMethod, RpcRunner,
};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
        fixtures: FixtureStore::default(),
        git: GitContext::default(),
        cache: CacheStore::new(std::env::temp_dir().join("canary-rpc-get-health-tests-cache")),
        options: RunOptions::default(),
    }
}

fn load_fixture(id: &str, body_toml: &str) -> RpcFixture {
    let loaded = parse_fixture_str(
        &format!(
            "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"network\"\ndescription = \"getHealth check\"\n{body_toml}"
        ),
        Path::new("get_health.toml"),
    )
    .expect("valid fixture header");
    RpcFixture::from_loaded(&loaded).expect("valid rpc fixture")
}

#[tokio::test]
async fn get_health_sends_jsonrpc_envelope_and_parses_minimal_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getHealth",
            "params": {}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy"
            }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client.get_health().await.expect("getHealth succeeds");
    assert_eq!(
        response,
        GetHealthResponse {
            status: "healthy".to_string(),
            latest_ledger: None,
            oldest_ledger: None,
            ledger_retention_window: None,
            latest_ledger_close_time: None,
            oldest_ledger_close_time: None,
        }
    );
}

#[tokio::test]
async fn get_health_parses_full_stellar_rpc_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getHealth",
            "params": {}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy",
                "latestLedger": 1_234_567,
                "oldestLedger": 1_113_608,
                "ledgerRetentionWindow": 120_960,
                "latestLedgerCloseTime": "1728463080",
                "oldestLedgerCloseTime": "1727858280"
            }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = observe_health(&client)
        .await
        .expect("observe_health succeeds");
    assert_eq!(response.status, "healthy");
    assert_eq!(response.latest_ledger, Some(1_234_567));
    assert_eq!(response.oldest_ledger, Some(1_113_608));
    assert_eq!(response.ledger_retention_window, Some(120_960));
    assert_eq!(
        response.latest_ledger_close_time.as_deref(),
        Some("1728463080")
    );
    assert_eq!(
        response.oldest_ledger_close_time.as_deref(),
        Some("1727858280")
    );
}

#[tokio::test]
async fn get_health_maps_method_not_found_and_jsonrpc_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32601, "message": "Method not found" }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_health().await.unwrap_err();
    match err {
        RpcError::MethodNotAvailable { method } => assert_eq!(method, "getHealth"),
        other => panic!("expected MethodNotAvailable, got {other:?}"),
    }
}

#[tokio::test]
async fn get_health_rejects_missing_status_field() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "latestLedger": 100 }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_health().await.unwrap_err();
    match err {
        RpcError::InvalidResponse { method, reason } => {
            assert_eq!(method, "getHealth");
            assert!(reason.contains("status"), "{reason}");
        }
        other => panic!("expected InvalidResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn runner_executes_get_health_fixture_pass_and_fail() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "healthy",
                "latestLedger": 500,
                "ledgerRetentionWindow": 120960
            }
        })))
        .mount(&server)
        .await;

    let pass_fixture = load_fixture(
        "p28-rpc-health-pass",
        r#"
        method = "get-health"

        [[assert]]
        kind = "field-equals"
        field = "status"
        value = "healthy"

        [[assert]]
        kind = "field-type"
        field = "latestLedger"
        expected_type = "number"
        "#,
    );
    assert_eq!(pass_fixture.method, RpcMethod::GetHealth);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let pass_result = runner.run(&pass_fixture, &context()).await.unwrap();
    assert_eq!(pass_result.status, Status::Pass);

    let fail_fixture = load_fixture(
        "p28-rpc-health-fail",
        r#"
        method = "get-health"

        [[assert]]
        kind = "field-equals"
        field = "status"
        value = "unhealthy"
        "#,
    );
    let fail_result = runner.run(&fail_fixture, &context()).await.unwrap();
    assert_eq!(fail_result.status, Status::Fail);
}
