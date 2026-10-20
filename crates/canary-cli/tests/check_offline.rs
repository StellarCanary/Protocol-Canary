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
fn all_surfaces_disabled_exits_two_with_an_actionable_message() {
    let dir = TempProject::new("check-all-surfaces-disabled");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[tests]\nxdr = false\nrpc = false\nsoroban = false\n",
    );

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(err.contains("configuration error"), "stderr: {err}");
    assert!(
        err.contains("at least one of [tests].xdr, [tests].rpc, [tests].soroban must be enabled"),
        "stderr: {err}"
    );
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

    // The only fixture targets another protocol, so nothing applies: that is
    // an empty run, which is refused unless explicitly allowed.
    let refused = run_in(&dir.path, &["check", "--verbose"]);
    assert_eq!(refused.status.code(), Some(2));

    let output = run_in(&dir.path, &["check", "--verbose", "--allow-empty"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("0/0 applicable checks passed."));
    assert!(text.contains("Skipped fixtures: 1"));
}

#[test]
fn json_output_is_valid_json_with_the_expected_top_level_fields() {
    let dir = TempProject::new("check-json");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["targetProtocol"], 28);
    assert_eq!(value["status"], "pass");
}

/// The Markdown reporter is unit-tested in canary-report, but the CLI
/// wiring that selects it via OutputFormat::Markdown is not exercised
/// anywhere else. This pins that wiring end-to-end against the offline
/// fixture setup and checks the documented leading shape.
#[test]
fn markdown_output_starts_with_the_documented_heading() {
    let dir = TempProject::new("check-markdown");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["check", "--format", "markdown"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(
        text.starts_with("## Stellar Protocol Canary"),
        "markdown output must begin with the documented heading, got: {text}"
    );
    assert!(text.contains("**Result: PASS**"));
    assert!(text.contains("| XDR | ✅ Pass |"));
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

// ---------------------------------------------------------------------------
// `check --offline`: local XDR checks run, network fixtures are skipped,
// and no socket is ever opened.
// ---------------------------------------------------------------------------

const LIVE_SURFACES_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = true
rpc = true
soroban = true
"#;

fn rpc_fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"test\"\ndescription = \"test\"\nmethod = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n"
    )
}

/// A well-formed Soroban fixture (the strkeys are the reserved Testnet
/// Stellar Asset Contract address and a null account, borrowed from the
/// repository's own `tests/fixtures/protocol-28` pack).
fn soroban_fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"test\"\ndescription = \"test\"\nsource_account = \"GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF\"\ncontract_id = \"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC\"\nfunction = \"name\"\nsequence_number = 1\n\n[expect]\nkind = \"simulation-success\"\n"
    )
}

/// The core promise of `--offline`: the XDR fixture executes, the RPC and
/// Soroban fixtures are reported as skipped, and the endpoint — pointed at
/// a port that refuses connections instantly — is never contacted. If any
/// socket were opened the run would end in an execution error (exit 3),
/// not a pass.
#[test]
fn offline_check_runs_xdr_and_skips_rpc_and_soroban_with_zero_network_calls() {
    let dir = TempProject::new("check-offline-flag");
    dir.write(".stellar-canary.toml", LIVE_SURFACES_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );
    dir.write("fixtures/p28-rpc-1.toml", &rpc_fixture("p28-rpc-1"));
    dir.write(
        "fixtures/p28-soroban-1.toml",
        &soroban_fixture("p28-soroban-1"),
    );

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--offline",
            "--rpc-url",
            "http://127.0.0.1:1",
            "--json",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "an offline run must pass without touching the network: {}",
        stderr(&output)
    );

    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(report["status"], "pass");
    assert_eq!(report["counts"]["total"], 1, "only the XDR fixture runs");
    assert_eq!(report["counts"]["passed"], 1);
    assert_eq!(report["counts"]["skipped"], 2);
    assert_eq!(report["results"][0]["testId"], "p28-xdr-1");
    assert_eq!(report["results"][0]["surface"], "xdr");

    let skipped: Vec<&serde_json::Value> = report["skipped"]
        .as_array()
        .expect("skipped array")
        .iter()
        .collect();
    assert_eq!(skipped.len(), 2);
    for entry in skipped {
        assert!(
            entry["reason"].as_str().unwrap_or("").contains("--offline"),
            "skip reason must name the flag: {entry}"
        );
    }
}

/// Control for the test above: without `--offline`, the same run against
/// the same dead endpoint fails with an execution error — proving it is
/// the flag, not the environment, that keeps the run socket-free.
#[test]
fn the_same_run_without_the_flag_does_contact_the_network_and_errors() {
    let dir = TempProject::new("check-offline-control");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = true\nsoroban = false\n",
    );
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );
    dir.write("fixtures/p28-rpc-1.toml", &rpc_fixture("p28-rpc-1"));

    let output = run_in(
        &dir.path,
        &["check", "--rpc-url", "http://127.0.0.1:1", "--json"],
    );
    assert_eq!(
        output.status.code(),
        Some(3),
        "without --offline the unreachable endpoint must surface as an execution error"
    );
}

/// The issue's negative case: a pack with only live RPC fixtures, run with
/// `--offline`, marks every fixture skipped without an error. The empty-run
/// invariant still applies (nothing executed is not evidence of
/// compatibility), so `--allow-empty` is required — and then the run
/// exits 0 with all fixtures skipped.
#[test]
fn offline_against_a_pack_with_only_live_fixtures_marks_all_skipped_without_error() {
    let dir = TempProject::new("check-offline-rpc-only");
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[tests]\nxdr = false\nrpc = true\nsoroban = true\n",
    );
    dir.write("fixtures/p28-rpc-1.toml", &rpc_fixture("p28-rpc-1"));
    dir.write(
        "fixtures/p28-soroban-1.toml",
        &soroban_fixture("p28-soroban-1"),
    );

    // Without --allow-empty: refused as an empty run (exit 2), and the
    // diagnostic names --offline as the reason nothing applies.
    let refused = run_in(&dir.path, &["check", "--offline"]);
    assert_eq!(refused.status.code(), Some(2));
    let err = stderr(&refused);
    assert!(
        err.contains("2 require network access and were skipped because --offline was given"),
        "stderr: {err}"
    );

    // With --allow-empty: every fixture is marked skipped, no error.
    let output = run_in(
        &dir.path,
        &[
            "check",
            "--offline",
            "--allow-empty",
            "--rpc-url",
            "http://127.0.0.1:1",
            "--json",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "all-skipped must not be an error: {}",
        stderr(&output)
    );
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(report["status"], "pass");
    assert_eq!(report["counts"]["total"], 0);
    assert_eq!(report["counts"]["skipped"], 2);
    assert_eq!(report["results"].as_array().unwrap().len(), 0);
}

/// `--offline` with an XDR-only configuration is behaviorally identical to
/// a normal run: the flag changes nothing when nothing would have touched
/// the network anyway.
#[test]
fn offline_flag_is_transparent_for_an_xdr_only_pack() {
    let dir = TempProject::new("check-offline-xdr-only");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );

    let plain = run_in(&dir.path, &["check"]);
    let offline = run_in(&dir.path, &["check", "--offline"]);
    assert_eq!(plain.status.code(), Some(0));
    assert_eq!(offline.status.code(), Some(0), "{}", stderr(&offline));
    let text = stdout(&offline);
    assert!(text.contains("1/1 PASS"), "{text}");
}

/// `check --format junit` wires the JUnit reporter into the check path
/// end-to-end (the reporter itself is unit-tested in canary-report).
#[test]
fn junit_format_outputs_junit_xml_for_check() {
    let dir = TempProject::new("check-junit");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "decode-success", VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["check", "--format", "junit"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        "junit output must start with the XML declaration, got: {text}"
    );
    assert!(text.contains("<testsuites name=\"stellar-canary\""));
    assert!(text.contains("name=\"p28-xdr-1\""));
    assert!(text.trim_end().ends_with("</testsuites>"));
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
    // Negative case for the `Hint:` line: an error with no advice to give
    // prints exactly one clean error line, with no hint boilerplate.
    assert!(!err.contains("Hint:"), "stderr: {err}");
}

#[test]
fn a_missing_config_file_is_reported_with_an_actionable_hint() {
    let dir = TempProject::new("check-missing-config-hint");

    let output = run_in(&dir.path, &["check", "--config", "no-such-config.toml"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(
        err.contains("failed to read configuration file") && err.contains("no-such-config.toml"),
        "stderr: {err}"
    );
    assert!(err.contains("Hint:"), "stderr: {err}");
    assert!(err.contains(".stellar-canary.toml"), "stderr: {err}");
    assert!(err.contains("--config"), "stderr: {err}");
}
