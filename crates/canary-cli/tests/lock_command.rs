//! End-to-end tests for `stellar-canary lock`
//! (`cargo test -p canary-cli --test lock_command`), covering issue
//! #445: a `.stellar-canary.lock` at the project root whose shape
//! conforms to CF-03 (lockVersion 1, `[fixtures]` with protocol and
//! packDigest), written deterministically, and never written partially
//! when the fixture pack cannot be resolved.

mod support;

use support::{run_in, stderr, stdout, TempProject};

const LOCK_FILE: &str = ".stellar-canary.lock";

const VALID_FIXTURE: &str = r#"
id = "p28-xdr-lock-001"
protocol = 28
surface = "xdr"
category = "cap-83"
description = "lock command test fixture"

[input]
kind = "roundtrip"
value_base64 = "AAAAAA=="
"#;

fn lockfile_path(project: &TempProject) -> std::path::PathBuf {
    project.path.join(LOCK_FILE)
}

fn read_lockfile(project: &TempProject) -> String {
    std::fs::read_to_string(lockfile_path(project)).expect("lockfile must exist")
}

#[test]
fn lock_writes_a_cf03_conforming_lockfile_at_the_project_root() {
    let project = TempProject::new("lock-basic");
    project.write("fixtures/p28-xdr-lock-001.toml", VALID_FIXTURE);

    let output = run_in(&project.path, &["lock"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let contents = read_lockfile(&project);
    assert!(
        contents.contains("lockVersion = 1"),
        "lockfile must set lockVersion = 1, got:\n{contents}"
    );
    assert!(
        contents.contains("[tool]")
            && contents.contains(&format!("version = \"{}\"", env!("CARGO_PKG_VERSION"))),
        "lockfile must pin the tool version in [tool].version, got:\n{contents}"
    );
    assert!(
        contents.contains("[fixtures]"),
        "lockfile must contain a [fixtures] table, got:\n{contents}"
    );
    assert!(
        contents.contains("protocol = 28"),
        "lockfile must pin the protocol, got:\n{contents}"
    );
    // registryVersion is a required CF-03 key naming the pack registry
    // format this build supports.
    assert!(
        contents.contains("registryVersion = 1"),
        "lockfile must pin the registry version, got:\n{contents}"
    );

    // packDigest is a canary-pack-v1 digest: "sha256:" followed by 64
    // hex characters.
    let digest_line = contents
        .lines()
        .find(|line| line.starts_with("packDigest"))
        .expect("lockfile must contain a packDigest");
    let digest = digest_line
        .split('"')
        .nth(1)
        .expect("packDigest must be a quoted string");
    let hex = digest
        .strip_prefix("sha256:")
        .expect("packDigest must carry the sha256: algorithm prefix");
    assert_eq!(hex.len(), 64, "packDigest must be a full SHA-256: {digest}");
    assert!(
        hex.chars().all(|c| c.is_ascii_hexdigit()),
        "packDigest must be hex: {digest}"
    );

    // The success message names the file it wrote.
    assert!(
        stdout(&output).contains("packDigest"),
        "stdout must summarize the lock, got: {}",
        stdout(&output)
    );
}

#[test]
fn the_protocol_flag_pins_the_requested_protocol() {
    let project = TempProject::new("lock-protocol-flag");
    project.write("fixtures/p28-xdr-lock-001.toml", VALID_FIXTURE);

    let output = run_in(&project.path, &["lock", "--protocol", "29"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(read_lockfile(&project).contains("protocol = 29"));
}

#[test]
fn the_configured_protocol_is_pinned_without_a_flag() {
    let project = TempProject::new("lock-config-protocol");
    project.write("fixtures/p28-xdr-lock-001.toml", VALID_FIXTURE);
    project.write(
        ".stellar-canary.toml",
        r#"
        version = 1
        protocol = 27

        [tests]
        xdr = true
        "#,
    );

    let output = run_in(&project.path, &["lock"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(read_lockfile(&project).contains("protocol = 27"));
}

#[test]
fn locking_twice_produces_byte_identical_output() {
    let project = TempProject::new("lock-deterministic");
    project.write("fixtures/p28-xdr-lock-001.toml", VALID_FIXTURE);

    assert!(run_in(&project.path, &["lock"]).status.success());
    let first = read_lockfile(&project);
    assert!(run_in(&project.path, &["lock"]).status.success());
    let second = read_lockfile(&project);
    assert_eq!(
        first, second,
        "the deterministic serializer must produce identical bytes on re-run"
    );
}

#[test]
fn identical_pack_content_at_different_locations_pins_the_same_digest() {
    // Content addressing (CF-02): the digest is independent of the pack's
    // root location — the same relative layout checked out at two paths
    // must pin the same digest. The relative layout itself *is* part of
    // the pack identity, so two checkouts use the same layout here.
    let project_a = TempProject::new("lock-pack-a");
    let project_b = TempProject::new("lock-pack-b");
    project_a.write("fixtures/p28-xdr-lock-001.toml", VALID_FIXTURE);
    project_b.write("fixtures/p28-xdr-lock-001.toml", VALID_FIXTURE);

    assert!(run_in(&project_a.path, &["lock"]).status.success());
    assert!(run_in(&project_b.path, &["lock"]).status.success());

    let digest_of = |project: &TempProject| {
        read_lockfile(project)
            .lines()
            .find(|line| line.starts_with("packDigest"))
            .expect("packDigest line")
            .to_string()
    };
    assert_eq!(
        digest_of(&project_a),
        digest_of(&project_b),
        "the same pack at different root paths must produce an identical packDigest"
    );
}

#[test]
fn changing_fixture_content_changes_the_digest() {
    let project = TempProject::new("lock-pack-change");
    project.write("fixtures/p28-xdr-lock-001.toml", VALID_FIXTURE);
    assert!(run_in(&project.path, &["lock"]).status.success());
    let before = read_lockfile(&project);

    let edited = VALID_FIXTURE.replace("roundtrip", "decode-failure");
    project.write("fixtures/p28-xdr-lock-001.toml", &edited);
    assert!(run_in(&project.path, &["lock"]).status.success());
    let after = read_lockfile(&project);

    assert_ne!(
        before, after,
        "an edited fixture pack must be reflected in the packDigest"
    );
}

#[test]
fn an_absent_fixtures_directory_fails_without_writing_a_lockfile() {
    let project = TempProject::new("lock-absent-dir");

    let output = run_in(&project.path, &["lock"]);
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(
        message.contains("does not exist") && message.contains("fixtures"),
        "error must clearly name the problem, got: {message}"
    );
    assert!(
        !lockfile_path(&project).exists(),
        "a failed lock must not leave a partial lockfile behind"
    );
}

#[test]
fn an_invalid_fixture_pack_fails_without_writing_a_lockfile() {
    let project = TempProject::new("lock-invalid-pack");
    // Two fixtures with the same id: `validate` rejects the pack before
    // any digest or lockfile is produced.
    project.write("fixtures/a.toml", VALID_FIXTURE);
    // Same fixture id under a different file name.
    project.write("fixtures/b.toml", VALID_FIXTURE);

    let output = run_in(&project.path, &["lock"]);
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(
        message.contains("duplicate fixture id"),
        "error must name the duplicate id, got: {message}"
    );
    assert!(
        !lockfile_path(&project).exists(),
        "an invalid pack must not leave a partial lockfile behind"
    );
}

#[test]
fn a_zero_protocol_is_rejected_before_anything_is_written() {
    let project = TempProject::new("lock-zero-protocol");
    project.write("fixtures/p28-xdr-lock-001.toml", VALID_FIXTURE);

    let output = run_in(&project.path, &["lock", "--protocol", "0"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("--protocol must be a positive protocol version number"),
        "got: {}",
        stderr(&output)
    );
    assert!(!lockfile_path(&project).exists());
}
