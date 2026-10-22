//! Unit and mock-server tests for `getLedgers` (E-027).
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
    DefaultRpcRunner, HttpRpcClient, LedgerHeaderInfo, RpcClient, RpcError, RpcFixture, RpcMethod,
    RpcRunner,
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
        cache: CacheStore::new(std::env::temp_dir().join("canary-rpc-get-ledgers-tests-cache")),
        options: RunOptions::default(),
    }
}

fn load_fixture(id: &str, body_toml: &str) -> RpcFixture {
    let loaded = parse_fixture_str(
        &format!(
            "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"ledgers\"\ndescription = \"getLedgers check\"\n{body_toml}"
        ),
        Path::new("get_ledgers.toml"),
    )
    .expect("valid fixture header");
    RpcFixture::from_loaded(&loaded).expect("valid rpc fixture")
}

/// One ledger entry the way a real `getLedgers` result reports it.
fn ledger_entry(sequence: u32) -> serde_json::Value {
    json!({
        "hash": format!("{sequence:064x}"),
        "sequence": sequence,
        "ledgerCloseTime": "1722464376",
        "headerXdr": "AAAA",
        "metadataXdr": "BBBB"
    })
}

fn result_with_ledgers(sequences: &[u32]) -> serde_json::Value {
    json!({
        "ledgers": sequences
            .iter()
            .map(|sequence| ledger_entry(*sequence))
            .collect::<Vec<_>>(),
        "latestLedger": 500,
        "latestLedgerCloseTime": "1722464400",
        "oldestLedger": 1,
        "oldestLedgerCloseTime": "1722460000",
        "cursor": sequences.last().map(u32::to_string),
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
async fn get_ledgers_sends_start_ledger_and_a_pagination_limit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgers",
            "params": { "startLedger": 456, "pagination": { "limit": 10 } }
        })))
        .respond_with(success_envelope(result_with_ledgers(&[456, 457])))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client
        .get_ledgers(456, Some(10))
        .await
        .expect("getLedgers succeeds");

    assert_eq!(response.ledgers.len(), 2);
    assert_eq!(response.ledgers[0].sequence, 456);
    assert_eq!(response.ledgers[1].sequence, 457);
    assert_eq!(response.latest_ledger, Some(500));
    assert_eq!(response.cursor.as_deref(), Some("457"));
    assert_eq!(
        response.ledgers[0],
        LedgerHeaderInfo {
            hash: format!("{:064x}", 456),
            sequence: 456,
            ledger_close_time: "1722464376".to_string(),
            header_xdr: "AAAA".to_string(),
            metadata_xdr: Some("BBBB".to_string()),
        }
    );
}

#[tokio::test]
async fn get_ledgers_omits_pagination_when_no_limit_is_given() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgers",
            "params": { "startLedger": 456 }
        })))
        .respond_with(success_envelope(result_with_ledgers(&[456])))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client.get_ledgers(456, None).await.expect("ok");
    assert_eq!(response.ledgers.len(), 1);
}

#[tokio::test]
async fn get_ledgers_parses_an_empty_range_cleanly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({ "ledgers": [] })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client.get_ledgers(456, Some(10)).await.expect("ok");
    assert!(response.is_empty());
    assert_eq!(response.latest_ledger, None);
}

#[tokio::test]
async fn get_ledgers_rejects_a_zero_start_ledger_before_sending_a_request() {
    let server = MockServer::start().await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_ledgers(0, None).await.unwrap_err();
    match err {
        RpcError::InvalidArgument { method, reason } => {
            assert_eq!(method, "getLedgers");
            assert!(reason.contains("startLedger"), "{reason}");
            assert!(reason.contains("greater than 0"), "{reason}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    // A negative sequence cannot be passed at all: the parameter is a u32,
    // so `0` is the only out-of-range value that reaches this validation.
    let requests = server.received_requests().await.expect("recorded requests");
    assert!(
        requests.is_empty(),
        "nothing may be sent for a bad argument"
    );
}

#[tokio::test]
async fn get_ledgers_rejects_limits_outside_the_upstream_range() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(result_with_ledgers(&[456])))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    for limit in [0, 201, u32::MAX] {
        let err = client.get_ledgers(456, Some(limit)).await.unwrap_err();
        match err {
            RpcError::InvalidArgument { method, reason } => {
                assert_eq!(method, "getLedgers");
                assert!(reason.contains("pagination.limit"), "{reason}");
            }
            other => panic!("limit {limit} should be InvalidArgument, got {other:?}"),
        }
    }

    // The documented maximum is accepted.
    assert!(client.get_ledgers(456, Some(200)).await.is_ok());

    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(requests.len(), 1, "only the valid limit reached the wire");
}

#[tokio::test]
async fn get_ledgers_raw_keeps_fields_the_typed_model_drops() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({
            "ledgers": [{
                "hash": "26a4b6c1",
                "sequence": 456,
                "ledgerCloseTime": "1722464376",
                "headerXdr": "AAAA",
                "someFutureField": { "nested": true }
            }],
            "someOtherFutureField": 7
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let raw = client
        .get_ledgers_raw(json!({ "startLedger": 456 }))
        .await
        .expect("ok");
    assert_eq!(
        raw["ledgers"][0]["someFutureField"],
        json!({ "nested": true })
    );
    assert_eq!(raw["someOtherFutureField"], json!(7));
}

#[tokio::test]
async fn get_ledgers_raw_rejects_a_result_that_is_not_an_object() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!([1, 2])))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_ledgers_raw(json!({})).await.unwrap_err();
    match err {
        RpcError::InvalidResponse { method, reason } => {
            assert_eq!(method, "getLedgers");
            assert!(reason.contains("array"), "{reason}");
        }
        other => panic!("expected InvalidResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn runner_executes_a_get_ledgers_fixture_with_pagination() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgers",
            "params": { "startLedger": 456, "pagination": { "limit": 10 } }
        })))
        .respond_with(success_envelope(result_with_ledgers(&[456])))
        .mount(&server)
        .await;

    let pass_fixture = load_fixture(
        "p28-rpc-ledgers-pass",
        r#"
        method = "get-ledgers"

        [request]
        startLedger = 456

        [request.pagination]
        limit = 10

        [[assert]]
        kind = "field-type"
        field = "latestLedger"
        expected_type = "number"

        [[assert]]
        kind = "field-equals"
        field = "cursor"
        value = "456"

        [[assert]]
        kind = "field-type"
        field = "ledgers"
        expected_type = "array"

        [[assert]]
        kind = "field-exists"
        field = "oldestLedger"
        "#,
    );
    assert_eq!(pass_fixture.method, RpcMethod::GetLedgers);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let pass_result = runner.run(&pass_fixture, &context()).await.unwrap();
    assert_eq!(
        pass_result.status,
        Status::Pass,
        "details: {:?}",
        pass_result.details
    );

    let fail_fixture = load_fixture(
        "p28-rpc-ledgers-fail",
        r#"
        method = "get-ledgers"

        [request]
        startLedger = 456

        [request.pagination]
        limit = 10

        [[assert]]
        kind = "field-equals"
        field = "cursor"
        value = "999"
        "#,
    );
    let fail_result = runner.run(&fail_fixture, &context()).await.unwrap();
    assert_eq!(fail_result.status, Status::Fail);
}
