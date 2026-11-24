//! Parsing and validation of an RPC fixture's `[request]` table (CA-01 §3).
//!
//! A fixture's `[request]` carries the parameters of one RPC call, keyed by
//! the upstream parameter names. Everything is checked when the fixture is
//! loaded, so a bad request is an invalid fixture, never a runtime surprise.
//! Values are data: they are validated and copied, never evaluated.
//!
//! The method set here is the closed CA-01 table. It is wider than what the
//! engine can execute today ([`crate::RpcMethod`]); validating a request for
//! a method the engine does not run yet lets a fixture author find a bad
//! request before the method is wired up.

use serde_json::{Map, Value};

/// Engine bound on `keys` (approved under D-06; upstream allows 200).
pub const MAX_KEYS: usize = 50;
/// Engine bound on `pagination.limit` (D-06).
pub const MAX_LIMIT: i64 = 50;
/// Engine bound on `filters` (D-06; upstream allows 5).
pub const MAX_FILTERS: usize = 2;
/// Largest ledger sequence number (a 32-bit unsigned integer).
const MAX_LEDGER: i64 = u32::MAX as i64;

/// The shape of one request parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParamKind {
    /// Array of non-empty base64 strings, at most [`MAX_KEYS`].
    Base64Array,
    /// 64 lowercase hex characters.
    Hash,
    /// Non-negative ledger sequence.
    Ledger,
    /// Table of `cursor` (string) and `limit` (integer).
    Pagination,
    /// Array of filter tables, at most [`MAX_FILTERS`].
    Filters,
    /// Only the string `"base64"`.
    XdrFormat,
}

struct Param {
    name: &'static str,
    kind: ParamKind,
    required: bool,
}

const fn p(name: &'static str, kind: ParamKind, required: bool) -> Param {
    Param {
        name,
        kind,
        required,
    }
}

/// Parameter list of a CA-01 method, or `None` if `method` is not in the
/// table. An empty list means the method takes no parameters.
fn parameters(method: &str) -> Option<&'static [Param]> {
    use ParamKind::*;
    const NONE: &[Param] = &[];
    const ENTRIES: &[Param] = &[
        p("keys", Base64Array, true),
        p("xdrFormat", XdrFormat, false),
    ];
    const TRANSACTION: &[Param] = &[p("hash", Hash, true), p("xdrFormat", XdrFormat, false)];
    const PAGED: &[Param] = &[
        p("startLedger", Ledger, false),
        p("pagination", Pagination, false),
        p("xdrFormat", XdrFormat, false),
    ];
    const EVENTS: &[Param] = &[
        p("startLedger", Ledger, false),
        p("endLedger", Ledger, false),
        p("filters", Filters, false),
        p("pagination", Pagination, false),
        p("xdrFormat", XdrFormat, false),
    ];
    Some(match method {
        "get-network" | "get-latest-ledger" | "get-health" | "get-version-info"
        | "get-fee-stats" => NONE,
        "get-ledger-entries" => ENTRIES,
        "get-transaction" => TRANSACTION,
        "get-ledgers" | "get-transactions" => PAGED,
        "get-events" => EVENTS,
        _ => return None,
    })
}

/// Whether `method` is in the CA-01 table.
pub fn is_known_method(method: &str) -> bool {
    parameters(method).is_some()
}

/// Validates the `[request]` table of a fixture for `method`.
///
/// `request` is the raw `[request]` value if the fixture has one. Returns the
/// parameters as the JSON object that will be sent as the call's named
/// `params`, or `None` when the method takes no parameters and none were
/// given.
///
/// # Errors
///
/// Returns a message naming the problem, suitable for an invalid-fixture
/// error: `[request]` on a parameterless method, a missing required
/// parameter, an unknown key, a wrong type, or a value out of range.
pub fn validate_request(
    method: &str,
    request: Option<&toml::Value>,
) -> Result<Option<Map<String, Value>>, String> {
    let params = parameters(method).ok_or_else(|| format!("unknown rpc method {method:?}"))?;
    let Some(request) = request else {
        if let Some(missing) = params.iter().find(|param| param.required) {
            return Err(format!(
                "method {method:?} requires a [request] table with {:?}",
                missing.name
            ));
        }
        return Ok(None);
    };
    if params.is_empty() {
        return Err(format!(
            "method {method:?} takes no parameters, so it cannot have a [request] table"
        ));
    }
    let table = request
        .as_table()
        .ok_or_else(|| "[request] must be a table".to_string())?;

    let mut unknown: Vec<&str> = table
        .keys()
        .map(String::as_str)
        .filter(|key| !params.iter().any(|param| param.name == *key))
        .collect();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        return Err(format!(
            "unknown [request] key(s) {unknown:?} for method {method:?}; its parameters are {:?}",
            params.iter().map(|param| param.name).collect::<Vec<_>>()
        ));
    }

    let mut out = Map::new();
    for param in params {
        match table.get(param.name) {
            Some(value) => {
                let json = check(param, value)
                    .map_err(|reason| format!("[request] {}: {reason}", param.name))?;
                out.insert(param.name.to_string(), json);
            }
            None if param.required => {
                return Err(format!(
                    "[request] is missing required key {:?}",
                    param.name
                ));
            }
            None => {}
        }
    }
    Ok(Some(out))
}

fn check(param: &Param, value: &toml::Value) -> Result<Value, String> {
    match param.kind {
        ParamKind::Base64Array => {
            let items = value.as_array().ok_or("must be an array of strings")?;
            if items.is_empty() {
                return Err("must not be empty".to_string());
            }
            if items.len() > MAX_KEYS {
                return Err(format!(
                    "has {} entries; the engine allows at most {MAX_KEYS}",
                    items.len()
                ));
            }
            let mut out = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let text = item
                    .as_str()
                    .ok_or_else(|| format!("entry {index} must be a string"))?;
                if !is_base64(text) {
                    return Err(format!("entry {index} is not a base64 string"));
                }
                out.push(Value::String(text.to_string()));
            }
            Ok(Value::Array(out))
        }
        ParamKind::Hash => {
            let text = value.as_str().ok_or("must be a string")?;
            let ok = text.len() == 64
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !ok {
                return Err("must be 64 lowercase hex characters".to_string());
            }
            Ok(Value::String(text.to_string()))
        }
        ParamKind::Ledger => Ok(Value::from(ledger(value)?)),
        ParamKind::Pagination => {
            let table = value.as_table().ok_or("must be a table")?;
            if let Some(key) = table.keys().find(|k| *k != "cursor" && *k != "limit") {
                return Err(format!(
                    "unknown key {key:?}; expected \"cursor\" or \"limit\""
                ));
            }
            let mut out = Map::new();
            if let Some(cursor) = table.get("cursor") {
                let text = cursor.as_str().filter(|s| !s.is_empty());
                let text = text.ok_or("cursor must be a non-empty string")?;
                out.insert("cursor".to_string(), Value::String(text.to_string()));
            }
            if let Some(limit) = table.get("limit") {
                let n = limit.as_integer().ok_or("limit must be an integer")?;
                if !(1..=MAX_LIMIT).contains(&n) {
                    return Err(format!("limit {n} is outside 1..={MAX_LIMIT}"));
                }
                out.insert("limit".to_string(), Value::from(n));
            }
            Ok(Value::Object(out))
        }
        ParamKind::Filters => {
            let items = value.as_array().ok_or("must be an array of tables")?;
            if items.len() > MAX_FILTERS {
                return Err(format!(
                    "has {} filters; the engine allows at most {MAX_FILTERS}",
                    items.len()
                ));
            }
            let mut out = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let table = item
                    .as_table()
                    .ok_or_else(|| format!("filter {index} must be a table"))?;
                out.push(filter(table).map_err(|r| format!("filter {index}: {r}"))?);
            }
            Ok(Value::Array(out))
        }
        ParamKind::XdrFormat => match value.as_str() {
            Some("base64") => Ok(Value::String("base64".to_string())),
            _ => Err("only \"base64\" is supported".to_string()),
        },
    }
}

fn ledger(value: &toml::Value) -> Result<i64, String> {
    let n = value.as_integer().ok_or("must be an integer")?;
    if !(0..=MAX_LEDGER).contains(&n) {
        return Err(format!("{n} is not a ledger sequence (0..={MAX_LEDGER})"));
    }
    Ok(n)
}

/// One `getEvents` filter: `type`, `contractIds` (strings), `topics`
/// (arrays of strings).
fn filter(table: &toml::value::Table) -> Result<Value, String> {
    if let Some(key) = table
        .keys()
        .find(|k| !["type", "contractIds", "topics"].contains(&k.as_str()))
    {
        return Err(format!("unknown key {key:?}"));
    }
    let mut out = Map::new();
    if let Some(kind) = table.get("type") {
        match kind.as_str() {
            Some(kind @ ("contract" | "system" | "diagnostic")) => {
                out.insert("type".to_string(), Value::String(kind.to_string()));
            }
            _ => return Err("type must be \"contract\", \"system\" or \"diagnostic\"".to_string()),
        }
    }
    if let Some(ids) = table.get("contractIds") {
        out.insert(
            "contractIds".to_string(),
            string_array(ids, 5, "contractIds")?,
        );
    }
    if let Some(topics) = table.get("topics") {
        let rows = topics
            .as_array()
            .ok_or("topics must be an array of arrays")?;
        if rows.len() > 5 {
            return Err("topics has more than 5 entries".to_string());
        }
        let mut converted = Vec::with_capacity(rows.len());
        for row in rows {
            converted.push(string_array(row, 5, "a topics entry")?);
        }
        out.insert("topics".to_string(), Value::Array(converted));
    }
    Ok(Value::Object(out))
}

fn string_array(value: &toml::Value, max: usize, what: &str) -> Result<Value, String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("{what} must be an array of strings"))?;
    if items.len() > max {
        return Err(format!("{what} has more than {max} entries"));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let text = item
            .as_str()
            .ok_or_else(|| format!("{what} must contain only strings"))?;
        out.push(Value::String(text.to_string()));
    }
    Ok(Value::Array(out))
}

/// Standard-alphabet base64 with `=` padding, length a multiple of four.
///
/// Shared with the typed `getLedgerEntries` client so a key is rejected by
/// the same rule whether it arrives through a fixture `[request]` or a
/// direct client call.
pub(crate) fn is_base64(text: &str) -> bool {
    if text.is_empty() || !text.len().is_multiple_of(4) {
        return false;
    }
    let body = text.trim_end_matches('=');
    text.len() - body.len() <= 2
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(text: &str) -> toml::Value {
        toml::from_str(text).unwrap()
    }

    fn run(method: &str, text: &str) -> Result<Option<Map<String, Value>>, String> {
        validate_request(method, Some(&request(text)))
    }

    #[test]
    fn contract_example_is_accepted() {
        let out = run("get-ledger-entries", "keys = [\"AAAAAA==\"]")
            .unwrap()
            .unwrap();
        assert_eq!(out["keys"], serde_json::json!(["AAAAAA=="]));
    }

    #[test]
    fn parameterless_methods_reject_a_request_table() {
        for method in [
            "get-network",
            "get-latest-ledger",
            "get-health",
            "get-fee-stats",
        ] {
            let err = run(method, "limit = 1").unwrap_err();
            assert!(err.contains("takes no parameters"), "{method}: {err}");
            assert_eq!(validate_request(method, None), Ok(None));
        }
    }

    #[test]
    fn required_parameters_must_be_present() {
        assert!(validate_request("get-ledger-entries", None).is_err());
        assert!(run("get-ledger-entries", "xdrFormat = \"base64\"").is_err());
        assert!(validate_request("get-ledgers", None).is_ok());
    }

    #[test]
    fn unknown_keys_are_rejected_not_forwarded() {
        let err = run(
            "get-transaction",
            &format!("hash = \"{}\"\nextra = 1", "a".repeat(64)),
        )
        .unwrap_err();
        assert!(err.contains("extra"), "{err}");
    }

    #[test]
    fn hash_must_be_64_lowercase_hex() {
        assert!(run(
            "get-transaction",
            &format!("hash = \"{}\"", "ab".repeat(32))
        )
        .is_ok());
        for bad in ["A".repeat(64), "a".repeat(63), "g".repeat(64)] {
            assert!(run("get-transaction", &format!("hash = \"{bad}\"")).is_err());
        }
        assert!(run("get-transaction", "hash = 7").is_err());
    }

    #[test]
    fn keys_are_bounded_and_base64() {
        let many = vec!["\"AAAA\""; MAX_KEYS + 1].join(",");
        assert!(run("get-ledger-entries", &format!("keys = [{many}]")).is_err());
        let at_limit = vec!["\"AAAA\""; MAX_KEYS].join(",");
        assert!(run("get-ledger-entries", &format!("keys = [{at_limit}]")).is_ok());
        assert!(run("get-ledger-entries", "keys = []").is_err());
        assert!(run("get-ledger-entries", "keys = [\"not base64!\"]").is_err());
        assert!(run("get-ledger-entries", "keys = [1]").is_err());
    }

    #[test]
    fn ledgers_are_non_negative_integers() {
        assert!(run("get-ledgers", "startLedger = 0").is_ok());
        assert!(run("get-ledgers", "startLedger = -1").is_err());
        assert!(run("get-ledgers", "startLedger = 1.5").is_err());
        assert!(run("get-ledgers", "startLedger = \"5\"").is_err());
        assert!(run("get-ledgers", "startLedger = 4294967296").is_err());
    }

    #[test]
    fn pagination_limit_is_within_the_engine_bound() {
        assert!(run("get-ledgers", "[pagination]\nlimit = 50").is_ok());
        assert!(run("get-ledgers", "[pagination]\nlimit = 51").is_err());
        assert!(run("get-ledgers", "[pagination]\nlimit = 0").is_err());
        assert!(run("get-ledgers", "[pagination]\ncursor = \"\"").is_err());
        assert!(run("get-ledgers", "[pagination]\nbogus = 1").is_err());
    }

    #[test]
    fn filters_are_bounded() {
        let two = "[[filters]]\ntype = \"contract\"\n[[filters]]\ntype = \"system\"";
        assert!(run("get-events", two).is_ok());
        let three = format!("{two}\n[[filters]]\ntype = \"diagnostic\"");
        assert!(run("get-events", &three).is_err());
        assert!(run("get-events", "[[filters]]\ntype = \"other\"").is_err());
    }

    #[test]
    fn xdr_format_json_is_refused() {
        assert!(run("get-ledgers", "xdrFormat = \"base64\"").is_ok());
        assert!(run("get-ledgers", "xdrFormat = \"json\"").is_err());
    }

    #[test]
    fn methods_outside_the_table_are_unknown() {
        assert!(!is_known_method("send-transaction"));
        assert!(validate_request("send-transaction", None).is_err());
    }
}
