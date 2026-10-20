//! Integration tests for capability evidence (CA-04, CF-07, E-008, issue #412).
//!
//! Validates:
//! - Each detected capability includes at least one evidence record.
//! - Evidence records contain normalized relative paths to manifest files.
//! - Populated `source_file`, `matched_text`, and `reason` fields.
//! - Negative testing: project roots without matching files produce empty capability sets with zero evidence.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_core::{Capability, ProjectType};
use canary_project::{detect, detect_details};

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
