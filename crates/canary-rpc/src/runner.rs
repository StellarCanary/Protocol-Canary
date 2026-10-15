//! The RPC compatibility runner: calls a Stellar RPC method and checks its
//! response shape against a fixture's declared field assertions.
//!
//! A fixture declares which fields of the response are `required`,
//! `optional`, or simply not mentioned (and therefore ignored) — an
//! unrelated new optional field appearing in a response must never fail a
//! check (see [`FieldAssertion`]).

use std::cmp::Ordering;
use std::time::Instant;

use serde_json::Value as JsonValue;

use canary_core::{
    CanaryError, CompatibilityResult, ExecutionContext, FixtureMetadata, Status, Surface,
};
use canary_fixtures::LoadedFixture;

use crate::client::{RpcClient, RpcError};
use crate::models::{LatestLedger, NetworkInfo};

#[derive(Debug, thiserror::Error)]
pub enum RpcFixtureError {
    #[error("invalid rpc fixture body in {source_path}: {reason}")]
    InvalidFixtureBody {
        source_path: std::path::PathBuf,
        reason: String,
    },
}

impl From<RpcFixtureError> for CanaryError {
    fn from(error: RpcFixtureError) -> Self {
        CanaryError::Rpc(error.to_string())
    }
}

/// Which RPC method a fixture exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcMethod {
    GetNetwork,
    GetLatestLedger,
}

impl std::str::FromStr for RpcMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "get-network" => Ok(RpcMethod::GetNetwork),
            "get-latest-ledger" => Ok(RpcMethod::GetLatestLedger),
            other => Err(format!(
                "unsupported rpc method {other:?}: supported methods are: \"get-network\", \"get-latest-ledger\""
            )),
        }
    }
}

/// The expected JSON type of a field, for `field-type` assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonType {
    String,
    Number,
    Bool,
    Object,
    Array,
    Null,
}

impl JsonType {
    fn matches(self, value: &JsonValue) -> bool {
        match self {
            JsonType::String => value.is_string(),
            JsonType::Number => value.is_number(),
            JsonType::Bool => value.is_boolean(),
            JsonType::Object => value.is_object(),
            JsonType::Array => value.is_array(),
            JsonType::Null => value.is_null(),
        }
    }
}

impl std::str::FromStr for JsonType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "string" => Ok(JsonType::String),
            "number" => Ok(JsonType::Number),
            "bool" => Ok(JsonType::Bool),
            "object" => Ok(JsonType::Object),
            "array" => Ok(JsonType::Array),
            "null" => Ok(JsonType::Null),
            other => Err(format!("unsupported json type {other:?}")),
        }
    }
}

/// The inclusive bounds of a `field-number-range` assertion (CA-01
/// section 4.2).
///
/// At least one bound is always set when the assertion is parsed from a
/// fixture: an open-ended range is written with only the bound that
/// exists (`>= min` or `<= max`), and a range with neither bound would
/// check nothing.
///
/// Bounds keep the JSON number they were parsed from (integer or
/// float), so integer bounds compare exactly and floating point is used
/// only when a bound or an observed value is genuinely fractional.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldRangeAssertion {
    pub field: String,
    pub min: Option<serde_json::Number>,
    pub max: Option<serde_json::Number>,
}

impl FieldRangeAssertion {
    /// Checks the observed value of the field against the bounds,
    /// returning a human-readable failure reason if it does not hold.
    ///
    /// A missing field or a value that is not a JSON number fails the
    /// assertion; nothing is coerced between strings and numbers.
    pub fn check_value(&self, found: Option<&JsonValue>) -> Result<(), String> {
        let Some(actual) = found else {
            return Err(format!(
                "expected field {:?} to be present, but it was missing",
                self.field
            ));
        };
        let JsonValue::Number(value) = actual else {
            return Err(format!(
                "expected field {:?} to be a number within {}, but found {}",
                self.field,
                self.bounds_description(),
                actual
            ));
        };
        if let Some(min) = &self.min {
            if compare_numbers(value, min) == Some(Ordering::Less) {
                return Err(self.out_of_range(actual));
            }
        }
        if let Some(max) = &self.max {
            if compare_numbers(value, max) == Some(Ordering::Greater) {
                return Err(self.out_of_range(actual));
            }
        }
        Ok(())
    }

    /// The bounds as they appear in failure messages: `[min, max]`,
    /// `>= min`, or `<= max`.
    fn bounds_description(&self) -> String {
        match (&self.min, &self.max) {
            (Some(min), Some(max)) => format!("[{min}, {max}]"),
            (Some(min), None) => format!(">= {min}"),
            (None, Some(max)) => format!("<= {max}"),
            (None, None) => "any number".to_string(),
        }
    }

    fn out_of_range(&self, actual: &JsonValue) -> String {
        format!(
            "expected field {:?} to be within {}, but found {}",
            self.field,
            self.bounds_description(),
            actual
        )
    }
}

/// Compares two JSON numbers, exactly when both sides are integers and
/// with `f64` only when at least one side is genuinely fractional.
/// Returns `None` when either value cannot be read as a number.
fn compare_numbers(value: &serde_json::Number, bound: &serde_json::Number) -> Option<Ordering> {
    if let (Some(value), Some(bound)) = (value.as_i64(), bound.as_i64()) {
        return Some(value.cmp(&bound));
    }
    if let (Some(value), Some(bound)) = (value.as_u64(), bound.as_u64()) {
        return Some(value.cmp(&bound));
    }
    value.as_f64()?.partial_cmp(&bound.as_f64()?)
}

/// One assertion about a field in the RPC response.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldAssertion {
    Exists { field: String },
    Absent { field: String },
    Equals { field: String, value: JsonValue },
    TypeIs { field: String, expected: JsonType },
    NumberRange(FieldRangeAssertion),
}

impl FieldAssertion {
    fn field(&self) -> &str {
        match self {
            FieldAssertion::Exists { field }
            | FieldAssertion::Absent { field }
            | FieldAssertion::Equals { field, .. }
            | FieldAssertion::TypeIs { field, .. } => field,
            FieldAssertion::NumberRange(assertion) => &assertion.field,
        }
    }

    /// Checks this assertion against a response object, returning a
    /// human-readable failure reason if it does not hold.
    fn check(&self, response: &JsonValue) -> Result<(), String> {
        let found = response.get(self.field());
        match self {
            FieldAssertion::Exists { field } => found.map(|_| ()).ok_or_else(|| {
                format!("expected field {field:?} to be present, but it was missing")
            }),
            FieldAssertion::Absent { field } => match found {
                None => Ok(()),
                Some(_) => Err(format!(
                    "expected field {field:?} to be absent, but it was present"
                )),
            },
            FieldAssertion::Equals { field, value } => match found {
                None => Err(format!(
                    "expected field {field:?} to equal {value}, but it was missing"
                )),
                Some(actual) if actual == value => Ok(()),
                Some(actual) => Err(format!(
                    "expected field {field:?} to equal {value}, but found {actual}"
                )),
            },
            FieldAssertion::TypeIs { field, expected } => match found {
                None => Err(format!(
                    "expected field {field:?} to be present, but it was missing"
                )),
                Some(actual) if expected.matches(actual) => Ok(()),
                Some(actual) => Err(format!(
                    "expected field {field:?} to have type {expected:?}, but found {actual}"
                )),
            },
            FieldAssertion::NumberRange(assertion) => assertion.check_value(found),
        }
    }
}

/// A fully parsed RPC compatibility fixture.
#[derive(Debug, Clone)]
pub struct RpcFixture {
    pub metadata: FixtureMetadata,
    pub method: RpcMethod,
    pub assertions: Vec<FieldAssertion>,
}

impl RpcFixture {
    /// Parses an [`RpcFixture`] from a generic [`LoadedFixture`].
    ///
    /// Expected body shape:
    ///
    /// ```toml
    /// method = "get-network"
    ///
    /// [[assert]]
    /// kind = "field-equals"
    /// field = "protocolVersion"
    /// value = 28
    ///
    /// [[assert]]
    /// kind = "field-exists"
    /// field = "passphrase"
    /// ```
    pub fn from_loaded(loaded: &LoadedFixture) -> Result<RpcFixture, RpcFixtureError> {
        let invalid = |reason: String| RpcFixtureError::InvalidFixtureBody {
            source_path: loaded.source_path.clone(),
            reason,
        };

        let table = loaded
            .body
            .as_table()
            .ok_or_else(|| invalid("fixture body must be a table".to_string()))?;

        if let Some(message) = canary_fixtures::unknown_body_keys(table, &["method", "assert"]) {
            return Err(invalid(message));
        }

        let method: RpcMethod = table
            .get("method")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| invalid("missing required string field \"method\"".to_string()))?
            .parse()
            .map_err(invalid)?;

        let assert_entries = table
            .get("assert")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default();

        if assert_entries.is_empty() {
            return Err(invalid(
                "an rpc fixture needs at least one [[assert]] table; without one it would pass while checking nothing (is a section misspelled?)".to_string(),
            ));
        }

        let mut assertions = Vec::with_capacity(assert_entries.len());
        for entry in &assert_entries {
            assertions.push(parse_assertion(entry, &invalid)?);
        }

        Ok(RpcFixture {
            metadata: loaded.metadata.clone(),
            method,
            assertions,
        })
    }
}

fn parse_assertion(
    entry: &toml::Value,
    invalid: &impl Fn(String) -> RpcFixtureError,
) -> Result<FieldAssertion, RpcFixtureError> {
    let table = entry
        .as_table()
        .ok_or_else(|| invalid("each [[assert]] entry must be a table".to_string()))?;

    if let Some(message) = canary_fixtures::unknown_body_keys(
        table,
        &["kind", "field", "value", "expected_type", "min", "max"],
    ) {
        return Err(invalid(format!("[[assert]] entry: {message}")));
    }

    let kind = table
        .get("kind")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| invalid("assertion entry missing \"kind\"".to_string()))?;
    let field = table
        .get("field")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| invalid("assertion entry missing \"field\"".to_string()))?
        .to_string();

    match kind {
        "field-exists" => Ok(FieldAssertion::Exists { field }),
        "field-absent" => Ok(FieldAssertion::Absent { field }),
        "field-equals" => {
            let value = table
                .get("value")
                .ok_or_else(|| invalid("field-equals assertion missing \"value\"".to_string()))?;
            Ok(FieldAssertion::Equals {
                field,
                value: toml_to_json(value),
            })
        }
        "field-type" => {
            let expected = table
                .get("expected_type")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| invalid("field-type assertion missing \"expected_type\"".to_string()))?
                .parse()
                .map_err(invalid)?;
            Ok(FieldAssertion::TypeIs { field, expected })
        }
        "field-number-range" => {
            let min = parse_range_bound(table, "min", &invalid)?;
            let max = parse_range_bound(table, "max", &invalid)?;
            if min.is_none() && max.is_none() {
                return Err(invalid(
                    "field-number-range assertion requires \"min\" or \"max\" (or both); a range with neither bound would check nothing"
                        .to_string(),
                ));
            }
            if let (Some(min), Some(max)) = (&min, &max) {
                if compare_numbers(min, max) == Some(Ordering::Greater) {
                    return Err(invalid(format!(
                        "field-number-range assertion has min {min} greater than max {max}"
                    )));
                }
            }
            Ok(FieldAssertion::NumberRange(FieldRangeAssertion {
                field,
                min,
                max,
            }))
        }
        other => Err(invalid(format!(
            "unsupported assertion kind {other:?}: expected one of \"field-exists\", \"field-absent\", \"field-equals\", \"field-type\", \"field-number-range\""
        ))),
    }
}

/// Parses one `field-number-range` bound: a TOML integer or float.
/// Anything else is an invalid fixture rather than a runtime surprise.
fn parse_range_bound(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    invalid: &impl Fn(String) -> RpcFixtureError,
) -> Result<Option<serde_json::Number>, RpcFixtureError> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    let number = match value {
        toml::Value::Integer(i) => serde_json::Number::from(*i),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f).ok_or_else(|| {
            invalid(format!(
                "field-number-range {key:?} must be a finite number"
            ))
        })?,
        other => {
            return Err(invalid(format!(
                "field-number-range {key:?} must be a number, but found {other}"
            )))
        }
    };
    Ok(Some(number))
}

fn toml_to_json(value: &toml::Value) -> JsonValue {
    match value {
        toml::Value::String(s) => JsonValue::String(s.clone()),
        toml::Value::Integer(i) => JsonValue::Number((*i).into()),
        toml::Value::Float(f) => {
            serde_json::Number::from_f64(*f).map_or(JsonValue::Null, JsonValue::Number)
        }
        toml::Value::Boolean(b) => JsonValue::Bool(*b),
        toml::Value::Datetime(dt) => JsonValue::String(dt.to_string()),
        toml::Value::Array(items) => JsonValue::Array(items.iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => JsonValue::Object(
            table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect(),
        ),
    }
}

/// Runs a single [`RpcFixture`] against a live [`RpcClient`].
pub trait RpcRunner {
    fn run(
        &self,
        fixture: &RpcFixture,
        context: &ExecutionContext,
    ) -> impl std::future::Future<Output = Result<CompatibilityResult, CanaryError>> + Send;
}

/// The runner used in production.
pub struct DefaultRpcRunner<C: RpcClient> {
    client: C,
}

impl<C: RpcClient> DefaultRpcRunner<C> {
    pub fn new(client: C) -> Self {
        DefaultRpcRunner { client }
    }
}

impl<C: RpcClient + Sync> RpcRunner for DefaultRpcRunner<C> {
    async fn run(
        &self,
        fixture: &RpcFixture,
        _context: &ExecutionContext,
    ) -> Result<CompatibilityResult, CanaryError> {
        let start = Instant::now();

        let response_json = match fixture.method {
            RpcMethod::GetNetwork => self.client.get_network().await.map(json_of),
            RpcMethod::GetLatestLedger => self.client.get_latest_ledger().await.map(json_of),
        };

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        let (status, summary, details) = match response_json {
            Err(rpc_error) => (
                Status::Error,
                format!("failed to call {:?}", fixture.method),
                Some(rpc_error.to_string()),
            ),
            Ok(response) => evaluate(fixture, &response),
        };

        Ok(CompatibilityResult {
            test_id: fixture.metadata.id.clone(),
            protocol: fixture.metadata.protocol,
            surface: Surface::Rpc,
            status,
            summary,
            details,
            duration_ms,
            fixture_id: Some(fixture.metadata.id.clone()),
            source: canary_core::ResultSource::Live,
        })
    }
}

fn json_of<T: serde::Serialize>(value: T) -> JsonValue {
    serde_json::to_value(value).unwrap_or(JsonValue::Null)
}

fn evaluate(fixture: &RpcFixture, response: &JsonValue) -> (Status, String, Option<String>) {
    let mut failures = Vec::new();
    for assertion in &fixture.assertions {
        if let Err(reason) = assertion.check(response) {
            failures.push(reason);
        }
    }

    if failures.is_empty() {
        (
            Status::Pass,
            format!(
                "{:?} response matched all {} assertion(s)",
                fixture.method,
                fixture.assertions.len()
            ),
            None,
        )
    } else {
        (
            Status::Fail,
            format!(
                "{:?} response failed {} of {} assertion(s)",
                fixture.method,
                failures.len(),
                fixture.assertions.len()
            ),
            Some(failures.join("\n")),
        )
    }
}

/// Fetches network identity from `client`, without treating any
/// comparison against the run's expectations as a hard failure to
/// execute: the caller decides what a mismatch means for its run. To
/// compare the result against an expected passphrase and target protocol
/// (constructing [`RpcError::NetworkMismatch`]/[`RpcError::ProtocolMismatch`]),
/// pass it to [`crate::validate_network_info`].
pub async fn observe_network(client: &impl RpcClient) -> Result<NetworkInfo, RpcError> {
    client.get_network().await
}

/// Fetches the latest ledger, primarily to cross-check protocol identity
/// against [`observe_network`].
pub async fn observe_latest_ledger(client: &impl RpcClient) -> Result<LatestLedger, RpcError> {
    client.get_latest_ledger().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::HttpRpcClient;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fixture(id: &str, body_toml: &str) -> LoadedFixture {
        canary_fixtures::parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"network\"\ndescription = \"test\"\n{body_toml}"
            ),
            std::path::Path::new("test.toml"),
        )
        .unwrap()
    }

    fn context() -> ExecutionContext {
        use canary_core::{
            GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType, ProtocolVersion,
            RunOptions,
        };
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
            cache: canary_core::CacheStore::new(
                std::env::temp_dir().join("canary-rpc-runner-tests-cache"),
            ),
            options: RunOptions::default(),
        }
    }

    #[tokio::test]
    async fn passes_when_all_assertions_hold() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 28 }
            })))
            .mount(&server)
            .await;

        let body = r#"
        method = "get-network"

        [[assert]]
        kind = "field-equals"
        field = "protocolVersion"
        value = 28

        [[assert]]
        kind = "field-exists"
        field = "passphrase"

        [[assert]]
        kind = "field-absent"
        field = "friendbotUrl"
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-1", body)).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Pass);
    }

    #[tokio::test]
    async fn fails_when_a_field_equals_assertion_does_not_hold() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 27 }
            })))
            .mount(&server)
            .await;

        let body = r#"
        method = "get-network"

        [[assert]]
        kind = "field-equals"
        field = "protocolVersion"
        value = 28
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-2", body)).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Fail);
    }

    #[tokio::test]
    async fn a_transport_failure_produces_an_error_status_not_a_fail() {
        let runner = DefaultRpcRunner::new(HttpRpcClient::new("http://127.0.0.1:0"));
        let fixture =
            RpcFixture::from_loaded(&fixture("p28-rpc-3", "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n")).unwrap();
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Error);
    }

    #[test]
    fn rejects_an_unsupported_method_name() {
        let err =
            RpcFixture::from_loaded(&fixture("bad", "method = \"not-a-method\"\n")).unwrap_err();
        assert!(matches!(err, RpcFixtureError::InvalidFixtureBody { .. }));
    }

    #[test]
    fn parses_and_checks_array_field_equals() {
        let body = r#"
        method = "get-network"

        [[assert]]
        kind = "field-equals"
        field = "history"
        value = ["a", "b", "c"]
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-array", body)).unwrap();
        assert!(fixture.assertions.contains(&FieldAssertion::Equals {
            field: "history".into(),
            value: json!(["a", "b", "c"])
        }));

        let assertion = &fixture.assertions[0];

        let match_response = json!({ "history": ["a", "b", "c"] });
        assert!(assertion.check(&match_response).is_ok());

        let mismatch_response = json!({ "history": ["a", "b", "d"] });
        assert!(assertion.check(&mismatch_response).is_err());
    }

    /// Parses a single `[[assert]]` entry of the given kind into a
    /// [`FieldAssertion`], the way a fixture author's entry reaches the
    /// runner.
    fn range_assertion(entry_toml: &str) -> FieldAssertion {
        RpcFixture::from_loaded(&fixture(
            "p28-rpc-range",
            &format!("method = \"get-network\"\n\n[[assert]]\n{entry_toml}"),
        ))
        .expect("valid range assertion")
        .assertions
        .remove(0)
    }

    fn range_err(entry_toml: &str) -> String {
        RpcFixture::from_loaded(&fixture(
            "p28-rpc-range-bad",
            &format!("method = \"get-network\"\n\n[[assert]]\n{entry_toml}"),
        ))
        .expect_err("invalid range assertion")
        .to_string()
    }

    #[test]
    fn field_number_range_passes_at_both_inclusive_boundaries() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"protocolVersion\"\nmin = 20\nmax = 30\n",
        );
        assert!(assertion.check(&json!({ "protocolVersion": 20 })).is_ok());
        assert!(assertion.check(&json!({ "protocolVersion": 30 })).is_ok());
        assert!(assertion.check(&json!({ "protocolVersion": 25 })).is_ok());
    }

    #[test]
    fn field_number_range_reports_the_observed_value_and_the_expected_range() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 20\nmax = 30\n",
        );
        let below = assertion.check(&json!({ "sequence": 19 })).unwrap_err();
        assert!(below.contains("[20, 30]"), "{below}");
        assert!(below.contains("but found 19"), "{below}");
        let above = assertion.check(&json!({ "sequence": 31 })).unwrap_err();
        assert!(above.contains("[20, 30]"), "{above}");
        assert!(above.contains("but found 31"), "{above}");
    }

    #[test]
    fn field_number_range_fails_when_the_field_is_missing() {
        let assertion =
            range_assertion("kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 1\n");
        let err = assertion.check(&json!({})).unwrap_err();
        assert!(err.contains("\"sequence\" to be present"), "{err}");
    }

    #[test]
    fn field_number_range_rejects_non_numerical_values_without_coercion() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 1\nmax = 100\n",
        );
        for value in [
            json!("30"),
            json!(true),
            json!(null),
            json!([30]),
            json!({ "sequence": 30 }),
        ] {
            let response = json!({ "sequence": value });
            let err = assertion.check(&response).unwrap_err();
            assert!(
                err.contains("\"sequence\" to be a number within [1, 100]"),
                "{err}"
            );
        }
    }

    #[test]
    fn field_number_range_supports_fractional_bounds_and_values() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"fee\"\nmin = 0.5\nmax = 2.5\n",
        );
        assert!(assertion.check(&json!({ "fee": 0.5 })).is_ok());
        assert!(assertion.check(&json!({ "fee": 1.75 })).is_ok());
        // An integer value still compares exactly against float bounds.
        assert!(assertion.check(&json!({ "fee": 2 })).is_ok());
        let err = assertion.check(&json!({ "fee": 3.0 })).unwrap_err();
        assert!(err.contains("[0.5, 2.5]"), "{err}");
        assert!(err.contains("but found 3.0"), "{err}");
        assert!(assertion.check(&json!({ "fee": 0.25 })).is_err());
    }

    #[test]
    fn field_number_range_accepts_integer_values_against_float_bounds_exactly() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"protocolVersion\"\nmin = 28.0\nmax = 29.0\n",
        );
        assert!(assertion.check(&json!({ "protocolVersion": 28 })).is_ok());
        assert!(assertion.check(&json!({ "protocolVersion": 29 })).is_ok());
    }

    #[test]
    fn field_number_range_supports_open_ended_bounds() {
        let min_only =
            range_assertion("kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 1\n");
        assert!(min_only.check(&json!({ "sequence": 1_000_000 })).is_ok());
        let min_err = min_only.check(&json!({ "sequence": 0 })).unwrap_err();
        assert!(min_err.contains(">= 1"), "{min_err}");

        let max_only =
            range_assertion("kind = \"field-number-range\"\nfield = \"sequence\"\nmax = 10\n");
        assert!(max_only.check(&json!({ "sequence": 0 })).is_ok());
        let max_err = max_only.check(&json!({ "sequence": 11 })).unwrap_err();
        assert!(max_err.contains("<= 10"), "{max_err}");
    }

    #[test]
    fn field_number_range_requires_at_least_one_bound() {
        let err = range_err("kind = \"field-number-range\"\nfield = \"sequence\"\n");
        assert!(err.contains("requires \"min\" or \"max\""), "{err}");
    }

    #[test]
    fn field_number_range_rejects_min_greater_than_max() {
        let err =
            range_err("kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 10\nmax = 5\n");
        assert!(err.contains("min 10 greater than max 5"), "{err}");
    }

    #[test]
    fn field_number_range_rejects_non_numerical_bounds() {
        let err =
            range_err("kind = \"field-number-range\"\nfield = \"sequence\"\nmin = \"twenty\"\n");
        assert!(
            err.contains("field-number-range \"min\" must be a number"),
            "{err}"
        );
    }

    #[test]
    fn field_number_range_rejects_unknown_keys_in_the_entry() {
        let err = range_err(
            "kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 1\nexpect = \"x\"\n",
        );
        assert!(err.contains("unknown key(s) \"expect\""), "{err}");
    }

    #[tokio::test]
    async fn field_number_range_passes_a_live_ledger_sequence_within_bounds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "id": "0000000000000000000000000000000000000000000000000000000000000000",
                    "protocolVersion": 28,
                    "sequence": 100
                }
            })))
            .mount(&server)
            .await;

        let body = r#"
        method = "get-latest-ledger"

        [[assert]]
        kind = "field-number-range"
        field = "sequence"
        min = 1
        max = 200
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-range-pass", body)).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Pass);
    }

    #[tokio::test]
    async fn field_number_range_out_of_bounds_is_a_fail_with_the_range_in_details() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "id": "0000000000000000000000000000000000000000000000000000000000000000",
                    "protocolVersion": 28,
                    "sequence": 100
                }
            })))
            .mount(&server)
            .await;

        let body = r#"
        method = "get-latest-ledger"

        [[assert]]
        kind = "field-number-range"
        field = "sequence"
        min = 1
        max = 10
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-range-fail", body)).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Fail);
        let details = result.details.expect("failure details");
        assert!(details.contains("[1, 10]"), "{details}");
        assert!(details.contains("but found 100"), "{details}");
    }

    #[test]
    fn parses_and_checks_table_and_datetime_field_equals() {
        let body = r#"
        method = "get-network"

        [[assert]]
        kind = "field-equals"
        field = "nested"
        value = { key1 = "value1", key2 = 42 }

        [[assert]]
        kind = "field-equals"
        field = "timestamp"
        value = 2023-01-01T12:00:00Z
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-table", body)).unwrap();
        assert!(fixture.assertions.contains(&FieldAssertion::Equals {
            field: "nested".into(),
            value: json!({ "key1": "value1", "key2": 42 })
        }));
        assert!(fixture.assertions.contains(&FieldAssertion::Equals {
            field: "timestamp".into(),
            value: json!("2023-01-01T12:00:00Z")
        }));

        let assertion_nested = &fixture.assertions[0];
        let assertion_timestamp = &fixture.assertions[1];

        let match_response = json!({
            "nested": { "key1": "value1", "key2": 42 },
            "timestamp": "2023-01-01T12:00:00Z"
        });
        assert!(assertion_nested.check(&match_response).is_ok());
        assert!(assertion_timestamp.check(&match_response).is_ok());

        let mismatch_response = json!({
            "nested": { "key1": "value1", "key2": 99 },
            "timestamp": "2023-01-01T12:00:00Z"
        });
        assert!(assertion_nested.check(&mismatch_response).is_err());
        assert!(assertion_timestamp.check(&mismatch_response).is_ok());
    }
}
