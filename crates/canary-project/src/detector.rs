//! Project type detection.

use std::path::Path;

use canary_core::{ProjectContext, ProjectType};

use crate::capabilities::{detect_capabilities, DetectionSignals};
use crate::manifest::read_cargo_manifest;

/// Directories that are never worth descending into while looking for a
/// checked-in or built WASM artifact.
const SKIP_DIRS: &[&str] = &[".git", "node_modules"];

/// Common Soroban build output locations, checked directly rather than by
/// a full recursive walk of `target/`, which can be very large.
const KNOWN_WASM_OUTPUT_DIRS: &[&str] = &[
    "target/wasm32-unknown-unknown/release",
    "target/wasm32-unknown-unknown/debug",
    "target/wasm32v1-none/release",
    "target/wasm32v1-none/debug",
];

/// Detects the [`ProjectContext`] for the project rooted at `root`.
///
/// `Unknown` is a valid, non-error outcome: it means the project has no
/// recognizable Stellar surface and needs explicit configuration.
pub fn detect(root: &Path) -> ProjectContext {
    let manifest = read_cargo_manifest(root);
    let has_stellar_toml = root.join("stellar.toml").is_file();
    let has_wasm_artifact = find_wasm_artifact(root);

    let signals = DetectionSignals {
        manifest,
        has_stellar_toml,
        has_wasm_artifact,
    };
    let capabilities = detect_capabilities(&signals);
    let project_type = classify(&capabilities, has_stellar_toml);

    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());

    ProjectContext {
        root: root.to_path_buf(),
        name,
        project_type,
        capabilities,
    }
}

/// Applies an explicit `[project].type` configuration override, if any, on
/// top of auto-detection. Explicit configuration always wins.
pub fn resolve_project_type(detected: ProjectType, explicit: Option<ProjectType>) -> ProjectType {
    explicit.unwrap_or(detected)
}

fn classify(capabilities: &[canary_core::Capability], has_stellar_toml: bool) -> ProjectType {
    use canary_core::Capability;

    if capabilities.contains(&Capability::SorobanContract) {
        ProjectType::Soroban
    } else if capabilities.contains(&Capability::RpcClient) {
        ProjectType::RpcConsumer
    } else if capabilities.contains(&Capability::StellarSdkDependency) {
        ProjectType::StellarSdk
    } else if has_stellar_toml || capabilities.contains(&Capability::WasmArtifact) {
        ProjectType::GenericStellar
    } else {
        ProjectType::Unknown
    }
}

fn find_wasm_artifact(root: &Path) -> bool {
    for known in KNOWN_WASM_OUTPUT_DIRS {
        if dir_contains_wasm(&root.join(known)) {
            return true;
        }
    }
    shallow_scan_for_wasm(root, 2)
}

fn dir_contains_wasm(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "wasm"))
}

fn shallow_scan_for_wasm(dir: &Path, remaining_depth: u32) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if path.is_dir() {
            if SKIP_DIRS.contains(&file_name.as_ref()) || file_name == "target" {
                continue;
            }
            if remaining_depth > 0 && shallow_scan_for_wasm(&path, remaining_depth - 1) {
                return true;
            }
        } else if path.extension().is_some_and(|ext| ext == "wasm") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soroban_manifest_classifies_as_soroban() {
        let dir = crate::test_support::temp_dir("detect-soroban");
        std::fs::write(
            dir.path.join("Cargo.toml"),
            "[package]\nname = \"c\"\nversion = \"0.1.0\"\n\n[dependencies]\nsoroban-sdk = \"22\"\n",
        )
        .unwrap();

        let ctx = detect(&dir.path);
        assert_eq!(ctx.project_type, ProjectType::Soroban);
        assert!(ctx
            .capabilities
            .contains(&canary_core::Capability::SorobanContract));
    }

    #[test]
    fn stellar_toml_alone_classifies_as_generic_stellar() {
        let dir = crate::test_support::temp_dir("detect-generic");
        std::fs::write(dir.path.join("stellar.toml"), "").unwrap();

        let ctx = detect(&dir.path);
        assert_eq!(ctx.project_type, ProjectType::GenericStellar);
    }

    #[test]
    fn empty_directory_classifies_as_unknown() {
        let dir = crate::test_support::temp_dir("detect-unknown");
        let ctx = detect(&dir.path);
        assert_eq!(ctx.project_type, ProjectType::Unknown);
        assert!(ctx.capabilities.is_empty());
    }

    #[test]
    fn explicit_override_always_wins_over_detection() {
        assert_eq!(
            resolve_project_type(ProjectType::Unknown, Some(ProjectType::Soroban)),
            ProjectType::Soroban
        );
        assert_eq!(
            resolve_project_type(ProjectType::Soroban, None),
            ProjectType::Soroban
        );
    }

    #[test]
    fn wasm_artifact_in_known_build_output_dir_is_detected() {
        let dir = crate::test_support::temp_dir("detect-wasm");
        let wasm_dir = dir.path.join("target/wasm32-unknown-unknown/release");
        std::fs::create_dir_all(&wasm_dir).unwrap();
        std::fs::write(wasm_dir.join("contract.wasm"), b"\0asm").unwrap();

        let ctx = detect(&dir.path);
        assert!(ctx
            .capabilities
            .contains(&canary_core::Capability::WasmArtifact));
    }
}
