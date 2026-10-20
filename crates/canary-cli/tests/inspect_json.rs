//! Integration tests for `stellar-canary inspect --format json` (CA-03, issue #453).
//!
//! Asserts that:
//! - `inspect --format json` emits valid machine-readable JSON matching CA-03.
//! - Detected capabilities, workspace members, capability evidence, fingerprint,
//!   and fixture execution plans are correctly included.
//! - `--json` shorthand produces identical output to `--format json`.
//! - Default without `--format` produces terminal format.
//! - Invalid `--format` arguments report argument parsing errors.

mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use serde_json::Value;

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn xdr_fixture(id: &str, protocol: u32) -> String {
    format!(
        "id = \"{id}\"\nprotocol = {protocol}\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

#[test]
fn inspect_format_json_emits_valid_ca03_json() {
    let dir = TempProject::new("inspect-json-basic");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "Cargo.toml",
        "[package]\nname = \"basic-project\"\nversion = \"0.1.0\"\n",
    );
    dir.write("fixtures/xdr/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", 28));
    dir.write(
        "fixtures/xdr/p27-xdr-old.toml",
        &xdr_fixture("p27-xdr-old", 27),
    );

    let output = run_in(
        &dir.path,
        &["inspect", "--format", "json", "--fixtures-dir", "fixtures"],
    );
    assert!(output.status.success(), "stdout: {}", stdout(&output));

    let raw = stdout(&output);
    let normalized = raw.replace("\r\n", "\n");
    assert!(
        normalized.ends_with("}\n"),
        "must end with newline: {normalized}"
    );
    assert!(
        !normalized.ends_with("}\n\n"),
        "must not end with double newline: {normalized}"
    );

    let doc: Value = serde_json::from_str(&raw).expect("valid JSON document");

    // Top-level CA-03 fields
    assert_eq!(doc["inspectVersion"], 1);
    assert_eq!(doc["offline"], true);
    assert!(doc["toolVersion"].is_string());

    // Project fields
    assert_eq!(doc["project"]["root"], ".");
    assert_eq!(doc["project"]["rootSelectedBy"], "current-directory");
    assert!(doc["project"]["type"].is_string());
    assert!(doc["project"]["typeSource"].is_string());
    assert!(doc["project"]["detectedType"].is_string());
    assert!(doc["project"]["capabilities"].is_array());
    assert!(doc["project"]["workspace"]["members"].is_array());
    assert_eq!(doc["project"]["detection"]["evidenceVersion"], 1);

    // Config fields
    assert_eq!(doc["config"]["found"], true);
    assert_eq!(doc["config"]["path"], ".stellar-canary.toml");
    assert_eq!(doc["config"]["protocol"], 28);
    assert_eq!(doc["config"]["tests"]["xdr"], true);
    assert_eq!(doc["config"]["tests"]["rpc"], false);
    assert_eq!(doc["config"]["tests"]["soroban"], false);

    // Target fields
    assert_eq!(doc["target"]["protocol"], 28);
    assert_eq!(doc["target"]["source"], "config");

    // Fixtures fields
    assert_eq!(doc["fixtures"]["directoryFound"], true);
    assert_eq!(doc["fixtures"]["loaded"], 2);

    // Plan fields
    assert_eq!(doc["plan"]["checkWouldRefuse"], false);
    assert_eq!(doc["plan"]["refusal"], Value::Null);
    let would_run = doc["plan"]["wouldRun"].as_array().expect("wouldRun array");
    assert_eq!(would_run.len(), 1);
    assert_eq!(would_run[0]["id"], "p28-xdr-1");
    assert_eq!(would_run[0]["surface"], "xdr");

    let skipped = doc["plan"]["skipped"].as_array().expect("skipped array");
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["id"], "p27-xdr-old");
    assert_eq!(skipped[0]["surface"], "xdr");
    assert_eq!(skipped[0]["code"], "UNSUPPORTED_PROTOCOL");

    // Workspace fields
    assert!(doc["workspace"]["members"].is_array());

    // Fingerprint fields
    assert_eq!(doc["fingerprint"]["fingerprintVersion"], 1);
    assert_eq!(doc["fingerprint"]["detector"], 1);
    let digest = doc["fingerprint"]["digest"]
        .as_str()
        .expect("digest string");
    assert!(digest.starts_with("sha256:"), "digest: {digest}");
    assert_eq!(digest.len(), 7 + 64);
    assert!(doc["fingerprint"]["inputs"].as_u64().unwrap() >= 1);
}

#[test]
fn inspect_format_json_detects_capabilities_and_evidence() {
    let dir = TempProject::new("inspect-json-evidence");
    dir.write(
        "Cargo.toml",
        "[package]\nname = \"soroban-example\"\nversion = \"0.1.0\"\n\n[dependencies]\nsoroban-sdk = \"22\"\nstellar-rpc-client = \"0.1\"\n",
    );

    let output = run_in(&dir.path, &["inspect", "--format", "json"]);
    assert!(output.status.success(), "stdout: {}", stdout(&output));

    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");

    let capabilities = doc["project"]["capabilities"]
        .as_array()
        .expect("capabilities array");
    let cap_strings: Vec<&str> = capabilities.iter().filter_map(|c| c.as_str()).collect();
    assert!(cap_strings.contains(&"soroban-contract"), "{cap_strings:?}");
    assert!(cap_strings.contains(&"rpc-client"), "{cap_strings:?}");

    let detection = &doc["project"]["detection"];
    assert_eq!(detection["evidenceVersion"], 1);
    assert_eq!(detection["root"], ".");

    let inspected = detection["inspected"].as_array().expect("inspected array");
    assert!(inspected.iter().any(|p| p == "Cargo.toml"));

    let cap_evidence = detection["capabilities"]
        .as_array()
        .expect("detection.capabilities array");

    let soroban_ev = cap_evidence
        .iter()
        .find(|entry| entry["capability"] == "soroban-contract")
        .expect("soroban capability evidence");
    let items = soroban_ev["evidence"].as_array().expect("evidence items");
    assert!(!items.is_empty());
    assert_eq!(items[0]["manifest"], "Cargo.toml");
    assert_eq!(items[0]["ecosystem"], "cargo");
    assert_eq!(items[0]["dependency"], "soroban-sdk");
    assert_eq!(items[0]["section"], "dependencies");

    let rpc_ev = cap_evidence
        .iter()
        .find(|entry| entry["capability"] == "rpc-client")
        .expect("rpc-client capability evidence");
    let rpc_items = rpc_ev["evidence"].as_array().expect("evidence items");
    assert!(!rpc_items.is_empty());
    assert_eq!(rpc_items[0]["manifest"], "Cargo.toml");
    assert_eq!(rpc_items[0]["ecosystem"], "cargo");
    assert_eq!(rpc_items[0]["dependency"], "stellar-rpc-client");
}

#[test]
fn inspect_format_json_detects_workspace_members_and_member_evidence() {
    let dir = TempProject::new("inspect-json-ws");
    dir.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"contracts/token\", \"contracts/governance\"]\n",
    );
    dir.write(
        "contracts/token/Cargo.toml",
        "[package]\nname = \"token\"\nversion = \"0.1.0\"\n\n[dependencies]\nsoroban-sdk = \"22\"\n",
    );
    dir.write(
        "contracts/governance/Cargo.toml",
        "[package]\nname = \"governance\"\nversion = \"0.1.0\"\n\n[dependencies]\nsoroban-sdk = \"22\"\n",
    );

    let output = run_in(&dir.path, &["inspect", "--format", "json"]);
    assert!(output.status.success(), "stdout: {}", stdout(&output));

    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");

    let members = doc["workspace"]["members"]
        .as_array()
        .expect("workspace.members array");
    let member_strings: Vec<&str> = members.iter().filter_map(|m| m.as_str()).collect();
    assert_eq!(
        member_strings,
        vec!["contracts/governance", "contracts/token"]
    );

    let proj_members = doc["project"]["workspace"]["members"]
        .as_array()
        .expect("project.workspace.members array");
    let proj_member_strings: Vec<&str> = proj_members.iter().filter_map(|m| m.as_str()).collect();
    assert_eq!(proj_member_strings, member_strings);

    let inspected = doc["project"]["detection"]["inspected"]
        .as_array()
        .expect("inspected array");
    assert!(inspected.iter().any(|p| p == "Cargo.toml"));
    assert!(inspected.iter().any(|p| p == "contracts/token/Cargo.toml"));
    assert!(inspected
        .iter()
        .any(|p| p == "contracts/governance/Cargo.toml"));

    // Soroban contract capability is detected from member evidence
    let capabilities = doc["project"]["capabilities"].as_array().unwrap();
    assert!(capabilities.iter().any(|c| c == "soroban-contract"));
}

#[test]
fn the_json_shorthand_produces_identical_output_to_format_flag() {
    let dir = TempProject::new("inspect-json-shorthand");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    let flagged = run_in(&dir.path, &["inspect", "--format", "json"]);
    let shorthand = run_in(&dir.path, &["inspect", "--json"]);

    assert!(flagged.status.success());
    assert!(shorthand.status.success());
    assert_eq!(stdout(&flagged), stdout(&shorthand));
}

#[test]
fn inspect_without_format_defaults_to_terminal_output() {
    let dir = TempProject::new("inspect-default-terminal");
    let output = run_in(&dir.path, &["inspect"]);
    assert!(output.status.success());

    let text = stdout(&output);
    assert!(text.contains("Project root:"));
    assert!(text.contains("Project type:"));
    assert!(!text.trim_start().starts_with('{'));
}

#[test]
fn inspect_rejects_invalid_format_argument() {
    let dir = TempProject::new("inspect-invalid-format");
    let output = run_in(&dir.path, &["inspect", "--format", "invalid"]);

    assert!(!output.status.success(), "must fail for invalid format");
    assert_eq!(output.status.code(), Some(2));

    let err = stderr(&output);
    assert!(
        err.contains("invalid value 'invalid' for '--format <FORMAT>'")
            || err.contains("invalid value"),
        "stderr: {err}"
    );
}

#[test]
fn inspect_reports_an_empty_plan_that_check_would_refuse() {
    let dir = TempProject::new("inspect-empty-plan");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    let output = run_in(&dir.path, &["inspect", "--format", "json"]);
    assert!(output.status.success());

    let doc: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(doc["plan"]["checkWouldRefuse"], true);
    assert!(doc["plan"]["wouldRun"].as_array().unwrap().is_empty());
    assert!(
        doc["plan"]["refusal"]
            .as_str()
            .expect("refusal text")
            .contains("no checks ran"),
        "refusal: {}",
        doc["plan"]["refusal"]
    );
}
