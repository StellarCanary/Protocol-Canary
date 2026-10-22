//! Verification evidence sidecar loading (`[E-082]`, issue #443).
//!
//! A fixture's CF-04 record can come from an embedded `[verification]`
//! table or from a `<name>.evidence.json` sidecar file. Each test here
//! builds a fixture directory with sidecars and asserts that records are
//! attached when they are valid, and — the negative cases — that a
//! malformed or CF-04-invalid sidecar is reported as a warning while the
//! fixture loads as unverified instead of crashing the load.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use canary_fixtures::{
    load_directory, load_directory_with_evidence, VerificationMethod, VerificationOutcome,
};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A directory under the system temp dir, removed when the test ends.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "canary-verification-loader-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir is creatable");
        TempDir { path }
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent is creatable");
        }
        std::fs::write(&path, contents).expect("file is writable");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A minimal valid fixture header, with the fixture's own body key so
/// the file differs from a bare manifest.
fn fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"test\"\ndescription = \"test fixture\"\nmethod = \"get-network\"\n"
    )
}

/// A complete, CF-04-valid evidence record for `fixture_id`, as an
/// `evidenceRef` capture would store it (CF-04 section 3).
fn valid_evidence(fixture_id: &str) -> String {
    format!(
        r#"{{
          "verificationVersion": 1,
          "fixtureId": "{fixture_id}",
          "method": "live-read-only",
          "checkedAt": "2026-09-28T07:20:00Z",
          "network": {{
            "name": "testnet",
            "passphrase": "Test SDF Network ; September 2015",
            "observedProtocol": 28
          }},
          "endpointHost": "rpc.example.invalid",
          "verifier": {{ "name": "stellar-canary", "version": "0.2.0" }},
          "evidenceRef": "evidence/{fixture_id}.json",
          "outcome": "matched"
        }}"#
    )
}

#[test]
fn a_valid_sidecar_attaches_the_record_and_its_verification_fields() {
    let dir = TempDir::new("valid-sidecar");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));
    dir.write("p28-rpc-001.evidence.json", &valid_evidence("p28-rpc-001"));

    let outcome = load_directory_with_evidence(&dir.path).expect("loads");
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    let loaded = &outcome.fixtures[0];
    let record = loaded.verification.as_ref().expect("a verification record");

    assert_eq!(record.method, VerificationMethod::LiveReadOnly);
    assert_eq!(record.checked_at, "2026-09-28T07:20:00Z");
    assert_eq!(record.outcome, VerificationOutcome::Matched);
    assert_eq!(record.fixture_id, "p28-rpc-001");
    let verifier = record.verifier.as_ref().expect("verifier");
    assert_eq!(verifier.name, "stellar-canary");
    assert_eq!(verifier.version, "0.2.0");
    assert_eq!(
        record.network.as_ref().map(|net| net.observed_protocol),
        Some(28)
    );
    assert_eq!(
        record.evidence_ref.as_deref(),
        Some("evidence/p28-rpc-001.json")
    );
}

#[test]
fn an_evidence_directory_sidecar_is_matched_by_fixture_id() {
    // The fixture file's stem (`network`) differs from its id, so this
    // pins that the id lookup reaches into `evidence/` elsewhere in the
    // tree.
    let dir = TempDir::new("evidence-dir");
    dir.write("rpc/network.toml", &fixture("p28-rpc-network"));
    dir.write(
        "evidence/p28-rpc-network.evidence.json",
        &valid_evidence("p28-rpc-network"),
    );

    let outcome = load_directory_with_evidence(&dir.path).expect("loads");
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    let record = outcome.fixtures[0]
        .verification
        .as_ref()
        .expect("a verification record");
    assert_eq!(record.fixture_id, "p28-rpc-network");
}

#[test]
fn a_malformed_evidence_json_warns_and_leaves_the_fixture_unverified() {
    let dir = TempDir::new("malformed");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));
    let sidecar = dir.write("p28-rpc-001.evidence.json", "{ not json");

    let outcome = load_directory_with_evidence(&dir.path).expect("the load does not crash");
    assert!(
        outcome.fixtures[0].verification.is_none(),
        "the fixture stays unverified"
    );
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert_eq!(outcome.warnings[0].path, sidecar);
    assert!(
        outcome.warnings[0]
            .reason
            .contains("invalid verification record"),
        "{:?}",
        outcome.warnings[0]
    );
}

#[test]
fn an_evidence_record_for_another_fixture_warns_and_leaves_the_fixture_unverified() {
    let dir = TempDir::new("id-mismatch");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));
    dir.write(
        "p28-rpc-001.evidence.json",
        &valid_evidence("p28-rpc-someone-else"),
    );

    let outcome = load_directory_with_evidence(&dir.path).expect("the load does not crash");
    assert!(outcome.fixtures[0].verification.is_none());
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert!(
        outcome.warnings[0]
            .reason
            .contains("fixtureId does not match"),
        "{:?}",
        outcome.warnings[0]
    );
}

#[test]
fn a_live_record_without_a_network_warns_and_leaves_the_fixture_unverified() {
    // CF-04 section 3: `network` is required for live methods. JSON has
    // no comments, so drop the network object wholesale.
    let dir = TempDir::new("live-no-network");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));
    let evidence = valid_evidence("p28-rpc-001");
    let without_network = evidence.replace(
        r#""network": {
            "name": "testnet",
            "passphrase": "Test SDF Network ; September 2015",
            "observedProtocol": 28
          },
          "#,
        "",
    );
    assert_ne!(without_network, evidence, "the network block was removed");
    dir.write("p28-rpc-001.evidence.json", &without_network);

    let outcome = load_directory_with_evidence(&dir.path).expect("the load does not crash");
    assert!(outcome.fixtures[0].verification.is_none());
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert!(
        outcome.warnings[0].reason.contains("network is required"),
        "{:?}",
        outcome.warnings[0]
    );
}

#[test]
fn an_unknown_key_in_an_evidence_file_warns_and_leaves_the_fixture_unverified() {
    let dir = TempDir::new("unknown-key");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));
    let evidence = valid_evidence("p28-rpc-001").replace(
        "\"outcome\": \"matched\"",
        "\"outcome\": \"matched\", \"methodx\": \"typo\"",
    );
    dir.write("p28-rpc-001.evidence.json", &evidence);

    let outcome = load_directory_with_evidence(&dir.path).expect("the load does not crash");
    assert!(outcome.fixtures[0].verification.is_none());
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
}

#[test]
fn the_embedded_verification_table_wins_over_a_sidecar() {
    let dir = TempDir::new("embedded-wins");
    dir.write(
        "p28-rpc-001.toml",
        &format!(
            "{}\n[verification]\nverificationVersion = 1\nfixtureId = \"p28-rpc-001\"\nmethod = \"structural\"\ncheckedAt = \"2026-09-28T07:20:00Z\"\noutcome = \"matched\"\n",
            fixture("p28-rpc-001")
        ),
    );
    dir.write("p28-rpc-001.evidence.json", &valid_evidence("p28-rpc-001"));

    let outcome = load_directory_with_evidence(&dir.path).expect("loads");
    let record = outcome.fixtures[0]
        .verification
        .as_ref()
        .expect("the embedded record");
    assert_eq!(record.method, VerificationMethod::Structural);
    // The conflicting sidecar is reported, not silently dropped.
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert!(
        outcome.warnings[0]
            .reason
            .contains("embedded [verification] table"),
        "{:?}",
        outcome.warnings[0]
    );
}

#[test]
fn a_fixture_without_any_evidence_is_plainly_unverified() {
    let dir = TempDir::new("no-evidence");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));

    let outcome = load_directory_with_evidence(&dir.path).expect("loads");
    assert!(outcome.fixtures[0].verification.is_none());
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
}

#[test]
fn an_evidence_file_that_matches_no_fixture_is_reported() {
    let dir = TempDir::new("orphan");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));
    dir.write(
        "evidence/p28-xdr-999.evidence.json",
        &valid_evidence("p28-xdr-999"),
    );

    let outcome = load_directory_with_evidence(&dir.path).expect("loads");
    assert!(outcome.fixtures[0].verification.is_none());
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert!(
        outcome.warnings[0].reason.contains("no fixture"),
        "{:?}",
        outcome.warnings[0]
    );
}

#[test]
fn duplicate_sidecars_for_one_stem_use_the_first_and_report_the_rest() {
    let dir = TempDir::new("duplicate");
    dir.write("rpc/p28-rpc-001.toml", &fixture("p28-rpc-001"));
    // "evidence/" sorts before "rpc/", so the first in sorted order is
    // the one under `evidence/`.
    dir.write(
        "evidence/p28-rpc-001.evidence.json",
        &valid_evidence("p28-rpc-001"),
    );
    let duplicate = dir.write("rpc/p28-rpc-001.evidence.json", "{ not json");

    let outcome = load_directory_with_evidence(&dir.path).expect("loads");
    let record = outcome.fixtures[0]
        .verification
        .as_ref()
        .expect("the valid sidecar is used");
    assert_eq!(record.method, VerificationMethod::LiveReadOnly);
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert_eq!(outcome.warnings[0].path, duplicate);
    assert!(
        outcome.warnings[0].reason.contains("duplicate evidence"),
        "{:?}",
        outcome.warnings[0]
    );
}

#[test]
fn an_oversized_evidence_file_is_skipped_with_a_warning() {
    let dir = TempDir::new("oversized");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));
    // One byte over the loader's fixture-file limit.
    dir.write(
        "p28-rpc-001.evidence.json",
        &" ".repeat(canary_fixtures::MAX_FIXTURE_BYTES as usize + 1),
    );

    let outcome = load_directory_with_evidence(&dir.path).expect("the load does not fail");
    assert!(outcome.fixtures[0].verification.is_none());
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert!(
        outcome.warnings[0].reason.contains("byte limit"),
        "{:?}",
        outcome.warnings[0]
    );
}

#[test]
fn load_directory_keeps_its_signature_and_survives_a_broken_sidecar() {
    // The plain entry point prints warnings and returns the fixtures;
    // the broken sidecar must not reach the caller as an error.
    let dir = TempDir::new("entry-point");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));
    dir.write("p28-rpc-001.evidence.json", "][");

    let fixtures = load_directory(&dir.path).expect("the load does not crash");
    assert_eq!(fixtures.len(), 1);
    assert!(fixtures[0].verification.is_none());
}

#[test]
fn non_evidence_json_files_are_left_alone() {
    // A capture named by a record's `evidenceRef` is data, not a record:
    // the loader must not read or warn about it.
    let dir = TempDir::new("capture");
    dir.write("p28-rpc-001.toml", &fixture("p28-rpc-001"));
    dir.write(
        "evidence/p28-rpc-001.json",
        "{\"jsonrpc\":\"2.0\",\"id\":1}",
    );

    let outcome = load_directory_with_evidence(&dir.path).expect("loads");
    assert!(outcome.fixtures[0].verification.is_none());
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
}

#[test]
fn multiple_fixtures_each_claim_their_own_sidecar() {
    let dir = TempDir::new("many");
    for id in ["p28-rpc-001", "p28-rpc-002", "p28-rpc-003"] {
        dir.write(&format!("{id}.toml"), &fixture(id));
        dir.write(&format!("evidence/{id}.evidence.json"), &valid_evidence(id));
    }
    // One broken sidecar must not disturb the others.
    dir.write("p28-rpc-004.toml", &fixture("p28-rpc-004"));
    dir.write("p28-rpc-004.evidence.json", "not json at all");

    let outcome = load_directory_with_evidence(&dir.path).expect("loads");
    let verified = outcome
        .fixtures
        .iter()
        .filter(|f| f.verification.is_some())
        .count();
    assert_eq!(verified, 3);
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
}
