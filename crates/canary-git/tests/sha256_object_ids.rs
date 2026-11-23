//! Verifies that repository discovery accepts SHA-256 (64-character) object IDs.
//!
//! The layout is built by hand so the test neither needs Git's SHA-256 object
//! storage nor a real commit object: `HEAD` names a branch, and that branch's
//! loose ref holds a 64-character hexadecimal ID.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_git::resolve_head;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Scratch directory cleaned up on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "canary-git-test-{prefix}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("failed to create scratch directory");
        Scratch(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn resolve_head_accepts_a_64_character_object_id() {
    let scratch = Scratch::new("sha256");
    let git_dir = scratch.path().join(".git");
    std::fs::create_dir_all(git_dir.join("refs").join("heads")).unwrap();
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();

    // Realistic-looking SHA-256 hex, deliberately not a commit this Git knows.
    let object_id = "9f2c4a71b8de0563a1c9f7d2e4b60a8c35d1e9f0a7b2c6d4e8f1a3b5c7d9e0f2";
    std::fs::write(
        git_dir.join("refs").join("heads").join("main"),
        format!("{object_id}\n"),
    )
    .unwrap();

    let head = resolve_head(scratch.path()).expect("a 64-character object ID is valid");
    assert_eq!(head.commit(), object_id);
    assert_eq!(head.branch(), Some("main"));
    assert!(!head.is_detached());
}
