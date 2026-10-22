//! End-to-end tests for `stellar-canary fixture-pack verify` (issue #432).
//!
//! The command is read-only and exits `0` for a pack whose registered file
//! hashes and canonical CF-02 digest match, `1` for a checksum mismatch, and
//! names every corrupted path on stderr.

mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use serde_json::{json, Value};

fn fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

/// Writes a registry describing exactly the files the canonical digest sees.
fn write_registry(project: &TempProject, pack: &str) -> (String, Vec<String>) {
    let root = project.path.join(pack);
    let digest = canary_fixtures::pack_digest(&root).expect("test pack digests");
    let fixtures: Vec<Value> = digest
        .files
        .iter()
        .map(|file| {
            let id = file
                .path
                .trim_end_matches(".toml")
                .replace('/', "-");
            json!({
                "id": id,
                "path": file.path,
                "sha256": file.sha256,
                "surface": "xdr",
                "category": "test",
                "description": "test",
                "payloads": []
            })
        })
        .collect();
    let registry = json!({
        "registryVersion": 1,
        "protocol": 28,
        "fixtures": fixtures,
        "packDigest": digest.digest
    });
    project.write(
        &format!("{pack}/registry.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&registry).expect("registry serializes")
        ),
    );
    let paths = digest.files.into_iter().map(|file| file.path).collect();
    (digest.digest, paths)
}

#[test]
#[ignore = "fixture-pack verify subcommand is not yet wired into the CLI (issue #432)"]
fn a_conforming_directory_pack_verifies_and_prints_its_digest() {
    let project = TempProject::new("fixture-pack-verify-ok");
    project.write("pack/xdr/example.toml", &fixture("p28-xdr-example"));
    let (digest, _) = write_registry(&project, "pack");

    let output = run_in(&project.path, &["fixture-pack", "verify", "pack"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stdout(&output).contains(&digest), "{}", stdout(&output));
    assert!(stdout(&output).contains("registryVersion: 1"));
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
}

#[test]
#[ignore = "fixture-pack verify subcommand is not yet wired into the CLI (issue #432)"]
fn one_corrupted_byte_exits_one_and_names_the_path() {
    let project = TempProject::new("fixture-pack-verify-corrupt");
    project.write("pack/xdr/example.toml", &fixture("p28-xdr-example"));
    write_registry(&project, "pack");
    project.write("pack/xdr/example.toml", "tampered\n");

    let output = run_in(&project.path, &["fixture-pack", "verify", "pack"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains("xdr/example.toml"), "{err}");
    assert!(err.contains("checksum mismatch"), "{err}");
}

#[test]
#[ignore = "fixture-pack verify subcommand is not yet wired into the CLI (issue #432)"]
fn a_pack_without_a_registry_is_an_invalid_fixture() {
    let project = TempProject::new("fixture-pack-verify-no-registry");
    project.write("pack/xdr/example.toml", &fixture("p28-xdr-example"));

    let output = run_in(&project.path, &["fixture-pack", "verify", "pack"]);
    assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
    assert!(stderr(&output).contains("no fixture registry"));
}

#[test]
#[ignore = "fixture-pack verify subcommand is not yet wired into the CLI (issue #432)"]
fn registry_v1_json_is_accepted() {
    let project = TempProject::new("fixture-pack-verify-v1-name");
    project.write("pack/xdr/example.toml", &fixture("p28-xdr-example"));
    let root = project.path.join("pack");
    let digest = canary_fixtures::pack_digest(&root).expect("test pack digests");
    let registry = json!({
        "registryVersion": 1,
        "protocol": 28,
        "fixtures": [{
            "id": "p28-xdr-example",
            "path": "xdr/example.toml",
            "sha256": digest.files[0].sha256,
            "payloads": []
        }],
        "packDigest": digest.digest
    });
    project.write(
        "pack/registry-v1.json",
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&registry).expect("registry serializes")
        ),
    );

    let output = run_in(&project.path, &["fixture-pack", "verify", "pack"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
}
