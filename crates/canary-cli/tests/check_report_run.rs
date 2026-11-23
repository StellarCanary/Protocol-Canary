//! End-to-end coverage for the report's `run.toolBuildCommit` (CF-01).
//!
//! The commit is whatever this test binary's build script captured, so the
//! assertions are about shape, not a specific value: a Git checkout gives a
//! full object name, a build without Git history gives an explicit `null`.
//! Nothing here touches the network.

mod support;

use support::{
    run_in, stderr, stdout, TempProject, DECODE_FAILURE_BASE64, VALID_STELLAR_VALUE_BASE64,
};

const OFFLINE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = true
rpc = false
soroban = false
"#;

const MIXED_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = true
rpc = false
soroban = false
"#;

fn json_report() -> serde_json::Value {
    let dir = TempProject::new("check-run-provenance");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &format!(
            "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );

    let output = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    serde_json::from_str(&stdout(&output)).expect("stdout is the JSON report")
}

#[test]
fn the_report_records_the_build_commit_under_run() {
    let report = json_report();

    let run = report["run"].as_object().expect("`run` is an object");
    let commit = run
        .get("toolBuildCommit")
        .expect("`toolBuildCommit` is always written, even when unknown");

    match commit {
        serde_json::Value::Null => {}
        serde_json::Value::String(value) => {
            assert!(value.len() == 40 || value.len() == 64, "{value:?}");
            assert!(
                value
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "{value:?}"
            );
        }
        other => panic!("toolBuildCommit must be a string or null, got {other}"),
    }
}

#[test]
fn the_report_carries_no_build_timestamp() {
    // CF-01 section 4: report identity excludes timestamps, and version 1
    // must stay byte-for-byte comparable across builds of one commit.
    let report = serde_json::to_string(&json_report()).unwrap();
    assert!(!report.contains("buildTimestamp"), "{report}");
    assert!(!report.contains("\"engine\""), "{report}");
}

#[test]
fn the_build_commit_is_stable_across_two_runs_of_one_binary() {
    let first = json_report();
    let second = json_report();
    assert_eq!(first["run"], second["run"]);
}

/// One fixture per outcome category the report distinguishes: a decode that
/// succeeds (`pass`), a decode that fails (`fail`), and a fixture for another
/// protocol that the planner skips.
fn mixed_fixture(id: &str, protocol: u32, value_base64: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = {protocol}\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{value_base64}\"\n"
    )
}

/// The `passed` and `total` halves of the `"{passed}/{total} applicable
/// checks passed."` summary line the terminal reporter prints.
fn terminal_applicable_counts(text: &str) -> (usize, usize) {
    let line = text
        .lines()
        .find(|line| line.contains("applicable checks passed."))
        .unwrap_or_else(|| panic!("no terminal summary line in: {text}"));
    let fraction = line
        .split_whitespace()
        .next()
        .expect("the summary line starts with passed/total");
    let (passed, total) = fraction
        .split_once('/')
        .unwrap_or_else(|| panic!("expected `passed/total`, got {fraction:?}"));
    (
        passed
            .parse()
            .unwrap_or_else(|e| panic!("passed count {passed:?}: {e}")),
        total
            .parse()
            .unwrap_or_else(|e| panic!("total count {total:?}: {e}")),
    )
}

/// The `Skipped fixtures: N` line, when present. A run with no planner skips
/// omits the line entirely, which reads as zero.
fn terminal_skipped_count(text: &str) -> usize {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("Skipped fixtures: "))
        .map(|count| {
            count
                .trim()
                .parse()
                .unwrap_or_else(|e| panic!("skipped count {count:?}: {e}"))
        })
        .unwrap_or(0)
}

/// Issue #736: the terminal report and the JSON report describe one run, so
/// the summary numbers a human reads must equal the `counts` a CI consumer
/// reads. The terminal summary only spells out `passed/total` and the skip
/// count; because this run has no warning or error results, the remaining
/// `total - passed` checks are exactly the failed ones.
#[test]
fn terminal_and_json_summary_statistics_agree_for_a_mixed_run() {
    let dir = TempProject::new("check-run-statistics-parity");
    dir.write(".stellar-canary.toml", MIXED_CONFIG);
    dir.write(
        "fixtures/p28-xdr-pass.toml",
        &mixed_fixture("p28-xdr-pass", 28, VALID_STELLAR_VALUE_BASE64),
    );
    dir.write(
        "fixtures/p28-xdr-fail.toml",
        &mixed_fixture("p28-xdr-fail", 28, DECODE_FAILURE_BASE64),
    );
    dir.write(
        "fixtures/p27-xdr-skipped.toml",
        &mixed_fixture("p27-xdr-skipped", 27, VALID_STELLAR_VALUE_BASE64),
    );

    let terminal = run_in(&dir.path, &["check"]);
    assert_eq!(
        terminal.status.code(),
        Some(1),
        "a failed decode must exit 1\n{}\n{}",
        stdout(&terminal),
        stderr(&terminal)
    );
    let text = stdout(&terminal);
    let (passed, total) = terminal_applicable_counts(&text);
    let skipped = terminal_skipped_count(&text);

    let json_run = run_in(&dir.path, &["check", "--json"]);
    assert_eq!(
        json_run.status.code(),
        Some(1),
        "the JSON run reports the same outcome\n{}\n{}",
        stdout(&json_run),
        stderr(&json_run)
    );
    let report: serde_json::Value =
        serde_json::from_str(&stdout(&json_run)).expect("stdout is the JSON report");
    let counts = report["counts"]
        .as_object()
        .unwrap_or_else(|| panic!("`counts` is an object: {report}"));

    // Non-vacuous coverage: the run really exercised all three categories.
    assert_eq!(
        (passed, total, skipped),
        (1, 2, 1),
        "terminal summary out of step with the fixtures: {text}"
    );
    assert_eq!(counts["warnings"].as_u64(), Some(0), "{report}");
    assert_eq!(counts["errors"].as_u64(), Some(0), "{report}");

    assert_eq!(counts["total"].as_u64(), Some(total as u64), "{report}");
    assert_eq!(counts["passed"].as_u64(), Some(passed as u64), "{report}");
    assert_eq!(
        counts["failed"].as_u64(),
        Some((total - passed) as u64),
        "the remaining applicable checks are exactly the failures: {report}"
    );
    assert_eq!(counts["skipped"].as_u64(), Some(skipped as u64), "{report}");
    assert_eq!(report["status"], "fail", "{report}");
}
