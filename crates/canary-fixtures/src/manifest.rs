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

    Ok(LoadedFixture {
        metadata: FixtureMetadata {
            id: raw.id,
            protocol: ProtocolVersion(raw.protocol),
            surface: raw.surface,
            category: raw.category,
            description: raw.description,
            source_reference: raw.source_reference,
            required_capabilities: raw.required_capabilities,
        },
        source_path: path.to_path_buf(),
        input_file: resolve_reference(path, dir, "input_file", raw.input_file)?,
        expected_file: resolve_reference(path, dir, "expected_file", raw.expected_file)?,
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
}
