//! Target protocol, observed protocol and fixture protocol are three
//! different things, and none of them may stand in for another.
//!
//! - The *target* is what the run is asked to check (`--protocol`, config).
//! - The *observed* protocol is what the endpoint's `getNetwork` reports.
//! - A fixture's *protocol* is what it asserts about.
//!
//! Detecting a network's protocol is not a compatibility result. These tests
//! pin that the report keeps the three apart, that no combination turns a
//! matching identity into a pass by itself, and that a compatibility failure
//! (exit 1), an execution error (exit 3), a skip, and unsupported coverage
//! (exit 2 without `--allow-empty`) stay distinguishable.

mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TESTNET: &str = "Test SDF Network ; September 2015";

const LIVE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = true\nsoroban = false\n";

/// Asserts `getNetwork` reports exactly `protocol`, as the shipped Protocol 28
/// identity fixture does.
fn identity_fixture(id: &str, protocol: u32) -> String {
    format!(
        "id = \"{id}\"\nprotocol = {protocol}\nsurface = \"rpc\"\ncategory = \"network\"\ndescription = \"test\"\nmethod = \"get-network\"\n\n[[assert]]\nkind = \"field-equals\"\nfield = \"protocolVersion\"\nvalue = {protocol}\n"
    )
}

fn xdr_fixture(id: &str, protocol: u32) -> String {
    format!(
        "id = \"{id}\"\nprotocol = {protocol}\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

fn mock_network(observed: u32) -> (tokio::runtime::Runtime, MockServer) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let server = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "passphrase": TESTNET, "protocolVersion": observed }
            })))
            .mount(&server)
            .await;
        server
    });
    (rt, server)
}

fn project(fixtures: &[(&str, String)]) -> TempProject {
    let dir = TempProject::new("protocol-identity");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);
    for (name, body) in fixtures {
        dir.write(&format!("fixtures/{name}.toml"), body);
    }
    dir
}

fn run_json(
    dir: &TempProject,
    uri: &str,
    extra: &[&str],
) -> (std::process::Output, serde_json::Value) {
    let mut args = vec!["check", "--json", "--rpc-url", uri];
    args.extend_from_slice(extra);
    let output = run_in(&dir.path, &args);
    let report = serde_json::from_str(&stdout(&output)).unwrap_or(serde_json::Value::Null);
    (output, report)
}

fn status_of(report: &serde_json::Value, id: &str) -> String {
    report["results"]
        .as_array()
        .and_then(|rs| rs.iter().find(|r| r["testId"] == id))
        .map(|r| r["status"].as_str().unwrap().to_string())
        .unwrap_or_else(|| "<not run>".to_string())
}

#[test]
fn matching_target_observed_and_fixture_protocols_pass() {
    let (_rt, server) = mock_network(28);
    let dir = project(&[
        ("net", identity_fixture("p28-net", 28)),
        ("x", xdr_fixture("p28-x", 28)),
    ]);

    let (output, report) = run_json(&dir, &server.uri(), &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(report["targetProtocol"], 28);
    assert_eq!(report["network"]["observedProtocol"], 28);
    assert_eq!(status_of(&report, "p28-net"), "pass");
    assert!(!stderr(&output).contains("RPC endpoint reports protocol"));
}

/// The live-network situation after Protocol 29: the endpoint is ahead of the
/// Protocol 28 fixtures. The identity assertion must fail on its own terms; it
/// is not edited to match the endpoint, and the mismatch stays visible.
#[test]
fn an_endpoint_ahead_of_the_target_fails_the_identity_fixture_and_says_so() {
    let (_rt, server) = mock_network(29);
    let dir = project(&[
        ("net", identity_fixture("p28-net", 28)),
        ("x", xdr_fixture("p28-x", 28)),
    ]);

    let (output, report) = run_json(&dir, &server.uri(), &[]);

    assert_eq!(
        output.status.code(),
        Some(1),
        "a compatibility failure, not an error"
    );
    assert_eq!(report["status"], "fail");
    assert_eq!(
        report["targetProtocol"], 28,
        "the target is what was asked for"
    );
    assert_eq!(
        report["network"]["observedProtocol"], 29,
        "the observation is what was seen"
    );
    assert_eq!(status_of(&report, "p28-net"), "fail");
    assert_eq!(
        status_of(&report, "p28-x"),
        "pass",
        "offline checks are unaffected"
    );
    assert_eq!(
        report["results"][1]["protocol"], 28,
        "the fixture still asserts protocol 28"
    );
    assert!(
        stderr(&output)
            .contains("the RPC endpoint reports protocol 29, but this run targets protocol 28"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_endpoint_behind_the_target_is_reported_not_absorbed() {
    let (_rt, server) = mock_network(27);
    let dir = project(&[("x", xdr_fixture("p28-x", 28))]);

    let (output, report) = run_json(&dir, &server.uri(), &[]);

    // The only applicable fixture is offline, so it passes; the mismatch is
    // still shown rather than hidden or turned into either outcome.
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["targetProtocol"], 28);
    assert_eq!(report["network"]["observedProtocol"], 27);
    assert!(stderr(&output).contains("reports protocol 27, but this run targets protocol 28"));
}

/// Matching identity is not compatibility: the endpoint reports exactly the
/// target protocol, but no fixture exists for it, so nothing was checked.
#[test]
fn a_matching_network_identity_with_no_fixtures_for_the_target_is_not_a_pass() {
    let (_rt, server) = mock_network(29);
    let dir = project(&[
        ("net", identity_fixture("p28-net", 28)),
        ("x", xdr_fixture("p28-x", 28)),
    ]);

    let (output, report) = run_json(&dir, &server.uri(), &["--protocol", "29"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "unsupported coverage is refused"
    );
    assert_eq!(
        report,
        serde_json::Value::Null,
        "and no report claims success"
    );
    let err = stderr(&output);
    assert!(err.contains("no checks ran"), "{err}");
    assert!(err.contains("this run targets protocol 29"), "{err}");
    assert!(err.contains("the fixtures target protocol 28"), "{err}");
}

#[test]
fn allowing_an_empty_run_reports_zero_checks_beside_the_observed_protocol() {
    let (_rt, server) = mock_network(29);
    let dir = project(&[("net", identity_fixture("p28-net", 28))]);

    let (output, report) = run_json(&dir, &server.uri(), &["--protocol", "29", "--allow-empty"]);
    assert_eq!(output.status.code(), Some(0));
    // The identity was detected and matches, and still nothing ran.
    assert_eq!(report["network"]["observedProtocol"], 29);
    assert_eq!(report["counts"]["total"], 0);
    assert_eq!(report["counts"]["passed"], 0);
    assert_eq!(report["counts"]["skipped"], 1);
}

#[test]
fn a_fixture_for_another_protocol_is_skipped_with_both_protocols_named() {
    let (_rt, server) = mock_network(28);
    let dir = project(&[
        ("x", xdr_fixture("p28-x", 28)),
        ("y", xdr_fixture("p29-y", 29)),
    ]);

    let (output, report) = run_json(&dir, &server.uri(), &[]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["counts"]["total"], 1);
    assert_eq!(report["counts"]["skipped"], 1);
    let skip = &report["skipped"][0];
    assert_eq!(skip["fixtureId"], "p29-y");
    let reason = skip["reason"].as_str().unwrap();
    assert!(
        reason.contains("targets protocol 29") && reason.contains("this run targets protocol 28"),
        "{reason}"
    );
    assert_eq!(
        status_of(&report, "p29-y"),
        "<not run>",
        "a skip is not a result"
    );
}

#[test]
fn an_unavailable_observation_is_an_execution_error_not_a_failure_or_a_pass() {
    // The endpoint answers, but not with a network identity.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("down"))
            .mount(&server)
            .await;
        server
    });
    let dir = project(&[
        ("net", identity_fixture("p28-net", 28)),
        ("x", xdr_fixture("p28-x", 28)),
    ]);

    let (output, report) = run_json(&dir, &server.uri(), &[]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(report["status"], "error");
    assert!(
        report["network"].get("observedProtocol").is_none(),
        "nothing was observed"
    );
    assert!(
        report["network"]["error"].is_string(),
        "and the reason is recorded"
    );
    assert_eq!(status_of(&report, "p28-net"), "error");
    assert_eq!(status_of(&report, "p28-x"), "pass");
}

#[test]
fn an_offline_run_has_no_network_section_and_never_invents_an_observation() {
    let dir = TempProject::new("protocol-identity-offline");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n",
    );
    dir.write("fixtures/x.toml", &xdr_fixture("p28-x", 28));

    let output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert!(report.get("network").is_none());
    assert_eq!(report["targetProtocol"], 28);
}

#[test]
fn the_four_outcomes_use_four_different_exit_codes() {
    // failure, execution error, unsupported coverage, and success.
    let (_rt29, ahead) = mock_network(29);
    let (_rt28, current) = mock_network(28);
    let net = ("net", identity_fixture("p28-net", 28));

    let failure = run_json(&project(std::slice::from_ref(&net)), &ahead.uri(), &[])
        .0
        .status
        .code();
    let success = run_json(&project(std::slice::from_ref(&net)), &current.uri(), &[])
        .0
        .status
        .code();
    let unsupported = run_json(
        &project(std::slice::from_ref(&net)),
        &current.uri(),
        &["--protocol", "29"],
    )
    .0
    .status
    .code();
    let error = run_json(&project(&[net]), "http://127.0.0.1:1", &[])
        .0
        .status
        .code();

    assert_eq!(
        (success, failure, unsupported, error),
        (Some(0), Some(1), Some(2), Some(3))
    );
}
