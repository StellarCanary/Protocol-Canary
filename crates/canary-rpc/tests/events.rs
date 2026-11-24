//! Multi-topic event assertions for `getEvents` (`[E-118]`, issue #516).
//!
//! The assertion reads the raw JSON `result` object (CA-01 section 4.1), so
//! these tests also pin that an untyped event payload survives the round
//! trip and that a mismatching topic is a clean `fail`, never an `error`.

use std::path::Path;

use canary_core::{
    CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext, NetworkName,
    ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::{
    DefaultRpcRunner, HttpRpcClient, RpcClient, RpcError, RpcFixture, RpcFixtureError, RpcMethod,
    RpcRunner,
};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const EVENTS_RESULT: &str = r#"{
    "latestLedger": 1000,
    "events": [
        {
            "ledger": 100,
            "contractId": "CAAA",
            "type": "contract",
            "topic": ["AAA", "BBB"],
            "value": { "xdr": "CCCC", "unmodeled": [1, 2, 3] }
        },
        {
            "ledger": 101,
            "contractId": "CAAA",
            "type": "contract",
            "topic": ["AAA", "BBB", "DDD"],
            "value": "EEEE"
        }
    ]
}"#;

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
        cache: CacheStore::new(std::env::temp_dir().join("canary-rpc-events-tests-cache")),
        options: RunOptions::default(),
    }
}

fn load_fixture(id: &str, body_toml: &str) -> RpcFixture {
    let loaded = parse_fixture_str(
        &format!(
            "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"events\"\ndescription = \"getEvents check\"\n{body_toml}"
        ),
        Path::new("events.toml"),
    )
    .expect("valid fixture header");
    RpcFixture::from_loaded(&loaded).expect("valid rpc fixture")
}

async fn server_with_events() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": serde_json::from_str::<serde_json::Value>(EVENTS_RESULT).unwrap()
        })))
        .mount(&server)
        .await;
    server
}

#[test]
fn get_events_is_a_supported_fixture_method() {
    assert_eq!(
        "get-events".parse::<RpcMethod>().unwrap(),
        RpcMethod::GetEvents
    );
}

#[test]
fn all_topics_of_an_event_are_asserted_in_order() {
    let fixture = load_fixture(
        "p28-rpc-events-multi-topic",
        r#"
        method = "get-events"

        [request]
        startLedger = 100

        [[assert]]
        kind = "event-topics"
        index = 1
        topics = ["AAA", "BBB", "DDD"]
        "#,
    );
    assert_eq!(fixture.method, RpcMethod::GetEvents);
    assert_eq!(
        fixture
            .request
            .as_ref()
            .and_then(|params| params.get("startLedger")),
        Some(&json!(100))
    );
}

#[tokio::test]
async fn runner_passes_a_matching_multi_topic_assertion() {
    let server = server_with_events().await;
    let fixture = load_fixture(
        "p28-rpc-events-pass",
        r#"
        method = "get-events"

        [request]
        startLedger = 100

        [[assert]]
        kind = "event-topics"
        index = 0
        topics = ["AAA", "BBB"]

        [[assert]]
        kind = "event-topics"
        index = 1
        topics = ["AAA", "BBB", "DDD"]

        [[assert]]
        kind = "field-equals"
        field = "latestLedger"
        value = 1000
        "#,
    );

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&fixture, &context()).await.unwrap();
    assert_eq!(result.status, Status::Pass, "{:?}", result.details);
}

#[tokio::test]
async fn runner_fails_cleanly_when_a_topic_differs() {
    let server = server_with_events().await;
    let fixture = load_fixture(
        "p28-rpc-events-topic-mismatch",
        r#"
        method = "get-events"

        [request]
        startLedger = 100

        [[assert]]
        kind = "event-topics"
        index = 0
        topics = ["AAA", "ZZZ"]
        "#,
    );

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&fixture, &context()).await.unwrap();
    assert_eq!(result.status, Status::Fail);
    let details = result.details.expect("a failure explains itself");
    assert!(
        details.contains("topic 1") && details.contains("ZZZ"),
        "{details}"
    );
}

#[tokio::test]
async fn runner_fails_cleanly_when_the_event_is_missing() {
    let server = server_with_events().await;
    let fixture = load_fixture(
        "p28-rpc-events-missing",
        r#"
        method = "get-events"

        [request]
        startLedger = 100

        [[assert]]
        kind = "event-topics"
        index = 7
        topics = ["AAA"]
        "#,
    );

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&fixture, &context()).await.unwrap();
    assert_eq!(result.status, Status::Fail);
    let details = result.details.expect("a failure explains itself");
    assert!(details.contains("index 7"), "{details}");
    assert!(details.contains("2 event(s)"), "{details}");
}

#[tokio::test]
async fn raw_event_payloads_keep_fields_the_models_do_not_name() {
    let server = server_with_events().await;
    let client = HttpRpcClient::new(server.uri());
    let raw = client
        .get_events_raw(json!({ "startLedger": 100 }))
        .await
        .expect("getEvents succeeds");

    assert_eq!(raw["events"][0]["value"]["unmodeled"], json!([1, 2, 3]));
    assert_eq!(raw["events"][0]["topic"], json!(["AAA", "BBB"]));
}

#[tokio::test]
async fn get_events_maps_method_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32601, "message": "Method not found" }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    match client.get_events_raw(json!({})).await.unwrap_err() {
        RpcError::MethodNotAvailable { method } => assert_eq!(method, "getEvents"),
        other => panic!("expected MethodNotAvailable, got {other:?}"),
    }
}

#[test]
fn an_event_assertion_without_topics_is_an_invalid_fixture() {
    let loaded = parse_fixture_str(
        r#"
        id = "p28-rpc-events-empty"
        protocol = 28
        surface = "rpc"
        category = "events"
        description = "getEvents check"

        method = "get-events"

        [[assert]]
        kind = "event-topics"
        index = 0
        topics = []
        "#,
        Path::new("events.toml"),
    )
    .expect("valid fixture header");

    match RpcFixture::from_loaded(&loaded) {
        Err(RpcFixtureError::InvalidFixtureBody { reason, .. }) => {
            assert!(reason.contains("at least one topic"), "{reason}");
        }
        other => panic!("expected InvalidFixtureBody, got {other:?}"),
    }
}
