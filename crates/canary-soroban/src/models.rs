//! Typed projection of the authorization entries a Soroban simulation
//! reports.
//!
//! `simulateTransaction` returns a `results` array whose entries carry an
//! `auth` array of required authorization entries — the addresses whose
//! signatures the transaction will need to be accepted on-chain. This
//! module turns those raw JSON objects into [`SimulationAuthEntry`]
//! values that fixture assertions can evaluate deterministically.
//!
//! Following the project's CA-01 section 4.1 rule, parsing is a *read*
//! of the raw JSON, never a rejection of it: an entry with a missing or
//! mistyped field parses into a [`SimulationAuthEntry`] with that field
//! absent, and a fixture assertion that depends on the field fails with
//! a diagnostic naming it. Nothing here panics on any input.

use serde_json::Value as JsonValue;

use canary_rpc::SimulationResult;

/// One required authorization entry parsed from a simulation result's
/// `auth` array.
///
/// Every field is optional: parsing is lenient so that a response with a
/// missing or unexpectedly typed field degrades into an absent value
/// here rather than an engine error. A fixture assertion that depends on
/// an absent field then fails gracefully, naming the field, instead of
/// the whole check erroring out. A JSON `null` is treated the same as an
/// absent field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationAuthEntry {
    /// The `address` whose authorization the invocation requires, as the
    /// `G...`/`C...` strkey string the endpoint returned.
    pub address: Option<String>,
    /// The replay-protected `nonce` paired with the address credentials.
    pub nonce: Option<i64>,
    /// Whether the endpoint reported a non-null `signature` alongside
    /// the address credentials.
    pub has_signature: bool,
    /// The `functionName` of the entry's `rootInvocation`, when reported
    /// as a string.
    pub root_function_name: Option<String>,
    /// How many entries the entry's `rootInvocation.subInvocations`
    /// array holds; `0` when the array is absent or not an array.
    pub sub_invocation_count: usize,
}

impl SimulationAuthEntry {
    /// Parses one raw `auth` array element, keeping every field this
    /// model does not name out of the way rather than failing on it.
    pub fn from_json(value: &JsonValue) -> Self {
        let root_invocation = value.get("rootInvocation");
        Self {
            address: value
                .get("address")
                .and_then(JsonValue::as_str)
                .map(str::to_string),
            nonce: value.get("nonce").and_then(JsonValue::as_i64),
            has_signature: value
                .get("signature")
                .is_some_and(|signature| !signature.is_null()),
            root_function_name: root_invocation
                .and_then(|root| root.get("functionName"))
                .and_then(JsonValue::as_str)
                .map(str::to_string),
            sub_invocation_count: root_invocation
                .and_then(|root| root.get("subInvocations"))
                .and_then(JsonValue::as_array)
                .map_or(0, Vec::len),
        }
    }
}

/// Parses every authorization entry across a `simulateTransaction`
/// response's `results` array, flattening the entries in the order the
/// endpoint returned them.
pub fn parse_auth_results(results: &[SimulationResult]) -> Vec<SimulationAuthEntry> {
    results
        .iter()
        .flat_map(|result| result.auth.iter())
        .map(SimulationAuthEntry::from_json)
        .collect()
}
