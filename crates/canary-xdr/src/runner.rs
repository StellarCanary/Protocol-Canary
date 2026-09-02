//! The XDR compatibility runner: turns a [`XdrFixture`] into a
//! [`CompatibilityResult`] by decoding/encoding it with the official
//! `stellar-xdr` crate and comparing against the fixture's declared
//! expectation.

use std::time::Instant;

use canary_core::{CanaryError, CompatibilityResult, ExecutionContext, FixtureMetadata, Surface};
use canary_fixtures::LoadedFixture;

use crate::decoder::{decode, XdrTypeName};
use crate::encoder::encode;

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
            "decode-success" => XdrAssertion::DecodeSuccess {
                value_base64: string_field("value_base64")?,
            },
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
            other => {
                return Err(invalid(format!(
                    "unsupported assertion kind {other:?}: expected one of \"decode-success\", \
                     \"decode-failure\", \"roundtrip\", \"encode-equals\""
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
}
