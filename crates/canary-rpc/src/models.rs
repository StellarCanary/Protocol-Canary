//! Typed request/response shapes for the small subset of Stellar RPC this
//! crate calls: `getNetwork`, `getLatestLedger`, `getHealth`, and `simulateTransaction`.
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
}
