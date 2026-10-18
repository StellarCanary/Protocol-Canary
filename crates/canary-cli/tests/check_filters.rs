mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = true\nsoroban = false\n";

fn xdr_fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

fn project(prefix: &str) -> TempProject {
    let dir = TempProject::new(prefix);
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir
}

#[test]
fn fixture_filter_executes_and_reports_only_the_requested_fixture() {
    let dir = project("check-filter-fixture");
    dir.write("fixtures/xdr-selected.toml", &xdr_fixture("xdr-selected"));
    dir.write("fixtures/xdr-other.toml", &xdr_fixture("xdr-other"));

    let output = run_in(&dir.path, &["check", "--fixture", "xdr-selected"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("xdr-selected"), "{out}");
    assert!(
        !out.contains("xdr-other"),
        "unselected result leaked: {out}"
    );
}

#[test]
fn surface_filter_executes_only_matching_fixtures() {
    let dir = project("check-filter-surface");
    dir.write("fixtures/xdr-selected.toml", &xdr_fixture("xdr-selected"));
    // This fixture would fail runner-specific parsing if the XDR filter did
    // not remove it before planning.
    dir.write(
        "fixtures/rpc-excluded.toml",
        "id = \"rpc-excluded\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"test\"\ndescription = \"test\"\nmethod = \"not-a-real-method\"\n",
    );

    let output = run_in(&dir.path, &["check", "--surface", "xdr"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("xdr-selected"), "{out}");
    assert!(
        !out.contains("rpc-excluded"),
        "unselected result leaked: {out}"
    );
}

#[test]
fn unknown_fixture_id_reports_no_matching_fixtures() {
    let dir = project("check-filter-missing");
    dir.write("fixtures/xdr-selected.toml", &xdr_fixture("xdr-selected"));

    let output = run_in(&dir.path, &["check", "--fixture", "missing-id"]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
    assert!(
        stderr(&output).contains("no matching fixtures found"),
        "{}",
        stderr(&output)
    );
}
