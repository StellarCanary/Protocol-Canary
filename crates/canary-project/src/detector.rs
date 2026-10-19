//! Project type detection.

use std::path::{Path, PathBuf};

use canary_core::{Capability, ProjectContext, ProjectType};

use crate::capabilities::{
    detect_capabilities, DetectionSignals, RPC_CLIENT_DEPENDENCIES, SOROBAN_DEPENDENCIES,
    STELLAR_SDK_DEPENDENCIES,
};
use crate::manifest::{
    parse_cargo_dependencies, parse_cargo_manifest, parse_package_json_dependencies,
    read_package_json,
};
use crate::workspace::scan_cargo_workspace;

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

/// One piece of evidence supporting a detected capability (CF-07 section 6, CA-04).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionEvidence {
    /// Root-relative path to the manifest or artifact with `/` separators.
    pub manifest: String,
    /// `cargo`, `npm`, or `wasm`.
    pub ecosystem: String,
    /// The matching dependency name, or `None` for artifact evidence.
    pub dependency: Option<String>,
    /// The manifest section (e.g. `dependencies`), or `None` for artifact evidence.
    pub section: Option<String>,
    /// Normalized root-relative path to the manifest or artifact file.
    pub source_file: String,
    /// The matched dependency, string, or artifact identifier.
    pub matched_text: String,
    /// The reason or manifest section explaining why this evidence produces the capability.
    pub reason: String,
}

impl DetectionEvidence {
    /// Converts this detection evidence into a [`crate::models::CapabilityEvidence`] record.
    pub fn to_capability_evidence(&self) -> crate::models::CapabilityEvidence {
        crate::models::CapabilityEvidence {
            source_file: self.source_file.clone(),
            matched_text: self.matched_text.clone(),
            reason: self.reason.clone(),
        }
    }
}

/// A detected capability together with the evidence records that produced it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityEvidence {
    pub capability: Capability,
    pub evidence: Vec<DetectionEvidence>,
}

impl CapabilityEvidence {
    /// Converts this detected capability into a [`crate::models::CapabilityModel`].
    pub fn to_model(&self) -> crate::models::CapabilityModel {
        crate::models::CapabilityModel {
            capability: self.capability.clone(),
            evidence: self
                .evidence
                .iter()
                .map(|e| e.to_capability_evidence())
                .collect(),
        }
    }
}

/// The complete detection outcome, including context, evidence, workspace members,
/// and diagnostics per CF-07 and CA-03.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailedDetection {
    pub context: ProjectContext,
    pub inspected: Vec<String>,
    pub workspace_members: Vec<String>,
    pub diagnostics: Vec<crate::workspace::Diagnostic>,
    pub capability_evidence: Vec<CapabilityEvidence>,
}

impl DetailedDetection {
    /// Returns the detected capabilities and their evidence as [`crate::models::CapabilityModel`] records.
    pub fn capability_models(&self) -> Vec<crate::models::CapabilityModel> {
        self.capability_evidence
            .iter()
            .map(|ce| ce.to_model())
            .collect()
    }
}

/// Detects the [`ProjectContext`] for the project rooted at `root`.
///
/// `Unknown` is a valid, non-error outcome: it means the project has no
/// recognizable Stellar surface and needs explicit configuration.
pub fn detect(root: &Path) -> ProjectContext {
    detect_details(root).context
}

/// Runs full detection on `root`, returning the project context along with
/// capability evidence, inspected manifests, workspace members, and diagnostics.
pub fn detect_details(root: &Path) -> DetailedDetection {
    let mut manifests = Vec::new();
    let mut inspected = Vec::new();
    let mut all_evidence = Vec::new();

    // 1. Root Cargo.toml
    let root_cargo = root.join("Cargo.toml");
    if let Ok(raw) = std::fs::read_to_string(&root_cargo) {
        inspected.push("Cargo.toml".to_string());
        if let Some(m) = parse_cargo_manifest(&raw) {
            manifests.push(m);
        }
        if let Some(deps) = parse_cargo_dependencies(&raw) {
            for dep in deps {
                record_dep_evidence(
                    &dep.name,
                    &dep.section,
                    "Cargo.toml",
                    "cargo",
                    &mut all_evidence,
                );
            }
        }
    }

    // 2. Cargo workspace members
    let workspace_scan = scan_cargo_workspace(root);
    let mut workspace_members = Vec::new();
    for member in &workspace_scan.members {
        workspace_members.push(member.path.clone());
        let member_rel = format!("{}/Cargo.toml", member.path);
        let member_path = root.join(&member_rel);
        if let Ok(raw) = std::fs::read_to_string(&member_path) {
            inspected.push(member_rel.clone());
            if let Some(deps) = parse_cargo_dependencies(&raw) {
                for dep in deps {
                    record_dep_evidence(
                        &dep.name,
                        &dep.section,
                        &member_rel,
                        "cargo",
                        &mut all_evidence,
                    );
                }
            }
        }
        manifests.push(member.manifest.clone());
    }
    workspace_members.sort();
    workspace_members.dedup();

    // 3. package.json
    let root_pkg = root.join("package.json");
    if let Ok(raw) = std::fs::read_to_string(&root_pkg) {
        inspected.push("package.json".to_string());
        if let Some(m) = read_package_json(root) {
            manifests.push(m);
        }
        if let Some(deps) = parse_package_json_dependencies(&raw) {
            for dep in deps {
                record_dep_evidence(
                    &dep.name,
                    &dep.section,
                    "package.json",
                    "npm",
                    &mut all_evidence,
                );
            }
        }
    }

    inspected.sort();
    inspected.dedup();

    let has_stellar_toml = root.join("stellar.toml").is_file();
    let wasm_artifacts = find_wasm_artifacts(root);
    let has_wasm_artifact = !wasm_artifacts.is_empty();

    if has_wasm_artifact {
        for wasm_path in &wasm_artifacts {
            let rel_str = relative_slash_path(root, wasm_path);
            let matched_name = wasm_path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "*.wasm".to_string());
            all_evidence.push((
                Capability::WasmArtifact,
                DetectionEvidence {
                    manifest: rel_str.clone(),
                    ecosystem: "wasm".to_string(),
                    dependency: None,
                    section: None,
                    source_file: rel_str,
                    matched_text: matched_name,
                    reason: "compiled Soroban WASM artifact found in build output".to_string(),
                },
            ));
        }
    }

    let signals = DetectionSignals {
        manifests,
        has_stellar_toml,
        has_wasm_artifact,
    };
    let capabilities = detect_capabilities(&signals);
    let project_type = classify(&capabilities, has_stellar_toml);

    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());

    let mut capability_evidence = Vec::new();
    for cap in &capabilities {
        let mut evidence: Vec<DetectionEvidence> = all_evidence
            .iter()
            .filter(|(c, _)| c == cap)
            .map(|(_, ev)| ev.clone())
            .collect();
        evidence.sort_by(|a, b| {
            a.manifest
                .cmp(&b.manifest)
                .then_with(|| a.dependency.cmp(&b.dependency))
        });
        evidence.dedup();
        capability_evidence.push(CapabilityEvidence {
            capability: cap.clone(),
            evidence,
        });
    }

    let context = ProjectContext {
        root: root.to_path_buf(),
        name,
        project_type,
        capabilities,
    };

    DetailedDetection {
        context,
        inspected,
        workspace_members,
        diagnostics: workspace_scan.diagnostics,
        capability_evidence,
    }
}

fn record_dep_evidence(
    name: &str,
    section: &str,
    manifest: &str,
    ecosystem: &str,
    all_evidence: &mut Vec<(Capability, DetectionEvidence)>,
) {
    let source_file = manifest.to_string();
    let matched_text = name.to_string();
    let reason = format!("{ecosystem} {section} declares {name}");
    if SOROBAN_DEPENDENCIES.contains(&name) {
        all_evidence.push((
            Capability::SorobanContract,
            DetectionEvidence {
                manifest: manifest.to_string(),
                ecosystem: ecosystem.to_string(),
                dependency: Some(name.to_string()),
                section: Some(section.to_string()),
                source_file: source_file.clone(),
                matched_text: matched_text.clone(),
                reason: reason.clone(),
            },
        ));
    }
    if STELLAR_SDK_DEPENDENCIES.contains(&name) {
        all_evidence.push((
            Capability::StellarSdkDependency,
            DetectionEvidence {
                manifest: manifest.to_string(),
                ecosystem: ecosystem.to_string(),
                dependency: Some(name.to_string()),
                section: Some(section.to_string()),
                source_file: source_file.clone(),
                matched_text: matched_text.clone(),
                reason: reason.clone(),
            },
        ));
    }
    if RPC_CLIENT_DEPENDENCIES.contains(&name) {
        all_evidence.push((
            Capability::RpcClient,
            DetectionEvidence {
                manifest: manifest.to_string(),
                ecosystem: ecosystem.to_string(),
                dependency: Some(name.to_string()),
                section: Some(section.to_string()),
                source_file,
                matched_text,
                reason,
            },
        ));
    }
}

fn relative_slash_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut parts = Vec::new();
    for component in rel.components() {
        if let std::path::Component::Normal(part) = component {
            parts.push(part.to_string_lossy().into_owned());
        }
    }
    parts.join("/")
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

/// The WASM artifacts the detector sees at `root`: files with a `.wasm`
/// extension in the known Cargo output directories or up to two
/// directories below the root, skipping `.git`, `node_modules` and
/// `target`.
///
/// Paths are absolute and in no particular order; callers that need a
/// deterministic, root-relative form (e.g. the CA-04 fingerprint) sort
/// and normalize them themselves.
pub fn find_wasm_artifacts(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for known in KNOWN_WASM_OUTPUT_DIRS {
        collect_wasm_in_dir(&root.join(known), &mut found);
    }
    collect_wasm_shallow(root, 2, &mut found);
    found
}

fn collect_wasm_in_dir(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "wasm") {
            found.push(path);
        }
    }
}

fn collect_wasm_shallow(dir: &Path, remaining_depth: u32, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if path.is_dir() {
            if SKIP_DIRS.contains(&file_name.as_ref()) || file_name == "target" {
                continue;
            }
            if remaining_depth > 0 {
                collect_wasm_shallow(&path, remaining_depth - 1, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "wasm") {
            found.push(path);
        }
    }
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

    #[test]
    fn a_wasm_file_outside_known_output_dirs_is_found_by_the_fallback_scan() {
        let dir = crate::test_support::temp_dir("detect-wasm-fallback");
        std::fs::write(dir.path.join("contract.wasm"), b"\0asm").unwrap();

        let ctx = detect(&dir.path);
        assert!(ctx
            .capabilities
            .contains(&canary_core::Capability::WasmArtifact));
    }

    #[test]
    fn wasm_files_inside_skipped_directories_are_not_detected() {
        let dir = crate::test_support::temp_dir("detect-wasm-skip");
        for skipped in [".git", "node_modules"] {
            std::fs::create_dir_all(dir.path.join(skipped)).unwrap();
            std::fs::write(dir.path.join(skipped).join("contract.wasm"), b"\0asm").unwrap();
        }

        let ctx = detect(&dir.path);
        assert!(!ctx
            .capabilities
            .contains(&canary_core::Capability::WasmArtifact));
    }

    #[test]
    fn a_wasm_file_deeper_than_the_scan_depth_limit_is_not_detected() {
        // shallow_scan_for_wasm starts with a remaining depth of 2, so a
        // .wasm file up to two directories below the root is still found...
        let shallow = crate::test_support::temp_dir("detect-wasm-shallow");
        std::fs::create_dir_all(shallow.path.join("a/b")).unwrap();
        std::fs::write(shallow.path.join("a/b/contract.wasm"), b"\0asm").unwrap();
        assert!(detect(&shallow.path)
            .capabilities
            .contains(&canary_core::Capability::WasmArtifact));

        // ...but three directories below it is beyond the limit.
        let deep = crate::test_support::temp_dir("detect-wasm-deep");
        std::fs::create_dir_all(deep.path.join("a/b/c")).unwrap();
        std::fs::write(deep.path.join("a/b/c/contract.wasm"), b"\0asm").unwrap();
        assert!(!detect(&deep.path)
            .capabilities
            .contains(&canary_core::Capability::WasmArtifact));
    }

    #[test]
    fn soroban_contract_outranks_rpc_client() {
        use canary_core::Capability;
        assert_eq!(
            classify(&[Capability::RpcClient, Capability::SorobanContract], false),
            ProjectType::Soroban
        );
    }

    #[test]
    fn rpc_client_outranks_stellar_sdk_dependency() {
        use canary_core::Capability;
        assert_eq!(
            classify(
                &[Capability::StellarSdkDependency, Capability::RpcClient],
                false
            ),
            ProjectType::RpcConsumer
        );
    }

    #[test]
    fn stellar_sdk_dependency_outranks_stellar_toml() {
        use canary_core::Capability;
        assert_eq!(
            classify(&[Capability::StellarSdkDependency], true),
            ProjectType::StellarSdk
        );
    }

    #[test]
    fn detect_details_gathers_evidence_inspected_and_workspace() {
        let dir = crate::test_support::temp_dir("detect-details-ws");
        std::fs::write(
            dir.path.join("Cargo.toml"),
            "[workspace]\nmembers = [\"contracts/*\"]\n",
        )
        .unwrap();

        let contract_dir = dir.path.join("contracts/token");
        std::fs::create_dir_all(&contract_dir).unwrap();
        std::fs::write(
            contract_dir.join("Cargo.toml"),
            "[package]\nname = \"token\"\nversion = \"0.1.0\"\n\n[dependencies]\nsoroban-sdk = \"22\"\n",
        )
        .unwrap();

        let details = detect_details(&dir.path);
        assert_eq!(details.context.project_type, ProjectType::Soroban);
        assert_eq!(details.workspace_members, vec!["contracts/token"]);
        assert!(details.inspected.contains(&"Cargo.toml".to_string()));
        assert!(details
            .inspected
            .contains(&"contracts/token/Cargo.toml".to_string()));

        let soroban_ev = details
            .capability_evidence
            .iter()
            .find(|c| c.capability == Capability::SorobanContract)
            .expect("soroban evidence");
        assert_eq!(soroban_ev.evidence.len(), 1);
        assert_eq!(
            soroban_ev.evidence[0].manifest,
            "contracts/token/Cargo.toml"
        );
        assert_eq!(soroban_ev.evidence[0].ecosystem, "cargo");
        assert_eq!(
            soroban_ev.evidence[0].dependency.as_deref(),
            Some("soroban-sdk")
        );
        assert_eq!(
            soroban_ev.evidence[0].section.as_deref(),
            Some("dependencies")
        );
        assert_eq!(
            soroban_ev.evidence[0].source_file,
            "contracts/token/Cargo.toml"
        );
        assert_eq!(soroban_ev.evidence[0].matched_text, "soroban-sdk");
        assert!(soroban_ev.evidence[0].reason.contains("soroban-sdk"));
    }

    #[test]
    fn every_detected_capability_has_at_least_one_evidence_record_with_normalized_path() {
        let dir = crate::test_support::temp_dir("evidence-normalized-paths");
        std::fs::write(
            dir.path.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\nsoroban-sdk = \"22\"\nstellar-rpc-client = \"0.1\"\n",
        )
        .unwrap();

        let details = detect_details(&dir.path);
        assert_eq!(details.capability_evidence.len(), 2);

        for cap_ev in &details.capability_evidence {
            assert!(
                !cap_ev.evidence.is_empty(),
                "each detected capability must have at least one evidence record"
            );
            for ev in &cap_ev.evidence {
                assert!(!ev.source_file.is_empty());
                assert!(
                    !ev.source_file.starts_with('/') && !ev.source_file.starts_with("./"),
                    "source_file must be a normalized relative path: {}",
                    ev.source_file
                );
                assert!(!ev.matched_text.is_empty());
                assert!(!ev.reason.is_empty());
            }
        }

        let models = details.capability_models();
        assert_eq!(models.len(), 2);
        for m in &models {
            assert!(m.has_evidence());
            assert_eq!(m.evidence[0].source_file, "Cargo.toml");
        }
    }

    #[test]
    fn negative_empty_project_root_produces_empty_capabilities_and_zero_evidence() {
        let dir = crate::test_support::temp_dir("evidence-negative-empty");
        let details = detect_details(&dir.path);
        assert!(details.context.capabilities.is_empty());
        assert!(details.capability_evidence.is_empty());
        assert!(details.capability_models().is_empty());
    }
}
