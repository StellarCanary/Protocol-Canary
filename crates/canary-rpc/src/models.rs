//! Typed request/response shapes for the Stellar RPC methods this crate
//! calls: `getNetwork`, `getLatestLedger`, `getHealth`, `getVersionInfo`,
//! `getFeeStats`, `getEvents`, `getLedgers`, `getLedgerEntries`,
//! `getTransactions`, and `simulateTransaction`.
//!
//! These mirror the documented Stellar RPC JSON-RPC methods; XDR-bearing
//! fields stay as base64 strings here; the Soroban runner decodes them
//! with `canary-xdr`/`stellar-xdr` rather than this crate reimplementing
//! that.

use serde::{Deserialize, Serialize};

/// The result of `getHealth`.
///
/// Reference: <https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getHealth>
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetHealthResponse {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ledger: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_ledger: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_retention_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ledger_close_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_ledger_close_time: Option<String>,
}

/// The result of `getNetwork`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friendbot_url: Option<String>,
    pub passphrase: String,
    pub protocol_version: u32,
}

/// The result of `getLatestLedger`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestLedger {
    pub id: String,
    pub protocol_version: u32,
    pub sequence: u32,
}

/// The result of `getVersionInfo`.
///
/// Reference: <https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getVersionInfo>
///
/// `version` is the endpoint's own release string; it is not the network
/// protocol version. Use [`VersionInfoResponse::observed_protocol`] for
/// the protocol version the endpoint reports, which is the value a run
/// compares against its target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfoResponse {
    /// The Stellar RPC release version, for example `29.0.0-b2b7016`.
    pub version: String,
    /// The git commit the endpoint was built from.
    pub commit_hash: String,
    /// The endpoint build timestamp, as the endpoint reports it.
    pub build_timestamp: String,
    /// The bundled Stellar Core version, when the endpoint reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captive_core_version: Option<String>,
    /// The Stellar protocol version the endpoint reports.
    pub protocol_version: u32,
}

impl VersionInfoResponse {
    /// The observed network protocol version.
    ///
    /// This is the integer a run compares against its target protocol; the
    /// similarly named `version` field is the RPC release string.
    pub fn observed_protocol(&self) -> u32 {
        self.protocol_version
    }
}

/// The result of `getFeeStats`.
///
/// Reference: <https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getFeeStats>
///
/// Fee amounts and transaction counts are string-encoded by the endpoint,
/// so they stay strings here rather than being parsed into integers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeStatsResponse {
    /// Inclusion fee statistics for Soroban transactions.
    pub soroban_inclusion_fee: FeeDistribution,
    /// Inclusion fee statistics for all transactions.
    pub inclusion_fee: FeeDistribution,
    /// The latest ledger the statistics cover.
    pub latest_ledger: u32,
}

/// One fee distribution in a `getFeeStats` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeDistribution {
    pub max: String,
    pub min: String,
    pub mode: String,
    pub p10: String,
    pub p20: String,
    pub p30: String,
    pub p40: String,
    pub p50: String,
    pub p60: String,
    pub p70: String,
    pub p80: String,
    pub p90: String,
    pub p95: String,
    pub p99: String,
    /// Number of transactions the distribution was computed from.
    pub transaction_count: String,
    /// Number of ledgers the distribution was computed over.
    pub ledger_count: u32,
}

/// The parameters of `simulateTransaction`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationRequest {
    /// A base64-encoded, unsigned `TransactionEnvelope` XDR.
    pub transaction: String,
}

/// One filter for `getEvents`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<Vec<String>>>,
}

/// The result of `getEvents`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventResponse {
    pub latest_ledger: u32,
    pub events: Vec<Event>,
}

/// One event in a `getEvents` response.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub ledger: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    pub topic: Vec<String>,
    pub value: String,
}

/// The result of `getLedgers`.
///
/// Reference: <https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getLedgers>
///
/// The range bounds are optional because the endpoint may answer a range it
/// retains nothing of with only `ledgers: []`: an empty range is a valid,
/// successful answer, not a malformed one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLedgersResponse {
    /// The ledgers in the requested range, in ascending sequence order.
    /// Empty when the endpoint returns no ledger for that range.
    #[serde(default)]
    pub ledgers: Vec<LedgerHeaderInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ledger: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ledger_close_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_ledger: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_ledger_close_time: Option<String>,
    /// Opaque cursor for the page after this one, when the endpoint returns
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl GetLedgersResponse {
    /// Whether the response carries no ledger for the requested range.
    pub fn is_empty(&self) -> bool {
        self.ledgers.is_empty()
    }
}

/// One ledger header in a `getLedgers` response.
///
/// `header_xdr` and `metadata_xdr` stay base64 strings here, like every
/// other XDR-bearing field in this crate; callers decode them with
/// `canary-xdr`/`stellar-xdr` rather than this crate reimplementing XDR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerHeaderInfo {
    /// Hex-encoded ledger hash.
    pub hash: String,
    /// The ledger's sequence number.
    pub sequence: u32,
    /// The ledger's closing time as the endpoint reports it — a
    /// string-encoded Unix timestamp, not a number.
    pub ledger_close_time: String,
    /// Base64-encoded `LedgerHeader` XDR.
    pub header_xdr: String,
    /// Base64-encoded `LedgerCloseMeta` XDR, when the endpoint includes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_xdr: Option<String>,
}

/// The result of `getLedgerEntries`.
///
/// Reference: <https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getLedgerEntries>
///
/// An entry the endpoint does not have is simply absent from `entries`;
/// an empty list is a successful answer, not a malformed one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLedgerEntriesResponse {
    /// The ledger entries found for the requested keys.
    #[serde(default)]
    pub entries: Vec<LedgerEntryResult>,
    /// The latest ledger known to the endpoint, when it reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ledger: Option<u32>,
}

impl GetLedgerEntriesResponse {
    /// Whether the endpoint returned no entry for the requested keys.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One ledger entry in a `getLedgerEntries` response.
///
/// `key` and `xdr` stay base64 strings here, like every other XDR-bearing
/// field in this crate; callers decode them with `canary-xdr`/`stellar-xdr`
/// rather than this crate reimplementing XDR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntryResult {
    /// Base64-encoded `LedgerKey` XDR for the entry.
    pub key: String,
    /// Base64-encoded `LedgerEntry` XDR.
    pub xdr: String,
    /// The ledger sequence in which the entry was last modified.
    pub last_modified_ledger_seq: u32,
    /// The ledger sequence after which the entry expires, when the entry
    /// is a temporary Soroban entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_until_ledger_seq: Option<u32>,
}

/// The result of `getTransactions`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTransactionsResponse {
    pub latest_ledger: u32,
    pub transactions: Vec<Transaction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Transaction status values returned by Stellar RPC's `getTransaction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionStatus {
    /// The transaction was included and applied successfully.
    Success,
    /// The transaction hash is outside the node's retained history.
    NotFound,
    /// The transaction was included but failed to apply.
    Failed,
}

/// The result of `getTransaction`.
///
/// XDR fields remain base64 strings for decoding by `canary-xdr`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTransactionResponse {
    pub status: TransactionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ledger: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ledger_close_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_ledger: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_ledger_close_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope_xdr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_xdr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_meta_xdr: Option<String>,
}

/// One transaction in a `getTransactions` response.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub ledger: u32,
    pub status: String,
}

#[cfg(test)]
mod transaction_tests {
    use super::{GetTransactionResponse, TransactionStatus};

    #[test]
    fn deserializes_each_transaction_status_and_optional_xdr() {
        for (status, expected) in [
            ("SUCCESS", TransactionStatus::Success),
            ("NOT_FOUND", TransactionStatus::NotFound),
            ("FAILED", TransactionStatus::Failed),
        ] {
            let response: GetTransactionResponse = serde_json::from_value(serde_json::json!({
                "status": status,
                "resultXdr": "AAAA",
                "resultMetaXdr": "BBBB",
                "envelopeXdr": "CCCC"
            }))
            .expect("known transaction status");
            assert_eq!(response.status, expected);
            assert_eq!(response.result_xdr.as_deref(), Some("AAAA"));
            assert_eq!(response.result_meta_xdr.as_deref(), Some("BBBB"));
            assert_eq!(response.envelope_xdr.as_deref(), Some("CCCC"));
        }
    }
}

/// The result of `simulateTransaction`.
///
/// Only the fields this project's fixtures currently assert on are
/// modeled; unrecognized fields are ignored rather than rejected (see the
/// project's rule that an unrelated new optional field must not break a
/// check).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationResponse {
    pub latest_ledger: u32,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub transaction_data: Option<String>,
    #[serde(default)]
    pub min_resource_fee: Option<String>,
}

impl SimulationResponse {
    pub fn succeeded(&self) -> bool {
        self.error.is_none()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn network_info_deserializes_camel_case_fields() {
        let json = r#"{"friendbotUrl":"https://friendbot.stellar.org","passphrase":"Test SDF Network ; September 2015","protocolVersion":28}"#;
        let info: NetworkInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.protocol_version, 28);
        assert_eq!(
            info.friendbot_url.as_deref(),
            Some("https://friendbot.stellar.org")
        );
    }

    #[test]
    fn network_info_tolerates_missing_optional_friendbot_url() {
        let json = r#"{"passphrase":"Public Global Stellar Network ; September 2015","protocolVersion":28}"#;
        let info: NetworkInfo = serde_json::from_str(json).unwrap();
        assert!(info.friendbot_url.is_none());
    }

    #[test]
    fn network_info_tolerates_unrecognized_extra_fields() {
        let json = r#"{"passphrase":"x","protocolVersion":28,"someNewField":true}"#;
        let info: NetworkInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.protocol_version, 28);
    }

    #[test]
    fn simulation_response_reports_failure_when_error_is_present() {
        let json = r#"{"latestLedger":100,"error":"host invocation failed"}"#;
        let resp: SimulationResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.succeeded());
    }

    #[test]
    fn simulation_response_reports_success_when_error_is_absent() {
        let json = r#"{"latestLedger":100,"transactionData":"AAAA"}"#;
        let resp: SimulationResponse = serde_json::from_str(json).unwrap();
        assert!(resp.succeeded());
    }

    #[test]
    fn event_response_parses_with_events() {
        let json = r#"{
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
        }"#;
        let resp: EventResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.latest_ledger, 1000);
        assert_eq!(resp.events.len(), 1);
        assert_eq!(resp.events[0].ledger, 100);
        assert_eq!(resp.events[0].contract_id, Some("AAA".to_string()));
        assert_eq!(resp.events[0].r#type, Some("contract".to_string()));
        assert_eq!(
            resp.events[0].topic,
            vec!["AAA".to_string(), "BBB".to_string()]
        );
        assert_eq!(resp.events[0].value, "CCCC");
    }

    #[test]
    fn event_filter_serializes_camel_case() {
        let filter = EventFilter {
            r#type: Some("contract".to_string()),
            contract_ids: Some(vec!["AAA".to_string()]),
            topics: Some(vec![vec!["BBB".to_string()]]),
        };
        let json = serde_json::to_string(&filter).unwrap();
        assert!(json.contains("type"));
        assert!(json.contains("contractIds"));
        assert!(json.contains("topics"));
    }

    #[test]
    fn get_ledgers_response_parses_the_ledger_list_and_sequence_numbers() {
        let json = r#"{
            "ledgers": [
                {
                    "hash": "26a4b6c1",
                    "sequence": 456,
                    "ledgerCloseTime": "1722464376",
                    "headerXdr": "AAAA",
                    "metadataXdr": "BBBB"
                },
                {
                    "hash": "26a4b6c2",
                    "sequence": 457,
                    "ledgerCloseTime": "1722464377",
                    "headerXdr": "CCCC"
                }
            ],
            "latestLedger": 500,
            "latestLedgerCloseTime": "1722464400",
            "oldestLedger": 1,
            "oldestLedgerCloseTime": "1722460000",
            "cursor": "457"
        }"#;
        let response: GetLedgersResponse = serde_json::from_str(json).unwrap();
        assert!(!response.is_empty());
        assert_eq!(response.ledgers.len(), 2);
        assert_eq!(response.ledgers[0].sequence, 456);
        assert_eq!(response.ledgers[1].sequence, 457);
        assert_eq!(response.ledgers[0].ledger_close_time, "1722464376");
        assert_eq!(response.ledgers[0].header_xdr, "AAAA");
        assert_eq!(response.ledgers[0].metadata_xdr.as_deref(), Some("BBBB"));
        assert!(response.ledgers[1].metadata_xdr.is_none());
        assert_eq!(response.latest_ledger, Some(500));
        assert_eq!(response.oldest_ledger, Some(1));
        assert_eq!(response.cursor.as_deref(), Some("457"));
    }

    #[test]
    fn get_ledgers_response_tolerates_an_empty_range() {
        for json in [
            r#"{"ledgers": []}"#,
            r#"{"ledgers": [], "latestLedger": 500, "oldestLedger": 1}"#,
        ] {
            let response: GetLedgersResponse = serde_json::from_str(json).unwrap();
            assert!(response.is_empty(), "{json}");
        }
    }

    #[test]
    fn get_ledgers_response_tolerates_unrecognized_extra_fields() {
        let json = r#"{
            "ledgers": [{
                "hash": "26a4b6c1",
                "sequence": 456,
                "ledgerCloseTime": "1722464376",
                "headerXdr": "AAAA",
                "someNewField": true
            }],
            "someOtherNewField": { "nested": 1 }
        }"#;
        let response: GetLedgersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.ledgers.len(), 1);
    }

    #[test]
    fn get_ledgers_response_requires_the_header_fields_it_asserts_on() {
        // Each case drops one field from an otherwise complete ledger entry.
        for (missing, json) in [
            (
                "sequence",
                r#"{"ledgers":[{"hash":"abc","ledgerCloseTime":"1722464376","headerXdr":"AAAA"}]}"#,
            ),
            (
                "ledgerCloseTime",
                r#"{"ledgers":[{"hash":"abc","sequence":1,"headerXdr":"AAAA"}]}"#,
            ),
            (
                "headerXdr",
                r#"{"ledgers":[{"hash":"abc","sequence":1,"ledgerCloseTime":"1722464376"}]}"#,
            ),
            (
                "hash",
                r#"{"ledgers":[{"sequence":1,"ledgerCloseTime":"1722464376","headerXdr":"AAAA"}]}"#,
            ),
        ] {
            let err = serde_json::from_str::<GetLedgersResponse>(json)
                .expect_err(&format!("missing {missing} must not deserialize"));
            assert!(err.to_string().contains(missing), "{missing}: {err}");
        }
    }

    #[test]
    fn get_ledgers_response_serializes_back_to_camel_case() {
        let json = r#"{"ledgers":[{"hash":"abc","sequence":7,"ledgerCloseTime":"1722464376","headerXdr":"AAAA"}],"latestLedger":9,"cursor":"7"}"#;
        let response: GetLedgersResponse = serde_json::from_str(json).unwrap();
        let roundtripped = serde_json::to_string(&response).unwrap();
        assert!(
            roundtripped.contains("\"ledgerCloseTime\""),
            "{roundtripped}"
        );
        assert!(roundtripped.contains("\"headerXdr\""), "{roundtripped}");
        assert!(roundtripped.contains("\"latestLedger\""), "{roundtripped}");
        let reparsed: GetLedgersResponse = serde_json::from_str(&roundtripped).unwrap();
        assert_eq!(reparsed, response);
    }

    #[test]
    fn get_transactions_response_parses_with_transactions() {
        let json = r#"{
            "latestLedger": 1000,
            "transactions": [
                {
                    "ledger": 100,
                    "status": "SUCCESS"
                }
            ],
            "cursor": "some-cursor"
        }"#;
        let resp: GetTransactionsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.latest_ledger, 1000);
        assert_eq!(resp.transactions.len(), 1);
        assert_eq!(resp.transactions[0].ledger, 100);
        assert_eq!(resp.transactions[0].status, "SUCCESS");
        assert_eq!(resp.cursor, Some("some-cursor".to_string()));
    }

    #[test]
    fn version_info_parses_the_observed_protocol_and_build_fields() {
        let json = r#"{
            "version": "29.0.0-b2b7016",
            "commitHash": "b2b701685c79aee17fe4eb22dbd08a5dfd11594d",
            "buildTimestamp": "2026-09-22T14:52:44",
            "captiveCoreVersion": "stellar-core 29.0.0 (b2b7016)",
            "protocolVersion": 29
        }"#;
        let info: VersionInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(info.version, "29.0.0-b2b7016");
        assert_eq!(info.commit_hash, "b2b701685c79aee17fe4eb22dbd08a5dfd11594d");
        assert_eq!(info.build_timestamp, "2026-09-22T14:52:44");
        assert_eq!(
            info.captive_core_version.as_deref(),
            Some("stellar-core 29.0.0 (b2b7016)")
        );
        assert_eq!(info.observed_protocol(), 29);
    }

    #[test]
    fn version_info_does_not_confuse_the_release_string_with_the_protocol() {
        let json = r#"{
            "version": "29.0.0",
            "commitHash": "abc",
            "buildTimestamp": "2026-09-22T14:52:44",
            "protocolVersion": 28
        }"#;
        let info: VersionInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(info.version, "29.0.0");
        assert_eq!(info.observed_protocol(), 28);
    }

    #[test]
    fn version_info_requires_an_integer_protocol_version() {
        let json = r#"{
            "version": "29.0.0",
            "commitHash": "abc",
            "buildTimestamp": "2026-09-22T14:52:44",
            "protocolVersion": "29"
        }"#;
        let err = serde_json::from_str::<VersionInfoResponse>(json).unwrap_err();
        assert!(err.to_string().contains("invalid type"), "{err}");
        assert!(err.to_string().contains("u32"), "{err}");
    }

    #[test]
    fn version_info_tolerates_unrecognized_extra_fields() {
        let json = r#"{
            "version": "29.0.0",
            "commitHash": "abc",
            "buildTimestamp": "2026-09-22T14:52:44",
            "protocolVersion": 29,
            "someNewField": true
        }"#;
        let info: VersionInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(info.observed_protocol(), 29);
    }

    fn fee_distribution(multiplier: u32) -> serde_json::Value {
        json!({
            "max": format!("{}", 100 * multiplier),
            "min": format!("{multiplier}"),
            "mode": format!("{}", 10 * multiplier),
            "p10": format!("{}", 2 * multiplier),
            "p20": format!("{}", 3 * multiplier),
            "p30": format!("{}", 4 * multiplier),
            "p40": format!("{}", 5 * multiplier),
            "p50": format!("{}", 6 * multiplier),
            "p60": format!("{}", 7 * multiplier),
            "p70": format!("{}", 8 * multiplier),
            "p80": format!("{}", 9 * multiplier),
            "p90": format!("{}", 11 * multiplier),
            "p95": format!("{}", 12 * multiplier),
            "p99": format!("{}", 13 * multiplier),
            "transactionCount": format!("{}", 20 * multiplier),
            "ledgerCount": 50
        })
    }

    #[test]
    fn fee_stats_parses_both_distributions_and_counts() {
        let json = json!({
            "sorobanInclusionFee": fee_distribution(2),
            "inclusionFee": fee_distribution(1),
            "latestLedger": 4519945
        })
        .to_string();
        let stats: FeeStatsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(stats.latest_ledger, 4519945);
        assert_eq!(stats.soroban_inclusion_fee.max, "200");
        assert_eq!(stats.soroban_inclusion_fee.p99, "26");
        assert_eq!(stats.soroban_inclusion_fee.transaction_count, "40");
        assert_eq!(stats.soroban_inclusion_fee.ledger_count, 50);
        assert_eq!(stats.inclusion_fee.min, "1");
        assert_eq!(stats.inclusion_fee.p50, "6");
    }

    #[test]
    fn fee_stats_requires_both_distributions() {
        let json = json!({
            "sorobanInclusionFee": fee_distribution(1),
            "latestLedger": 4519945
        })
        .to_string();
        let err = serde_json::from_str::<FeeStatsResponse>(&json).unwrap_err();
        assert!(err.to_string().contains("inclusionFee"), "{err}");
    }

    #[test]
    fn fee_stats_rejects_a_string_where_a_numeric_field_belongs() {
        let json = json!({
            "sorobanInclusionFee": fee_distribution(1),
            "inclusionFee": fee_distribution(1),
            "latestLedger": "4519945"
        })
        .to_string();
        let err = serde_json::from_str::<FeeStatsResponse>(&json).unwrap_err();
        assert!(err.to_string().contains("invalid type"), "{err}");
        assert!(err.to_string().contains("u32"), "{err}");
    }

    #[test]
    fn fee_stats_rejects_a_string_ledger_count() {
        let mut distribution = fee_distribution(1);
        distribution["ledgerCount"] = json!("50");
        let json = json!({
            "sorobanInclusionFee": distribution.clone(),
            "inclusionFee": distribution,
            "latestLedger": 4519945
        })
        .to_string();
        let err = serde_json::from_str::<FeeStatsResponse>(&json).unwrap_err();
        assert!(err.to_string().contains("invalid type"), "{err}");
        assert!(err.to_string().contains("u32"), "{err}");
    }

    #[test]
    fn ledger_entries_parse_base64_entries_and_optional_expiry() {
        let json = r#"{
            "entries": [
                {
                    "key": "AAAAAA==",
                    "xdr": "BBBBBB==",
                    "lastModifiedLedgerSeq": 123,
                    "liveUntilLedgerSeq": 456
                },
                {
                    "key": "CCCCCC==",
                    "xdr": "DDDDDD==",
                    "lastModifiedLedgerSeq": 124
                }
            ],
            "latestLedger": 500
        }"#;
        let response: GetLedgerEntriesResponse = serde_json::from_str(json).unwrap();
        assert!(!response.is_empty());
        assert_eq!(response.latest_ledger, Some(500));
        assert_eq!(response.entries.len(), 2);
        assert_eq!(response.entries[0].key, "AAAAAA==");
        assert_eq!(response.entries[0].xdr, "BBBBBB==");
        assert_eq!(response.entries[0].last_modified_ledger_seq, 123);
        assert_eq!(response.entries[0].live_until_ledger_seq, Some(456));
        assert!(response.entries[1].live_until_ledger_seq.is_none());
    }

    #[test]
    fn ledger_entries_tolerate_an_empty_result() {
        for json in [
            r#"{"entries": []}"#,
            r#"{"entries": [], "latestLedger": 500}"#,
        ] {
            let response: GetLedgerEntriesResponse = serde_json::from_str(json).unwrap();
            assert!(response.is_empty(), "{json}");
        }
    }

    #[test]
    fn ledger_entries_require_the_entry_fields_the_model_names() {
        for (missing, json) in [
            (
                "key",
                r#"{"entries":[{"xdr":"BBBBBB==","lastModifiedLedgerSeq":123}]}"#,
            ),
            (
                "xdr",
                r#"{"entries":[{"key":"AAAAAA==","lastModifiedLedgerSeq":123}]}"#,
            ),
            (
                "lastModifiedLedgerSeq",
                r#"{"entries":[{"key":"AAAAAA==","xdr":"BBBBBB=="}]}"#,
            ),
        ] {
            let err = serde_json::from_str::<GetLedgerEntriesResponse>(json)
                .expect_err(&format!("missing {missing} must not deserialize"));
            assert!(err.to_string().contains(missing), "{missing}: {err}");
        }
    }

    #[test]
    fn ledger_entries_tolerate_unrecognized_extra_fields() {
        let json = r#"{
            "entries": [{
                "key": "AAAAAA==",
                "xdr": "BBBBBB==",
                "lastModifiedLedgerSeq": 123,
                "someNewField": true
            }],
            "someOtherNewField": 1
        }"#;
        let response: GetLedgerEntriesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.entries.len(), 1);
    }
}
