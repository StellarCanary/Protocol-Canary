//! Unit and mock-server tests for `getVersionInfo` (E-017).
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
    VersionInfoResponse,
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
        cache: CacheStore::new(std::env::temp_dir().join("canary-rpc-version-info-tests-cache")),
        options: RunOptions::default(),
    }
}

fn load_fixture(id: &str, body_toml: &str) -> RpcFixture {
    let loaded = parse_fixture_str(
        &format!(
            "id = \"{id}\"\nprotocol = 29\nsurface = \"rpc\"\ncategory = \"version\"\ndescription = \"getVersionInfo check\"\n{body_toml}"
        ),
        Path::new("get_version_info.toml"),
    )
    .expect("valid fixture header");
    RpcFixture::from_loaded(&loaded).expect("valid rpc fixture")
}

fn version_info_result() -> serde_json::Value {
    json!({
        "version": "29.0.0-b2b701685c79aee17fe4eb22dbd08a5dfd11594d",
        "commitHash": "b2b701685c79aee17fe4eb22dbd08a5dfd11594d",
        "buildTimestamp": "2026-09-22T14:52:44",
        "captiveCoreVersion": "stellar-core 29.0.0 (b2b701685c79aee17fe4eb22dbd08a5dfd11594d)",
        "protocolVersion": 29
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
async fn get_version_info_sends_no_params_and_parses_the_observed_protocol() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getVersionInfo",
            "params": {}
        })))
        .respond_with(success_envelope(version_info_result()))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client
        .get_version_info()
        .await
        .expect("getVersionInfo succeeds");

    assert_eq!(response.observed_protocol(), 29);
    assert_eq!(
        response.version,
        "29.0.0-b2b701685c79aee17fe4eb22dbd08a5dfd11594d"
    );
    assert_eq!(
        response.commit_hash,
        "b2b701685c79aee17fe4eb22dbd08a5dfd11594d"
    );
    assert_eq!(response.build_timestamp, "2026-09-22T14:52:44");
    assert!(response
        .captive_core_version
        .as_deref()
        .unwrap()
        .starts_with("stellar-core 29.0.0"));
}

#[tokio::test]
async fn get_version_info_tolerates_a_missing_captive_core_version() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({
            "version": "29.0.0",
            "commitHash": "abc",
            "buildTimestamp": "2026-09-22T14:52:44",
            "protocolVersion": 29
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client.get_version_info().await.expect("ok");
    assert!(response.captive_core_version.is_none());
    assert_eq!(response.observed_protocol(), 29);
}

#[tokio::test]
async fn get_version_info_reports_a_malformed_protocol_version_as_an_invalid_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({
            "version": "29.0.0",
            "commitHash": "abc",
            "buildTimestamp": "2026-09-22T14:52:44",
            "protocolVersion": "29"
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_version_info().await.unwrap_err();
    match err {
        RpcError::InvalidResponse { method, reason } => {
            assert_eq!(method, "getVersionInfo");
            assert!(reason.contains("invalid type"), "{reason}");
            assert!(reason.contains("u32"), "{reason}");
        }
        other => panic!("expected InvalidResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn get_version_info_raw_keeps_fields_the_typed_model_drops() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({
            "version": "29.0.0",
            "commitHash": "abc",
            "buildTimestamp": "2026-09-22T14:52:44",
            "protocolVersion": 29,
            "someFutureField": { "nested": true }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let raw = client.get_version_info_raw().await.expect("ok");
    assert_eq!(raw["someFutureField"], json!({ "nested": true }));
}

#[tokio::test]
async fn runner_executes_a_get_version_info_fixture() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getVersionInfo",
            "params": {}
        })))
        .respond_with(success_envelope(version_info_result()))
        .mount(&server)
        .await;

    let fixture = load_fixture(
        "p29-rpc-version-info",
        r#"
        method = "get-version-info"

        [[assert]]
        kind = "field-type"
        field = "protocolVersion"
        expected_type = "number"

        [[assert]]
        kind = "field-equals"
        field = "protocolVersion"
        value = 29

        [[assert]]
        kind = "field-type"
        field = "version"
        expected_type = "string"
        "#,
    );
    assert_eq!(fixture.method, RpcMethod::GetVersionInfo);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&fixture, &context()).await.unwrap();
    assert_eq!(result.status, Status::Pass, "details: {:?}", result.details);
}

#[test]
fn version_info_response_is_exported_with_the_observed_protocol_accessor() {
    let response = VersionInfoResponse {
        version: "29.0.0".to_string(),
        commit_hash: "abc".to_string(),
        build_timestamp: "2026-09-22T14:52:44".to_string(),
        captive_core_version: None,
        protocol_version: 29,
    };
    assert_eq!(response.observed_protocol(), 29);
}
