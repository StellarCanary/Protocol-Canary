//! Project detection for Stellar Protocol Canary.

pub mod capabilities;
pub mod detector;
pub mod manifest;

pub use capabilities::{detect_capabilities, DetectionSignals};
pub use detector::{detect, resolve_project_type};
pub use manifest::{read_cargo_manifest, read_package_json, ProjectManifest};

/// Test-only temp-directory helper shared by this crate's unit tests, so no
/// crate needs a `tempfile` dev-dependency for simple filesystem fixtures.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    pub struct TempDir {
        pub path: PathBuf,
    }

    /// Creates a directory under [`std::env::temp_dir`], removed on drop.
    ///
    /// The name combines the process id, a nanosecond timestamp and this
    /// per-process counter. The timestamp alone is not a uniqueness guarantee
    /// across the threads the test harness runs in parallel, since clock
    /// resolution on some hosts is coarser than the interval between two
    /// threads' reads; a collision would share one directory between two
    /// tests, and one test's `Drop` would delete the other's fixture mid-run.
    pub fn temp_dir(prefix: &str) -> TempDir {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "{prefix}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        path.push(unique);
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
