//! Project detection for Stellar Protocol Canary.

pub mod capabilities;
pub mod detector;
pub mod manifest;

pub use capabilities::{detect_capabilities, DetectionSignals};
pub use detector::{detect, resolve_project_type};
pub use manifest::{read_cargo_manifest, CargoManifest};

/// Test-only temp-directory helper shared by this crate's unit tests, so no
/// crate needs a `tempfile` dev-dependency for simple filesystem fixtures.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    pub struct TempDir {
        pub path: PathBuf,
    }

    pub fn temp_dir(prefix: &str) -> TempDir {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
