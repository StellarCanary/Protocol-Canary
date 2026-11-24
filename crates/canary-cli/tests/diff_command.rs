//! End-to-end tests for `stellar-canary diff`
//! (`cargo test -p canary-cli --test diff_command`), covering issue
//! #448's CF-05 exit-code contract as documented in
//! `docs/contracts/cf-05-report-comparison.md`:
//!
//! - 0 = verdict `no-change` or `improved`
//! - 1 = verdict `regression` or `coverage-lost`
//! - 2 = reports not comparable, or an input file is missing/invalid

mod support;

use support::{run_in, stderr, stdout, TempProject};

/// A minimal-but-valid `check --json` report: exactly the fields
/// `JsonReporter::parse` requires, with everything else defaulted the
/// same way a real report defaults it.
fn report_json(target_protocol: u32, results: &[(&str, &str, &str)]) -> String {
    let results: Vec<serde_json::Value> = results
        .iter()
        .map(|(test_id, surface, status)| {
            serde_json::json!({
                "testId": test_id,
                "protocol": target_protocol,
                "surface": surface,
                "status": status,
                "summary": "synthetic",
                "durationMs": 1,
                "fixtureId": test_id,
            })
        })
        .collect();
    let value = serde_json::json!({
        "schemaVersion": 1,
        "toolVersion": "0.0.0-test",
        "targetProtocol": target_protocol,
        "project": { "name": "diff-command", "type": "unknown" },
        "status": "pass",
        "results": results,
    });
    serde_json::to_string_pretty(&value).expect("serializes")
}

struct DiffProject {
    dir: support::TempProject,
}

impl DiffProject {
    fn new(prefix: &str) -> Self {
        DiffProject {
            dir: TempProject::new(prefix),
        }
    }

    fn write_report(&self, name: &str, target_protocol: u32, results: &[(&str, &str, &str)]) {
        self.dir.write(name, &report_json(target_protocol, results));
    }

    fn diff(&self, current: &str, baseline: &str) -> std::process::Output {
        run_in(&self.dir.path, &["diff", current, baseline])
    }
}

fn exit_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .expect("diff must exit with a normal process code")
}

#[test]
fn identical_reports_exit_zero_with_a_no_change_verdict() {
    let project = DiffProject::new("diff-identical");
    let results = [("p28-xdr-1", "xdr", "pass"), ("p28-rpc-1", "rpc", "pass")];
    project.write_report("current.json", 28, &results);
    project.write_report("baseline.json", 28, &results);

    let output = project.diff("current.json", "baseline.json");
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));

    let text = stdout(&output);
    let value: serde_json::Value = serde_json::from_str(&text).expect("stdout is diff JSON");
    assert_eq!(value["diffVersion"], 1);
    assert_eq!(value["verdict"], "no-change");
}

#[test]
fn an_improved_report_exits_zero() {
    let project = DiffProject::new("diff-improved");
    project.write_report("baseline.json", 28, &[("p28-xdr-1", "xdr", "fail")]);
    project.write_report("current.json", 28, &[("p28-xdr-1", "xdr", "pass")]);

    let output = project.diff("current.json", "baseline.json");
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert_eq!(value["verdict"], "improved");
}

#[test]
fn a_regressed_report_exits_one() {
    let project = DiffProject::new("diff-regression");
    project.write_report("baseline.json", 28, &[("p28-xdr-1", "xdr", "pass")]);
    project.write_report("current.json", 28, &[("p28-xdr-1", "xdr", "fail")]);

    let output = project.diff("current.json", "baseline.json");
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert_eq!(value["verdict"], "regression");
    assert_eq!(value["entries"][0]["from"], "pass");
    assert_eq!(value["entries"][0]["to"], "fail");
    assert_eq!(value["entries"][0]["classification"], "regressed");
    assert_eq!(value["entries"][0]["regression"], true);
}

#[test]
fn lost_coverage_exits_one() {
    let project = DiffProject::new("diff-coverage-lost");
    project.write_report(
        "baseline.json",
        28,
        &[("p28-xdr-1", "xdr", "pass"), ("p28-rpc-1", "rpc", "pass")],
    );
    project.write_report("current.json", 28, &[("p28-xdr-1", "xdr", "pass")]);

    let output = project.diff("current.json", "baseline.json");
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert_eq!(value["verdict"], "coverage-lost");
    assert_eq!(value["missing"][0]["id"], "p28-rpc-1");
}

#[test]
fn argument_order_is_current_then_baseline() {
    // The same two reports as the regression test, swapped: what was a
    // pass->fail regression must now read as a fail->pass improvement.
    let project = DiffProject::new("diff-arg-order");
    project.write_report("baseline.json", 28, &[("p28-xdr-1", "xdr", "pass")]);
    project.write_report("current.json", 28, &[("p28-xdr-1", "xdr", "fail")]);

    let output = project.diff("baseline.json", "current.json");
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert_eq!(value["verdict"], "improved");
}

#[test]
fn a_missing_report_file_exits_two_and_names_the_file() {
    let project = DiffProject::new("diff-missing-file");
    project.write_report("baseline.json", 28, &[("p28-xdr-1", "xdr", "pass")]);

    let output = project.diff("no-such-current.json", "baseline.json");
    assert_eq!(exit_code(&output), 2);
    let message = stderr(&output);
    assert!(
        message.contains("failed to read report file") && message.contains("no-such-current.json"),
        "error must name the unreadable file, got: {message}"
    );
}

#[test]
fn an_unparseable_report_exits_two() {
    let project = DiffProject::new("diff-unparseable");
    project.dir.write("current.json", "not json at all");
    project.write_report("baseline.json", 28, &[("p28-xdr-1", "xdr", "pass")]);

    let output = project.diff("current.json", "baseline.json");
    assert_eq!(exit_code(&output), 2);
    let message = stderr(&output);
    assert!(
        message.contains("not a valid report"),
        "error must say the report is invalid, got: {message}"
    );
    // A refused comparison produces no diff JSON on stdout at all.
    assert!(
        stdout(&output).trim().is_empty(),
        "stdout must stay empty on refusal, got: {}",
        stdout(&output)
    );
}

#[test]
fn comparing_protocol_28_against_29_exits_two_with_a_protocol_mismatch() {
    let project = DiffProject::new("diff-protocol-mismatch");
    project.write_report("baseline.json", 28, &[("p28-xdr-1", "xdr", "pass")]);
    project.write_report("current.json", 29, &[("p28-xdr-1", "xdr", "pass")]);

    let output = project.diff("current.json", "baseline.json");
    assert_eq!(exit_code(&output), 2);
    let message = stderr(&output);
    assert!(
        message.contains("not comparable") && message.contains("28") && message.contains("29"),
        "error must explain the protocol mismatch, got: {message}"
    );
    // A refused comparison produces no diff JSON on stdout at all.
    assert!(
        stdout(&output).trim().is_empty(),
        "stdout must stay empty on refusal, got: {}",
        stdout(&output)
    );
}

#[test]
fn mismatched_schema_versions_exit_two() {
    let project = DiffProject::new("diff-schema-mismatch");
    let current = serde_json::json!({
        "schemaVersion": 2,
        "toolVersion": "0.0.0-test",
        "targetProtocol": 28,
        "project": { "name": "diff-command", "type": "unknown" },
        "status": "pass",
        "results": [{
            "testId": "p28-xdr-1",
            "protocol": 28,
            "surface": "xdr",
            "status": "pass",
            "summary": "synthetic",
            "durationMs": 1,
        }],
    });
    project.dir.write(
        "current.json",
        &serde_json::to_string_pretty(&current).unwrap(),
    );
    project.write_report("baseline.json", 28, &[("p28-xdr-1", "xdr", "pass")]);

    let output = project.diff("current.json", "baseline.json");
    assert_eq!(exit_code(&output), 2);
    let message = stderr(&output);
    assert!(
        message.contains("schemaVersion") && message.contains("does not support"),
        "error must explain the schema mismatch, got: {message}"
    );
    assert!(
        stdout(&output).trim().is_empty(),
        "stdout must stay empty on refusal, got: {}",
        stdout(&output)
    );
}

#[test]
fn a_diff_of_the_same_reports_is_repeatable() {
    let project = DiffProject::new("diff-repeatable");
    project.write_report("baseline.json", 28, &[("p28-xdr-1", "xdr", "pass")]);
    project.write_report("current.json", 28, &[("p28-xdr-1", "xdr", "fail")]);

    let first = project.diff("current.json", "baseline.json");
    let second = project.diff("current.json", "baseline.json");
    assert_eq!(stdout(&first), stdout(&second));
}
