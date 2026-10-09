//! Parsing a single fixture file's common metadata.
//!
//! A fixture file is TOML with a fixed set of metadata keys (mirroring
//! [`canary_core::FixtureMetadata`]) plus an arbitrary surface-specific
//! remainder (e.g. `[input]` / `[expect]` tables) that this crate does not
//! interpret — that is the job of the surface crate that consumes the
//! fixture (`canary-xdr`, `canary-rpc`, `canary-soroban`).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use canary_core::{Capability, FixtureMetadata, ProtocolVersion, Surface};

use crate::loader::FixtureError;

#[derive(Debug, Deserialize)]
struct RawFixtureFile {
    id: String,
    protocol: u32,
    surface: Surface,
    category: String,
    description: String,
    #[serde(default)]
    source_reference: Option<String>,
    #[serde(default)]
    required_capabilities: Vec<Capability>,
    #[serde(default)]
    input_file: Option<String>,
    #[serde(default)]
    expected_file: Option<String>,
    #[serde(flatten)]
    body: toml::Value,
}

/// A fixture file, parsed and resolved relative to its own directory.
#[derive(Debug, Clone)]
pub struct LoadedFixture {
    pub metadata: FixtureMetadata,
    /// The fixture file's own path, for error messages.
    pub source_path: PathBuf,
    /// Resolved, not-yet-checked path to an externally referenced input
    /// file, if `input_file` was set.
    pub input_file: Option<PathBuf>,
    /// Resolved, not-yet-checked path to an externally referenced expected
    /// output file, if `expected_file` was set.
    pub expected_file: Option<PathBuf>,
    /// Everything in the fixture file other than the common metadata keys,
    /// for the surface-specific runner to interpret.
    pub body: toml::Value,
}

/// Parses one fixture file at `path`.
pub fn parse_fixture_file(path: &Path) -> Result<LoadedFixture, FixtureError> {
    let raw_text = std::fs::read_to_string(path).map_err(|source| FixtureError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_fixture_str(&raw_text, path)
}

/// Checks that every key of a fixture body table is one the surface reads.
///
/// Surface bodies are carried as raw TOML, so without this a misspelled key
/// (`[[asert]]` for `[[assert]]`, `expected_base64x`) is dropped silently and
/// the fixture can pass while checking nothing. Returns a message naming the
/// unknown keys and the accepted ones.
pub fn unknown_body_keys(table: &toml::value::Table, allowed: &[&str]) -> Option<String> {
    let mut unknown: Vec<&str> = table
        .keys()
        .map(String::as_str)
        .filter(|key| !allowed.contains(key))
        .collect();
    if unknown.is_empty() {
        return None;
    }
    unknown.sort_unstable();
    let mut accepted = allowed.to_vec();
    accepted.sort_unstable();
    Some(format!(
        "unknown key(s) {}; this surface accepts {}",
        unknown
            .iter()
            .map(|k| format!("{k:?}"))
            .collect::<Vec<_>>()
            .join(", "),
        accepted
            .iter()
            .map(|k| format!("{k:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Digest of everything that defines a fixture's behavior: its file and the
/// payload files it references.
///
/// Raw bytes are hashed with no newline or encoding normalization, so any
/// edit changes the digest. A referenced payload that is missing, unreadable
/// or a symbolic link is hashed as "unavailable", which differs from an empty
/// file; validation reports the underlying problem separately. Paths are not
/// part of the digest, so it is the same in any checkout location.
pub fn content_digest(
    fixture_bytes: &[u8],
    input_file: Option<&Path>,
    expected_file: Option<&Path>,
) -> String {
    let input = payload_part(input_file);
    let expected = payload_part(expected_file);
    canary_core::sha256_hex(&[b"canary-fixture/1", fixture_bytes, &input, &expected])
}

/// Frames one optional payload: `0` absent, `1` unavailable, `2` + bytes.
fn payload_part(path: Option<&Path>) -> Vec<u8> {
    let Some(path) = path else {
        return vec![0];
    };
    let is_plain_file = std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_file())
        .unwrap_or(false);
    match is_plain_file.then(|| std::fs::read(path).ok()).flatten() {
        Some(bytes) => {
            let mut part = vec![2];
            part.extend(bytes);
            part
        }
        None => vec![1],
    }
}

/// Checks that `reference` is a plain relative path and joins it to `dir`.
///
/// The rules are lexical, so they behave the same on every platform and do
/// not depend on what exists on disk: a reference may not be empty, absolute,
/// rooted, drive-qualified, contain a backslash or NUL, or have an empty,
/// `.` or `..` segment. The accepted separator is `/`.
fn resolve_reference(
    source_path: &Path,
    dir: &Path,
    field: &'static str,
    reference: Option<String>,
) -> Result<Option<PathBuf>, FixtureError> {
    let Some(reference) = reference else {
        return Ok(None);
    };
    if let Some(reason) = unsafe_reference_reason(&reference) {
        return Err(FixtureError::UnsafeReference {
            source_path: source_path.to_path_buf(),
            field,
            reference,
            reason,
        });
    }
    Ok(Some(
        reference
            .split('/')
            .fold(dir.to_path_buf(), |p, seg| p.join(seg)),
    ))
}

fn unsafe_reference_reason(reference: &str) -> Option<&'static str> {
    if reference.is_empty() {
        return Some("the path is empty");
    }
    if reference.contains('\0') {
        return Some("the path contains a NUL character");
    }
    if reference.contains('\\') {
        return Some("the path contains a backslash; use '/' as the separator");
    }
    if reference.starts_with('/') {
        return Some("the path is absolute");
    }
    let mut segments = reference.split('/');
    let first = segments.clone().next().unwrap_or("");
    let bytes = first.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Some("the path has a drive prefix");
    }
    for segment in segments.by_ref() {
        match segment {
            "" => return Some("the path has an empty segment"),
            "." => return Some("the path has a '.' segment"),
            ".." => return Some("the path escapes the fixture directory with '..'"),
            _ => {}
        }
    }
    None
}

/// Parses fixture TOML already in memory, as if it had come from `path`
/// (used to resolve `input_file`/`expected_file` and for error messages).
///
/// This is split out from [`parse_fixture_file`] so surface-runner crates
/// can build a [`LoadedFixture`] in tests without touching the filesystem.
pub fn parse_fixture_str(raw_text: &str, path: &Path) -> Result<LoadedFixture, FixtureError> {
    let raw: RawFixtureFile = toml::from_str(raw_text).map_err(|source| FixtureError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));

    let input_file = resolve_reference(path, dir, "input_file", raw.input_file)?;
    let expected_file = resolve_reference(path, dir, "expected_file", raw.expected_file)?;
    let content_digest = content_digest(
        raw_text.as_bytes(),
        input_file.as_deref(),
        expected_file.as_deref(),
    );

    Ok(LoadedFixture {
        metadata: FixtureMetadata {
            id: raw.id,
            protocol: ProtocolVersion(raw.protocol),
            surface: raw.surface,
            category: raw.category,
            description: raw.description,
            source_reference: raw.source_reference,
            required_capabilities: raw.required_capabilities,
            content_digest,
        },
        source_path: path.to_path_buf(),
        input_file,
        expected_file,
        body: raw.body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn parses_common_metadata_and_keeps_the_remainder_as_body() {
        let dir = crate::test_support::temp_dir("manifest-parse");
        let path = write(
            &dir.path,
            "fixture.toml",
            r#"
            id = "p28-xdr-cap83-001"
            protocol = 28
            surface = "xdr"
            category = "cap-83"
            description = "StellarValue roundtrip"
            source_reference = "CAP-0083"

            [input]
            kind = "roundtrip"
            value_base64 = "AAAAAA=="
            "#,
        );

        let loaded = parse_fixture_file(&path).expect("parses");
        assert_eq!(loaded.metadata.id, "p28-xdr-cap83-001");
        assert_eq!(loaded.metadata.protocol, ProtocolVersion(28));
        assert_eq!(loaded.metadata.surface, Surface::Xdr);
        assert_eq!(
            loaded.metadata.source_reference.as_deref(),
            Some("CAP-0083")
        );
        assert!(loaded.body.get("input").is_some());
    }

    #[test]
    fn resolves_referenced_files_relative_to_the_fixture_directory() {
        let dir = crate::test_support::temp_dir("manifest-refs");
        let path = write(
            &dir.path,
            "fixture.toml",
            r#"
            id = "p28-xdr-cap83-002"
            protocol = 28
            surface = "xdr"
            category = "cap-83"
            description = "large payload"
            input_file = "input.xdr.b64"
            expected_file = "expected.xdr.b64"
            "#,
        );
        let loaded = parse_fixture_file(&path).expect("parses");
        assert_eq!(loaded.input_file, Some(dir.path.join("input.xdr.b64")));
        assert_eq!(
            loaded.expected_file,
            Some(dir.path.join("expected.xdr.b64"))
        );
    }

    #[test]
    fn rejects_malformed_toml() {
        let dir = crate::test_support::temp_dir("manifest-malformed");
        let path = write(&dir.path, "fixture.toml", "not valid [[[ toml");
        let err = parse_fixture_file(&path).unwrap_err();
        assert!(matches!(err, FixtureError::Parse { .. }));
    }

    #[test]
    fn rejects_unsupported_surface_name() {
        let dir = crate::test_support::temp_dir("manifest-bad-surface");
        let path = write(
            &dir.path,
            "fixture.toml",
            r#"
            id = "bad"
            protocol = 28
            surface = "not-a-surface"
            category = "x"
            description = "x"
            "#,
        );
        let err = parse_fixture_file(&path).unwrap_err();
        assert!(matches!(err, FixtureError::Parse { .. }));
    }

    const DIGEST_FIXTURE: &str = "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ninput_file = \"in.bin\"\n";

    fn digest_of(dir: &Path, text: &str) -> String {
        parse_fixture_str(text, &dir.join("fixture.toml"))
            .unwrap()
            .metadata
            .content_digest
    }

    #[test]
    fn digest_changes_when_the_fixture_text_changes() {
        let dir = crate::test_support::temp_dir("digest-text");
        std::fs::write(dir.path.join("in.bin"), "payload").unwrap();
        let a = digest_of(&dir.path, DIGEST_FIXTURE);
        let b = digest_of(
            &dir.path,
            &DIGEST_FIXTURE.replace(
                "\"test\"\ndescription = \"test\"",
                "\"test\"\ndescription = \"edited\"",
            ),
        );
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert_eq!(
            a,
            digest_of(&dir.path, DIGEST_FIXTURE),
            "stable for equal input"
        );
    }

    #[test]
    fn digest_changes_when_a_referenced_payload_changes() {
        let dir = crate::test_support::temp_dir("digest-payload");
        std::fs::write(dir.path.join("in.bin"), "payload one").unwrap();
        let before = digest_of(&dir.path, DIGEST_FIXTURE);
        std::fs::write(dir.path.join("in.bin"), "payload two").unwrap();
        assert_ne!(before, digest_of(&dir.path, DIGEST_FIXTURE));
    }

    #[test]
    fn digest_does_not_depend_on_where_the_fixture_lives() {
        let one = crate::test_support::temp_dir("digest-path-one");
        let two = crate::test_support::temp_dir("digest-path-two");
        for dir in [&one, &two] {
            std::fs::write(dir.path.join("in.bin"), "same bytes").unwrap();
        }
        assert_eq!(
            digest_of(&one.path, DIGEST_FIXTURE),
            digest_of(&two.path, DIGEST_FIXTURE)
        );
    }

    #[test]
    fn a_missing_payload_does_not_digest_like_an_empty_one() {
        let dir = crate::test_support::temp_dir("digest-missing");
        let missing = digest_of(&dir.path, DIGEST_FIXTURE);
        std::fs::write(dir.path.join("in.bin"), "").unwrap();
        assert_ne!(missing, digest_of(&dir.path, DIGEST_FIXTURE));
    }

    #[test]
    fn a_payload_in_the_wrong_slot_changes_the_digest() {
        // The same bytes as an input and as an expected file are different
        // fixtures.
        let dir = crate::test_support::temp_dir("digest-slot");
        std::fs::write(dir.path.join("in.bin"), "x").unwrap();
        let as_input = digest_of(&dir.path, DIGEST_FIXTURE);
        let as_expected = digest_of(
            &dir.path,
            &DIGEST_FIXTURE.replace("input_file", "expected_file"),
        );
        assert_ne!(as_input, as_expected);
    }
}
