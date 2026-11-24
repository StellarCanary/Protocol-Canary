//! Captures the Git commit the binary is built from, for the report's
//! `run.toolBuildCommit` (CF-01).
//!
//! The commit is exposed to the crate as the compile-time variable
//! `CANARY_BUILD_COMMIT`, which is always set: to the full object name, or to
//! the empty string when it cannot be determined (a source archive, a
//! packaged crate, no `git` executable). This script never fails the build,
//! and never records a timestamp, so two builds of one commit stay
//! byte-for-byte comparable.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let commit = git_output(&["rev-parse", "HEAD"])
        .filter(|commit| is_full_object_name(commit))
        .unwrap_or_default();
    println!("cargo:rustc-env=CANARY_BUILD_COMMIT={commit}");

    if commit.is_empty() {
        return;
    }

    // Re-run when the checked-out commit moves. A branch commit rewrites the
    // ref file (or `packed-refs`), a checkout rewrites `HEAD`.
    watch(git_path("HEAD"));
    if let Some(reference) = git_output(&["symbolic-ref", "-q", "HEAD"]) {
        watch(git_path(&reference));
    }
    watch(git_path("packed-refs"));
}

fn watch(path: Option<PathBuf>) {
    if let Some(path) = path.filter(|path| path.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn git_path(name: &str) -> Option<PathBuf> {
    git_output(&["rev-parse", "--git-path", name]).map(PathBuf::from)
}

/// Runs `git` in the crate directory and returns its trimmed stdout, or
/// `None` on any failure: git missing, not a repository, non-zero exit.
fn git_output(args: &[&str]) -> Option<String> {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")?;
    let output = Command::new("git")
        .args(args)
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// A full SHA-1 (40) or SHA-256 (64) object name in lowercase hex.
fn is_full_object_name(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64)
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
