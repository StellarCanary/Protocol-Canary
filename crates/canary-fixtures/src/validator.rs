//! Validating a loaded set of fixtures.
//!
//! A broken fixture is reported as [`FixtureError`], never silently turned
//! into a project incompatibility: bad fixture data is a problem with the
//! fixture pack, not evidence about the project under test.

use std::collections::HashMap;

use canary_core::FixtureStore;

use crate::loader::FixtureError;
use crate::manifest::LoadedFixture;

/// Validates `fixtures` and, if valid, builds a [`FixtureStore`] from their
/// metadata.
pub fn validate(fixtures: &[LoadedFixture]) -> Result<FixtureStore, FixtureError> {
    check_unique_ids(fixtures)?;
    check_referenced_files_exist(fixtures)?;

    Ok(FixtureStore::new(
        fixtures.iter().map(|f| f.metadata.clone()).collect(),
    ))
}

fn check_unique_ids(fixtures: &[LoadedFixture]) -> Result<(), FixtureError> {
    let mut seen = HashMap::new();
    for fixture in fixtures {
        if let Some(first) = seen.insert(fixture.metadata.id.clone(), &fixture.source_path) {
            return Err(FixtureError::DuplicateId {
                id: fixture.metadata.id.clone(),
                first: first.clone(),
                second: fixture.source_path.clone(),
            });
        }
    }
    Ok(())
}

fn check_referenced_files_exist(fixtures: &[LoadedFixture]) -> Result<(), FixtureError> {
    for fixture in fixtures {
        if let Some(input_file) = &fixture.input_file {
            if !input_file.is_file() {
                return Err(FixtureError::MissingReferencedFile {
                    id: fixture.metadata.id.clone(),
                    source_path: fixture.source_path.clone(),
                    kind: "input",
                    referenced: input_file.clone(),
                });
            }
        }
        if let Some(expected_file) = &fixture.expected_file {
            if !expected_file.is_file() {
                return Err(FixtureError::MissingReferencedFile {
                    id: fixture.metadata.id.clone(),
                    source_path: fixture.source_path.clone(),
                    kind: "expected",
                    referenced: expected_file.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use canary_core::{ProtocolVersion, Surface};
    use std::path::PathBuf;

    fn fixture(id: &str, source_path: &str) -> LoadedFixture {
        LoadedFixture {
            metadata: canary_core::FixtureMetadata {
                id: id.to_string(),
                protocol: ProtocolVersion(28),
                surface: Surface::Xdr,
                category: "test".into(),
                description: "test".into(),
                source_reference: None,
                required_capabilities: vec![],
            },
            source_path: PathBuf::from(source_path),
            input_file: None,
            expected_file: None,
            body: toml::Value::Table(Default::default()),
        }
    }

    #[test]
    fn accepts_unique_fixtures_with_no_missing_files() {
        let fixtures = vec![fixture("a", "a.toml"), fixture("b", "b.toml")];
        let store = validate(&fixtures).expect("valid");
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let fixtures = vec![fixture("dup", "a.toml"), fixture("dup", "b.toml")];
        let err = validate(&fixtures).unwrap_err();
        assert!(matches!(err, FixtureError::DuplicateId { .. }));
    }

    #[test]
    fn rejects_missing_referenced_input_file() {
        let dir = crate::test_support::temp_dir("validator-missing-file");
        let mut f = fixture("a", "a.toml");
        f.input_file = Some(dir.path.join("does-not-exist.bin"));
        let err = validate(&[f]).unwrap_err();
        assert!(matches!(err, FixtureError::MissingReferencedFile { .. }));
    }

    #[test]
    fn accepts_an_existing_referenced_input_file() {
        let dir = crate::test_support::temp_dir("validator-existing-file");
        let file_path = dir.path.join("input.bin");
        std::fs::write(&file_path, b"data").unwrap();
        let mut f = fixture("a", "a.toml");
        f.input_file = Some(file_path);
        assert!(validate(&[f]).is_ok());
    }
}
