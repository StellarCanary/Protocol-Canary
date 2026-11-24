//! `fixtures --format json` carries each fixture's CF-04 verification
//! record (issue #444, CA-02-E), and `check`'s terminal report shows a
//! verification badge per result — `[verified: <method>]` for a
//! `matched` record and `[unverified]` when there is none, so the
//! unknown state is shown rather than omitted.

mod support;

use support::{run_in, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use serde_json::Value;

/// The published CA-02 inventory schema, so the unverified-state assertions
/// below are checked against the contract rather than a hand-written shape.
const SCHEMA_TEXT: &str = include_str!("../../../docs/schemas/fixtures-v1.schema.json");

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn schema_type_matches(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.is_u64() || value.is_i64(),
        "number" => value.is_number(),
        _ => false,
    }
}

/// Validates the parts of the fixtures-v1 schema that can reject an
/// inventory: required members, the declared primitive type of every
/// present member, and the fixture `surface` enum. `additionalProperties`
/// is deliberately open in the schema, so unknown members are allowed.
fn assert_conforms_to_fixtures_schema(document: &Value) {
    let schema: Value =
        serde_json::from_str(SCHEMA_TEXT).expect("fixtures-v1 schema is valid JSON");
    let required = schema["required"]
        .as_array()
        .expect("top-level required list");
    for field in required.iter().filter_map(Value::as_str) {
        assert!(
            document.get(field).is_some(),
            "inventory is missing required `{field}`: {document}"
        );
    }

    let top_properties = schema["properties"]
        .as_object()
        .expect("top-level properties");
    for (name, sub_schema) in top_properties {
        if let Some(value) = document.get(name) {
            if let Some(expected) = sub_schema["type"].as_str() {
                assert!(
                    schema_type_matches(value, expected),
                    "`{name}` must be {expected}, found {value}"
                );
            }
            if let Some(constant) = sub_schema.get("const") {
                assert_eq!(value, constant, "`{name}` must be {constant}");
            }
        }
    }

    let item_schema = &schema["properties"]["fixtures"]["items"];
    let item_required = item_schema["required"]
        .as_array()
        .expect("fixture required list");
    let item_properties = item_schema["properties"]
        .as_object()
        .expect("fixture properties");
    let surfaces = item_properties["surface"]["enum"]
        .as_array()
        .expect("surface enum");

    for fixture in document["fixtures"].as_array().expect("fixtures array") {
        for field in item_required.iter().filter_map(Value::as_str) {
            assert!(
                fixture.get(field).is_some(),
                "fixture is missing required `{field}`: {fixture}"
            );
        }
        for (name, sub_schema) in item_properties {
            let Some(value) = fixture.get(name) else {
                continue;
            };
            if let Some(expected) = sub_schema["type"].as_str() {
                assert!(
                    schema_type_matches(value, expected),
                    "fixture `{name}` must be {expected}, found {value}"
                );
            }
            if let Some(allowed) = sub_schema["enum"].as_array() {
                assert!(
                    allowed.contains(value),
                    "fixture `{name}` value {value} is not one of {allowed:?}"
                );
            }
        }
        assert!(
            surfaces.contains(&fixture["surface"]),
            "fixture surface must be one of {surfaces:?}: {fixture}"
        );
    }
}

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

/// CF-04 section 4 makes "no record" the `unverified` state, and CA-02's
/// fixtures-v1 schema encodes it by omitting the `verification` member
/// ("Absent when unverified"). An unverified fixture must therefore be
/// listed with every metadata field and no `verification` member — not a
/// `null` value and not an invented placeholder status — and the document
/// must still validate against the published schema (issue #749).
#[test]
fn unverified_fixtures_omit_the_record_and_pass_schema_validation() {
    let dir = TempProject::new("fixtures-json-unverified-schema");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/xdr/plain.toml", &xdr_fixture("p28-xdr-plain"));
    dir.write("fixtures/xdr/second.toml", &xdr_fixture("p28-xdr-second"));

    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        support::stderr(&output)
    );
    let document: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");

    let fixtures = document["fixtures"].as_array().expect("fixtures array");
    assert_eq!(fixtures.len(), 2);
    for fixture in fixtures {
        assert!(
            fixture.get("verification").is_none(),
            "no record is the unverified state; the member must be absent: {fixture}"
        );
        assert!(
            fixture.get("verificationStatus").is_none(),
            "the state is carried by the record's absence, never a separate status field: {fixture}"
        );
        // The rest of the record is a normal fixture entry, not a stub.
        assert_eq!(fixture["protocol"], 28);
        assert_eq!(fixture["surface"], "xdr");
        assert_eq!(fixture["type"], "StellarValue");
        assert_eq!(fixture["kind"], "decode-success");
        assert!(fixture["path"].as_str().is_some_and(|p| !p.is_empty()));
        assert!(fixture["description"]
            .as_str()
            .is_some_and(|d| !d.is_empty()));
    }

    assert_eq!(document["counts"]["total"], 2);
    assert_conforms_to_fixtures_schema(&document);
}

/// A pack that mixes an attested fixture with unattested ones must keep the
/// two states distinguishable: the verified entry carries its record, the
/// unverified entries carry none, and the whole document still validates.
#[test]
fn mixed_packs_keep_unverified_entries_distinct_and_schema_valid() {
    let dir = project_with_verification("fixtures-json-mixed-verification-schema");
    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        support::stderr(&output)
    );
    let document: Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");

    let fixtures = document["fixtures"].as_array().expect("fixtures array");
    let plain = fixtures
        .iter()
        .find(|fixture| fixture["id"] == "p28-xdr-plain")
        .expect("the unverified fixture is listed");
    assert!(
        plain.get("verification").is_none(),
        "an unattested fixture must not borrow another fixture's record: {plain}"
    );

    let verified = fixtures
        .iter()
        .find(|fixture| fixture["id"] == "p28-xdr-verified")
        .expect("the verified fixture is listed");
    assert_eq!(verified["verification"]["outcome"], "matched");
    assert_eq!(verified["verification"]["fixtureId"], "p28-xdr-verified");

    assert_conforms_to_fixtures_schema(&document);
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
