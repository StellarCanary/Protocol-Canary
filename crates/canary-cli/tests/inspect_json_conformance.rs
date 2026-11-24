//! CA-03 schema and conformance test for `stellar-canary inspect --format json` (issue #484, [E-041]).
//!
//! Validates that:
//! - Output from `stellar-canary inspect --format json` strictly conforms to `docs/schemas/inspect-v1.schema.json`.
//! - Emitted JSON matches the golden reference file byte-for-byte.
//! - Negative testing: invalid schema structures in inspect output trigger test failure.

mod support;

use support::{run_in, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use serde_json::Value;

const SCHEMA_TEXT: &str = include_str!("../../../docs/schemas/inspect-v1.schema.json");
const GOLDEN_TEXT: &str = include_str!("golden/inspect-output.json");

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn schema() -> Value {
    serde_json::from_str(SCHEMA_TEXT).expect("published inspect-v1 schema is valid JSON")
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

/// Validates `instance` against `schema`, returning `Err` with a path-qualified message for the first violation.
fn validate(instance: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("type") {
        let matches = match expected {
            Value::String(wanted) => type_of(instance) == wanted.as_str(),
            Value::Array(wanted) => wanted.iter().any(|w| w.as_str() == Some(type_of(instance))),
            other => return Err(format!("{path}: schema has unusable type: {other}")),
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
            return Err(format!(
                "{path}: expected const {constant}, found {instance}"
            ));
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
            (Some(val), Some(min)) if val >= min => {}
            _ => return Err(format!("{path}: {instance} is below minimum {minimum}")),
        }
    }

    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
        if let Some(s) = instance.as_str() {
            if pattern == "^sha256:[0-9a-f]{64}$" {
                let ok = s.starts_with("sha256:")
                    && s.len() == 7 + 64
                    && s[7..]
                        .chars()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
                if !ok {
                    return Err(format!("{path}: `{s}` does not match pattern {pattern}"));
                }
            }
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

    if let Some(additional) = schema.get("additionalProperties") {
        if let Some(object) = instance.as_object() {
            let properties = schema.get("properties").and_then(Value::as_object);
            for (key, val) in object {
                if properties.is_none_or(|p| !p.contains_key(key)) {
                    if let Value::Bool(allowed) = additional {
                        if !allowed {
                            return Err(format!("{path}.{key}: unexpected property"));
                        }
                    } else {
                        validate(val, additional, &format!("{path}.{key}"))?;
                    }
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

fn validate_document(doc_json: &str) -> Result<(), String> {
    let value: Value = serde_json::from_str(doc_json).map_err(|e| e.to_string())?;
    validate(&value, &schema(), "$")
}

fn create_conformance_project(name: &str) -> TempProject {
    let dir = TempProject::new(name);
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "Cargo.toml",
        "[package]\nname = \"basic-project\"\nversion = \"0.1.0\"\n",
    );
    dir.write(
        "fixtures/xdr/p28-xdr-1.toml",
        &format!(
            "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir.write(
        "fixtures/xdr/p27-xdr-old.toml",
        &format!(
            "id = \"p27-xdr-old\"\nprotocol = 27\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir
}

#[test]
fn inspect_format_json_conforms_to_schema_and_matches_golden_file() {
    let dir = create_conformance_project("inspect-json-conformance");
    let output = run_in(
        &dir.path,
        &["inspect", "--format", "json", "--fixtures-dir", "fixtures"],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        support::stderr(&output)
    );

    let emitted = stdout(&output);

    // Validate emitted output against the formal JSON schema
    validate_document(&emitted).expect("inspect output must conform to inspect-v1 schema");

    // Assert byte-for-byte match with committed golden file
    let normalized_emitted = emitted.replace("\r\n", "\n");
    let normalized_golden = GOLDEN_TEXT.replace("\r\n", "\n");
    assert_eq!(
        normalized_emitted, normalized_golden,
        "emitted JSON must match golden reference byte-for-byte"
    );
}

#[test]
fn invalid_schema_structures_in_inspect_output_trigger_test_failure() {
    let valid: Value = serde_json::from_str(GOLDEN_TEXT).unwrap();

    // 1. Missing top-level required field: plan
    let mut bad = valid.clone();
    bad.as_object_mut().unwrap().remove("plan");
    let err = validate(&bad, &schema(), "$").unwrap_err();
    assert!(
        err.contains("required field `plan` is missing"),
        "err: {err}"
    );

    // 2. Missing top-level required field: fingerprint
    let mut bad = valid.clone();
    bad.as_object_mut().unwrap().remove("fingerprint");
    let err = validate(&bad, &schema(), "$").unwrap_err();
    assert!(
        err.contains("required field `fingerprint` is missing"),
        "err: {err}"
    );

    // 3. Invalid inspectVersion constant
    let mut bad = valid.clone();
    bad["inspectVersion"] = Value::from(2);
    let err = validate(&bad, &schema(), "$").unwrap_err();
    assert!(err.contains("expected const 1"), "err: {err}");

    // 4. offline must be const true
    let mut bad = valid.clone();
    bad["offline"] = Value::Bool(false);
    let err = validate(&bad, &schema(), "$").unwrap_err();
    assert!(err.contains("expected const true"), "err: {err}");

    // 5. Invalid project.type enum value
    let mut bad = valid.clone();
    bad["project"]["type"] = Value::String("invalid-type".into());
    let err = validate(&bad, &schema(), "$").unwrap_err();
    assert!(err.contains("is not one of"), "err: {err}");

    // 6. Missing required field in plan.wouldRun
    let mut bad = valid.clone();
    bad["plan"]["wouldRun"][0]
        .as_object_mut()
        .unwrap()
        .remove("surface");
    let err = validate(&bad, &schema(), "$").unwrap_err();
    assert!(
        err.contains("required field `surface` is missing"),
        "err: {err}"
    );

    // 7. Invalid fingerprint digest pattern
    let mut bad = valid.clone();
    bad["fingerprint"]["digest"] = Value::String("not-a-valid-sha256".into());
    let err = validate(&bad, &schema(), "$").unwrap_err();
    assert!(err.contains("does not match pattern"), "err: {err}");

    // 8. Missing required field in config
    let mut bad = valid.clone();
    bad["config"].as_object_mut().unwrap().remove("protocol");
    let err = validate(&bad, &schema(), "$").unwrap_err();
    assert!(
        err.contains("required field `protocol` is missing"),
        "err: {err}"
    );
}
