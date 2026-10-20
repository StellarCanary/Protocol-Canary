//! The Soroban compatibility runner: builds an unsigned invocation
//! transaction, simulates it via Stellar RPC, and checks the outcome
//! against a fixture's declared expectation.

use std::time::Instant;

use canary_core::{
    CanaryError, CompatibilityResult, ExecutionContext, FixtureMetadata, Status, Surface,
};
use canary_fixtures::LoadedFixture;
use canary_rpc::RpcClient;

use crate::builder::{
    build_invoke_transaction_envelope, BuilderError, InvocationSpec, ScValInput, MAX_ARG_DEPTH,
};
use crate::simulation::simulate;

/// Errors raised while turning a [`LoadedFixture`] into a [`SorobanFixture`].
///
/// One failure mode today: the fixture body is structurally valid TOML but
/// does not describe a usable Soroban invocation. That covers a missing or
/// mistyped required field (`source_account`, `contract_id`, `function`,
/// `sequence_number`), an `[[args]]` entry whose `kind` this crate does not
/// support or whose `value` does not match that `kind`, and an `[expect]`
/// table that is absent or declares an unknown expectation.
///
/// The error names the offending fixture (`source_path`) and carries a
/// human-readable `reason`, and converts into `CanaryError::Soroban` so it
/// propagates through the shared error type without ever failing a run as an
/// unhandled panic. Returned by [`SorobanFixture::from_loaded`]; constructing
/// a [`SorobanFixture`] never panics on malformed input.
#[derive(Debug, thiserror::Error)]
pub enum SorobanFixtureError {
    /// The fixture's body could not be parsed into a [`SorobanFixture`].
    ///
    /// `source_path` is the path the fixture was loaded from (used to point
    /// the user at the file to fix) and `reason` explains which field or
    /// assertion was rejected.
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

        if let Some(message) = canary_fixtures::unknown_body_keys(
            table,
            &[
                "source_account",
                "contract_id",
                "function",
                "sequence_number",
                "args",
                "expect",
            ],
        ) {
            return Err(invalid(message));
        }

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
        if let Some(message) =
            canary_fixtures::unknown_body_keys(expect, &["kind", "message_contains"])
        {
            return Err(invalid(format!("[expect]: {message}")));
        }
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
    parse_arg_at(entry, invalid, 0)
}

/// Parses one `{ kind, value }` entry; `depth` is how many `vec`/`map`
/// containers enclose it.
fn parse_arg_at(
    entry: &toml::Value,
    invalid: &impl Fn(String) -> SorobanFixtureError,
    depth: usize,
) -> Result<ScValInput, SorobanFixtureError> {
    let table = entry
        .as_table()
        .ok_or_else(|| invalid("each [[args]] entry must be a table".to_string()))?;
    if let Some(message) = canary_fixtures::unknown_body_keys(table, &["kind", "value"]) {
        return Err(invalid(format!("[[args]] entry: {message}")));
    }
    let kind = table
        .get("kind")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| invalid("arg entry missing \"kind\"".to_string()))?;

    if kind == "void" {
        // Void carries no payload, so no `value` key is required; any value
        // that is present carries no meaning and is ignored.
        return Ok(ScValInput::Void);
    }

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
        "i128" => value
            .as_str()
            .map(|s| ScValInput::I128(s.to_string()))
            .ok_or_else(|| {
                invalid(
                    "i128 arg value must be a decimal or 0x-prefixed hex string".to_string(),
                )
            }),
        "u128" => value
            .as_str()
            .map(|s| ScValInput::U128(s.to_string()))
            .ok_or_else(|| {
                invalid(
                    "u128 arg value must be a decimal or 0x-prefixed hex string".to_string(),
                )
            }),
        "i256" => value
            .as_str()
            .map(|s| ScValInput::I256(s.to_string()))
            .ok_or_else(|| {
                invalid(
                    "i256 arg value must be a decimal or 0x-prefixed hex string".to_string(),
                )
            }),
        "u256" => value
            .as_str()
            .map(|s| ScValInput::U256(s.to_string()))
            .ok_or_else(|| {
                invalid(
                    "u256 arg value must be a decimal or 0x-prefixed hex string".to_string(),
                )
            }),
        "duration" => value
            .as_integer()
            .and_then(|i| u64::try_from(i).ok())
            .map(ScValInput::Duration)
            .ok_or_else(|| {
                invalid("duration arg value must be a non-negative integer".to_string())
            }),
        "timepoint" => value
            .as_integer()
            .and_then(|i| u64::try_from(i).ok())
            .map(ScValInput::Timepoint)
            .ok_or_else(|| {
                invalid("timepoint arg value must be a non-negative integer".to_string())
            }),
        "symbol" => value
            .as_str()
            .map(|s| ScValInput::Symbol(s.to_string()))
            .ok_or_else(|| invalid("symbol arg value must be a string".to_string())),
        "string" => value
            .as_str()
            .map(|s| ScValInput::String(s.to_string()))
            .ok_or_else(|| invalid("string arg value must be a string".to_string())),
        "bytes" => value
            .as_str()
            .map(|s| ScValInput::Bytes(s.to_string()))
            .ok_or_else(|| {
                invalid(
                    "bytes arg value must be a 0x-prefixed hex string or a base64 string"
                        .to_string(),
                )
            }),
        "address" => value
            .as_str()
            .map(|s| ScValInput::Address(s.to_string()))
            .ok_or_else(|| {
                invalid("address arg value must be a G... or C... strkey string".to_string())
            }),
        "vec" => {
            check_depth(depth, invalid)?;
            let items = value.as_array().ok_or_else(|| {
                invalid("vec arg value must be an array of { kind, value } tables".to_string())
            })?;
            items
                .iter()
                .map(|item| parse_arg_at(item, invalid, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(ScValInput::Vec)
        }
        "map" => {
            check_depth(depth, invalid)?;
            let items = value.as_array().ok_or_else(|| {
                invalid(
                    "map arg value must be an array of { key, value } tables, each holding { kind, value } entries"
                        .to_string(),
                )
            })?;
            let mut entries = Vec::with_capacity(items.len());
            for item in items {
                let pair = item
                    .as_table()
                    .ok_or_else(|| invalid("each map entry must be a table".to_string()))?;
                if let Some(message) = canary_fixtures::unknown_body_keys(pair, &["key", "value"]) {
                    return Err(invalid(format!("map entry: {message}")));
                }
                let key = pair
                    .get("key")
                    .ok_or_else(|| invalid("map entry missing \"key\"".to_string()))?;
                let val = pair
                    .get("value")
                    .ok_or_else(|| invalid("map entry missing \"value\"".to_string()))?;
                entries.push((
                    parse_arg_at(key, invalid, depth + 1)?,
                    parse_arg_at(val, invalid, depth + 1)?,
                ));
            }
            Ok(ScValInput::Map(entries))
        }
        other => Err(invalid(format!(
            "unsupported arg kind {other:?}: expected one of \"bool\", \"u32\", \"i32\", \"u64\", \"i64\", \"i128\", \"u128\", \"i256\", \"u256\", \"void\", \"duration\", \"timepoint\", \"symbol\", \"string\", \"bytes\", \"address\", \"vec\", \"map\""
        ))),
    }
}

/// Rejects a `vec`/`map` that would nest deeper than [`MAX_ARG_DEPTH`].
fn check_depth(
    depth: usize,
    invalid: &impl Fn(String) -> SorobanFixtureError,
) -> Result<(), SorobanFixtureError> {
    if depth + 1 > MAX_ARG_DEPTH {
        return Err(invalid(format!(
            "vec/map arguments are nested deeper than the limit of {MAX_ARG_DEPTH} levels"
        )));
    }
    Ok(())
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
    /// Creates a new [`DefaultSorobanRunner`] that uses `client` to call
    /// `simulateTransaction` on the Stellar RPC.
    ///
    /// The runner stores `client` and reuses it for every [`SorobanFixture`]
    /// it is asked to [`run`](SorobanRunner::run). No network I/O happens at
    /// construction time.
    ///
    /// # Parameters
    ///
    /// - `client` — any value that implements [`RpcClient`]. In production
    ///   this will normally be [`canary_rpc::HttpRpcClient`]; in tests it can
    ///   be a mock or the wiremock-backed [`canary_rpc::HttpRpcClient`] pointing at a
    ///   local server.
    ///
    /// # Panics
    ///
    /// This constructor never panics.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use canary_rpc::HttpRpcClient;
    /// use canary_soroban::runner::DefaultSorobanRunner;
    ///
    /// let client = HttpRpcClient::new("https://soroban-testnet.stellar.org");
    /// let runner = DefaultSorobanRunner::new(client);
    /// ```
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
            source: canary_core::ResultSource::Live,
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

    #[tokio::test]
    async fn fails_when_simulation_unexpectedly_succeeds() {
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
            SorobanFixture::from_loaded(&fixture("p28-soroban-4", "simulation-error", "")).unwrap();
        let runner = DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Fail);
        assert_eq!(
            result.summary,
            "expected simulation to fail, but it succeeded"
        );
    }

    #[tokio::test]
    async fn fails_when_simulation_error_does_not_match_expected_substring() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "latestLedger": 1000, "error": "Error(Budget, ExceededLimit)" }
            })))
            .mount(&server)
            .await;

        let fixture = SorobanFixture::from_loaded(&fixture(
            "p28-soroban-5",
            "simulation-error",
            "message_contains = \"Contract\"\n",
        ))
        .unwrap();
        let runner = DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Fail);
    }

    #[tokio::test]
    async fn a_transport_failure_produces_an_error_status_not_a_fail() {
        // Port 0 on loopback has no listener, so the simulateTransaction
        // POST fails at the transport level before any JSON-RPC exchange.
        let runner = DefaultSorobanRunner::new(HttpRpcClient::new("http://127.0.0.1:0"));
        let fixture =
            SorobanFixture::from_loaded(&fixture("p28-soroban-6", "simulation-success", ""))
                .unwrap();
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Error);
        assert_eq!(result.summary, "failed to call simulateTransaction");
        assert!(result.details.is_some());
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

    /// Parses a fixture whose single `[[args]]` entry has the given
    /// `kind`, including a raw TOML `value = ...` line only when `value`
    /// is `Some` (the `void` kind carries no value).
    fn from_loaded_with_kind(
        kind: &str,
        value: Option<&str>,
    ) -> Result<SorobanFixture, SorobanFixtureError> {
        let value_line = value
            .map(|raw| format!("value = {raw}\n"))
            .unwrap_or_default();
        let toml = format!(
            "id = \"bad-arg\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"x\"\ndescription = \"x\"\nsource_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"f\"\nsequence_number = 1\n\n[[args]]\nkind = \"{kind}\"\n{value_line}\n[expect]\nkind = \"simulation-success\"\n",
            StrkeyPublicKey([0u8; 32]),
            StrkeyContract([0u8; 32]),
        );
        let loaded =
            canary_fixtures::parse_fixture_str(&toml, std::path::Path::new("t.toml")).unwrap();
        SorobanFixture::from_loaded(&loaded)
    }

    /// Parses a fixture whose single `[[args]]` entry has the given
    /// `kind` and raw TOML `value`.
    fn from_loaded_with_arg(
        kind: &str,
        value: &str,
    ) -> Result<SorobanFixture, SorobanFixtureError> {
        from_loaded_with_kind(kind, Some(value))
    }

    #[test]
    fn rejects_a_negative_u32_arg() {
        let err = from_loaded_with_arg("u32", "-1").unwrap_err();
        assert!(matches!(
            err,
            SorobanFixtureError::InvalidFixtureBody { .. }
        ));
    }

    #[test]
    fn rejects_a_string_value_for_a_bool_arg() {
        let err = from_loaded_with_arg("bool", "\"true\"").unwrap_err();
        assert!(matches!(
            err,
            SorobanFixtureError::InvalidFixtureBody { .. }
        ));
    }

    #[test]
    fn parses_bytes_and_address_args() {
        let fixture = from_loaded_with_arg("bytes", "\"0xdeadbeef\"").unwrap();
        assert_eq!(
            fixture.invocation.args,
            vec![ScValInput::Bytes("0xdeadbeef".to_string())]
        );
        let fixture = from_loaded_with_arg("address", "\"GABC\"").unwrap();
        assert_eq!(
            fixture.invocation.args,
            vec![ScValInput::Address("GABC".to_string())]
        );
        assert!(from_loaded_with_arg("bytes", "7").is_err());
        assert!(from_loaded_with_arg("address", "true").is_err());
    }

    #[test]
    fn parses_nested_vec_and_map_args() {
        let fixture = from_loaded_with_arg(
            "vec",
            "[{ kind = \"u32\", value = 1 }, { kind = \"vec\", value = [{ kind = \"bool\", value = true }] }]",
        )
        .unwrap();
        assert_eq!(
            fixture.invocation.args,
            vec![ScValInput::Vec(vec![
                ScValInput::U32(1),
                ScValInput::Vec(vec![ScValInput::Bool(true)]),
            ])]
        );

        let fixture = from_loaded_with_arg(
            "map",
            "[{ key = { kind = \"symbol\", value = \"a\" }, value = { kind = \"u32\", value = 1 } }]",
        )
        .unwrap();
        assert_eq!(
            fixture.invocation.args,
            vec![ScValInput::Map(vec![(
                ScValInput::Symbol("a".to_string()),
                ScValInput::U32(1)
            )])]
        );
    }

    #[test]
    fn rejects_malformed_vec_and_map_args() {
        assert!(from_loaded_with_arg("vec", "1").is_err());
        assert!(from_loaded_with_arg("map", "[{ key = { kind = \"u32\", value = 1 } }]").is_err());
        assert!(from_loaded_with_arg(
            "map",
            "[{ key = { kind = \"u32\", value = 1 }, value = { kind = \"u32\", value = 1 }, extra = 1 }]"
        )
        .is_err());
    }

    #[test]
    fn rejects_vec_nesting_deeper_than_the_limit() {
        // Nine nested vecs: one more than MAX_ARG_DEPTH.
        let mut value = "[]".to_string();
        for _ in 0..MAX_ARG_DEPTH {
            value = format!("[{{ kind = \"vec\", value = {value} }}]");
        }
        let err = from_loaded_with_arg("vec", &value).unwrap_err();
        assert!(err.to_string().contains("nested deeper"), "{err}");

        // Eight nested vecs are accepted.
        let mut value = "[]".to_string();
        for _ in 0..MAX_ARG_DEPTH - 1 {
            value = format!("[{{ kind = \"vec\", value = {value} }}]");
        }
        assert!(from_loaded_with_arg("vec", &value).is_ok());
    }

    #[test]
    fn rejects_an_unsupported_arg_kind_and_lists_the_supported_ones() {
        let err = from_loaded_with_arg("enum", "1").unwrap_err();
        assert!(err.to_string().contains(
            "expected one of \"bool\", \"u32\", \"i32\", \"u64\", \"i64\", \"i128\", \"u128\", \"i256\", \"u256\", \"void\", \"duration\", \"timepoint\", \"symbol\", \"string\""
        ));
    }

    #[test]
    fn parses_wide_integer_string_args() {
        let fixture = from_loaded_with_arg("i128", "\"-42\"").unwrap();
        assert_eq!(
            fixture.invocation.args,
            vec![ScValInput::I128("-42".to_string())]
        );

        let fixture = from_loaded_with_arg("u128", "\"0xff\"").unwrap();
        assert_eq!(
            fixture.invocation.args,
            vec![ScValInput::U128("0xff".to_string())]
        );

        let fixture = from_loaded_with_arg("i256", "\"-1\"").unwrap();
        assert_eq!(
            fixture.invocation.args,
            vec![ScValInput::I256("-1".to_string())]
        );

        let fixture = from_loaded_with_arg(
            "u256",
            "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"",
        )
        .unwrap();
        assert_eq!(fixture.invocation.args.len(), 1);
        let ScValInput::U256(value) = &fixture.invocation.args[0] else {
            panic!("expected a u256 arg");
        };
        assert!(value.starts_with("1157920892373161"));
    }

    #[test]
    fn parses_a_void_arg_without_a_value() {
        let fixture = from_loaded_with_kind("void", None).unwrap();
        assert_eq!(fixture.invocation.args, vec![ScValInput::Void]);
    }

    #[test]
    fn parses_duration_and_timepoint_args() {
        let fixture = from_loaded_with_arg("duration", "300").unwrap();
        assert_eq!(fixture.invocation.args, vec![ScValInput::Duration(300)]);

        let fixture = from_loaded_with_arg("timepoint", "1700000000").unwrap();
        assert_eq!(
            fixture.invocation.args,
            vec![ScValInput::Timepoint(1_700_000_000)]
        );
    }

    #[test]
    fn rejects_a_toml_integer_for_a_wide_integer_string_arg() {
        let err = from_loaded_with_arg("i128", "1").unwrap_err();
        assert!(err
            .to_string()
            .contains("i128 arg value must be a decimal or 0x-prefixed hex string"));
        assert!(matches!(
            err,
            SorobanFixtureError::InvalidFixtureBody { .. }
        ));
    }

    #[test]
    fn rejects_a_negative_duration_arg() {
        let err = from_loaded_with_arg("duration", "-1").unwrap_err();
        assert!(err
            .to_string()
            .contains("duration arg value must be a non-negative integer"));
    }

    #[test]
    fn rejects_a_missing_value_for_a_typed_arg() {
        let err = from_loaded_with_kind("u32", None).unwrap_err();
        assert!(err.to_string().contains("arg entry missing \"value\""));
    }
}
