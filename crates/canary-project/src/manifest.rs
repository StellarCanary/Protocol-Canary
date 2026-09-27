//! Lightweight manifest inspection.
//!
//! This intentionally does not use `cargo_metadata` or invoke package managers: for
//! detection purposes we only need the declared dependency names, and
//! reading manifests directly keeps detection fast and independent of
//! whether the target project's dependencies are even fetched yet.

use std::path::Path;

/// The dependency names declared by a project manifest (e.g. Cargo.toml, package.json).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectManifest {
    pub dependency_names: Vec<String>,
}

impl ProjectManifest {
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
pub fn read_cargo_manifest(root: &Path) -> Option<ProjectManifest> {
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

    Some(ProjectManifest { dependency_names })
}

/// Reads and parses `<root>/package.json`, if present.
pub fn read_package_json(root: &Path) -> Option<ProjectManifest> {
    let raw = std::fs::read_to_string(root.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;

    let mut dependency_names = Vec::new();
    for section in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(obj) = value.get(section).and_then(|v| v.as_object()) {
            dependency_names.extend(obj.keys().cloned());
        }
    }
    dependency_names.sort();
    dependency_names.dedup();

    Some(ProjectManifest { dependency_names })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, contents: &str) {
        std::fs::write(dir.join("Cargo.toml"), contents).unwrap();
    }

    fn write_package_json(dir: &Path, contents: &str) {
        std::fs::write(dir.join("package.json"), contents).unwrap();
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
    fn reads_package_json_dependencies() {
        let dir = super::super::test_support::temp_dir("manifest-package");
        write_package_json(
            &dir.path,
            r#"{
                "name": "example",
                "dependencies": {
                    "@stellar/stellar-sdk": "^12.0.0"
                },
                "devDependencies": {
                    "typescript": "^5.0.0"
                }
            }"#,
        );

        let manifest = read_package_json(&dir.path).expect("manifest");
        assert!(manifest.has_dependency("@stellar/stellar-sdk"));
        assert!(manifest.has_dependency("typescript"));
        assert!(!manifest.has_dependency("nonexistent"));
    }

    #[test]
    fn missing_manifest_returns_none() {
        let dir = super::super::test_support::temp_dir("manifest-missing");
        assert!(read_cargo_manifest(&dir.path).is_none());
        assert!(read_package_json(&dir.path).is_none());
    }

    #[test]
    fn has_any_dependency_matches_if_one_name_is_present() {
        let manifest = ProjectManifest {
            dependency_names: vec!["stellar-rpc-client".to_string()],
        };
        assert!(manifest.has_any_dependency(&["stellar-sdk", "stellar-rpc-client"]));
        assert!(!manifest.has_any_dependency(&["soroban-sdk"]));
    }
}
