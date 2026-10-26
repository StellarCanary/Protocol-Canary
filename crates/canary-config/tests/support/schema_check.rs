//! The schema interpreter shared by the two configuration schema suites.
//!
//! `docs/schemas/config-v1.schema.json` is validated *as data* here rather
//! than re-stated as Rust: [`validate`] reads whatever keywords the published
//! document uses, and [`SUPPORTED_KEYWORDS`] is the set this interpreter
//! implements, so a schema that grows a keyword nobody taught it about is
//! reported by
//! `the_schema_uses_only_keywords_the_test_validator_interprets` in each suite
//! instead of being quietly ignored.
//!
//! Both `tests/schema.rs` (the schema's own conformance tests) and
//! `tests/schema_parity.rs` (the schema against the loader) need exactly this,
//! which is why it lives once. `pattern` is delegated to
//! [`crate::schema_pattern`], the narrow subset the published file is written
//! in.
//!
//! TOML documents reach [`validate`] through [`to_json`], which converts a
//! parsed TOML value into the JSON instance model. The one lossy step is a
//! date-time value: `.stellar-canary.toml` has no key that accepts one and the
//! loader refuses one for every key, so the converter renders it as JSON null
//! and the schema's `type` keyword refuses it for the same reason.

use serde_json::{json, Map, Value};

/// Every keyword the schema document may use. Adding a keyword to the schema
/// without teaching this interpreter about it fails
/// `the_schema_uses_only_keywords_the_test_validator_interprets`, which keeps
/// the validator an honest — if small — implementation of exactly the subset
/// the schema needs.
pub const SUPPORTED_KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "title",
    "description",
    "type",
    "const",
    "enum",
    "default",
    "required",
    "properties",
    "additionalProperties",
    "minimum",
    "maximum",
    "pattern",
    "not",
];

/// The JSON type name a schema `type` keyword compares against.
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
pub fn validate(instance: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("type") {
        let wanted: Vec<&str> = match expected {
            Value::String(name) => vec![name.as_str()],
            Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
            other => return Err(format!("{path}: schema has an unusable `type`: {other}")),
        };
        if !wanted.contains(&type_of(instance)) {
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
        if !values
            .as_array()
            .is_some_and(|values| values.contains(instance))
        {
            return Err(format!("{path}: {instance} is not one of {values}"));
        }
    }
    if let Some(minimum) = schema.get("minimum") {
        match (instance.as_f64(), minimum.as_f64()) {
            (Some(value), Some(min)) if value >= min => {}
            _ => return Err(format!("{path}: {instance} is below the minimum {minimum}")),
        }
    }
    if let Some(maximum) = schema.get("maximum") {
        match (instance.as_f64(), maximum.as_f64()) {
            (Some(value), Some(max)) if value <= max => {}
            _ => return Err(format!("{path}: {instance} is above the maximum {maximum}")),
        }
    }
    if let Some(pattern) = schema.get("pattern") {
        // `pattern` constrains strings; a value of another type is refused by
        // the `type` keyword, exactly as JSON Schema prescribes.
        let text = pattern
            .as_str()
            .ok_or_else(|| format!("{path}: schema has a non-string `pattern`"))?;
        if let Some(value) = instance.as_str() {
            let matched = crate::schema_pattern::matches(text, value)
                .map_err(|error| format!("{path}: {error}"))?;
            if !matched {
                return Err(format!("{path}: {value:?} does not match `{text}`"));
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
    if let Some(extra) = schema.get("additionalProperties") {
        if let Some(object) = instance.as_object() {
            let declared = schema.get("properties").and_then(Value::as_object);
            for (name, value) in object {
                if declared.is_some_and(|properties| properties.contains_key(name)) {
                    continue;
                }
                match extra {
                    Value::Bool(true) => {}
                    Value::Bool(false) => {
                        return Err(format!("{path}.{name} is not a declared configuration key"));
                    }
                    sub_schema => validate(value, sub_schema, &format!("{path}.{name}"))?,
                }
            }
        }
    }
    // A `not` subschema is a rejection: matching it is the violation. The
    // configuration schema uses exactly one, for the rule that a file cannot
    // turn off every surface at once.
    if let Some(negated) = schema.get("not") {
        if validate(instance, negated, path).is_ok() {
            return Err(format!("{path}: matches the schema's `not` constraint"));
        }
    }
    Ok(())
}

/// Converts a TOML value into the JSON instance model the schema validates.
///
/// A date-time becomes `null` deliberately: `.stellar-canary.toml` has no key
/// that accepts one, and the loader refuses it for every key, so the converted
/// document has to be refused by the schema for the same reason.
pub fn to_json(value: toml::Value) -> Value {
    match value {
        toml::Value::String(text) => Value::String(text),
        toml::Value::Integer(number) => json!(number),
        toml::Value::Float(number) => serde_json::Number::from_f64(number)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml::Value::Boolean(flag) => json!(flag),
        toml::Value::Datetime(_) => Value::Null,
        toml::Value::Array(items) => Value::Array(items.into_iter().map(to_json).collect()),
        toml::Value::Table(table) => {
            let mut object = Map::new();
            for (key, item) in table {
                object.insert(key, to_json(item));
            }
            Value::Object(object)
        }
    }
}

/// Parses `text` as TOML and validates the resulting document against
/// `schema`, so a caller never has to repeat the conversion step.
pub fn validate_toml_with(text: &str, schema: &Value) -> Result<(), String> {
    let table: toml::Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(error) => return Err(format!("not a TOML document: {error}")),
    };
    validate(&to_json(toml::Value::Table(table)), schema, "$")
}
