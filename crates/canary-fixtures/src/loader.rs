//! Loading a directory of fixture files.

use std::path::{Path, PathBuf};

use canary_core::CanaryError;

use crate::manifest::{parse_fixture_file, LoadedFixture};

#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("failed to read fixture file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse fixture file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("failed to read fixture directory {path}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("duplicate fixture id {id:?}: defined in both {first} and {second}")]
    DuplicateId {
        id: String,
        first: PathBuf,
        second: PathBuf,
    },

    #[error(
        "fixture {id:?} ({source_path}) references a {kind} file that does not exist: {referenced}"
    )]
    MissingReferencedFile {
        id: String,
        source_path: PathBuf,
        kind: &'static str,
        referenced: PathBuf,
    },
}

impl From<FixtureError> for CanaryError {
    fn from(error: FixtureError) -> Self {
        CanaryError::Fixture(error.to_string())
    }
}

/// Recursively loads every `*.toml` fixture file under `dir`.
///
/// Returns fixtures in a deterministic (sorted-by-path) order so that
/// downstream planning and reporting stay reproducible.
pub fn load_directory(dir: &Path) -> Result<Vec<LoadedFixture>, FixtureError> {
    let mut paths = Vec::new();
    collect_toml_files(dir, &mut paths)?;
    paths.sort();

    paths.iter().map(|path| parse_fixture_file(path)).collect()
}

fn collect_toml_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), FixtureError> {
    let entries = std::fs::read_dir(dir).map_err(|source| FixtureError::ReadDir {
        path: dir.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| FixtureError::ReadDir {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_toml_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "toml") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn fixture_toml(id: &str, surface: &str) -> String {
        format!(
            r#"
            id = "{id}"
            protocol = 28
            surface = "{surface}"
            category = "test"
            description = "test fixture"
            "#
        )
    }

    #[test]
    fn loads_fixtures_recursively_in_deterministic_order() {
        let dir = crate::test_support::temp_dir("loader-recursive");
        write(
            &dir.path.join("xdr/p28-xdr-001.toml"),
            &fixture_toml("p28-xdr-001", "xdr"),
        );
        write(
            &dir.path.join("rpc/p28-rpc-001.toml"),
            &fixture_toml("p28-rpc-001", "rpc"),
        );

        let fixtures = load_directory(&dir.path).expect("loads");
        assert_eq!(fixtures.len(), 2);
        let ids: Vec<_> = fixtures.iter().map(|f| f.metadata.id.clone()).collect();
        assert_eq!(ids, vec!["p28-rpc-001", "p28-xdr-001"]);
    }

    #[test]
    fn ignores_non_toml_files() {
        let dir = crate::test_support::temp_dir("loader-ignore");
        write(&dir.path.join("README.md"), "not a fixture");
        write(
            &dir.path.join("p28-xdr-001.toml"),
            &fixture_toml("p28-xdr-001", "xdr"),
        );

        let fixtures = load_directory(&dir.path).expect("loads");
        assert_eq!(fixtures.len(), 1);
    }

    #[test]
    fn propagates_parse_errors_with_the_offending_path() {
        let dir = crate::test_support::temp_dir("loader-bad-parse");
        write(&dir.path.join("broken.toml"), "not valid [[[ toml");

        let err = load_directory(&dir.path).unwrap_err();
        assert!(matches!(err, FixtureError::Parse { .. }));
    }
}
