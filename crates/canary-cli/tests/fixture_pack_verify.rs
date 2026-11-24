//! Source-checkout and archive registry paths for `fixture-pack verify`.

mod support;

use canary_fixtures::digest::{pack_digest, sha256_hex};
use serde_json::json;
use support::{run_in, stderr, stdout, TempProject};

fn prepared_pack() -> (TempProject, std::path::PathBuf, Vec<u8>) {
    let temp = TempProject::new("fixture-pack-verify");
    let pack = temp.path.join("protocol-30");
    let fixture = b"# test-only synthetic input; no protocol claim\nid = \"test-only-fixture\"\n";
    std::fs::create_dir_all(pack.join("xdr")).unwrap();
    std::fs::write(pack.join("xdr/test-only.toml"), fixture).unwrap();

    let digest = pack_digest(&pack).unwrap().digest;
    let registry = json!({
        "registryVersion": 1,
        "protocol": 30,
        "fixtures": [{
            "id": "test-only-fixture",
            "path": "xdr/test-only.toml",
            "sha256": sha256_hex(fixture),
            "surface": "xdr",
            "category": "test-only",
            "description": "synthetic verifier input, not protocol evidence",
            "payloads": [],
            "requiredCapabilities": []
        }],
        "packDigest": digest
    });
    let bytes = serde_json::to_vec_pretty(&registry).unwrap();
    (temp, pack, bytes)
}

/// Runs `fixture-pack verify` with an explicit, external registry so the
/// pack itself never changes between digest computation and verification.
fn verify_with_registry(
    temp: &TempProject,
    pack: &std::path::Path,
    registry: &std::path::Path,
) -> std::process::Output {
    run_in(
        &temp.path,
        &[
            "fixture-pack",
            "verify",
            pack.to_str().unwrap(),
            "--registry",
            registry.to_str().unwrap(),
        ],
    )
}

/// Returns a copy of a prepared registry with one field changed, keeping
/// the original fixture bytes and digest intact.
fn altered_registry(bytes: &[u8], change: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut registry: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    change(&mut registry);
    serde_json::to_vec_pretty(&registry).unwrap()
}

#[test]
fn verifies_a_source_checkout_with_an_external_generated_registry() {
    let (temp, pack, registry_bytes) = prepared_pack();
    let registry_path = temp.path.join("generated-registry.json");
    std::fs::write(&registry_path, registry_bytes).unwrap();

    let output = run_in(
        &temp.path,
        &[
            "fixture-pack",
            "verify",
            pack.to_str().unwrap(),
            "--registry",
            registry_path.to_str().unwrap(),
        ],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Fixture pack verification passed"));
    assert!(stdout(&output).contains("Pack digest matches registry"));
}

#[test]
fn verifies_the_existing_root_registry_path_used_by_release_archives() {
    let (temp, pack, registry_bytes) = prepared_pack();
    std::fs::write(pack.join("registry.json"), registry_bytes).unwrap();

    let output = run_in(
        &temp.path,
        &["fixture-pack", "verify", pack.to_str().unwrap()],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Fixture pack verification passed"));
}

#[test]
fn a_malformed_registry_is_a_configuration_error() {
    let (temp, pack, _) = prepared_pack();
    let registry_path = temp.path.join("malformed-registry.json");
    std::fs::write(&registry_path, b"{ this is not JSON }").unwrap();

    let output = verify_with_registry(&temp, &pack, &registry_path);
    let diagnostic = stderr(&output);

    assert_eq!(output.status.code(), Some(2), "{diagnostic}");
    assert!(
        diagnostic.contains("failed to parse registry manifest"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains(registry_path.to_str().unwrap()),
        "{diagnostic}"
    );
}

#[test]
fn a_missing_registry_is_a_configuration_error() {
    let (temp, pack, _) = prepared_pack();
    let registry_path = temp.path.join("absent-registry.json");
    assert!(!registry_path.exists());

    let output = verify_with_registry(&temp, &pack, &registry_path);
    let diagnostic = stderr(&output);

    assert_eq!(output.status.code(), Some(2), "{diagnostic}");
    assert!(
        diagnostic.contains("failed to read registry manifest"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains(registry_path.to_str().unwrap()),
        "{diagnostic}"
    );
}

#[test]
fn a_pack_digest_mismatch_is_a_verification_failure() {
    let (temp, pack, registry_bytes) = prepared_pack();
    let wrong_digest = format!("sha256:{}", "0".repeat(64));
    let registry = altered_registry(&registry_bytes, |value| {
        value["packDigest"] = serde_json::Value::String(wrong_digest.clone());
    });
    let registry_path = temp.path.join("wrong-pack-digest.json");
    std::fs::write(&registry_path, registry).unwrap();

    let output = verify_with_registry(&temp, &pack, &registry_path);
    let diagnostic = stderr(&output);

    assert_eq!(output.status.code(), Some(6), "{diagnostic}");
    assert!(diagnostic.contains("pack digest mismatch"), "{diagnostic}");
    assert!(diagnostic.contains("computed:"), "{diagnostic}");
    assert!(diagnostic.contains(&wrong_digest), "{diagnostic}");
    assert!(
        !diagnostic.contains("fixture checksum mismatch"),
        "the digest diagnostic must be distinct: {diagnostic}"
    );
}

#[test]
fn a_fixture_checksum_mismatch_is_a_verification_failure() {
    let (temp, pack, registry_bytes) = prepared_pack();
    let wrong_checksum = "0".repeat(64);
    let registry = altered_registry(&registry_bytes, |value| {
        value["fixtures"][0]["sha256"] = serde_json::Value::String(wrong_checksum.clone());
    });
    let registry_path = temp.path.join("wrong-fixture-checksum.json");
    std::fs::write(&registry_path, registry).unwrap();

    let output = verify_with_registry(&temp, &pack, &registry_path);
    let diagnostic = stderr(&output);

    assert_eq!(output.status.code(), Some(6), "{diagnostic}");
    assert!(
        diagnostic.contains("fixture checksum mismatch for xdr/test-only.toml"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains(&wrong_checksum), "{diagnostic}");
    assert!(
        !diagnostic.contains("pack digest mismatch"),
        "the checksum diagnostic must be distinct: {diagnostic}"
    );
}
