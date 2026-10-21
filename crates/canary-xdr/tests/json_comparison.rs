//! `decode-json-equals`: comparing decoded XDR to expected JSON
//! (`[E-032]`, issue #509).
//!
//! Values are real XDR, encoded with `stellar-xdr` and run through the
//! public fixture path ([`XdrFixture::from_loaded`] and
//! [`DefaultXdrRunner`]), so these tests pin the JSON form fixture authors
//! write against, not just the comparison helper.

use std::path::Path;

use canary_core::{
    CacheStore, CompatibilityResult, ExecutionContext, FixtureStore, GitContext, NetworkContext,
    NetworkName, ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_xdr::{
    compare_json, decode, DefaultXdrRunner, JsonDifferenceKind, XdrAssertion, XdrError, XdrFixture,
    XdrRunner, XdrTypeName,
};
use serde_json::json;
use stellar_xdr::{
    Limits, Memo, ScVal, StellarValue, Transaction, TransactionEnvelope, TransactionV1Envelope,
    WriteXdr,
};

fn context() -> ExecutionContext {
    ExecutionContext {
        protocol: ProtocolVersion(28),
        project: ProjectContext {
            root: ".".into(),
            name: "test".into(),
            project_type: ProjectType::Unknown,
            capabilities: vec![],
        },
        network: NetworkContext {
            name: NetworkName::Testnet,
            rpc_url: "https://example.invalid".into(),
            passphrase: "Test SDF Network ; September 2015".into(),
            observed_protocol: None,
        },
        fixtures: FixtureStore::default(),
        git: GitContext::default(),
        cache: CacheStore::new(std::env::temp_dir().join("canary-xdr-json-comparison-cache")),
        options: RunOptions::default(),
    }
}

fn fixture(body: &str) -> Result<XdrFixture, XdrError> {
    let text = format!(
        "id = \"p28-xdr-json\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"json comparison\"\n{body}"
    );
    XdrFixture::from_loaded(&parse_fixture_str(&text, Path::new("xdr/json.toml")).unwrap())
}

/// A `decode-json-equals` fixture body for `type_name`.
fn json_fixture(type_name: &str, value_base64: &str, expected_json: &str) -> XdrFixture {
    fixture(&format!(
        "type = \"{type_name}\"\nkind = \"decode-json-equals\"\nvalue_base64 = \"{value_base64}\"\nexpected_json = '''\n{expected_json}\n'''\n"
    ))
    .expect("fixture parses")
}

fn run(fixture: &XdrFixture) -> CompatibilityResult {
    DefaultXdrRunner.run(fixture, &context()).unwrap()
}

fn details(result: &CompatibilityResult) -> &str {
    result
        .details
        .as_deref()
        .expect("a failure explains itself")
}

const ZERO_ACCOUNT: &str = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";

/// A V1 envelope with a fee and a text memo, so nested fields have values
/// worth asserting on.
fn envelope_base64() -> String {
    TransactionEnvelope::Tx(TransactionV1Envelope {
        tx: Transaction {
            fee: 100,
            memo: Memo::Text("canary".try_into().unwrap()),
            ..Transaction::default()
        },
        signatures: Default::default(),
    })
    .to_xdr_base64(Limits::none())
    .unwrap()
}

fn envelope_json() -> serde_json::Value {
    json!({
        "tx": {
            "tx": {
                "source_account": ZERO_ACCOUNT,
                "fee": 100,
                "seq_num": "0",
                "cond": "none",
                "memo": { "text": "canary" },
                "operations": [],
                "ext": "v0"
            },
            "signatures": []
        }
    })
}

fn scval_vec_base64(items: &[u32]) -> String {
    let items: Vec<ScVal> = items.iter().map(|n| ScVal::U32(*n)).collect();
    ScVal::Vec(Some(items.try_into().unwrap()))
        .to_xdr_base64(Limits::none())
        .unwrap()
}

// --- JSON form of decoded values ---------------------------------------------

#[test]
fn decoded_values_use_the_stellar_xdr_json_form() {
    let base64 = ScVal::U64(u64::MAX).to_xdr_base64(Limits::none()).unwrap();
    let decoded = decode(XdrTypeName::ScVal, &base64).unwrap();
    // 64-bit integers are strings, so no JSON reader loses precision.
    assert_eq!(
        decoded.to_json().unwrap(),
        json!({ "u64": "18446744073709551615" })
    );

    let decoded = decode(XdrTypeName::TransactionEnvelope, &envelope_base64()).unwrap();
    assert_eq!(decoded.to_json().unwrap(), envelope_json());
}

#[test]
fn json_form_is_deterministic() {
    let decoded = decode(XdrTypeName::TransactionEnvelope, &envelope_base64()).unwrap();
    let first = serde_json::to_string(&decoded.to_json().unwrap()).unwrap();
    let second = serde_json::to_string(&decoded.to_json().unwrap()).unwrap();
    assert_eq!(first, second);
}

// --- passing comparisons -----------------------------------------------------

#[test]
fn matching_json_passes() {
    let fixture = json_fixture(
        "TransactionEnvelope",
        &envelope_base64(),
        &envelope_json().to_string(),
    );
    let result = run(&fixture);
    assert_eq!(result.status, Status::Pass, "{:?}", result.details);
    assert_eq!(
        result.summary,
        "decoded TransactionEnvelope matched the expected JSON"
    );
}

#[test]
fn object_key_order_and_whitespace_do_not_matter() {
    let base64 = StellarValue::default()
        .to_xdr_base64(Limits::none())
        .unwrap();
    // Keys reversed relative to the XDR definition, spread over lines.
    let expected = r#"{
        "ext": "basic",
        "upgrades": [],
        "close_time": "0",
        "tx_set_hash": "0000000000000000000000000000000000000000000000000000000000000000"
    }"#;
    let result = run(&json_fixture("StellarValue", &base64, expected));
    assert_eq!(result.status, Status::Pass, "{:?}", result.details);
}

// --- mismatches --------------------------------------------------------------

#[test]
fn nested_value_mismatch_names_the_path_and_both_values() {
    let mut expected = envelope_json();
    expected["tx"]["tx"]["fee"] = json!(200);
    let result = run(&json_fixture(
        "TransactionEnvelope",
        &envelope_base64(),
        &expected.to_string(),
    ));

    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        result.summary,
        "decoded TransactionEnvelope did not match the expected JSON (1 difference(s))"
    );
    assert_eq!(details(&result), "/tx/tx/fee: expected 200, found 100");
}

#[test]
fn nested_string_mismatch_inside_a_union_arm() {
    let mut expected = envelope_json();
    expected["tx"]["tx"]["memo"]["text"] = json!("sparrow");
    let result = run(&json_fixture(
        "TransactionEnvelope",
        &envelope_base64(),
        &expected.to_string(),
    ));
    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        details(&result),
        "/tx/tx/memo/text: expected \"sparrow\", found \"canary\""
    );
}

#[test]
fn a_field_the_value_lacks_is_reported_as_missing() {
    let mut expected = envelope_json();
    expected["tx"]["tx"]["time_bounds"] = json!(null);
    let result = run(&json_fixture(
        "TransactionEnvelope",
        &envelope_base64(),
        &expected.to_string(),
    ));
    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        details(&result),
        "/tx/tx/time_bounds: missing field (expected null)"
    );
}

#[test]
fn a_field_the_expectation_lacks_is_reported_as_unexpected() {
    // Exact comparison: leaving a field out of the expectation is not a
    // wildcard.
    let mut expected = envelope_json();
    expected["tx"]["tx"]
        .as_object_mut()
        .unwrap()
        .remove("seq_num");
    let result = run(&json_fixture(
        "TransactionEnvelope",
        &envelope_base64(),
        &expected.to_string(),
    ));
    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        details(&result),
        "/tx/tx/seq_num: unexpected field (found \"0\")"
    );
}

#[test]
fn missing_and_unexpected_fields_are_distinguishable() {
    let differences = compare_json(&json!({ "a": 1 }), &json!({ "b": 1 }));
    assert_eq!(differences.len(), 2);
    assert_eq!(differences[0].path, "/a");
    assert!(matches!(
        differences[0].kind,
        JsonDifferenceKind::MissingField { .. }
    ));
    assert_eq!(differences[1].path, "/b");
    assert!(matches!(
        differences[1].kind,
        JsonDifferenceKind::UnexpectedField { .. }
    ));
}

#[test]
fn array_element_mismatch_names_the_index() {
    let result = run(&json_fixture(
        "ScVal",
        &scval_vec_base64(&[1, 2, 3]),
        r#"{ "vec": [{ "u32": 1 }, { "u32": 9 }, { "u32": 3 }] }"#,
    ));
    assert_eq!(result.status, Status::Fail);
    assert_eq!(details(&result), "/vec/1/u32: expected 9, found 2");
}

#[test]
fn array_length_mismatch_reports_the_lengths_and_the_surplus_elements() {
    let result = run(&json_fixture(
        "ScVal",
        &scval_vec_base64(&[1, 2, 3]),
        r#"{ "vec": [{ "u32": 1 }, { "u32": 2 }] }"#,
    ));
    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        details(&result),
        "/vec: expected an array of 2 element(s), found 3\n/vec/2: unexpected element (found {\"u32\":3})"
    );

    let result = run(&json_fixture(
        "ScVal",
        &scval_vec_base64(&[1]),
        r#"{ "vec": [{ "u32": 1 }, { "u32": 2 }] }"#,
    ));
    assert_eq!(
        details(&result),
        "/vec: expected an array of 2 element(s), found 1\n/vec/1: missing element (expected {\"u32\":2})"
    );
}

#[test]
fn array_order_matters() {
    let result = run(&json_fixture(
        "ScVal",
        &scval_vec_base64(&[1, 2]),
        r#"{ "vec": [{ "u32": 2 }, { "u32": 1 }] }"#,
    ));
    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        details(&result),
        "/vec/0/u32: expected 2, found 1\n/vec/1/u32: expected 1, found 2"
    );
}

#[test]
fn json_type_mismatch_names_both_types() {
    // `seq_num` is an i64, written as a string; a number is not accepted
    // in its place.
    let mut expected = envelope_json();
    expected["tx"]["tx"]["seq_num"] = json!(0);
    let result = run(&json_fixture(
        "TransactionEnvelope",
        &envelope_base64(),
        &expected.to_string(),
    ));
    assert_eq!(result.status, Status::Fail);
    assert_eq!(
        details(&result),
        "/tx/tx/seq_num: expected number 0, found string \"0\""
    );
}

#[test]
fn a_union_arm_mismatch_is_a_type_mismatch_at_the_union() {
    // `memo = "none"` (a bare string arm) against a `{ "text": ... }` arm.
    let mut expected = envelope_json();
    expected["tx"]["tx"]["memo"] = json!("none");
    let result = run(&json_fixture(
        "TransactionEnvelope",
        &envelope_base64(),
        &expected.to_string(),
    ));
    assert_eq!(
        details(&result),
        "/tx/tx/memo: expected string \"none\", found object {\"text\":\"canary\"}"
    );
}

#[test]
fn every_difference_is_listed_in_path_order() {
    let mut expected = envelope_json();
    expected["tx"]["tx"]["fee"] = json!(1);
    expected["tx"]["tx"]["ext"] = json!("v1");
    expected["tx"]["signatures"] = json!([{}]);
    let result = run(&json_fixture(
        "TransactionEnvelope",
        &envelope_base64(),
        &expected.to_string(),
    ));
    assert!(
        result.summary.ends_with("(4 difference(s))"),
        "{}",
        result.summary
    );
    let paths: Vec<&str> = details(&result)
        .lines()
        .map(|line| line.split(':').next().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "/tx/signatures",
            "/tx/signatures/0",
            "/tx/tx/ext",
            "/tx/tx/fee"
        ]
    );
}

// --- decode failures ---------------------------------------------------------

#[test]
fn input_that_does_not_decode_fails_without_comparing() {
    let result = run(&json_fixture("ScVal", "!!!not-xdr!!!", "{}"));
    assert_eq!(result.status, Status::Fail);
    assert_eq!(result.summary, "failed to decode ScVal");
}

#[test]
fn input_of_another_type_fails_to_decode() {
    // A StellarValue's bytes are not a TransactionEnvelope.
    let base64 = StellarValue::default()
        .to_xdr_base64(Limits::none())
        .unwrap();
    let result = run(&json_fixture("TransactionEnvelope", &base64, "{}"));
    assert_eq!(result.status, Status::Fail);
    assert_eq!(result.summary, "failed to decode TransactionEnvelope");
}

// --- fixture validation ------------------------------------------------------

fn invalid_reason(body: &str) -> String {
    match fixture(body).expect_err("fixture should be rejected") {
        XdrError::InvalidFixtureBody {
            source_path,
            reason,
        } => {
            assert_eq!(source_path, Path::new("xdr/json.toml"));
            reason
        }
    }
}

#[test]
fn malformed_expected_json_is_an_invalid_fixture_with_its_position() {
    let reason = invalid_reason(
        "type = \"ScVal\"\nkind = \"decode-json-equals\"\nvalue_base64 = \"AAAAAQ==\"\nexpected_json = '''\n{ \"u32\": 1,\n}\n'''\n",
    );
    assert!(
        reason.contains("\"expected_json\" is not valid JSON"),
        "{reason}"
    );
    // TOML drops the newline after the opening `'''`, so the dangling
    // comma's closing brace is on line 2 of the JSON text.
    assert!(reason.contains("trailing comma at line 2"), "{reason}");
    assert!(reason.contains("expected_json text"), "{reason}");
}

#[test]
fn expected_json_must_be_json_text_not_a_toml_table() {
    let reason = invalid_reason(
        "type = \"ScVal\"\nkind = \"decode-json-equals\"\nvalue_base64 = \"AAAAAQ==\"\n\n[expected_json]\nu32 = 1\n",
    );
    assert!(
        reason.contains("missing required string field \"expected_json\""),
        "{reason}"
    );
}

#[test]
fn decode_json_equals_requires_expected_json() {
    let reason = invalid_reason(
        "type = \"ScVal\"\nkind = \"decode-json-equals\"\nvalue_base64 = \"AAAAAQ==\"\n",
    );
    assert!(reason.contains("\"expected_json\""), "{reason}");
}

#[test]
fn expected_json_is_rejected_on_other_kinds() {
    let reason = invalid_reason(
        "type = \"ScVal\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAAAQ==\"\nexpected_json = '\"void\"'\n",
    );
    assert!(
        reason.contains("unknown key(s) \"expected_json\""),
        "{reason}"
    );
}

#[test]
fn the_unsupported_kind_message_lists_the_new_kind() {
    let reason =
        invalid_reason("type = \"ScVal\"\nkind = \"decode-json\"\nvalue_base64 = \"AAAAAQ==\"\n");
    assert!(reason.contains("\"decode-json-equals\""), "{reason}");
}

#[test]
fn a_parsed_fixture_carries_the_expected_json_value() {
    let fixture = json_fixture("ScVal", "AAAAAQ==", "\"void\"");
    assert_eq!(
        fixture.assertion,
        XdrAssertion::DecodeJsonEquals {
            value_base64: "AAAAAQ==".to_string(),
            expected_json: json!("void"),
        }
    );
    assert_eq!(run(&fixture).status, Status::Pass);
}

// --- backward compatibility --------------------------------------------------

#[test]
fn fixtures_without_expected_json_are_unchanged() {
    let base64 = StellarValue::default()
        .to_xdr_base64(Limits::none())
        .unwrap();
    for kind in ["decode-success", "roundtrip"] {
        let fixture = fixture(&format!(
            "type = \"StellarValue\"\nkind = \"{kind}\"\nvalue_base64 = \"{base64}\"\n"
        ))
        .unwrap();
        assert_eq!(run(&fixture).status, Status::Pass, "{kind}");
    }
    let fixture = fixture(&format!(
        "type = \"StellarValue\"\nkind = \"encode-equals\"\nvalue_base64 = \"{base64}\"\nexpected_base64 = \"{base64}\"\n"
    ))
    .unwrap();
    assert_eq!(run(&fixture).status, Status::Pass);
}
