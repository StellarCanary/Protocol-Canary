//! Integration tests for Python SDK detection in requirements.txt (CF-07, E-003, issue #408).
//!
//! Validates:
//! - Detection of `stellar-sdk` dependency across `requirements.txt` and `requirements/*.txt`.
//! - Version operators (`==`, `>=`, `<=`, `~=`, `!=`, etc.).
//! - Case insensitivity and normalization (`stellar_sdk` vs `stellar-sdk`).
//! - Extras and environment markers.
//! - Ignoring comments (`#`) and pip options (`-r`, `-i`, `--extra-index-url`, etc.).
//! - Negative testing: commented-out package lines do not trigger detection.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use canary_core::{Capability, ProjectType};
use canary_project::{
    detect, detect_details, parse_requirements_dependencies, parse_requirements_line,
    parse_requirements_manifest, parse_requirements_txt, read_requirements_manifest,
    read_requirements_txt,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "canary-req-{name}-{}-{}",
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
fn detects_stellar_sdk_in_root_requirements_txt() {
    let scratch = Scratch::new("root-req");
    scratch.write("requirements.txt", "stellar-sdk==9.0.0\nrequests>=2.28.0\n");

    let ctx = detect(scratch.path());
    assert_eq!(ctx.project_type, ProjectType::StellarSdk);
    assert!(ctx.capabilities.contains(&Capability::PythonSdk));
    assert!(ctx.capabilities.contains(&Capability::StellarSdkDependency));

    let details = detect_details(scratch.path());
    assert!(details.inspected.contains(&"requirements.txt".to_string()));

    let ev = details
        .capability_evidence
        .iter()
        .find(|c| c.capability == Capability::PythonSdk)
        .expect("PythonSdk evidence");
    assert_eq!(ev.evidence.len(), 1);
    assert_eq!(ev.evidence[0].manifest, "requirements.txt");
    assert_eq!(ev.evidence[0].ecosystem, "python");
    assert_eq!(ev.evidence[0].dependency.as_deref(), Some("stellar-sdk"));
    assert_eq!(ev.evidence[0].section.as_deref(), Some("dependencies"));
}

#[test]
fn detects_stellar_sdk_with_various_operators() {
    let scratch = Scratch::new("operators");
    scratch.write("requirements.txt", "stellar-sdk~=9.0.0\n");
    let ctx = detect(scratch.path());
    assert!(ctx.capabilities.contains(&Capability::PythonSdk));

    scratch.write("requirements.txt", "stellar-sdk>=9.0.0,<10.0.0\n");
    let ctx = detect(scratch.path());
    assert!(ctx.capabilities.contains(&Capability::PythonSdk));

    scratch.write("requirements.txt", "stellar-sdk<=9.9.9\n");
    let ctx = detect(scratch.path());
    assert!(ctx.capabilities.contains(&Capability::PythonSdk));
}

#[test]
fn detects_stellar_sdk_with_extras_and_environment_markers() {
    let scratch = Scratch::new("extras-markers");
    scratch.write(
        "requirements.txt",
        "stellar-sdk[aiohttp]>=9.0.0; python_version >= '3.10'\n",
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
}

#[test]
fn normalizes_underscores_and_casing() {
    let scratch = Scratch::new("casing-norm");
    scratch.write("requirements.txt", "Stellar_SDK==9.0.0\n");

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
}

#[test]
fn detects_stellar_sdk_in_requirements_dir() {
    let scratch = Scratch::new("req-dir");
    scratch.write(
        "requirements/prod.txt",
        "stellar-sdk==9.0.0\nrequests>=2.28\n",
    );

    let ctx = detect(scratch.path());
    assert_eq!(ctx.project_type, ProjectType::StellarSdk);
    assert!(ctx.capabilities.contains(&Capability::PythonSdk));

    let details = detect_details(scratch.path());
    assert!(details
        .inspected
        .contains(&"requirements/prod.txt".to_string()));

    let ev = details
        .capability_evidence
        .iter()
        .find(|c| c.capability == Capability::PythonSdk)
        .expect("PythonSdk evidence");
    assert_eq!(ev.evidence.len(), 1);
    assert_eq!(ev.evidence[0].manifest, "requirements/prod.txt");
    assert_eq!(ev.evidence[0].ecosystem, "python");
    assert_eq!(ev.evidence[0].dependency.as_deref(), Some("stellar-sdk"));
}

#[test]
fn ignores_comment_lines_and_pip_options() {
    let scratch = Scratch::new("comments-pip-options");
    scratch.write(
        "requirements.txt",
        r#"
# Primary stellar packages
-r base.txt
-i https://pypi.org/simple
--extra-index-url https://custom.pypi.org/simple
stellar-sdk==9.0.0 # for horizon integration
requests>=2.28.0
-c constraints.txt
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
}

#[test]
fn negative_commented_out_package_does_not_trigger_detection() {
    let scratch = Scratch::new("negative-commented");
    scratch.write(
        "requirements.txt",
        r#"
# stellar-sdk==9.0.0
# stellar_sdk
#   stellar-sdk
fastapi>=0.100.0
"#,
    );

    let ctx = detect(scratch.path());
    assert_eq!(ctx.project_type, ProjectType::Unknown);
    assert!(!ctx.capabilities.contains(&Capability::PythonSdk));
    assert!(!ctx.capabilities.contains(&Capability::StellarSdkDependency));

    let details = detect_details(scratch.path());
    assert!(details.inspected.contains(&"requirements.txt".to_string()));
    assert!(details.capability_evidence.is_empty());
}

#[test]
fn unrelated_python_project_has_no_stellar_capability() {
    let scratch = Scratch::new("unrelated");
    scratch.write(
        "requirements.txt",
        "fastapi>=0.100.0\nuvicorn>=0.23.0\npydantic>=2.0\n",
    );

    let ctx = detect(scratch.path());
    assert_eq!(ctx.project_type, ProjectType::Unknown);
    assert!(!ctx.capabilities.contains(&Capability::PythonSdk));
    assert!(!ctx.capabilities.contains(&Capability::StellarSdkDependency));

    let details = detect_details(scratch.path());
    assert!(details.inspected.contains(&"requirements.txt".to_string()));
    assert!(details.capability_evidence.is_empty());
}

#[test]
fn requirements_helper_functions_parse_cleanly() {
    assert_eq!(
        parse_requirements_line("stellar-sdk==9.0.0").as_deref(),
        Some("stellar-sdk")
    );
    assert_eq!(
        parse_requirements_line("stellar_sdk>=9.0.0").as_deref(),
        Some("stellar-sdk")
    );
    assert_eq!(
        parse_requirements_line("  # commented out").as_deref(),
        None
    );
    assert_eq!(
        parse_requirements_line("-r requirements/base.txt").as_deref(),
        None
    );

    let deps = parse_requirements_txt("stellar-sdk==9.0.0\nrequests>=2.28\n");
    assert_eq!(deps, vec!["requests", "stellar-sdk"]);

    let manifest = parse_requirements_manifest("stellar-sdk==9.0.0\n");
    assert!(manifest.has_dependency("stellar-sdk"));

    let dep_entries = parse_requirements_dependencies("stellar-sdk==9.0.0\n");
    assert_eq!(dep_entries.len(), 1);
    assert_eq!(dep_entries[0].name, "stellar-sdk");
    assert_eq!(dep_entries[0].section, "dependencies");

    let scratch = Scratch::new("helpers");
    scratch.write("requirements.txt", "stellar-sdk==9.0.0\n");
    let read_m = read_requirements_txt(scratch.path()).expect("read_requirements_txt");
    assert!(read_m.has_dependency("stellar-sdk"));

    let read_m2 = read_requirements_manifest(&scratch.path().join("requirements.txt"))
        .expect("read_requirements_manifest");
    assert!(read_m2.has_dependency("stellar-sdk"));
}
