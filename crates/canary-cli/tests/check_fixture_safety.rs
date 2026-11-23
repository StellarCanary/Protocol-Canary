mod support;

use support::{run_in, stderr, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn xdr_fixture(id: &str, extra: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n{extra}"
    )
}

#[test]
fn a_payload_reference_that_escapes_the_fixture_directory_is_an_invalid_fixture() {
    let dir = TempProject::new("fixture-safety-escape");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("secret.bin", "outside the fixtures directory");
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "input_file = \"../secret.bin\"\n"),
    );

    for command in ["check", "inspect", "fixtures"] {
        let output = run_in(&dir.path, &[command]);
        assert_eq!(output.status.code(), Some(4), "{command}");
        let text = stderr(&output);
        assert!(text.contains("input_file"), "{command}: {text}");
        assert!(text.contains(".."), "{command}: {text}");
    }
}

#[test]
fn an_absolute_payload_reference_is_an_invalid_fixture() {
    let dir = TempProject::new("fixture-safety-absolute");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "expected_file = \"/etc/hostname\"\n"),
    );

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("absolute"));
}

#[cfg(unix)]
#[test]
fn a_symlink_inside_the_fixture_directory_is_an_invalid_fixture() {
    let dir = TempProject::new("fixture-safety-symlink");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("elsewhere/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", ""));
    std::fs::create_dir_all(dir.path.join("fixtures")).unwrap();
    std::os::unix::fs::symlink(dir.path.join("elsewhere"), dir.path.join("fixtures/linked"))
        .unwrap();

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("symbolic link"));
}

/// A fixture path the engine cannot use as a directory is a usage error
/// (exit 2) reported before any fixture loads or runs: the run stops with a
/// description naming the path and the fix, and the process does not panic.
fn assert_a_hostile_fixture_path_is_rejected(path: &str) -> String {
    let dir = TempProject::new("fixture-safety-hostile-path");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", ""));

    let output = run_in(&dir.path, &["check", "--offline", "--fixtures-dir", path]);
    let err = stderr(&output);

    assert_eq!(
        output.status.code(),
        Some(2),
        "a hostile fixture path is a usage error, not a crash\nstderr:\n{err}"
    );
    assert!(!err.contains("panicked"), "the engine panicked: {err}");
    assert!(err.contains("no checks ran"), "{err}");
    assert!(err.contains("--fixtures-dir"), "the fix is named: {err}");
    assert!(
        err.contains(path),
        "the rejected path is echoed back unchanged: {err}"
    );
    err
}

/// `%2f` is not a separator. The engine must not percent-decode a fixture
/// path, so this stays one ordinary directory name instead of silently
/// becoming `fixtures/sub`, and nothing behind it is read. The control run
/// proves the decoded target is a working fixture directory, so the failure
/// below is caused by the encoded spelling and not by an absent directory.
#[test]
fn a_fixture_path_with_percent_encoded_slashes_is_rejected_as_a_usage_error() {
    let dir = TempProject::new("fixture-safety-encoded-slash");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/sub/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", ""));

    let control = run_in(
        &dir.path,
        &["check", "--offline", "--fixtures-dir", "fixtures/sub"],
    );
    assert_eq!(
        control.status.code(),
        Some(0),
        "the decoded target is a usable fixture directory: {}",
        stderr(&control)
    );

    let err = assert_a_hostile_fixture_path_is_rejected("fixtures%2fsub");
    assert!(
        !err.contains("fixtures/sub"),
        "the path was percent-decoded into a real directory: {err}"
    );
}

/// A path longer than the system `PATH_MAX` (4096 bytes on Linux, 1024 on
/// some BSDs) cannot name a directory on any target. It must be refused
/// cleanly rather than panicking inside the loader or the filesystem walk,
/// and it must be echoed in full rather than silently truncated. The long
/// component also exceeds the 255-byte per-name limit, so this is unusable
/// on every supported platform, not only on long-path filesystems. The
/// control run shows the same project's fixtures load normally, so length is
/// the only difference.
#[test]
fn a_fixture_path_longer_than_path_max_is_rejected_as_a_usage_error() {
    let dir = TempProject::new("fixture-safety-long-path");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", ""));

    let control = run_in(
        &dir.path,
        &["check", "--offline", "--fixtures-dir", "fixtures"],
    );
    assert_eq!(control.status.code(), Some(0), "{}", stderr(&control));

    let path = format!("fixtures/{}", "a".repeat(5_000));
    // The helper asserts the diagnostic echoes this whole string, so a
    // truncated path fails the test rather than passing as a shorter one.
    assert_a_hostile_fixture_path_is_rejected(&path);
}

/// A fixture path that already exists but is a regular file is just as
/// unusable as an absent directory: it must be refused before the loader
/// walks it, not surface later as an opaque read error.
#[test]
fn a_fixture_path_that_is_a_file_is_rejected_as_a_usage_error() {
    let dir = TempProject::new("fixture-safety-file-path");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", ""));

    assert_a_hostile_fixture_path_is_rejected("fixtures/p28-xdr-1.toml");
}
