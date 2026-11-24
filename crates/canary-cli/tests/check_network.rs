mod support;

use support::{run_in, stderr, stdout, TempProject};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RPC_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = false
rpc = true
soroban = false
"#;

#[tokio::test]
async fn a_failing_get_network_call_surfaces_the_error() {
    let server = MockServer::start().await;
    // Mock getNetwork returning an HTTP 500 error
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let dir = TempProject::new("check-network-fail");
    dir.write(".stellar-canary.toml", RPC_CONFIG);

    // We only need an empty fixture to make it try to run RPC
    dir.write(
        "fixtures/p28-rpc-1.toml",
        "id = \"p28-rpc-1\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"test\"\ndescription = \"test\"\nmethod = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n",
    );

    let server_uri = server.uri();

    let output = run_in(
        &dir.path,
        &["check", "--network", "testnet", "--rpc-url", &server_uri],
    );

    let text = support::stdout(&output);
    assert!(
        text.contains("protocol not observed"),
        "Expected 'protocol not observed' in output, but got stdout: '{}' and stderr: '{}'",
        text,
        support::stderr(&output)
    );
    assert!(
        text.contains("Internal Server Error") || text.contains("500"),
        "Output was: {}",
        text
    );

    let json_output = run_in(
        &dir.path,
        &[
            "check",
            "--network",
            "testnet",
            "--rpc-url",
            &server_uri,
            "--json",
        ],
    );

    let json_text = support::stdout(&json_output);
    let value: serde_json::Value = serde_json::from_str(&json_text).expect("valid json");

    let error_text = value["network"]["error"]
        .as_str()
        .expect("Network error field should be present and a string");
    assert!(
        error_text.contains("Internal Server Error") || error_text.contains("500"),
        "JSON error was: {}",
        error_text
    );
}

const RPC_FIXTURE: &str = "id = \"p28-rpc-1\"\n\
     protocol = 28\n\
     surface = \"rpc\"\n\
     category = \"test\"\n\
     description = \"test\"\n\
     method = \"get-network\"\n\
     \n\
     [[assert]]\n\
     kind = \"field-exists\"\n\
     field = \"passphrase\"\n";

/// A gateway response (502/504) is a transport-level failure, not a bad
/// invocation: the probe records the endpoint as not observed, every RPC
/// fixture becomes `Status::Error`, and the run exits `3`
/// (`ExecutionError`) — the contract documented in
/// `docs-site/src/exit-codes.md` section 3 and pinned end to end by
/// `check_rpc_unreachable.rs`. Exit `2` (`ConfigurationError`) would claim no
/// check ran because the invocation was unusable, which is false here: the
/// checks ran and the endpoint answered badly.
///
/// Retries are disabled so the mock sees one attempt per call and the test
/// never waits out the default backoff.
async fn assert_gateway_status_is_a_network_error(status: u16) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(status))
        .mount(&server)
        .await;

    let dir = TempProject::new(&format!("check-network-{status}"));
    dir.write(".stellar-canary.toml", RPC_CONFIG);
    dir.write("fixtures/p28-rpc-1.toml", RPC_FIXTURE);

    let uri = server.uri();
    let output = run_in(
        &dir.path,
        &[
            "check",
            "--network",
            "testnet",
            "--rpc-url",
            &uri,
            "--rpc-max-retries",
            "0",
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(3),
        "an endpoint answering HTTP {status} is an execution error\nstdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );

    let text = stdout(&output);
    assert!(
        text.contains("protocol not observed"),
        "the failed probe must be recorded rather than hidden: {text}"
    );
    assert!(
        text.contains(&format!("server returned status {status}"))
            || text.contains(&status.to_string()),
        "the report must name the HTTP status it received: {text}"
    );
    assert!(
        text.contains("‼ ERROR"),
        "the fixture must be reported as an execution error, not a pass: {text}"
    );
    assert!(
        text.contains("Status: ERROR"),
        "the terminal verdict must be ERROR: {text}"
    );
    assert!(
        text.contains("0/1 applicable checks passed."),
        "the endpoint failure must not be counted as a pass: {text}"
    );

    // The hint after the report says what to check next, so an operator is
    // not left with only the status code.
    let err = stderr(&output);
    assert!(err.contains("Hint:"), "stderr: {err}");
    assert!(
        err.to_lowercase().contains("reachable"),
        "the hint must point at endpoint reachability: {err}"
    );

    let json_output = run_in(
        &dir.path,
        &[
            "check",
            "--network",
            "testnet",
            "--rpc-url",
            &uri,
            "--rpc-max-retries",
            "0",
            "--json",
        ],
    );
    assert_eq!(
        json_output.status.code(),
        Some(3),
        "the JSON run reports the same execution error\nstdout: {}\nstderr: {}",
        stdout(&json_output),
        stderr(&json_output)
    );

    let value: serde_json::Value =
        serde_json::from_str(&stdout(&json_output)).expect("stdout is the JSON report");
    let error_text = value["network"]["error"]
        .as_str()
        .expect("the network error is recorded in the JSON report");
    assert!(
        error_text.contains(&status.to_string()),
        "the JSON network error must name HTTP {status}: {error_text}"
    );
    assert_eq!(value["results"][0]["status"], "error", "{value}");
    assert_eq!(value["counts"]["errors"], 1, "{value}");
}

#[tokio::test]
async fn a_502_bad_gateway_response_is_reported_as_a_network_error() {
    assert_gateway_status_is_a_network_error(502).await;
}

#[tokio::test]
async fn a_504_gateway_timeout_response_is_reported_as_a_network_error() {
    assert_gateway_status_is_a_network_error(504).await;
}
