//! Fixture model, loading, and validation for Stellar Protocol Canary.
//!
//! This crate loads fixture *metadata* and hands each fixture's
//! surface-specific body (as a raw [`toml::Value`]) to the caller; it does
//! not know how to interpret an XDR, RPC, or Soroban fixture body itself.

pub mod loader;
pub mod manifest;
pub mod validator;

pub use loader::{load_directory, FixtureError};
pub use manifest::{parse_fixture_file, parse_fixture_str, LoadedFixture};
pub use validator::validate;

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
