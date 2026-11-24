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

// ---------------------------------------------------------------------------
// Staging-file cleanup on write failure (issue #743)
// ---------------------------------------------------------------------------
//
// `atomic_write_report` stages the bytes in `<file>.tmp`, flushes them, and
// renames the staging file over the destination. The cases below make a step
// of that sequence fail on purpose — std only, since the crate has no
// `tempfile` dev-dependency — and check the two guarantees a failure must
// keep: the staging file is gone afterwards, and the destination still holds
// exactly what it held before.

/// The names in `dir`, sorted, so a test can assert that a failed write left
/// the directory exactly as it found it.
fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|entry| {
            entry
                .expect("a readable directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// A destination the final rename cannot replace on any platform: a
/// directory that is not empty. Everything before the rename — creating the
/// staging file, writing it, flushing it — succeeds, so the failure happens
/// with a fully written `.tmp` file on disk, which is the cleanup this issue
/// is about.
fn occupied_directory(dir: &Path, name: &str) -> PathBuf {
    let target = dir.join(name);
    fs::create_dir(&target).unwrap();
    fs::write(target.join("previous-report.json"), "{\"keep\": true}\n").unwrap();
    target
}

#[test]
fn a_failed_rename_removes_the_fully_written_staging_file() {
    let dir = temp_dir("rename-fails");
    let target = occupied_directory(&dir, "report.json");
    let before = listing(&dir);

    let err = atomic_write_report(&target, "{\"schemaVersion\": 1}\n")
        .expect_err("a non-empty directory cannot be replaced by a file");

    let message = err.to_string();
    assert!(
        message.contains("failed to write report to") && message.contains("report.json"),
        "the error names the destination, got: {message}"
    );
    assert_no_orphaned_temp(&dir);
    assert_eq!(
        listing(&dir),
        before,
        "a failed write leaves the directory as it was"
    );
    // The destination is untouched: still a directory, still holding its
    // previous contents byte for byte.
    assert!(target.is_dir(), "the destination was not replaced");
    assert_eq!(
        fs::read_to_string(target.join("previous-report.json")).unwrap(),
        "{\"keep\": true}\n",
        "the destination's previous contents survive the failure"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_stale_staging_file_from_an_earlier_crash_is_removed_when_the_write_fails() {
    let dir = temp_dir("stale-then-fail");
    let target = occupied_directory(&dir, "report.json");
    // What a crash between write and rename in an earlier run leaves behind.
    fs::write(dir.join("report.json.tmp"), "{\"truncated\": ").unwrap();

    atomic_write_report(&target, "new report\n").expect_err("the rename still fails");

    assert_no_orphaned_temp(&dir);
    assert_eq!(
        listing(&dir),
        vec!["report.json".to_string()],
        "neither the stale nor the new staging file survives"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_stale_staging_file_is_overwritten_not_appended_to_on_success() {
    let dir = temp_dir("stale-then-succeed");
    let target = dir.join("report.json");
    fs::write(&target, "previous report\n").unwrap();
    // Longer than the new content: appending to it, or writing over only its
    // prefix, would leave stale bytes in the renamed report.
    fs::write(
        dir.join("report.json.tmp"),
        "stale staging bytes from a crashed run, much longer than the new report",
    )
    .unwrap();

    atomic_write_report(&target, "new\n").expect("a stale staging file does not block a write");

    assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
    assert_no_orphaned_temp(&dir);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_staging_path_that_cannot_be_opened_fails_before_the_destination_is_touched() {
    let dir = temp_dir("staging-blocked");
    let target = dir.join("report.json");
    fs::write(&target, "original report\n").unwrap();
    // Occupy the staging name with a directory, so creating the `.tmp` file
    // — the very first I/O step — fails.
    let blocker = dir.join("report.json.tmp");
    fs::create_dir(&blocker).unwrap();
    fs::write(blocker.join("unrelated.txt"), "not the writer's").unwrap();

    let err = atomic_write_report(&target, "replacement\n")
        .expect_err("the staging file cannot be created");

    assert!(
        err.to_string().contains("failed to write report to"),
        "actionable error, got: {err}"
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "original report\n",
        "the destination keeps its previous bytes when staging fails"
    );
    // Cleanup is limited to a staging *file*: a directory the writer did not
    // create is never removed, and nothing inside it is touched.
    assert!(blocker.is_dir(), "the cleanup must not delete a directory");
    assert_eq!(
        fs::read_to_string(blocker.join("unrelated.txt")).unwrap(),
        "not the writer's"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn repeated_failures_never_accumulate_staging_files() {
    let dir = temp_dir("repeated-failures");
    let target = occupied_directory(&dir, "report.json");
    let before = listing(&dir);

    for attempt in 0..3 {
        atomic_write_report(&target, &format!("attempt {attempt}\n"))
            .expect_err("every attempt fails at the rename");
        assert_eq!(
            listing(&dir),
            before,
            "attempt {attempt} left something behind"
        );
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failure_for_one_destination_leaves_a_neighbouring_report_alone() {
    // The staging name is derived from the destination's own file name, so a
    // failed write cleans up only its own `.tmp` sibling — never another
    // report's staging file or another report.
    let dir = temp_dir("neighbour");
    let neighbour = dir.join("other.json");
    fs::write(&neighbour, "neighbour report\n").unwrap();
    fs::write(dir.join("other.json.tmp"), "neighbour staging\n").unwrap();
    let target = occupied_directory(&dir, "report.json");

    atomic_write_report(&target, "x").expect_err("the rename fails");

    assert_no_orphaned_temp(&dir);
    assert_eq!(
        fs::read_to_string(&neighbour).unwrap(),
        "neighbour report\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("other.json.tmp")).unwrap(),
        "neighbour staging\n",
        "another destination's staging file is not this write's to remove"
    );
    fs::remove_dir_all(&dir).unwrap();
}
