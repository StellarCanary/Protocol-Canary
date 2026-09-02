//! The Soroban compatibility runner: builds an unsigned invocation
//! transaction, simulates it via Stellar RPC, and checks the outcome
//! against a fixture's declared expectation.

use std::time::Instant;

use canary_core::{
    CanaryError, CompatibilityResult, ExecutionContext, FixtureMetadata, Status, Surface,
};
use canary_fixtures::LoadedFixture;
use canary_rpc::RpcClient;

use crate::builder::{build_invoke_transaction_envelope, BuilderError, InvocationSpec, ScValInput};
use crate::simulation::simulate;

#[derive(Debug, thiserror::Error)]
pub enum SorobanFixtureError {
    #[error("invalid soroban fixture body in {source_path}: {reason}")]
    InvalidFixtureBody {
        source_path: std::path::PathBuf,
        reason: String,
    },
}

impl From<SorobanFixtureError> for CanaryError {
    fn from(error: SorobanFixtureError) -> Self {
        CanaryError::Soroban(error.to_string())
    }
}

impl From<BuilderError> for CanaryError {
    fn from(error: BuilderError) -> Self {
        CanaryError::Soroban(error.to_string())
    }
}

/// What a [`SorobanFixture`] expects the simulation to do.
#[derive(Debug, Clone, PartialEq)]
pub enum SorobanAssertion {
    /// Simulation must succeed (no `error` field in the response).
    SimulationSuccess,
    /// Simulation must fail, optionally with an error message containing
    /// `message_contains`.
    SimulationError { message_contains: Option<String> },
}

/// A fully parsed Soroban compatibility fixture.
#[derive(Debug, Clone)]
pub struct SorobanFixture {
    pub metadata: FixtureMetadata,
    pub invocation: InvocationSpec,
    pub assertion: SorobanAssertion,
}

impl SorobanFixture {
    /// Parses a [`SorobanFixture`] from a generic [`LoadedFixture`].
    ///
    /// Expected body shape:
    ///
    /// ```toml
    /// source_account = "G..."
    /// contract_id = "C..."
    /// function = "hello"
    /// sequence_number = 1
    ///
    /// [[args]]
    /// kind = "symbol"
    /// value = "world"
    ///
    /// [expect]
    /// kind = "simulation-success" # or "simulation-error"
    /// ```
    pub fn from_loaded(loaded: &LoadedFixture) -> Result<SorobanFixture, SorobanFixtureError> {
        let invalid = |reason: String| SorobanFixtureError::InvalidFixtureBody {
            source_path: loaded.source_path.clone(),
            reason,
        };

        let table = loaded
            .body
            .as_table()
            .ok_or_else(|| invalid("fixture body must be a table".to_string()))?;

        let string_field = |name: &str| -> Result<String, SorobanFixtureError> {
            table
                .get(name)
                .and_then(toml::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| invalid(format!("missing required string field {name:?}")))
        };

        let source_account = string_field("source_account")?;
        let contract_id = string_field("contract_id")?;
        let function_name = string_field("function")?;
        let sequence_number = table
            .get("sequence_number")
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| {
                invalid("missing required integer field \"sequence_number\"".to_string())
            })?;

        let arg_entries = table
            .get("args")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut args = Vec::with_capacity(arg_entries.len());
        for entry in &arg_entries {
            args.push(parse_arg(entry, &invalid)?);
        }

        let expect = table
            .get("expect")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| invalid("missing required table \"expect\"".to_string()))?;
        let assertion = match expect.get("kind").and_then(toml::Value::as_str) {
            Some("simulation-success") => SorobanAssertion::SimulationSuccess,
            Some("simulation-error") => SorobanAssertion::SimulationError {
                message_contains: expect
                    .get("message_contains")
                    .and_then(toml::Value::as_str)
                    .map(str::to_string),
            },
            Some(other) => {
                return Err(invalid(format!(
                    "unsupported expectation kind {other:?}: expected \"simulation-success\" or \"simulation-error\""
                )))
            }
            None => return Err(invalid("expect.kind is required".to_string())),
        };

        Ok(SorobanFixture {
            metadata: loaded.metadata.clone(),
            invocation: InvocationSpec {
                source_account,
                contract_id,
                function_name,
                args,
                sequence_number,
            },
            assertion,
        })
    }
}

fn parse_arg(
    entry: &toml::Value,
    invalid: &impl Fn(String) -> SorobanFixtureError,
) -> Result<ScValInput, SorobanFixtureError> {
    let table = entry
        .as_table()
        .ok_or_else(|| invalid("each [[args]] entry must be a table".to_string()))?;
    let kind = table
        .get("kind")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| invalid("arg entry missing \"kind\"".to_string()))?;

    let value = table
        .get("value")
        .ok_or_else(|| invalid("arg entry missing \"value\"".to_string()))?;

    match kind {
        "bool" => value
            .as_bool()
            .map(ScValInput::Bool)
            .ok_or_else(|| invalid("bool arg value must be a boolean".to_string())),
        "u32" => value
            .as_integer()
            .and_then(|i| u32::try_from(i).ok())
            .map(ScValInput::U32)
            .ok_or_else(|| invalid("u32 arg value must be a non-negative integer".to_string())),
        "i32" => value
            .as_integer()
            .and_then(|i| i32::try_from(i).ok())
            .map(ScValInput::I32)
            .ok_or_else(|| invalid("i32 arg value must be an integer".to_string())),
        "u64" => value
            .as_integer()
            .and_then(|i| u64::try_from(i).ok())
            .map(ScValInput::U64)
            .ok_or_else(|| invalid("u64 arg value must be a non-negative integer".to_string())),
        "i64" => value
            .as_integer()
            .map(ScValInput::I64)
            .ok_or_else(|| invalid("i64 arg value must be an integer".to_string())),
        "symbol" => value
            .as_str()
            .map(|s| ScValInput::Symbol(s.to_string()))
            .ok_or_else(|| invalid("symbol arg value must be a string".to_string())),
        "string" => value
            .as_str()
            .map(|s| ScValInput::String(s.to_string()))
            .ok_or_else(|| invalid("string arg value must be a string".to_string())),
        other => Err(invalid(format!(
            "unsupported arg kind {other:?}: expected one of \"bool\", \"u32\", \"i32\", \"u64\", \"i64\", \"symbol\", \"string\""
        ))),
    }
}

/// Runs a single [`SorobanFixture`] against a live [`RpcClient`].
pub trait SorobanRunner {
    fn run(
        &self,
        fixture: &SorobanFixture,
        context: &ExecutionContext,
    ) -> impl std::future::Future<Output = Result<CompatibilityResult, CanaryError>> + Send;
}

/// The runner used in production.
pub struct DefaultSorobanRunner<C: RpcClient> {
    client: C,
}

impl<C: RpcClient> DefaultSorobanRunner<C> {
    pub fn new(client: C) -> Self {
        DefaultSorobanRunner { client }
    }
}

impl<C: RpcClient + Sync> SorobanRunner for DefaultSorobanRunner<C> {
    async fn run(
        &self,
        fixture: &SorobanFixture,
        _context: &ExecutionContext,
    ) -> Result<CompatibilityResult, CanaryError> {
        let start = Instant::now();

        let (status, summary, details) =
            match build_invoke_transaction_envelope(&fixture.invocation) {
                Err(e) => (
                    Status::Error,
                    "failed to build the invocation transaction envelope".to_string(),
                    Some(e.to_string()),
                ),
                Ok(envelope) => match simulate(&self.client, envelope).await {
                    Err(e) => (
                        Status::Error,
                        "failed to call simulateTransaction".to_string(),
                        Some(e.to_string()),
                    ),
                    Ok(response) => evaluate(fixture, &response),
                },
            };

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        Ok(CompatibilityResult {
            test_id: fixture.metadata.id.clone(),
            protocol: fixture.metadata.protocol,
            surface: Surface::Soroban,
            status,
            summary,
            details,
            duration_ms,
            fixture_id: Some(fixture.metadata.id.clone()),
        })
    }
}

fn evaluate(
    fixture: &SorobanFixture,
    response: &canary_rpc::SimulationResponse,
) -> (Status, String, Option<String>) {
    match &fixture.assertion {
        SorobanAssertion::SimulationSuccess => {
            if response.succeeded() {
                (
                    Status::Pass,
                    "simulation succeeded as expected".to_string(),
                    None,
                )
            } else {
                (
                    Status::Fail,
                    "expected simulation to succeed, but it failed".to_string(),
                    response.error.clone(),
                )
            }
        }
        SorobanAssertion::SimulationError { message_contains } => {
            if response.succeeded() {
                (
                    Status::Fail,
                    "expected simulation to fail, but it succeeded".to_string(),
                    None,
                )
            } else {
                match message_contains {
                    None => (
                        Status::Pass,
                        "simulation failed as expected".to_string(),
                        response.error.clone(),
                    ),
                    Some(needle) => {
                        let message = response.error.clone().unwrap_or_default();
                        if message.contains(needle.as_str()) {
                            (
                                Status::Pass,
                                "simulation failed with the expected error".to_string(),
                                Some(message),
                            )
                        } else {
                            (
                                Status::Fail,
                                "simulation failed, but not with the expected error message"
                                    .to_string(),
                                Some(format!("expected to contain: {needle}\nactual: {message}")),
                            )
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canary_rpc::HttpRpcClient;
    use serde_json::json;
    use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn body(kind: &str, extra: &str) -> String {
        let source_account = StrkeyPublicKey([0u8; 32]).to_string();
        let contract_id = StrkeyContract([0u8; 32]).to_string();
        format!(
            "source_account = \"{source_account}\"\ncontract_id = \"{contract_id}\"\nfunction = \"hello\"\nsequence_number = 1\n\n[[args]]\nkind = \"symbol\"\nvalue = \"world\"\n\n[expect]\nkind = \"{kind}\"\n{extra}"
        )
    }

    fn fixture(id: &str, kind: &str, extra: &str) -> LoadedFixture {
        canary_fixtures::parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"cap-85\"\ndescription = \"test\"\n{}",
                body(kind, extra)
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
                project_type: ProjectType::Soroban,
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
                std::env::temp_dir().join("canary-soroban-runner-tests-cache"),
            ),
            options: RunOptions::default(),
        }
    }

    #[tokio::test]
    async fn passes_when_simulation_succeeds_as_expected() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "latestLedger": 1000, "transactionData": "AAAA" }
            })))
            .mount(&server)
            .await;

        let fixture =
            SorobanFixture::from_loaded(&fixture("p28-soroban-1", "simulation-success", ""))
                .unwrap();
        let runner = DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Pass);
    }

    #[tokio::test]
    async fn fails_when_simulation_unexpectedly_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "latestLedger": 1000, "error": "host invocation failed" }
            })))
            .mount(&server)
            .await;

        let fixture =
            SorobanFixture::from_loaded(&fixture("p28-soroban-2", "simulation-success", ""))
                .unwrap();
        let runner = DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Fail);
    }

    #[tokio::test]
    async fn passes_when_simulation_error_matches_expected_substring() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "latestLedger": 1000, "error": "Error(Contract, #1)" }
            })))
            .mount(&server)
            .await;

        let fixture = SorobanFixture::from_loaded(&fixture(
            "p28-soroban-3",
            "simulation-error",
            "message_contains = \"Contract\"\n",
        ))
        .unwrap();
        let runner = DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn rejects_a_fixture_missing_the_expect_table() {
        let toml = format!(
            "id = \"bad\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"x\"\ndescription = \"x\"\nsource_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"f\"\nsequence_number = 1\n",
            StrkeyPublicKey([0u8; 32]),
            StrkeyContract([0u8; 32]),
        );
        let loaded =
            canary_fixtures::parse_fixture_str(&toml, std::path::Path::new("t.toml")).unwrap();
        let err = SorobanFixture::from_loaded(&loaded).unwrap_err();
        assert!(matches!(
            err,
            SorobanFixtureError::InvalidFixtureBody { .. }
        ));
    }
}
