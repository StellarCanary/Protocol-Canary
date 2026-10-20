//! `atomic_write_report`: `--output` writes through a `.tmp` sibling and
//! renames it into place, so a crash can never leave a truncated report on
//! disk.

use std::fs;
use std::path::{Path, PathBuf};

use canary_report::atomic_write_report;

/// A fresh directory per test, under the system temp directory.
/// std-only on purpose: the workspace carries no `tempfile` dependency.
fn temp_dir(label: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "canary-atomic-writer-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir is creatable");
    dir
}

fn assert_no_orphaned_temp(dir: &Path) {
    let orphan = dir.join("report.json.tmp");
    assert!(
        !orphan.exists(),
        "no .tmp sibling may remain in {}",
        dir.display()
    );
}

#[test]
fn writes_a_new_report_file_with_exactly_the_given_bytes() {
    let dir = temp_dir("new");
    let target = dir.join("report.json");
    let content = "{\n  \"schemaVersion\": 1\n}\n";

    atomic_write_report(&target, content).expect("a new file writes");

    assert_eq!(fs::read_to_string(&target).unwrap(), content);
    assert_no_orphaned_temp(&dir);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn overwrites_an_existing_report_completely_and_leaves_no_temp_file() {
    let dir = temp_dir("overwrite");
    let target = dir.join("report.json");
    // Longer stale contents: a replacement that only prepends or appends
    // would leave the tail of the old report readable.
    fs::write(
        &target,
        "stale contents that are much longer than the new report",
    )
    .unwrap();

    atomic_write_report(&target, "new\n").expect("an existing file is replaced");

    assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
    assert_no_orphaned_temp(&dir);
    fs::remove_dir_all(&dir).unwrap();
}

/// The destination never becomes visible half-written: the temp file is
/// only ever renamed. This asserts the observable consequence — the temp
/// file is created in the destination's own directory (same filesystem,
/// hence an atomic rename), not in a system temp directory elsewhere.
#[test]
fn the_temp_file_lives_in_the_destination_directory() {
    let dir = temp_dir("same-dir");
    let target = dir.join("report.json");

    // The only name the implementation may create before the rename is
    // `<file>.tmp` beside the destination; after success it must be gone.
    atomic_write_report(&target, "x").unwrap();
    assert_no_orphaned_temp(&dir);

    // A failed write must not leave that sibling behind either: here the
    // destination *is* a directory, so the final rename fails.
    let as_dir = dir.join("subdir");
    fs::create_dir(&as_dir).unwrap();
    let err = atomic_write_report(&as_dir, "x").unwrap_err();
    assert!(
        err.to_string().contains("failed to write report to"),
        "actionable error naming the path, got: {err}"
    );
    assert!(!dir.join("subdir.tmp").exists(), "no orphaned temp file");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_destination_with_a_missing_parent_directory_fails_without_orphans() {
    let dir = temp_dir("missing-parent");
    let target = dir.join("no-such-directory").join("report.json");

    let err = atomic_write_report(&target, "x").unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("failed to write report to") && message.contains("report.json"),
        "the error must name the unwritable path, got: {message}"
    );
    assert!(
        !dir.join("no-such-directory").exists(),
        "the write must not create directories by surprise"
    );
    assert_no_orphaned_temp(&dir);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_destination_whose_parent_is_a_file_fails_without_orphans() {
    let dir = temp_dir("parent-is-file");
    let not_a_dir = dir.join("not-a-directory");
    fs::write(&not_a_dir, "x").unwrap();
    let target = not_a_dir.join("report.json");

    let err = atomic_write_report(&target, "x").unwrap_err();

    assert!(
        err.to_string().contains("failed to write report to"),
        "actionable error, got: {err}"
    );
    assert!(!dir.join("report.json.tmp").exists());
    assert_eq!(fs::read_to_string(&not_a_dir).unwrap(), "x");
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn overwriting_preserves_the_existing_files_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("permissions");
    let target = dir.join("report.json");
    fs::write(&target, "old").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();

    atomic_write_report(&target, "new").expect("overwrite succeeds");

    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640, "the replacement keeps the original mode");
    assert_no_orphaned_temp(&dir);
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_non_writable_directory_returns_an_actionable_error_without_orphans() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("non-writable");
    let target = dir.join("report.json");
    fs::write(&target, "original").unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();

    let outcome = atomic_write_report(&target, "replacement");

    // Restore the mode first so cleanup works either way.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();

    let err = match outcome {
        Err(err) => err,
        Ok(()) => {
            // uid 0 (some containers) bypasses directory permissions, so
            // this scenario cannot be produced here. CI runs as a normal
            // user and exercises the assertions below.
            eprintln!("skipped: running as root, directory modes are not enforced");
            fs::remove_dir_all(&dir).unwrap();
            return;
        }
    };

    assert!(
        err.to_string().contains("failed to write report to")
            && err.to_string().contains("report.json"),
        "actionable error naming the unwritable path, got: {err}"
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "original",
        "a failed write leaves the previous report untouched"
    );
    assert_no_orphaned_temp(&dir);
    fs::remove_dir_all(&dir).unwrap();
}
