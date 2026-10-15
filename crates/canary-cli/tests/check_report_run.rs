//! End-to-end coverage for the report's `run.toolBuildCommit` (CF-01).
//!
//! The commit is whatever this test binary's build script captured, so the
//! assertions are about shape, not a specific value: a Git checkout gives a
//! full object name, a build without Git history gives an explicit `null`.
//! Nothing here touches the network.

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
