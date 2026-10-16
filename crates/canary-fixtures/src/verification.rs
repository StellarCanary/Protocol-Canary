//! The CF-04 verification evidence record (`verificationVersion` 1).
//!
//! Verification is a separate, dated, attributed event — not a run's
//! pass result (CF-04 section 2). A fixture carries at most one record,
//! embedded as a `[verification]` table beside its other metadata; a
//! fixture without a record is simply unverified, which is the unknown
//! state — there is no `unknown` method and no record is ever invented.

use serde::{Deserialize, Serialize};

/// The only supported record version.
pub const VERIFICATION_VERSION: u32 = 1;

/// How a fixture was verified (CF-04 section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationMethod {
    /// The fixture file conforms to the schema; proves nothing about
    /// Stellar behavior.
    Structural,
    /// A reviewer read the cited upstream source and confirmed the
    /// assertion matches it.
    SourceChecked,
    /// Executed against a real endpoint using only read-only RPC calls.
    LiveReadOnly,
    /// Executed with `simulateTransaction` on an unsigned transaction.
    LiveSimulation,
}

impl VerificationMethod {
    /// The stable kebab-case name, as stored in a record.
    pub fn as_str(self) -> &'static str {
        match self {
            VerificationMethod::Structural => "structural",
            VerificationMethod::SourceChecked => "source-checked",
            VerificationMethod::LiveReadOnly => "live-read-only",
            VerificationMethod::LiveSimulation => "live-simulation",
        }
    }

    /// Whether this method is a live one, which CF-04 requires a
    /// `network` block for.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            VerificationMethod::LiveReadOnly | VerificationMethod::LiveSimulation
        )
    }
}

/// The result of the verification step (CF-04 section 3). A
/// `Mismatched` record is retained: it explains why a fixture is not
/// trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationOutcome {
    Matched,
    Mismatched,
}

impl VerificationOutcome {
    /// The stable name, as stored in a record.
    pub fn as_str(self) -> &'static str {
        match self {
            VerificationOutcome::Matched => "matched",
            VerificationOutcome::Mismatched => "mismatched",
        }
    }
}

/// Which network a live verification ran against (CF-04 section 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationNetwork {
    pub name: String,
    pub passphrase: String,
    /// Copied from the real `getNetwork` response, never from the
    /// fixture's own `protocol`.
    pub observed_protocol: u32,
}

/// Who ran the verification, and with which binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationVerifier {
    pub name: String,
    pub version: String,
}

/// One verification evidence record (`verificationVersion` 1).
///
/// Embedded in a fixture as a `[verification]` table with the same
/// camelCase keys the JSON form of CF-04 section 3 uses, so the record
/// round-trips "as stored" into inventories and reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationRecord {
    pub verification_version: u32,
    /// The id of the fixture that was verified; must equal the fixture
    /// the record is embedded in.
    pub fixture_id: String,
    /// The CF-02 hash of the fixture file that was verified, lowercase
    /// hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixture_sha256: Option<String>,
    pub method: VerificationMethod,
    /// RFC 3339 in UTC, e.g. `2026-09-28T07:20:00Z`.
    pub checked_at: String,
    /// Required for `live-read-only` and `live-simulation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<VerificationNetwork>,
    /// A host name only: no path, query or token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier: Option<VerificationVerifier>,
    /// A path inside the repository to the captured request and
    /// response; `null` (absent) for `structural` and `source-checked`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<String>,
    pub outcome: VerificationOutcome,
}

impl VerificationRecord {
    /// Checks the CF-04 section 3 rules that a single record can be
    /// held to, returning a reason when it cannot.
    ///
    /// Freshness (the 30-day window of D-05) and the historical state
    /// are deliberately *not* checked here: they are derived at display
    /// time against the clock and the fixture's current hash, never
    /// stored.
    pub fn check(&self, fixture_id: &str) -> Result<(), &'static str> {
        if self.verification_version != VERIFICATION_VERSION {
            return Err("verificationVersion must be 1");
        }
        if self.fixture_id != fixture_id {
            return Err("fixtureId does not match the fixture it is embedded in");
        }
        if !is_rfc3339_utc(&self.checked_at) {
            return Err("checkedAt must be an RFC 3339 UTC instant, e.g. 2026-09-28T07:20:00Z");
        }
        if self.method.is_live() && self.network.is_none() {
            return Err("network is required for live verification methods");
        }
        if let Some(host) = &self.endpoint_host {
            if host.is_empty() || host.contains(['/', '?', '#', ' ']) || host.starts_with('[') {
                return Err("endpointHost must be a bare host name: no path, query, port or token");
            }
        }
        if let Some(digest) = &self.fixture_sha256 {
            let is_hex = digest.len() == 64
                && digest
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
            if !is_hex {
                return Err("fixtureSha256 must be 64 lowercase hex characters");
            }
        }
        Ok(())
    }
}

/// A structural check that `value` looks like an RFC 3339 UTC instant:
/// `YYYY-MM-DDTHH:MM:SSZ`. Deliberately shape-only — CF-04 asks for the
/// full instant, and validating the calendar belongs to a date library,
/// not to fixture loading.
fn is_rfc3339_utc(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && bytes[10] == b'T'
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16)
        && bytes[16] == b':'
        && digits(17..19)
        && bytes[19] == b'Z'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> VerificationRecord {
        VerificationRecord {
            verification_version: 1,
            fixture_id: "p28-rpc-1".to_string(),
            fixture_sha256: None,
            method: VerificationMethod::Structural,
            checked_at: "2026-09-28T07:20:00Z".to_string(),
            network: None,
            endpoint_host: None,
            verifier: None,
            evidence_ref: None,
            outcome: VerificationOutcome::Matched,
        }
    }

    #[test]
    fn a_structural_record_without_a_network_is_valid() {
        assert_eq!(record().check("p28-rpc-1"), Ok(()));
    }

    #[test]
    fn a_live_record_needs_a_network() {
        let mut live = record();
        live.method = VerificationMethod::LiveReadOnly;
        assert_eq!(
            live.check("p28-rpc-1"),
            Err("network is required for live verification methods")
        );
        live.network = Some(VerificationNetwork {
            name: "testnet".to_string(),
            passphrase: "Test SDF Network ; September 2015".to_string(),
            observed_protocol: 28,
        });
        assert_eq!(live.check("p28-rpc-1"), Ok(()));
    }

    #[test]
    fn a_record_for_another_fixture_id_is_rejected() {
        assert_eq!(
            record().check("p28-rpc-2"),
            Err("fixtureId does not match the fixture it is embedded in")
        );
    }

    #[test]
    fn checked_at_must_be_an_rfc3339_utc_instant() {
        let mut record = record();
        record.checked_at = "2026-09-28".to_string();
        assert!(record.check("p28-rpc-1").is_err());
        record.checked_at = "2026-09-28T07:20:00+02:00".to_string();
        assert!(record.check("p28-rpc-1").is_err());
        record.checked_at = "2026-09-28T07:20:00Z".to_string();
        assert_eq!(record.check("p28-rpc-1"), Ok(()));
    }

    #[test]
    fn endpoint_host_is_a_bare_host_name() {
        let mut record = record();
        record.endpoint_host = Some("rpc.example.invalid".to_string());
        assert_eq!(record.check("p28-rpc-1"), Ok(()));
        for bad in [
            "https://rpc.example.invalid",
            "rpc.example.invalid/path",
            "rpc.example.invalid?token=x",
            "",
        ] {
            record.endpoint_host = Some(bad.to_string());
            assert!(record.check("p28-rpc-1").is_err(), "{bad}");
        }
    }

    #[test]
    fn fixture_sha256_must_be_lowercase_hex() {
        let mut record = record();
        record.fixture_sha256 = Some("a".repeat(64));
        assert_eq!(record.check("p28-rpc-1"), Ok(()));
        record.fixture_sha256 = Some("A".repeat(64));
        assert!(record.check("p28-rpc-1").is_err());
        record.fixture_sha256 = Some("abc".to_string());
        assert!(record.check("p28-rpc-1").is_err());
    }

    #[test]
    fn methods_and_outcomes_keep_their_kebab_case_names() {
        assert_eq!(VerificationMethod::Structural.as_str(), "structural");
        assert_eq!(VerificationMethod::SourceChecked.as_str(), "source-checked");
        assert_eq!(VerificationMethod::LiveReadOnly.as_str(), "live-read-only");
        assert_eq!(
            VerificationMethod::LiveSimulation.as_str(),
            "live-simulation"
        );
        assert_eq!(VerificationOutcome::Matched.as_str(), "matched");
        assert_eq!(VerificationOutcome::Mismatched.as_str(), "mismatched");
    }

    #[test]
    fn a_record_round_trips_through_toml() {
        let mut stored = record();
        stored.method = VerificationMethod::LiveSimulation;
        stored.network = Some(VerificationNetwork {
            name: "testnet".to_string(),
            passphrase: "Test SDF Network ; September 2015".to_string(),
            observed_protocol: 28,
        });
        stored.verifier = Some(VerificationVerifier {
            name: "stellar-canary".to_string(),
            version: "0.2.0".to_string(),
        });
        // The camelCase keys the JSON form of CF-04 uses, written out:
        // the table is stored in a fixture exactly like this.
        let table: toml::Value = toml::from_str(
            r#"
            verificationVersion = 1
            fixtureId = "p28-rpc-1"
            method = "live-simulation"
            checkedAt = "2026-09-28T07:20:00Z"
            outcome = "matched"

            [network]
            name = "testnet"
            passphrase = "Test SDF Network ; September 2015"
            observedProtocol = 28

            [verifier]
            name = "stellar-canary"
            version = "0.2.0"
            "#,
        )
        .unwrap();
        let parsed: VerificationRecord = table.try_into().expect("deserializes");
        assert_eq!(parsed, stored);

        let rendered = toml::Value::try_from(&parsed).expect("serializes");
        assert_eq!(
            rendered.get("method").unwrap().as_str(),
            Some("live-simulation")
        );
        assert_eq!(
            rendered["network"]["observedProtocol"].as_integer(),
            Some(28)
        );
    }

    #[test]
    fn an_unknown_key_in_the_table_is_rejected() {
        let table: toml::Value = toml::from_str(
            r#"
            verificationVersion = 1
            fixtureId = "p28-rpc-1"
            method = "structural"
            checkedAt = "2026-09-28T07:20:00Z"
            outcome = "matched"
            methodx = "typo"
            "#,
        )
        .unwrap();
        let parsed: Result<VerificationRecord, _> = table.try_into();
        assert!(parsed.is_err(), "deny_unknown_fields must reject methodx");
    }
}
