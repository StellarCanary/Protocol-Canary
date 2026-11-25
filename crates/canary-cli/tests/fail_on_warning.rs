mod support;

use serde_json::Value;
use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn project(prefix: &str) -> TempProject {
    let dir = TempProject::new(prefix);
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &format!(
            "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir
}

/// Creates a real successful cache entry, then changes only its fixture
/// status so the CLI exercises the warning policy through normal cache reads.
fn seed_warning_result(dir: &TempProject) {
    let first = run_in(&dir.path, &["check"]);
    assert_eq!(first.status.code(), Some(0), "{}", stdout(&first));

    let cache_dir = dir.path.join(".stellar-canary-cache");
    let entry_path = std::fs::read_dir(&cache_dir)
        .unwrap()
        .next()
        .expect("check should cache the XDR result")
        .unwrap()
        .path();
    let mut entry: Value = serde_json::from_slice(&std::fs::read(&entry_path).unwrap()).unwrap();
    entry["result"]["status"] = Value::String("warning".into());
    std::fs::write(&entry_path, serde_json::to_vec_pretty(&entry).unwrap()).unwrap();
}

#[test]
fn warnings_exit_zero_without_fail_on_warning() {
    let dir = project("fail-on-warning-default");
    seed_warning_result(&dir);

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("WARNING"), "{}", stdout(&output));
}

#[test]
fn warnings_exit_one_with_fail_on_warning() {
    let dir = project("fail-on-warning-enabled");
    seed_warning_result(&dir);

    let output = run_in(&dir.path, &["check", "--fail-on-warning"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(stdout(&output).contains("WARN"), "{}", stdout(&output));
}

#[test]
fn no_warnings_exit_zero_with_fail_on_warning() {
    let dir = project("fail-on-warning-clean");
    let output = run_in(&dir.path, &["check", "--fail-on-warning"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}

/// Parses a JSON report and asserts the parts every caller needs: the run is
/// reported as `warning`, the warning count is right, and the individual
/// fixture that warned is still listed with its `warning` status.
fn assert_preserved_warning_report(report: &Value) {
    assert_eq!(report["schemaVersion"], 1, "{report}");
    assert_eq!(report["targetProtocol"], 28, "{report}");
    assert_eq!(
        report["status"], "warning",
        "fail-on-warning gates the exit code but must not rewrite the report: {report}"
    );
    assert_eq!(report["counts"]["warnings"], 1, "{report}");
    assert_eq!(report["counts"]["failed"], 0, "{report}");

    let results = report["results"]
        .as_array()
        .unwrap_or_else(|| panic!("results must be an array: {report}"));
    let warning = results
        .iter()
        .find(|result| result["fixtureId"] == "p28-xdr-1")
        .unwrap_or_else(|| panic!("the warning fixture must be in the report: {report}"));
    assert_eq!(warning["status"], "warning", "{report}");
    assert_eq!(warning["surface"], "xdr", "{report}");
}

/// `--fail-on-warning` makes the process exit 1, but the machine-readable
/// report must still be emitted in full on stdout so CI can inspect which
/// fixture warned. The exit code and the report are both part of the
/// contract: suppressing the payload on failure would leave a failing job
/// with nothing to inspect.
#[test]
fn fail_on_warning_still_emits_the_warning_json_report_on_stdout() {
    let dir = project("fail-on-warning-json-stdout");
    seed_warning_result(&dir);

    let output = run_in(
        &dir.path,
        &["check", "--fail-on-warning", "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));

    let report: Value = serde_json::from_str(&stdout(&output))
        .expect("stdout must carry the complete JSON report even when the run exits 1");
    assert_preserved_warning_report(&report);
}

/// The same guarantee through `--output`: the file holds the full report and
/// stdout still receives it, while the exit code is the failing one.
#[test]
fn fail_on_warning_writes_the_warning_json_report_to_the_output_file() {
    let dir = project("fail-on-warning-json-output");
    seed_warning_result(&dir);
    let report_path = dir.path.join("report.json");

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--fail-on-warning",
            "--format",
            "json",
            "--output",
            report_path.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));

    let written = std::fs::read_to_string(&report_path)
        .expect("--output must create the report even when the run exits 1");
    assert_eq!(
        written,
        stdout(&output),
        "the file must hold exactly the report printed to stdout"
    );

    let report: Value =
        serde_json::from_str(&written).expect("the written report must be valid JSON");
    assert_preserved_warning_report(&report);
}
