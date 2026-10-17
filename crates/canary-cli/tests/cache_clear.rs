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
