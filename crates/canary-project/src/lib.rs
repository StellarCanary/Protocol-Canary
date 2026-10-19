//! Project detection for Stellar Protocol Canary.

pub mod capabilities;
pub mod detector;
pub mod fingerprint;
pub mod manifest;
pub mod models;
pub mod workspace;

pub use capabilities::{detect_capabilities, detect_capabilities_models, DetectionSignals};
pub use detector::{
    detect, detect_details, find_wasm_artifacts, resolve_project_type, CapabilityEvidence,
    DetailedDetection, DetectionEvidence,
};
pub use fingerprint::{
    compute_fingerprint, ProjectFingerprint, DETECTOR_REVISION, FINGERPRINT_VERSION,
};
pub use manifest::{
    parse_cargo_dependencies, parse_package_json_dependencies, read_cargo_manifest,
    read_package_json, ManifestDep, ProjectManifest,
};
pub use models::{CapabilityEvidence as ModelCapabilityEvidence, CapabilityModel};
pub use workspace::{
    scan_cargo_workspace, Diagnostic, DiagnosticCode, WorkspaceMember, WorkspaceScan,
    MAX_WORKSPACE_DEPTH, MAX_WORKSPACE_MEMBERS,
};

/// Test-only temp-directory helper shared by this crate's unit tests, so no
/// crate needs a `tempfile` dev-dependency for simple filesystem fixtures.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Owns a test fixture directory and removes it when dropped.
    ///
    /// Instances are created by [`temp_dir`] and expose the directory path
    /// used by the calling test to create filesystem fixtures.
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
