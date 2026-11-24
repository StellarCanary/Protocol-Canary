//! Integration tests for capability evidence (CA-04, CF-07, E-008, issue #412).
//!
//! Validates:
//! - Each detected capability includes at least one evidence record.
//! - Evidence records contain normalized relative paths to manifest files.
//! - Populated `source_file`, `matched_text`, and `reason` fields.
//! - Negative testing: project roots without matching files produce empty capability sets with zero evidence.
//! - `[package.metadata.stellar-canary]` capability annotations in sample
//!   `Cargo.toml` strings (issue #753): annotations use the engine's
//!   capability vocabulary, every annotation backed by a declared dependency
//!   has evidence whose origin is the Cargo manifest (path, ecosystem and
//!   section), and the metadata table itself is never mistaken for a
//!   dependency section or a source of evidence.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_core::{Capability, ProjectType};
use canary_project::{
    detect, detect_details, parse_cargo_dependencies, read_cargo_manifest, DetailedDetection,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "canary-ev-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, text: &str) {
        let path = relative
            .split('/')
            .fold(self.0.clone(), |path, segment| path.join(segment));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn each_detected_capability_includes_at_least_one_evidence_record() {
    let scratch = Scratch::new("detected-caps");
    scratch.write(
        "Cargo.toml",
        r#"
[package]
name = "my-stellar-service"
version = "0.1.0"

[dependencies]
soroban-sdk = "22"
stellar-rpc-client = "0.1"
"#,
    );

    let details = detect_details(scratch.path());
    assert_eq!(details.context.project_type, ProjectType::Soroban);
    assert_eq!(details.context.capabilities.len(), 2);
    assert_eq!(details.capability_evidence.len(), 2);

    for cap_ev in &details.capability_evidence {
        assert!(
            !cap_ev.evidence.is_empty(),
            "detected capability {:?} must include at least one evidence record",
            cap_ev.capability
        );
        for item in &cap_ev.evidence {
            assert_eq!(item.manifest, "Cargo.toml");
            assert_eq!(item.source_file, "Cargo.toml");
            assert!(!item.matched_text.is_empty());
            assert!(!item.reason.is_empty());
        }
    }
}

#[test]
fn evidence_records_contain_normalized_relative_paths() {
    let scratch = Scratch::new("normalized-paths");
    scratch.write(
        "Cargo.toml",
        r#"
[workspace]
members = ["contracts/vault"]
"#,
    );
    scratch.write(
        "contracts/vault/Cargo.toml",
        r#"
[package]
name = "vault"
version = "0.1.0"

[dependencies]
soroban-sdk = "22"
"#,
    );

    let details = detect_details(scratch.path());
    let soroban = details
        .capability_evidence
        .iter()
        .find(|c| c.capability == Capability::SorobanContract)
        .expect("soroban capability evidence");

    assert!(!soroban.evidence.is_empty());
    let ev = &soroban.evidence[0];
    assert_eq!(ev.manifest, "contracts/vault/Cargo.toml");
    assert_eq!(ev.source_file, "contracts/vault/Cargo.toml");
    assert!(!ev.source_file.starts_with('/'));
    assert!(!ev.source_file.starts_with("./"));
    assert_eq!(ev.matched_text, "soroban-sdk");
    assert!(ev.reason.contains("soroban-sdk"));
}

#[test]
fn wasm_artifact_capability_includes_evidence_record() {
    let scratch = Scratch::new("wasm-evidence");
    let wasm_dir = scratch.path().join("target/wasm32-unknown-unknown/release");
    std::fs::create_dir_all(&wasm_dir).unwrap();
    std::fs::write(wasm_dir.join("token.wasm"), b"\0asm").unwrap();

    let details = detect_details(scratch.path());
    let wasm_ev = details
        .capability_evidence
        .iter()
        .find(|c| c.capability == Capability::WasmArtifact)
        .expect("wasm artifact capability evidence");

    assert_eq!(wasm_ev.evidence.len(), 1);
    let item = &wasm_ev.evidence[0];
    assert_eq!(
        item.source_file,
        "target/wasm32-unknown-unknown/release/token.wasm"
    );
    assert_eq!(item.manifest, item.source_file);
    assert_eq!(item.matched_text, "token.wasm");
    assert_eq!(item.ecosystem, "wasm");
    assert!(!item.reason.is_empty());
}

#[test]
fn capability_models_conversion_preserves_evidence() {
    let scratch = Scratch::new("models-conversion");
    scratch.write(
        "package.json",
        r#"
{
  "name": "js-stellar-app",
  "dependencies": {
    "@stellar/stellar-sdk": "^11.0.0"
  }
}
"#,
    );

    let details = detect_details(scratch.path());
    let models = details.capability_models();
    assert!(!models.is_empty());

    for model in &models {
        assert!(model.has_evidence());
        assert_eq!(model.evidence[0].source_file, "package.json");
        assert_eq!(model.evidence[0].matched_text, "@stellar/stellar-sdk");
        assert!(model.evidence[0].reason.contains("@stellar/stellar-sdk"));
    }
}

#[test]
fn negative_project_roots_without_matching_files_produce_empty_capability_sets_with_zero_evidence()
{
    let empty_scratch = Scratch::new("empty-root");
    let ctx = detect(empty_scratch.path());
    assert!(ctx.capabilities.is_empty());

    let details = detect_details(empty_scratch.path());
    assert!(details.context.capabilities.is_empty());
    assert!(
        details.capability_evidence.is_empty(),
        "empty directory must produce zero capability evidence"
    );
    assert!(details.capability_models().is_empty());

    let unrelated_scratch = Scratch::new("unrelated-root");
    unrelated_scratch.write(
        "package.json",
        r#"
{
  "name": "unrelated-react-app",
  "dependencies": {
    "react": "^18.0.0",
    "react-dom": "^18.0.0"
  }
}
"#,
    );

    let unrelated_details = detect_details(unrelated_scratch.path());
    assert!(unrelated_details.context.capabilities.is_empty());
    assert!(
        unrelated_details.capability_evidence.is_empty(),
        "unrelated project must produce zero capability evidence"
    );
    assert!(unrelated_details.capability_models().is_empty());
}

// ---------------------------------------------------------------------------
// `[package.metadata.stellar-canary]` annotations (issue #753)
// ---------------------------------------------------------------------------

/// A contract crate that annotates its capabilities in the metadata table
/// Cargo reserves for tools, and declares the dependencies backing them.
const ANNOTATED_CONTRACT: &str = r#"
[package]
name = "annotated-contract"
version = "0.1.0"
edition = "2021"

[package.metadata.stellar-canary]
capabilities = ["soroban-contract", "rpc-client"]
notes = "token contract with an off-chain RPC helper"

[dependencies]
soroban-sdk = "22"
stellar-rpc-client = "0.1"

[dev-dependencies]
soroban-sdk = { version = "22", features = ["testutils"] }
"#;

/// Reads the `capabilities` annotation out of a manifest's
/// `[package.metadata.stellar-canary]` table and resolves every entry
/// against the engine's [`Capability`] vocabulary.
///
/// A manifest without the table yields an empty list; an entry that is not
/// a string, or not a tag the engine declares, is an error naming it.
fn metadata_capabilities(raw: &str) -> Result<Vec<Capability>, String> {
    let value: toml::Value = toml::from_str(raw).map_err(|e| format!("invalid TOML: {e}"))?;
    let Some(annotations) = value
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("stellar-canary"))
        .and_then(|table| table.get("capabilities"))
    else {
        return Ok(Vec::new());
    };
    let entries = annotations
        .as_array()
        .ok_or_else(|| format!("capabilities must be an array, got {annotations}"))?;
    entries
        .iter()
        .map(|entry| {
            let tag = entry
                .as_str()
                .ok_or_else(|| format!("capability annotation {entry} is not a string"))?;
            serde_json::from_value::<Capability>(serde_json::Value::String(tag.to_string()))
                .map_err(|_| format!("unknown capability annotation {tag:?}"))
        })
        .collect()
}

/// The evidence records the detector produced for `capability`, if any.
fn evidence_for(
    details: &DetailedDetection,
    capability: &Capability,
) -> Vec<canary_project::DetectionEvidence> {
    details
        .capability_evidence
        .iter()
        .filter(|entry| &entry.capability == capability)
        .flat_map(|entry| entry.evidence.iter().cloned())
        .collect()
}

#[test]
fn metadata_table_capability_annotations_resolve_to_engine_capabilities() {
    let annotations = metadata_capabilities(ANNOTATED_CONTRACT).expect("annotations are valid");
    assert_eq!(
        annotations,
        vec![Capability::SorobanContract, Capability::RpcClient],
        "annotations are extracted in the order they are written"
    );
    for capability in &annotations {
        assert!(
            Capability::all().contains(capability),
            "{capability:?} is not a capability the engine declares"
        );
    }

    // A manifest without the metadata table annotates nothing.
    assert_eq!(
        metadata_capabilities("[package]\nname = \"plain\"\nversion = \"0.1.0\"\n"),
        Ok(Vec::new())
    );
}

#[test]
fn metadata_annotations_outside_the_capability_vocabulary_are_rejected() {
    let unknown = r#"
[package]
name = "typo"
version = "0.1.0"

[package.metadata.stellar-canary]
capabilities = ["soroban-contract", "soroban_contract"]
"#;
    let error = metadata_capabilities(unknown).expect_err("snake_case tag is not a capability");
    assert!(error.contains("soroban_contract"), "{error}");

    let not_a_string = r#"
[package]
name = "typo"
version = "0.1.0"

[package.metadata.stellar-canary]
capabilities = ["rpc-client", 7]
"#;
    let error = metadata_capabilities(not_a_string).expect_err("non-string entry is rejected");
    assert!(error.contains("not a string"), "{error}");
}

#[test]
fn annotated_capabilities_have_evidence_originating_in_the_cargo_manifest() {
    let scratch = Scratch::new("metadata-annotated");
    scratch.write("Cargo.toml", ANNOTATED_CONTRACT);

    let details = detect_details(scratch.path());
    assert_eq!(details.context.project_type, ProjectType::Soroban);
    assert!(details.inspected.contains(&"Cargo.toml".to_string()));
    assert!(
        details.diagnostics.is_empty(),
        "a metadata table must not produce diagnostics: {:?}",
        details.diagnostics
    );

    for capability in metadata_capabilities(ANNOTATED_CONTRACT).unwrap() {
        assert!(
            details.context.capabilities.contains(&capability),
            "annotated {capability:?} must be detected"
        );
        let evidence = evidence_for(&details, &capability);
        assert!(
            !evidence.is_empty(),
            "annotated {capability:?} must carry at least one evidence record"
        );
        for item in &evidence {
            // The evidence origin is the Cargo manifest itself...
            assert_eq!(item.manifest, "Cargo.toml");
            assert_eq!(item.source_file, "Cargo.toml");
            assert_eq!(item.ecosystem, "cargo");
            // ...and, inside it, a real dependency section rather than the
            // tool metadata table.
            let section = item
                .section
                .as_deref()
                .expect("manifest evidence names a section");
            assert!(
                matches!(section, "dependencies" | "dev-dependencies"),
                "unexpected section {section:?}"
            );
            assert!(!section.contains("metadata"), "{section:?}");
            let dependency = item.dependency.as_deref().expect("names its dependency");
            assert_eq!(item.matched_text, dependency);
            assert_eq!(
                item.reason,
                format!("cargo {section} declares {dependency}")
            );
        }
    }

    // soroban-sdk is declared in both [dependencies] and [dev-dependencies]:
    // each section is its own record, both pointing at the same manifest.
    let soroban_sections: Vec<String> = evidence_for(&details, &Capability::SorobanContract)
        .into_iter()
        .filter_map(|item| item.section)
        .collect();
    assert!(soroban_sections.contains(&"dependencies".to_string()));
    assert!(soroban_sections.contains(&"dev-dependencies".to_string()));
}

#[test]
fn the_metadata_table_is_never_read_as_a_dependency_section() {
    // Dependency-like keys nested under the tool metadata table must not
    // turn into dependencies, capabilities or evidence.
    let raw = r#"
[package]
name = "metadata-only"
version = "0.1.0"

[package.metadata.stellar-canary]
capabilities = ["soroban-contract"]

[package.metadata.stellar-canary.dependencies]
soroban-sdk = "22"
stellar-rpc-client = "0.1"
"#;

    let deps = parse_cargo_dependencies(raw).expect("valid TOML parses");
    assert!(
        deps.is_empty(),
        "metadata keys leaked into dependencies: {deps:?}"
    );

    let scratch = Scratch::new("metadata-only");
    scratch.write("Cargo.toml", raw);
    let manifest = read_cargo_manifest(scratch.path()).expect("manifest parses");
    assert!(manifest.dependency_names.is_empty());

    let details = detect_details(scratch.path());
    assert!(details.inspected.contains(&"Cargo.toml".to_string()));
    assert!(details.diagnostics.is_empty(), "{:?}", details.diagnostics);
    assert!(
        details.context.capabilities.is_empty(),
        "an annotation with no backing dependency is not detection evidence today; \
         if metadata-driven capabilities are introduced, update this test with them"
    );
    assert!(details.capability_evidence.is_empty());
}

#[test]
fn an_annotation_without_a_backing_dependency_does_not_fabricate_evidence() {
    let raw = r#"
[package]
name = "over-annotated"
version = "0.1.0"

[package.metadata.stellar-canary]
capabilities = ["soroban-contract", "rpc-client", "raw-ledger-access"]

[dependencies]
soroban-sdk = "22"
"#;
    let scratch = Scratch::new("metadata-over-annotated");
    scratch.write("Cargo.toml", raw);

    let details = detect_details(scratch.path());
    assert_eq!(
        details.context.capabilities,
        vec![Capability::SorobanContract]
    );
    assert_eq!(details.capability_evidence.len(), 1);
    let soroban = &details.capability_evidence[0];
    assert_eq!(soroban.capability, Capability::SorobanContract);
    assert_eq!(soroban.evidence.len(), 1);
    assert_eq!(soroban.evidence[0].manifest, "Cargo.toml");
    assert_eq!(
        soroban.evidence[0].dependency.as_deref(),
        Some("soroban-sdk")
    );
    assert!(evidence_for(&details, &Capability::RpcClient).is_empty());
    assert!(evidence_for(&details, &Capability::RawLedgerAccess).is_empty());
}

#[test]
fn member_metadata_annotations_attribute_evidence_to_the_member_manifest() {
    let scratch = Scratch::new("metadata-workspace");
    scratch.write(
        "Cargo.toml",
        r#"
[workspace]
members = ["contracts/vault"]

[workspace.metadata.stellar-canary]
capabilities = ["rpc-client"]
"#,
    );
    let member = r#"
[package]
name = "vault"
version = "0.1.0"

[package.metadata.stellar-canary]
capabilities = ["soroban-contract"]

[dependencies]
soroban-sdk = "22"
"#;
    scratch.write("contracts/vault/Cargo.toml", member);

    let details = detect_details(scratch.path());
    assert_eq!(details.workspace_members, vec!["contracts/vault"]);
    assert!(details.diagnostics.is_empty(), "{:?}", details.diagnostics);

    // The workspace-level metadata table adds nothing of its own.
    assert_eq!(
        details.context.capabilities,
        vec![Capability::SorobanContract]
    );

    for capability in metadata_capabilities(member).unwrap() {
        let evidence = evidence_for(&details, &capability);
        assert_eq!(evidence.len(), 1, "{capability:?}: {evidence:?}");
        assert_eq!(evidence[0].manifest, "contracts/vault/Cargo.toml");
        assert_eq!(evidence[0].source_file, "contracts/vault/Cargo.toml");
        assert_eq!(evidence[0].ecosystem, "cargo");
        assert_eq!(evidence[0].section.as_deref(), Some("dependencies"));
    }
}
