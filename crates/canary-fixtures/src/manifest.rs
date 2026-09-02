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
    let raw: RawFixtureFile = toml::from_str(&raw_text).map_err(|source| FixtureError::Parse {
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
        input_file: raw.input_file.map(|f| dir.join(f)),
        expected_file: raw.expected_file.map(|f| dir.join(f)),
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
