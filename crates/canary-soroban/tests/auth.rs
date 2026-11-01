//! Auth parsing and assertion evaluation for Soroban simulations
//! (issue #517, CA-01).
//!
//! A successful `simulateTransaction` response carries a `results`
//! array whose entries hold the `auth` array — the authorization entries
//! the invocation will require on-chain. These tests cover the three
//! behaviors the engine must have:
//!
//! - **Parsing**: the `auth` entries of a simulation result become
//!   [`SimulationAuthEntry`] values, and a missing or mistyped field
//!   degrades into an absent value rather than an error.
//! - **Evaluation**: a fixture's `[[expect.required_auth]]` entries are
//!   checked against the parsed entries, deterministically.
//! - **Graceful failure**: an unmet expectation — including one unmet
//!   *because* an auth field was missing — is a `Status::Fail` with a
//!   diagnostic naming the field, never a panic or a `Status::Error`.

use std::path::Path;

use canary_core::{
    CompatibilityResult, ExecutionContext, GitContext, NetworkContext, NetworkName, ProjectContext,
    ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::LoadedFixture;
use canary_rpc::{HttpRpcClient, SimulationResponse};
use canary_soroban::models::{parse_auth_results, SimulationAuthEntry};
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
            std::env::temp_dir().join("canary-soroban-auth-tests-cache"),
        ),
        options: RunOptions::default(),
    }
}

/// A deterministic `C...` strkey for the given seed.
fn contract_address(seed: u8) -> String {
    let contract = StrkeyContract([seed; 32]);
    format!("{contract}")
}

/// A raw `auth` array element. `address` and `function_name` are omitted
/// entirely when `None`, to exercise lenient parsing of missing fields;
/// `sub_invocation_count` sizes the `rootInvocation.subInvocations`
/// array (only meaningful when `function_name` is `Some`).
fn auth_entry(
    address: Option<&str>,
    function_name: Option<&str>,
    sub_invocation_count: usize,
) -> Value {
    let mut entry = json!({ "nonce": 42, "signature": { "tag": "v0", "entries": [] } });
    if let Some(address) = address {
        entry["address"] = json!(address);
    }
    if let Some(function_name) = function_name {
        let sub_invocations: Vec<Value> = (0..sub_invocation_count).map(|_| json!({})).collect();
        entry["rootInvocation"] =
            json!({ "functionName": function_name, "subInvocations": sub_invocations });
    }
    entry
}

/// A full JSON-RPC `simulateTransaction` response. `auth` of `None`
/// omits the `results` array entirely, like a response that reports no
/// simulation results.
fn simulation_body(auth: Option<Vec<Value>>) -> Value {
    let mut result = json!({ "latestLedger": 1000, "transactionData": "AAAA" });
    if let Some(auth) = auth {
        result["results"] = json!([{ "auth": auth, "xdr": "AAAA" }]);
    }
    json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

/// A Soroban fixture whose `[expect]` table body is `expect_toml` (raw
/// TOML lines, starting with `kind = ...`).
fn fixture(expect_toml: &str) -> LoadedFixture {
    canary_fixtures::parse_fixture_str(
        &format!(
            "id = \"p28-soroban-auth-1\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"cap-85\"\ndescription = \"auth\"\nsource_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"hello\"\nsequence_number = 1\n\n[expect]\n{expect_toml}",
            StrkeyPublicKey([0u8; 32]),
            StrkeyContract([0u8; 32]),
        ),
        Path::new("auth.toml"),
    )
    .unwrap()
}

/// Mocks the RPC with `body`, runs the fixture declaring `expect_toml`
/// against it, and returns the resulting check.
async fn run(body: Value, expect_toml: &str) -> CompatibilityResult {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let fixture = SorobanFixture::from_loaded(&fixture(expect_toml)).unwrap();
    let runner = DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()));
    runner.run(&fixture, &context()).await.unwrap()
}

/// The `details` of a result, as an owned string for assertions.
fn details_of(result: &CompatibilityResult) -> String {
    result.details.clone().unwrap_or_default()
}

// -- Parsing ---------------------------------------------------------------

#[test]
fn parses_auth_entries_from_a_simulation_response() {
    let address = contract_address(1);
    let body = simulation_body(Some(vec![auth_entry(Some(&address), Some("transfer"), 2)]));
    let response: SimulationResponse =
        serde_json::from_value(body["result"].clone()).expect("response must deserialize");

    let entries = parse_auth_results(&response.results);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0],
        SimulationAuthEntry {
            address: Some(address),
            nonce: Some(42),
            has_signature: true,
            root_function_name: Some("transfer".to_string()),
            sub_invocation_count: 2,
        }
    );
}

#[test]
fn parses_multiple_auth_entries_across_results_in_order() {
    let first = contract_address(1);
    let second = contract_address(2);
    let mut result = json!({ "latestLedger": 1000, "transactionData": "AAAA" });
    result["results"] = json!([
        { "auth": [auth_entry(Some(&first), None, 0)] },
        { "auth": [auth_entry(Some(&second), None, 0)], "xdr": "AAAA" }
    ]);
    let response: SimulationResponse = serde_json::from_value(result).unwrap();

    let entries = parse_auth_results(&response.results);
    let addresses: Vec<Option<&str>> = entries
        .iter()
        .map(|entry| entry.address.as_deref())
        .collect();
    assert_eq!(addresses, vec![Some(first.as_str()), Some(second.as_str())]);
}

#[test]
fn missing_or_malformed_auth_fields_parse_as_absent_values() {
    // An entry missing every modeled field, plus a `null` signature.
    let body = simulation_body(Some(vec![json!({
        "nonce": "not-a-number",
        "signature": null,
        "rootInvocation": { "subInvocations": "not-an-array" },
    })]));
    let response: SimulationResponse = serde_json::from_value(body["result"].clone()).unwrap();

    let entries = parse_auth_results(&response.results);
    assert_eq!(
        entries,
        vec![SimulationAuthEntry {
            address: None,
            nonce: None,
            has_signature: false,
            root_function_name: None,
            sub_invocation_count: 0,
        }]
    );
}

#[test]
fn a_response_without_results_yields_no_auth_entries() {
    let response: SimulationResponse =
        serde_json::from_value(simulation_body(None)["result"].clone()).unwrap();
    assert!(parse_auth_results(&response.results).is_empty());
}

// -- Evaluation: positive ---------------------------------------------------

#[tokio::test]
async fn passes_when_every_required_auth_entry_is_satisfied() {
    let address = contract_address(1);
    let result = run(
        simulation_body(Some(vec![auth_entry(
            Some(&address),
            Some("hello"),
            2,
        )])),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{address}\"\nfunction_name = \"hello\"\nmin_sub_invocations = 2\n"
        ),
    )
    .await;

    assert_eq!(result.status, Status::Pass);
    assert!(
        result.summary.contains("required authorization"),
        "{}",
        result.summary
    );
    assert_eq!(result.details, None);
}

#[tokio::test]
async fn passes_with_an_address_only_expectation_and_extra_auth_entries() {
    let expected = contract_address(1);
    let other = contract_address(2);
    let result = run(
        simulation_body(Some(vec![
            auth_entry(Some(&other), Some("burn"), 0),
            auth_entry(Some(&expected), None, 0),
        ])),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{expected}\"\n"
        ),
    )
    .await;

    assert_eq!(result.status, Status::Pass);
}

// -- Evaluation: negative ---------------------------------------------------

#[tokio::test]
async fn fails_when_a_required_auth_address_is_not_required_by_the_simulation() {
    let expected = contract_address(1);
    let observed = contract_address(2);
    let result = run(
        simulation_body(Some(vec![auth_entry(Some(&observed), Some("hello"), 0)])),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{expected}\"\n"
        ),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    let details = details_of(&result);
    assert!(
        details.contains("not required by the simulation"),
        "{details}"
    );
    assert!(details.contains(&observed), "{details}");
}

#[tokio::test]
async fn fails_gracefully_when_the_simulation_reports_no_auth_at_all() {
    let expected = contract_address(1);
    let result = run(
        simulation_body(None),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{expected}\"\n"
        ),
    )
    .await;

    assert_eq!(
        result.status,
        Status::Fail,
        "a missing auth array must fail the check, not error the run"
    );
    let details = details_of(&result);
    assert!(details.contains("observed addresses: []"), "{details}");
}

#[tokio::test]
async fn fails_gracefully_when_an_auth_entry_is_missing_its_address_field() {
    let expected = contract_address(1);
    let result = run(
        simulation_body(Some(vec![
            auth_entry(None, Some("hello"), 0),
            auth_entry(Some(&expected), None, 1),
            auth_entry(Some(&contract_address(3)), None, 0),
        ])),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{}\"\n",
            contract_address(9)
        ),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    let details = details_of(&result);
    // The addressless entry is reported as such rather than panicking or
    // being silently dropped.
    assert!(details.contains("<missing address>"), "{details}");
}

#[tokio::test]
async fn fails_when_the_root_function_name_does_not_match() {
    let address = contract_address(1);
    let result = run(
        simulation_body(Some(vec![auth_entry(Some(&address), Some("burn"), 0)])),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{address}\"\nfunction_name = \"transfer\"\n"
        ),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    let details = details_of(&result);
    assert!(
        details.contains("root invocation function name expected \"transfer\""),
        "{details}"
    );
    assert!(details.contains("\"burn\""), "{details}");
}

#[tokio::test]
async fn fails_when_the_root_function_name_is_absent() {
    let address = contract_address(1);
    let mut entry = auth_entry(Some(&address), None, 0);
    entry["rootInvocation"] = json!({ "subInvocations": [] });
    let result = run(
        simulation_body(Some(vec![entry])),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{address}\"\nfunction_name = \"transfer\"\n"
        ),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    let details = details_of(&result);
    assert!(details.contains("reported none"), "{details}");
}

#[tokio::test]
async fn fails_when_min_sub_invocations_is_unmet() {
    let address = contract_address(1);
    let result = run(
        simulation_body(Some(vec![auth_entry(Some(&address), Some("hello"), 1)])),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{address}\"\nmin_sub_invocations = 2\n"
        ),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    let details = details_of(&result);
    assert!(
        details.contains("expected at least 2 sub-invocations, observed 1"),
        "{details}"
    );
}

#[tokio::test]
async fn reports_every_unmet_expectation_in_one_diagnostic() {
    let missing = contract_address(1);
    let mismatched = contract_address(2);
    let result = run(
        simulation_body(Some(vec![auth_entry(
            Some(&mismatched),
            Some("burn"),
            0,
        )])),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{missing}\"\n\n[[expect.required_auth]]\naddress = \"{mismatched}\"\nfunction_name = \"transfer\"\n"
        ),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    let details = details_of(&result);
    assert!(
        details.contains("not required by the simulation"),
        "{details}"
    );
    assert!(
        details.contains("root invocation function name"),
        "{details}"
    );
    let expectation_count = details.matches("address CA").count();
    assert_eq!(expectation_count, 2, "both unmet lines reported: {details}");
}

#[tokio::test]
async fn a_failed_simulation_with_required_auth_fails_with_the_simulation_error() {
    let address = contract_address(1);
    let result = run(
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "latestLedger": 1000, "error": "Error(Auth, InvalidAction)" }
        }),
        &format!(
            "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{address}\"\n"
        ),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        result.summary,
        "expected simulation to succeed, but it failed"
    );
    assert_eq!(
        result.details.as_deref(),
        Some("Error(Auth, InvalidAction)")
    );
}

#[tokio::test]
async fn a_fixture_without_required_auth_never_checks_auth() {
    // Regression guard: the success path keeps its original summary when
    // the fixture declares no auth expectations, even though the
    // response carries auth entries.
    let address = contract_address(1);
    let result = run(
        simulation_body(Some(vec![auth_entry(Some(&address), Some("hello"), 0)])),
        "kind = \"simulation-success\"\n",
    )
    .await;

    assert_eq!(result.status, Status::Pass);
    assert_eq!(result.summary, "simulation succeeded as expected");
}

// -- Fixture parsing --------------------------------------------------------

#[test]
fn parses_required_auth_expectations_from_a_fixture() {
    let address = contract_address(1);
    let loaded = fixture(&format!(
        "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{address}\"\nfunction_name = \"hello\"\nmin_sub_invocations = 3\n\n[[expect.required_auth]]\naddress = \"{}\"\n",
        contract_address(2)
    ));
    let parsed = SorobanFixture::from_loaded(&loaded).unwrap();

    assert_eq!(parsed.required_auth.len(), 2);
    assert_eq!(parsed.required_auth[0].address, address);
    assert_eq!(
        parsed.required_auth[0].function_name.as_deref(),
        Some("hello")
    );
    assert_eq!(parsed.required_auth[0].min_sub_invocations, Some(3));
    assert_eq!(parsed.required_auth[1].address, contract_address(2));
    assert_eq!(parsed.required_auth[1].function_name, None);
    assert_eq!(parsed.required_auth[1].min_sub_invocations, None);
}

#[test]
fn a_fixture_without_required_auth_has_none() {
    let parsed = SorobanFixture::from_loaded(&fixture("kind = \"simulation-success\"\n")).unwrap();
    assert!(parsed.required_auth.is_empty());
}

#[test]
fn rejects_required_auth_on_a_simulation_error_fixture() {
    let loaded = fixture(&format!(
        "kind = \"simulation-error\"\n\n[[expect.required_auth]]\naddress = \"{}\"\n",
        contract_address(1)
    ));
    let err = SorobanFixture::from_loaded(&loaded).unwrap_err();
    assert!(
        err.to_string()
            .contains("only supported with kind = \"simulation-success\""),
        "{err}"
    );
}

#[test]
fn rejects_a_required_auth_entry_without_an_address() {
    let loaded = fixture(
        "kind = \"simulation-success\"\n\n[[expect.required_auth]]\nfunction_name = \"hello\"\n",
    );
    let err = SorobanFixture::from_loaded(&loaded).unwrap_err();
    assert!(
        err.to_string()
            .contains("required_auth entry missing \"address\""),
        "{err}"
    );
}

#[test]
fn rejects_an_invalid_required_auth_address_strkey() {
    let loaded = fixture(
        "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"not-a-strkey\"\n",
    );
    let err = SorobanFixture::from_loaded(&loaded).unwrap_err();
    assert!(
        err.to_string()
            .contains("is not a valid G... or C... strkey"),
        "{err}"
    );
}

#[test]
fn rejects_unknown_keys_in_required_auth_entries() {
    let loaded = fixture(&format!(
        "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{}\"\nextra = true\n",
        contract_address(1)
    ));
    let err = SorobanFixture::from_loaded(&loaded).unwrap_err();
    assert!(err.to_string().contains("unknown key"), "{err}");
}

#[test]
fn rejects_a_negative_min_sub_invocations() {
    let loaded = fixture(&format!(
        "kind = \"simulation-success\"\n\n[[expect.required_auth]]\naddress = \"{}\"\nmin_sub_invocations = -1\n",
        contract_address(1)
    ));
    let err = SorobanFixture::from_loaded(&loaded).unwrap_err();
    assert!(
        err.to_string()
            .contains("min_sub_invocations must be a non-negative integer"),
        "{err}"
    );
}

#[test]
fn rejects_a_required_auth_value_that_is_not_an_array() {
    let loaded = fixture(&format!(
        "kind = \"simulation-success\"\nrequired_auth = \"{}\"\n",
        contract_address(1)
    ));
    let err = SorobanFixture::from_loaded(&loaded).unwrap_err();
    assert!(
        err.to_string()
            .contains("expect.required_auth must be an array of tables"),
        "{err}"
    );
}
