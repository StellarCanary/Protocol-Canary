//! `stellar-canary lock verify` (CF-03-D): the command reports whether the
//! lockfile still pins the fixture pack it was written against, without
//! writing anything.
//!
//! The tests build a real fixture pack on disk, write a real lockfile for
//! its digest, and then drive the actual binary, mutating the pack or the
//! lock the way real damage happens: an edited fixture byte, a new file, a
//! missing lockfile, a lock that fails the CF-03 section 3 parser rules.

mod support;

use canary_config::{
    LockFile, LockFixtures, LockSource, SUPPORTED_LOCK_VERSION, SUPPORTED_REGISTRY_VERSION,
};
use canary_fixtures::pack_digest;
use support::{run_in, stderr, stdout, TempProject};

/// Writes a minimal valid fixture pack under `root/fixtures` and returns the
/// pack digest of the freshly written bytes.
fn make_pack(dir: &TempProject) -> String {
    dir.write(
        "fixtures/xdr/a.toml",
        "id = \"p28-xdr-a\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"cap-0083\"\n\
         description = \"golden xdr\"\nsource_reference = \"CAP-0083\"\n\
         required_capabilities = [\"wasm-artifact\"]\n\
         input_file = \"a.bin\"\n\
         type = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAAAA==\"\n",
    );
    dir.write("fixtures/xdr/a.bin", "fixture-payload-bytes\n");
    pack_digest(&dir.path.join("fixtures"))
        .expect("the test pack must be digestible")
        .digest
}

/// Writes a `.stellar-canary.lock` pinning `digest`.
fn write_lock(dir: &TempProject, digest: &str) {
    let lock = LockFile {
        lock_version: SUPPORTED_LOCK_VERSION,
        tool: None,
        fixtures: LockFixtures {
            protocol: 28,
            pack_digest: digest.to_string(),
            registry_version: SUPPORTED_REGISTRY_VERSION,
            source: Some(LockSource::Directory),
            revision: None,
        },
    };
    canary_config::write_lock(&dir.path.join(".stellar-canary.lock"), &lock)
        .expect("the test lock must write");
}

#[test]
fn a_lock_that_matches_the_pack_verifies_with_exit_code_zero() {
    let dir = TempProject::new("lock-verify-pass");
    let digest = make_pack(&dir);
    write_lock(&dir, &digest);

    let output = run_in(&dir.path, &["lock", "verify"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("Verified"), "{text}");
    assert!(text.contains(&digest), "{text}");
}

/// The one thing this command must never do is write: no cache directory,
/// no lockfile rewrite, no temporary file behind.
#[test]
fn verification_is_read_only() {
    let dir = TempProject::new("lock-verify-readonly");
    let digest = make_pack(&dir);
    write_lock(&dir, &digest);
    let lock_before = std::fs::read(dir.path.join(".stellar-canary.lock")).unwrap();

    let output = run_in(&dir.path, &["lock", "verify"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let lock_after = std::fs::read(dir.path.join(".stellar-canary.lock")).unwrap();
    assert_eq!(lock_before, lock_after, "the lockfile must not change");
    assert!(
        !dir.path.join(".stellar-canary.lock.tmp").exists(),
        "no temporary file may survive"
    );
    assert!(!dir.path.join(".stellar-canary-cache").exists());
}

/// A one-byte fixture edit changes the digest: verification fails with exit
/// code 1 and the message names both digests.
#[test]
fn an_edited_fixture_byte_fails_verification_with_both_digests() {
    let dir = TempProject::new("lock-verify-edit");
    let digest = make_pack(&dir);
    write_lock(&dir, &digest);
    dir.write("fixtures/xdr/a.bin", "fixture-payload-bytes!\n");
    let computed = pack_digest(&dir.path.join("fixtures")).unwrap().digest;

    let output = run_in(&dir.path, &["lock", "verify"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let text = stderr(&output);
    assert!(text.contains(&digest), "pinned digest: {text}");
    assert!(text.contains(&computed), "computed digest: {text}");
    assert_ne!(digest, computed);
}

/// Adding a file to the pack — even an unreferenced one of the hashed kinds
/// (a `.toml`) — changes the digest too.
#[test]
fn an_added_file_fails_verification() {
    let dir = TempProject::new("lock-verify-add");
    let digest = make_pack(&dir);
    write_lock(&dir, &digest);
    dir.write("fixtures/xdr/b.toml", "id = \"p28-xdr-b\"\n");

    let output = run_in(&dir.path, &["lock", "verify"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("hashes to"), "{}", stdout(&output));
}

/// A missing lockfile is not "verified by absence": it is the unverified
/// outcome with exit code 1 and a message saying nothing is pinned.
#[test]
fn a_missing_lockfile_fails_verification() {
    let dir = TempProject::new("lock-verify-missing");
    make_pack(&dir);

    let output = run_in(&dir.path, &["lock", "verify"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("no lockfile"), "{text}");
    assert!(text.contains(".stellar-canary.lock"), "{text}");
}

/// A lockfile that fails the CF-03 section 3 parser rules (an unsupported
/// `lockVersion`, a `packDigest` that does not match the sha256 shape) is
/// unverified, never accepted.
#[test]
fn locks_from_the_invalid_examples_of_the_contract_fails() {
    // lockVersion 2: the rebel lock.
    let dir = TempProject::new("lock-verify-version");
    make_pack(&dir);
    dir.write(".stellar-canary.lock", "lockVersion = 2\n");
    let output = run_in(&dir.path, &["lock", "verify"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("lockVersion"),
        "{}",
        stdout(&output)
    );

    // packDigest = "md5:abc": the CF-03 section 8 example.
    let dir = TempProject::new("lock-verify-md5");
    make_pack(&dir);
    dir.write(
        ".stellar-canary.lock",
        "lockVersion = 1\n\n[fixtures]\nprotocol = 28\npackDigest = \"md5:abc\"\nregistryVersion = 1\n",
    );
    let output = run_in(&dir.path, &["lock", "verify"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("packDigest"), "{text}");
}

/// `--fixtures-dir` chooses a location but does not waive the pin: a
/// directory whose pack the lock does not pin still fails (CF-03 section 4).
#[test]
fn overriding_the_fixtures_dir_still_verifies_against_the_pin() {
    let dir = TempProject::new("lock-verify-override");
    let digest = make_pack(&dir);
    write_lock(&dir, &digest);
    // A second pack with different bytes.
    dir.write("ci/fixtures/xdr/a.toml", "id = \"p28-xdr-other\"\n");

    let output = run_in(
        &dir.path,
        &["lock", "verify", "--fixtures-dir", "ci/fixtures"],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));

    // And the digest that ci/fixtures actually hashes would verify a lock
    // written for *it* — proving the override reached the right directory.
    let ci_digest = pack_digest(&dir.path.join("ci/fixtures")).unwrap().digest;
    assert_ne!(ci_digest, digest);
    write_lock(&dir, &ci_digest);
    let output = run_in(
        &dir.path,
        &["lock", "verify", "--fixtures-dir", "ci/fixtures"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains(&ci_digest));
}

/// Neither a crash nor an unhandled backtrace is an acceptable way to reject
/// a bad `--fixtures-dir`: the CLI must diagnose the path and exit normally.
fn assert_no_panic(output: &std::process::Output) {
    let text = stderr(output);
    assert!(
        !text.contains("panicked at"),
        "the CLI must not panic: {text}"
    );
    assert!(
        !text.contains("stack backtrace"),
        "the CLI must not print an unhandled backtrace: {text}"
    );
    assert!(
        output.status.code().is_some(),
        "the process must exit normally"
    );
}

/// A `--fixtures-dir` that does not exist cannot be digested, so the pin does
/// not hold: verification fails with exit code 1, naming the missing path on
/// stderr rather than panicking or claiming success.
#[test]
fn a_missing_fixtures_dir_fails_verification_without_panicking() {
    let dir = TempProject::new("lock-verify-missing-dir");
    let digest = make_pack(&dir);
    write_lock(&dir, &digest);

    let output = run_in(
        &dir.path,
        &["lock", "verify", "--fixtures-dir", "no-such-fixtures"],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(!stdout(&output).contains("Verified"), "{}", stdout(&output));

    let text = stderr(&output);
    assert!(text.contains("no-such-fixtures"), "{text}");
    assert!(text.contains("could not be verified"), "{text}");
    assert!(text.contains("failed to read"), "{text}");
    assert_no_panic(&output);
}

/// A regular file passed as `--fixtures-dir` is not a pack: it cannot be
/// walked or digested, so verification fails with exit code 1 and a clear
/// diagnostic instead of a panic or a misleading "verified" outcome.
#[test]
fn a_regular_file_as_fixtures_dir_fails_verification_without_panicking() {
    let dir = TempProject::new("lock-verify-file-dir");
    let digest = make_pack(&dir);
    write_lock(&dir, &digest);
    dir.write("not-a-pack", "this is a file, not a fixture pack\n");

    let output = run_in(
        &dir.path,
        &["lock", "verify", "--fixtures-dir", "not-a-pack"],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(!stdout(&output).contains("Verified"), "{}", stdout(&output));

    let text = stderr(&output);
    assert!(text.contains("not-a-pack"), "{text}");
    assert!(text.contains("could not be verified"), "{text}");
    assert!(text.contains("failed to read"), "{text}");
    assert_no_panic(&output);
}

/// A `--project-root` that does not exist is a configuration error (exit
/// code 2): the run could not start, which is different from failing.
#[test]
fn a_missing_project_root_is_a_configuration_error() {
    let dir = TempProject::new("lock-verify-root");
    let missing = dir.path.join("no-such-root");
    let output = run_in(
        &dir.path,
        &["lock", "verify", "--project-root", "no-such-root"],
    );
    assert_eq!(output.status.code(), Some(2));
    let _ = missing;
}
