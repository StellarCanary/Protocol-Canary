//! Integration tests for Python SDK detection in pyproject.toml (CF-07, E-002, issue #407).
//!
//! Validates:
//! - Detection of `stellar-sdk` dependency across PEP 621 `[project.dependencies]`
//!   and `[project.optional-dependencies]`.
//! - Detection of `stellar-sdk` dependency across Poetry `[tool.poetry.dependencies]`,
//!   `[tool.poetry.dev-dependencies]`, and `[tool.poetry.group.*.dependencies]`.
//! - Generation of capability evidence for `Capability::PythonSdk` / `Capability::StellarSdkDependency`.
//! - Unrelated Python projects do not trigger Stellar capabilities.
//! - Invalid TOML syntax is handled gracefully without panicking and recorded
//!   as a `manifest-unparseable` warning diagnostic.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_core::{Capability, ProjectType};
use canary_project::{
    detect, detect_details, parse_pyproject_dependencies, parse_pyproject_manifest,
    read_pyproject_manifest, read_pyproject_toml, DiagnosticCode,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "canary-manifest-{name}-{}-{}",
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
fn detects_stellar_sdk_in_pep621_dependencies_array() {
    let scratch = Scratch::new("pep621-deps");
    scratch.write(
        "pyproject.toml",
        r#"
[project]
name = "stellar-pep621-app"
version = "0.1.0"
dependencies = [
    "stellar-sdk>=9.0.0",
    "requests>=2.28.0",
]
"#,
    );

    let ctx = detect(scratch.path());
    assert_eq!(ctx.project_type, ProjectType::StellarSdk);
    assert!(ctx.capabilities.contains(&Capability::PythonSdk));
    assert!(ctx.capabilities.contains(&Capability::StellarSdkDependency));

    let details = detect_details(scratch.path());
    assert!(details.inspected.contains(&"pyproject.toml".to_string()));
    assert!(details.diagnostics.is_empty());

    let ev = details
        .capability_evidence
        .iter()
        .find(|c| c.capability == Capability::PythonSdk)
        .expect("PythonSdk capability evidence");
    assert_eq!(ev.evidence.len(), 1);
    assert_eq!(ev.evidence[0].manifest, "pyproject.toml");
    assert_eq!(ev.evidence[0].ecosystem, "python");
    assert_eq!(ev.evidence[0].dependency.as_deref(), Some("stellar-sdk"));
    assert_eq!(
        ev.evidence[0].section.as_deref(),
        Some("project.dependencies")
    );
}

#[test]
fn detects_stellar_sdk_in_pep621_with_extras_and_markers() {
    let scratch = Scratch::new("pep621-extras");
    scratch.write(
        "pyproject.toml",
        r#"
[project]
name = "complex-python-stellar"
dependencies = [
    "stellar-sdk[aiohttp]>=9.0.0; python_version >= '3.10'",
]
"#,
    );

    let ctx = detect(scratch.path());
    assert_eq!(ctx.project_type, ProjectType::StellarSdk);
    assert!(ctx.capabilities.contains(&Capability::PythonSdk));

    let details = detect_details(scratch.path());
    let ev = details
        .capability_evidence
        .iter()
        .find(|c| c.capability == Capability::PythonSdk)
        .expect("PythonSdk evidence");
    assert_eq!(ev.evidence[0].dependency.as_deref(), Some("stellar-sdk"));
    assert_eq!(
        ev.evidence[0].section.as_deref(),
        Some("project.dependencies")
    );
}

#[test]
fn detects_stellar_sdk_in_pep621_optional_dependencies() {
    let scratch = Scratch::new("pep621-optional");
    scratch.write(
        "pyproject.toml",
        r#"
[project]
name = "optional-deps-stellar"
dependencies = [
    "fastapi>=0.100.0",
]

[project.optional-dependencies]
stellar = [
    "stellar-sdk>=9.0.0",
]
"#,
    );

    let details = detect_details(scratch.path());
    assert_eq!(details.context.project_type, ProjectType::StellarSdk);
    assert!(details
        .context
        .capabilities
        .contains(&Capability::PythonSdk));

    let ev = details
        .capability_evidence
        .iter()
        .find(|c| c.capability == Capability::PythonSdk)
        .expect("PythonSdk evidence");
    assert_eq!(ev.evidence[0].dependency.as_deref(), Some("stellar-sdk"));
    assert_eq!(
        ev.evidence[0].section.as_deref(),
        Some("project.optional-dependencies.stellar")
    );
}

#[test]
fn detects_stellar_sdk_in_poetry_dependencies_table() {
    let scratch = Scratch::new("poetry-deps");
    scratch.write(
        "pyproject.toml",
        r#"
[tool.poetry]
name = "poetry-stellar-app"
version = "0.1.0"
description = "Poetry Stellar app"
authors = ["Canary Team"]

[tool.poetry.dependencies]
python = "^3.11"
stellar-sdk = "^9.0.0"
requests = ">=2.28.0"
"#,
    );

    let ctx = detect(scratch.path());
    assert_eq!(ctx.project_type, ProjectType::StellarSdk);
    assert!(ctx.capabilities.contains(&Capability::PythonSdk));

    let details = detect_details(scratch.path());
    assert!(details.inspected.contains(&"pyproject.toml".to_string()));
    assert!(details.diagnostics.is_empty());

    let ev = details
        .capability_evidence
        .iter()
        .find(|c| c.capability == Capability::PythonSdk)
        .expect("PythonSdk capability evidence");
    assert_eq!(ev.evidence.len(), 1);
    assert_eq!(ev.evidence[0].manifest, "pyproject.toml");
    assert_eq!(ev.evidence[0].ecosystem, "python");
    assert_eq!(ev.evidence[0].dependency.as_deref(), Some("stellar-sdk"));
    assert_eq!(
        ev.evidence[0].section.as_deref(),
        Some("tool.poetry.dependencies")
    );
}

#[test]
fn detects_stellar_sdk_in_poetry_group_dependencies() {
    let scratch = Scratch::new("poetry-group");
    scratch.write(
        "pyproject.toml",
        r#"
[tool.poetry]
name = "poetry-grouped"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.10"

[tool.poetry.group.sdk.dependencies]
stellar-sdk = "^9.0.0"
"#,
    );

    let details = detect_details(scratch.path());
    assert_eq!(details.context.project_type, ProjectType::StellarSdk);
    assert!(details
        .context
        .capabilities
        .contains(&Capability::PythonSdk));

    let ev = details
        .capability_evidence
        .iter()
        .find(|c| c.capability == Capability::PythonSdk)
        .expect("PythonSdk capability evidence");
    assert_eq!(ev.evidence[0].dependency.as_deref(), Some("stellar-sdk"));
    assert_eq!(
        ev.evidence[0].section.as_deref(),
        Some("tool.poetry.group.sdk.dependencies")
    );
}

#[test]
fn does_not_trigger_on_unrelated_python_projects() {
    let scratch = Scratch::new("unrelated-python");
    scratch.write(
        "pyproject.toml",
        r#"
[project]
name = "unrelated-web-service"
version = "1.0.0"
dependencies = [
    "fastapi>=0.100.0",
    "uvicorn>=0.23.0",
    "pydantic>=2.0",
]
"#,
    );

    let ctx = detect(scratch.path());
    assert_eq!(ctx.project_type, ProjectType::Unknown);
    assert!(!ctx.capabilities.contains(&Capability::PythonSdk));
    assert!(!ctx.capabilities.contains(&Capability::StellarSdkDependency));

    let details = detect_details(scratch.path());
    assert!(details.inspected.contains(&"pyproject.toml".to_string()));
    assert!(details.capability_evidence.is_empty());
    assert!(details.diagnostics.is_empty());
}

#[test]
fn negative_invalid_toml_caught_as_warning_diagnostic_without_halting() {
    let scratch = Scratch::new("invalid-toml");
    scratch.write(
        "pyproject.toml",
        r#"
[project
name = "broken"
dependencies = [ invalid toml syntax ]]]
"#,
    );

    // Negative testing: invalid TOML must not panic or halt project scanning.
    let ctx = detect(scratch.path());
    assert_eq!(ctx.project_type, ProjectType::Unknown);
    assert!(ctx.capabilities.is_empty());

    let details = detect_details(scratch.path());
    assert!(details.inspected.contains(&"pyproject.toml".to_string()));
    assert_eq!(details.context.project_type, ProjectType::Unknown);

    let unparseable = details
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::ManifestUnparseable);
    assert!(
        unparseable.is_some(),
        "expected DiagnosticCode::ManifestUnparseable warning diagnostic, got: {:?}",
        details.diagnostics
    );

    let diag = unparseable.unwrap();
    assert_eq!(diag.code.as_str(), "manifest-unparseable");
    assert!(
        diag.detail.contains("pyproject.toml"),
        "diagnostic detail should mention pyproject.toml: {}",
        diag.detail
    );
}

#[test]
fn pyproject_manifest_read_and_parse_helpers() {
    let scratch = Scratch::new("helpers");
    scratch.write(
        "pyproject.toml",
        r#"
[project]
name = "app"
dependencies = ["stellar-sdk>=9.0"]
"#,
    );

    let manifest = read_pyproject_manifest(scratch.path()).expect("read_pyproject_manifest");
    assert!(manifest.has_dependency("stellar-sdk"));
    assert!(!manifest.has_dependency("soroban-sdk"));

    let toml_manifest = read_pyproject_toml(scratch.path()).expect("read_pyproject_toml");
    assert!(toml_manifest.has_dependency("stellar-sdk"));

    let deps = parse_pyproject_dependencies(
        r#"
[project]
dependencies = ["stellar-sdk>=9.0"]
"#,
    )
    .expect("parse_pyproject_dependencies");
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].name, "stellar-sdk");
    assert_eq!(deps[0].section, "project.dependencies");

    let parsed = parse_pyproject_manifest(
        r#"
[project]
dependencies = ["stellar-sdk>=9.0"]
"#,
    )
    .expect("parse_pyproject_manifest");
    assert!(parsed.has_dependency("stellar-sdk"));

    // Invalid TOML returns Err / None without panicking
    assert!(parse_pyproject_manifest("invalid toml [[[").is_none());
    assert!(parse_pyproject_dependencies("invalid toml [[[").is_err());
}
