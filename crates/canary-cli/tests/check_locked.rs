//! `check --locked` (issue #446): with the flag, a missing or stale
//! `.stellar-canary.lock` fails the run with exit code 2 before any
//! fixture executes, and a fresh lockfile lets the run proceed. The flag
//! is opt-in: without it, the lockfile is not consulted at all.

mod support;

use canary_config::{
    LockFile, LockFixtures, LockSource, SUPPORTED_LOCK_VERSION, SUPPORTED_REGISTRY_VERSION,
};
use canary_fixtures::pack_digest;
use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = true
rpc = false
soroban = false
"#;

/// A digest-shaped string that is well-formed enough for the CF-03
/// parser but cannot be the digest of any pack in these tests.
const STALE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn xdr_fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
    )
}

/// Writes a config and a one-fixture pack under `root`, returning the
/// pack's digest.
fn make_project(dir: &TempProject) -> String {
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("fixtures/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1"));
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

/// The acceptance criterion of the issue: a missing lockfile under
/// `--locked` exits 2 with a diagnostic that names the command that
/// fixes it, and nothing runs.
#[test]
fn a_missing_lockfile_exits_two_and_instructs_the_user_to_lock() {
    let dir = TempProject::new("check-locked-missing");
    make_project(&dir);

    let output = run_in(&dir.path, &["check", "--locked", "--offline"]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));

    let err = stderr(&output);
    assert!(err.contains("no lockfile"), "{err}");
    assert!(
        err.contains("stellar-canary lock"),
        "the diagnostic must instruct the user to run 'stellar-canary lock': {err}"
    );
    // The refusal happens before any fixture executes.
    assert!(
        !stdout(&output).contains("PASS"),
        "no fixture may run under a missing pin"
    );
}

/// The second acceptance criterion: a digest that no longer matches the
/// pack exits 2 and names both digests.
#[test]
fn a_stale_lockfile_exits_two_and_names_both_digests() {
    let dir = TempProject::new("check-locked-stale");
    let computed = make_project(&dir);
    write_lock(&dir, STALE_DIGEST);
    assert_ne!(STALE_DIGEST, computed);

    let output = run_in(&dir.path, &["check", "--locked", "--offline"]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));

    let err = stderr(&output);
    assert!(err.contains(STALE_DIGEST), "pinned digest: {err}");
    assert!(err.contains(&computed), "computed digest: {err}");
    assert!(err.contains("stellar-canary lock"), "{err}");
    assert!(!stdout(&output).contains("PASS"));
}

/// The third acceptance criterion: a fresh, matching lockfile does not
/// fail the gate, and the run itself proceeds normally.
#[test]
fn a_fresh_lockfile_lets_the_run_proceed() {
    let dir = TempProject::new("check-locked-fresh");
    let digest = make_project(&dir);
    write_lock(&dir, &digest);

    let output = run_in(&dir.path, &["check", "--locked", "--offline"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("1/1 PASS"), "{text}");
}

/// The gate sees pack changes made after the lock was written, not only a
/// mismatched digest constant: an added fixture file changes the pack.
#[test]
fn an_added_fixture_file_makes_a_previously_fresh_lock_stale() {
    let dir = TempProject::new("check-locked-added");
    let digest = make_project(&dir);
    write_lock(&dir, &digest);
    dir.write("fixtures/p28-xdr-2.toml", &xdr_fixture("p28-xdr-2"));

    let output = run_in(&dir.path, &["check", "--locked", "--offline"]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
    assert!(stderr(&output).contains("hashes to"), "{}", stderr(&output));
}

/// An edited fixture byte — the smallest possible real drift — also
/// fails the gate.
#[test]
fn an_edited_fixture_byte_makes_the_lock_stale() {
    let dir = TempProject::new("check-locked-edit");
    let _ = make_project(&dir);
    let digest = pack_digest(&dir.path.join("fixtures")).unwrap().digest;
    write_lock(&dir, &digest);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1").replace("test\"", "edited\""),
    );

    let output = run_in(&dir.path, &["check", "--locked", "--offline"]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
}

/// A lockfile that fails the CF-03 section 3 parser is not a pin the
/// gate can trust: it exits 2 with the parser failure and the repair
/// command.
#[test]
fn an_unparseable_lockfile_exits_two() {
    let dir = TempProject::new("check-locked-corrupt");
    make_project(&dir);
    dir.write(".stellar-canary.lock", "lockVersion = 99\n");

    let output = run_in(&dir.path, &["check", "--locked", "--offline"]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
    let err = stderr(&output);
    assert!(err.contains("stellar-canary lock"), "{err}");
}

/// Without the flag the lockfile is not consulted: the same stale pin
/// that fails `--locked` leaves a plain run untouched. This pins that
/// the gate is opt-in.
#[test]
fn without_the_flag_a_stale_lockfile_does_not_fail_the_run() {
    let dir = TempProject::new("check-locked-opt-in");
    make_project(&dir);
    write_lock(&dir, STALE_DIGEST);

    let output = run_in(&dir.path, &["check", "--offline"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("1/1 PASS"));
}

/// The gate runs before the fixtures directory is even needed: with a
/// fresh pin pointing at a pack that has since vanished, the run exits 2
/// rather than silently checking nothing.
#[test]
fn a_vanished_pack_under_a_lock_exits_two() {
    let dir = TempProject::new("check-locked-vanished");
    let digest = make_project(&dir);
    write_lock(&dir, &digest);
    std::fs::remove_dir_all(dir.path.join("fixtures")).unwrap();

    let output = run_in(&dir.path, &["check", "--locked", "--offline"]);
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
    let err = stderr(&output);
    assert!(err.contains("could not digest"), "{err}");
    assert!(err.contains("stellar-canary lock"), "{err}");
}
