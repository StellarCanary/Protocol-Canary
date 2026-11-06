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
