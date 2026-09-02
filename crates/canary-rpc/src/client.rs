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

    #[error("the RPC endpoint's network passphrase ({actual:?}) does not match the expected passphrase ({expected:?})")]
    NetworkMismatch { expected: String, actual: String },

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
pub struct HttpRpcClient {
    http: reqwest::Client,
    endpoint: String,
    retry_policy: RetryPolicy,
}

impl HttpRpcClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        HttpRpcClient {
            http: reqwest::Client::new(),
            endpoint: endpoint.into(),
            retry_policy: RetryPolicy::default(),
        }
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
}
