//! Unit and mock-server tests for `getLedgerEntries` (E-025).
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
    DefaultRpcRunner, GetLedgerEntriesResponse, HttpRpcClient, RpcClient, RpcError, RpcFixture,
    RpcMethod, RpcRunner,
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
        cache: CacheStore::new(std::env::temp_dir().join("canary-rpc-ledger-entries-tests-cache")),
        options: RunOptions::default(),
    }
}

fn load_fixture(id: &str, body_toml: &str) -> RpcFixture {
    let loaded = parse_fixture_str(
        &format!(
            "id = \"{id}\"\nprotocol = 29\nsurface = \"rpc\"\ncategory = \"entries\"\ndescription = \"getLedgerEntries check\"\n{body_toml}"
        ),
        Path::new("get_ledger_entries.toml"),
    )
    .expect("valid fixture header");
    RpcFixture::from_loaded(&loaded).expect("valid rpc fixture")
}

fn ledger_entries_result() -> serde_json::Value {
    json!({
        "entries": [
            {
                "key": "AAAAAA==",
                "xdr": "BBBBBB==",
                "lastModifiedLedgerSeq": 123,
                "liveUntilLedgerSeq": 456
            },
            {
                "key": "CCCCCC==",
                "xdr": "DDDDDD==",
                "lastModifiedLedgerSeq": 124
            }
        ],
        "latestLedger": 500
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
async fn get_ledger_entries_sends_the_base64_keys_and_parses_the_entries() {
    let server = MockServer::start().await;
    let keys = vec!["AAAAAA==".to_string(), "CCCCCC==".to_string()];
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgerEntries",
            "params": { "keys": keys }
        })))
        .respond_with(success_envelope(ledger_entries_result()))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client
        .get_ledger_entries(&keys)
        .await
        .expect("getLedgerEntries succeeds");

    assert!(!response.is_empty());
    assert_eq!(response.latest_ledger, Some(500));
    assert_eq!(response.entries.len(), 2);
    assert_eq!(response.entries[0].key, "AAAAAA==");
    assert_eq!(response.entries[0].xdr, "BBBBBB==");
    assert_eq!(response.entries[0].last_modified_ledger_seq, 123);
    assert_eq!(response.entries[0].live_until_ledger_seq, Some(456));
    assert_eq!(response.entries[1].last_modified_ledger_seq, 124);
    assert!(response.entries[1].live_until_ledger_seq.is_none());
}

#[tokio::test]
async fn get_ledger_entries_parses_an_empty_result_cleanly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(
            json!({ "entries": [], "latestLedger": 500 }),
        ))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client
        .get_ledger_entries(&["AAAAAA==".to_string()])
        .await
        .expect("ok");
    assert!(response.is_empty());
    assert_eq!(response.latest_ledger, Some(500));
}

#[tokio::test]
async fn get_ledger_entries_rejects_invalid_base64_before_sending_a_request() {
    let server = MockServer::start().await;

    let client = HttpRpcClient::new(server.uri());
    for bad in ["not base64!", "AAAAA", "", "AA=A"] {
        let err = client
            .get_ledger_entries(&[bad.to_string()])
            .await
            .unwrap_err();
        match err {
            RpcError::InvalidArgument { method, reason } => {
                assert_eq!(method, "getLedgerEntries");
                assert!(reason.contains("keys[0]"), "{bad:?}: {reason}");
                assert!(reason.contains("base64"), "{bad:?}: {reason}");
            }
            other => panic!("expected InvalidArgument for {bad:?}, got {other:?}"),
        }
    }

    let requests = server.received_requests().await.expect("recorded requests");
    assert!(
        requests.is_empty(),
        "nothing may be sent for a malformed key"
    );
}

#[tokio::test]
async fn get_ledger_entries_rejects_empty_and_oversized_key_lists_before_sending() {
    let server = MockServer::start().await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_ledger_entries(&[]).await.unwrap_err();
    assert!(matches!(
        err,
        RpcError::InvalidArgument { ref method, .. } if method == "getLedgerEntries"
    ));

    let too_many = vec!["AAAAAA==".to_string(); 201];
    let err = client.get_ledger_entries(&too_many).await.unwrap_err();
    match err {
        RpcError::InvalidArgument { method, reason } => {
            assert_eq!(method, "getLedgerEntries");
            assert!(reason.contains("201"), "{reason}");
            assert!(reason.contains("200"), "{reason}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    let requests = server.received_requests().await.expect("recorded requests");
    assert!(
        requests.is_empty(),
        "nothing may be sent for an out-of-range key list"
    );
}

#[tokio::test]
async fn get_ledger_entries_accepts_the_documented_maximum_number_of_keys() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({ "entries": [] })))
        .mount(&server)
        .await;

    let keys = vec!["AAAAAA==".to_string(); 200];
    let client = HttpRpcClient::new(server.uri());
    assert!(client.get_ledger_entries(&keys).await.is_ok());
}

#[tokio::test]
async fn get_ledger_entries_reports_a_missing_last_modified_sequence_as_invalid() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({
            "entries": [{ "key": "AAAAAA==", "xdr": "BBBBBB==" }],
            "latestLedger": 500
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client
        .get_ledger_entries(&["AAAAAA==".to_string()])
        .await
        .unwrap_err();
    match err {
        RpcError::InvalidResponse { method, reason } => {
            assert_eq!(method, "getLedgerEntries");
            assert!(reason.contains("lastModifiedLedgerSeq"), "{reason}");
        }
        other => panic!("expected InvalidResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn get_ledger_entries_raw_keeps_fields_the_typed_model_drops() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(success_envelope(json!({
            "entries": [{
                "key": "AAAAAA==",
                "xdr": "BBBBBB==",
                "lastModifiedLedgerSeq": 123,
                "someFutureField": { "nested": true }
            }],
            "someOtherFutureField": 7
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let raw = client
        .get_ledger_entries_raw(json!({ "keys": ["AAAAAA=="] }))
        .await
        .expect("ok");
    assert_eq!(
        raw["entries"][0]["someFutureField"],
        json!({ "nested": true })
    );
    assert_eq!(raw["someOtherFutureField"], json!(7));
}

#[tokio::test]
async fn runner_executes_a_get_ledger_entries_fixture() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgerEntries",
            "params": { "keys": ["AAAAAA==", "CCCCCC=="] }
        })))
        .respond_with(success_envelope(ledger_entries_result()))
        .mount(&server)
        .await;

    let fixture = load_fixture(
        "p29-rpc-ledger-entries",
        r#"
        method = "get-ledger-entries"

        [request]
        keys = ["AAAAAA==", "CCCCCC=="]

        [[assert]]
        kind = "field-type"
        field = "entries"
        expected_type = "array"

        [[assert]]
        kind = "field-number-range"
        field = "latestLedger"
        min = 1
        "#,
    );
    assert_eq!(fixture.method, RpcMethod::GetLedgerEntries);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&fixture, &context()).await.unwrap();
    assert_eq!(result.status, Status::Pass, "details: {:?}", result.details);
}

#[test]
fn ledger_entries_response_is_exported() {
    let response = GetLedgerEntriesResponse {
        entries: vec![],
        latest_ledger: None,
    };
    assert!(response.is_empty());
}
