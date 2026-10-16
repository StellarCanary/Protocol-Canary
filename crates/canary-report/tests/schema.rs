//! Conformance tests for the published report schema (CF-01-A / CF-01-B).
//!
//! `docs/schemas/report-v1.schema.json` is the formal JSON Schema (draft
//! 2020-12) for the `--format json` report at `schemaVersion` 1. These
//! tests pin both directions of the contract: real engine reports
//! validate against it, and reports that break CF-01 — a missing required
//! section, an out-of-set status — are rejected.
//!
//! The validator below interprets the schema *as data*, using only the
//! keywords the schema document contains (asserted by a test below), so
//! the checks exercise the published file rather than a parallel
//! re-statement of it. Like the consumers CF-01 addresses, it ignores
//! properties it was not told about: extra fields never invalidate a
//! report.

use serde_json::Value;

use canary_core::{
    CompatibilityResult, GitContext, NetworkName, PolicyDecision, ProjectType, ProtocolVersion,
    ResultSource, Status, Surface,
};
use canary_report::{
    JsonReporter, NetworkSummary, ProjectSummary, ReportInput, RunSummary, SkipSummary,
};

const SCHEMA: &str = include_str!("../../../docs/schemas/report-v1.schema.json");
const CF01: &str = include_str!("../../../docs/contracts/cf-01-report.md");

/// Every keyword the schema document may use. Adding a keyword to the
/// schema without teaching the validator about it fails
/// [`the_schema_uses_only_keywords_the_test_validator_interprets`], which
/// keeps this file an honest — if small — implementation of exactly the
/// subset the schema needs.
const SUPPORTED_KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "title",
    "description",
    "type",
    "enum",
    "const",
    "required",
    "properties",
    "items",
    "minimum",
];

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the published schema is valid JSON")
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Validates `instance` against `schema`, returning `Err` with a
/// path-qualified message for the first violation.
fn validate(instance: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("type") {
        let matches: bool = match expected {
            Value::String(wanted) => type_of(instance) == wanted.as_str(),
            Value::Array(wanted) => wanted.iter().any(|w| w.as_str() == Some(type_of(instance))),
            other => return Err(format!("{path}: schema has an unusable `type`: {other}")),
        };
        if !matches {
            return Err(format!(
                "{path}: expected type {expected}, found {}",
                type_of(instance)
            ));
        }
    }
    if let Some(constant) = schema.get("const") {
        if instance != constant {
            return Err(format!("{path}: expected {constant}, found {instance}"));
        }
    }
    if let Some(values) = schema.get("enum") {
        let allowed = values.as_array().is_some_and(|v| v.contains(instance));
        if !allowed {
            return Err(format!("{path}: {instance} is not one of {values}"));
        }
    }
    if let Some(minimum) = schema.get("minimum") {
        match (instance.as_f64(), minimum.as_f64()) {
            (Some(value), Some(min)) if value >= min => {}
            _ => return Err(format!("{path}: {instance} is below the minimum {minimum}")),
        }
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        if let Some(object) = instance.as_object() {
            for field in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(field) {
                    return Err(format!("{path}: required field `{field}` is missing"));
                }
            }
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        if let Some(object) = instance.as_object() {
            for (name, sub_schema) in properties {
                if let Some(value) = object.get(name) {
                    validate(value, sub_schema, &format!("{path}.{name}"))?;
                }
            }
        }
    }
    if let Some(items) = schema.get("items") {
        if let Some(array) = instance.as_array() {
            for (index, value) in array.iter().enumerate() {
                validate(value, items, &format!("{path}[{index}]"))?;
            }
        }
    }
    Ok(())
}

fn validate_report(json_text: &str) -> Result<(), String> {
    let report: Value = serde_json::from_str(json_text).expect("report is valid JSON");
    validate(&report, &schema(), "$")
}

/// A full-featured report, shaped the way `check` renders one: live and
/// cached results, a skip, observed network identity, Git context and
/// build provenance.
fn rendered_report() -> String {
    let result = CompatibilityResult {
        test_id: "p28-xdr-cap83-empty-tx-set".into(),
        protocol: ProtocolVersion(28),
        surface: Surface::Xdr,
        status: Status::Pass,
        summary: "StellarValue round-tripped byte-for-byte".into(),
        details: None,
        duration_ms: 1,
        fixture_id: Some("p28-xdr-cap83-empty-tx-set".into()),
        source: ResultSource::Live,
    };
    let input = ReportInput {
        tool_version: "0.2.0".into(),
        target_protocol: ProtocolVersion(28),
        project: ProjectSummary {
            name: "example-project".into(),
            project_type: ProjectType::Soroban,
        },
        network: Some(NetworkSummary {
            name: NetworkName::Testnet,
            observed_protocol: Some(ProtocolVersion(28)),
            error: None,
        }),
        results: vec![
            result.clone(),
            CompatibilityResult {
                details: Some("expected 28, observed 29".into()),
                source: ResultSource::Cache,
                ..result
            },
        ],
        skipped: vec![SkipSummary {
            fixture_id: "p29-xdr-1".into(),
            surface: Surface::Xdr,
            reason: "fixture targets protocol 29, this run targets protocol 28".into(),
        }],
        decision: PolicyDecision::Warning,
        git: GitContext {
            commit: Some("68a5f8d52573fa741e095bc321443015f0f21250".into()),
            branch: Some("main".into()),
            is_dirty: Some(false),
        },
        verbose: true,
        run: Some(RunSummary {
            tool_build_commit: Some("68a5f8d52573fa741e095bc321443015f0f21250".into()),
        }),
        // CF-04 evidence lives with the fixture pack, never in the report
        // JSON, so a schema conformance report carries none.
        verification: Vec::new(),
    };
    JsonReporter::render(&input)
}

/// The valid examples embedded in CF-01 itself must validate: the schema
/// and the contract document cannot drift apart.
fn cf01_valid_examples() -> Vec<String> {
    let doc = CF01.replace("\r\n", "\n");
    let marker = "<!-- contract-example: report-v1-valid -->";
    let mut blocks = Vec::new();
    let mut rest = doc.as_str();
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let fence = "```json\n";
        let start = rest
            .find(fence)
            .expect("marker is followed by a json block")
            + fence.len();
        let end = rest[start..].find("```").expect("json block is closed") + start;
        blocks.push(rest[start..end].to_string());
        rest = &rest[end..];
    }
    blocks
}

#[test]
fn the_published_schema_is_draft_2020_12_for_report_version_1() {
    let schema = schema();
    assert_eq!(
        schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
        "the published schema targets draft 2020-12"
    );
    assert!(schema["$id"].is_string(), "the schema is identified");
    assert_eq!(schema["properties"]["schemaVersion"]["const"], 1);

    // The required fields are exactly CF-01 section 3's.
    let required = schema["required"].as_array().expect("required is an array");
    for field in [
        "schemaVersion",
        "toolVersion",
        "targetProtocol",
        "project",
        "status",
        "counts",
        "results",
    ] {
        assert!(
            required.iter().any(|v| v == field),
            "`{field}` must be required by the schema"
        );
    }
    assert_eq!(required.len(), 7, "no optional field may be required");

    // The counts table requires all six members (CF-01 section 3).
    let counts = schema["properties"]["counts"]["required"]
        .as_array()
        .expect("counts.required is an array");
    for member in ["total", "passed", "failed", "warnings", "errors", "skipped"] {
        assert!(
            counts.iter().any(|v| v == member),
            "`counts.{member}` must be required"
        );
    }
}

#[test]
fn the_schema_uses_only_keywords_the_test_validator_interprets() {
    // Walks schema positions only: each value of `properties` and the
    // single `items` are themselves schemas, while names, enums and
    // required lists are data.
    fn walk(node: &Value) {
        let Some(map) = node.as_object() else {
            return;
        };
        for (keyword, value) in map {
            assert!(
                SUPPORTED_KEYWORDS.contains(&keyword.as_str()),
                "the schema uses keyword `{keyword}` this test suite does not interpret; \
                 teach tests/schema.rs about it (and extend the conformance tests)"
            );
            match keyword.as_str() {
                "properties" => value
                    .as_object()
                    .into_iter()
                    .flat_map(|properties| properties.values())
                    .for_each(walk),
                "items" => walk(value),
                _ => {}
            }
        }
    }
    walk(&schema());
}

#[test]
fn a_rendered_engine_report_validates() {
    validate_report(&rendered_report()).expect("a real rendered report must validate");
}

#[test]
fn an_offline_report_without_optional_sections_validates() {
    // The oldest accepted shape: no network (fully offline), no git, no
    // run provenance — every one of them optional under CF-01 section 3.
    let json = r#"{
        "schemaVersion": 1,
        "toolVersion": "0.1.1",
        "targetProtocol": 28,
        "project": { "name": "example-project", "type": "unknown" },
        "status": "fail",
        "counts": { "total": 1, "passed": 0, "failed": 1, "warnings": 0, "errors": 0, "skipped": 0 },
        "results": [
            {
                "testId": "p28-xdr-1",
                "protocol": 28,
                "surface": "xdr",
                "status": "fail",
                "summary": "failed to decode StellarValue",
                "durationMs": 0
            }
        ]
    }"#;
    validate_report(json).expect("the minimal shape must validate");
}

#[test]
fn the_cf01_documented_examples_validate() {
    let examples = cf01_valid_examples();
    assert_eq!(examples.len(), 2, "expected two valid examples in CF-01");
    for example in examples {
        validate_report(&example).unwrap_or_else(|e| panic!("example rejected: {e}\n{example}"));
    }
}

#[test]
fn a_fixture_id_may_be_null_or_absent() {
    let mut report: Value =
        serde_json::from_str(&rendered_report()).expect("rendered report parses");
    report["results"][0]["fixtureId"] = Value::Null;
    validate(&report, &schema(), "$").expect("a null fixtureId is allowed");

    let mut report: Value =
        serde_json::from_str(&rendered_report()).expect("rendered report parses");
    report["results"][0]
        .as_object_mut()
        .expect("result is an object")
        .remove("fixtureId");
    validate(&report, &schema(), "$").expect("an absent fixtureId is allowed");
}

/// The negative direction, first case: a report missing the summary
/// counts table is invalid, however small its other problems are.
#[test]
fn a_report_missing_the_summary_counts_is_rejected() {
    let mut report: Value =
        serde_json::from_str(&rendered_report()).expect("rendered report parses");
    report.as_object_mut().expect("report").remove("counts");

    let err = validate(&report, &schema(), "$").expect_err("`counts` is required");
    assert!(err.contains("counts"), "{err}");
}

#[test]
fn an_overall_status_outside_the_closed_set_is_rejected() {
    let mut report: Value =
        serde_json::from_str(&rendered_report()).expect("rendered report parses");
    report["status"] = Value::String("inconclusive".into());

    let err = validate(&report, &schema(), "$").expect_err("`status` is a closed set");
    assert!(err.contains("$.status"), "{err}");
    assert!(err.contains("inconclusive"), "{err}");
}

#[test]
fn an_invalid_result_surface_is_rejected() {
    let mut report: Value =
        serde_json::from_str(&rendered_report()).expect("rendered report parses");
    report["results"][0]["surface"] = Value::String("wire".into());

    let err = validate(&report, &schema(), "$").expect_err("`surface` is a closed set");
    assert!(err.contains("$.results[0].surface"), "{err}");
}

#[test]
fn a_result_missing_a_required_field_is_rejected() {
    let mut report: Value =
        serde_json::from_str(&rendered_report()).expect("rendered report parses");
    report["results"][0]
        .as_object_mut()
        .expect("result is an object")
        .remove("testId");

    let err = validate(&report, &schema(), "$").expect_err("`testId` is required per result");
    assert!(err.contains("testId"), "{err}");
}

#[test]
fn a_schema_version_other_than_one_is_rejected() {
    let mut report: Value =
        serde_json::from_str(&rendered_report()).expect("rendered report parses");
    report["schemaVersion"] = 2.into();

    let err = validate(&report, &schema(), "$").expect_err("the v1 schema pins version 1");
    assert!(err.contains("schemaVersion"), "{err}");
}

#[test]
fn unknown_additional_fields_are_tolerated_everywhere() {
    // CF-01 section 2: schemaVersion 1 only grows additively, so a report
    // carrying fields this version has never heard of still validates.
    let mut report: Value =
        serde_json::from_str(&rendered_report()).expect("rendered report parses");
    report["futureSection"] = serde_json::json!({ "anything": [1, 2, 3] });
    report["results"][0]["futureResultField"] = Value::Bool(true);
    report["properties"] = serde_json::json!(["nested", "unknown", "namespaces"]);
    validate(&report, &schema(), "$").expect("unknown fields must not invalidate a report");
}
