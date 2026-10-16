//! Simulation failure mapping (issue #504).
//!
//! `simulateTransaction` can fail in several ways. Each must come back from
//! [`DefaultSorobanRunner`] as a structured [`CompatibilityResult`] and never
//! as a panic or an `Err`, so a contract developer can tell what went wrong:
//!
//! - **Contract-level failures** that the RPC reports in the response's
//!   `error` field (a host error, an exhausted budget, a failed
//!   authorization). They are the fixture's subject, so they map to `Pass`
//!   when the fixture expects them and to `Fail` when it expects success.
//!   The raw error string is kept in `details`.
//! - **Engine-level failures**, where the call itself did not produce a
//!   usable answer (a JSON-RPC error object, a body that is not JSON, a
//!   response of the wrong shape). They map to `Status::Error` with a summary
//!   and a descriptive `details`, because they say nothing about the contract.

use canary_core::{
    CompatibilityResult, ExecutionContext, GitContext, NetworkContext, NetworkName, ProjectContext,
    ProjectType, ProtocolVersion, ResultSource, RunOptions, Status, Surface,
};
use canary_rpc::HttpRpcClient;
use canary_soroban::{DefaultSorobanRunner, SorobanFixture, SorobanRunner};
use serde_json::{json, Value};
use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// What the RPC reports for a contract that traps in the host.
const HOST_ERROR: &str =
    "HostError: Error(WasmVm, InvalidAction)\n\nEvent log (newest first):\n   0: [Diagnostic Event] contract:CAAA..., topics:[error, Error(WasmVm, InvalidAction)], data:\"escalating Wasm trap to Error(WasmVm, InvalidAction)\"";
/// What the RPC reports when the invocation runs out of CPU instructions.
const BUDGET_ERROR: &str = "HostError: Error(Budget, ExceededLimit)\n\nEvent log (newest first):\n   0: [Diagnostic Event] topics:[error, Error(Budget, ExceededLimit)], data:[\"operation budget exceeded\", CPU_INSTRUCTIONS, 100000000, 100000001]";
/// What the RPC reports when `require_auth` is not satisfied.
const AUTH_ERROR: &str = "HostError: Error(Auth, InvalidAction)\n\nEvent log (newest first):\n   0: [Diagnostic Event] topics:[error, Error(Auth, InvalidAction)], data:\"[recording authorization only] encountered authorization not tied to the root contract invocation for an address\"";

fn context() -> ExecutionContext {
    ExecutionContext {
        protocol: ProtocolVersion(28),
        project: ProjectContext {
            root: ".".into(),
            name: "test".into(),
            project_type: ProjectType::Soroban,
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
            std::env::temp_dir().join("canary-soroban-simulation-errors-cache"),
        ),
        options: RunOptions::default(),
    }
}

/// A Soroban fixture whose `[expect]` table is `expect` (raw TOML lines).
fn fixture(id: &str, expect: &str) -> SorobanFixture {
    let toml = format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"cap-85\"\ndescription = \"test\"\nsource_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"hello\"\nsequence_number = 1\n\n[[args]]\nkind = \"symbol\"\nvalue = \"world\"\n\n[expect]\n{expect}",
        StrkeyPublicKey([0u8; 32]),
        StrkeyContract([0u8; 32]),
    );
    let loaded = canary_fixtures::parse_fixture_str(&toml, std::path::Path::new("test.toml"))
        .expect("fixture metadata parses");
    SorobanFixture::from_loaded(&loaded).expect("fixture body parses")
}

/// An endpoint that answers every request with `response` as JSON.
async fn server_answering(response: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .mount(&server)
        .await;
    server
}

/// An endpoint whose simulation reports `error` in its result.
async fn server_failing_with(error: &str) -> MockServer {
    server_answering(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "latestLedger": 1000, "error": error }
    }))
    .await
}

async fn run(server: &MockServer, fixture: &SorobanFixture) -> CompatibilityResult {
    DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()))
        .run(fixture, &context())
        .await
        .expect("a simulation failure is a result, not an Err")
}

/// The fields every result carries, whatever happened.
fn assert_identity(result: &CompatibilityResult, id: &str) {
    assert_eq!(result.test_id, id);
    assert_eq!(result.fixture_id.as_deref(), Some(id));
    assert_eq!(result.protocol, ProtocolVersion(28));
    assert_eq!(result.surface, Surface::Soroban);
    assert_eq!(result.source, ResultSource::Live);
}

// --- a fixture that expects the failure ---------------------------------

#[tokio::test]
async fn a_host_error_is_a_pass_for_a_fixture_that_expects_it() {
    let server = server_failing_with(HOST_ERROR).await;
    let fixture = fixture(
        "p28-sim-host",
        "kind = \"simulation-error\"\nmessage_contains = \"Error(WasmVm, InvalidAction)\"\n",
    );

    let result = run(&server, &fixture).await;

    assert_identity(&result, "p28-sim-host");
    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "simulation failed with the expected error");
    // The diagnostic payload is the full error the RPC reported, so the
    // developer sees the event log and not only the matched fragment.
    assert_eq!(result.details.as_deref(), Some(HOST_ERROR));
}

#[tokio::test]
async fn an_exceeded_budget_is_a_pass_for_a_fixture_that_expects_it() {
    let server = server_failing_with(BUDGET_ERROR).await;
    let fixture = fixture(
        "p28-sim-budget",
        "kind = \"simulation-error\"\nmessage_contains = \"Error(Budget, ExceededLimit)\"\n",
    );

    let result = run(&server, &fixture).await;

    assert_identity(&result, "p28-sim-budget");
    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "simulation failed with the expected error");
    let details = result.details.expect("the budget diagnostic is kept");
    assert_eq!(details, BUDGET_ERROR);
    assert!(details.contains("CPU_INSTRUCTIONS"), "{details}");
}

#[tokio::test]
async fn an_authorization_failure_is_a_pass_for_a_fixture_that_expects_it() {
    let server = server_failing_with(AUTH_ERROR).await;
    let fixture = fixture(
        "p28-sim-auth",
        "kind = \"simulation-error\"\nmessage_contains = \"Error(Auth, InvalidAction)\"\n",
    );

    let result = run(&server, &fixture).await;

    assert_identity(&result, "p28-sim-auth");
    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "simulation failed with the expected error");
    assert_eq!(result.details.as_deref(), Some(AUTH_ERROR));
}

#[tokio::test]
async fn a_fixture_expecting_any_failure_passes_for_each_failure_mode() {
    for (name, error) in [
        ("host", HOST_ERROR),
        ("budget", BUDGET_ERROR),
        ("auth", AUTH_ERROR),
    ] {
        let server = server_failing_with(error).await;
        let id = format!("p28-sim-any-{name}");
        let result = run(&server, &fixture(&id, "kind = \"simulation-error\"\n")).await;

        assert_eq!(result.status, Status::Pass, "{name}");
        assert_eq!(result.summary, "simulation failed as expected", "{name}");
        assert_eq!(result.details.as_deref(), Some(error), "{name}");
    }
}

// --- a fixture that expects success -------------------------------------

#[tokio::test]
async fn each_failure_mode_is_a_fail_for_a_fixture_that_expects_success() {
    for (name, error) in [
        ("host", HOST_ERROR),
        ("budget", BUDGET_ERROR),
        ("auth", AUTH_ERROR),
    ] {
        let server = server_failing_with(error).await;
        let id = format!("p28-sim-unexpected-{name}");
        let result = run(&server, &fixture(&id, "kind = \"simulation-success\"\n")).await;

        assert_identity(&result, &id);
        assert_eq!(result.status, Status::Fail, "{name}");
        assert_eq!(
            result.summary, "expected simulation to succeed, but it failed",
            "{name}"
        );
        assert_eq!(result.details.as_deref(), Some(error), "{name}");
    }
}

// --- a failure that is not the expected one -----------------------------

#[tokio::test]
async fn a_different_failure_mode_than_expected_is_a_fail_naming_both() {
    // The fixture expects an authorization failure; the contract ran out of
    // budget instead. That is a regression, and the diagnostic must show the
    // expectation next to what actually happened.
    let server = server_failing_with(BUDGET_ERROR).await;
    let fixture = fixture(
        "p28-sim-mismatch",
        "kind = \"simulation-error\"\nmessage_contains = \"Error(Auth, InvalidAction)\"\n",
    );

    let result = run(&server, &fixture).await;

    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        result.summary,
        "simulation failed, but not with the expected error message"
    );
    assert_eq!(
        result.details,
        Some(format!(
            "expected to contain: Error(Auth, InvalidAction)\nactual: {BUDGET_ERROR}"
        ))
    );
}

#[tokio::test]
async fn a_fixture_expecting_failure_fails_when_the_simulation_succeeds() {
    let server = server_answering(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "latestLedger": 1000, "transactionData": "AAAA", "minResourceFee": "100" }
    }))
    .await;

    let result = run(
        &server,
        &fixture("p28-sim-ok", "kind = \"simulation-error\"\n"),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        result.summary,
        "expected simulation to fail, but it succeeded"
    );
    assert_eq!(result.details, None);
}

// --- engine-level failures ----------------------------------------------

/// The engine could not get a usable answer: `Status::Error`, a summary that
/// names the call, and a `details` that says what went wrong.
fn assert_engine_error(result: &CompatibilityResult, id: &str) -> String {
    assert_identity(result, id);
    assert_eq!(result.status, Status::Error);
    assert_eq!(result.summary, "failed to call simulateTransaction");
    result
        .details
        .clone()
        .expect("an engine error carries a description")
}

#[tokio::test]
async fn an_unparseable_payload_is_a_descriptive_engine_error() {
    // An HTML error page from a proxy, as opposed to a JSON-RPC response.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("<html><body>502 Bad Gateway</body></html>"),
        )
        .mount(&server)
        .await;

    for assertion in [
        "kind = \"simulation-success\"\n",
        "kind = \"simulation-error\"\n",
    ] {
        let result = run(&server, &fixture("p28-sim-html", assertion)).await;
        let details = assert_engine_error(&result, "p28-sim-html");

        assert!(
            details.contains("invalid JSON response from simulateTransaction"),
            "{details}"
        );
    }
}

#[tokio::test]
async fn a_truncated_payload_is_a_descriptive_engine_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"latest"),
        )
        .mount(&server)
        .await;

    let result = run(
        &server,
        &fixture("p28-sim-cut", "kind = \"simulation-error\"\n"),
    )
    .await;
    let details = assert_engine_error(&result, "p28-sim-cut");

    assert!(
        details.contains("invalid JSON response from simulateTransaction"),
        "{details}"
    );
}

#[tokio::test]
async fn a_response_of_the_wrong_shape_is_an_engine_error_not_a_verdict() {
    // Valid JSON-RPC, but `result` is not a simulation response: it has no
    // `latestLedger`. Reading that as "no error, so it succeeded" would turn
    // a broken endpoint into a pass.
    let server = server_answering(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "unexpected": "shape" }
    }))
    .await;

    for assertion in [
        "kind = \"simulation-success\"\n",
        "kind = \"simulation-error\"\n",
    ] {
        let result = run(&server, &fixture("p28-sim-shape", assertion)).await;
        let details = assert_engine_error(&result, "p28-sim-shape");

        assert!(!details.is_empty());
        assert_ne!(result.status, Status::Pass);
    }
}

#[tokio::test]
async fn a_json_rpc_error_object_is_an_engine_error_with_its_code_and_message() {
    let server = server_answering(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": { "code": -32602, "message": "invalid transaction envelope" }
    }))
    .await;

    let result = run(
        &server,
        &fixture("p28-sim-rpc", "kind = \"simulation-error\"\n"),
    )
    .await;
    let details = assert_engine_error(&result, "p28-sim-rpc");

    assert!(details.contains("-32602"), "{details}");
    assert!(
        details.contains("invalid transaction envelope"),
        "{details}"
    );
}

#[tokio::test]
async fn a_server_error_status_is_an_engine_error_naming_the_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let result = run(
        &server,
        &fixture("p28-sim-500", "kind = \"simulation-success\"\n"),
    )
    .await;
    let details = assert_engine_error(&result, "p28-sim-500");

    assert!(details.contains("500"), "{details}");
}

#[tokio::test]
async fn an_unbuildable_invocation_is_an_engine_error_before_any_request() {
    // A function name longer than the 32 characters a Soroban symbol allows
    // cannot be built into an envelope, so nothing may reach the endpoint.
    let server = MockServer::start().await;
    let long_name = "f".repeat(40);
    let mut fixture = fixture("p28-sim-build", "kind = \"simulation-success\"\n");
    fixture.invocation.function_name = long_name;

    let result = run(&server, &fixture).await;

    assert_identity(&result, "p28-sim-build");
    assert_eq!(result.status, Status::Error);
    assert_eq!(
        result.summary,
        "failed to build the invocation transaction envelope"
    );
    assert!(result.details.is_some());
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "no request may be made for an invocation that cannot be built"
    );
}
