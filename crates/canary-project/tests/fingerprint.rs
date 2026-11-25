use canary_core::{Capability, ProjectType};
use canary_project::compute_fingerprint;
use std::path::PathBuf;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = format!(
            "fingerprint-integration-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fingerprint_is_deterministic_and_ignores_unrelated_files() {
    let project = TempDir::new();
    std::fs::write(
        project.0.join("Cargo.toml"),
        "[package]\nname = \"fingerprint-fixture\"\n",
    )
    .unwrap();

    let first = compute_fingerprint(&project.0, ProjectType::Unknown, &[], None);
    let repeated = compute_fingerprint(&project.0, ProjectType::Unknown, &[], None);
    assert_eq!(first.digest(), repeated.digest());
    assert!(first
        .digest()
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))));

    std::fs::write(project.0.join("comment.txt"), "unrelated note\n").unwrap();
    let with_comment = compute_fingerprint(&project.0, ProjectType::Unknown, &[], None);
    assert_eq!(first.digest(), with_comment.digest());
}

/// CA-04: the fingerprint covers the bytes of the manifests the detector
/// read, so editing a dependency must change the digest even though the
/// path is unchanged.
#[test]
fn modifying_manifest_dependency_content_changes_the_digest() {
    let project = TempDir::new();
    std::fs::write(
        project.0.join("Cargo.toml"),
        "[package]\nname = \"fingerprint-fixture\"\n\n[dependencies]\nstellar-xdr = \"21\"\n",
    )
    .unwrap();
    let before = compute_fingerprint(&project.0, ProjectType::Unknown, &[], None);

    std::fs::write(
        project.0.join("Cargo.toml"),
        "[package]\nname = \"fingerprint-fixture\"\n\n[dependencies]\nstellar-xdr = \"22\"\n",
    )
    .unwrap();
    let after = compute_fingerprint(&project.0, ProjectType::Unknown, &[], None);

    assert_ne!(
        before.digest(),
        after.digest(),
        "a changed dependency must change the digest"
    );
    assert_eq!(before.inputs(), after.inputs());
}

/// CA-04: detected capabilities are digest input, so a project that gains a
/// capability must not keep the fingerprint of one that lacks it.
#[test]
fn detected_capabilities_change_the_digest() {
    let project = TempDir::new();
    std::fs::write(
        project.0.join("Cargo.toml"),
        "[package]\nname = \"fingerprint-fixture\"\n",
    )
    .unwrap();

    let without = compute_fingerprint(&project.0, ProjectType::Unknown, &[], None);
    let with_rpc = compute_fingerprint(
        &project.0,
        ProjectType::Unknown,
        &[Capability::RpcClient],
        None,
    );

    assert_ne!(
        without.digest(),
        with_rpc.digest(),
        "declaring a capability must change the digest"
    );
}

/// CA-04 canonicalizes capability order: the same set in a different order
/// produces the same digest.
#[test]
fn capability_order_does_not_change_the_digest() {
    let project = TempDir::new();
    std::fs::write(
        project.0.join("Cargo.toml"),
        "[package]\nname = \"fingerprint-fixture\"\n",
    )
    .unwrap();

    let first = compute_fingerprint(
        &project.0,
        ProjectType::Unknown,
        &[Capability::RpcClient, Capability::SorobanContract],
        None,
    );
    let reordered = compute_fingerprint(
        &project.0,
        ProjectType::Unknown,
        &[Capability::SorobanContract, Capability::RpcClient],
        None,
    );

    assert_eq!(
        first.digest(),
        reordered.digest(),
        "capabilities are sorted before hashing"
    );
}
