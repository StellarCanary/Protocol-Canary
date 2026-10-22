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

use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

use canary_core::CanaryError;

use crate::models::{
    EventFilter, EventResponse, FeeStatsResponse, GetHealthResponse, GetLedgerEntriesResponse,
    GetLedgersResponse, GetTransactionsResponse, LatestLedger, NetworkInfo, SimulationRequest,
    SimulationResponse, VersionInfoResponse,
};

/// Errors returned by the Stellar RPC client.
///
/// Every fallible operation in this crate — the [`RpcClient`] trait
/// (implemented by [`HttpRpcClient`]) and [`validate_network_info`] — reports
/// failure as this type. Callers never have to expect a panic from a bad
/// endpoint, an unreachable host, or a malformed response: all of those come
/// back as one of the variants below.
///
/// The variants fall into three groups:
///
/// - **Transport-level** ([`RpcError::Transport`], [`RpcError::Timeout`],
///   [`RpcError::RateLimited`]): the request could not be completed.
///   [`HttpRpcClient`] retries these according to the configured
///   [`RetryPolicy`] and only surfaces them once the attempts are exhausted,
///   at which point `attempts` holds how many attempts were made.
/// - **Protocol-level** ([`RpcError::InvalidJson`],
///   [`RpcError::JsonRpcError`], [`RpcError::MethodNotAvailable`],
///   [`RpcError::InvalidResponse`], [`RpcError::ResponseTooLarge`]): the
///   endpoint answered, but the answer is unusable — not JSON, a JSON-RPC
///   `error` object, a `result` that does not deserialize into the
///   expected type, or a body larger than [`MAX_RESPONSE_BYTES`]. These are
///   deterministic, so retrying them would never help and they are returned
///   on the first attempt.
/// - **Semantic** ([`RpcError::NetworkMismatch`],
///   [`RpcError::ProtocolMismatch`]): the endpoint answered correctly but
///   reports a different network passphrase or protocol version than the run
///   targets. Only [`validate_network_info`] constructs these; see its docs
///   for how callers are expected to interpret a mismatch.
///
/// - **Local validation** ([`RpcError::InvalidArgument`]): the arguments
///   cannot be sent, so no request was made. Deterministic, and returned
///   before the transport is involved.
///
/// The [`std::fmt::Display`] implementation (via `thiserror`) always names
/// the RPC method involved, so the rendered message is safe to log or surface
/// directly. `RpcError` also converts into
/// [`CanaryError`] via `?` for propagation through
/// the rest of the workspace.
///
/// # Examples
///
/// Branch on the variant when the caller needs to react differently, and fall
/// back on the rendered message otherwise:
///
/// ```
/// use canary_rpc::RpcError;
///
/// let err = RpcError::JsonRpcError {
///     method: "simulateTransaction".to_string(),
///     code: -32602,
///     message: "invalid params".to_string(),
/// };
///
/// // Deterministic protocol errors are reported verbatim...
/// assert!(err.to_string().contains("code -32602"));
///
/// // ...while transport-level ones are the only ones worth retrying.
/// let transient = RpcError::RateLimited {
///     method: "getLatestLedger".to_string(),
///     attempts: 3,
/// };
/// assert!(transient.to_string().contains("3 attempt(s)"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    /// The HTTP request itself failed: connection refused or reset, DNS or
    /// TLS failure, or the endpoint answering with a 5xx status.
    ///
    /// Transient by nature, so [`HttpRpcClient`] retries it under the
    /// configured [`RetryPolicy`]. `message` carries the underlying transport
    /// detail (or the status code) for diagnosis.
    #[error("network transport error calling {method}: {message}")]
    Transport { method: String, message: String },

    /// The request exceeded the client's timeout (see
    /// [`HttpRpcClient::with_timeout`]).
    ///
    /// Transient, so it is retried under [`RetryPolicy`]; `attempts` records
    /// how many attempts were made before giving up.
    #[error("timed out calling {method} after {attempts} attempt(s)")]
    Timeout { method: String, attempts: u32 },

    /// The endpoint returned a body that is not valid JSON — typically an
    /// HTML error page from a proxy or a truncated response.
    ///
    /// Never retried: `message` holds the serde parse error.
    #[error("invalid JSON response from {method}: {message}")]
    InvalidJson { method: String, message: String },

    /// The endpoint processed the request and returned a JSON-RPC `error`
    /// object, i.e. a protocol-level rejection such as `invalid params`
    /// (`-32602`).
    ///
    /// `code` and `message` are copied from that object; fields missing from
    /// the error object fall back to `0` and `"unknown error"`. Never
    /// retried.
    #[error("RPC {method} returned a JSON-RPC error (code {code}): {message}")]
    JsonRpcError {
        method: String,
        code: i64,
        message: String,
    },

    /// The endpoint reported that the requested JSON-RPC method does not
    /// exist (error code `-32601`, "Method not found").
    ///
    /// Kept distinct from [`RpcError::JsonRpcError`] so callers can tell an
    /// optional or unsupported method apart from a genuine call failure: the
    /// endpoint is reachable and answered correctly, it simply does not
    /// implement this method. `method` preserves the method name for the
    /// diagnostic. Never retried, and never a transport-level failure.
    #[error("RPC method {method} is not available on this endpoint (JSON-RPC error code -32601)")]
    MethodNotAvailable { method: String },

    /// The response parsed as JSON but is not what the method should return:
    /// either it has neither a `result` nor an `error` key, or its `result`
    /// failed to deserialize into the expected type.
    ///
    /// `reason` carries the structural or serde explanation. Never retried.
    #[error("unexpected response shape from {method}: {reason}")]
    InvalidResponse { method: String, reason: String },

    /// An argument the caller passed cannot be sent to the method, so no
    /// request was made.
    ///
    /// Raised before the wire, by the typed method that validates its own
    /// parameters (for example `getLedgers`' `startLedger`, which must be a
    /// real ledger sequence). `reason` names the offending value and the
    /// accepted range. Deterministic, so it is never retried — and because
    /// nothing was sent, retrying would not help even for a transient
    /// failure.
    #[error("invalid argument for {method}: {reason}")]
    InvalidArgument { method: String, reason: String },

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

    /// The endpoint rejected the request with HTTP 429 (Too Many Requests).
    ///
    /// Transient, so it is retried under [`RetryPolicy`]; `attempts` records
    /// how many attempts were made before giving up.
    #[error("RPC endpoint rate-limited {method} after {attempts} attempt(s)")]
    RateLimited { method: String, attempts: u32 },

    /// The response body is larger than [`MAX_RESPONSE_BYTES`].
    ///
    /// Raised before the body is buffered — from the declared
    /// `Content-Length` when the endpoint sends one, otherwise as soon as
    /// the streamed body passes the cap — and the connection is dropped at
    /// that point rather than read to completion, so a runaway or
    /// malicious endpoint cannot exhaust this process's memory (D-06).
    ///
    /// Deterministic for the same response, so it is not retried. `limit`
    /// echoes the cap that was exceeded, in bytes, for the diagnostic.
    #[error("response body from {method} exceeds the {limit}-byte size cap and was not read")]
    ResponseTooLarge { method: String, limit: usize },
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
///   against (see `canary-cli`'s `default_passphrase`), and nothing was
///   supplied for them. Callers that accept an explicit passphrase —
///   `stellar-canary check --network-passphrase <PASSPHRASE>` rejects an
///   empty value at argument-parse time precisely so that flag cannot
///   reach here as `""` and silently disable this check — pass it
///   through as-is: a non-empty custom passphrase is compared exactly
///   like a well-known one.
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

/// Hard cap on the number of bytes read from one RPC response body
/// (16 MiB, approved under D-06).
///
/// It is far above the largest response the project has observed (a
/// Mainnet `getLatestLedger` with its `metadataXdr` is about 1.3 MB) and
/// far below anything that would exhaust memory, so it only ever rejects
/// a runaway or hostile endpoint. Exceeding it is
/// [`RpcError::ResponseTooLarge`], never a truncated or partially parsed
/// body.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// The subset of Stellar RPC this project depends on.
pub trait RpcClient {
    fn get_network(
        &self,
    ) -> impl std::future::Future<Output = Result<NetworkInfo, RpcError>> + Send;

    fn get_latest_ledger(
        &self,
    ) -> impl std::future::Future<Output = Result<LatestLedger, RpcError>> + Send;

    fn get_health(
        &self,
    ) -> impl std::future::Future<Output = Result<GetHealthResponse, RpcError>> + Send;

    /// `getVersionInfo`, the endpoint's RPC and protocol build information.
    ///
    /// The returned [`VersionInfoResponse::observed_protocol`] is the
    /// protocol version the endpoint itself reports, which may differ from
    /// the network's target protocol (see [`validate_network_info`]).
    fn get_version_info(
        &self,
    ) -> impl std::future::Future<Output = Result<VersionInfoResponse, RpcError>> + Send;

    /// `getFeeStats`, the current inclusion-fee distributions.
    fn get_fee_stats(
        &self,
    ) -> impl std::future::Future<Output = Result<FeeStatsResponse, RpcError>> + Send;

    /// `getLedgerEntries` for the requested `LedgerKey` XDR values.
    ///
    /// Each key is a base64-encoded `LedgerKey`. At least one key and at
    /// most 200 keys are accepted; a key that is not standard-alphabet
    /// base64 is rejected with [`RpcError::InvalidArgument`] before a
    /// request is sent, because the endpoint would otherwise report the
    /// malformed key as a protocol-level error that looks like a failed
    /// check.
    fn get_ledger_entries(
        &self,
        keys: &[String],
    ) -> impl std::future::Future<Output = Result<GetLedgerEntriesResponse, RpcError>> + Send;

    /// `getNetwork`'s JSON `result` object exactly as the endpoint returned
    /// it, including every field [`NetworkInfo`] does not model.
    ///
    /// Fixture assertions run against this rather than the typed projection
    /// so that a field the model drops can still be asserted on, and so that
    /// `field-absent` evaluates truthfully (CA-01 section 4.1). A `result`
    /// that is not a JSON object is an [`RpcError::InvalidResponse`].
    fn get_network_raw(
        &self,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, RpcError>> + Send;

    /// `getLatestLedger`'s JSON `result` object exactly as the endpoint
    /// returned it. See [`RpcClient::get_network_raw`].
    fn get_latest_ledger_raw(
        &self,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, RpcError>> + Send;

    /// `getHealth`'s JSON `result` object exactly as the endpoint
    /// returned it. See [`RpcClient::get_network_raw`].
    fn get_health_raw(
        &self,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, RpcError>> + Send;

    /// `getVersionInfo`'s JSON `result` object exactly as the endpoint
    /// returned it. See [`RpcClient::get_network_raw`].
    fn get_version_info_raw(
        &self,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, RpcError>> + Send;

    /// `getFeeStats`'s JSON `result` object exactly as the endpoint
    /// returned it. See [`RpcClient::get_network_raw`].
    fn get_fee_stats_raw(
        &self,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, RpcError>> + Send;

    /// `getLedgerEntries`'s JSON `result` object exactly as the endpoint
    /// returned it, for the request parameters in `params`. See
    /// [`RpcClient::get_network_raw`].
    fn get_ledger_entries_raw(
        &self,
        params: serde_json::Value,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, RpcError>> + Send;

    fn simulate_transaction(
        &self,
        request: SimulationRequest,
    ) -> impl std::future::Future<Output = Result<SimulationResponse, RpcError>> + Send;

    fn get_events(
        &self,
        start_ledger: u32,
        filters: &[EventFilter],
    ) -> impl std::future::Future<Output = Result<EventResponse, RpcError>> + Send;

    /// `getLedgers` for the ledgers from `start_ledger` onward.
    ///
    /// `start_ledger` is a real ledger sequence and must be greater than
    /// zero: ledger 0 does not exist, so it is rejected with
    /// [`RpcError::InvalidArgument`] before a request is sent. A negative
    /// value cannot be expressed — the parameter is a `u32`.
    ///
    /// `limit` bounds the page size and is sent as
    /// `pagination.limit`; `None` omits the `pagination` object altogether
    /// so the endpoint applies its own default. A `limit` outside the
    /// upstream range `1..=200` is also [`RpcError::InvalidArgument`].
    fn get_ledgers(
        &self,
        start_ledger: u32,
        limit: Option<u32>,
    ) -> impl std::future::Future<Output = Result<GetLedgersResponse, RpcError>> + Send;

    /// `getLedgers`'s JSON `result` object exactly as the endpoint returned
    /// it, for the request parameters in `params`. See
    /// [`RpcClient::get_network_raw`]: the runner asserts on the raw object
    /// so a field the typed model does not name stays assertable.
    fn get_ledgers_raw(
        &self,
        params: serde_json::Value,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, RpcError>> + Send;

    /// `getEvents`'s JSON `result` object exactly as the endpoint returned
    /// it, for the request parameters in `params`. See
    /// [`RpcClient::get_network_raw`]: the runner asserts on the raw object so
    /// an untyped event payload stays intact.
    fn get_events_raw(
        &self,
        params: serde_json::Value,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, RpcError>> + Send;

    fn get_transactions(
        &self,
        start_ledger: u32,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> impl std::future::Future<Output = Result<GetTransactionsResponse, RpcError>> + Send;
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

impl RetryPolicy {
    /// Builds a policy from a *retry count* rather than an attempt count.
    ///
    /// `max_retries` is how many times a failed attempt may be repeated, so
    /// `max_attempts` becomes `max_retries + 1`: the first attempt is always
    /// made, and everything after it is a retry. `from_retries(0, …)`
    /// therefore yields a policy that issues exactly one request and
    /// surfaces the first network error — what the CLI's
    /// `--rpc-max-retries 0` promises. The count saturates instead of
    /// overflowing at `u32::MAX`.
    ///
    /// `base_delay` is the linear-backoff base: attempt *n* waits `n *
    /// base_delay` before the next attempt (see [`HttpRpcClient::with_retry_policy`]
    /// for the timing model).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use canary_rpc::RetryPolicy;
    ///
    /// // `--rpc-max-retries 3 --rpc-retry-delay-ms 500`: one attempt plus
    /// // three retries, 500ms base delay.
    /// let policy = RetryPolicy::from_retries(3, Duration::from_millis(500));
    /// assert_eq!(policy.max_attempts, 4);
    /// assert_eq!(policy.base_delay, Duration::from_millis(500));
    ///
    /// // `--rpc-max-retries 0`: exactly one attempt, no retries.
    /// let single = RetryPolicy::from_retries(0, Duration::from_millis(500));
    /// assert_eq!(single.max_attempts, 1);
    /// ```
    pub fn from_retries(max_retries: u32, base_delay: Duration) -> Self {
        RetryPolicy {
            max_attempts: max_retries.saturating_add(1),
            base_delay,
        }
    }
}

/// A [`RpcClient`] backed by a real HTTP endpoint.
#[derive(Clone)]
pub struct HttpRpcClient {
    http: reqwest::Client,
    endpoint: String,
    retry_policy: RetryPolicy,
    /// When `true`, every RPC attempt is logged to stderr with its attempt
    /// number, total attempt budget, and elapsed time — with the endpoint
    /// URL scrubbed (see [`scrub_url`]). Off by default, so existing
    /// callers' stderr output is unchanged.
    verbose: bool,
}

impl HttpRpcClient {
    /// Creates a client that sends JSON-RPC requests to `endpoint`, for
    /// example `https://soroban-testnet.stellar.org`.
    ///
    /// The endpoint is stored verbatim: it is not resolved or validated
    /// here, so a malformed or unreachable URL does not fail until the
    /// first request, which reports it as a [`RpcError::Transport`]. The
    /// client starts with a 10-second request timeout and the default
    /// `RetryPolicy`; chain [`Self::with_timeout`] or
    /// [`Self::with_retry_policy`] to override either one before making
    /// a call.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use canary_rpc::HttpRpcClient;
    ///
    /// let _client = HttpRpcClient::new("https://soroban-testnet.stellar.org");
    /// ```
    ///
    /// This never panics: if the underlying HTTP client cannot be built
    /// with the default timeout, a default client is used instead.
    pub fn new(endpoint: impl Into<String>) -> Self {
        HttpRpcClient {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            endpoint: endpoint.into(),
            retry_policy: RetryPolicy::default(),
            verbose: false,
        }
    }

    /// Returns this client with its per-request HTTP timeout replaced by
    /// `timeout`.
    ///
    /// Builder-style: this consumes the client and hands back a new one, so
    /// it chains onto [`HttpRpcClient::new`] (and alongside
    /// [`HttpRpcClient::with_retry_policy`]). The timeout is a whole-request
    /// deadline applied by the underlying `reqwest` client, so it covers
    /// connecting, sending, and reading the response, and it bounds every
    /// method on [`RpcClient`] — `getNetwork`, `getLatestLedger`, and
    /// `simulateTransaction` alike. The default set by
    /// [`HttpRpcClient::new`] is 10 seconds; `canary-runner` overrides it
    /// with the run's `--rpc-timeout` option.
    ///
    /// A timeout is not a failure of the compatibility assertion itself.
    /// When the deadline elapses, `reqwest` reports a timeout error, which
    /// this crate maps to [`RpcError::Timeout`]. That is a retryable error,
    /// so a call may still be retried up to
    /// [`RetryPolicy::max_attempts`] times (with a linear
    /// `base_delay * attempt` backoff) before the run sees
    /// [`RpcError::Timeout`] — each attempt gets the full `timeout`, so the
    /// worst case for one call is `timeout * max_attempts` plus backoff.
    /// Callers should therefore treat `Timeout` as "this run could not
    /// execute", not as evidence that the endpoint is incompatible.
    ///
    /// This method does not panic and does not report failure: it rebuilds
    /// the underlying `reqwest` client, and if that build fails for any
    /// reason the previously configured client is kept and `timeout` is
    /// silently ignored — the call then returns a client that still uses
    /// whatever deadline it already had. Note that rebuilding the client
    /// also discards the previous connection pool, so prefer one call with
    /// the final value over repeated calls in a loop.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use canary_rpc::{HttpRpcClient, RetryPolicy};
    ///
    /// // Two seconds per request, up to five attempts.
    /// let _client = HttpRpcClient::new("https://soroban-testnet.stellar.org")
    ///     .with_timeout(Duration::from_secs(2))
    ///     .with_retry_policy(RetryPolicy {
    ///         max_attempts: 5,
    ///         base_delay: Duration::from_millis(200),
    ///     });
    /// ```
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        if let Ok(http) = reqwest::Client::builder().timeout(timeout).build() {
            self.http = http;
        }
        self
    }

    /// Returns this client with its [`RetryPolicy`] replaced by `policy`.
    ///
    /// Builder-style: this consumes the client and hands back a new one, so
    /// it chains onto [`HttpRpcClient::new`] (and alongside
    /// [`HttpRpcClient::with_timeout`]). The policy is read per request, so
    /// it governs every method on [`RpcClient`] — `getNetwork`,
    /// `getLatestLedger`, and `simulateTransaction` alike. If it is never
    /// called, the client keeps [`RetryPolicy::default`] (3 attempts,
    /// 200 ms base delay).
    ///
    /// # What gets retried
    ///
    /// Only [`RpcError::Transport`], [`RpcError::Timeout`], and
    /// [`RpcError::RateLimited`] are retried. Those are the transient
    /// failures — a dropped connection, an elapsed
    /// [`HttpRpcClient::with_timeout`] deadline, or an HTTP 429.
    /// [`RpcError::InvalidJson`], [`RpcError::JsonRpcError`], and
    /// [`RpcError::InvalidResponse`] are deterministic: the same request
    /// would produce the same malformed/errored response, so they are
    /// returned on the first attempt no matter what the policy says.
    ///
    /// # Timing
    ///
    /// At most `max_attempts` requests are made in total, and the wait
    /// between attempt *n* and attempt *n+1* is `base_delay * n` — a linear
    /// backoff, so there is no delay after the final attempt and the total
    /// time spent sleeping is `base_delay * n(n-1)/2`. Combined with a
    /// per-request deadline, the worst-case wall time for one call is
    /// roughly `max_attempts * timeout` plus that backoff, which is worth
    /// keeping in mind when both values are configured.
    ///
    /// # Failure conditions
    ///
    /// This method cannot fail and does not panic — it only stores `policy`.
    /// The field values are not validated: `max_attempts: 0` is not
    /// rejected but behaves as "try once, do not retry" (the first attempt
    /// is always made, so one attempt is the effective minimum), and a
    /// `base_delay` of zero retries immediately with no pause.
    ///
    /// Because this consumes and returns a new client, the policy applies
    /// only to the returned value and to clones made from it — a client
    /// cloned before this call keeps the old policy, and any call already
    /// in flight on such a clone is unaffected. Unlike
    /// [`HttpRpcClient::with_timeout`], this does not rebuild the
    /// underlying HTTP client, so no connection pool is discarded.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use canary_rpc::{HttpRpcClient, RetryPolicy};
    ///
    /// // Up to five attempts, backing off 100ms, 200ms, 300ms, 400ms.
    /// let _client = HttpRpcClient::new("https://soroban-testnet.stellar.org")
    ///     .with_timeout(Duration::from_secs(5))
    ///     .with_retry_policy(RetryPolicy {
    ///         max_attempts: 5,
    ///         base_delay: Duration::from_millis(100),
    ///     });
    /// ```
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Returns this client with attempt-level logging switched on or off.
    ///
    /// Builder-style: this consumes the client and hands back a new one,
    /// so it chains onto [`HttpRpcClient::new`], [`HttpRpcClient::with_timeout`]
    /// and [`HttpRpcClient::with_retry_policy`].
    ///
    /// When verbose, every attempt writes one line to **stderr** after it
    /// completes — success or failure — of the shape
    ///
    /// ```text
    /// [RPC attempt 1/3: 124ms] getNetwork https://***:***@rpc.example.com
    /// ```
    ///
    /// where `1/3` is the attempt number against the configured attempt
    /// budget, `124ms` is that attempt's own duration (the backoff sleep
    /// between attempts is not counted), and the URL is passed through
    /// [`scrub_url`] so basic-auth credentials and query tokens never reach
    /// a log or CI transcript (CF-08 section 3.5).
    ///
    /// The default is off: a client built without this method emits no
    /// attempt lines at all, so non-verbose runs keep exactly the stderr
    /// output they had before. This method cannot fail and does not panic.
    ///
    /// ```
    /// use canary_rpc::HttpRpcClient;
    ///
    /// let _client = HttpRpcClient::new("https://soroban-testnet.stellar.org")
    ///     .with_verbose(true);
    /// ```
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
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
            let started = Instant::now();
            let outcome = self.try_call::<R>(method, &body).await;
            if self.verbose {
                eprintln!(
                    "{}",
                    attempt_log_line(
                        attempt,
                        self.retry_policy.max_attempts.max(1),
                        started.elapsed(),
                        method,
                        &scrub_url(&self.endpoint),
                    )
                );
            }
            match outcome {
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

    /// Like [`HttpRpcClient::call`], but returns the `result` untyped and
    /// requires it to be a JSON object.
    ///
    /// Every method this client calls is documented to answer with an
    /// object, so anything else is a malformed response rather than
    /// something an assertion should be evaluated against.
    async fn call_object<P: Serialize>(
        &self,
        method: &str,
        params: P,
    ) -> Result<serde_json::Value, RpcError> {
        let result: serde_json::Value = self.call(method, params).await?;
        if result.is_object() {
            Ok(result)
        } else {
            Err(RpcError::InvalidResponse {
                method: method.to_string(),
                reason: format!(
                    "expected \"result\" to be a JSON object, but found {}",
                    json_kind(&result)
                ),
            })
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

        let text = read_body_capped(response, method).await?;

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
            // JSON-RPC "Method not found": the endpoint answered properly
            // but does not implement this method, so it is classified as a
            // missing capability rather than a generic JSON-RPC rejection.
            if code == JSON_RPC_METHOD_NOT_FOUND {
                return Err(RpcError::MethodNotAvailable {
                    method: method.to_string(),
                });
            }
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

/// Reads a response body while enforcing [`MAX_RESPONSE_BYTES`].
///
/// The declared `Content-Length` is checked first, so a response that
/// announces an oversized body is refused without reading any of it. When
/// there is no length (a chunked or connection-closed body) the stream is
/// read incrementally and abandoned as soon as the running total would
/// pass the cap, so an oversized body is never fully buffered even though
/// its length was unknown in advance. Dropping the response at that point
/// closes the connection.
///
/// Bytes are decoded lossily rather than strictly, matching what the
/// previous `Response::text()` call did: an invalid UTF-8 body is handed to
/// the JSON parser, which reports it as [`RpcError::InvalidJson`], instead
/// of being reported as a transport failure.
async fn read_body_capped(
    mut response: reqwest::Response,
    method: &str,
) -> Result<String, RpcError> {
    if let Some(declared) = response.content_length() {
        if declared > MAX_RESPONSE_BYTES as u64 {
            return Err(RpcError::ResponseTooLarge {
                method: method.to_string(),
                limit: MAX_RESPONSE_BYTES,
            });
        }
    }

    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| RpcError::Transport {
        method: method.to_string(),
        message: e.to_string(),
    })? {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(RpcError::ResponseTooLarge {
                method: method.to_string(),
                limit: MAX_RESPONSE_BYTES,
            });
        }
        body.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// JSON-RPC standard error code for "Method not found".
const JSON_RPC_METHOD_NOT_FOUND: i64 = -32601;

fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

/// The largest `pagination.limit` `getLedgers` accepts upstream.
///
/// This is the documented `getLedgers` maximum. The fixture engine's own
/// bound on a `[request]` limit is lower (see `crate::request::MAX_LIMIT`);
/// this one only keeps a caller of the typed method within what the
/// endpoint will accept.
const MAX_GET_LEDGERS_LIMIT: u32 = 200;

/// The largest number of keys `getLedgerEntries` accepts upstream.
///
/// The fixture engine's own bound on `[request] keys` is lower (see
/// `crate::request::MAX_KEYS`); this one only keeps a caller of the typed
/// method within what the endpoint will accept.
const MAX_GET_LEDGER_ENTRY_KEYS: usize = 200;

/// Builds the `getLedgerEntries` params object, rejecting arguments the
/// method cannot accept before anything is sent.
fn get_ledger_entries_params(keys: &[String]) -> Result<serde_json::Value, RpcError> {
    let invalid = |reason: String| RpcError::InvalidArgument {
        method: "getLedgerEntries".to_string(),
        reason,
    };
    if keys.is_empty() {
        return Err(invalid(
            "keys must contain at least one base64 LedgerKey".to_string(),
        ));
    }
    if keys.len() > MAX_GET_LEDGER_ENTRY_KEYS {
        return Err(invalid(format!(
            "keys has {} entries; at most {MAX_GET_LEDGER_ENTRY_KEYS} are accepted",
            keys.len()
        )));
    }
    for (index, key) in keys.iter().enumerate() {
        if !crate::request::is_base64(key) {
            return Err(invalid(format!("keys[{index}] is not a base64 string")));
        }
    }
    Ok(json!({ "keys": keys }))
}

/// Builds the `getLedgers` params object, rejecting arguments the method
/// cannot accept before anything is sent.
///
/// `start_ledger` must be a real ledger sequence, so `0` is an argument
/// error rather than a round trip: ledger sequences start at 1, and an
/// endpoint would answer `0` with a protocol-level error that looks like a
/// failed check instead of a caller mistake.
fn get_ledgers_params(
    start_ledger: u32,
    limit: Option<u32>,
) -> Result<serde_json::Value, RpcError> {
    let invalid = |reason: String| RpcError::InvalidArgument {
        method: "getLedgers".to_string(),
        reason,
    };
    if start_ledger == 0 {
        return Err(invalid(
            "startLedger must be greater than 0 (ledger sequences start at 1)".to_string(),
        ));
    }
    if let Some(limit) = limit {
        if !(1..=MAX_GET_LEDGERS_LIMIT).contains(&limit) {
            return Err(invalid(format!(
                "pagination.limit {limit} is outside 1..={MAX_GET_LEDGERS_LIMIT}"
            )));
        }
    }

    let mut params = json!({ "startLedger": start_ledger });
    if let Some(limit) = limit {
        params["pagination"] = json!({ "limit": limit });
    }
    Ok(params)
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
            message: scrub_url_out_of_message(&error.to_string(), error.url().map(|u| u.as_str())),
        }
    }
}

/// Replaces `raw_url` with its [`scrub_url`] form inside `message`, so a
/// transport error that echoes the request URL cannot carry userinfo or
/// query tokens into a report or a CI log.
///
/// The match is a plain substring replacement on the URL reqwest reports:
/// when it is absent from the message (or there is no URL at all) the
/// message is returned unchanged. This is best effort by construction —
/// CF-08 section 3.5 already treats pattern redaction as best effort — and
/// only ever removes secret material, never diagnostic text.
fn scrub_url_out_of_message(message: &str, raw_url: Option<&str>) -> String {
    match raw_url {
        Some(raw) if !raw.is_empty() && message.contains(raw) => {
            message.replace(raw, &scrub_url(raw))
        }
        _ => message.to_string(),
    }
}

/// Masks credentials in a URL before it is logged (CF-08 section 3.5).
///
/// - Basic-auth userinfo is replaced wholesale: `https://user:pass@host`
///   becomes `https://***:***@host` (and `https://user@host` the same, so
///   a username alone is not leaked either).
/// - Every query parameter value is masked: `?token=abc&limit=50` becomes
///   `?token=***&limit=***`, and a valueless `?flag` becomes `?***`.
/// - Scheme, host, port, path and fragment are kept verbatim, so the log
///   still shows *which* endpoint was called.
///
/// This is deliberately not a URL parser: it works on the structural
/// delimiters only (`://`, `/`, `?`, `#`, `@`), so a string that is not an
/// absolute URL is returned unchanged, and no URL crate is pulled in for a
/// logging helper.
fn scrub_url(url: &str) -> String {
    let (without_fragment, fragment) = match url.split_once('#') {
        Some((head, tail)) => (head, Some(tail)),
        None => (url, None),
    };
    let (base, query) = match without_fragment.split_once('?') {
        Some((head, tail)) => (head, Some(tail)),
        None => (without_fragment, None),
    };

    let mut scrubbed = scrub_userinfo(base);
    if let Some(query) = query {
        scrubbed.push('?');
        scrubbed.push_str(&scrub_query(query));
    }
    if let Some(fragment) = fragment {
        scrubbed.push('#');
        scrubbed.push_str(fragment);
    }
    scrubbed
}

/// Replaces the userinfo (`user:pass@`) part of a URL's authority with
/// `***:***@`. A URL without `://` or without userinfo is unchanged.
fn scrub_userinfo(base: &str) -> String {
    let Some((scheme, rest)) = base.split_once("://") else {
        return base.to_string();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let Some(at) = authority.rfind('@') else {
        return base.to_string();
    };
    format!("{scheme}://***:***@{}{tail}", &authority[at + 1..])
}

/// Masks every value in a query string: `token=abc` → `token=***`, and a
/// valueless `flag` → `***`.
fn scrub_query(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) => format!("{name}=***"),
            None => "***".to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Renders one verbose attempt log line, e.g.
/// `[RPC attempt 1/3: 124ms] getNetwork https://***:***@rpc.example.com`.
///
/// `max_attempts` is the configured attempt budget (the caller passes
/// `RetryPolicy::max_attempts`, floored at 1 so the line can never read
/// `…/0`), and `scrubbed_url` must already have been through
/// [`scrub_url`] — this function does not re-scrub, so callers keep the
/// redaction in one place.
fn attempt_log_line(
    attempt: u32,
    max_attempts: u32,
    elapsed: Duration,
    method: &str,
    scrubbed_url: &str,
) -> String {
    format!(
        "[RPC attempt {attempt}/{max_attempts}: {}ms] {method} {scrubbed_url}",
        elapsed.as_millis()
    )
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

    async fn get_health(&self) -> Result<GetHealthResponse, RpcError> {
        self.call("getHealth", json!({})).await
    }

    async fn get_version_info(&self) -> Result<VersionInfoResponse, RpcError> {
        self.call("getVersionInfo", json!({})).await
    }

    async fn get_fee_stats(&self) -> Result<FeeStatsResponse, RpcError> {
        self.call("getFeeStats", json!({})).await
    }

    async fn get_ledger_entries(
        &self,
        keys: &[String],
    ) -> Result<GetLedgerEntriesResponse, RpcError> {
        self.call("getLedgerEntries", get_ledger_entries_params(keys)?)
            .await
    }

    async fn get_network_raw(&self) -> Result<serde_json::Value, RpcError> {
        self.call_object("getNetwork", json!({})).await
    }

    async fn get_latest_ledger_raw(&self) -> Result<serde_json::Value, RpcError> {
        self.call_object("getLatestLedger", json!({})).await
    }

    async fn get_health_raw(&self) -> Result<serde_json::Value, RpcError> {
        self.call_object("getHealth", json!({})).await
    }

    async fn get_version_info_raw(&self) -> Result<serde_json::Value, RpcError> {
        self.call_object("getVersionInfo", json!({})).await
    }

    async fn get_fee_stats_raw(&self) -> Result<serde_json::Value, RpcError> {
        self.call_object("getFeeStats", json!({})).await
    }

    async fn get_ledger_entries_raw(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcError> {
        self.call_object("getLedgerEntries", params).await
    }

    async fn simulate_transaction(
        &self,
        request: SimulationRequest,
    ) -> Result<SimulationResponse, RpcError> {
        self.call("simulateTransaction", request).await
    }

    async fn get_events(
        &self,
        start_ledger: u32,
        filters: &[EventFilter],
    ) -> Result<EventResponse, RpcError> {
        let params = json!({
            "startLedger": start_ledger,
            "filters": filters,
        });
        self.call("getEvents", params).await
    }

    async fn get_ledgers(
        &self,
        start_ledger: u32,
        limit: Option<u32>,
    ) -> Result<GetLedgersResponse, RpcError> {
        self.call("getLedgers", get_ledgers_params(start_ledger, limit)?)
            .await
    }

    async fn get_ledgers_raw(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcError> {
        self.call_object("getLedgers", params).await
    }

    async fn get_events_raw(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcError> {
        self.call_object("getEvents", params).await
    }

    async fn get_transactions(
        &self,
        start_ledger: u32,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<GetTransactionsResponse, RpcError> {
        let mut params = json!({
            "startLedger": start_ledger,
        });
        if let Some(c) = cursor {
            params["cursor"] = json!(c);
        }
        if let Some(l) = limit {
            params["limit"] = json!(l);
        }
        self.call("getTransactions", params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::EventFilter;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn get_network_raw_keeps_fields_the_typed_model_drops() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "passphrase": "Test SDF Network ; September 2015",
                    "protocolVersion": 28,
                    "someFutureField": { "nested": true }
                }
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let raw = client.get_network_raw().await.expect("ok");
        assert_eq!(raw["someFutureField"], json!({ "nested": true }));
        assert_eq!(raw["protocolVersion"], json!(28));
    }

    #[tokio::test]
    async fn get_latest_ledger_raw_keeps_fields_the_typed_model_drops() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "id": "abc",
                    "protocolVersion": 28,
                    "sequence": 7,
                    "closeTime": "1700000000"
                }
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let raw = client.get_latest_ledger_raw().await.expect("ok");
        assert_eq!(raw["closeTime"], json!("1700000000"));
    }

    #[tokio::test]
    async fn raw_methods_reject_a_result_that_is_not_an_object() {
        for (result, kind) in [
            (json!([1, 2]), "an array"),
            (json!("text"), "a string"),
            (json!(28), "a number"),
            (json!(true), "a boolean"),
            (json!(null), "null"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": result
                })))
                .mount(&server)
                .await;

            let client = HttpRpcClient::new(server.uri());
            let err = client.get_network_raw().await.unwrap_err();
            match err {
                RpcError::InvalidResponse { method, reason } => {
                    assert_eq!(method, "getNetwork");
                    assert!(reason.contains(kind), "{reason:?} should name {kind}");
                }
                other => panic!("expected InvalidResponse, got {other:?}"),
            }
        }
    }

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
    async fn maps_minus_32601_to_method_not_available() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": { "code": -32601, "message": "Method not found" }
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let err = client.get_latest_ledger().await.unwrap_err();
        match &err {
            RpcError::MethodNotAvailable { method } => assert_eq!(method, "getLatestLedger"),
            other => panic!("expected MethodNotAvailable, got {other:?}"),
        }
        // The diagnostic preserves the method name and the JSON-RPC code.
        let diagnostic = err.to_string();
        assert!(diagnostic.contains("getLatestLedger"), "{diagnostic}");
        assert!(diagnostic.contains("-32601"), "{diagnostic}");
        // It is a protocol-level classification, not a transport failure.
        assert!(!matches!(err, RpcError::Transport { .. }));
    }

    #[tokio::test]
    async fn other_json_rpc_codes_stay_distinct_from_method_not_available() {
        for code in [-32600_i64, -32603, -32602, -32000] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "error": { "code": code, "message": "boom" }
                })))
                .mount(&server)
                .await;

            let client = HttpRpcClient::new(server.uri());
            let err = client.get_network().await.unwrap_err();
            match &err {
                RpcError::JsonRpcError { code: got, .. } => assert_eq!(*got, code),
                other => panic!("code {code} should be JsonRpcError, got {other:?}"),
            }
        }
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
    async fn maps_missing_result_and_error_keys_to_invalid_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let err = client.get_network().await.unwrap_err();
        assert!(matches!(
            err,
            RpcError::InvalidResponse { ref reason, .. } if reason.contains("neither \"result\" nor \"error\"")
        ));
    }

    #[tokio::test]
    async fn maps_deserialization_failure_to_invalid_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "protocolVersion": 28
                }
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let err = client.get_network().await.unwrap_err();
        assert!(matches!(
            err,
            RpcError::InvalidResponse { ref reason, .. } if reason.contains("missing field `passphrase`")
        ));
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

    /// Every other retry test mounts a mock that fails on *every* request and
    /// asserts the client eventually gives up, so none of them proves the
    /// point of retrying at all: that a transient failure followed by a good
    /// response is an overall success. A regression that kept counting
    /// attempts but discarded the successful response would pass the whole
    /// existing suite.
    #[tokio::test]
    async fn recovers_from_a_transient_failure_and_returns_the_successful_response() {
        let server = MockServer::start().await;
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // 503 on the first request, then success — the shape of a real
        // transient outage.
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(move |_request: &wiremock::Request| {
                if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(503)
                } else {
                    ResponseTemplate::new(200).set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "passphrase": "Test SDF Network ; September 2015",
                            "protocolVersion": 28
                        }
                    }))
                }
            })
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri()).with_retry_policy(RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
        });

        let info = client
            .get_network()
            .await
            .expect("the second, successful response must be returned as Ok");
        assert_eq!(info.passphrase, "Test SDF Network ; September 2015");
        assert_eq!(info.protocol_version, 28);

        // Exactly one retry: the failure was recovered, not exhausted, and
        // the success was not re-requested either.
        let requests = server.received_requests().await.expect("requests");
        assert_eq!(requests.len(), 2, "expected the 503 plus one retry");
    }

    #[tokio::test]
    async fn retries_a_transient_failure_and_returns_the_successful_response() {
        let server = MockServer::start().await;
        // First request: a transient 503. `up_to_n_times(1)` stops this mock
        // from matching afterwards, so the retried request falls through to
        // the success mock mounted below (same-priority mocks match in
        // insertion order).
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .named("transient 503")
            .mount(&server)
            .await;
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
            .named("successful retry")
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri()).with_retry_policy(RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
        });
        let info = client.get_network().await.expect("retry succeeds");
        assert_eq!(info.protocol_version, 28);
        assert_eq!(info.passphrase, "Test SDF Network ; September 2015");

        let requests = server.received_requests().await.expect("requests");
        assert_eq!(requests.len(), 2);
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

    #[tokio::test]
    async fn get_events_parses_a_successful_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "latestLedger": 1000,
                    "events": [
                        {
                            "ledger": 100,
                            "contractId": "AAA",
                            "type": "contract",
                            "topic": ["AAA", "BBB"],
                            "value": "CCCC"
                        }
                    ]
                }
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let filters = vec![EventFilter {
            r#type: Some("contract".to_string()),
            contract_ids: Some(vec!["AAA".to_string()]),
            topics: None,
        }];
        let response = client.get_events(100, &filters).await.expect("ok");
        assert_eq!(response.latest_ledger, 1000);
        assert_eq!(response.events.len(), 1);
        assert_eq!(response.events[0].ledger, 100);
    }

    #[tokio::test]
    async fn get_transactions_parses_a_successful_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "latestLedger": 1000,
                    "transactions": [
                        {
                            "ledger": 100,
                            "status": "SUCCESS"
                        }
                    ],
                    "cursor": "some-cursor"
                }
            })))
            .mount(&server)
            .await;

        let client = HttpRpcClient::new(server.uri());
        let response = client
            .get_transactions(100, Some("cursor-123"), Some(10))
            .await
            .expect("ok");
        assert_eq!(response.latest_ledger, 1000);
        assert_eq!(response.transactions.len(), 1);
        assert_eq!(response.transactions[0].ledger, 100);
        assert_eq!(response.transactions[0].status, "SUCCESS");
        assert_eq!(response.cursor, Some("some-cursor".to_string()));
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

    /// A custom passphrase supplied by the caller (the CLI's
    /// `--network-passphrase`) is compared exactly like a well-known one:
    /// this is the path that makes private/standalone networks verifiable.
    #[test]
    fn validate_network_info_accepts_a_custom_passphrase_when_it_matches() {
        let custom = "Local Standalone Network ; October 2026";
        let info = network_info(custom, 28);
        assert!(validate_network_info(&info, custom, 28).is_ok());
    }

    /// Negative side of the same path: a supplied custom passphrase that
    /// differs from what the endpoint reports must construct a
    /// `NetworkMismatch` naming both values, never a silent pass.
    #[test]
    fn validate_network_info_rejects_a_custom_passphrase_that_differs() {
        let info = network_info("Some Other Net ; October 2026", 28);
        let err = validate_network_info(&info, "Local Standalone Network ; October 2026", 28)
            .expect_err("custom passphrase differs");
        assert!(
            matches!(
                err,
                RpcError::NetworkMismatch { ref expected, ref actual }
                    if expected == "Local Standalone Network ; October 2026"
                        && actual == "Some Other Net ; October 2026"
            ),
            "unexpected: {err}"
        );
    }
}
