//! Lightweight `Cargo.toml` inspection.
//!
//! This intentionally does not use `cargo_metadata` or invoke `cargo`: for
//! detection purposes we only need the declared dependency names, and
//! reading `Cargo.toml` directly keeps detection fast and independent of
//! whether the target project's dependencies are even fetched yet.

use std::path::Path;

/// The dependency names declared by a `Cargo.toml`, gathered from
/// `[dependencies]`, `[dev-dependencies]`, and `[build-dependencies]`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CargoManifest {
    pub dependency_names: Vec<String>,
}

impl CargoManifest {
    pub fn has_dependency(&self, name: &str) -> bool {
        self.dependency_names.iter().any(|d| d == name)
    }

    pub fn has_any_dependency(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.has_dependency(n))
    }
}

/// Reads and parses `<root>/Cargo.toml`, if present.
///
/// Returns `None` (not an error) when there is no `Cargo.toml` or it fails
/// to parse: a missing/unreadable manifest is a detection signal, not a
/// hard failure.
pub fn read_cargo_manifest(root: &Path) -> Option<CargoManifest> {
    let raw = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let value: toml::Value = toml::from_str(&raw).ok()?;

    let mut dependency_names = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = value.get(section).and_then(toml::Value::as_table) {
            dependency_names.extend(table.keys().cloned());
        }
        if let Some(table) = value
            .get("workspace")
            .and_then(|w| w.get(section))
            .and_then(toml::Value::as_table)
        {
            dependency_names.extend(table.keys().cloned());
        }
    }
    dependency_names.sort();
    dependency_names.dedup();

    Some(CargoManifest { dependency_names })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, contents: &str) {
        std::fs::write(dir.join("Cargo.toml"), contents).unwrap();
    }

    #[test]
    fn reads_direct_and_workspace_dependencies() {
        let dir = super::super::test_support::temp_dir("manifest-direct");
        write_manifest(
            &dir.path,
            r#"
            [package]
            name = "example"
            version = "0.1.0"

            [dependencies]
            soroban-sdk = "22"

            [workspace.dependencies]
            stellar-xdr = "28"
            "#,
        );

        let manifest = read_cargo_manifest(&dir.path).expect("manifest");
        assert!(manifest.has_dependency("soroban-sdk"));
        assert!(manifest.has_dependency("stellar-xdr"));
        assert!(!manifest.has_dependency("nonexistent"));
    }

    #[test]
    fn missing_manifest_returns_none() {
        let dir = super::super::test_support::temp_dir("manifest-missing");
        assert!(read_cargo_manifest(&dir.path).is_none());
    }

    #[test]
    fn has_any_dependency_matches_if_one_name_is_present() {
        let manifest = CargoManifest {
            dependency_names: vec!["stellar-rpc-client".to_string()],
        };
        assert!(manifest.has_any_dependency(&["stellar-sdk", "stellar-rpc-client"]));
        assert!(!manifest.has_any_dependency(&["soroban-sdk"]));
    }
}
