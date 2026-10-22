//! Unit and mock-server tests for `getFeeStats` (E-018).
//!
//! The method is checked at the three layers it lives in: the typed client
//! call (the exact JSON-RPC params it sends and the response it parses), the
//! raw call the runner asserts against, and a fixture end to end through
//! [`DefaultRpcRunner`].

use std::path::Path;

use canary_core::{
    CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext, NetworkName,
    ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::{
    DefaultRpcRunner, HttpRpcClient, RpcClient, RpcError, RpcFixture, RpcMethod, RpcRunner,
};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn context() -> ExecutionContext {
    ExecutionContext {
        protocol: ProtocolVersion(29),
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
        cache: CacheStore::new(std::env::temp_dir().join("canary-rpc-fee-stats-tests-cache")),
        options: RunOptions::default(),
    }
}

fn load_fixture(id: &str, body_toml: &str) -> RpcFixture {
    let loaded = parse_fixture_str(
        &format!(
            "id = \"{id}\"\nprotocol = 29\nsurface = \"rpc\"\ncategory = \"fees\"\ndescription = \"getFeeStats check\"\n{body_toml}"
        ),
        Path::new("get_fee_stats.toml"),
    )
    .expect("valid fixture header");
    RpcFixture::from_loaded(&loaded).expect("valid rpc fixture")
}

fn fee_distribution(base: u32) -> serde_json::Value {
    json!({
        "max": format!("{}", base * 100),
        "min": format!("{base}"),
        "mode": format!("{}", base * 10),
        "p10": format!("{}", base * 2),
        "p20": format!("{}", base * 3),
        "p30": format!("{}", base * 4),
        "p40": format!("{}", base * 5),
        "p50": format!("{}", base * 6),
        "p60": format!("{}", base * 7),
        "p70": format!("{}", base * 8),
        "p80": format!("{}", base * 9),
        "p90": format!("{}", base * 11),
        "p95": format!("{}", base * 12),
        "p99": format!("{}", base * 13),
        "transactionCount": format!("{}", base * 20),
        "ledgerCount": 50
    })
}

fn fee_stats_result() -> serde_json::Value {
    json!({
        "sorobanInclusionFee": fee_distribution(2),
        "inclusionFee": fee_distribution(1),
        "latestLedger": 4519945
    })
}

fn success_envelope(result: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": result
    }))
}

#[tokio::test]
async fn get_fee_stats_sends_no_params_and_parses_both_distributions() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getFeeStats",
            "params": {}
        })))
        .respond_with(success_envelope(fee_stats_result()))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client.get_fee_stats().await.expect("getFeeStats succeeds");

    assert_eq!(response.latest_ledger, 4519945);
    assert_eq!(response.soroban_inclusion_fee.max, "200");
    assert_eq!(response.soroban_inclusion_fee.p50, "12");
    assert_eq!(response.soroban_inclusion_fee.transaction_count, "40");
    assert_eq!(response.inclusion_fee.min, "1");
    assert_eq!(response.inclusion_fee.ledger_count, 50);
}

#[tokio::test]
async fn get_fee_stats_rejects_a_string_latest_ledger_as_an_invalid_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({
            "sorobanInclusionFee": fee_distribution(1),
            "inclusionFee": fee_distribution(1),
            "latestLedger": "4519945"
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_fee_stats().await.unwrap_err();
    match err {
        RpcError::InvalidResponse { method, reason } => {
            assert_eq!(method, "getFeeStats");
            assert!(reason.contains("invalid type"), "{reason}");
            assert!(reason.contains("u32"), "{reason}");
        }
        other => panic!("expected InvalidResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn get_fee_stats_raw_keeps_fields_the_typed_model_drops() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({
            "sorobanInclusionFee": fee_distribution(1),
            "inclusionFee": fee_distribution(1),
            "latestLedger": 4519945,
            "someFutureField": { "nested": true }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let raw = client.get_fee_stats_raw().await.expect("ok");
    assert_eq!(raw["someFutureField"], json!({ "nested": true }));
    assert_eq!(raw["inclusionFee"]["p50"], json!("6"));
}

#[tokio::test]
async fn runner_executes_a_get_fee_stats_fixture() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getFeeStats",
            "params": {}
        })))
        .respond_with(success_envelope(fee_stats_result()))
        .mount(&server)
        .await;

    let fixture = load_fixture(
        "p29-rpc-fee-stats",
        r#"
        method = "get-fee-stats"

        [[assert]]
        kind = "field-type"
        field = "sorobanInclusionFee"
        expected_type = "object"

        [[assert]]
        kind = "field-type"
        field = "inclusionFee"
        expected_type = "object"

        [[assert]]
        kind = "field-number-range"
        field = "latestLedger"
        min = 1
        "#,
    );
    assert_eq!(fixture.method, RpcMethod::GetFeeStats);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&fixture, &context()).await.unwrap();
    assert_eq!(result.status, Status::Pass, "details: {:?}", result.details);
}
