//! Fixture model, loading, and validation for Stellar Protocol Canary.
//!
//! This crate loads fixture *metadata* and hands each fixture's
//! surface-specific body (as a raw [`toml::Value`]) to the caller; it does
//! not know how to interpret an XDR, RPC, or Soroban fixture body itself.

pub mod archive;
pub mod digest;
pub mod loader;
pub mod manifest;
pub mod validator;
pub mod verification;
pub mod registry;

pub use archive::{
    extract_archive, extract_archive_with_limits, ArchiveLimits, ArchiveSafetyError,
    ExtractionSummary, MAX_ENTRIES, MAX_FILE_BYTES, MAX_PATH_DEPTH, MAX_TOTAL_BYTES,
};
pub use digest::{pack_digest, PackDigest, PackError, PackFile};
pub use loader::{load_directory, FixtureError, MAX_FIXTURE_BYTES};
pub use manifest::{
    content_digest, parse_fixture_file, parse_fixture_str, unknown_body_keys, LoadedFixture,
};
pub use validator::validate;
pub use verification::{
    VerificationMethod, VerificationNetwork, VerificationOutcome, VerificationRecord,
    VerificationVerifier, VERIFICATION_VERSION,
};

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    /// A scratch directory created by [`temp_dir`], removed when dropped.
    ///
    /// Dropping it deletes [`path`](TempDir::path) and everything in it
    /// (errors are ignored). That includes a test that panics, since the
    /// value is dropped during unwinding, but not a process that aborts or
    /// is killed, which leaves the directory behind. Bind it to a named
    /// variable for the whole test: `let _ = temp_dir(..)` drops it, and so
    /// deletes the directory, immediately.
    pub struct TempDir {
        pub path: PathBuf,
    }

    /// Creates a fresh directory under [`std::env::temp_dir`] and returns a
    /// [`TempDir`] that removes it on drop, so callers don't clean up.
    ///
    /// The directory is named
    /// `{prefix}-{process id}-{nanoseconds since the Unix epoch}-{counter}`.
    /// The process id separates concurrent test processes, while the
    /// timestamp and the per-process counter separate calls within one
    /// process: the timestamp alone is not a uniqueness guarantee across the
    /// threads the test harness runs in parallel, since clock resolution on
    /// some hosts is coarser than the interval between two threads' reads. A
    /// collision would make two tests share one directory, and one test's
    /// `Drop` would then delete the other's fixture mid-run.
    ///
    /// # Panics
    ///
    /// Panics if the directory cannot be created.
    pub fn temp_dir(prefix: &str) -> TempDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

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
