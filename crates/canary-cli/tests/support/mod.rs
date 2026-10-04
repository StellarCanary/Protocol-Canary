//! Shared helpers for `stellar-canary` end-to-end tests.
//!
//! These invoke the actual compiled binary in an isolated temporary
//! directory, so they exercise the full pipeline the way a real user
//! would, not the crate's internal APIs.
//!
//! This module is compiled once per integration test binary (each test
//! file that does `mod support;` gets its own copy), and no single test
//! file uses every helper here, so an unconditional `dead_code` allow is
//! appropriate rather than a per-binary lie about what's "used".
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::{Command, Output};

/// A temporary directory that is removed when dropped.
pub struct TempProject {
    pub path: PathBuf,
}

impl TempProject {
    /// Creates a unique scratch directory under [`std::env::temp_dir`].
    ///
    /// The name combines the process id, a nanosecond timestamp and a
    /// per-process counter: the timestamp alone cannot separate the threads
    /// the test harness runs in parallel, since clock resolution on some
    /// hosts is coarser than the interval between two threads' reads, and two
    /// tests sharing one directory would delete each other's project via
    /// `Drop`.
    pub fn new(prefix: &str) -> Self {
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
        TempProject { path }
    }

    pub fn write(&self, relative_path: &str, contents: &str) {
        let full = self.path.join(relative_path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A well-formed, 48-zero-byte `StellarValue` (STELLAR_VALUE_BASIC),
/// base64-encoded — verified in canary-xdr's own tests to decode and
/// round-trip successfully.
pub const VALID_STELLAR_VALUE_BASE64: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// Runs the built `stellar-canary` binary with `args` inside `dir`.
pub fn run_in(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stellar-canary"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to execute stellar-canary binary")
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
