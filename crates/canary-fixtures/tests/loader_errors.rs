//! Every `FixtureError` variant the loader and validator can produce
//! (`[E-113]`, issue #507).
//!
//! Each test builds a fixture directory that trips exactly one variant and
//! asserts the enum itself, not just the message, so a variant that stopped
//! being reachable would fail here.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use canary_fixtures::{
    load_directory, parse_fixture_file, validate, FixtureError, MAX_FIXTURE_BYTES,
};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A directory under the system temp dir, removed when the test ends.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "canary-loader-errors-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir is creatable");
        TempDir { path }
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent is creatable");
        }
        std::fs::write(&path, contents).expect("fixture is writable");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A minimal valid fixture header plus `body`.
fn fixture(id: &str, body: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test fixture\"\n{body}"
    )
}

#[test]
fn read_reports_a_file_that_cannot_be_read() {
    let missing = std::env::temp_dir().join("canary-loader-errors-does-not-exist.toml");
    let err = parse_fixture_file(&missing).unwrap_err();
    assert!(matches!(err, FixtureError::Read { .. }), "{err}");
    assert!(
        err.to_string().contains("does-not-exist.toml"),
        "the message names the file: {err}"
    );
}

#[test]
fn parse_reports_the_file_and_line_of_invalid_toml() {
    let dir = TempDir::new("parse");
    let path = dir.write("broken.toml", "id = \"p28-broken\"\nprotocol = [[[\n");

    let err = load_directory(&dir.path).unwrap_err();
    match &err {
        FixtureError::Parse { path: source, .. } => assert_eq!(source, &path),
        other => panic!("expected Parse, got {other}"),
    }
    let message = err.to_string();
    assert!(message.contains("broken.toml"), "{message}");
    assert!(message.contains("line 2"), "{message}");
}

#[test]
fn an_empty_fixture_file_is_a_parse_error_naming_the_missing_metadata() {
    let dir = TempDir::new("empty");
    let path = dir.write("empty.toml", "");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0, "a 0-byte file");

    let err = load_directory(&dir.path).unwrap_err();
    match &err {
        FixtureError::Parse { path: source, .. } => assert_eq!(source, &path),
        other => panic!("expected Parse, got {other}"),
    }
    let message = err.to_string();
    assert!(message.contains("empty.toml"), "names the file: {message}");
    assert!(
        message.contains("missing field"),
        "says required metadata is missing: {message}"
    );
}

#[test]
fn a_fixture_of_binary_nul_bytes_is_a_parse_error_naming_the_file() {
    let dir = TempDir::new("binary");
    let path = dir.path.join("binary.toml");
    std::fs::write(&path, b"\0\0\0\0id = \"p28-binary\"\0\0\0").unwrap();

    let err = load_directory(&dir.path).unwrap_err();
    match &err {
        FixtureError::Parse { path: source, .. } => assert_eq!(source, &path),
        other => panic!("expected Parse, got {other}"),
    }
    let message = err.to_string();
    assert!(message.contains("binary.toml"), "names the file: {message}");
    assert!(
        message.len() > "failed to parse fixture file binary.toml: ".len(),
        "carries the parser's description: {message}"
    );
}

#[test]
fn read_dir_reports_a_directory_that_cannot_be_read() {
    let missing = std::env::temp_dir().join("canary-loader-errors-no-such-dir");
    let err = load_directory(&missing).unwrap_err();
    assert!(matches!(err, FixtureError::ReadDir { .. }), "{err}");
}

#[test]
fn duplicate_id_names_both_files() {
    let dir = TempDir::new("duplicate");
    dir.write("a.toml", &fixture("p28-duplicate", ""));
    dir.write("b.toml", &fixture("p28-duplicate", ""));

    let fixtures =
        load_directory(&dir.path).expect("both files load; the clash is found on validation");
    let err = validate(&fixtures).unwrap_err();
    match &err {
        FixtureError::DuplicateId { id, first, second } => {
            assert_eq!(id, "p28-duplicate");
            assert!(first.ends_with("a.toml"), "{}", first.display());
            assert!(second.ends_with("b.toml"), "{}", second.display());
        }
        other => panic!("expected DuplicateId, got {other}"),
    }
}

#[test]
fn file_too_large_rejects_a_fixture_over_one_mib() {
    let dir = TempDir::new("too-large");
    let path = dir.path.join("huge.toml");
    let oversized = vec![b'x'; usize::try_from(MAX_FIXTURE_BYTES).unwrap() + 1];
    std::fs::write(&path, &oversized).unwrap();

    let err = load_directory(&dir.path).unwrap_err();
    match &err {
        FixtureError::FileTooLarge {
            path: source,
            size,
            limit,
        } => {
            assert_eq!(source, &path);
            assert_eq!(*size, MAX_FIXTURE_BYTES + 1);
            assert_eq!(*limit, MAX_FIXTURE_BYTES);
        }
        other => panic!("expected FileTooLarge, got {other}"),
    }
    assert!(err.to_string().contains("huge.toml"), "{err}");
}

#[test]
fn a_fixture_at_the_limit_is_accepted() {
    let dir = TempDir::new("at-limit");
    let header = fixture("p28-at-limit", "");
    let padding = MAX_FIXTURE_BYTES as usize - header.len();
    let contents = format!("{header}{}", "#".repeat(padding));
    assert_eq!(contents.len() as u64, MAX_FIXTURE_BYTES);
    dir.write("at-limit.toml", &contents);

    let fixtures = load_directory(&dir.path).expect("a fixture at the limit loads");
    assert_eq!(fixtures.len(), 1);
}

#[test]
fn invalid_verification_is_reported_strictly() {
    let dir = TempDir::new("verification");
    dir.write(
        "bad-verification.toml",
        &fixture("p28-bad-verification", "[verification]\nbogus = 1\n"),
    );

    let err = load_directory(&dir.path).unwrap_err();
    assert!(
        matches!(err, FixtureError::InvalidVerification { .. }),
        "{err}"
    );
    assert!(
        err.to_string().contains("bad-verification.toml"),
        "the message names the file: {err}"
    );
}

#[test]
fn unknown_tables_are_kept_for_the_surface_to_reject() {
    // The loader carries the body as raw TOML, so an unknown table reaches
    // the surface unchanged and is reported there instead of being dropped
    // while parsing.
    let loaded = canary_fixtures::parse_fixture_str(
        &fixture("p28-unknown-table", "[unknown_table]\nkey = 1\n"),
        Path::new("unknown-table.toml"),
    )
    .expect("metadata parses; the body is still raw");

    let message = canary_fixtures::unknown_body_keys(
        loaded.body.as_table().expect("body is a table"),
        &["method", "request", "assert"],
    )
    .expect("an unknown table is reported");
    assert!(message.contains("unknown_table"), "{message}");
}

#[test]
fn unsafe_reference_names_the_field_and_reason() {
    let dir = TempDir::new("unsafe-reference");
    dir.write(
        "unsafe.toml",
        &fixture("p28-unsafe", "input_file = \"../outside.bin\"\n"),
    );

    let err = load_directory(&dir.path).unwrap_err();
    match &err {
        FixtureError::UnsafeReference { field, reason, .. } => {
            assert_eq!(*field, "input_file");
            assert!(
                reason.contains("escapes"),
                "the reason explains the escape: {reason}"
            );
        }
        other => panic!("expected UnsafeReference, got {other}"),
    }
}

#[test]
fn missing_referenced_file_names_the_payload() {
    let dir = TempDir::new("missing-reference");
    dir.write(
        "missing.toml",
        &fixture("p28-missing-ref", "input_file = \"payload.bin\"\n"),
    );

    let fixtures = load_directory(&dir.path).expect("the file loads; only its payload is missing");
    let err = validate(&fixtures).unwrap_err();
    match &err {
        FixtureError::MissingReferencedFile {
            id,
            kind,
            referenced,
            ..
        } => {
            assert_eq!(id, "p28-missing-ref");
            assert_eq!(*kind, "input");
            assert!(
                referenced.ends_with("payload.bin"),
                "{}",
                referenced.display()
            );
        }
        other => panic!("expected MissingReferencedFile, got {other}"),
    }
}

#[cfg(unix)]
#[test]
fn symbolic_link_is_refused() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new("symlink");
    let outside = TempDir::new("symlink-target");
    let real = outside.write("real.toml", &fixture("p28-linked", ""));
    symlink(&real, dir.path.join("link.toml")).unwrap();

    let err = load_directory(&dir.path).unwrap_err();
    assert!(matches!(err, FixtureError::SymbolicLink { .. }), "{err}");
}
