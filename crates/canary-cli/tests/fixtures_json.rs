//! `fixtures --format json` carries each fixture's CF-04 verification
//! record (issue #444, CA-02-E), and `check`'s terminal report shows a
//! verification badge per result — `[verified: <method>]` for a
//! `matched` record and `[unverified]` when there is none, so the
//! unknown state is shown rather than omitted.

mod support;

use support::{run_in, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use serde_json::Value;

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn xdr_fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

/// A `[verification]` table matching the fixture it is appended to.
fn verification_table(fixture_id: &str) -> String {
    format!(
        "\n[verification]\nverificationVersion = 1\nfixtureId = \"{fixture_id}\"\nmethod = \"source-checked\"\ncheckedAt = \"2026-09-28T07:20:00Z\"\noutcome = \"matched\"\n"
    )
}

fn project_with_verification(name: &str) -> TempProject {
    let dir = TempProject::new(name);
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/xdr/verified.toml",
        &format!(
            "{}{}",
            xdr_fixture("p28-xdr-verified"),
            verification_table("p28-xdr-verified")
        ),
    );
    dir.write("fixtures/xdr/plain.toml", &xdr_fixture("p28-xdr-plain"));
    // A fixture for another protocol, to exercise `otherProtocols`.
    dir.write(
        "fixtures/xdr/old.toml",
        &format!(
            "id = \"p27-xdr-old\"\nprotocol = 27\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir
}

#[test]
fn fixtures_format_json_lists_fixtures_with_verification_metadata() {
    let dir = project_with_verification("fixtures-json-verification");
    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert!(output.status.success(), "{}", stdout(&output));

    let document: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(document["inventoryVersion"], 1);
    assert_eq!(document["protocol"], 28);
    assert_eq!(document["directoryFound"], true);
    assert_eq!(document["counts"]["total"], 2);
    assert_eq!(document["counts"]["xdr"], 2);
    assert_eq!(document["otherProtocols"]["27"], 1);

    // Sorted by id: plain before verified.
    let fixtures = document["fixtures"].as_array().expect("fixtures array");
    assert_eq!(fixtures.len(), 2);
    assert_eq!(fixtures[0]["id"], "p28-xdr-plain");
    assert_eq!(fixtures[0]["path"], "xdr/plain.toml");
    assert_eq!(fixtures[0]["type"], "StellarValue");
    assert_eq!(fixtures[0]["kind"], "decode-success");
    assert!(
        fixtures[0].get("verification").is_none(),
        "no record means no field: {}",
        fixtures[0]
    );

    let verified = &fixtures[1];
    assert_eq!(verified["id"], "p28-xdr-verified");
    let verification = &verified["verification"];
    assert_eq!(verification["verificationVersion"], 1);
    assert_eq!(verification["fixtureId"], "p28-xdr-verified");
    assert_eq!(verification["method"], "source-checked");
    assert_eq!(verification["checkedAt"], "2026-09-28T07:20:00Z");
    assert_eq!(verification["outcome"], "matched");
}

#[test]
fn the_json_shorthand_selects_the_same_inventory_as_the_format_flag() {
    let dir = project_with_verification("fixtures-json-shorthand");
    let flagged = run_in(&dir.path, &["fixtures", "--format", "json"]);
    let shorthand = run_in(&dir.path, &["fixtures", "--json"]);
    assert!(flagged.status.success() && shorthand.status.success());
    assert_eq!(stdout(&flagged), stdout(&shorthand));
}

#[test]
fn the_terminal_format_is_still_the_default() {
    let dir = project_with_verification("fixtures-terminal-default");
    let defaulted = run_in(&dir.path, &["fixtures"]);
    let explicit = run_in(&dir.path, &["fixtures", "--format", "terminal"]);
    assert!(defaulted.status.success() && explicit.status.success());
    assert_eq!(stdout(&defaulted), stdout(&explicit));
    assert!(
        stdout(&defaulted).contains("p28-xdr-verified"),
        "{}",
        stdout(&defaulted)
    );
}

#[test]
fn check_terminal_output_shows_verification_badges_per_result() {
    let dir = project_with_verification("check-verification-badges");
    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));

    let text = stdout(&output);
    assert!(
        text.contains("p28-xdr-verified") && text.contains("[verified: source-checked]"),
        "{text}"
    );
    assert!(
        text.contains("p28-xdr-plain") && text.contains("[unverified]"),
        "an unverified fixture shows the unknown state rather than silence: {text}"
    );
}

#[test]
fn check_json_output_is_unaffected_by_verification() {
    // The JSON report schema is versioned separately; verification is a
    // fixture property, not a run result (CF-04 section 2).
    let dir = project_with_verification("check-json-verification");
    let output = run_in(&dir.path, &["check", "--json"]);
    assert!(output.status.success());
    let report: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(report["schemaVersion"], 1);
    let results = report["results"].as_array().expect("results");
    assert_eq!(results.len(), 2);
    assert!(
        results.iter().all(|r| r.get("verification").is_none()),
        "the run report does not grow a verification field: {results:?}"
    );
}

#[test]
fn an_invalid_verification_table_is_an_invalid_fixture() {
    let dir = TempProject::new("invalid-verification-table");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    // The fixtureId does not match the fixture: a record can never claim
    // to verify a different fixture.
    dir.write(
        "fixtures/xdr/bad.toml",
        &format!(
            "{}{}",
            xdr_fixture("p28-xdr-bad"),
            verification_table("p28-xdr-someone-else")
        ),
    );

    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert_eq!(output.status.code(), Some(4));
    let err = support::stderr(&output);
    assert!(err.contains("[verification]"), "{err}");
    assert!(err.contains("fixtureId does not match"), "{err}");
}

#[test]
fn a_fixture_without_verification_still_lists_normally() {
    // The shipped pack carries no records yet; the command must not
    // invent any field for it.
    let dir = TempProject::new("fixtures-json-unverified-pack");
    dir.write("fixtures/xdr/plain.toml", &xdr_fixture("p28-xdr-plain"));
    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert!(output.status.success());
    let document: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(document["fixtures"][0].get("verification"), None);
    assert_eq!(document["fixtures"].as_array().unwrap().len(), 1);
}

#[test]
fn fixtures_rejects_unsupported_format() {
    let dir = TempProject::new("fixtures-unsupported-format");
    let output = run_in(&dir.path, &["fixtures", "--format", "xml"]);

    assert!(!output.status.success(), "must fail for unsupported format");
    assert_eq!(output.status.code(), Some(2));
    assert!(support::stdout(&output).is_empty());

    let err = support::stderr(&output);
    assert!(
        err.contains("invalid value 'xml' for '--format <FORMAT>'")
            || err.contains("invalid value"),
        "stderr: {err}"
    );
    assert!(
        err.contains("terminal") && err.contains("json"),
        "stderr should list allowed formats: {err}"
    );
}

#[test]
fn fixtures_format_json_conforms_to_ca02_schema_and_deterministic_ordering() {
    let dir = TempProject::new("fixtures-ca02-conformance");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    dir.write(
        "fixtures/soroban/contract.toml",
        r#"
id = "z-soroban-token"
protocol = 28
surface = "soroban"
category = "token"
description = "token contract fixture"
required_capabilities = ["wasm-artifact", "soroban-contract"]
function = "balance"
"#,
    );
    dir.write(
        "fixtures/rpc/network.toml",
        r#"
id = "a-rpc-network"
protocol = 28
surface = "rpc"
category = "network"
description = "network fixture"
source_reference = "https://example.invalid/ref"
required_capabilities = ["rpc-client"]
method = "get-network"
"#,
    );
    dir.write(
        "fixtures/xdr/value.toml",
        &format!(
            "id = \"m-xdr-value\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"ledger value fixture\"\nrequired_capabilities = [\"stellar-sdk-dependency\"]\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir.write(
        "fixtures/xdr/old.toml",
        &format!(
            "id = \"p27-old\"\nprotocol = 27\nsurface = \"xdr\"\ncategory = \"legacy\"\ndescription = \"legacy fixture\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );

    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        support::stderr(&output)
    );
    assert!(support::stderr(&output).is_empty());

    let raw = stdout(&output);
    assert!(raw.ends_with("}\n"), "must end with single LF newline");
    assert!(
        !raw.ends_with("}\n\n"),
        "must not have double trailing newline"
    );
    assert!(
        raw.contains("\n  \"inventoryVersion\": 1"),
        "2-space indentation"
    );

    let document: Value = serde_json::from_str(&raw).expect("valid JSON");
    assert_eq!(document["inventoryVersion"], 1);
    assert_eq!(document["toolVersion"], env!("CARGO_PKG_VERSION"));
    assert_eq!(document["fixtureFormat"], 1);
    assert_eq!(document["protocol"], 28);
    assert_eq!(document["directoryFound"], true);
    assert_eq!(document["counts"]["total"], 3);
    assert_eq!(document["counts"]["xdr"], 1);
    assert_eq!(document["counts"]["rpc"], 1);
    assert_eq!(document["counts"]["soroban"], 1);
    assert_eq!(document["otherProtocols"]["27"], 1);

    let fixtures = document["fixtures"].as_array().expect("fixtures array");
    assert_eq!(fixtures.len(), 3);

    // Sorted deterministically by id (UTF-8 bytes): a-rpc-network, m-xdr-value, z-soroban-token
    let ids: Vec<&str> = fixtures.iter().map(|f| f["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["a-rpc-network", "m-xdr-value", "z-soroban-token"]);

    // Verify first fixture: RPC
    let rpc = &fixtures[0];
    assert_eq!(rpc["id"], "a-rpc-network");
    assert_eq!(rpc["protocol"], 28);
    assert_eq!(rpc["surface"], "rpc");
    assert_eq!(rpc["category"], "network");
    assert_eq!(rpc["description"], "network fixture");
    assert_eq!(rpc["path"], "rpc/network.toml");
    assert_eq!(
        rpc["requiredCapabilities"],
        serde_json::json!(["rpc-client"])
    );
    assert_eq!(rpc["sourceReference"], "https://example.invalid/ref");
    assert_eq!(rpc["method"], "get-network");
    assert!(rpc.get("type").is_none());
    assert!(rpc.get("kind").is_none());
    assert!(rpc.get("function").is_none());

    // Verify second fixture: XDR
    let xdr = &fixtures[1];
    assert_eq!(xdr["id"], "m-xdr-value");
    assert_eq!(xdr["protocol"], 28);
    assert_eq!(xdr["surface"], "xdr");
    assert_eq!(xdr["category"], "ledger");
    assert_eq!(xdr["description"], "ledger value fixture");
    assert_eq!(xdr["path"], "xdr/value.toml");
    assert_eq!(
        xdr["requiredCapabilities"],
        serde_json::json!(["stellar-sdk-dependency"])
    );
    assert_eq!(xdr["type"], "StellarValue");
    assert_eq!(xdr["kind"], "decode-success");
    assert!(xdr.get("sourceReference").is_none());
    assert!(xdr.get("method").is_none());
    assert!(xdr.get("function").is_none());

    // Verify third fixture: Soroban
    let soroban = &fixtures[2];
    assert_eq!(soroban["id"], "z-soroban-token");
    assert_eq!(soroban["protocol"], 28);
    assert_eq!(soroban["surface"], "soroban");
    assert_eq!(soroban["category"], "token");
    assert_eq!(soroban["description"], "token contract fixture");
    assert_eq!(soroban["path"], "soroban/contract.toml");
    // Sorted, lowercase kebab
    assert_eq!(
        soroban["requiredCapabilities"],
        serde_json::json!(["soroban-contract", "wasm-artifact"])
    );
    assert_eq!(soroban["function"], "balance");
    assert!(soroban.get("sourceReference").is_none());
    assert!(soroban.get("method").is_none());
    assert!(soroban.get("type").is_none());
    assert!(soroban.get("kind").is_none());
}

#[test]
fn fixtures_format_json_with_missing_directory_reports_empty_inventory() {
    let dir = TempProject::new("fixtures-missing-dir");
    let output = run_in(
        &dir.path,
        &[
            "fixtures",
            "--format",
            "json",
            "--fixtures-dir",
            "does-not-exist",
        ],
    );
    assert!(output.status.success());
    let document: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(document["directoryFound"], false);
    assert_eq!(document["counts"]["total"], 0);
    assert_eq!(document["fixtures"], serde_json::json!([]));
    assert_eq!(document["otherProtocols"], serde_json::json!({}));
}
