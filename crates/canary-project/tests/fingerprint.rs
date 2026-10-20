use canary_core::ProjectType;
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
