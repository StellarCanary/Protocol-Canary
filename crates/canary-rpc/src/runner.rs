//! The RPC compatibility runner: calls a Stellar RPC method and checks its
//! response shape against a fixture's declared field assertions.
//!
//! A fixture declares which fields of the response are `required`,
//! `optional`, or simply not mentioned (and therefore ignored) — an
//! unrelated new optional field appearing in a response must never fail a
//! check (see [`FieldAssertion`]).

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

/// One assertion about a field in the RPC response.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldAssertion {
    Exists { field: String },
    Absent { field: String },
    Equals { field: String, value: JsonValue },
    TypeIs { field: String, expected: JsonType },
}

impl FieldAssertion {
    fn field(&self) -> &str {
        match self {
            FieldAssertion::Exists { field }
            | FieldAssertion::Absent { field }
            | FieldAssertion::Equals { field, .. }
            | FieldAssertion::TypeIs { field, .. } => field,
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
        other => Err(invalid(format!(
            "unsupported assertion kind {other:?}: expected one of \"field-exists\", \"field-absent\", \"field-equals\", \"field-type\""
        ))),
    }
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

/// Fetches network identity from `client` and reports how it compares to
/// `target_protocol`, without treating a mismatch as a hard failure to
/// execute: the caller decides what a mismatch means for its run.
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
            RpcFixture::from_loaded(&fixture("p28-rpc-3", "method = \"get-network\"\n")).unwrap();
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Error);
    }

    #[test]
    fn rejects_an_unsupported_method_name() {
        let err =
            RpcFixture::from_loaded(&fixture("bad", "method = \"not-a-method\"\n")).unwrap_err();
        assert!(matches!(err, RpcFixtureError::InvalidFixtureBody { .. }));
    }
}
