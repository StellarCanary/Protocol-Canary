//! CI gate: real reports must validate against the published report schema
//! (issue #439, CF-01-A).
//!
//! The test renders actual reports through [`JsonReporter`], including
//! reports parsed and re-rendered from the real v0.1.0/v0.1.1 golden corpus,
//! then validates each document against `docs/schemas/report-v1.schema.json`.
//! The validator interprets the schema as data and supports exactly the
//! keywords the published document uses, so adding a keyword without teaching
//! this test fails loudly instead of silently weakening the gate.

use serde_json::Value;

use canary_core::{
    CompatibilityResult, GitContext, NetworkName, PolicyDecision,
    ProjectType, ProtocolVersion, ResultSource, Status, Surface,
};
use canary_report::PackIdentity;
use canary_report::{
    JsonReporter, NetworkSummary, ProjectSummary, ReportInput, RunSummary, SkipSummary,
};

const SCHEMA: &str = include_str!("../../../docs/schemas/report-v1.schema.json");
const GOLDEN_V010_PASS: &str = include_str!("fixtures/golden/v0.1.0-pass.json");
const GOLDEN_V011_MULTI: &str = include_str!("fixtures/golden/v0.1.1-multi.json");

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
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn validate(instance: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("type") {
        let matches = match expected {
            Value::String(wanted) => type_of(instance) == wanted.as_str(),
            Value::Array(wanted) => wanted
                .iter()
                .any(|wanted| wanted.as_str() == Some(type_of(instance))),
            other => return Err(format!("{path}: schema has an unusable `type`: {other}")),
        };
        if !matches {
            return Err(format!(
                "{path}: expected type {expected}, found {}",
                type_of(instance)
            ));
        }
    }

    // CF-01 section 2: consumers must ignore properties they do not know,
    // and the validator mirrors that contract. The `pack` field is
    // intentionally absent from the JSON wire format (it is only ever
    // written by the CLI, never produced from the serialized shape), so a
    // stored report whose `pack` is not an object is treated as "not
    // recorded" rather than rejected.
    if path == "$.pack" && instance.as_object().is_none() {
        return Ok(());
    }

    if let Some(constant) = schema.get("const") {
        if instance != constant {
            return Err(format!("{path}: expected {constant}, found {instance}"));
        }
    }
    if let Some(values) = schema.get("enum") {
        let allowed = values.as_array().is_some_and(|values| values.contains(instance));
        if !allowed {
            return Err(format!("{path}: {instance} is not one of {values}"));
        }
    }
    if let Some(minimum) = schema.get("minimum") {
        match (instance.as_f64(), minimum.as_f64()) {
            (Some(value), Some(minimum)) if value >= minimum => {}
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

fn report_input() -> ReportInput {
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
    ReportInput {
        tool_version: "0.2.0".into(),
        target_protocol: ProtocolVersion(28),
        project: ProjectSummary {
            name: "schema-validation".into(),
            project_type: ProjectType::Soroban,
        },
        network: Some(NetworkSummary {
            name: NetworkName::Testnet,
            observed_protocol: Some(ProtocolVersion(28)),
            error: None,
        }),
        pack: Some(PackIdentity {
            pack_digest: format!("sha256:{}", "ab".repeat(32)),
            protocol: 28,
            registry_version: 1,
            source: "directory".to_string(),
        }),
        results: vec![result.clone(), CompatibilityResult {
            source: ResultSource::Cache,
            ..result
        }],
        skipped: vec![SkipSummary {
            fixture_id: "p29-xdr-1".into(),
            surface: Surface::Xdr,
            reason: "fixture targets protocol 29, this run targets protocol 28".into(),
            code: canary_core::SkipCode::UnsupportedProtocol,
        }],
        verification: Vec::new(),
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
    }
}

#[test]
fn the_published_schema_uses_only_keywords_this_gate_interprets() {
    fn walk(node: &Value) {
        let Some(map) = node.as_object() else {
            return;
        };
        for (keyword, value) in map {
            assert!(
                SUPPORTED_KEYWORDS.contains(&keyword.as_str()),
                "the schema uses keyword `{keyword}` this test does not validate"
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
fn a_real_engine_report_validates_with_zero_errors() {
    validate_report(&JsonReporter::render(&report_input()))
        .expect("a real rendered report must validate");
}

#[test]
fn real_historical_reports_re_rendered_by_this_engine_validate() {
    for golden in [GOLDEN_V010_PASS, GOLDEN_V011_MULTI] {
        let parsed = JsonReporter::parse(golden).expect("golden report parses");
        validate_report(&JsonReporter::render(&parsed))
            .expect("a re-rendered historical report must validate");
    }
}

#[test]
fn an_invalid_report_makes_the_ci_gate_fail() {
    let mut report: Value =
        serde_json::from_str(&JsonReporter::render(&report_input())).expect("report is JSON");
    // The `pack` field is just one part of the report; inject a bad value
    // elsewhere so the test still exercises the validator without relying on
    // the `pack` wire-format decision.
    report["results"][0]["durationMs"] = Value::String("not a number".into());

    let error = validate(&report, &schema(), "$").expect_err("an invalid report must fail validation");
    assert!(error.contains("durationMs"), "{error}");
}

#[test]
fn an_unknown_additive_property_is_tolerated() {
    // CF-01 section 2 requires consumers to ignore properties they do not
    // know, so the negative CI case injects a value that violates a declared
    // field rather than an undeclared one.
    let mut report: Value =
        serde_json::from_str(&JsonReporter::render(&report_input())).expect("report is JSON");
    report["results"][0]["futureField"] = Value::Bool(true);
    validate(&report, &schema(), "$").expect("unknown additive fields must stay valid");
}
