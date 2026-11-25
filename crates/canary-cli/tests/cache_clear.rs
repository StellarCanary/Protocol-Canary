mod support;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn xdr_fixture(value_base64: &str) -> String {
    format!(
        "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{value_base64}\"\n"
    )
}

/// Runs `check` once so the default cache directory holds an entry, then
/// verifies `cache clear` removes it and reports reclaimed bytes.
#[test]
fn cache_clear_reports_reclaimed_bytes() {
    let dir = TempProject::new("cache-clear");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture(VALID_STELLAR_VALUE_BASE64),
    );

    let first = run_in(&dir.path, &["check"]);
    assert_eq!(first.status.code(), Some(0), "{}", stdout(&first));
    assert!(
        dir.path.join(".stellar-canary-cache").is_dir(),
        "a result was cached"
    );

    let output = run_in(&dir.path, &["cache", "clear"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(
        out.contains("bytes"),
        "cache clear should report reclaimed bytes: {out}"
    );
    assert!(
        !dir.path.join(".stellar-canary-cache").exists(),
        "the cache directory was removed"
    );
}

/// An already-empty cache directory is not an error: it reports zero bytes.
#[test]
fn cache_clear_on_empty_cache_reports_zero_bytes() {
    let dir = TempProject::new("cache-clear-empty");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture(VALID_STELLAR_VALUE_BASE64),
    );

    let output = run_in(&dir.path, &["cache", "clear"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(
        out.contains("0 bytes"),
        "an empty cache reports zero bytes: {out}"
    );
}

/// A non-existent cache directory reports zero bytes without error.
#[test]
fn cache_clear_on_nonexistent_cache_directory_reports_zero_bytes() {
    let dir = TempProject::new("cache-clear-missing");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    let output = run_in(&dir.path, &["cache", "clear"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(
        out.contains("0 bytes"),
        "a missing cache reports zero bytes: {out}"
    );
    assert!(
        !dir.path.join(".stellar-canary-cache").exists(),
        "cache clear must not create a missing directory"
    );
}

/// Tests that `cache clear` fails when the default cache path
/// (`.stellar-canary-cache`) is a regular file instead of a directory.
#[test]
fn cache_clear_fails_when_cache_target_is_a_file() {
    let dir = TempProject::new("cache-clear-file");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    // Create a regular file at the default cache path.
    let cache_path = dir.path.join(".stellar-canary-cache");
    std::fs::write(&cache_path, "this is not a cache directory").unwrap();
    assert!(cache_path.is_file(), "setup: cache path should be a file");

    // Run cache clear and verify it fails.
    let output = run_in(&dir.path, &["cache", "clear"]);
    assert!(
        !output.status.success(),
        "cache clear should fail when cache path is a regular file; stdout: {}, stderr: {}",
        stdout(&output),
        stderr(&output)
    );

    // Verify the error message is informative.
    let err = stderr(&output);
    assert!(
        err.contains("failed to read cache directory") || err.contains("failed to clear cache"),
        "stderr should contain an informative error message; stderr: {err}"
    );

    // Verify the original file still exists and its contents are intact.
    assert!(
        cache_path.exists(),
        "original file should still exist"
    );
    let contents = std::fs::read_to_string(&cache_path).unwrap();
    assert_eq!(
        contents, "this is not a cache directory",
        "file contents should be preserved"
    );
}

/// Tests that `cache clear` fails when `--cache-dir` points to a regular
/// file instead of a directory.
#[test]
fn cache_clear_fails_with_explicit_file_cache_dir() {
    let dir = TempProject::new("cache-clear-explicit-file");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    // Create a regular file at an explicit cache directory path.
    let custom_cache = dir.path.join("custom-cache-file");
    std::fs::write(&custom_cache, "this is not a cache directory either").unwrap();
    assert!(custom_cache.is_file(), "setup: custom cache path should be a file");

    // Run cache clear with --cache-dir pointing to the file.
    let output = run_in(
        &dir.path,
        &["cache", "clear", "--cache-dir", "custom-cache-file"],
    );
    assert!(
        !output.status.success(),
        "cache clear should fail when --cache-dir points to a regular file; stdout: {}, stderr: {}",
        stdout(&output),
        stderr(&output)
    );

    // Verify the error message is informative.
    let err = stderr(&output);
    assert!(
        err.contains("failed to read cache directory") || err.contains("failed to clear cache") || err.contains("could not be created"),
        "stderr should contain an informative error message; stderr: {err}"
    );

    // Verify the original file still exists and its contents are intact.
    assert!(
        custom_cache.exists(),
        "original file should still exist"
    );
    let contents = std::fs::read_to_string(&custom_cache).unwrap();
    assert_eq!(
        contents, "this is not a cache directory either",
        "file contents should be preserved"
    );
}
