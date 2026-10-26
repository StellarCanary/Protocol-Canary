//! CA-02 schema and conformance test for `stellar-canary fixtures --format json` (issue #483, [E-037]).
//!
//! Validates that:
//! - Output from `stellar-canary fixtures --format json` strictly conforms to `docs/schemas/fixtures-v1.schema.json`.
//! - Emitted JSON matches the golden reference file byte-for-byte.
//! - Fixtures are deterministically ordered by `id` (UTF-8 bytes).
//! - Negative testing: missing required metadata or invalid fields trigger schema conformance failures.

mod support;

use support::{run_in, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

use serde_json::Value;

const SCHEMA_TEXT: &str = include_str!("../../../docs/schemas/fixtures-v1.schema.json");
const GOLDEN_TEXT: &str = include_str!("golden/fixtures-output.json");

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn schema() -> Value {
    serde_json::from_str(SCHEMA_TEXT).expect("published fixtures-v1 schema is valid JSON")
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
        "fixtures/soroban/contract.toml",
        r#"
id = "z-soroban-token"
protocol = 28
surface = "soroban"
category = "token"
description = "token contract fixture"
required_capabilities = ["wasm-artifact", "soroban-contract"]
function = "balance"
"#,
    );
    dir.write(
        "fixtures/rpc/network.toml",
        r#"
id = "a-rpc-network"
protocol = 28
surface = "rpc"
category = "network"
description = "network fixture"
source_reference = "https://example.invalid/ref"
required_capabilities = ["rpc-client"]
method = "get-network"
"#,
    );
    dir.write(
        "fixtures/xdr/value.toml",
        &format!(
            "id = \"m-xdr-value\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"ledger value fixture\"\nrequired_capabilities = [\"stellar-sdk-dependency\"]\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir.write(
        "fixtures/xdr/old.toml",
        &format!(
            "id = \"p27-old\"\nprotocol = 27\nsurface = \"xdr\"\ncategory = \"legacy\"\ndescription = \"legacy fixture\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );

    dir
}

#[test]
fn fixtures_format_json_conforms_to_schema_and_matches_golden_file() {
    let dir = create_conformance_project("fixtures-json-conformance");
    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        support::stderr(&output)
    );

    let emitted = stdout(&output);

    // Validate emitted output against the formal JSON schema
    validate_document(&emitted).expect("fixtures output must conform to fixtures-v1 schema");

    // Assert byte-for-byte match with committed golden file
    let normalized_emitted = emitted.replace("\r\n", "\n");
    let normalized_golden = GOLDEN_TEXT.replace("\r\n", "\n");
    assert_eq!(
        normalized_emitted, normalized_golden,
        "emitted JSON must match golden reference byte-for-byte"
    );
}

#[test]
fn fixtures_ordering_is_deterministic_by_id_regardless_of_file_layout() {
    let dir = TempProject::new("fixtures-ordering-deterministic");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);

    // Write in reverse order and in arbitrary paths
    dir.write(
        "fixtures/z/file1.toml",
        r#"
id = "z-fixture"
protocol = 28
surface = "soroban"
category = "test"
description = "z fixture"
function = "fn_z"
"#,
    );
    dir.write(
        "fixtures/a/file2.toml",
        r#"
id = "a-fixture"
protocol = 28
surface = "rpc"
category = "test"
description = "a fixture"
method = "get_a"
"#,
    );
    dir.write(
        "fixtures/m/file3.toml",
        &format!(
            "id = \"m-fixture\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"m fixture\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );

    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert!(output.status.success());
    let emitted = stdout(&output);
    validate_document(&emitted).expect("must conform to schema");

    let doc: Value = serde_json::from_str(&emitted).unwrap();
    let ids: Vec<&str> = doc["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["a-fixture", "m-fixture", "z-fixture"]);
}

#[test]
fn missing_required_metadata_on_fixtures_triggers_schema_conformance_failure() {
    let valid: Value = serde_json::from_str(GOLDEN_TEXT).unwrap();

    // 1. Missing fixture required field: category
    let mut bad_fixture = valid.clone();
    bad_fixture["fixtures"][0]
        .as_object_mut()
        .unwrap()
        .remove("category");
    let err = validate(&bad_fixture, &schema(), "$").unwrap_err();
    assert!(
        err.contains("required field `category` is missing"),
        "error: {err}"
    );

    // 2. Missing fixture required field: requiredCapabilities
    let mut bad_fixture = valid.clone();
    bad_fixture["fixtures"][0]
        .as_object_mut()
        .unwrap()
        .remove("requiredCapabilities");
    let err = validate(&bad_fixture, &schema(), "$").unwrap_err();
    assert!(
        err.contains("required field `requiredCapabilities` is missing"),
        "error: {err}"
    );

    // 3. Missing fixture required field: surface
    let mut bad_fixture = valid.clone();
    bad_fixture["fixtures"][0]
        .as_object_mut()
        .unwrap()
        .remove("surface");
    let err = validate(&bad_fixture, &schema(), "$").unwrap_err();
    assert!(
        err.contains("required field `surface` is missing"),
        "error: {err}"
    );

    // 4. Missing top-level required field: directoryFound
    let mut bad_top = valid.clone();
    bad_top.as_object_mut().unwrap().remove("directoryFound");
    let err = validate(&bad_top, &schema(), "$").unwrap_err();
    assert!(
        err.contains("required field `directoryFound` is missing"),
        "error: {err}"
    );

    // 5. Invalid surface value
    let mut bad_surface = valid.clone();
    bad_surface["fixtures"][0]["surface"] = Value::String("invalid-surface".into());
    let err = validate(&bad_surface, &schema(), "$").unwrap_err();
    assert!(err.contains("is not one of"), "error: {err}");

    // 6. Invalid inventoryVersion
    let mut bad_ver = valid.clone();
    bad_ver["inventoryVersion"] = Value::from(2);
    let err = validate(&bad_ver, &schema(), "$").unwrap_err();
    assert!(err.contains("expected const 1"), "error: {err}");
}
