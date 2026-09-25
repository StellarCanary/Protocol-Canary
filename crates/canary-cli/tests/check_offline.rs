mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = true
rpc = false
soroban = false
"#;

fn xdr_fixture(id: &str, kind: &str, value_base64: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"{kind}\"\nvalue_base64 = \"{value_base64}\"\n"
    )
}

#[test]
fn an_offline_run_with_no_fixtures_passes_trivially() {
    let dir = TempProject::new("check-empty");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    let output = run_in(&dir.path, &["check"]);
    assert!(output.status.success());
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("0/0 applicable checks passed."));
    assert!(text.contains("Status: PASS"));
    // An offline (xdr-only) run must never mention the network.
    assert!(!text.contains("Network:"));
}

#[test]
fn a_passing_xdr_fixture_exits_zero() {
    let dir = TempProject::new("check-xdr-pass");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("1/1 PASS"));
    assert!(text.contains("1/1 applicable checks passed."));
}

#[test]
fn a_failing_xdr_fixture_exits_one_and_explains_the_failure() {
    let dir = TempProject::new("check-xdr-fail");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", "not-valid-xdr-bytes!!!"),
    );

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("Status: NOT READY"));
    assert!(text.contains("Failure:"));
    assert!(text.contains("p28-xdr-1"));
}

#[test]
fn an_unsupported_configuration_schema_version_exits_two() {
    let dir = TempProject::new("check-bad-config");
    dir.write(".stellar-canary.toml", "version = 2\nprotocol = 28\n");

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_duplicate_fixture_id_exits_four() {
    let dir = TempProject::new("check-dup-fixture");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/a.toml",
        &xdr_fixture("dup-id", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );
    dir.write(
        "fixtures/b.toml",
        &xdr_fixture("dup-id", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(4));
}

#[test]
fn a_protocol_mismatched_fixture_is_skipped_not_run() {
    let dir = TempProject::new("check-protocol-mismatch");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p27-xdr-1.toml",
        &format!(
            "id = \"p27-xdr-1\"\nprotocol = 27\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );

    let output = run_in(&dir.path, &["check", "--verbose"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("0/0 applicable checks passed."));
    assert!(text.contains("Skipped fixtures: 1"));
}

#[test]
fn json_output_is_valid_json_with_the_expected_top_level_fields() {
    let dir = TempProject::new("check-json");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    let output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["targetProtocol"], 28);
    assert_eq!(value["status"], "pass");
}

/// Regression test for the full deterministic-failure path: a real failed
/// assertion (not a manually forced exit code) must produce Status::Fail,
/// exit code 1, and a JSON report whose top-level "status" also reads
/// "fail" — in both the default terminal format and --json.
#[test]
fn a_real_compatibility_failure_is_reported_consistently_in_terminal_and_json() {
    let fail_fixture = xdr_fixture(
        "p28-xdr-regression-fail",
        "decode-success",
        "not-valid-xdr-bytes!!!",
    );

    let terminal_dir = TempProject::new("check-fail-terminal");
    terminal_dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    terminal_dir.write("fixtures/p28-xdr-regression-fail.toml", &fail_fixture);
    let terminal_output = run_in(&terminal_dir.path, &["check"]);
    assert_eq!(
        terminal_output.status.code(),
        Some(1),
        "a real failed assertion must exit with the documented compatibility-failure code"
    );
    assert!(stdout(&terminal_output).contains("Status: NOT READY"));

    let json_dir = TempProject::new("check-fail-json");
    json_dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    json_dir.write("fixtures/p28-xdr-regression-fail.toml", &fail_fixture);
    let json_output = run_in(&json_dir.path, &["check", "--json"]);
    assert_eq!(json_output.status.code(), Some(1));

    let value: serde_json::Value =
        serde_json::from_str(&stdout(&json_output)).expect("valid json even on failure");
    assert_eq!(value["status"], "fail");
    assert_eq!(value["counts"]["total"], 1);
    assert_eq!(value["counts"]["passed"], 0);
    assert_eq!(value["counts"]["failed"], 1);
    assert_eq!(value["results"][0]["testId"], "p28-xdr-regression-fail");
    assert_eq!(value["results"][0]["status"], "fail");
    assert!(
        value["results"][0]["details"].is_string(),
        "a failure result must carry details explaining what went wrong"
    );
}

/// Issue #69 regression coverage: `check --quiet` (terminal format) must
/// print exactly one `Status: <STATUS>` line instead of the full
/// multi-line report — PASS for a green run, NOT READY for a red one.
#[test]
fn quiet_flag_prints_exactly_one_status_line_for_pass_and_fail_runs() {
    // Passing run: stdout is exactly one line, "Status: PASS".
    let pass_dir = TempProject::new("check-quiet-pass");
    pass_dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    pass_dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );

    let pass_output = run_in(&pass_dir.path, &["check", "--quiet"]);
    assert_eq!(pass_output.status.code(), Some(0));
    let pass_text = stdout(&pass_output);
    assert_eq!(
        pass_text.trim_end(),
        "Status: PASS",
        "--quiet must shorten terminal output to a single status line, got: {pass_text:?}"
    );
    assert_eq!(
        pass_text.lines().count(),
        1,
        "--quiet terminal output must be exactly one line, got: {pass_text:?}"
    );

    // Failing run: still one line, but "Status: NOT READY".
    let fail_dir = TempProject::new("check-quiet-fail");
    fail_dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    fail_dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", "not-valid-xdr-bytes!!!"),
    );

    let fail_output = run_in(&fail_dir.path, &["check", "--quiet"]);
    assert_eq!(fail_output.status.code(), Some(1));
    let fail_text = stdout(&fail_output);
    assert_eq!(
        fail_text.trim_end(),
        "Status: NOT READY",
        "--quiet must shorten terminal output to a single status line, got: {fail_text:?}"
    );
    assert_eq!(
        fail_text.lines().count(),
        1,
        "--quiet terminal output must be exactly one line, got: {fail_text:?}"
    );
}

/// Issue #69: `--quiet` only shortens terminal-format output. Combined
/// with `--json` the full JSON report must still be printed.
#[test]
fn quiet_with_json_still_prints_the_full_json_report() {
    let dir = TempProject::new("check-quiet-json");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["check", "--quiet", "--json"]);
    assert_eq!(output.status.code(), Some(0));

    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("--quiet --json must print valid JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["targetProtocol"], 28);
    assert_eq!(value["status"], "pass");
    assert_eq!(value["counts"]["total"], 1);
    assert_eq!(value["counts"]["passed"], 1);
    assert_eq!(value["results"][0]["testId"], "p28-xdr-1");
    assert_eq!(value["results"][0]["status"], "pass");
}

#[test]
fn protocol_flag_zero_is_rejected_as_a_configuration_error() {
    let dir = TempProject::new("check-protocol-zero");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["check", "--protocol", "0"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(err.contains("configuration error"), "stderr: {err}");
    assert!(err.contains("--protocol"), "stderr: {err}");
}
