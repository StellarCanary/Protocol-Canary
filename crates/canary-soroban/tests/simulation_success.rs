//! Successful Soroban simulation outcomes and auth requirement matching
//! (issue #823).
//!
//! `simulation_errors.rs` covers the failure path of
//! [`DefaultSorobanRunner`]. These tests cover the success path:
//!
//! - A successful simulation with no auth expectations is a
//!   [`Status::Pass`] with the documented summary text.
//! - A response carrying a simulated return value (`transactionData` and
//!   `results`) still passes, and the value is preserved by the runner.
//! - Every `[[expect.required_auth]]` entry is matched against the parsed
//!   simulation authorization entries; unmet expectations are a
//!   [`Status::Fail`] with a structured diagnostic naming what was missing,
//!   never an engine error or a panic.
//! - The elapsed execution duration is recorded in milliseconds.

use std::time::Duration;

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
            std::env::temp_dir().join("canary-soroban-simulation-success-cache"),
        ),
        options: RunOptions::default(),
    }
}

/// A deterministic `C...` strkey for the given seed.
fn contract_address(seed: u8) -> String {
    format!("{}", StrkeyContract([seed; 32]))
}

/// A raw `auth` array element, matching the shape Stellar RPC reports.
fn auth_entry(address: &str, function_name: Option<&str>, sub_invocation_count: usize) -> Value {
    let mut entry = json!({ "nonce": 7, "signature": { "tag": "v0", "entries": [] } });
    entry["address"] = json!(address);
    if let Some(function_name) = function_name {
        let sub_invocations: Vec<Value> = (0..sub_invocation_count).map(|_| json!({})).collect();
        entry["rootInvocation"] =
            json!({ "functionName": function_name, "subInvocations": sub_invocations });
    }
    entry
}

/// A successful `simulateTransaction` response, optionally carrying a
/// simulated return value and auth entries.
fn simulation_response(transaction_data: Option<&str>, auth: Option<Vec<Value>>) -> Value {
    let mut result = json!({ "latestLedger": 1000 });
    if let Some(transaction_data) = transaction_data {
        result["transactionData"] = json!(transaction_data);
        result["minResourceFee"] = json!("100");
    }
    if let Some(auth) = auth {
        result["results"] = json!([{ "auth": auth, "xdr": "AAAA" }]);
    }
    json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

/// A fixture whose `[expect]` table is `expect_toml` (raw TOML lines).
fn fixture(id: &str, expect_toml: &str) -> SorobanFixture {
    let toml = format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"cap-85\"\ndescription = \"test\"\nsource_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"hello\"\nsequence_number = 1\n\n[[args]]\nkind = \"symbol\"\nvalue = \"world\"\n\n[expect]\n{expect_toml}",
        StrkeyPublicKey([0u8; 32]),
        StrkeyContract([0u8; 32]),
    );
    let loaded = canary_fixtures::parse_fixture_str(&toml, std::path::Path::new("success.toml"))
        .expect("fixture metadata parses");
    SorobanFixture::from_loaded(&loaded).expect("fixture body parses")
}

async fn server_answering(body: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    server
}

async fn run(server: &MockServer, fixture: &SorobanFixture) -> CompatibilityResult {
    DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()))
        .run(fixture, &context())
        .await
        .expect("a simulation outcome is a result, not an Err")
}

fn assert_identity(result: &CompatibilityResult, id: &str) {
    assert_eq!(result.test_id, id);
    assert_eq!(result.fixture_id.as_deref(), Some(id));
    assert_eq!(result.protocol, ProtocolVersion(28));
    assert_eq!(result.surface, Surface::Soroban);
    assert_eq!(result.source, ResultSource::Live);
}

// -- Successful simulations ------------------------------------------------

#[tokio::test]
async fn a_successful_simulation_without_auth_expectations_passes() {
    let server = server_answering(simulation_response(Some("AAAA"), None)).await;
    let fixture = fixture("p28-sim-success-1", "kind = \"simulation-success\"\n");

    let result = run(&server, &fixture).await;

    assert_identity(&result, "p28-sim-success-1");
    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "simulation succeeded as expected");
    assert_eq!(result.details, None);
}

#[tokio::test]
async fn a_simulated_return_value_does_not_change_the_pass_verdict() {
    // A successful response may carry a simulated return value
    // (`transactionData`, `results[].xdr`). It is parsed by the client and
    // must not be mistaken for a failure.
    let server = server_answering(simulation_response(
        Some("AAAA"),
        Some(vec![auth_entry(&contract_address(1), Some("hello"), 0)]),
    ))
    .await;
    let fixture = fixture("p28-sim-return", "kind = \"simulation-success\"\n");

    let result = run(&server, &fixture).await;

    assert_identity(&result, "p28-sim-return");
    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "simulation succeeded as expected");
    assert_eq!(result.details, None);
}

#[tokio::test]
async fn a_successful_simulation_records_its_elapsed_duration() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(simulation_response(Some("AAAA"), None))
                .set_delay(Duration::from_millis(200)),
        )
        .mount(&server)
        .await;
    let fixture = fixture("p28-sim-timed", "kind = \"simulation-success\"\n");

    let result = run(&server, &fixture).await;

    assert_eq!(result.status, Status::Pass);
    assert!(
        result.duration_ms >= 150,
        "the 200ms delayed simulation must be timed, got {}ms",
        result.duration_ms
    );
    assert!(
        result.duration_ms < 5_000,
        "duration must be a plausible millisecond measurement, got {}ms",
        result.duration_ms
    );
}

// -- Auth requirement matching: satisfying expectations --------------------

#[tokio::test]
async fn multiple_auth_expectations_are_all_matched() {
    let alice = contract_address(1);
    let token = contract_address(2);
    let server = server_answering(simulation_response(
        Some("AAAA"),
        Some(vec![
            auth_entry(&alice, Some("hello"), 2),
            auth_entry(&token, Some("transfer"), 1),
        ]),
    ))
    .await;
    let fixture = fixture(
        "p28-sim-auth-ok",
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{alice}\"\nfunction_name = \"hello\"\nmin_sub_invocations = 2\n\n[[expect.required_auth]]\naddress = \"{token}\"\nfunction_name = \"transfer\"\nmin_sub_invocations = 1\n"
        ),
    );

    let result = run(&server, &fixture).await;

    assert_identity(&result, "p28-sim-auth-ok");
    assert_eq!(result.status, Status::Pass);
    assert_eq!(
        result.summary,
        "simulation succeeded with all 2 required authorization entries as expected"
    );
    assert_eq!(result.details, None);
}

#[tokio::test]
async fn a_single_auth_expectation_uses_the_singular_summary() {
    let address = contract_address(3);
    let server = server_answering(simulation_response(
        Some("AAAA"),
        Some(vec![auth_entry(&address, Some("hello"), 0)]),
    ))
    .await;
    let fixture = fixture(
        "p28-sim-auth-one",
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{address}\"\n"
        ),
    );

    let result = run(&server, &fixture).await;

    assert_eq!(result.status, Status::Pass);
    assert_eq!(
        result.summary,
        "simulation succeeded with all 1 required authorization entry as expected"
    );
}

#[tokio::test]
async fn an_address_only_expectation_ignores_extra_auth_entries() {
    let expected = contract_address(4);
    let extra = contract_address(5);
    let server = server_answering(simulation_response(
        Some("AAAA"),
        Some(vec![
            auth_entry(&extra, Some("burn"), 0),
            auth_entry(&expected, None, 0),
        ]),
    ))
    .await;
    let fixture = fixture(
        "p28-sim-auth-extra",
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{expected}\"\n"
        ),
    );

    let result = run(&server, &fixture).await;

    assert_eq!(result.status, Status::Pass);
}

// -- Auth requirement matching: unmet expectations -------------------------

#[tokio::test]
async fn a_missing_auth_address_fails_with_a_structured_diagnostic() {
    let expected = contract_address(6);
    let observed = contract_address(7);
    let server = server_answering(simulation_response(
        Some("AAAA"),
        Some(vec![auth_entry(&observed, Some("hello"), 0)]),
    ))
    .await;
    let fixture = fixture(
        "p28-sim-auth-missing",
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{expected}\"\n"
        ),
    );

    let result = run(&server, &fixture).await;

    assert_identity(&result, "p28-sim-auth-missing");
    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        result.summary,
        "simulation succeeded, but required authorization entries were not satisfied"
    );
    let details = result.details.expect("a failed auth check carries details");
    assert!(
        details.contains(&expected),
        "the diagnostic names the missing address: {details}"
    );
    assert!(
        details.contains("not required by the simulation"),
        "{details}"
    );
    assert!(
        details.contains(&observed),
        "the diagnostic lists the addresses the simulation did report: {details}"
    );
}

#[tokio::test]
async fn unmet_function_and_sub_invocation_expectations_are_reported_together() {
    let address = contract_address(8);
    let server = server_answering(simulation_response(
        Some("AAAA"),
        Some(vec![auth_entry(&address, Some("burn"), 1)]),
    ))
    .await;
    let fixture = fixture(
        "p28-sim-auth-partial",
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{address}\"\nfunction_name = \"transfer\"\nmin_sub_invocations = 2\n"
        ),
    );

    let result = run(&server, &fixture).await;

    assert_eq!(result.status, Status::Fail);
    let details = result
        .details
        .expect("details name every unmet requirement");
    assert!(
        details.contains("root invocation function name expected \"transfer\""),
        "{details}"
    );
    assert!(
        details.contains("\"burn\""),
        "the observed function name is shown: {details}"
    );
    assert!(
        details.contains("expected at least 2 sub-invocations, observed 1"),
        "{details}"
    );
}

#[tokio::test]
async fn every_unmet_expectation_gets_its_own_diagnostic_line() {
    let missing = contract_address(9);
    let mismatched = contract_address(10);
    let server = server_answering(simulation_response(
        Some("AAAA"),
        Some(vec![auth_entry(&mismatched, Some("burn"), 0)]),
    ))
    .await;
    let fixture = fixture(
        "p28-sim-auth-many",
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{missing}\"\n\n[[expect.required_auth]]\naddress = \"{mismatched}\"\nfunction_name = \"transfer\"\n"
        ),
    );

    let result = run(&server, &fixture).await;

    assert_eq!(result.status, Status::Fail);
    let details = result.details.expect("details are present");
    assert!(
        details.contains("not required by the simulation"),
        "{details}"
    );
    assert!(
        details.contains("root invocation function name"),
        "{details}"
    );
    assert_eq!(
        details.matches("address C").count(),
        2,
        "each unmet expectation is named: {details}"
    );
}

#[tokio::test]
async fn a_simulation_without_auth_entries_fails_auth_expectations_gracefully() {
    let expected = contract_address(11);
    let server = server_answering(simulation_response(Some("AAAA"), None)).await;
    let fixture = fixture(
        "p28-sim-auth-none",
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{expected}\"\n"
        ),
    );

    let result = run(&server, &fixture).await;

    // A missing `results`/`auth` array is a failed authorization check, not
    // an engine error or a panic.
    assert_eq!(result.status, Status::Fail);
    let details = result.details.expect("details are present");
    assert!(details.contains("observed addresses: []"), "{details}");
}

#[tokio::test]
async fn a_failed_simulation_with_auth_expectations_reports_the_simulation_error() {
    let expected = contract_address(12);
    let server = server_answering(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "latestLedger": 1000, "error": "HostError: Error(Auth, InvalidAction)" }
    }))
    .await;
    let fixture = fixture(
        "p28-sim-auth-error",
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{expected}\"\n"
        ),
    );

    let result = run(&server, &fixture).await;

    // The simulation failed before auth could be evaluated: the simulation
    // failure is the verdict, and auth matching must not turn it into an
    // engine error.
    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        result.summary,
        "expected simulation to succeed, but it failed"
    );
    assert_eq!(
        result.details.as_deref(),
        Some("HostError: Error(Auth, InvalidAction)")
    );
}

#[tokio::test]
async fn an_unusable_response_with_auth_expectations_is_an_engine_error() {
    // A malformed payload cannot be evaluated at all; it must come back as
    // `Status::Error` with a diagnostic, never as a panic or an `Err`.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>502 Bad Gateway</html>"))
        .mount(&server)
        .await;
    let expected = contract_address(13);
    let fixture = fixture(
        "p28-sim-auth-engine",
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{expected}\"\n"
        ),
    );

    let result = run(&server, &fixture).await;

    assert_identity(&result, "p28-sim-auth-engine");
    assert_eq!(result.status, Status::Error);
    assert_eq!(result.summary, "failed to call simulateTransaction");
    assert!(result.details.is_some());
}
