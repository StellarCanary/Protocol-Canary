//! A fixture whose body has a key the engine does not read must be rejected,
//! not run: a misspelled section used to drop the fixture's assertions and let
//! it pass while checking nothing.

mod support;

use support::{run_in, stderr, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = true\nsoroban = false\n";

fn xdr(extra: &str) -> String {
    format!(
        "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n{extra}"
    )
}

const RPC_HEAD: &str = "id = \"p28-rpc-1\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"test\"\ndescription = \"test\"\nmethod = \"get-network\"\n";

fn project(name: &str, body: &str) -> TempProject {
    let dir = TempProject::new(name);
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/f.toml", body);
    dir
}

#[test]
fn an_unknown_key_in_an_xdr_fixture_is_an_invalid_fixture() {
    let dir = project("strict-xdr", &xdr("expected_base64x = \"ignored\"\n"));
    let output = run_in(&dir.path, &["check", "--allow-empty"]);
    assert_eq!(output.status.code(), Some(4));
    let err = stderr(&output);
    assert!(err.contains("unknown key(s) \"expected_base64x\""), "{err}");
}

#[test]
fn expected_base64_is_only_accepted_for_encode_equals() {
    let dir = project("strict-xdr-expected", &xdr("expected_base64 = \"AAAA\"\n"));
    let output = run_in(&dir.path, &["check", "--allow-empty"]);
    assert!(
        stderr(&output).contains("unknown key(s) \"expected_base64\""),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_misspelled_assert_table_does_not_leave_an_rpc_fixture_with_nothing_to_check() {
    let body = format!(
        "{RPC_HEAD}\n[[asert]]\nkind = \"field-equals\"\nfield = \"protocolVersion\"\nvalue = 99\n"
    );
    let dir = project("strict-rpc-typo", &body);
    let output = run_in(&dir.path, &["check", "--rpc-url", "http://127.0.0.1:1"]);
    let err = stderr(&output);
    assert!(err.contains("unknown key(s) \"asert\""), "{err}");
    assert_eq!(output.status.code(), Some(4));
}

#[test]
fn an_rpc_fixture_without_any_assertion_is_rejected() {
    let dir = project("strict-rpc-empty", RPC_HEAD);
    let output = run_in(&dir.path, &["check", "--rpc-url", "http://127.0.0.1:1"]);
    assert!(
        stderr(&output).contains("needs at least one [[assert]]"),
        "{}",
        stderr(&output)
    );
    assert_eq!(output.status.code(), Some(4));
}

#[test]
fn an_unknown_key_inside_an_assert_entry_is_rejected() {
    let body = format!(
        "{RPC_HEAD}\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\nexpect = \"x\"\n"
    );
    let dir = project("strict-rpc-entry", &body);
    let output = run_in(&dir.path, &["check", "--rpc-url", "http://127.0.0.1:1"]);
    assert!(
        stderr(&output).contains("[[assert]] entry: unknown key(s) \"expect\""),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_well_formed_xdr_fixture_still_runs() {
    let dir = project("strict-ok", &xdr(""));
    let output = run_in(&dir.path, &["check", "--rpc-url", "http://127.0.0.1:1"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}
