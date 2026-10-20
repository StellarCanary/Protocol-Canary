//! Coverage for the `.stellar-canary.lock` parser and serializer (CF-03,
//! section 3), through the crate's public API only.
//!
//! Every parser rule has a rejecting test, and the serializer is pinned to
//! exact bytes so the file format cannot drift unnoticed.

use std::path::{Path, PathBuf};

use canary_config::{
    load_lock, parse_lock, serialize_lock, LockError, LockFile, LockFixtures, LockSource, LockTool,
    LOCK_FILE_NAME, MAX_LOCK_FILE_BYTES, SUPPORTED_LOCK_VERSION, SUPPORTED_REGISTRY_VERSION,
};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const REVISION: &str = "0000000000000000000000000000000000000000";

const FULL: &str = "lockVersion = 1

[tool]
version = \"0.1.1\"

[fixtures]
protocol = 28
packDigest = \"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"
registryVersion = 1
source = \"release\"
revision = \"0000000000000000000000000000000000000000\"
";

const MINIMAL: &str = "lockVersion = 1

[fixtures]
protocol = 28
packDigest = \"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"
registryVersion = 1
";

fn parse(raw: &str) -> Result<LockFile, LockError> {
    parse_lock(raw, Path::new(LOCK_FILE_NAME))
}

fn rejected(raw: &str) -> String {
    match parse(raw) {
        Ok(lock) => panic!("expected a rejection, got {lock:?}\n{raw}"),
        Err(error) => error.to_string(),
    }
}

/// A scratch directory removed on drop, with no dependency beyond `std`.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "canary-lock-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn file(&self, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(LOCK_FILE_NAME);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn full_lock() -> LockFile {
    LockFile {
        lock_version: 1,
        tool: Some(LockTool {
            version: "0.1.1".to_string(),
        }),
        fixtures: LockFixtures {
            protocol: 28,
            pack_digest: DIGEST.to_string(),
            registry_version: 1,
            source: Some(LockSource::Release),
            revision: Some(REVISION.to_string()),
        },
    }
}

// ----------------------------------------------------------------- accepted

#[test]
fn parses_a_lock_with_every_field() {
    let lock = parse(FULL).expect("valid lock");
    assert_eq!(lock, full_lock());
}

#[test]
fn parses_a_lock_with_only_the_required_fields() {
    let lock = parse(MINIMAL).expect("valid lock");
    assert_eq!(lock.lock_version, SUPPORTED_LOCK_VERSION);
    assert_eq!(lock.tool, None);
    assert_eq!(lock.fixtures.protocol, 28);
    assert_eq!(lock.fixtures.pack_digest, DIGEST);
    assert_eq!(lock.fixtures.registry_version, SUPPORTED_REGISTRY_VERSION);
    assert_eq!(lock.fixtures.source, None);
    assert_eq!(lock.fixtures.revision, None);
}

#[test]
fn accepts_both_source_values() {
    for (spelling, expected) in [
        ("directory", LockSource::Directory),
        ("release", LockSource::Release),
    ] {
        let raw = format!("{MINIMAL}source = \"{spelling}\"\n");
        assert_eq!(parse(&raw).unwrap().fixtures.source, Some(expected));
    }
}

#[test]
fn protocol_zero_is_a_valid_non_negative_integer() {
    let raw = MINIMAL.replace("protocol = 28", "protocol = 0");
    assert_eq!(parse(&raw).unwrap().fixtures.protocol, 0);
}

#[test]
fn accepts_windows_line_endings_and_comments_on_read() {
    // Reading is lenient about layout; only writing is canonical.
    let raw = format!("# written by hand\r\n{}", FULL.replace('\n', "\r\n"));
    assert_eq!(parse(&raw).unwrap(), full_lock());
}

// ------------------------------------------------------------ serialization

#[test]
fn serializes_every_field_in_contract_order() {
    assert_eq!(serialize_lock(&full_lock()).unwrap(), FULL);
}

#[test]
fn serializes_only_the_present_optional_fields() {
    let mut lock = full_lock();
    lock.tool = None;
    lock.fixtures.source = None;
    lock.fixtures.revision = None;
    assert_eq!(serialize_lock(&lock).unwrap(), MINIMAL);
}

#[test]
fn canonical_input_round_trips_to_identical_bytes() {
    for canonical in [FULL, MINIMAL] {
        let lock = parse(canonical).unwrap();
        assert_eq!(serialize_lock(&lock).unwrap(), canonical);
    }
}

#[test]
fn output_is_byte_identical_for_identical_input() {
    let first = serialize_lock(&full_lock()).unwrap();
    let second = serialize_lock(&full_lock()).unwrap();
    assert_eq!(first, second);
}

#[test]
fn output_uses_lf_has_one_trailing_newline_and_no_comments() {
    let text = serialize_lock(&full_lock()).unwrap();
    assert!(!text.contains('\r'));
    assert!(text.ends_with("\"\n"));
    assert!(!text.ends_with("\n\n"));
    assert!(!text.contains('#'));
    assert!(text.lines().all(|line| line == line.trim_end()));
}

#[test]
fn serialization_normalizes_a_hand_written_lock() {
    let messy = "# my lock\r\nlockVersion=1\r\n[fixtures]\r\nrevision = 'abc'\r\nregistryVersion = 1\r\npackDigest = \"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\r\nprotocol = 28\r\n";
    let expected = "lockVersion = 1

[fixtures]
protocol = 28
packDigest = \"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"
registryVersion = 1
revision = \"abc\"
";
    assert_eq!(serialize_lock(&parse(messy).unwrap()).unwrap(), expected);
}

#[test]
fn revision_is_escaped_and_survives_a_round_trip() {
    for revision in [
        "plain",
        "with \"quotes\"",
        "back\\slash",
        "line\nbreak and\ttab",
        "control \u{1} and del \u{7f}",
        "unicode \u{e9}\u{4e2d}\u{1f600}",
        "",
    ] {
        let mut lock = full_lock();
        lock.fixtures.revision = Some(revision.to_string());
        let text = serialize_lock(&lock).unwrap();
        assert_eq!(
            parse(&text).unwrap().fixtures.revision.as_deref(),
            Some(revision),
            "{revision:?} -> {text}"
        );
    }
}

#[test]
fn a_lock_that_would_be_rejected_on_read_is_not_written() {
    let mut bad_digest = full_lock();
    bad_digest.fixtures.pack_digest = "md5:abc".to_string();
    assert!(matches!(
        serialize_lock(&bad_digest),
        Err(LockError::Invalid { .. })
    ));

    let mut bad_registry = full_lock();
    bad_registry.fixtures.registry_version = 2;
    assert!(serialize_lock(&bad_registry).is_err());

    let mut bad_tool = full_lock();
    bad_tool.tool = Some(LockTool {
        version: "v0.1.1".to_string(),
    });
    assert!(serialize_lock(&bad_tool).is_err());

    let mut bad_version = full_lock();
    bad_version.lock_version = 2;
    assert!(serialize_lock(&bad_version).is_err());
}

// ---------------------------------------------------------------- rejected

#[test]
fn rejects_an_unsupported_lock_version_naming_both_versions() {
    let message = rejected("lockVersion = 2\n");
    assert!(message.contains("lockVersion 2"), "{message}");
    assert!(message.contains("supports version 1"), "{message}");

    // The version is judged before anything else, so a future file with
    // keys this build does not know is a version error, not a key error.
    let future = "lockVersion = 3\n[fixtures]\nprotocol = 28\npackDigest = \"x\"\nregistryVersion = 1\nnewKey = true\n";
    assert!(matches!(
        parse(future),
        Err(LockError::UnsupportedVersion {
            found: 3,
            supported: 1,
            ..
        })
    ));
    assert!(matches!(
        parse("lockVersion = 0\n"),
        Err(LockError::UnsupportedVersion { found: 0, .. })
    ));
    assert!(matches!(
        parse("lockVersion = -1\n"),
        Err(LockError::UnsupportedVersion { found: -1, .. })
    ));
}

#[test]
fn rejects_a_lock_version_that_is_not_the_integer_one() {
    for (raw, kind) in [
        ("lockVersion = \"1\"\n", "a string"),
        ("lockVersion = 1.0\n", "a float"),
        ("lockVersion = true\n", "a boolean"),
        ("lockVersion = [1]\n", "an array"),
    ] {
        let message = rejected(raw);
        assert!(message.contains("must be the integer 1"), "{message}");
        assert!(message.contains(kind), "{message}");
    }
}

#[test]
fn rejects_a_missing_lock_version() {
    let message = rejected(&MINIMAL.replacen("lockVersion = 1\n", "", 1));
    assert!(message.contains("lockVersion"), "{message}");
}

#[test]
fn rejects_unknown_keys_and_tables() {
    let unknown_top_level = format!("extra = 1\n{MINIMAL}");
    assert!(rejected(&unknown_top_level).contains("extra"));

    let unknown_table = format!("{MINIMAL}\n[extra]\nkey = 1\n");
    assert!(rejected(&unknown_table).contains("extra"));

    let unknown_in_fixtures = format!("{MINIMAL}unknownKey = 1\n");
    assert!(rejected(&unknown_in_fixtures).contains("unknownKey"));

    let unknown_in_tool = MINIMAL.replace(
        "[fixtures]",
        "[tool]\nversion = \"0.1.1\"\nchannel = \"beta\"\n\n[fixtures]",
    );
    assert!(rejected(&unknown_in_tool).contains("channel"));

    let nested_table = format!("{MINIMAL}\n[fixtures.nested]\nkey = 1\n");
    assert!(parse(&nested_table).is_err());
}

#[test]
fn rejects_duplicate_keys() {
    let duplicated = MINIMAL.replace("protocol = 28\n", "protocol = 28\nprotocol = 29\n");
    assert!(parse(&duplicated).is_err());
    let duplicated_top = format!("lockVersion = 1\n{MINIMAL}");
    assert!(parse(&duplicated_top).is_err());
}

#[test]
fn rejects_missing_required_fields() {
    let no_fixtures = "lockVersion = 1\n";
    assert!(rejected(no_fixtures).contains("fixtures"));

    for key in ["protocol", "packDigest", "registryVersion"] {
        let without: String = MINIMAL
            .lines()
            .filter(|line| !line.starts_with(key))
            .collect::<Vec<_>>()
            .join("\n");
        let message = rejected(&without);
        assert!(message.contains(key), "{key}: {message}");
    }
}

#[test]
fn rejects_a_malformed_pack_digest() {
    let good = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    for bad in [
        "md5:abc".to_string(),
        format!("sha256:{}", good.to_uppercase()),
        format!("sha256:{}", &good[..63]),
        format!("sha256:{good}0"),
        good.to_string(),
        format!("sha256: {good}"),
        format!("sha256:{good} "),
        format!("sha256:{}g", &good[..63]),
        "sha256:".to_string(),
        String::new(),
    ] {
        let raw = MINIMAL.replace(DIGEST, &bad);
        let message = rejected(&raw);
        assert!(message.contains("packDigest"), "{bad:?}: {message}");
    }
}

#[test]
fn rejects_an_invalid_protocol() {
    for value in ["-1", "\"28\"", "28.5", "4294967296", "true"] {
        let raw = MINIMAL.replace("protocol = 28", &format!("protocol = {value}"));
        assert!(parse(&raw).is_err(), "protocol = {value}");
    }
}

#[test]
fn rejects_an_unsupported_registry_version() {
    for value in ["0", "2", "99"] {
        let raw = MINIMAL.replace("registryVersion = 1", &format!("registryVersion = {value}"));
        let message = rejected(&raw);
        assert!(message.contains("registryVersion"), "{message}");
        assert!(message.contains("supports version 1"), "{message}");
    }
    let wrong_type = MINIMAL.replace("registryVersion = 1", "registryVersion = \"1\"");
    assert!(parse(&wrong_type).is_err());
}

#[test]
fn rejects_an_unknown_source() {
    let raw = format!("{MINIMAL}source = \"tarball\"\n");
    let message = rejected(&raw);
    assert!(message.contains("tarball"), "{message}");
    let wrong_case = format!("{MINIMAL}source = \"Release\"\n");
    assert!(parse(&wrong_case).is_err());
}

#[test]
fn rejects_a_revision_that_is_not_a_string() {
    let raw = format!("{MINIMAL}revision = 42\n");
    assert!(parse(&raw).is_err());
}

#[test]
fn rejects_a_tool_version_that_is_not_major_minor_patch() {
    for bad in [
        "v0.1.1",
        "0.1",
        "0.1.1.1",
        "0.1.x",
        "0.01.1",
        "00.1.1",
        "0.1.1-rc.1",
        "0.1.1+build",
        "1..1",
        "",
        " 0.1.1",
        "-1.0.0",
    ] {
        let raw = MINIMAL.replace(
            "[fixtures]",
            &format!("[tool]\nversion = \"{bad}\"\n\n[fixtures]"),
        );
        let message = rejected(&raw);
        assert!(message.contains("tool.version"), "{bad:?}: {message}");
    }
}

#[test]
fn accepts_well_formed_tool_versions() {
    for good in ["0.0.0", "0.1.1", "1.20.300", "10.0.0"] {
        let raw = MINIMAL.replace(
            "[fixtures]",
            &format!("[tool]\nversion = \"{good}\"\n\n[fixtures]"),
        );
        assert!(parse(&raw).is_ok(), "{good}");
    }
}

#[test]
fn rejects_text_that_is_not_toml() {
    for raw in ["", "not toml at all", "lockVersion = ", "[fixtures"] {
        assert!(parse(raw).is_err(), "{raw:?}");
    }
}

// --------------------------------------------------------------- files

#[test]
fn loads_a_regular_file() {
    let dir = Scratch::new("load");
    let path = dir.file(FULL.as_bytes());
    assert_eq!(load_lock(&path).unwrap(), full_lock());
}

#[test]
fn a_missing_file_is_a_read_error() {
    let dir = Scratch::new("missing");
    let error = load_lock(&dir.0.join(LOCK_FILE_NAME)).unwrap_err();
    assert!(matches!(error, LockError::Read { .. }), "{error}");
}

#[test]
fn a_file_of_exactly_the_limit_is_accepted_and_one_byte_more_is_not() {
    let dir = Scratch::new("size");
    let limit = usize::try_from(MAX_LOCK_FILE_BYTES).unwrap();

    // Pad the informational `revision` so the file is exactly `limit` bytes.
    let base = MINIMAL.len() + "revision = \"\"\n".len();
    let at_limit = format!("{MINIMAL}revision = \"{}\"\n", "a".repeat(limit - base));
    assert_eq!(at_limit.len(), limit);
    let path = dir.file(at_limit.as_bytes());
    assert!(load_lock(&path).is_ok());

    let over = format!("{MINIMAL}revision = \"{}\"\n", "a".repeat(limit - base + 1));
    assert_eq!(over.len(), limit + 1);
    let path = dir.file(over.as_bytes());
    match load_lock(&path).unwrap_err() {
        LockError::TooLarge { size, limit, .. } => {
            assert_eq!(size, MAX_LOCK_FILE_BYTES + 1);
            assert_eq!(limit, MAX_LOCK_FILE_BYTES);
        }
        other => panic!("expected TooLarge, got {other}"),
    }
}

#[test]
fn the_size_limit_also_applies_to_text_parsed_in_memory() {
    let big = "#".repeat(usize::try_from(MAX_LOCK_FILE_BYTES).unwrap() + 1);
    assert!(matches!(parse(&big), Err(LockError::TooLarge { .. })));
}

#[test]
fn rejects_a_file_that_is_not_utf8() {
    let dir = Scratch::new("utf8");
    let mut bytes = MINIMAL.as_bytes().to_vec();
    bytes.extend_from_slice(b"revision = \"\xff\xfe\"\n");
    let path = dir.file(&bytes);
    assert!(matches!(
        load_lock(&path).unwrap_err(),
        LockError::NotUtf8 { .. }
    ));
}

#[test]
fn rejects_a_directory() {
    let dir = Scratch::new("dir");
    let path = dir.0.join(LOCK_FILE_NAME);
    std::fs::create_dir(&path).unwrap();
    assert!(matches!(
        load_lock(&path).unwrap_err(),
        LockError::NotRegularFile { .. }
    ));
}

#[cfg(unix)]
#[test]
fn rejects_a_symbolic_link_even_to_a_valid_lock() {
    let dir = Scratch::new("symlink");
    let target = dir.0.join("real.lock");
    std::fs::write(&target, FULL).unwrap();
    let link = dir.0.join(LOCK_FILE_NAME);
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(matches!(
        load_lock(&link).unwrap_err(),
        LockError::Symlink { .. }
    ));
}

#[cfg(unix)]
#[test]
fn rejects_a_dangling_symbolic_link_as_a_link_not_as_missing() {
    let dir = Scratch::new("dangling");
    let link = dir.0.join(LOCK_FILE_NAME);
    std::os::unix::fs::symlink(dir.0.join("does-not-exist"), &link).unwrap();
    assert!(matches!(
        load_lock(&link).unwrap_err(),
        LockError::Symlink { .. }
    ));
}

// ---------------------------------------------------- diagnostics and exit

#[test]
fn every_lock_error_is_a_configuration_error_with_the_path_in_it() {
    let dir = Scratch::new("diagnostics");
    let path = dir.file(b"lockVersion = 2\n");
    let error = load_lock(&path).unwrap_err();
    let message = error.to_string();
    assert!(message.contains(LOCK_FILE_NAME), "{message}");

    let canary_error: canary_core::CanaryError = error.into();
    assert!(matches!(
        canary_error,
        canary_core::CanaryError::Configuration(_)
    ));
    assert_eq!(
        canary_core::ExitCode::from(&canary_error),
        canary_core::ExitCode::ConfigurationError
    );
}
