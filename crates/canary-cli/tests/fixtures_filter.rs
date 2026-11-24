//! Integration tests for `stellar-canary fixtures` filtering by `--surface` and `--category` (issue #467, [E-039]).
//!
//! Asserts that:
//! - `--surface` filters fixtures across all surfaces (xdr, rpc, soroban).
//! - `--category` filters fixtures across categories (e.g. auth, token, ledger).
//! - Filter combinations (--surface and --category together) work as expected.
//! - Filtering works across both text table (terminal) and `--format json` (including `--json` shorthand).
//! - Filtering by a non-existent category outputs 0 fixtures cleanly with an empty array in JSON mode and exit code 0.
//! - Invalid fixtures in the directory still cause exit code 4 before filtering.

mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use serde_json::Value;

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = true\nsoroban = true\n";

fn setup_test_project(name: &str) -> TempProject {
    let dir = TempProject::new(name);
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    // XDR fixtures
    dir.write(
        "fixtures/xdr/ledger.toml",
        &format!(
            "id = \"p28-xdr-ledger\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"ledger entry fixture\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir.write(
        "fixtures/xdr/auth.toml",
        &format!(
            "id = \"p28-xdr-auth\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"auth\"\ndescription = \"auth entry fixture\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );

    // RPC fixtures
    dir.write(
        "fixtures/rpc/network.toml",
        "id = \"p28-rpc-network\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"network\"\ndescription = \"rpc network fixture\"\nmethod = \"get-network\"\n",
    );
    dir.write(
        "fixtures/rpc/auth.toml",
        "id = \"p28-rpc-auth\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"auth\"\ndescription = \"rpc auth fixture\"\nmethod = \"get-auth\"\n",
    );

    // Soroban fixtures
    dir.write(
        "fixtures/soroban/token.toml",
        "id = \"p28-soroban-token\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"token\"\ndescription = \"soroban token fixture\"\nfunction = \"balance\"\n",
    );
    dir.write(
        "fixtures/soroban/auth.toml",
        "id = \"p28-soroban-auth\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"auth\"\ndescription = \"soroban auth fixture\"\nfunction = \"authenticate\"\n",
    );

    dir
}

#[test]
fn filter_by_surface_json() {
    let dir = setup_test_project("fixtures-filter-surface-json");

    // Filter XDR
    let output = run_in(
        &dir.path,
        &["fixtures", "--surface", "xdr", "--format", "json"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(doc["counts"]["total"], 2);
    assert_eq!(doc["counts"]["xdr"], 2);
    assert_eq!(doc["counts"]["rpc"], 0);
    assert_eq!(doc["counts"]["soroban"], 0);
    let ids: Vec<&str> = doc["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["p28-xdr-auth", "p28-xdr-ledger"]);

    // Filter RPC
    let output = run_in(&dir.path, &["fixtures", "--surface", "rpc", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(doc["counts"]["total"], 2);
    assert_eq!(doc["counts"]["xdr"], 0);
    assert_eq!(doc["counts"]["rpc"], 2);
    assert_eq!(doc["counts"]["soroban"], 0);
    let ids: Vec<&str> = doc["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["p28-rpc-auth", "p28-rpc-network"]);

    // Filter Soroban
    let output = run_in(
        &dir.path,
        &["fixtures", "--surface", "soroban", "--format", "json"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(doc["counts"]["total"], 2);
    assert_eq!(doc["counts"]["xdr"], 0);
    assert_eq!(doc["counts"]["rpc"], 0);
    assert_eq!(doc["counts"]["soroban"], 2);
    let ids: Vec<&str> = doc["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["p28-soroban-auth", "p28-soroban-token"]);
}

#[test]
fn filter_by_surface_terminal() {
    let dir = setup_test_project("fixtures-filter-surface-terminal");

    // XDR only
    let output = run_in(&dir.path, &["fixtures", "--surface", "xdr"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("XDR"));
    assert!(text.contains("p28-xdr-auth"));
    assert!(text.contains("p28-xdr-ledger"));
    assert!(!text.contains("RPC"));
    assert!(!text.contains("p28-rpc-auth"));
    assert!(!text.contains("Soroban"));
    assert!(!text.contains("p28-soroban-token"));

    // RPC only
    let output = run_in(&dir.path, &["fixtures", "--surface", "rpc"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(!text.contains("XDR"));
    assert!(text.contains("RPC"));
    assert!(text.contains("p28-rpc-auth"));
    assert!(text.contains("p28-rpc-network"));
    assert!(!text.contains("Soroban"));

    // Soroban only
    let output = run_in(&dir.path, &["fixtures", "--surface", "soroban"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(!text.contains("XDR"));
    assert!(!text.contains("RPC"));
    assert!(text.contains("Soroban"));
    assert!(text.contains("p28-soroban-auth"));
    assert!(text.contains("p28-soroban-token"));
}

#[test]
fn filter_by_category_json() {
    let dir = setup_test_project("fixtures-filter-category-json");

    // Category "auth" across all 3 surfaces
    let output = run_in(
        &dir.path,
        &["fixtures", "--category", "auth", "--format", "json"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(doc["counts"]["total"], 3);
    assert_eq!(doc["counts"]["xdr"], 1);
    assert_eq!(doc["counts"]["rpc"], 1);
    assert_eq!(doc["counts"]["soroban"], 1);
    let ids: Vec<&str> = doc["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec!["p28-rpc-auth", "p28-soroban-auth", "p28-xdr-auth"]
    );

    // Category "token"
    let output = run_in(
        &dir.path,
        &["fixtures", "--category", "token", "--format", "json"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(doc["counts"]["total"], 1);
    assert_eq!(doc["counts"]["soroban"], 1);
    let ids: Vec<&str> = doc["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["p28-soroban-token"]);

    // Category "ledger"
    let output = run_in(
        &dir.path,
        &["fixtures", "--category", "ledger", "--format", "json"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(doc["counts"]["total"], 1);
    assert_eq!(doc["counts"]["xdr"], 1);
    let ids: Vec<&str> = doc["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["p28-xdr-ledger"]);
}

#[test]
fn filter_by_category_terminal() {
    let dir = setup_test_project("fixtures-filter-category-terminal");

    let output = run_in(&dir.path, &["fixtures", "--category", "auth"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("XDR"));
    assert!(text.contains("p28-xdr-auth"));
    assert!(!text.contains("p28-xdr-ledger"));
    assert!(text.contains("RPC"));
    assert!(text.contains("p28-rpc-auth"));
    assert!(!text.contains("p28-rpc-network"));
    assert!(text.contains("Soroban"));
    assert!(text.contains("p28-soroban-auth"));
    assert!(!text.contains("p28-soroban-token"));

    let output = run_in(&dir.path, &["fixtures", "--category", "token"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(!text.contains("XDR"));
    assert!(!text.contains("RPC"));
    assert!(text.contains("Soroban"));
    assert!(text.contains("p28-soroban-token"));
}

#[test]
fn filter_combinations_surface_and_category() {
    let dir = setup_test_project("fixtures-filter-combinations");

    // XDR + auth
    let output = run_in(
        &dir.path,
        &[
            "fixtures",
            "--surface",
            "xdr",
            "--category",
            "auth",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(doc["counts"]["total"], 1);
    assert_eq!(doc["counts"]["xdr"], 1);
    assert_eq!(doc["fixtures"][0]["id"], "p28-xdr-auth");

    // RPC + auth
    let output = run_in(
        &dir.path,
        &[
            "fixtures",
            "--surface",
            "rpc",
            "--category",
            "auth",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(doc["counts"]["total"], 1);
    assert_eq!(doc["counts"]["rpc"], 1);
    assert_eq!(doc["fixtures"][0]["id"], "p28-rpc-auth");

    // Soroban + auth in terminal
    let output = run_in(
        &dir.path,
        &["fixtures", "--surface", "soroban", "--category", "auth"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(!text.contains("XDR"));
    assert!(!text.contains("RPC"));
    assert!(text.contains("Soroban"));
    assert!(text.contains("p28-soroban-auth"));
    assert!(!text.contains("p28-soroban-token"));

    // Soroban + ledger (does not exist)
    let output = run_in(
        &dir.path,
        &[
            "fixtures",
            "--surface",
            "soroban",
            "--category",
            "ledger",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(doc["counts"]["total"], 0);
    assert_eq!(doc["fixtures"].as_array().unwrap().len(), 0);
}

#[test]
fn filter_by_nonexistent_category_outputs_zero_fixtures_cleanly() {
    let dir = setup_test_project("fixtures-filter-nonexistent");

    // In JSON mode: outputs 0 fixtures cleanly with empty array in JSON mode
    let output = run_in(
        &dir.path,
        &[
            "fixtures",
            "--category",
            "nonexistent-category",
            "--format",
            "json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(output.status.code(), Some(0));
    assert!(stderr(&output).is_empty());

    let raw = stdout(&output);
    let doc: Value = serde_json::from_str(&raw).expect("valid json");
    assert_eq!(doc["inventoryVersion"], 1);
    assert_eq!(doc["directoryFound"], true);
    assert_eq!(doc["counts"]["total"], 0);
    assert_eq!(doc["counts"]["xdr"], 0);
    assert_eq!(doc["counts"]["rpc"], 0);
    assert_eq!(doc["counts"]["soroban"], 0);
    assert_eq!(doc["fixtures"], serde_json::json!([]));

    // In terminal mode: outputs "(no fixtures found in <dir>)" cleanly with exit code 0
    let output = run_in(
        &dir.path,
        &["fixtures", "--category", "nonexistent-category"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("(no fixtures found in "));
}

#[test]
fn invalid_fixture_still_triggers_failure_with_exit_code_4() {
    let dir = TempProject::new("fixtures-filter-invalid-fixture");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/xdr/valid.toml",
        &format!(
            "id = \"p28-xdr-valid\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"valid\"\ndescription = \"valid\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    // Malformed TOML file
    dir.write("fixtures/rpc/bad.toml", "invalid toml [[[]");

    // Filtering by surface or category still validates the entire directory first
    let output = run_in(
        &dir.path,
        &["fixtures", "--surface", "xdr", "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(stdout(&output).is_empty());
}
