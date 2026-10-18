//! Tests verifying the end-to-end integration test workflow definition in
//! `.github/workflows/e2e-release.yml` (#468).
//!
//! Asserts that:
//! 1. The workflow file exists and defines a valid GitHub Actions workflow.
//! 2. It runs on a clean `ubuntu-latest` runner without a local cargo build.
//! 3. It downloads the pinned official release binary (`v0.2.0`) and verifies its SHA-256 checksum.
//! 4. It downloads the official tagged fixture pack (`protocol-28`).
//! 5. It executes `stellar-canary check --fixtures-dir .` and asserts exit code 0 with valid report JSON.
//! 6. It includes a negative testing step where a simulated execution error triggers a clear CI failure step.
//! 7. Negative test assertions trigger clear errors when invalid configurations or reports are simulated.

use serde_json::Value;

const WORKFLOW_STR: &str = include_str!("../../../.github/workflows/e2e-release.yml");

#[derive(Debug, PartialEq, Eq)]
pub struct WorkflowCheckSummary {
    pub name: String,
    pub has_push: bool,
    pub has_pull_request: bool,
    pub has_workflow_dispatch: bool,
    pub runner: String,
    pub engine_version: String,
    pub fixtures_tag: String,
    pub expected_sha256: String,
    pub no_local_cargo_build: bool,
    pub verifies_checksum: bool,
    pub executes_fixtures_dir_dot: bool,
    pub generates_report_json: bool,
    pub asserts_exit_code_zero: bool,
    pub has_negative_test_step: bool,
}

/// Parses and validates the workflow content.
pub fn validate_workflow_definition(content: &str) -> Result<WorkflowCheckSummary, String> {
    let lines: Vec<&str> = content.lines().collect();

    let mut name = String::new();
    let mut has_push = false;
    let mut has_pull_request = false;
    let mut has_workflow_dispatch = false;
    let mut runner = String::new();
    let mut engine_version = String::new();
    let mut fixtures_tag = String::new();
    let mut expected_sha256 = String::new();

    for line in &lines {
        let trimmed = line.trim();
        if let Some(stripped) = trimmed.strip_prefix("name:") {
            if name.is_empty() {
                name = stripped.trim().to_string();
            }
        } else if let Some(stripped) = trimmed.strip_prefix("runs-on:") {
            if runner.is_empty() {
                runner = stripped.trim().to_string();
            }
        } else if let Some(stripped) = trimmed.strip_prefix("ENGINE_VERSION:") {
            engine_version = stripped.trim().trim_matches('"').to_string();
        } else if let Some(stripped) = trimmed.strip_prefix("FIXTURES_TAG:") {
            fixtures_tag = stripped.trim().trim_matches('"').to_string();
        } else if let Some(stripped) = trimmed.strip_prefix("EXPECTED_SHA256:") {
            expected_sha256 = stripped.trim().trim_matches('"').to_string();
        } else if trimmed == "push:" || trimmed.starts_with("push:") {
            has_push = true;
        } else if trimmed == "pull_request:" || trimmed.starts_with("pull_request:") {
            has_pull_request = true;
        } else if trimmed == "workflow_dispatch:" || trimmed.starts_with("workflow_dispatch:") {
            has_workflow_dispatch = true;
        }
    }

    if name.is_empty() {
        return Err("workflow missing 'name' attribute".to_string());
    }
    if !has_push {
        return Err("workflow missing 'push' trigger".to_string());
    }
    if !has_pull_request {
        return Err("workflow missing 'pull_request' trigger".to_string());
    }
    if !has_workflow_dispatch {
        return Err("workflow missing 'workflow_dispatch' trigger".to_string());
    }
    if runner != "ubuntu-latest" {
        return Err(format!(
            "unexpected runner: {runner}, expected ubuntu-latest"
        ));
    }
    if engine_version.is_empty() {
        return Err("missing ENGINE_VERSION in workflow environment".to_string());
    }
    if fixtures_tag.is_empty() {
        return Err("missing FIXTURES_TAG in workflow environment".to_string());
    }
    if expected_sha256.len() != 64 {
        return Err(format!(
            "invalid EXPECTED_SHA256 digest length: {}",
            expected_sha256.len()
        ));
    }

    // Clean runner requirement: must not build cargo locally
    let has_cargo_build = lines
        .iter()
        .any(|l| l.contains("cargo build") || l.contains("cargo install") || l.contains("rustup"));
    if has_cargo_build {
        return Err(
            "workflow violates clean runner requirement by invoking cargo build/install"
                .to_string(),
        );
    }

    // Checksum verification
    let verifies_checksum = content.contains("sha256sum --check")
        && content.contains("SHA256SUMS")
        && content.contains("${EXPECTED_SHA256}");
    if !verifies_checksum {
        return Err("workflow missing SHA256 checksum verification".to_string());
    }

    // Command execution
    let executes_fixtures_dir_dot = content.contains("stellar-canary check --fixtures-dir .");
    if !executes_fixtures_dir_dot {
        return Err(
            "workflow does not execute 'stellar-canary check --fixtures-dir .'".to_string(),
        );
    }

    let generates_report_json = content.contains("--json") && content.contains("report.json");
    if !generates_report_json {
        return Err("workflow does not generate report.json".to_string());
    }

    let asserts_exit_code_zero = content
        .contains("Assert exit code 0 and valid report JSON generation")
        && (content.contains("schemaVersion") || content.contains("schema_version"))
        && content.contains("counts");
    if !asserts_exit_code_zero {
        return Err("workflow missing exit code 0 or report validity assertion".to_string());
    }

    // Negative testing step
    let has_negative_test_step = content.contains("Negative test")
        && content.contains("protocol 999")
        && content.contains("no checks ran")
        && content.contains("EXIT_CODE");
    if !has_negative_test_step {
        return Err("workflow missing simulated execution error negative test step".to_string());
    }

    Ok(WorkflowCheckSummary {
        name,
        has_push,
        has_pull_request,
        has_workflow_dispatch,
        runner,
        engine_version,
        fixtures_tag,
        expected_sha256,
        no_local_cargo_build: !has_cargo_build,
        verifies_checksum,
        executes_fixtures_dir_dot,
        generates_report_json,
        asserts_exit_code_zero,
        has_negative_test_step,
    })
}

/// Validates report JSON content matching the workflow's verification script.
pub fn validate_report_json(json_str: &str) -> Result<(), String> {
    let report: Value =
        serde_json::from_str(json_str).map_err(|e| format!("failed to parse report JSON: {e}"))?;

    let schema_version = report
        .get("schemaVersion")
        .or_else(|| report.get("schema_version"))
        .and_then(Value::as_u64)
        .ok_or_else(|| "missing or invalid 'schemaVersion'".to_string())?;
    if schema_version != 1 {
        return Err(format!("unexpected schema_version: {schema_version}"));
    }

    let status = report
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing or invalid 'status'".to_string())?;
    if status != "pass" {
        return Err(format!("expected 'pass' status, got: {status}"));
    }

    let counts = report
        .get("counts")
        .and_then(Value::as_object)
        .ok_or_else(|| "missing 'counts' object".to_string())?;

    let failed = counts
        .get("failed")
        .and_then(Value::as_u64)
        .ok_or_else(|| "missing 'counts.failed'".to_string())?;
    if failed != 0 {
        return Err(format!("expected counts.failed == 0, got {failed}"));
    }

    let errors = counts
        .get("errors")
        .and_then(Value::as_u64)
        .ok_or_else(|| "missing 'counts.errors'".to_string())?;
    if errors != 0 {
        return Err(format!("expected counts.errors == 0, got {errors}"));
    }

    let total = counts
        .get("total")
        .and_then(Value::as_u64)
        .ok_or_else(|| "missing 'counts.total'".to_string())?;
    if total == 0 {
        return Err("expected counts.total > 0".to_string());
    }

    let passed = counts
        .get("passed")
        .and_then(Value::as_u64)
        .ok_or_else(|| "missing 'counts.passed'".to_string())?;
    if passed == 0 {
        return Err("expected counts.passed > 0".to_string());
    }

    let results = report
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing 'results' array".to_string())?;
    if results.len() as u64 != total {
        return Err(format!(
            "results array length ({}) does not match total count ({total})",
            results.len()
        ));
    }

    for (i, r) in results.iter().enumerate() {
        let r_status = r.get("status").and_then(Value::as_str).unwrap_or("");
        if r_status != "pass" {
            return Err(format!(
                "result index {i} has status {r_status:?}, expected 'pass'"
            ));
        }
        let r_surface = r.get("surface").and_then(Value::as_str).unwrap_or("");
        if r_surface != "xdr" {
            return Err(format!(
                "result index {i} has surface {r_surface:?}, expected 'xdr'"
            ));
        }
    }

    Ok(())
}

#[test]
fn e2e_release_workflow_definition_is_valid() {
    let summary = validate_workflow_definition(WORKFLOW_STR)
        .expect("e2e-release.yml must be a valid workflow definition");

    assert_eq!(summary.name, "E2E Release");
    assert!(summary.has_push);
    assert!(summary.has_pull_request);
    assert!(summary.has_workflow_dispatch);
    assert_eq!(summary.runner, "ubuntu-latest");
    assert_eq!(summary.engine_version, "v0.2.0");
    assert_eq!(summary.fixtures_tag, "protocol-28");
    assert_eq!(
        summary.expected_sha256,
        "34656189e15169a9f9d339288f7a8a95bb81d86ecee14ca278a165d041d28943"
    );
    assert!(summary.no_local_cargo_build, "must not build cargo locally");
    assert!(summary.verifies_checksum, "must verify sha256 checksums");
    assert!(
        summary.executes_fixtures_dir_dot,
        "must execute stellar-canary check --fixtures-dir ."
    );
    assert!(summary.generates_report_json, "must generate report.json");
    assert!(
        summary.asserts_exit_code_zero,
        "must assert exit code 0 and valid report JSON"
    );
    assert!(
        summary.has_negative_test_step,
        "must contain negative test step"
    );
}

#[test]
fn negative_testing_workflow_definition_rejects_invalid_workflows() {
    // 1. Rejected if cargo build is introduced
    let invalid_cargo_build =
        WORKFLOW_STR.replace("stellar-canary version", "cargo build --release");
    let err = validate_workflow_definition(&invalid_cargo_build).unwrap_err();
    assert!(
        err.contains("clean runner requirement"),
        "expected error on cargo build: {err}"
    );

    // 2. Rejected if checksum verification is removed
    let missing_checksum = WORKFLOW_STR.replace("sha256sum --check", "echo skipping");
    let err = validate_workflow_definition(&missing_checksum).unwrap_err();
    assert!(
        err.contains("checksum verification"),
        "expected error on missing checksum: {err}"
    );

    // 3. Rejected if execution command is missing
    let missing_command = WORKFLOW_STR.replace(
        "stellar-canary check --fixtures-dir .",
        "stellar-canary inspect",
    );
    let err = validate_workflow_definition(&missing_command).unwrap_err();
    assert!(
        err.contains("stellar-canary check --fixtures-dir ."),
        "expected error on missing command: {err}"
    );

    // 4. Rejected if negative test step is missing
    let missing_negative = WORKFLOW_STR.replace("Negative test", "Ignored");
    let err = validate_workflow_definition(&missing_negative).unwrap_err();
    assert!(
        err.contains("simulated execution error negative test step"),
        "expected error on missing negative step: {err}"
    );
}

#[test]
fn negative_testing_report_validator_rejects_simulated_execution_failures() {
    let valid_report = r#"{
        "schema_version": 1,
        "status": "pass",
        "counts": {
            "total": 3,
            "passed": 3,
            "failed": 0,
            "errors": 0,
            "skipped": 0
        },
        "results": [
            { "id": "p28-xdr-1", "surface": "xdr", "status": "pass" },
            { "id": "p28-xdr-2", "surface": "xdr", "status": "pass" },
            { "id": "p28-xdr-3", "surface": "xdr", "status": "pass" }
        ]
    }"#;
    assert!(validate_report_json(valid_report).is_ok());

    // 1. Status failure
    let failed_status_report = valid_report.replace(r#""status": "pass""#, r#""status": "fail""#);
    let err = validate_report_json(&failed_status_report).unwrap_err();
    assert!(
        err.contains("expected 'pass' status"),
        "expected status error: {err}"
    );

    // 2. Failed counts > 0
    let failed_counts_report = valid_report.replace(r#""failed": 0"#, r#""failed": 1"#);
    let err = validate_report_json(&failed_counts_report).unwrap_err();
    assert!(
        err.contains("counts.failed == 0"),
        "expected failed count error: {err}"
    );

    // 3. Execution error count > 0
    let error_counts_report = valid_report.replace(r#""errors": 0"#, r#""errors": 1"#);
    let err = validate_report_json(&error_counts_report).unwrap_err();
    assert!(
        err.contains("counts.errors == 0"),
        "expected errors count error: {err}"
    );

    // 4. Empty run (total == 0)
    let empty_report = valid_report
        .replace(r#""total": 3"#, r#""total": 0"#)
        .replace(r#""passed": 3"#, r#""passed": 0"#);
    let err = validate_report_json(&empty_report).unwrap_err();
    assert!(
        err.contains("counts.total > 0"),
        "expected total error: {err}"
    );

    // 5. Schema version mismatch
    let bad_schema_report =
        valid_report.replace(r#""schema_version": 1"#, r#""schema_version": 2"#);
    let err = validate_report_json(&bad_schema_report).unwrap_err();
    assert!(
        err.contains("unexpected schema_version"),
        "expected schema error: {err}"
    );

    // 6. Malformed JSON
    let err = validate_report_json("not valid json").unwrap_err();
    assert!(
        err.contains("failed to parse report JSON"),
        "expected parse error: {err}"
    );
}
