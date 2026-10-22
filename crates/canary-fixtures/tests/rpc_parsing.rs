//! Diagnostics for malformed RPC fixtures (`[E-108]`, issue #502).
//!
//! Each test feeds a small inline fixture through the path a real fixture
//! file takes: [`parse_fixture_str`] for the TOML and common metadata, then
//! [`RpcFixture::from_loaded`] for the RPC body. Both are synchronous and
//! take no client, so every error here is raised before any request could
//! be sent; nothing in this file needs a network.
//!
//! The tests match the error variant and check that the message names the
//! offending key or value, rather than comparing whole messages.

use std::path::Path;

use canary_fixtures::{parse_fixture_str, FixtureError, LoadedFixture};
use canary_rpc::{FieldAssertion, RpcFixture, RpcFixtureError, RpcMethod};

const SOURCE: &str = "rpc/p28-rpc-bad.toml";

/// The common metadata every fixture needs, followed by `body`.
fn fixture_text(body: &str) -> String {
    format!(
        "id = \"p28-rpc-bad\"\n\
         protocol = 28\n\
         surface = \"rpc\"\n\
         category = \"network\"\n\
         description = \"malformed rpc fixture\"\n\
         {body}"
    )
}

fn load(body: &str) -> LoadedFixture {
    parse_fixture_str(&fixture_text(body), Path::new(SOURCE)).expect("metadata and TOML are valid")
}

fn parse_rpc(body: &str) -> Result<RpcFixture, RpcFixtureError> {
    RpcFixture::from_loaded(&load(body))
}

/// Parses `body`, asserts it is rejected as an invalid RPC fixture body for
/// [`SOURCE`], and returns the reason.
fn invalid_reason(body: &str) -> String {
    let err = parse_rpc(body).expect_err("fixture should be rejected");
    let message = err.to_string();
    let RpcFixtureError::InvalidFixtureBody {
        source_path,
        reason,
    } = err;
    assert_eq!(source_path, Path::new(SOURCE));
    assert!(
        message.contains("invalid rpc fixture body") && message.contains(&reason),
        "message should carry the reason: {message}"
    );
    reason
}

fn assert_mentions(reason: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            reason.contains(needle),
            "diagnostic should mention {needle}: {reason}"
        );
    }
}

const ASSERT_EXISTS: &str = "\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n";

#[test]
fn a_well_formed_fixture_parses() {
    let fixture = parse_rpc(&format!("method = \"get-network\"\n{ASSERT_EXISTS}")).unwrap();
    assert_eq!(fixture.method, RpcMethod::GetNetwork);
    assert_eq!(
        fixture.assertions,
        vec![FieldAssertion::Exists {
            field: "passphrase".to_string()
        }]
    );
}

// --- method -----------------------------------------------------------------

#[test]
fn missing_method_is_rejected_and_named() {
    let reason = invalid_reason(ASSERT_EXISTS);
    assert_mentions(&reason, &["missing required", "\"method\""]);
}

#[test]
fn non_string_method_is_rejected_as_missing_a_string_method() {
    let reason = invalid_reason(&format!("method = 28\n{ASSERT_EXISTS}"));
    assert_mentions(&reason, &["string field \"method\""]);
}

#[test]
fn unsupported_method_names_the_value_and_the_supported_methods() {
    let reason = invalid_reason(&format!("method = \"getNetwork\"\n{ASSERT_EXISTS}"));
    assert_mentions(
        &reason,
        &["unsupported rpc method \"getNetwork\"", "\"get-network\""],
    );
}

// --- [request] params -------------------------------------------------------

#[test]
fn params_on_a_parameterless_method_are_rejected() {
    let reason = invalid_reason(&format!(
        "method = \"get-health\"\n\n[request]\nstartLedger = 1\n{ASSERT_EXISTS}"
    ));
    assert_mentions(&reason, &["\"get-health\"", "takes no parameters"]);
}

#[test]
fn unknown_param_key_is_named_with_the_accepted_parameters() {
    let reason = invalid_reason(&format!(
        "method = \"get-events\"\n\n[request]\nstartLedgr = 1\n{ASSERT_EXISTS}"
    ));
    assert_mentions(
        &reason,
        &["unknown [request] key", "startLedgr", "startLedger"],
    );
}

#[test]
fn param_of_the_wrong_type_names_the_param() {
    let reason = invalid_reason(&format!(
        "method = \"get-events\"\n\n[request]\nstartLedger = \"ten\"\n{ASSERT_EXISTS}"
    ));
    assert_mentions(&reason, &["[request] startLedger", "must be an integer"]);
}

#[test]
fn out_of_range_param_reports_the_value() {
    let reason = invalid_reason(&format!(
        "method = \"get-events\"\n\n[request]\nstartLedger = -1\n{ASSERT_EXISTS}"
    ));
    assert_mentions(
        &reason,
        &["[request] startLedger", "-1", "not a ledger sequence"],
    );
}

#[test]
fn nested_param_error_names_the_nested_key() {
    let reason = invalid_reason(&format!(
        "method = \"get-events\"\n\n[request.pagination]\nlimit = 0\n{ASSERT_EXISTS}"
    ));
    assert_mentions(&reason, &["[request] pagination", "limit 0"]);
}

#[test]
fn request_that_is_not_a_table_is_rejected() {
    let reason = invalid_reason(&format!(
        "method = \"get-events\"\nrequest = 5\n{ASSERT_EXISTS}"
    ));
    assert_mentions(&reason, &["[request] must be a table"]);
}

// --- unknown keys -----------------------------------------------------------

#[test]
fn unknown_top_level_key_is_named_with_the_accepted_keys() {
    let reason = invalid_reason(&format!(
        "method = \"get-network\"\nexpected = 1\n{ASSERT_EXISTS}"
    ));
    assert_mentions(
        &reason,
        &[
            "unknown key(s) \"expected\"",
            "\"assert\"",
            "\"method\"",
            "\"request\"",
        ],
    );
}

#[test]
fn misspelled_assert_section_is_an_unknown_key_not_a_silent_pass() {
    let reason = invalid_reason(
        "method = \"get-network\"\n\n[[asert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n",
    );
    assert_mentions(&reason, &["unknown key(s) \"asert\""]);
}

#[test]
fn unknown_key_inside_an_assert_entry_is_attributed_to_the_entry() {
    let reason = invalid_reason(
        "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\nfeild = \"x\"\n",
    );
    assert_mentions(&reason, &["[[assert]] entry", "unknown key(s) \"feild\""]);
}

// --- assertion kinds and operators -----------------------------------------

#[test]
fn unknown_assertion_kind_names_the_kind_and_the_supported_kinds() {
    let reason = invalid_reason(
        "method = \"get-network\"\n\n[[assert]]\nkind = \"field-contains\"\nfield = \"passphrase\"\n",
    );
    assert_mentions(
        &reason,
        &[
            "unsupported assertion kind \"field-contains\"",
            "\"field-exists\"",
            "\"field-equals\"",
            "\"field-number-range\"",
            "\"field-one-of\"",
            "\"event-topics\"",
        ],
    );
}

#[test]
fn unknown_expected_type_is_named() {
    let reason = invalid_reason(
        "method = \"get-network\"\n\n[[assert]]\nkind = \"field-type\"\nfield = \"protocolVersion\"\nexpected_type = \"int\"\n",
    );
    assert_mentions(&reason, &["unsupported json type \"int\"", "\"integer\""]);
}

/// CA-01 section 4.2 adds `integer` to the accepted `expected_type`
/// values, so a fixture using it is valid rather than an invalid fixture.
#[test]
fn integer_is_an_accepted_expected_type() {
    let fixture = parse_rpc(
        "method = \"get-network\"\n\n[[assert]]\nkind = \"field-type\"\nfield = \"protocolVersion\"\nexpected_type = \"integer\"\n",
    )
    .expect("integer is a supported expected_type");
    assert_eq!(fixture.assertions.len(), 1);
}

#[test]
fn non_numeric_range_bound_is_named() {
    let reason = invalid_reason(
        "method = \"get-network\"\n\n[[assert]]\nkind = \"field-number-range\"\nfield = \"protocolVersion\"\nmin = \"28\"\n",
    );
    assert_mentions(&reason, &["field-number-range \"min\"", "must be a number"]);
}

// --- assertion block structure ---------------------------------------------

#[test]
fn fixture_without_assertions_is_rejected() {
    let reason = invalid_reason("method = \"get-network\"\n");
    assert_mentions(&reason, &["at least one [[assert]] table"]);
}

#[test]
fn single_bracket_assert_table_is_not_accepted_as_an_assertion_list() {
    let reason = invalid_reason(
        "method = \"get-network\"\n\n[assert]\nkind = \"field-exists\"\nfield = \"passphrase\"\n",
    );
    assert_mentions(&reason, &["at least one [[assert]] table"]);
}

#[test]
fn assert_entry_that_is_not_a_table_is_rejected() {
    let reason = invalid_reason("method = \"get-network\"\nassert = [\"field-exists\"]\n");
    assert_mentions(&reason, &["each [[assert]] entry must be a table"]);
}

#[test]
fn assert_entry_without_kind_is_rejected() {
    let reason = invalid_reason("method = \"get-network\"\n\n[[assert]]\nfield = \"passphrase\"\n");
    assert_mentions(&reason, &["missing \"kind\""]);
}

#[test]
fn assert_entry_without_field_is_rejected() {
    let reason =
        invalid_reason("method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\n");
    assert_mentions(&reason, &["missing \"field\""]);
}

#[test]
fn non_string_field_is_rejected() {
    let reason = invalid_reason(
        "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = 1\n",
    );
    assert_mentions(&reason, &["\"field\" must be a string"]);
}

#[test]
fn field_equals_without_value_is_rejected() {
    let reason = invalid_reason(
        "method = \"get-network\"\n\n[[assert]]\nkind = \"field-equals\"\nfield = \"protocolVersion\"\n",
    );
    assert_mentions(&reason, &["field-equals", "missing \"value\""]);
}

#[test]
fn one_bad_entry_rejects_the_whole_fixture() {
    let reason = invalid_reason(&format!(
        "method = \"get-network\"\n{ASSERT_EXISTS}\n[[assert]]\nkind = \"field-is\"\nfield = \"x\"\n"
    ));
    assert_mentions(&reason, &["\"field-is\""]);
}

// --- TOML syntax: reported by the fixture loader with a line number --------

/// Parses `body` as a whole fixture file and returns the loader's parse
/// error message, after checking the variant and the reported path.
fn toml_parse_error(body: &str) -> String {
    let err = parse_fixture_str(&fixture_text(body), Path::new(SOURCE))
        .expect_err("TOML should be rejected");
    let message = err.to_string();
    match err {
        FixtureError::Parse { path, .. } => assert_eq!(path, Path::new(SOURCE)),
        other => panic!("expected FixtureError::Parse, got {other:?}"),
    }
    message
}

#[test]
fn unquoted_assertion_kind_reports_its_line() {
    // Metadata takes lines 1-5; the bad `kind` is on line 9.
    let message = toml_parse_error("method = \"get-network\"\n\n[[assert]]\nkind = field-exists\n");
    assert_mentions(&message, &["line 9", "kind = field-exists"]);
}

#[test]
fn unterminated_assert_header_reports_its_line() {
    let message = toml_parse_error("method = \"get-network\"\n\n[[assert]\nkind = \"x\"\n");
    assert_mentions(&message, &["line 8", "[[assert]"]);
}

#[test]
fn duplicate_method_key_reports_the_second_definition() {
    let message = toml_parse_error(&format!(
        "method = \"get-network\"\nmethod = \"get-health\"\n{ASSERT_EXISTS}"
    ));
    assert_mentions(&message, &["line 7", "method"]);
}
