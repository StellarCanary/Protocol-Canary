//! The XDR compatibility runner: turns a [`XdrFixture`] into a
//! [`CompatibilityResult`] by decoding/encoding it with the official
//! `stellar-xdr` crate and comparing against the fixture's declared
//! expectation.

use std::time::Instant;

use canary_core::{CanaryError, CompatibilityResult, ExecutionContext, FixtureMetadata, Surface};
use canary_fixtures::LoadedFixture;

use crate::decoder::{decode, XdrTypeName};
use crate::encoder::encode;

/// Errors raised while turning a [`LoadedFixture`] into an [`XdrFixture`].
///
/// One failure mode today: the fixture body is structurally valid YAML/JSON
/// but does not describe a usable XDR assertion — e.g. a required field is
/// missing or has the wrong type. The error names the offending fixture
/// (`source_path`) and carries a human-readable `reason`, and converts into
/// `CanaryError::Xdr` so it propagates through the shared error type without
/// ever failing a run as an unhandled panic.
#[derive(Debug, thiserror::Error)]
pub enum XdrError {
    #[error("invalid xdr fixture body in {source_path}: {reason}")]
    InvalidFixtureBody {
        source_path: std::path::PathBuf,
        reason: String,
    },
}

impl From<XdrError> for CanaryError {
    fn from(error: XdrError) -> Self {
        CanaryError::Xdr(error.to_string())
    }
}

/// The assertion an [`XdrFixture`] makes about a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XdrAssertion {
    /// The value must decode successfully as `type_name`.
    DecodeSuccess { value_base64: String },
    /// The value must fail to decode as `type_name`.
    DecodeFailure { value_base64: String },
    /// Decoding then re-encoding the value must reproduce the same bytes.
    Roundtrip { value_base64: String },
    /// Decoding then re-encoding the value must produce `expected_base64`,
    /// which may differ from the input when the fixture is specifically
    /// testing canonicalization.
    EncodeEquals {
        value_base64: String,
        expected_base64: String,
    },
    /// Decoding the value must produce a JSON structure matching `expected_json`.
    DecodeJsonEquals {
        value_base64: String,
        expected_json: serde_json::Value,
    },
}

/// A fully parsed XDR compatibility fixture.
#[derive(Debug, Clone)]
pub struct XdrFixture {
    pub metadata: FixtureMetadata,
    pub type_name: XdrTypeName,
    pub assertion: XdrAssertion,
}

impl XdrFixture {
    /// Parses an [`XdrFixture`] out of a generic [`LoadedFixture`].
    ///
    /// Expected body shape:
    ///
    /// ```toml
    /// type = "StellarValue"
    /// kind = "decode-success" # | "decode-failure" | "roundtrip" | "encode-equals"
    /// value_base64 = "..."
    /// expected_base64 = "..." # only for "encode-equals"
    /// ```
    pub fn from_loaded(loaded: &LoadedFixture) -> Result<XdrFixture, XdrError> {
        let invalid = |reason: String| XdrError::InvalidFixtureBody {
            source_path: loaded.source_path.clone(),
            reason,
        };

        let table = loaded
            .body
            .as_table()
            .ok_or_else(|| invalid("fixture body must be a table".to_string()))?;

        let type_name: XdrTypeName = table
            .get("type")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| invalid("missing required string field \"type\"".to_string()))?
            .parse()
            .map_err(invalid)?;

        let kind = table
            .get("kind")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| invalid("missing required string field \"kind\"".to_string()))?;

        let string_field = |name: &str| -> Result<String, XdrError> {
            table
                .get(name)
                .and_then(toml::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| invalid(format!("missing required string field {name:?}")))
        };

        let assertion = match kind {
            "decode-success" => {
                let value_base64 = string_field("value_base64")?;
                if let Some(expected_json_val) = table.get("expected_json") {
                    XdrAssertion::DecodeJsonEquals {
                        value_base64,
                        expected_json: toml_to_json(expected_json_val),
                    }
                } else {
                    XdrAssertion::DecodeSuccess { value_base64 }
                }
            }
            "decode-failure" => XdrAssertion::DecodeFailure {
                value_base64: string_field("value_base64")?,
            },
            "roundtrip" => XdrAssertion::Roundtrip {
                value_base64: string_field("value_base64")?,
            },
            "encode-equals" => XdrAssertion::EncodeEquals {
                value_base64: string_field("value_base64")?,
                expected_base64: string_field("expected_base64")?,
            },
            "decode-json-equals" => {
                let value_base64 = string_field("value_base64")?;
                let expected_json_val = table.get("expected_json").ok_or_else(|| {
                    invalid("decode-json-equals assertion missing \"expected_json\"".to_string())
                })?;
                XdrAssertion::DecodeJsonEquals {
                    value_base64,
                    expected_json: toml_to_json(expected_json_val),
                }
            }
            other => {
                return Err(invalid(format!(
                    "unsupported assertion kind {other:?}: expected one of \"decode-success\", \
                     \"decode-failure\", \"roundtrip\", \"encode-equals\", \"decode-json-equals\""
                )))
            }
        };

        Ok(XdrFixture {
            metadata: loaded.metadata.clone(),
            type_name,
            assertion,
        })
    }
}

/// Runs a single [`XdrFixture`] and produces its [`CompatibilityResult`].
pub trait XdrRunner {
    fn run(
        &self,
        fixture: &XdrFixture,
        context: &ExecutionContext,
    ) -> Result<CompatibilityResult, CanaryError>;
}

/// The runner used in production: decodes/encodes with `stellar-xdr`.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultXdrRunner;

impl XdrRunner for DefaultXdrRunner {
    fn run(
        &self,
        fixture: &XdrFixture,
        _context: &ExecutionContext,
    ) -> Result<CompatibilityResult, CanaryError> {
        let start = Instant::now();
        let (status, summary, details) = evaluate(fixture);
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        Ok(CompatibilityResult {
            test_id: fixture.metadata.id.clone(),
            protocol: fixture.metadata.protocol,
            surface: Surface::Xdr,
            status,
            summary,
            details,
            duration_ms,
            fixture_id: Some(fixture.metadata.id.clone()),
        })
    }
}

fn evaluate(fixture: &XdrFixture) -> (canary_core::Status, String, Option<String>) {
    use canary_core::Status;

    match &fixture.assertion {
        XdrAssertion::DecodeSuccess { value_base64 } => {
            match decode(fixture.type_name, value_base64) {
                Ok(_) => (
                    Status::Pass,
                    format!("decoded {} successfully", fixture.type_name),
                    None,
                ),
                Err(e) => (
                    Status::Fail,
                    format!("failed to decode {}", fixture.type_name),
                    Some(e.to_string()),
                ),
            }
        }
        XdrAssertion::DecodeFailure { value_base64 } => {
            match decode(fixture.type_name, value_base64) {
                Ok(_) => (
                    Status::Fail,
                    format!(
                        "expected decoding {} to fail, but it succeeded",
                        fixture.type_name
                    ),
                    None,
                ),
                Err(e) => (
                    Status::Pass,
                    format!("{} was correctly rejected", fixture.type_name),
                    Some(e.to_string()),
                ),
            }
        }
        XdrAssertion::Roundtrip { value_base64 } => match decode(fixture.type_name, value_base64) {
            Err(e) => (
                Status::Fail,
                format!("failed to decode {} for roundtrip", fixture.type_name),
                Some(e.to_string()),
            ),
            Ok(decoded) => match encode(&decoded) {
                Err(e) => (
                    Status::Error,
                    "failed to re-encode a successfully decoded value".to_string(),
                    Some(e.to_string()),
                ),
                Ok(re_encoded) if &re_encoded == value_base64 => (
                    Status::Pass,
                    format!("{} round-tripped byte-for-byte", fixture.type_name),
                    None,
                ),
                Ok(re_encoded) => (
                    Status::Fail,
                    format!("{} did not round-trip byte-for-byte", fixture.type_name),
                    Some(format!("input:  {value_base64}\noutput: {re_encoded}")),
                ),
            },
        },
        XdrAssertion::EncodeEquals {
            value_base64,
            expected_base64,
        } => match decode(fixture.type_name, value_base64) {
            Err(e) => (
                Status::Fail,
                format!("failed to decode {} input", fixture.type_name),
                Some(e.to_string()),
            ),
            Ok(decoded) => match encode(&decoded) {
                Err(e) => (
                    Status::Error,
                    "failed to re-encode a successfully decoded value".to_string(),
                    Some(e.to_string()),
                ),
                Ok(actual) if &actual == expected_base64 => (
                    Status::Pass,
                    format!("{} encoded to the expected bytes", fixture.type_name),
                    None,
                ),
                Ok(actual) => (
                    Status::Fail,
                    format!("{} did not encode to the expected bytes", fixture.type_name),
                    Some(format!("expected: {expected_base64}\nactual:   {actual}")),
                ),
            },
        },
        XdrAssertion::DecodeJsonEquals {
            value_base64,
            expected_json,
        } => match decode(fixture.type_name, value_base64) {
            Err(e) => (
                Status::Fail,
                format!("failed to decode {} input", fixture.type_name),
                Some(e.to_string()),
            ),
            Ok(decoded) => match decoded.to_json() {
                Err(e) => (
                    Status::Error,
                    "failed to serialize decoded XDR value to JSON".to_string(),
                    Some(e.to_string()),
                ),
                Ok(actual_json) => {
                    let diffs = compare_json_values("$", expected_json, &actual_json);
                    if diffs.is_empty() {
                        (
                            Status::Pass,
                            format!("decoded {} JSON matched expectations", fixture.type_name),
                            None,
                        )
                    } else {
                        (
                            Status::Fail,
                            format!(
                                "decoded {} JSON did not match expected structure ({} mismatch(es))",
                                fixture.type_name,
                                diffs.len()
                            ),
                            Some(diffs.join("\n")),
                        )
                    }
                }
            },
        },
    }
}

pub fn compare_json_values(
    path: &str,
    expected: &serde_json::Value,
    actual: &serde_json::Value,
) -> Vec<String> {
    let mut diffs = Vec::new();
    compare_inner(path, expected, actual, &mut diffs);
    diffs
}

fn json_type_name(val: &serde_json::Value) -> &'static str {
    match val {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn path_join(parent: &str, key: &str) -> String {
    let is_simple_ident = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if is_simple_ident {
        format!("{parent}.{key}")
    } else {
        format!("{parent}[\"{key}\"]")
    }
}

fn compare_inner(
    path: &str,
    expected: &serde_json::Value,
    actual: &serde_json::Value,
    diffs: &mut Vec<String>,
) {
    match (expected, actual) {
        (serde_json::Value::Object(exp_map), serde_json::Value::Object(act_map)) => {
            for (key, exp_val) in exp_map {
                let key_path = path_join(path, key);
                match act_map.get(key) {
                    None => {
                        diffs.push(format!(
                            "at {key_path}: missing expected field (expected {exp_val})"
                        ));
                    }
                    Some(act_val) => {
                        compare_inner(&key_path, exp_val, act_val, diffs);
                    }
                }
            }
            for (key, act_val) in act_map {
                let key_path = path_join(path, key);
                if !exp_map.contains_key(key) {
                    diffs.push(format!(
                        "at {key_path}: unexpected field in actual value (found {act_val})"
                    ));
                }
            }
        }
        (serde_json::Value::Array(exp_arr), serde_json::Value::Array(act_arr)) => {
            if exp_arr.len() != act_arr.len() {
                diffs.push(format!(
                    "at {path}: array length mismatch (expected {}, found {})",
                    exp_arr.len(),
                    act_arr.len()
                ));
            }
            let min_len = exp_arr.len().min(act_arr.len());
            for i in 0..min_len {
                let elem_path = format!("{path}[{i}]");
                compare_inner(&elem_path, &exp_arr[i], &act_arr[i], diffs);
            }
        }
        (exp, act) if json_type_name(exp) != json_type_name(act) => {
            diffs.push(format!(
                "at {path}: type mismatch (expected {}, found {})",
                json_type_name(exp),
                json_type_name(act)
            ));
        }
        (exp, act) => {
            if exp != act {
                diffs.push(format!("at {path}: expected {exp}, found {act}"));
            }
        }
    }
}

fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(toml_to_json).collect())
        }
        toml::Value::Table(table) => serde_json::Value::Object(
            table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canary_core::{
        CacheStore, GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType,
        ProtocolVersion, RunOptions,
    };
    use stellar_xdr::{Limits, StellarValue, WriteXdr};

    fn valid_stellar_value_base64() -> String {
        StellarValue::default()
            .to_xdr_base64(Limits::none())
            .unwrap()
    }

    fn loaded_fixture(id: &str, body_toml: &str) -> LoadedFixture {
        canary_fixtures::parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"cap-83\"\ndescription = \"test\"\n{body_toml}"
            ),
            std::path::Path::new("test.toml"),
        )
        .unwrap()
    }

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
            fixtures: canary_core::FixtureStore::default(),
            git: GitContext::default(),
            cache: CacheStore::new(std::env::temp_dir().join("canary-xdr-runner-tests-cache")),
            options: RunOptions::default(),
        }
    }

    #[test]
    fn decode_success_fixture_passes_on_valid_input() {
        let body = format!(
            "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{}\"\n",
            valid_stellar_value_base64()
        );
        let fixture = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-1", &body)).unwrap();
        let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
        assert_eq!(result.status, canary_core::Status::Pass);
    }

    #[test]
    fn decode_success_fixture_fails_on_garbage_input() {
        let body = "type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"!!!not-xdr!!!\"\n";
        let fixture = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-2", body)).unwrap();
        let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
        assert_eq!(result.status, canary_core::Status::Fail);
    }

    #[test]
    fn decode_failure_fixture_passes_when_input_is_correctly_rejected() {
        let body = "type = \"StellarValue\"\nkind = \"decode-failure\"\nvalue_base64 = \"!!!not-xdr!!!\"\n";
        let fixture = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-3", body)).unwrap();
        let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
        assert_eq!(result.status, canary_core::Status::Pass);
    }

    #[test]
    fn decode_failure_fixture_fails_when_input_unexpectedly_decodes() {
        let body = format!(
            "type = \"StellarValue\"\nkind = \"decode-failure\"\nvalue_base64 = \"{}\"\n",
            valid_stellar_value_base64()
        );
        let fixture = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-4", &body)).unwrap();
        let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
        assert_eq!(result.status, canary_core::Status::Fail);
    }

    #[test]
    fn roundtrip_fixture_passes_for_canonical_input() {
        let body = format!(
            "type = \"StellarValue\"\nkind = \"roundtrip\"\nvalue_base64 = \"{}\"\n",
            valid_stellar_value_base64()
        );
        let fixture = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-5", &body)).unwrap();
        let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
        assert_eq!(result.status, canary_core::Status::Pass);
    }

    #[test]
    fn encode_equals_fixture_passes_when_output_matches_expectation() {
        let base64 = valid_stellar_value_base64();
        let body = format!(
            "type = \"StellarValue\"\nkind = \"encode-equals\"\nvalue_base64 = \"{base64}\"\nexpected_base64 = \"{base64}\"\n"
        );
        let fixture = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-6", &body)).unwrap();
        let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
        assert_eq!(result.status, canary_core::Status::Pass);
    }

    #[test]
    fn encode_equals_fixture_fails_when_output_does_not_match_expectation() {
        let base64 = valid_stellar_value_base64();
        let body = format!(
            "type = \"StellarValue\"\nkind = \"encode-equals\"\nvalue_base64 = \"{base64}\"\nexpected_base64 = \"AAAA\"\n"
        );
        let fixture = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-7", &body)).unwrap();
        let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
        assert_eq!(result.status, canary_core::Status::Fail);
    }

    #[test]
    fn encode_equals_fixture_with_malformed_expected_base64_fails_with_mismatch() {
        // `expected_base64` is compared byte-for-byte as a plain string, and the
        // fixture loader does not validate that it is syntactically valid base64.
        // A typo'd `expected_base64` therefore currently parses fine and is
        // reported as an ordinary encode mismatch (`Status::Fail`), not as an
        // `InvalidFixtureBody` parse error. This test locks in that current
        // behavior; whether it should become a parse-time error is a separate
        // design decision.
        let base64 = valid_stellar_value_base64();
        let body = format!(
            "type = \"StellarValue\"\nkind = \"encode-equals\"\nvalue_base64 = \"{base64}\"\nexpected_base64 = \"not valid base64!!!\"\n"
        );
        let fixture = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-10", &body)).unwrap();
        let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();

        assert_eq!(result.status, canary_core::Status::Fail);
        assert_eq!(
            result.summary,
            "StellarValue did not encode to the expected bytes"
        );
        let details = result.details.expect("details should be present");
        assert!(details.contains("expected: not valid base64!!!"));
        assert!(details.contains(&format!("actual:   {base64}")));
    }

    #[test]
    fn rejects_a_fixture_body_missing_the_type_field() {
        let body = "kind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n";
        let err = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-8", body)).unwrap_err();
        assert!(matches!(err, XdrError::InvalidFixtureBody { .. }));
    }

    #[test]
    fn rejects_an_unsupported_assertion_kind() {
        let body = "type = \"StellarValue\"\nkind = \"not-a-kind\"\nvalue_base64 = \"AAAA\"\n";
        let err = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-9", body)).unwrap_err();
        assert!(matches!(err, XdrError::InvalidFixtureBody { .. }));
    }

    /// `LedgerEntry` is a real XDR type name — just not one of the ones
    /// this tool supports. The rejection must surface through fixture
    /// parsing with `XdrTypeName::from_str`'s supported-types list intact,
    /// so a fixture author can self-correct from the error alone.
    #[test]
    fn rejects_a_well_formed_but_unsupported_xdr_type_name_and_lists_the_supported_ones() {
        let body = "type = \"LedgerEntry\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n";
        let err = XdrFixture::from_loaded(&loaded_fixture("p28-xdr-10", body)).unwrap_err();
        match err {
            XdrError::InvalidFixtureBody { reason, .. } => {
                assert!(reason.contains("LedgerEntry"), "reason: {reason}");
                for supported in ["StellarValue", "ContractExecutable", "ScVal"] {
                    assert!(
                        reason.contains(supported),
                        "reason must list supported type {supported:?}: {reason}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_roundtrip_fixture_fails_for_non_canonical_input() {
        // A non-canonical boolean in ScVal: ScVal::B(true) encoded with non-canonical 2 instead of 1.
        let input_base64 = "AAAAAAAAAAI=";
        let expected_output = "AAAAAAAAAAA=";

        let body =
            format!("type = \"ScVal\"\nkind = \"roundtrip\"\nvalue_base64 = \"{input_base64}\"\n");
        let fixture =
            XdrFixture::from_loaded(&loaded_fixture("p28-xdr-roundtrip-fail", &body)).unwrap();
        let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();

        assert_eq!(result.status, canary_core::Status::Fail);
        let details = result.details.expect("details should be present");
        assert!(details.contains(input_base64));
        assert!(details.contains(expected_output));
    }
}
