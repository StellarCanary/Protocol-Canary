//! End-to-end coverage for `stellar-canary check --output <PATH>`.
//!
//! These run the real binary against the same offline (XDR-only)
//! configuration and fixture shape `check_offline.rs` uses, so nothing
//! here touches the network.

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

fn xdr_fixture(id: &str, value_base64: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{value_base64}\"\n"
    )
}

/// A project directory with the offline configuration and a single
/// passing XDR fixture, plus the absolute path to hand to `--output`.
fn passing_offline_project(prefix: &str) -> TempProject {
    let dir = TempProject::new(prefix);
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", VALID_STELLAR_VALUE_BASE64),
    );
    dir
}

#[test]
fn output_writes_the_same_report_that_goes_to_stdout() {
    let dir = passing_offline_project("check-output-terminal");
    let report = dir.path.join("result.txt");

    let output = run_in(&dir.path, &["check", "--output", report.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(0));
    let printed = stdout(&output);
    let written = std::fs::read_to_string(&report).expect("--output must create the file");
    assert_eq!(
        written, printed,
        "the file must hold exactly what stdout received"
    );
    assert!(written.contains("1/1 applicable checks passed."));
}

#[test]
fn output_with_json_writes_a_parseable_report_matching_stdout() {
    let dir = passing_offline_project("check-output-json");
    let report = dir.path.join("result.json");

    let output = run_in(
        &dir.path,
        &["check", "--json", "--output", report.to_str().unwrap()],
    );

    assert_eq!(output.status.code(), Some(0));
    let written = std::fs::read_to_string(&report).expect("--output must create the file");
    assert_eq!(written, stdout(&output));

    let value: serde_json::Value = serde_json::from_str(&written).expect("valid json");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["targetProtocol"], 28);
    assert_eq!(value["counts"]["passed"], 1);
}

/// The contract in one test: `--output` is additive. Stdout still carries
/// the full report, and exit codes are untouched by the flag.
#[test]
fn output_is_additive_to_stdout_and_does_not_change_the_exit_code_on_failure() {
    let dir = TempProject::new("check-output-fail");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "not-valid-xdr-bytes!!!"),
    );
    let report = dir.path.join("result.txt");

    let output = run_in(&dir.path, &["check", "--output", report.to_str().unwrap()]);

    assert_eq!(
        output.status.code(),
        Some(1),
        "a compatibility failure must still exit 1 with --output set"
    );
    let written = std::fs::read_to_string(&report).expect("a failing run still writes its report");
    assert_eq!(written, stdout(&output));
    assert!(written.contains("Status: NOT READY"));
}

#[test]
fn output_in_quiet_mode_writes_the_single_status_line() {
    let dir = passing_offline_project("check-output-quiet");
    let report = dir.path.join("result.txt");

    let output = run_in(
        &dir.path,
        &["check", "--quiet", "--output", report.to_str().unwrap()],
    );

    assert_eq!(output.status.code(), Some(0));
    let written = std::fs::read_to_string(&report).expect("--output must create the file");
    assert_eq!(written, stdout(&output));
    assert_eq!(written.trim(), "Status: PASS");
}

#[test]
fn output_truncates_an_existing_file() {
    let dir = passing_offline_project("check-output-truncate");
    let report = dir.path.join("result.txt");
    dir.write("result.txt", "stale contents from an earlier run\n");

    let output = run_in(&dir.path, &["check", "--output", report.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(0));
    let written = std::fs::read_to_string(&report).expect("--output must write the file");
    assert!(!written.contains("stale contents"));
    assert_eq!(written, stdout(&output));
}

#[test]
fn an_unwritable_output_path_is_a_configuration_error() {
    let dir = passing_offline_project("check-output-unwritable");
    // `std::fs::write` does not create parent directories, so this path
    // cannot be written without one existing first.
    let report = dir.path.join("no-such-directory").join("result.txt");

    let output = run_in(&dir.path, &["check", "--output", report.to_str().unwrap()]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "an unusable --output path is a configuration error, not a compatibility result"
    );
    assert!(
        stdout(&output).is_empty(),
        "nothing can be printed for a run whose report was not written"
    );
    let err = stderr(&output);
    assert!(err.contains("failed to write report to"), "stderr: {err}");
    assert!(err.contains("result.txt"), "stderr: {err}");
}
