//! A minimal Stellar RPC (JSON-RPC 2.0 over HTTP) client.
//!
//! `stellar-rpc-client` on crates.io is, at the time this was written, only
//! published as an unstable `28.0.0-rc.1` release coupled to the much
//! larger `stellar-cli` workspace with a materially higher MSRV (1.93.0
//! vs. `stellar-xdr`'s 1.84.0). Rather than pull that dependency graph in
//! for three JSON-RPC methods, this client talks to the documented,
//! stable Stellar RPC JSON-RPC methods directly; XDR-bearing fields are
//! passed through as base64 strings and decoded by callers using the
//! official `stellar-xdr` crate (via `canary-xdr`), so no XDR parsing is
//! reimplemented here.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

use canary_core::CanaryError;

use crate::models::{LatestLedger, NetworkInfo, SimulationRequest, SimulationResponse};

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("network transport error calling {method}: {message}")]
    Transport { method: String, message: String },

    #[error("timed out calling {method} after {attempts} attempt(s)")]
    Timeout { method: String, attempts: u32 },

    #[error("invalid JSON response from {method}: {message}")]
    InvalidJson { method: String, message: String },

    #[error("RPC {method} returned a JSON-RPC error (code {code}): {message}")]
    JsonRpcError {
        method: String,
        code: i64,
        message: String,
    },

    #[error("unexpected response shape from {method}: {reason}")]
    InvalidResponse { method: String, reason: String },

    /// The endpoint is on a different network than the run requested.
    ///
    /// Raised by [`validate_network_info`], never by the transport layer:
    /// `getNetwork` succeeds against *any* network, so only a comparison
    /// against the run's expected passphrase can produce this. Callers
    /// decide what it means for their run (the CLI aborts as a
    /// configuration error, since results from the wrong network must not
    /// be attributed to the requested one).
    #[error("the RPC endpoint's network passphrase ({actual:?}) does not match the expected passphrase ({expected:?})")]
    NetworkMismatch { expected: String, actual: String },

    /// The endpoint reports a different protocol version than the run
    /// targets.
    ///
    /// Raised by [`validate_network_info`], never by the transport layer.
    /// A mismatch here is not necessarily a mistake — rehearsing an
    /// upcoming protocol against a not-yet-upgraded network is a core use
    /// case — so callers decide whether to treat it as a warning (the CLI
    /// does) or a failure.
    #[error(
        "the RPC endpoint reports protocol {observed}, but this run targets protocol {target}"
    )]
    ProtocolMismatch { target: u32, observed: u32 },

    #[error("RPC endpoint rate-limited {method} after {attempts} attempt(s)")]
    RateLimited { method: String, attempts: u32 },
}

impl From<RpcError> for CanaryError {
    fn from(error: RpcError) -> Self {
        CanaryError::Rpc(error.to_string())
    }
}

/// Checks a freshly fetched [`NetworkInfo`] against the run's expected
/// network passphrase and target protocol, constructing
/// [`RpcError::NetworkMismatch`]/[`RpcError::ProtocolMismatch`] when they
/// differ.
///
/// This is the intended (and only) constructor of those two variants: the
/// transport-level `getNetwork` call itself only fails on wire/JSON
/// errors, while these two capture a *semantic* mismatch between what the
/// endpoint reports and what the run assumed.
///
/// - The passphrase comparison is skipped when `expected_passphrase` is
///   empty: custom networks have no well-known passphrase to compare
///   against (see `canary-cli`'s `default_passphrase`).
/// - The passphrase is checked first — confirming *which* network the
///   endpoint is on before comparing protocol versions.
pub fn validate_network_info(
    info: &NetworkInfo,
    expected_passphrase: &str,
    target_protocol: u32,
) -> Result<(), RpcError> {
    if !expected_passphrase.is_empty() && info.passphrase != expected_passphrase {
        return Err(RpcError::NetworkMismatch {
            expected: expected_passphrase.to_string(),
            actual: info.passphrase.clone(),
        });
    }
    if info.protocol_version != target_protocol {
        return Err(RpcError::ProtocolMismatch {
            target: target_protocol,
            observed: info.protocol_version,
        });
    }
    Ok(())
}

/// The subset of Stellar RPC this project depends on.
pub trait RpcClient {
    fn get_network(
        &self,
    ) -> impl std::future::Future<Output = Result<NetworkInfo, RpcError>> + Send;

    fn get_latest_ledger(
        &self,
    ) -> impl std::future::Future<Output = Result<LatestLedger, RpcError>> + Send;

    fn simulate_transaction(
        &self,
        request: SimulationRequest,
    ) -> impl std::future::Future<Output = Result<SimulationResponse, RpcError>> + Send;
}

/// Bounded retry policy for transient failures.
///
/// Only transport errors, timeouts, and rate limiting are retried;
/// malformed requests, JSON-RPC errors, and invalid responses are
/// deterministic and retrying them would never help.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(200),
        }
    }
}

/// A [`RpcClient`] backed by a real HTTP endpoint.
#[derive(Clone)]
pub struct HttpRpcClient {
    http: reqwest::Client,
    endpoint: String,
    retry_policy: RetryPolicy,
}

impl HttpRpcClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        HttpRpcClient {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            endpoint: endpoint.into(),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        if let Ok(http) = reqwest::Client::builder().timeout(timeout).build() {
            self.http = http;
        }
        self
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    async fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, RpcError> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.try_call::<R>(method, &body).await {
                Ok(value) => return Ok(value),
                Err(err) if is_retryable(&err) && attempt < self.retry_policy.max_attempts => {
                    tokio::time::sleep(self.retry_policy.base_delay * attempt).await;
                }
                Err(RpcError::Timeout { method, .. }) => {
                    return Err(RpcError::Timeout {
                        method,
                        attempts: attempt,
                    })
                }
                Err(RpcError::RateLimited { method, .. }) => {
                    return Err(RpcError::RateLimited {
                        method,
                        attempts: attempt,
                    })
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn try_call<R: DeserializeOwned>(
        &self,
        method: &str,
        body: &serde_json::Value,
    ) -> Result<R, RpcError> {
        let response = self
            .http
            .post(&self.endpoint)
            .json(body)
            .send()
            .await
            .map_err(|e| classify_reqwest_error(method, &e))?;

        if response.status().as_u16() == 429 {
            return Err(RpcError::RateLimited {
                method: method.to_string(),
                attempts: 0,
            });
        }
        if response.status().is_server_error() {
            return Err(RpcError::Transport {
                method: method.to_string(),
                message: format!("server returned status {}", response.status()),
            });
        }

        let text = response.text().await.map_err(|e| RpcError::Transport {
            method: method.to_string(),
            message: e.to_string(),
        })?;

        let envelope: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| RpcError::InvalidJson {
                method: method.to_string(),
                message: e.to_string(),
            })?;

        if let Some(error) = envelope.get("error") {
            let code = error
                .get("code")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let message = error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown error")
                .to_string();
            return Err(RpcError::JsonRpcError {
                method: method.to_string(),
                code,
                message,
            });
        }

        let result = envelope
            .get("result")
            .ok_or_else(|| RpcError::InvalidResponse {
                method: method.to_string(),
                reason: "response has neither \"result\" nor \"error\"".to_string(),
            })?;

        serde_json::from_value(result.clone()).map_err(|e| RpcError::InvalidResponse {
            method: method.to_string(),
            reason: e.to_string(),
        })
    }
}

fn classify_reqwest_error(method: &str, error: &reqwest::Error) -> RpcError {
    if error.is_timeout() {
        RpcError::Timeout {
            method: method.to_string(),
            attempts: 0,
        }
    } else {
        RpcError::Transport {
            method: method.to_string(),
            message: error.to_string(),
        }
    }
}

fn is_retryable(error: &RpcError) -> bool {
    matches!(
        error,
        RpcError::Transport { .. } | RpcError::Timeout { .. } | RpcError::RateLimited { .. }
    )
}

impl RpcClient for HttpRpcClient {
    async fn get_network(&self) -> Result<NetworkInfo, RpcError> {
        self.call("getNetwork", json!({})).await
    }

    async fn get_latest_ledger(&self) -> Result<LatestLedger, RpcError> {
        self.call("getLatestLedger", json!({})).await
    }

    async fn simulate_transaction(
        &self,
        request: SimulationRequest,
    ) -> Result<SimulationResponse, RpcError> {
        self.call("simulateTransaction", request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn get_network_parses_a_successful_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "passphrase": "Test SDF Network ; September 2015",
                    "protocolVersion": 28
                }
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let info = client.get_network().await.expect("ok");
        assert_eq!(info.protocol_version, 28);
    }

    #[tokio::test]
    async fn maps_a_json_rpc_error_object_to_json_rpc_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": { "code": -32602, "message": "invalid params" }
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let err = client.get_network().await.unwrap_err();
        assert!(matches!(err, RpcError::JsonRpcError { code: -32602, .. }));
    }

    #[tokio::test]
    async fn maps_malformed_json_to_invalid_json_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let err = client.get_network().await.unwrap_err();
        assert!(matches!(err, RpcError::InvalidJson { .. }));
    }

    #[tokio::test]
    async fn retries_server_errors_up_to_the_configured_attempt_limit() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri()).with_retry_policy(RetryPolicy {
            max_attempts: 2,
            base_delay: Duration::from_millis(1),
        });
        let err = client.get_network().await.unwrap_err();
        assert!(matches!(err, RpcError::Transport { .. }));
    }

    #[tokio::test]
    async fn custom_max_attempts_one_disables_retries() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri()).with_retry_policy(RetryPolicy {
            max_attempts: 1,
            base_delay: Duration::from_millis(1),
        });
        let err = client.get_network().await.unwrap_err();
        assert!(matches!(err, RpcError::Transport { .. }));

        let requests = server.received_requests().await.expect("requests");
        assert_eq!(requests.len(), 1);
    }

    #[tokio::test]
    async fn simulate_transaction_reports_a_host_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "latestLedger": 1000,
                    "error": "host invocation failed"
                }
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let response = client
            .simulate_transaction(SimulationRequest {
                transaction: "AAAA".to_string(),
            })
            .await
            .expect("ok");
        assert!(!response.succeeded());
    }

    fn network_info(passphrase: &str, protocol_version: u32) -> NetworkInfo {
        NetworkInfo {
            friendbot_url: None,
            passphrase: passphrase.to_string(),
            protocol_version,
        }
    }

    #[test]
    fn validate_network_info_accepts_a_matching_passphrase_and_protocol() {
        let info = network_info("Test SDF Network ; September 2015", 28);
        let result = validate_network_info(&info, "Test SDF Network ; September 2015", 28);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_network_info_constructs_network_mismatch_on_a_wrong_passphrase() {
        let info = network_info("Public Global Stellar Network ; September 2015", 28);
        let err = validate_network_info(&info, "Test SDF Network ; September 2015", 28)
            .expect_err("passphrase differs");
        assert!(matches!(
            err,
            RpcError::NetworkMismatch { ref expected, ref actual }
                if expected == "Test SDF Network ; September 2015"
                    && actual == "Public Global Stellar Network ; September 2015"
        ));
    }

    #[test]
    fn validate_network_info_constructs_protocol_mismatch_on_a_different_protocol() {
        let info = network_info("Test SDF Network ; September 2015", 27);
        let err = validate_network_info(&info, "Test SDF Network ; September 2015", 28)
            .expect_err("protocol differs");
        assert!(matches!(
            err,
            RpcError::ProtocolMismatch {
                target: 28,
                observed: 27
            }
        ));
    }

    #[test]
    fn validate_network_info_checks_the_passphrase_before_the_protocol() {
        let info = network_info("Public Global Stellar Network ; September 2015", 27);
        let err = validate_network_info(&info, "Test SDF Network ; September 2015", 28)
            .expect_err("both differ");
        assert!(matches!(err, RpcError::NetworkMismatch { .. }));
    }

    #[test]
    fn validate_network_info_skips_the_passphrase_check_for_custom_networks() {
        // An empty expected passphrase means "no well-known passphrase"
        // (custom networks): only the protocol is compared.
        let info = network_info("Standalone Network ; February 2017", 28);
        assert!(validate_network_info(&info, "", 28).is_ok());

        let err = validate_network_info(&info, "", 27).expect_err("protocol differs");
        assert!(matches!(err, RpcError::ProtocolMismatch { .. }));
    }
}
