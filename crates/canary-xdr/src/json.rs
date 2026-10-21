//! Semantic comparison of a decoded value's JSON against an expected JSON
//! value, for `decode-json-equals` fixtures.
//!
//! The comparison is exact structural equality on parsed JSON, not on text:
//! object key order and whitespace do not matter, while every value, every
//! array element and its position, every key, and every JSON type does. A
//! missing key, an extra key, a different array length, an integer written
//! as a float (`1` vs `1.0`), or a number written as a string (`"1"` vs
//! `1`) is a difference. Nothing is normalized away.
//!
//! Each difference names its location as a JSON Pointer (RFC 6901), so keys
//! containing `/` or `~` stay unambiguous: `/` is written `~1` and `~` is
//! written `~0`. The empty pointer `""` is the whole document.

use std::fmt;

use serde_json::Value;

/// Most differences [`format_differences`] lists before summarizing the rest.
pub const MAX_LISTED_DIFFERENCES: usize = 20;

/// Longest rendering of a single value in a diagnostic, in characters.
const MAX_VALUE_CHARS: usize = 120;

/// One way the actual JSON differs from the expected JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonDifference {
    /// JSON Pointer (RFC 6901) to the differing location.
    pub path: String,
    pub kind: JsonDifferenceKind,
}

/// What differs at a [`JsonDifference::path`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonDifferenceKind {
    /// Both sides have a value of the same JSON type, but not the same value.
    ValueMismatch { expected: Value, actual: Value },
    /// The two sides have different JSON types.
    TypeMismatch { expected: Value, actual: Value },
    /// The expected object has a key the actual object lacks.
    MissingField { expected: Value },
    /// The actual object has a key the expected object lacks.
    UnexpectedField { actual: Value },
    /// The arrays differ in length. Elements up to the shorter length are
    /// compared as well; each surplus element is reported on its own as
    /// [`JsonDifferenceKind::MissingElement`] or
    /// [`JsonDifferenceKind::UnexpectedElement`].
    LengthMismatch { expected: usize, actual: usize },
    /// The expected array has an element at this index; the actual one
    /// ends before it.
    MissingElement { expected: Value },
    /// The actual array has an element at this index; the expected one
    /// ends before it.
    UnexpectedElement { actual: Value },
}

impl fmt::Display for JsonDifference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = if self.path.is_empty() {
            "(root)"
        } else {
            &self.path
        };
        match &self.kind {
            JsonDifferenceKind::ValueMismatch { expected, actual } => write!(
                f,
                "{path}: expected {}, found {}",
                render(expected),
                render(actual)
            ),
            JsonDifferenceKind::TypeMismatch { expected, actual } => write!(
                f,
                "{path}: expected {} {}, found {} {}",
                type_name(expected),
                render(expected),
                type_name(actual),
                render(actual)
            ),
            JsonDifferenceKind::MissingField { expected } => {
                write!(f, "{path}: missing field (expected {})", render(expected))
            }
            JsonDifferenceKind::UnexpectedField { actual } => {
                write!(f, "{path}: unexpected field (found {})", render(actual))
            }
            JsonDifferenceKind::LengthMismatch { expected, actual } => write!(
                f,
                "{path}: expected an array of {expected} element(s), found {actual}"
            ),
            JsonDifferenceKind::MissingElement { expected } => {
                write!(f, "{path}: missing element (expected {})", render(expected))
            }
            JsonDifferenceKind::UnexpectedElement { actual } => {
                write!(f, "{path}: unexpected element (found {})", render(actual))
            }
        }
    }
}

/// Every difference between `expected` and `actual`, in a deterministic
/// order: depth first, object keys in sorted order, array elements by
/// index. An empty result means the two values are equal.
pub fn compare_json(expected: &Value, actual: &Value) -> Vec<JsonDifference> {
    let mut differences = Vec::new();
    let mut path = String::new();
    compare_at(&mut path, expected, actual, &mut differences);
    differences
}

/// Renders `differences` one per line, listing at most
/// [`MAX_LISTED_DIFFERENCES`] and summarizing the rest.
pub fn format_differences(differences: &[JsonDifference]) -> String {
    let mut lines: Vec<String> = differences
        .iter()
        .take(MAX_LISTED_DIFFERENCES)
        .map(ToString::to_string)
        .collect();
    if differences.len() > MAX_LISTED_DIFFERENCES {
        lines.push(format!(
            "... and {} more difference(s)",
            differences.len() - MAX_LISTED_DIFFERENCES
        ));
    }
    lines.join("\n")
}

fn compare_at(path: &mut String, expected: &Value, actual: &Value, out: &mut Vec<JsonDifference>) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            let mut keys: Vec<&String> = expected.keys().chain(actual.keys()).collect();
            keys.sort_unstable();
            keys.dedup();
            for key in keys {
                with_segment(path, key, |path| {
                    match (expected.get(key.as_str()), actual.get(key.as_str())) {
                        (Some(e), Some(a)) => compare_at(path, e, a, out),
                        (Some(e), None) => out.push(difference(
                            path,
                            JsonDifferenceKind::MissingField {
                                expected: e.clone(),
                            },
                        )),
                        (None, Some(a)) => out.push(difference(
                            path,
                            JsonDifferenceKind::UnexpectedField { actual: a.clone() },
                        )),
                        (None, None) => {}
                    }
                });
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            if expected.len() != actual.len() {
                out.push(difference(
                    path,
                    JsonDifferenceKind::LengthMismatch {
                        expected: expected.len(),
                        actual: actual.len(),
                    },
                ));
            }
            for index in 0..expected.len().max(actual.len()) {
                with_segment(path, &index.to_string(), |path| {
                    match (expected.get(index), actual.get(index)) {
                        (Some(e), Some(a)) => compare_at(path, e, a, out),
                        (Some(e), None) => out.push(difference(
                            path,
                            JsonDifferenceKind::MissingElement {
                                expected: e.clone(),
                            },
                        )),
                        (None, Some(a)) => out.push(difference(
                            path,
                            JsonDifferenceKind::UnexpectedElement { actual: a.clone() },
                        )),
                        (None, None) => {}
                    }
                });
            }
        }
        _ if type_name(expected) != type_name(actual) => out.push(difference(
            path,
            JsonDifferenceKind::TypeMismatch {
                expected: expected.clone(),
                actual: actual.clone(),
            },
        )),
        _ if expected != actual => out.push(difference(
            path,
            JsonDifferenceKind::ValueMismatch {
                expected: expected.clone(),
                actual: actual.clone(),
            },
        )),
        _ => {}
    }
}

fn difference(path: &str, kind: JsonDifferenceKind) -> JsonDifference {
    JsonDifference {
        path: path.to_string(),
        kind,
    }
}

/// Runs `f` with `segment` appended to `path` as one escaped JSON Pointer
/// reference token, then restores `path`.
fn with_segment(path: &mut String, segment: &str, f: impl FnOnce(&mut String)) {
    let len = path.len();
    path.push('/');
    for c in segment.chars() {
        match c {
            '~' => path.push_str("~0"),
            '/' => path.push_str("~1"),
            c => path.push(c),
        }
    }
    f(path);
    path.truncate(len);
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Compact JSON for `value`, shortened to [`MAX_VALUE_CHARS`] characters.
fn render(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() <= MAX_VALUE_CHARS {
        return text;
    }
    let mut short: String = text.chars().take(MAX_VALUE_CHARS).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pointer_segments_escape_tilde_before_slash() {
        let differences = compare_json(&json!({ "a/b~c": 1 }), &json!({ "a/b~c": 2 }));
        assert_eq!(differences[0].path, "/a~1b~0c");
    }

    #[test]
    fn root_difference_is_labelled() {
        let differences = compare_json(&json!(1), &json!(2));
        assert_eq!(differences[0].path, "");
        assert_eq!(differences[0].to_string(), "(root): expected 1, found 2");
    }

    #[test]
    fn long_values_are_shortened_in_messages() {
        let long = "x".repeat(500);
        let differences = compare_json(&json!(long), &json!("y"));
        let message = differences[0].to_string();
        assert!(message.contains('…'), "{message}");
        assert!(message.len() < 300, "{message}");
    }

    #[test]
    fn format_differences_caps_the_list() {
        let expected: Vec<u32> = (0..30).collect();
        let actual: Vec<u32> = (100..130).collect();
        let differences = compare_json(&json!(expected), &json!(actual));
        assert_eq!(differences.len(), 30);
        let text = format_differences(&differences);
        assert_eq!(text.lines().count(), MAX_LISTED_DIFFERENCES + 1);
        assert!(text.ends_with("... and 10 more difference(s)"), "{text}");
    }
}
