//! A run that executes no fixture must not look like a pass.
//!
//! Releases up to 0.1.1 exited 0 with `status: pass` when nothing applied.
//! From the next release that is exit code 2 with a diagnostic, unless
//! `--allow-empty` is given.

mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn xdr_fixture(id: &str, protocol: u32, extra: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = {protocol}\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n{extra}"
    )
}

fn project() -> TempProject {
    let dir = TempProject::new("empty-run");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir
}

/// The refusal is a configuration error: exit 2, a diagnostic on stderr, and
/// nothing on stdout that could be mistaken for a report or a success.
fn assert_refused(output: &std::process::Output) -> String {
    assert_eq!(output.status.code(), Some(2), "stdout: {}", stdout(output));
    assert_eq!(stdout(output), "", "no report or status line on stdout");
    let err = stderr(output);
    assert!(
        err.starts_with("error: configuration error: no checks ran: "),
        "{err}"
    );
    assert!(err.contains("not evidence of compatibility"), "{err}");
    assert!(err.contains("--allow-empty"), "{err}");
    err
}

#[test]
fn a_missing_fixtures_directory_fails_and_says_so() {
    let dir = project();
    let err = assert_refused(&run_in(&dir.path, &["check"]));
    assert!(
        err.contains("the fixtures directory fixtures does not exist"),
        "{err}"
    );
}

#[test]
fn a_directory_without_fixture_files_fails_and_says_so() {
    let dir = project();
    dir.write("fixtures/README.md", "not a fixture");
    let err = assert_refused(&run_in(&dir.path, &["check"]));
    assert!(
        err.contains("no fixture files (*.toml) were found in fixtures"),
        "{err}"
    );
}

#[test]
fn fixtures_for_another_protocol_fail_and_name_both_protocols() {
    // The shape of `check --protocol 29` against the shipped Protocol 28 pack.
    let dir = project();
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28, ""));
    dir.write("fixtures/p28-xdr-2.toml", &xdr_fixture("p28-xdr-2", 28, ""));

    let err = assert_refused(&run_in(&dir.path, &["check", "--protocol", "29"]));
    assert!(err.contains("all 2 loaded fixtures were skipped"), "{err}");
    assert!(err.contains("2 target another protocol"), "{err}");
    assert!(err.contains("this run targets protocol 29"), "{err}");
    assert!(err.contains("the fixtures target protocol 28"), "{err}");
}

#[test]
fn json_mode_does_not_print_a_passing_report_for_an_empty_run() {
    let dir = project();
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28, ""));
    let output = run_in(&dir.path, &["check", "--protocol", "29", "--json"]);
    assert_refused(&output);
}

#[test]
fn a_disabled_surface_fails_and_says_so() {
    let dir = project();
    dir.write(
        ".stellar-canary.toml",
        "version = 1\nprotocol = 28\n\n[tests]\nxdr = false\nrpc = true\nsoroban = false\n",
    );
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28, ""));

    // Port 1 refuses connections at once, so no endpoint is needed: the plan
    // is empty whatever the network does.
    let err = assert_refused(&run_in(
        &dir.path,
        &["check", "--rpc-url", "http://127.0.0.1:1"],
    ));
    assert!(
        err.contains("1 are on a surface that is disabled in configuration"),
        "{err}"
    );
}

#[test]
fn a_missing_required_capability_fails_and_says_so() {
    let dir = project();
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture(
            "p28-xdr-1",
            28,
            "required_capabilities = [\"soroban-contract\"]\n",
        ),
    );

    let err = assert_refused(&run_in(&dir.path, &["check"]));
    assert!(
        err.contains("1 require a capability this project does not declare"),
        "{err}"
    );
}

#[test]
fn every_cause_is_reported_when_they_are_mixed() {
    let dir = project();
    dir.write(
        "fixtures/a-other-protocol.toml",
        &xdr_fixture("a-other-protocol", 27, ""),
    );
    dir.write(
        "fixtures/b-needs-soroban.toml",
        &xdr_fixture(
            "b-needs-soroban",
            28,
            "required_capabilities = [\"soroban-contract\"]\n",
        ),
    );

    let err = assert_refused(&run_in(&dir.path, &["check"]));
    assert!(err.contains("all 2 loaded fixtures were skipped"), "{err}");
    assert!(err.contains("1 target another protocol"), "{err}");
    assert!(err.contains("1 require a capability"), "{err}");
}

#[test]
fn allow_empty_runs_and_warns_that_the_result_is_not_evidence() {
    let dir = project();
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28, ""));

    let output = run_in(
        &dir.path,
        &["check", "--protocol", "29", "--allow-empty", "--json"],
    );
    assert_eq!(output.status.code(), Some(0));
    let err = stderr(&output);
    assert!(err.contains("warning: no checks ran"), "{err}");
    assert!(err.contains("not evidence of compatibility"), "{err}");

    // The report keeps its documented v1 shape: an empty `results`, the
    // fixture listed under `skipped`, and counts that show nothing ran.
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["counts"]["total"], 0);
    assert_eq!(report["counts"]["skipped"], 1);
    assert_eq!(report["results"].as_array().unwrap().len(), 0);
}

#[test]
fn allow_empty_does_not_hide_a_failure() {
    let dir = project();
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", 28, "").replace(VALID_STELLAR_VALUE_BASE64, "not-valid-xdr!!"),
    );

    let output = run_in(&dir.path, &["check", "--allow-empty"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!stderr(&output).contains("no checks ran"));
}

#[test]
fn a_run_that_executes_at_least_one_fixture_is_unaffected() {
    let dir = project();
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28, ""));
    dir.write("fixtures/p27-xdr-1.toml", &xdr_fixture("p27-xdr-1", 27, ""));

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("1/1 applicable checks passed."));
    assert!(!stderr(&output).contains("no checks ran"));
}

#[test]
fn inspect_and_fixtures_still_work_on_an_empty_plan() {
    // The guard belongs to `check`; the offline commands exist to show why a
    // plan is empty.
    let dir = project();
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28, ""));
    for command in ["inspect", "fixtures"] {
        let output = run_in(&dir.path, &[command, "--protocol", "29"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{command}: {}",
            stderr(&output)
        );
    }
}
