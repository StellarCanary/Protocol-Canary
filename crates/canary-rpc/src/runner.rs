//! The RPC compatibility runner: calls a Stellar RPC method and checks its
//! response shape against a fixture's declared field assertions.
//!
//! Assertions are evaluated against the JSON `result` object exactly as the
//! endpoint returned it (CA-01 section 4.1), not against a typed model of it,
//! so a field the typed model does not name can still be asserted on and a
//! `field-absent` assertion is never satisfied just because a model dropped
//! the field. A `result` that is not an object is an `error`, not a failed
//! assertion.
//!
//! A fixture declares which fields of the response are `required`,
//! `optional`, or simply not mentioned (and therefore ignored) — an
//! unrelated new optional field appearing in a response must never fail a
//! check (see [`FieldAssertion`]).

use std::cmp::Ordering;
use std::time::Instant;

use serde_json::Value as JsonValue;

use canary_core::{
    CanaryError, CompatibilityResult, ExecutionContext, FixtureMetadata, Status, Surface,
};
use canary_fixtures::LoadedFixture;

use crate::client::{RpcClient, RpcError};
use crate::models::{GetHealthResponse, LatestLedger, NetworkInfo};

#[derive(Debug, thiserror::Error)]
pub enum RpcFixtureError {
    #[error("invalid rpc fixture body in {source_path}: {reason}")]
    InvalidFixtureBody {
        source_path: std::path::PathBuf,
        reason: String,
    },
}

impl From<RpcFixtureError> for CanaryError {
    fn from(error: RpcFixtureError) -> Self {
        CanaryError::Rpc(error.to_string())
    }
}

/// Which RPC method a fixture exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcMethod {
    GetNetwork,
    GetLatestLedger,
    GetHealth,
    GetVersionInfo,
    GetFeeStats,
    GetEvents,
    GetLedgers,
    GetTransaction,
    GetLedgerEntries,
}

impl std::str::FromStr for RpcMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "get-network" => Ok(RpcMethod::GetNetwork),
            "get-latest-ledger" => Ok(RpcMethod::GetLatestLedger),
            "get-health" => Ok(RpcMethod::GetHealth),
            "get-version-info" => Ok(RpcMethod::GetVersionInfo),
            "get-fee-stats" => Ok(RpcMethod::GetFeeStats),
            "get-events" => Ok(RpcMethod::GetEvents),
            "get-ledgers" => Ok(RpcMethod::GetLedgers),
            "get-transaction" => Ok(RpcMethod::GetTransaction),
            "get-ledger-entries" => Ok(RpcMethod::GetLedgerEntries),
            other => Err(format!(
                "unsupported rpc method {other:?}: supported methods are: \"get-network\", \
                 \"get-latest-ledger\", \"get-health\", \"get-version-info\", \"get-fee-stats\", \
                 \"get-events\", \"get-ledgers\", \"get-ledger-entries\", \"get-transaction\""
            )),
        }
    }
}

/// The expected JSON type of a field, for `field-type` assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonType {
    String,
    Number,
    /// A JSON number with no fractional part (CA-01 section 4.2).
    ///
    /// Distinct from [`JsonType::Number`], which also accepts a fractional
    /// value: `1` and `1.0` both satisfy `integer`, while `1.5` does not.
    /// The comparison is made on the number itself, never by coercing a
    /// string to a number or the reverse, so an upstream field returned as
    /// the string `"1234"` fails `integer` by design.
    Integer,
    Bool,
    Object,
    Array,
    Null,
}

impl JsonType {
    fn matches(self, value: &JsonValue) -> bool {
        match self {
            JsonType::String => value.is_string(),
            JsonType::Number => value.is_number(),
            JsonType::Integer => match value {
                JsonValue::Number(number) => {
                    // `is_i64`/`is_u64` keep values beyond 2^53 exact;
                    // `as_f64` is only consulted for a fractional token,
                    // where `fract() == 0.0` says it holds no fraction.
                    number.is_i64()
                        || number.is_u64()
                        || number
                            .as_f64()
                            .is_some_and(|value| value.is_finite() && value.fract() == 0.0)
                }
                _ => false,
            },
            JsonType::Bool => value.is_boolean(),
            JsonType::Object => value.is_object(),
            JsonType::Array => value.is_array(),
            JsonType::Null => value.is_null(),
        }
    }
}

impl std::str::FromStr for JsonType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "string" => Ok(JsonType::String),
            "number" => Ok(JsonType::Number),
            "integer" => Ok(JsonType::Integer),
            "bool" => Ok(JsonType::Bool),
            "object" => Ok(JsonType::Object),
            "array" => Ok(JsonType::Array),
            "null" => Ok(JsonType::Null),
            other => Err(format!(
                "unsupported json type {other:?}: supported types are: \"string\", \"number\", \"integer\", \"bool\", \"object\", \"array\", \"null\""
            )),
        }
    }
}

/// The inclusive bounds of a `field-number-range` assertion (CA-01
/// section 4.2).
///
/// At least one bound is always set when the assertion is parsed from a
/// fixture: an open-ended range is written with only the bound that
/// exists (`>= min` or `<= max`), and a range with neither bound would
/// check nothing.
///
/// Bounds keep the JSON number they were parsed from (integer or
/// float), so integer bounds compare exactly and floating point is used
/// only when a bound or an observed value is genuinely fractional.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldRangeAssertion {
    pub field: String,
    pub min: Option<serde_json::Number>,
    pub max: Option<serde_json::Number>,
}

/// Inclusive item-count bounds for a JSON array field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldLengthAssertion {
    pub field: String,
    pub exact: Option<usize>,
    pub min_items: Option<usize>,
    pub max_items: Option<usize>,
}

impl FieldLengthAssertion {
    fn check_value(&self, found: Option<&JsonValue>) -> Result<(), String> {
        let Some(value) = found else {
            return Err(format!(
                "expected field {:?} to be present, but it was missing",
                self.field
            ));
        };
        let Some(items) = value.as_array() else {
            return Err(format!(
                "expected field {:?} to be an array, but found {}",
                self.field, value
            ));
        };
        if self.exact.is_some_and(|exact| items.len() != exact)
            || self.min_items.is_some_and(|min| items.len() < min)
            || self.max_items.is_some_and(|max| items.len() > max)
        {
            return Err(format!(
                "expected field {:?} array length in {}, but found {}",
                self.field,
                self.bounds_description(),
                items.len()
            ));
        }
        Ok(())
    }

    fn bounds_description(&self) -> String {
        match (self.min_items, self.max_items) {
            _ if self.exact.is_some() => format!("exactly {}", self.exact.unwrap_or_default()),
            (Some(min), Some(max)) => format!("[{min}, {max}]"),
            (Some(min), None) => format!("at least {min}"),
            (None, Some(max)) => format!("at most {max}"),
            (None, None) => "any length".to_string(),
        }
    }
}

impl FieldRangeAssertion {
    /// Checks the observed value of the field against the bounds,
    /// returning a human-readable failure reason if it does not hold.
    ///
    /// A missing field or a value that is not a JSON number fails the
    /// assertion; nothing is coerced between strings and numbers.
    pub fn check_value(&self, found: Option<&JsonValue>) -> Result<(), String> {
        let Some(actual) = found else {
            return Err(format!(
                "expected field {:?} to be present, but it was missing",
                self.field
            ));
        };
        let JsonValue::Number(value) = actual else {
            return Err(format!(
                "expected field {:?} to be a number within {}, but found {}",
                self.field,
                self.bounds_description(),
                actual
            ));
        };
        if let Some(min) = &self.min {
            if compare_numbers(value, min) == Some(Ordering::Less) {
                return Err(self.out_of_range(actual));
            }
        }
        if let Some(max) = &self.max {
            if compare_numbers(value, max) == Some(Ordering::Greater) {
                return Err(self.out_of_range(actual));
            }
        }
        Ok(())
    }

    /// The bounds as they appear in failure messages: `[min, max]`,
    /// `>= min`, or `<= max`.
    fn bounds_description(&self) -> String {
        match (&self.min, &self.max) {
            (Some(min), Some(max)) => format!("[{min}, {max}]"),
            (Some(min), None) => format!(">= {min}"),
            (None, Some(max)) => format!("<= {max}"),
            (None, None) => "any number".to_string(),
        }
    }

    fn out_of_range(&self, actual: &JsonValue) -> String {
        format!(
            "expected field {:?} to be within {}, but found {}",
            self.field,
            self.bounds_description(),
            actual
        )
    }
}

/// Compares two JSON numbers, exactly when both sides are integers and
/// with `f64` only when at least one side is genuinely fractional.
/// Returns `None` when either value cannot be read as a number.
fn compare_numbers(value: &serde_json::Number, bound: &serde_json::Number) -> Option<Ordering> {
    if let (Some(value), Some(bound)) = (value.as_i64(), bound.as_i64()) {
        return Some(value.cmp(&bound));
    }
    if let (Some(value), Some(bound)) = (value.as_u64(), bound.as_u64()) {
        return Some(value.cmp(&bound));
    }
    value.as_f64()?.partial_cmp(&bound.as_f64()?)
}

/// A `field-one-of` assertion (CA-01 section 4.2): the field's value must
/// equal one of a fixed list of allowed values.
///
/// This is how a fixture asserts on an enumeration or on a transition with
/// several acceptable outcomes — `"status" in ["SUCCESS", "FAILED"]` —
/// without one `field-equals` assertion per candidate.
///
/// Comparison is exact JSON equality, so nothing is coerced: a string is
/// never equal to a number, and `true` is never equal to `1`. Integers keep
/// their full 64-bit precision because each side is compared as the number
/// it was parsed as rather than through `f64`.
///
/// The assertion fails (rather than errors) when the field is missing or
/// when none of the allowed values match, and the failure reason lists both
/// the observed value and every candidate (CA-01 section 4.2).
///
/// `path` addressing (CA-01 section 4.3) is not implemented yet; like the
/// other assertion kinds in this crate, this one addresses a top-level
/// `field`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldOneOfAssertion {
    pub field: String,
    /// The allowed values. Always non-empty when parsed from a fixture: an
    /// empty list would check nothing, so it is rejected as invalid.
    pub values: Vec<JsonValue>,
}

impl FieldOneOfAssertion {
    /// Checks the observed value of the field against the allowed values,
    /// returning a human-readable failure reason if it matches none.
    pub fn check_value(&self, found: Option<&JsonValue>) -> Result<(), String> {
        let Some(actual) = found else {
            return Err(format!(
                "expected field {:?} to be present, but it was missing",
                self.field
            ));
        };
        if self.values.iter().any(|allowed| allowed == actual) {
            return Ok(());
        }
        Err(format!(
            "expected field {:?} to equal one of {}, but found {actual}",
            self.field,
            render_allowed(&self.values)
        ))
    }
}

/// Renders an assertion's allowed values for a failure diagnostic, e.g.
/// `["SUCCESS", 2, true]`.
fn render_allowed(values: &[JsonValue]) -> String {
    let rendered = values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

/// One assertion about the `topic` array of an event in a `getEvents`
/// result.
///
/// Every topic of the event is compared in one assertion, in order, so a
/// fixture can pin a multi-topic event (for example an approval event whose
/// topics are the event name, the contract and the spender) without one
/// `field` assertion per position. The check reads the raw JSON `result`
/// object (CA-01 section 4.1) rather than [`crate::models::Event`], so an
/// untyped payload is never projected away before it is asserted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTopicsAssertion {
    /// Position of the event in the result's `events` array.
    pub index: usize,
    /// The expected `topic` entries, in order.
    pub topics: Vec<String>,
}

impl EventTopicsAssertion {
    /// Checks the event at `index`, returning a human-readable failure
    /// reason when the event, its `topic` array, or a topic differs.
    pub fn check_value(&self, response: &JsonValue) -> Result<(), String> {
        let events = response
            .get("events")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| {
                "expected an \"events\" array in the getEvents result, but it was missing"
                    .to_string()
            })?;
        let event = events.get(self.index).ok_or_else(|| {
            format!(
                "expected an event at index {}, but the result held {} event(s)",
                self.index,
                events.len()
            )
        })?;
        let found = event
            .get("topic")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| {
                format!(
                    "expected event {} to have a \"topic\" array, but it was missing",
                    self.index
                )
            })?;

        if found.len() != self.topics.len() {
            return Err(format!(
                "expected event {} to have {} topic(s) {:?}, but found {} topic(s) {found:?}",
                self.index,
                self.topics.len(),
                self.topics,
                found.len()
            ));
        }

        for (position, (found, expected)) in found.iter().zip(&self.topics).enumerate() {
            if found.as_str() != Some(expected.as_str()) {
                return Err(format!(
                    "expected event {} topic {position} to equal {expected:?}, but found {found}",
                    self.index
                ));
            }
        }

        Ok(())
    }
}

/// One assertion about a field in the RPC response.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldAssertion {
    Exists { field: String },
    Absent { field: String },
    Equals { field: String, value: JsonValue },
    TypeIs { field: String, expected: JsonType },
    NumberRange(FieldRangeAssertion),
    Length(FieldLengthAssertion),
    OneOf(FieldOneOfAssertion),
    EventTopics(EventTopicsAssertion),
}

impl FieldAssertion {
    /// The top-level field this assertion names, if it names one. An
    /// `event-topics` assertion addresses an entry in the `events` array
    /// instead.
    fn field(&self) -> Option<&str> {
        match self {
            FieldAssertion::Exists { field }
            | FieldAssertion::Absent { field }
            | FieldAssertion::Equals { field, .. }
            | FieldAssertion::TypeIs { field, .. } => Some(field),
            FieldAssertion::NumberRange(assertion) => Some(&assertion.field),
            FieldAssertion::Length(assertion) => Some(&assertion.field),
            FieldAssertion::OneOf(assertion) => Some(&assertion.field),
            FieldAssertion::EventTopics(_) => None,
        }
    }

    /// Checks this assertion against a response object, returning a
    /// human-readable failure reason if it does not hold.
    fn check(&self, response: &JsonValue) -> Result<(), String> {
        if let FieldAssertion::EventTopics(assertion) = self {
            return assertion.check_value(response);
        }
        let field = self
            .field()
            .expect("a non-event assertion always names a field");
        let found = resolve_field(response, field);
        match self {
            FieldAssertion::Exists { field } => found.map(|_| ()).ok_or_else(|| {
                format!("expected field {field:?} to be present, but it was missing")
            }),
            FieldAssertion::Absent { field } => match found {
                None => Ok(()),
                Some(_) => Err(format!(
                    "expected field {field:?} to be absent, but it was present"
                )),
            },
            FieldAssertion::Equals { field, value } => match found {
                None => Err(format!(
                    "expected field {field:?} to equal {value}, but it was missing"
                )),
                Some(actual) if actual == value => Ok(()),
                Some(actual) => Err(format!(
                    "expected field {field:?} to equal {value}, but found {actual}"
                )),
            },
            FieldAssertion::TypeIs { field, expected } => match found {
                None => Err(format!(
                    "expected field {field:?} to be present, but it was missing"
                )),
                Some(actual) if expected.matches(actual) => Ok(()),
                Some(actual) => Err(format!(
                    "expected field {field:?} to have type {expected:?}, but found {actual}"
                )),
            },
            FieldAssertion::NumberRange(assertion) => assertion.check_value(found),
            FieldAssertion::Length(assertion) => assertion.check_value(found),
            FieldAssertion::OneOf(assertion) => assertion.check_value(found),
            // Handled by the early return above; repeated so the match stays
            // exhaustive.
            FieldAssertion::EventTopics(assertion) => assertion.check_value(response),
        }
    }
}

/// Resolves a field path using object keys separated by dots and array
/// indexes in brackets (for example `result.fee.max` or `items[0].id`).
fn resolve_field<'a>(response: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut current = response;
    let mut token = String::new();
    let mut chars = path.chars().peekable();
    let mut after_index = false;
    while let Some(ch) = chars.next() {
        match ch {
            '.' => {
                if !token.is_empty() {
                    current = current.get(&token)?;
                    token.clear();
                } else if !after_index {
                    return None;
                }
                after_index = false;
            }
            '[' => {
                if !token.is_empty() {
                    current = current.get(&token)?;
                    token.clear();
                }
                let mut index = String::new();
                for next in chars.by_ref() {
                    if next == ']' {
                        break;
                    }
                    index.push(next);
                }
                if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                current = current.get(index.parse::<usize>().ok()?)?;
                if chars
                    .peek()
                    .is_some_and(|next| *next != '.' && *next != '[')
                {
                    return None;
                }
                after_index = true;
            }
            ']' => return None,
            _ => {
                token.push(ch);
                after_index = false;
            }
        }
    }
    if !token.is_empty() {
        current = current.get(&token)?;
    }
    Some(current)
}

/// A fully parsed RPC compatibility fixture.
#[derive(Debug, Clone)]
pub struct RpcFixture {
    pub metadata: FixtureMetadata,
    pub method: RpcMethod,
    /// Validated `[request]` parameters, as the named-params object to
    /// send. `None` when the fixture has no `[request]` table.
    pub request: Option<serde_json::Map<String, serde_json::Value>>,
    pub assertions: Vec<FieldAssertion>,
}

impl RpcFixture {
    /// Parses an [`RpcFixture`] from a generic [`LoadedFixture`].
    ///
    /// Expected body shape:
    ///
    /// ```toml
    /// method = "get-network"
    ///
    /// [[assert]]
    /// kind = "field-equals"
    /// field = "protocolVersion"
    /// value = 28
    ///
    /// [[assert]]
    /// kind = "field-exists"
    /// field = "passphrase"
    /// ```
    ///
    /// A `[request]` table, for methods that take parameters, is validated
    /// against the CA-01 parameter table (see [`crate::request`]).
    pub fn from_loaded(loaded: &LoadedFixture) -> Result<RpcFixture, RpcFixtureError> {
        let invalid = |reason: String| RpcFixtureError::InvalidFixtureBody {
            source_path: loaded.source_path.clone(),
            reason,
        };

        let table = loaded
            .body
            .as_table()
            .ok_or_else(|| invalid("fixture body must be a table".to_string()))?;

        if let Some(message) =
            canary_fixtures::unknown_body_keys(table, &["method", "request", "assert"])
        {
            return Err(invalid(message));
        }

        let method_name = table
            .get("method")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| invalid("missing required string field \"method\"".to_string()))?;
        // A bad `[request]` is reported even for a method the engine does
        // not run yet, so the author sees it before the method is wired up.
        let request = if crate::request::is_known_method(method_name) {
            crate::request::validate_request(method_name, table.get("request")).map_err(&invalid)?
        } else {
            None
        };
        let method: RpcMethod = method_name.parse().map_err(invalid)?;

        let assert_entries = table
            .get("assert")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default();

        if assert_entries.is_empty() {
            return Err(invalid(
                "an rpc fixture needs at least one [[assert]] table; without one it would pass while checking nothing (is a section misspelled?)".to_string(),
            ));
        }

        let mut assertions = Vec::with_capacity(assert_entries.len());
        for entry in &assert_entries {
            assertions.push(parse_assertion(entry, &invalid)?);
        }

        Ok(RpcFixture {
            metadata: loaded.metadata.clone(),
            method,
            request,
            assertions,
        })
    }
}

fn parse_assertion(
    entry: &toml::Value,
    invalid: &impl Fn(String) -> RpcFixtureError,
) -> Result<FieldAssertion, RpcFixtureError> {
    let table = entry
        .as_table()
        .ok_or_else(|| invalid("each [[assert]] entry must be a table".to_string()))?;

    if let Some(message) = canary_fixtures::unknown_body_keys(
        table,
        &[
            "kind",
            "field",
            "value",
            "expected_type",
            "min",
            "max",
            "min_items",
            "max_items",
            "exact",
            "values",
            "index",
            "topics",
        ],
    ) {
        return Err(invalid(format!("[[assert]] entry: {message}")));
    }

    let kind = table
        .get("kind")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| invalid("assertion entry missing \"kind\"".to_string()))?;
    // `field` is required by every kind except `event-topics`, which
    // addresses an entry in the `events` array instead.
    let field = match table.get("field") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or_else(|| invalid("assertion \"field\" must be a string".to_string()))?
                .to_string(),
        ),
    };
    let field_or_error = || {
        field
            .clone()
            .ok_or_else(|| invalid("assertion entry missing \"field\"".to_string()))
    };

    match kind {
        "field-exists" => Ok(FieldAssertion::Exists {
            field: field_or_error()?,
        }),
        "field-absent" => Ok(FieldAssertion::Absent {
            field: field_or_error()?,
        }),
        "field-equals" => {
            let value = table
                .get("value")
                .ok_or_else(|| invalid("field-equals assertion missing \"value\"".to_string()))?;
            Ok(FieldAssertion::Equals {
                field: field_or_error()?,
                value: toml_to_json(value),
            })
        }
        "field-type" => {
            let expected = table
                .get("expected_type")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| {
                    invalid("field-type assertion missing \"expected_type\"".to_string())
                })?
                .parse()
                .map_err(invalid)?;
            Ok(FieldAssertion::TypeIs {
                field: field_or_error()?,
                expected,
            })
        }
        "field-number-range" => {
            let min = parse_range_bound(table, "min", &invalid)?;
            let max = parse_range_bound(table, "max", &invalid)?;
            if min.is_none() && max.is_none() {
                return Err(invalid(
                    "field-number-range assertion requires \"min\" or \"max\" (or both); a range with neither bound would check nothing"
                        .to_string(),
                ));
            }
            if let (Some(min), Some(max)) = (&min, &max) {
                if compare_numbers(min, max) == Some(Ordering::Greater) {
                    return Err(invalid(format!(
                        "field-number-range assertion has min {min} greater than max {max}"
                    )));
                }
            }
            Ok(FieldAssertion::NumberRange(FieldRangeAssertion {
                field: field_or_error()?,
                min,
                max,
            }))
        }
        "field-length" => {
            let exact = parse_length_bound(table, "exact", &invalid)?;
            let min_items = parse_length_alias(table, "min", "min_items", &invalid)?;
            let max_items = parse_length_alias(table, "max", "max_items", &invalid)?;
            if exact.is_none() && min_items.is_none() && max_items.is_none() {
                return Err(invalid(
                    "field-length assertion requires \"exact\", \"min_items\", or \"max_items\""
                        .to_string(),
                ));
            }
            if let Some(exact) = exact {
                if min_items.is_some_and(|min| exact < min)
                    || max_items.is_some_and(|max| exact > max)
                {
                    return Err(invalid(
                        "field-length exact value conflicts with its bounds".to_string(),
                    ));
                }
            }
            if let (Some(min), Some(max)) = (min_items, max_items) {
                if min > max {
                    return Err(invalid(format!(
                        "field-length assertion has min_items {min} greater than max_items {max}"
                    )));
                }
            }
            Ok(FieldAssertion::Length(FieldLengthAssertion {
                field: field_or_error()?,
                exact,
                min_items,
                max_items,
            }))
        }
        "field-one-of" => {
            let values = table
                .get("values")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| {
                    invalid("field-one-of assertion requires a \"values\" array".to_string())
                })?
                .iter()
                .map(toml_to_json)
                .collect::<Vec<_>>();
            if values.is_empty() {
                return Err(invalid(
                    "field-one-of assertion requires at least one value; an empty list would check nothing"
                        .to_string(),
                ));
            }
            Ok(FieldAssertion::OneOf(FieldOneOfAssertion {
                field: field_or_error()?,
                values,
            }))
        }
        "event-topics" => {
            let index = match table.get("index") {
                None => 0,
                Some(value) => value
                    .as_integer()
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| {
                        invalid("event-topics \"index\" must be a non-negative integer".to_string())
                    })?,
            };
            let topics = table
                .get("topics")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| {
                    invalid("event-topics assertion requires a \"topics\" array".to_string())
                })?
                .iter()
                .map(|topic| {
                    topic.as_str().map(str::to_string).ok_or_else(|| {
                        invalid("event-topics \"topics\" entries must be strings".to_string())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if topics.is_empty() {
                return Err(invalid(
                    "event-topics assertion requires at least one topic; an empty list would check nothing"
                        .to_string(),
                ));
            }
            Ok(FieldAssertion::EventTopics(EventTopicsAssertion {
                index,
                topics,
            }))
        }
        other => Err(invalid(format!(
            "unsupported assertion kind {other:?}: expected one of \"field-exists\", \"field-absent\", \"field-equals\", \"field-type\", \"field-number-range\", \"field-one-of\", \"field-length\", \"event-topics\""
        ))),
    }
}

fn parse_length_bound(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    invalid: &impl Fn(String) -> RpcFixtureError,
) -> Result<Option<usize>, RpcFixtureError> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    let bound = value
        .as_integer()
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| {
            invalid(format!(
                "field-length {key:?} must be a non-negative integer"
            ))
        })?;
    Ok(Some(bound))
}

fn parse_length_alias(
    table: &toml::map::Map<String, toml::Value>,
    alias: &str,
    canonical: &str,
    invalid: &impl Fn(String) -> RpcFixtureError,
) -> Result<Option<usize>, RpcFixtureError> {
    if table.contains_key(alias) && table.contains_key(canonical) {
        return Err(invalid(format!(
            "field-length cannot specify both {alias:?} and {canonical:?}"
        )));
    }
    parse_length_bound(
        table,
        if table.contains_key(alias) {
            alias
        } else {
            canonical
        },
        invalid,
    )
}

/// Parses one `field-number-range` bound: a TOML integer or float.
/// Anything else is an invalid fixture rather than a runtime surprise.
fn parse_range_bound(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    invalid: &impl Fn(String) -> RpcFixtureError,
) -> Result<Option<serde_json::Number>, RpcFixtureError> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    let number = match value {
        toml::Value::Integer(i) => serde_json::Number::from(*i),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f).ok_or_else(|| {
            invalid(format!(
                "field-number-range {key:?} must be a finite number"
            ))
        })?,
        other => {
            return Err(invalid(format!(
                "field-number-range {key:?} must be a number, but found {other}"
            )));
        }
    };
    Ok(Some(number))
}

fn toml_to_json(value: &toml::Value) -> JsonValue {
    match value {
        toml::Value::String(s) => JsonValue::String(s.clone()),
        toml::Value::Integer(i) => JsonValue::Number((*i).into()),
        toml::Value::Float(f) => {
            serde_json::Number::from_f64(*f).map_or(JsonValue::Null, JsonValue::Number)
        }
        toml::Value::Boolean(b) => JsonValue::Bool(*b),
        toml::Value::Datetime(dt) => JsonValue::String(dt.to_string()),
        toml::Value::Array(items) => JsonValue::Array(items.iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => JsonValue::Object(
            table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect(),
        ),
    }
}

/// Runs a single [`RpcFixture`] against a live [`RpcClient`].
pub trait RpcRunner {
    fn run(
        &self,
        fixture: &RpcFixture,
        context: &ExecutionContext,
    ) -> impl std::future::Future<Output = Result<CompatibilityResult, CanaryError>> + Send;
}

/// The runner used in production.
pub struct DefaultRpcRunner<C: RpcClient> {
    client: C,
}

/// The named `params` object to send for a fixture: its validated
/// `[request]` table, or an empty object when it has none.
fn request_params(fixture: &RpcFixture) -> serde_json::Value {
    fixture
        .request
        .clone()
        .map(serde_json::Value::Object)
        .unwrap_or_else(|| serde_json::Value::Object(Default::default()))
}

/// Summary reported when the endpoint says the fixture's RPC method does
/// not exist (JSON-RPC `-32601`): the check is skipped rather than marked
/// as an error, because an optional or unsupported method says nothing
/// about the endpoint's health — it is not a connection failure.
const SKIP_METHOD_NOT_AVAILABLE: &str =
    "method not available on this endpoint, check skipped (JSON-RPC -32601)";

impl<C: RpcClient> DefaultRpcRunner<C> {
    pub fn new(client: C) -> Self {
        DefaultRpcRunner { client }
    }
}

impl<C: RpcClient + Sync> RpcRunner for DefaultRpcRunner<C> {
    async fn run(
        &self,
        fixture: &RpcFixture,
        _context: &ExecutionContext,
    ) -> Result<CompatibilityResult, CanaryError> {
        let start = Instant::now();

        let response_json = match fixture.method {
            RpcMethod::GetNetwork => self.client.get_network_raw().await,
            RpcMethod::GetLatestLedger => self.client.get_latest_ledger_raw().await,
            RpcMethod::GetHealth => self.client.get_health_raw().await,
            RpcMethod::GetVersionInfo => self.client.get_version_info_raw().await,
            RpcMethod::GetFeeStats => self.client.get_fee_stats_raw().await,
            RpcMethod::GetEvents => self.client.get_events_raw(request_params(fixture)).await,
            RpcMethod::GetLedgers => self.client.get_ledgers_raw(request_params(fixture)).await,
            RpcMethod::GetTransaction => {
                self.client
                    .get_transaction_raw(request_params(fixture))
                    .await
            }
            RpcMethod::GetLedgerEntries => {
                self.client
                    .get_ledger_entries_raw(request_params(fixture))
                    .await
            }
        };

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        let (status, summary, details) = match response_json {
            Err(rpc_error) => match &rpc_error {
                // The endpoint answered and does not implement this method:
                // skip the check instead of reporting an endpoint failure.
                RpcError::MethodNotAvailable { method } => (
                    Status::Skipped,
                    format!("{SKIP_METHOD_NOT_AVAILABLE}: {method}"),
                    Some(rpc_error.to_string()),
                ),
                _ => (
                    Status::Error,
                    format!("failed to call {:?}", fixture.method),
                    Some(rpc_error.to_string()),
                ),
            },
            Ok(response) => evaluate(fixture, &response),
        };

        Ok(CompatibilityResult {
            test_id: fixture.metadata.id.clone(),
            protocol: fixture.metadata.protocol,
            surface: Surface::Rpc,
            status,
            summary,
            details,
            duration_ms,
            fixture_id: Some(fixture.metadata.id.clone()),
            source: canary_core::ResultSource::Live,
        })
    }
}

fn evaluate(fixture: &RpcFixture, response: &JsonValue) -> (Status, String, Option<String>) {
    let mut failures = Vec::new();
    for assertion in &fixture.assertions {
        if let Err(reason) = assertion.check(response) {
            failures.push(reason);
        }
    }

    if failures.is_empty() {
        (
            Status::Pass,
            format!(
                "{:?} response matched all {} assertion(s)",
                fixture.method,
                fixture.assertions.len()
            ),
            None,
        )
    } else {
        (
            Status::Fail,
            format!(
                "{:?} response failed {} of {} assertion(s)",
                fixture.method,
                failures.len(),
                fixture.assertions.len()
            ),
            Some(failures.join("\n")),
        )
    }
}

/// Fetches network identity from `client`, without treating any
/// comparison against the run's expectations as a hard failure to
/// execute: the caller decides what a mismatch means for its run. To
/// compare the result against an expected passphrase and target protocol
/// (constructing [`RpcError::NetworkMismatch`]/[`RpcError::ProtocolMismatch`]),
/// pass it to [`crate::validate_network_info`].
pub async fn observe_network(client: &impl RpcClient) -> Result<NetworkInfo, RpcError> {
    client.get_network().await
}

/// Fetches the latest ledger, primarily to cross-check protocol identity
/// against [`observe_network`].
pub async fn observe_latest_ledger(client: &impl RpcClient) -> Result<LatestLedger, RpcError> {
    client.get_latest_ledger().await
}

/// Fetches endpoint health via `getHealth`.
pub async fn observe_health(client: &impl RpcClient) -> Result<GetHealthResponse, RpcError> {
    client.get_health().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::HttpRpcClient;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fixture(id: &str, body_toml: &str) -> LoadedFixture {
        canary_fixtures::parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"network\"\ndescription = \"test\"\n{body_toml}"
            ),
            std::path::Path::new("test.toml"),
        )
        .unwrap()
    }

    fn context() -> ExecutionContext {
        use canary_core::{
            GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType, ProtocolVersion,
            RunOptions,
        };
        ExecutionContext {
            protocol: ProtocolVersion(28),
            project: ProjectContext {
                root: ".".into(),
                name: "test".into(),
                project_type: ProjectType::Unknown,
                capabilities: vec![],
            },
            network: NetworkContext {
                name: NetworkName::Testnet,
                rpc_url: "https://example.invalid".into(),
                passphrase: "Test SDF Network ; September 2015".into(),
                observed_protocol: None,
            },
            fixtures: canary_core::FixtureStore::default(),
            git: GitContext::default(),
            cache: canary_core::CacheStore::new(
                std::env::temp_dir().join("canary-rpc-runner-tests-cache"),
            ),
            options: RunOptions::default(),
        }
    }

    #[tokio::test]
    async fn passes_when_all_assertions_hold() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 28 }
            })))
            .mount(&server)
            .await;

        let body = r#"
        method = "get-network"

        [[assert]]
        kind = "field-equals"
        field = "protocolVersion"
        value = 28

        [[assert]]
        kind = "field-exists"
        field = "passphrase"

        [[assert]]
        kind = "field-absent"
        field = "friendbotUrl"
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-1", body)).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Pass);
    }

    #[tokio::test]
    async fn fails_when_a_field_equals_assertion_does_not_hold() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 27 }
            })))
            .mount(&server)
            .await;

        let body = r#"
        method = "get-network"

        [[assert]]
        kind = "field-equals"
        field = "protocolVersion"
        value = 28
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-2", body)).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Fail);
    }

    #[tokio::test]
    async fn a_transport_failure_produces_an_error_status_not_a_fail() {
        let runner = DefaultRpcRunner::new(HttpRpcClient::new("http://127.0.0.1:0"));
        let fixture =
            RpcFixture::from_loaded(&fixture("p28-rpc-3", "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n")).unwrap();
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Error);
    }

    #[tokio::test]
    async fn method_not_available_skips_the_check_instead_of_erroring() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": { "code": -32601, "message": "Method not found" }
            })))
            .mount(&server)
            .await;

        let fixture =
            RpcFixture::from_loaded(&fixture("p28-rpc-method-missing", "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n")).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Skipped);
        assert!(
            result.summary.contains(SKIP_METHOD_NOT_AVAILABLE),
            "{}",
            result.summary
        );
        assert!(result.summary.contains("getNetwork"), "{}", result.summary);
        let details = result.details.expect("skip carries the diagnostic");
        assert!(details.contains("-32601"), "{details}");
    }

    #[tokio::test]
    async fn other_json_rpc_errors_stay_errors_rather_than_skips() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": { "code": -32602, "message": "invalid params" }
            })))
            .mount(&server)
            .await;

        let fixture =
            RpcFixture::from_loaded(&fixture("p28-rpc-bad-params", "method = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n")).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Error);
        assert!(result.summary.starts_with("failed to call"));
        let details = result.details.expect("error carries the diagnostic");
        assert!(details.contains("code -32602"), "{details}");
    }

    #[test]
    fn rejects_an_unsupported_method_name() {
        let err =
            RpcFixture::from_loaded(&fixture("bad", "method = \"not-a-method\"\n")).unwrap_err();
        assert!(matches!(err, RpcFixtureError::InvalidFixtureBody { .. }));
    }

    #[test]
    fn parses_and_checks_array_field_equals() {
        let body = r#"
        method = "get-network"

        [[assert]]
        kind = "field-equals"
        field = "history"
        value = ["a", "b", "c"]
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-array", body)).unwrap();
        assert!(fixture.assertions.contains(&FieldAssertion::Equals {
            field: "history".into(),
            value: json!(["a", "b", "c"])
        }));

        let assertion = &fixture.assertions[0];

        let match_response = json!({ "history": ["a", "b", "c"] });
        assert!(assertion.check(&match_response).is_ok());

        let mismatch_response = json!({ "history": ["a", "b", "d"] });
        assert!(assertion.check(&mismatch_response).is_err());
    }

    #[test]
    fn resolves_nested_dot_and_index_paths() {
        let response = json!({"result": {"fee": {"max": 42}}, "items": [{"id": "abc"}]});
        assert_eq!(resolve_field(&response, "result.fee.max"), Some(&json!(42)));
        assert_eq!(resolve_field(&response, "items[0].id"), Some(&json!("abc")));
        assert_eq!(resolve_field(&response, "items[1].id"), None);
        assert_eq!(resolve_field(&response, "items[0].missing"), None);
    }

    #[test]
    fn checks_array_length_bounds_and_rejects_non_arrays() {
        let assertion = FieldLengthAssertion {
            field: "items".into(),
            exact: None,
            min_items: Some(1),
            max_items: Some(3),
        };
        assert!(assertion.check_value(Some(&json!([1, 2]))).is_ok());
        assert!(assertion.check_value(Some(&json!([]))).is_err());
        let error = assertion.check_value(Some(&json!("items"))).unwrap_err();
        assert!(error.contains("expected field \"items\" to be an array"));
    }

    /// Parses a single `[[assert]]` entry of the given kind into a
    /// [`FieldAssertion`], the way a fixture author's entry reaches the
    /// runner.
    fn range_assertion(entry_toml: &str) -> FieldAssertion {
        RpcFixture::from_loaded(&fixture(
            "p28-rpc-range",
            &format!("method = \"get-network\"\n\n[[assert]]\n{entry_toml}"),
        ))
        .expect("valid range assertion")
        .assertions
        .remove(0)
    }

    fn range_err(entry_toml: &str) -> String {
        RpcFixture::from_loaded(&fixture(
            "p28-rpc-range-bad",
            &format!("method = \"get-network\"\n\n[[assert]]\n{entry_toml}"),
        ))
        .expect_err("invalid range assertion")
        .to_string()
    }

    #[test]
    fn field_number_range_passes_at_both_inclusive_boundaries() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"protocolVersion\"\nmin = 20\nmax = 30\n",
        );
        assert!(assertion.check(&json!({ "protocolVersion": 20 })).is_ok());
        assert!(assertion.check(&json!({ "protocolVersion": 30 })).is_ok());
        assert!(assertion.check(&json!({ "protocolVersion": 25 })).is_ok());
    }

    #[test]
    fn field_number_range_reports_the_observed_value_and_the_expected_range() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 20\nmax = 30\n",
        );
        let below = assertion.check(&json!({ "sequence": 19 })).unwrap_err();
        assert!(below.contains("[20, 30]"), "{below}");
        assert!(below.contains("but found 19"), "{below}");
        let above = assertion.check(&json!({ "sequence": 31 })).unwrap_err();
        assert!(above.contains("[20, 30]"), "{above}");
        assert!(above.contains("but found 31"), "{above}");
    }

    #[test]
    fn field_number_range_fails_when_the_field_is_missing() {
        let assertion =
            range_assertion("kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 1\n");
        let err = assertion.check(&json!({})).unwrap_err();
        assert!(err.contains("\"sequence\" to be present"), "{err}");
    }

    #[test]
    fn field_number_range_rejects_non_numerical_values_without_coercion() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 1\nmax = 100\n",
        );
        for value in [
            json!("30"),
            json!(true),
            json!(null),
            json!([30]),
            json!({ "sequence": 30 }),
        ] {
            let response = json!({ "sequence": value });
            let err = assertion.check(&response).unwrap_err();
            assert!(
                err.contains("\"sequence\" to be a number within [1, 100]"),
                "{err}"
            );
        }
    }

    #[test]
    fn field_number_range_supports_fractional_bounds_and_values() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"fee\"\nmin = 0.5\nmax = 2.5\n",
        );
        assert!(assertion.check(&json!({ "fee": 0.5 })).is_ok());
        assert!(assertion.check(&json!({ "fee": 1.75 })).is_ok());
        // An integer value still compares exactly against float bounds.
        assert!(assertion.check(&json!({ "fee": 2 })).is_ok());
        let err = assertion.check(&json!({ "fee": 3.0 })).unwrap_err();
        assert!(err.contains("[0.5, 2.5]"), "{err}");
        assert!(err.contains("but found 3.0"), "{err}");
        assert!(assertion.check(&json!({ "fee": 0.25 })).is_err());
    }

    #[test]
    fn field_number_range_accepts_integer_values_against_float_bounds_exactly() {
        let assertion = range_assertion(
            "kind = \"field-number-range\"\nfield = \"protocolVersion\"\nmin = 28.0\nmax = 29.0\n",
        );
        assert!(assertion.check(&json!({ "protocolVersion": 28 })).is_ok());
        assert!(assertion.check(&json!({ "protocolVersion": 29 })).is_ok());
    }

    #[test]
    fn field_number_range_supports_open_ended_bounds() {
        let min_only =
            range_assertion("kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 1\n");
        assert!(min_only.check(&json!({ "sequence": 1_000_000 })).is_ok());
        let min_err = min_only.check(&json!({ "sequence": 0 })).unwrap_err();
        assert!(min_err.contains(">= 1"), "{min_err}");

        let max_only =
            range_assertion("kind = \"field-number-range\"\nfield = \"sequence\"\nmax = 10\n");
        assert!(max_only.check(&json!({ "sequence": 0 })).is_ok());
        let max_err = max_only.check(&json!({ "sequence": 11 })).unwrap_err();
        assert!(max_err.contains("<= 10"), "{max_err}");
    }

    #[test]
    fn field_number_range_requires_at_least_one_bound() {
        let err = range_err("kind = \"field-number-range\"\nfield = \"sequence\"\n");
        assert!(err.contains("requires \"min\" or \"max\""), "{err}");
    }

    #[test]
    fn field_number_range_rejects_min_greater_than_max() {
        let err =
            range_err("kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 10\nmax = 5\n");
        assert!(err.contains("min 10 greater than max 5"), "{err}");
    }

    #[test]
    fn field_number_range_rejects_non_numerical_bounds() {
        let err =
            range_err("kind = \"field-number-range\"\nfield = \"sequence\"\nmin = \"twenty\"\n");
        assert!(
            err.contains("field-number-range \"min\" must be a number"),
            "{err}"
        );
    }

    #[test]
    fn field_number_range_rejects_unknown_keys_in_the_entry() {
        let err = range_err(
            "kind = \"field-number-range\"\nfield = \"sequence\"\nmin = 1\nexpect = \"x\"\n",
        );
        assert!(err.contains("unknown key(s) \"expect\""), "{err}");
    }

    /// Parses one `[[assert]]` entry into a [`FieldAssertion`], the way a
    /// fixture author's entry reaches the runner.
    fn parse_entry(entry_toml: &str) -> FieldAssertion {
        RpcFixture::from_loaded(&fixture(
            "p28-rpc-assert",
            &format!("method = \"get-network\"\n\n[[assert]]\n{entry_toml}"),
        ))
        .expect("valid assertion")
        .assertions
        .remove(0)
    }

    /// The invalid-fixture message for one `[[assert]]` entry.
    fn invalid_assertion(entry_toml: &str) -> String {
        RpcFixture::from_loaded(&fixture(
            "p28-rpc-assert-bad",
            &format!("method = \"get-network\"\n\n[[assert]]\n{entry_toml}"),
        ))
        .expect_err("invalid assertion")
        .to_string()
    }

    #[test]
    fn field_one_of_passes_when_the_value_matches_any_candidate() {
        let strings = parse_entry(
            "kind = \"field-one-of\"\nfield = \"status\"\nvalues = [\"SUCCESS\", \"FAILED\"]\n",
        );
        assert!(strings.check(&json!({ "status": "SUCCESS" })).is_ok());
        assert!(strings.check(&json!({ "status": "FAILED" })).is_ok());
        assert!(strings.check(&json!({ "status": "PENDING" })).is_err());

        let integers = parse_entry(
            "kind = \"field-one-of\"\nfield = \"protocolVersion\"\nvalues = [27, 28]\n",
        );
        assert!(integers.check(&json!({ "protocolVersion": 28 })).is_ok());
        assert!(integers.check(&json!({ "protocolVersion": 26 })).is_err());

        let booleans =
            parse_entry("kind = \"field-one-of\"\nfield = \"friendbotEnabled\"\nvalues = [true]\n");
        assert!(booleans.check(&json!({ "friendbotEnabled": true })).is_ok());
        assert!(booleans
            .check(&json!({ "friendbotEnabled": false }))
            .is_err());
    }

    #[test]
    fn field_one_of_reports_the_observed_value_and_every_candidate() {
        let assertion = parse_entry(
            "kind = \"field-one-of\"\nfield = \"status\"\nvalues = [\"SUCCESS\", \"FAILED\"]\n",
        );
        let err = assertion
            .check(&json!({ "status": "PENDING" }))
            .unwrap_err();
        assert!(err.contains("\"status\""), "{err}");
        assert!(err.contains("one of"), "{err}");
        assert!(err.contains("but found \"PENDING\""), "{err}");
        // Both candidates are listed, not just the one that came closest.
        assert!(err.contains("\"SUCCESS\""), "{err}");
        assert!(err.contains("\"FAILED\""), "{err}");
    }

    #[test]
    fn field_one_of_fails_when_the_field_is_missing() {
        let assertion = parse_entry("kind = \"field-one-of\"\nfield = \"status\"\nvalues = [1]\n");
        let err = assertion.check(&json!({})).unwrap_err();
        assert!(err.contains("\"status\" to be present"), "{err}");
        // `null` is present, so this is a value mismatch, not a missing field.
        let null_err = assertion.check(&json!({ "status": null })).unwrap_err();
        assert!(null_err.contains("but found null"), "{null_err}");
    }

    #[test]
    fn field_one_of_never_coerces_between_json_types() {
        let numbers = parse_entry("kind = \"field-one-of\"\nfield = \"v\"\nvalues = [1, 2]\n");
        for value in [json!("1"), json!(true), json!(null), json!([1])] {
            assert!(
                numbers.check(&json!({ "v": value.clone() })).is_err(),
                "{value} must not equal a number"
            );
        }

        let strings = parse_entry("kind = \"field-one-of\"\nfield = \"v\"\nvalues = [\"1\"]\n");
        assert!(strings.check(&json!({ "v": 1 })).is_err());
    }

    #[test]
    fn field_one_of_compares_64_bit_integers_exactly() {
        // 2^53 + 1 is the smallest integer an `f64` cannot represent, so a
        // comparison that round-tripped through `f64` would treat it as
        // equal to 2^53.
        const TWO_POW_53: i64 = 9_007_199_254_740_992;
        const TWO_POW_53_PLUS_ONE: i64 = TWO_POW_53 + 1;
        let assertion = parse_entry(&format!(
            "kind = \"field-one-of\"\nfield = \"sequence\"\nvalues = [{TWO_POW_53_PLUS_ONE}]\n"
        ));
        assert!(assertion
            .check(&json!({ "sequence": TWO_POW_53_PLUS_ONE }))
            .is_ok());
        let err = assertion
            .check(&json!({ "sequence": TWO_POW_53 }))
            .unwrap_err();
        assert!(err.contains(&TWO_POW_53_PLUS_ONE.to_string()), "{err}");

        // The same holds at the top of the `i64` range.
        let max = parse_entry(&format!(
            "kind = \"field-one-of\"\nfield = \"sequence\"\nvalues = [{}]\n",
            i64::MAX
        ));
        assert!(max.check(&json!({ "sequence": i64::MAX })).is_ok());
        assert!(max.check(&json!({ "sequence": i64::MAX - 1 })).is_err());
    }

    #[test]
    fn field_one_of_requires_a_non_empty_values_array() {
        let missing = invalid_assertion("kind = \"field-one-of\"\nfield = \"status\"\n");
        assert!(missing.contains("requires a \"values\" array"), "{missing}");

        let empty = invalid_assertion("kind = \"field-one-of\"\nfield = \"status\"\nvalues = []\n");
        assert!(empty.contains("at least one value"), "{empty}");

        let not_an_array = invalid_assertion(
            "kind = \"field-one-of\"\nfield = \"status\"\nvalues = \"SUCCESS\"\n",
        );
        assert!(
            not_an_array.contains("requires a \"values\" array"),
            "{not_an_array}"
        );
    }

    #[test]
    fn field_one_of_requires_a_field_and_rejects_unknown_keys() {
        let missing_field = invalid_assertion("kind = \"field-one-of\"\nvalues = [\"SUCCESS\"]\n");
        assert!(
            missing_field.contains("missing \"field\""),
            "{missing_field}"
        );

        let unknown = invalid_assertion(
            "kind = \"field-one-of\"\nfield = \"status\"\nvalues = [\"SUCCESS\"]\nallow = true\n",
        );
        assert!(unknown.contains("unknown key(s) \"allow\""), "{unknown}");
    }

    #[test]
    fn integer_type_accepts_numbers_without_a_fractional_part() {
        for json_type in ["number", "integer"] {
            let assertion = parse_entry(&format!(
                "kind = \"field-type\"\nfield = \"sequence\"\nexpected_type = \"{json_type}\"\n"
            ));
            assert!(assertion.check(&json!({ "sequence": 0 })).is_ok());
            assert!(assertion.check(&json!({ "sequence": -7 })).is_ok());
            assert!(assertion.check(&json!({ "sequence": i64::MAX })).is_ok());
        }

        let integer = parse_entry(
            "kind = \"field-type\"\nfield = \"sequence\"\nexpected_type = \"integer\"\n",
        );
        // 1.0 holds no fractional part, so it is an integer value.
        assert!(integer.check(&json!({ "sequence": 1.0 })).is_ok());
        // 1.5 and a numeric string are not: nothing is coerced.
        assert!(integer.check(&json!({ "sequence": 1.5 })).is_err());
        assert!(integer.check(&json!({ "sequence": "1" })).is_err());
        assert!(integer.check(&json!({ "sequence": true })).is_err());
        assert!(integer.check(&json!({ "sequence": null })).is_err());
    }

    #[test]
    fn an_unsupported_expected_type_names_integer_as_an_option() {
        // `integer` is a supported `expected_type`; an unknown one still is
        // not, and the message now advertises `integer` as an option.
        let err = invalid_assertion(
            "kind = \"field-type\"\nfield = \"sequence\"\nexpected_type = \"int\"\n",
        );
        assert!(err.contains("unsupported json type \"int\""), "{err}");
        assert!(err.contains("\"integer\""), "{err}");
    }

    #[tokio::test]
    async fn field_one_of_passes_and_fails_against_a_live_response() {
        let pass = run_against(
            json!({ "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 28 }),
            "get-network",
            r#"
            [[assert]]
            kind = "field-one-of"
            field = "protocolVersion"
            values = [27, 28]
            "#,
        )
        .await;
        assert_eq!(pass.status, Status::Pass, "{:?}", pass.details);

        let fail = run_against(
            json!({ "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 29 }),
            "get-network",
            r#"
            [[assert]]
            kind = "field-one-of"
            field = "protocolVersion"
            values = [27, 28]
            "#,
        )
        .await;
        assert_eq!(fail.status, Status::Fail);
        let details = fail.details.expect("failure details");
        assert!(details.contains("[27, 28]"), "{details}");
        assert!(details.contains("but found 29"), "{details}");
    }

    #[tokio::test]
    async fn field_number_range_passes_a_live_ledger_sequence_within_bounds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "id": "0000000000000000000000000000000000000000000000000000000000000000",
                    "protocolVersion": 28,
                    "sequence": 100
                }
            })))
            .mount(&server)
            .await;

        let body = r#"
        method = "get-latest-ledger"

        [[assert]]
        kind = "field-number-range"
        field = "sequence"
        min = 1
        max = 200
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-range-pass", body)).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Pass);
    }

    #[tokio::test]
    async fn field_number_range_out_of_bounds_is_a_fail_with_the_range_in_details() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "id": "0000000000000000000000000000000000000000000000000000000000000000",
                    "protocolVersion": 28,
                    "sequence": 100
                }
            })))
            .mount(&server)
            .await;

        let body = r#"
        method = "get-latest-ledger"

        [[assert]]
        kind = "field-number-range"
        field = "sequence"
        min = 1
        max = 10
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-range-fail", body)).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        let result = runner.run(&fixture, &context()).await.unwrap();
        assert_eq!(result.status, Status::Fail);
        let details = result.details.expect("failure details");
        assert!(details.contains("[1, 10]"), "{details}");
        assert!(details.contains("but found 100"), "{details}");
    }

    #[test]
    fn parses_and_checks_table_and_datetime_field_equals() {
        let body = r#"
        method = "get-network"

        [[assert]]
        kind = "field-equals"
        field = "nested"
        value = { key1 = "value1", key2 = 42 }

        [[assert]]
        kind = "field-equals"
        field = "timestamp"
        value = 2023-01-01T12:00:00Z
        "#;
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-table", body)).unwrap();
        assert!(fixture.assertions.contains(&FieldAssertion::Equals {
            field: "nested".into(),
            value: json!({ "key1": "value1", "key2": 42 })
        }));
        assert!(fixture.assertions.contains(&FieldAssertion::Equals {
            field: "timestamp".into(),
            value: json!("2023-01-01T12:00:00Z")
        }));

        let assertion_nested = &fixture.assertions[0];
        let assertion_timestamp = &fixture.assertions[1];

        let match_response = json!({
            "nested": { "key1": "value1", "key2": 42 },
            "timestamp": "2023-01-01T12:00:00Z"
        });
        assert!(assertion_nested.check(&match_response).is_ok());
        assert!(assertion_timestamp.check(&match_response).is_ok());

        let mismatch_response = json!({
            "nested": { "key1": "value1", "key2": 99 },
            "timestamp": "2023-01-01T12:00:00Z"
        });
        assert!(assertion_nested.check(&mismatch_response).is_err());
        assert!(assertion_timestamp.check(&mismatch_response).is_ok());
    }

    /// Serves `result` as the JSON-RPC result and runs a fixture with the
    /// given assertion tables against it.
    async fn run_against(
        result: serde_json::Value,
        method_name: &str,
        assertions: &str,
    ) -> CompatibilityResult {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": result
            })))
            .mount(&server)
            .await;

        let body = format!("method = \"{method_name}\"\n{assertions}");
        let fixture = RpcFixture::from_loaded(&fixture("p28-rpc-raw", &body)).unwrap();
        let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
        runner.run(&fixture, &context()).await.unwrap()
    }

    #[tokio::test]
    async fn assertions_can_name_a_field_the_typed_model_does_not() {
        // `closeTime`, `headerXdr` and `metadataXdr` are real getLatestLedger
        // fields that `LatestLedger` does not model, so the old typed
        // projection dropped them before any assertion saw them.
        let result = run_against(
            json!({
                "id": "e9f5c2a1",
                "protocolVersion": 28,
                "sequence": 1234,
                "closeTime": "1700000000",
                "headerXdr": "AAAA",
                "metadataXdr": "BBBB"
            }),
            "get-latest-ledger",
            r#"
            [[assert]]
            kind = "field-equals"
            field = "closeTime"
            value = "1700000000"

            [[assert]]
            kind = "field-exists"
            field = "headerXdr"

            [[assert]]
            kind = "field-type"
            field = "metadataXdr"
            expected_type = "string"
            "#,
        )
        .await;
        assert_eq!(result.status, Status::Pass, "{:?}", result.details);
    }

    #[tokio::test]
    async fn field_absent_is_not_satisfied_by_a_field_the_typed_model_drops() {
        // On the typed projection this assertion passed for the wrong reason:
        // the model had already dropped `closeTime`.
        let result = run_against(
            json!({
                "id": "e9f5c2a1",
                "protocolVersion": 28,
                "sequence": 1234,
                "closeTime": "1700000000"
            }),
            "get-latest-ledger",
            r#"
            [[assert]]
            kind = "field-absent"
            field = "closeTime"
            "#,
        )
        .await;
        assert_eq!(result.status, Status::Fail);
        assert!(result
            .details
            .as_deref()
            .is_some_and(|d| d.contains("closeTime") && d.contains("absent")));
    }

    #[tokio::test]
    async fn a_result_that_is_not_an_object_is_an_error_not_a_failed_assertion() {
        let assertions = "[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n";
        for (result, kind) in [
            (json!([1, 2]), "an array"),
            (json!("text"), "a string"),
            (json!(28), "a number"),
            (json!(true), "a boolean"),
            (json!(null), "null"),
        ] {
            let outcome = run_against(result, "get-network", assertions).await;
            assert_eq!(outcome.status, Status::Error, "{kind}");
            let details = outcome.details.unwrap_or_default();
            assert!(details.contains("JSON object"), "{details:?}");
            assert!(details.contains(kind), "{details:?} should name {kind}");
        }
    }

    /// CA-01 section 9: the shipped `p28-*` RPC fixtures must yield the same
    /// status on the raw `result` as they did on the typed projection. The
    /// responses below are recorded shapes of a real Testnet `getNetwork`.
    #[test]
    fn shipped_p28_get_network_fixture_has_the_same_outcome_on_the_raw_result() {
        const SHIPPED: &str =
            include_str!("../../../tests/fixtures/protocol-28/rpc/p28-rpc-get-network.toml");
        let loaded =
            canary_fixtures::parse_fixture_str(SHIPPED, std::path::Path::new("shipped.toml"))
                .unwrap();
        let fixture = RpcFixture::from_loaded(&loaded).unwrap();

        let recorded = [
            (
                "protocol 28 with friendbotUrl",
                json!({
                    "friendbotUrl": "https://friendbot.stellar.org",
                    "passphrase": "Test SDF Network ; September 2015",
                    "protocolVersion": 28
                }),
                Status::Pass,
            ),
            (
                "protocol 28 without friendbotUrl",
                json!({
                    "passphrase": "Public Global Stellar Network ; September 2015",
                    "protocolVersion": 28
                }),
                Status::Pass,
            ),
            (
                "protocol 29",
                json!({
                    "passphrase": "Test SDF Network ; September 2015",
                    "protocolVersion": 29
                }),
                Status::Fail,
            ),
            (
                "an unrelated new field is ignored",
                json!({
                    "passphrase": "Test SDF Network ; September 2015",
                    "protocolVersion": 28,
                    "someNewField": 1
                }),
                Status::Pass,
            ),
        ];

        for (label, raw, expected) in recorded {
            let typed: NetworkInfo = serde_json::from_value(raw.clone()).unwrap();
            let projection = serde_json::to_value(typed).unwrap();

            let on_raw = evaluate(&fixture, &raw).0;
            let on_projection = evaluate(&fixture, &projection).0;
            assert_eq!(on_raw, on_projection, "{label}: raw and typed disagree");
            assert_eq!(on_raw, expected, "{label}");
        }
    }
}
