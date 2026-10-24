//! Schema parity between the published configuration schema and the Rust
//! loader (issue #456 / E-048).
//!
//! `docs/schemas/config-v1.schema.json` is the formal JSON Schema (draft
//! 2020-12) for `.stellar-canary.toml`; `crates/canary-config` is its
//! implementation. This suite pins the two together so neither can drift:
//! every key the loader's structs carry is declared by the schema, every key
//! the schema declares is known to the loader, and across the corpus below no
//! document the schema accepts is one the loader refuses.
//!
//! One direction is deliberately asymmetric. Issue #455 pinned the *shapes* of
//! the three optional run settings — `network`, `rpc_url`, `fixtures_dir` — so
//! an editor or a CI job reports a malformed endpoint before a run is spent,
//! while the loader keeps storing those values verbatim, because a bad stored
//! value must not stop an explicit `--network`/`--rpc-url`/`--fixtures-dir`
//! from winning (issue #474). The schema is therefore the stricter side at
//! exactly those documents, and [`DOCUMENTED_DIVERGENCES`] is the list of them:
//! a case that is not on the list diverging, a listed case that stops
//! diverging, or the schema ever being the *more accepting* side fails
//! [`the_loader_and_the_schema_reach_the_same_decision_for_every_case`].
//!
//! The validator in `tests/support/schema_check.rs`, shared with
//! `tests/schema.rs` so the published file has exactly one interpreter here,
//! reads the schema *as data* using only the keywords the document contains
//! (asserted by
//! [`the_schema_uses_only_keywords_the_test_validator_interprets`]), so these
//! checks exercise that file rather than a parallel re-statement of it. TOML
//! documents are converted to the JSON instance model first; the one lossy step
//! is a date-time value, which no key of this file accepts and which the
//! converter therefore renders as JSON null, so that the schema refuses it for
//! the same reason the loader does.
//!
//! Two tests mutate the schema on purpose. They are the negative direction the
//! issue asks for: they show that a loader field the schema has not caught up
//! with, or a schema key the loader never heard of, is reported as drift
//! instead of being silently tolerated.

use std::path::{Path, PathBuf};

use canary_config::{
    parse_config, ConfigFile, PolicySection, ProjectSection, ProjectTypeSetting, TestsSection,
    DEFAULT_PROTOCOL, SUPPORTED_CONFIG_VERSION,
};
use serde_json::{json, Value};

/// The `pattern` matcher these suites share with `tests/schema.rs`: the
/// published schema constrains string shapes with it, so parity cannot be
/// checked without interpreting it.
#[path = "support/schema_pattern.rs"]
mod schema_pattern;

/// The keyword interpreter these suites share with `tests/schema.rs`, so that
/// one implementation reads the published document.
#[path = "support/schema_check.rs"]
mod schema_check;

const SCHEMA: &str = include_str!("../../../docs/schemas/config-v1.schema.json");

/// The name used in diagnostics only: nothing here touches the filesystem.
const PATH: &str = ".stellar-canary.toml";

/// The corpus cases on which the schema is allowed to be the stricter side,
/// each with the reason it is on the list: the shapes are issue #455's, the
/// loader's verbatim storage is issue #474's. A case not named here may not
/// diverge, and a case named here that stops diverging fails the corpus test —
/// so this list cannot quietly grow into a way of excusing drift.
const DOCUMENTED_DIVERGENCES: &[(&str, &str)] = &[
    (
        "rpc_url the runtime cannot use",
        "an ftp endpoint is invalid configuration to an editor, and only a \
         runtime failure to the loader",
    ),
    (
        "rpc_url with no authority",
        "a bare scheme names no endpoint, and the loader stores it anyway",
    ),
    (
        "network name the runtime cannot use",
        "a name with a space is unusable, and the loader stores it anyway",
    ),
    (
        "fixtures_dir given as an absolute path",
        "the file documents a path relative to the project root; an absolute \
         one works through --fixtures-dir",
    ),
];

/// Why the schema may be the stricter side for this corpus case, if it may.
fn documented_divergence(name: &str) -> Option<&'static str> {
    DOCUMENTED_DIVERGENCES
        .iter()
        .find(|(case, _)| *case == name)
        .map(|(_, reason)| *reason)
}

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the published schema is valid JSON")
}

/// What one side of the contract decided about one document.
enum Outcome {
    Accepted,
    Rejected(String),
}

impl Outcome {
    fn accepted(&self) -> bool {
        matches!(self, Outcome::Accepted)
    }

    fn describe(&self) -> String {
        match self {
            Outcome::Accepted => "accept".to_string(),
            Outcome::Rejected(reason) => format!("reject ({reason})"),
        }
    }
}

fn rust_outcome(text: &str) -> Outcome {
    match parse_config(text, Path::new(PATH)) {
        Ok(_) => Outcome::Accepted,
        Err(error) => Outcome::Rejected(error.to_string()),
    }
}

fn schema_outcome(text: &str, schema: &Value) -> Outcome {
    match schema_check::validate_toml_with(text, schema) {
        Ok(()) => Outcome::Accepted,
        Err(reason) => Outcome::Rejected(reason),
    }
}
/// Why one corpus case breaks parity, or `None` when the two sides agree or
/// differ only in the one documented way.
///
/// The schema may never be the more accepting side: a document the loader
/// refuses has to be refused here too, or the file promises a shape the CLI
/// does not implement. The reverse — schema refuses, loader accepts — is
/// allowed only for the cases in [`DOCUMENTED_DIVERGENCES`].
fn parity_miss(name: &str, text: &str, schema: &Value) -> Option<String> {
    let rust = rust_outcome(text);
    let model = schema_outcome(text, schema);
    if rust.accepted() == model.accepted() {
        return None;
    }
    if rust.accepted() && documented_divergence(name).is_some() {
        return None;
    }
    let difference = if rust.accepted() {
        "the loader accepts it and the schema does not, which is only allowed for a documented \
         divergence"
            .to_string()
    } else {
        "the schema accepts a document the loader refuses".to_string()
    };
    Some(format!(
        "{name}: the loader would {} but the schema would {}; {difference}\ndocument:\n{text}",
        rust.describe(),
        model.describe()
    ))
}

/// One key of one table: the table it belongs to, its name, and the TOML text
/// of its value. An empty table name is the document root.
type Item = (String, String, String);

/// The tables in document order, so a generated document reads like a file.
const TABLES: [&str; 4] = ["", "project", "tests", "policy"];

fn item(table: &str, key: &str, value: &str) -> Item {
    (table.to_string(), key.to_string(), value.to_string())
}

/// A configuration carrying every key the loader knows, each with a value the
/// loader accepts.
fn full_config() -> Vec<Item> {
    vec![
        item("", "version", "1"),
        item("", "protocol", "28"),
        item("", "network", "\"testnet\""),
        item("", "rpc_url", "\"https://soroban-testnet.stellar.org\""),
        item("", "fixtures_dir", "\"fixtures\""),
        item("project", "type", "\"auto\""),
        item("tests", "xdr", "true"),
        item("tests", "rpc", "true"),
        item("tests", "soroban", "true"),
        item("policy", "warnings_are_failures", "false"),
    ]
}

/// Renders items as a TOML document: `skip` drops one key, `replace` gives one
/// key new text, and `added` contributes extra keys. A table whose keys all
/// disappear loses its header, which is what a hand-written file does when it
/// leaves the section out.
fn render(
    entries: &[Item],
    skip: Option<(&str, &str)>,
    replace: Option<(&str, &str, &str)>,
    added: &[Item],
) -> String {
    let mut out = String::from("# written by crates/canary-config/tests/schema_parity.rs\n");
    for table in TABLES {
        let mut lines = Vec::new();
        for (owner, key, value) in entries.iter().chain(added.iter()) {
            if owner != table {
                continue;
            }
            if skip == Some((table, key)) {
                continue;
            }
            let written = match replace {
                Some((at, target, text)) if at == table && target == key => text,
                _ => value,
            };
            lines.push(format!("{key} = {written}\n"));
        }
        if lines.is_empty() {
            continue;
        }
        if !table.is_empty() {
            out.push_str(&format!("\n[{table}]\n"));
        }
        for line in lines {
            out.push_str(&line);
        }
    }
    out
}

/// A value of a different type than the one it replaces, for the generated
/// wrong-type cases.
fn mistyped(value: &str) -> String {
    if value.starts_with('"') {
        "1".to_string()
    } else if value == "true" || value == "false" {
        "\"yes\"".to_string()
    } else {
        "\"text\"".to_string()
    }
}

/// The corpus: hand-written documents covering the loader's own rules, plus
/// every single-key mutation of the fully-populated document (one omitted, one
/// mistyped per key, one unknown key per table).
fn corpus() -> Vec<(String, String)> {
    let full = full_config();
    let mut cases: Vec<(String, String)> = Vec::new();

    // The documented shape, and the files a real project writes.
    cases.push((
        "fully-populated".to_string(),
        render(&full, None, None, &[]),
    ));
    for (name, text) in [
        ("minimal", "version = 1\nprotocol = 28\n"),
        (
            "one surface off",
            "version = 1\nprotocol = 28\n\n[tests]\nxdr = false\n",
        ),
        (
            "one surface on",
            "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n",
        ),
        (
            "custom network and fixture directory",
            "version = 1\nprotocol = 22\nnetwork = \"mine\"\nfixtures_dir = \"my-fixtures\"\n",
        ),
        (
            "project table with nothing in it",
            "version = 1\nprotocol = 28\n\n[project]\n",
        ),
        // The four documented divergences: the loader stores these values
        // verbatim, while the schema pins the shape issue #455 documents. Each
        // name is in DOCUMENTED_DIVERGENCES, which is what keeps them honest.
        (
            "rpc_url the runtime cannot use",
            "version = 1\nprotocol = 28\nrpc_url = \"ftp://rpc.example.invalid\"\n",
        ),
        (
            "rpc_url with no authority",
            "version = 1\nprotocol = 28\nrpc_url = \"https://\"\n",
        ),
        (
            "network name the runtime cannot use",
            "version = 1\nprotocol = 28\nnetwork = \"main net\"\n",
        ),
        (
            "fixtures_dir given as an absolute path",
            "version = 1\nprotocol = 28\nfixtures_dir = \"/srv/fixtures\"\n",
        ),
    ] {
        cases.push((name.to_string(), text.to_string()));
    }
    for project_type in [
        "auto",
        "soroban",
        "rpc-consumer",
        "stellar-sdk",
        "generic-stellar",
        "unknown",
    ] {
        cases.push((
            format!("project type {project_type}"),
            render(
                &full,
                None,
                Some(("project", "type", &format!("\"{project_type}\""))),
                &[],
            ),
        ));
    }

    // Rejections that come from the loader's rules rather than the file shape.
    for (name, text) in [
        ("unsupported version", "version = 2\nprotocol = 28\n"),
        ("zero version", "version = 0\nprotocol = 28\n"),
        ("zero protocol", "version = 1\nprotocol = 0\n"),
        ("protocol above u32", "version = 1\nprotocol = 4294967296\n"),
        ("negative protocol", "version = 1\nprotocol = -1\n"),
        (
            "every surface off",
            "version = 1\nprotocol = 28\n\n[tests]\nxdr = false\nrpc = false\nsoroban = false\n",
        ),
        (
            "project type outside the set",
            "version = 1\nprotocol = 28\n\n[project]\ntype = \"smart-contract\"\n",
        ),
    ] {
        cases.push((name.to_string(), text.to_string()));
    }

    // Rejections that come from the file shape.
    for (name, text) in [
        (
            "misspelled table",
            "version = 1\nprotocol = 28\n\n[test]\nrpc = false\n",
        ),
        (
            "misspelled top-level key",
            "version = 1\nprotocol = 28\nnetworks = \"mainnet\"\n",
        ),
        (
            "misspelled key in a table",
            "version = 1\nprotocol = 28\n\n[policy]\nwarnings_are_failure = true\n",
        ),
        ("empty file", ""),
        ("comment-only file", "# nothing here\n"),
        ("malformed toml", "this is not valid toml [[[\n"),
        ("duplicate key", "version = 1\nversion = 1\nprotocol = 28\n"),
        (
            "date-time value",
            "version = 1\nprotocol = 28\nnetwork = 1979-05-27T07:32:00Z\n",
        ),
        (
            "array value for a scalar key",
            "version = 1\nprotocol = 28\nnetwork = [\"testnet\"]\n",
        ),
        (
            "inline table for a scalar key",
            "version = 1\nprotocol = 28\nnetwork = { name = \"testnet\" }\n",
        ),
        (
            "table where a scalar belongs",
            "version = 1\nprotocol = 28\n\n[network]\nname = \"testnet\"\n",
        ),
    ] {
        cases.push((name.to_string(), text.to_string()));
    }

    // Every declared key, mutated.
    for entry in &full {
        let (table, key, value) = (&entry.0, &entry.1, &entry.2);
        cases.push((
            format!("omitted {table:?}/{key}"),
            render(&full, Some((table, key)), None, &[]),
        ));
        cases.push((
            format!("mistyped {table:?}/{key}"),
            render(&full, None, Some((table, key, &mistyped(value))), &[]),
        ));
    }
    for table in TABLES {
        cases.push((
            format!("unknown key in {table:?}"),
            render(
                &full,
                None,
                None,
                &[item(table, "an_unmapped_key", "\"a value\"")],
            ),
        ));
    }

    cases
}

#[test]
fn the_published_schema_is_draft_2020_12_for_configuration_version_1() {
    let schema = schema();
    assert_eq!(
        schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
        "the published schema targets draft 2020-12"
    );
    assert!(schema["$id"].is_string(), "the schema is identified");
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["version"]["const"], 1);

    let required = schema["required"]
        .as_array()
        .expect("required is an array")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<&str>>();
    assert_eq!(
        required,
        vec!["version", "protocol"],
        "only the keys the loader has no default for may be required"
    );
}

#[test]
fn the_schema_uses_only_keywords_the_test_validator_interprets() {
    // Walks schema positions only: each value of `properties`, the single
    // `not`, and a schema-valued `additionalProperties` are themselves
    // schemas, while key names, enums and required lists are data.
    fn walk(node: &Value) {
        let Some(map) = node.as_object() else {
            return;
        };
        for (keyword, value) in map {
            assert!(
                schema_check::SUPPORTED_KEYWORDS.contains(&keyword.as_str()),
                "the schema uses keyword `{keyword}` these suites do not interpret; \
                 teach tests/support/schema_check.rs about it (and extend the corpus)"
            );
            match keyword.as_str() {
                "properties" => value
                    .as_object()
                    .into_iter()
                    .flat_map(|properties| properties.values())
                    .for_each(walk),
                "not" => walk(value),
                "additionalProperties" if value.is_object() => walk(value),
                _ => {}
            }
        }
    }
    walk(&schema());
}

/// The schema node describing one table; the root document is `""`.
fn schema_table<'a>(schema: &'a Value, table: &str) -> &'a Value {
    if table.is_empty() {
        return schema;
    }
    schema
        .get("properties")
        .and_then(|properties| properties.get(table))
        .unwrap_or_else(|| panic!("the schema declares no `{table}` table"))
}

fn declared_keys(node: &Value) -> Vec<String> {
    let mut keys: Vec<String> = node
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

/// The keys of an instance object, for the Rust side of the comparison: a
/// serialized `ConfigFile` has no `properties` keyword, it *is* the object.
fn object_keys(node: &Value) -> Vec<String> {
    let mut keys: Vec<String> = node
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

/// The key tree of the Rust structs, read from the serializable form of a
/// fully-populated `ConfigFile` rather than restated by hand.
fn rust_keys() -> Vec<(String, Vec<String>)> {
    let populated = ConfigFile {
        version: SUPPORTED_CONFIG_VERSION,
        protocol: DEFAULT_PROTOCOL,
        network: Some("testnet".to_string()),
        rpc_url: Some("https://soroban-testnet.stellar.org".to_string()),
        fixtures_dir: Some(PathBuf::from("fixtures")),
        project: ProjectSection {
            project_type: ProjectTypeSetting::Auto,
        },
        tests: TestsSection::default(),
        policy: PolicySection {
            warnings_are_failures: true,
        },
    };
    let value = serde_json::to_value(&populated).expect("a configuration always serializes");
    TABLES
        .iter()
        .map(|table| {
            let node = if table.is_empty() {
                &value
            } else {
                value
                    .get(*table)
                    .unwrap_or_else(|| panic!("ConfigFile carries a `{table}` table"))
            };
            ((*table).to_string(), object_keys(node))
        })
        .collect()
}

#[test]
fn every_key_of_the_rust_structs_is_declared_by_the_schema() {
    let schema = schema();
    let full = full_config();
    for (table, keys) in rust_keys() {
        assert_eq!(
            declared_keys(schema_table(&schema, &table)),
            keys,
            "the keys of {table:?} must match the schema's declarations exactly",
        );
        for key in &keys {
            let leaf = full
                .iter()
                .any(|(owner, declared, _)| *owner == table && *declared == *key);
            // A table key of the root is exercised through its own members.
            let table_key = full.iter().any(|(owner, _, _)| owner == key);
            assert!(
                leaf || table_key,
                "the corpus must exercise {table:?}.{key}",
            );
        }
    }
}

#[test]
fn every_table_the_schema_declares_is_closed_against_unknown_keys() {
    // Parity with `#[serde(deny_unknown_fields)]`: an open table would accept
    // the misspelling the loader refuses.
    let schema = schema();
    for table in TABLES {
        assert_eq!(
            schema_table(&schema, table)["additionalProperties"],
            Value::Bool(false),
            "the table {table:?} must reject keys the loader does not know",
        );
    }
}

#[test]
fn the_loader_and_the_schema_reach_the_same_decision_for_every_case() {
    let cases = corpus();
    let mut misses = Vec::new();
    let mut accepted = 0;
    for (name, text) in &cases {
        if let Some(difference) = parity_miss(name, text, &schema()) {
            misses.push(difference);
        }
        if rust_outcome(text).accepted() {
            accepted += 1;
        }
    }
    assert!(
        misses.is_empty(),
        "{} of {} cases disagree:\n{}",
        misses.len(),
        cases.len(),
        misses.join("\n\n")
    );
    // Guards against the corpus degenerating into one direction only: a suite
    // that only rejects proves nothing about what either side accepts.
    assert!(
        cases.len() >= 45,
        "the corpus is {} cases; it must exercise every declared key",
        cases.len()
    );
    assert!(
        accepted >= 12 && accepted * 2 <= cases.len(),
        "the corpus has {accepted} accepted and {} rejected cases; it needs both",
        cases.len() - accepted
    );
}

#[test]
fn required_lists_match_the_keys_the_loader_cannot_omit() {
    let schema = schema();
    let full = full_config();
    for table in TABLES {
        let mut loader_requires = Vec::new();
        for (owner, key, _) in full.iter().filter(|(owner, _, _)| owner == table) {
            let text = render(&full, Some((owner, key)), None, &[]);
            if !rust_outcome(&text).accepted() {
                loader_requires.push(key.clone());
            }
        }
        let required: Vec<String> = schema_table(&schema, table)
            .get("required")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            required, loader_requires,
            "the schema's `required` for {table:?} must be the keys the loader has no default for",
        );
    }
}

#[test]
fn the_schemas_documented_defaults_are_the_loaders_defaults() {
    // `default` is an annotation this validator ignores, so compare it against
    // the values `ConfigFile::default()` actually produces.
    let schema = schema();
    let default = ConfigFile::default();
    for (path, expected) in [
        ("tests.xdr", default.tests.xdr),
        ("tests.rpc", default.tests.rpc),
        ("tests.soroban", default.tests.soroban),
        (
            "policy.warnings_are_failures",
            default.policy.warnings_are_failures,
        ),
    ] {
        let mut node = &schema;
        for part in path.split('.') {
            node = &node["properties"][part];
        }
        assert_eq!(
            node["default"],
            Value::Bool(expected),
            "the schema documents the wrong default for {path}",
        );
    }
    assert_eq!(
        schema["properties"]["project"]["properties"]["type"]["default"],
        Value::String("auto".to_string()),
        "`auto` is what an absent [project].type means",
    );
}

#[test]
fn a_loader_field_the_schema_has_not_caught_up_with_breaks_parity() {
    // The issue's negative direction. `rpc_url` stands in for a key a future
    // change adds to `ConfigFile`: dropping it from the schema must make the
    // checks report the drift rather than tolerate it.
    let mut mutated = schema();
    mutated
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("the root table declares properties")
        .remove("rpc_url")
        .expect("the schema declares rpc_url");

    let text = render(&full_config(), None, None, &[]);
    assert!(
        rust_outcome(&text).accepted(),
        "the loader accepts the document it was written for",
    );
    let miss = parity_miss("drift probe", &text, &mutated)
        .expect("an undeclared loader key must show up as a parity miss");
    assert!(miss.contains("rpc_url"), "{miss}");

    let declared = declared_keys(&mutated);
    let from_rust = rust_keys()
        .into_iter()
        .find(|(table, _)| table.is_empty())
        .map(|(_, keys)| keys)
        .expect("the root table is compared");
    assert_eq!(
        from_rust
            .iter()
            .filter(|key| !declared.contains(key))
            .cloned()
            .collect::<Vec<String>>(),
        vec!["rpc_url".to_string()],
        "the structural comparison names the key that drifted",
    );
}

#[test]
fn a_schema_key_the_loader_does_not_know_breaks_parity() {
    // The other direction: a schema that grows a key the structs never gained
    // must be caught by the accept-or-reject corpus, since the loader refuses
    // it with `deny_unknown_fields`.
    let mut mutated = schema();
    mutated["properties"]["adopted_a_future_key"] = json!({ "type": "string" });

    let text = "version = 1\nprotocol = 28\nadopted_a_future_key = \"yes\"\n";
    assert!(
        !rust_outcome(text).accepted(),
        "the loader refuses a key it does not have",
    );
    let miss = parity_miss("drift probe", text, &mutated)
        .expect("a schema key the loader lacks must show up as a parity miss");
    assert!(miss.contains("adopted_a_future_key"), "{miss}");
}

#[test]
fn every_pattern_is_anchored_and_inside_the_supported_subset() {
    // `pattern` is the one keyword here that carries syntax of its own, and
    // these suites implement a deliberately narrow subset of it (see
    // `support/schema_pattern.rs`). A schema pattern that steps outside that
    // subset would either fail to parse or match substrings instead of whole
    // values, silently making the schema the more accepting side.
    let mut patterns = Vec::new();
    schema_pattern::collect_patterns(&schema(), &mut patterns);
    assert!(
        !patterns.is_empty(),
        "the schema constrains string shapes with `pattern`, so this guard must have \
         something to check"
    );
    for text in &patterns {
        let pattern = schema_pattern::parse(text)
            .unwrap_or_else(|error| panic!("the schema pattern `{text}` cannot be read: {error}"));
        assert!(
            pattern.is_anchored(),
            "the schema pattern `{text}` is not anchored at both ends, so it constrains a \
             substring rather than a whole value"
        );
    }
}

#[test]
fn every_documented_divergence_still_diverges() {
    // The allowlist in DOCUMENTED_DIVERGENCES is only honest while each case
    // really does diverge. If the loader ever starts validating these shapes,
    // or the constraints are loosened, the entry must go — otherwise the list
    // becomes a permanent excuse for the two sides to drift apart.
    let schema = schema();
    let cases = corpus();
    for (name, reason) in DOCUMENTED_DIVERGENCES {
        let (_, text) = cases
            .iter()
            .find(|(case, _)| case == name)
            .unwrap_or_else(|| panic!("the corpus must contain the case {name:?}"));
        assert!(
            rust_outcome(text).accepted(),
            "{name:?}: the loader no longer accepts it, so it is not a divergence \
             ({reason})",
        );
        assert!(
            !schema_outcome(text, &schema).accepted(),
            "{name:?}: the schema no longer refuses it, so remove it from \
             DOCUMENTED_DIVERGENCES or restore the constraint ({reason})",
        );
    }
}
