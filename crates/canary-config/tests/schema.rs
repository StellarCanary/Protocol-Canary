//! Conformance tests for the published configuration schema (E-047, issue #455).
//!
//! `docs/schemas/config-v1.schema.json` is the formal JSON Schema (draft
//! 2020-12) for a `.stellar-canary.toml` file. A TOML document maps onto JSON
//! without renaming anything, so these tests feed the schema real configuration
//! text: the example files in `examples/`, the example documented in
//! `README.md`, and hand-written cases for each rule version 1 states. Both
//! directions are pinned — every configuration this project ships validates,
//! and a file that breaks version 1 is rejected: an unknown key or section, a
//! wrong type, `protocol = 0`, a disabled-everything `[tests]`, an `ftp://` RPC
//! endpoint.
//!
//! The schema is read *as data* by the interpreter in
//! `tests/support/schema_check.rs`, which implements only the keywords the
//! document uses (asserted by
//! [`the_schema_uses_only_keywords_the_test_validator_interprets]), so these
//! checks exercise the published file rather than a parallel re-statement of
//! it. `pattern` is the one keyword carrying syntax of its own: the matcher in
//! `tests/support/schema_pattern.rs` implements anchored literals and character
//! classes with `+ * ?` repetitions,
//! [`every_pattern_is_anchored_and_inside_the_supported_subset`] fails if a
//! schema pattern steps outside that subset, and
//! [`the_published_patterns_accept_and_reject_the_documented_shapes`] runs the
//! three published patterns against the values the descriptions name.
//!
//! A consumer that validates a configuration with a real JSON Schema validator
//! should see the verdict these tests reach for the documented shapes; the
//! patterns are written to be no wider than the file's grammar. The stricter
//! relationship between this schema and the Rust loader itself is the subject
//! of `tests/schema_parity.rs` (issue #456), which is where the shapes pinned
//! here and the values the loader stores verbatim are reconciled.

use serde_json::Value;

use canary_config::{ConfigFile, DEFAULT_PROTOCOL, SUPPORTED_CONFIG_VERSION};

#[path = "support/schema_pattern.rs"]
mod schema_pattern;

#[path = "support/schema_check.rs"]
mod schema_check;

const SCHEMA: &str = include_str!("../../../docs/schemas/config-v1.schema.json");
const README: &str = include_str!("../../../README.md");
const EXAMPLE_MINIMAL: &str = include_str!("../../../examples/minimal/.stellar-canary.toml");
const EXAMPLE_CONFIGURATION: &str =
    include_str!("../../../examples/configuration/.stellar-canary.toml");

/// The marker that opens a configuration example `README.md` documents as a
/// version 1 file, so the published schema is tested against the text a user
/// actually copies rather than against a paraphrase of it.
const README_EXAMPLE_MARKER: &str = "<!-- config-example: v1 -->";

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the published schema is valid JSON")
}

/// Validates configuration text against the published schema.
fn validate_toml(text: &str) -> Result<(), String> {
    schema_check::validate_toml_with(text, &schema())
}

/// Validates an in-memory instance against the published schema, for the
/// documents that are Rust values rather than files.
fn validate_instance(instance: &Value) -> Result<(), String> {
    schema_check::validate(instance, &schema(), "$")
}

/// The document every optional setting filled in: what `ConfigFile` looks like
/// when nothing is left to the defaults.
fn full_config() -> ConfigFile {
    use canary_config::{PolicySection, ProjectSection, ProjectTypeSetting, TestsSection};
    use std::path::PathBuf;

    ConfigFile {
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
            warnings_are_failures: false,
        },
    }
}

/// The fenced TOML blocks under `README_EXAMPLE_MARKER` in `README.md`.
fn readme_examples() -> Vec<String> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = README.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].trim() != README_EXAMPLE_MARKER {
            index += 1;
            continue;
        }
        index += 1;
        while index < lines.len() && !lines[index].starts_with("```toml") {
            index += 1;
        }
        assert!(
            index < lines.len(),
            "a `{README_EXAMPLE_MARKER}` marker must be followed by a ```toml block"
        );
        let mut body = String::new();
        for line in lines[index + 1..].iter() {
            if line.starts_with("```") {
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
        blocks.push(body);
        index += 1;
    }
    blocks
}

/// Configuration text for one value of one top-level string setting.
fn config_with(key: &str, value: &str) -> String {
    format!("version = 1\nprotocol = 28\n{key} = '{value}'\n")
}

#[test]
fn the_published_schema_is_draft_2020_12_for_configuration_version_1() {
    let schema = schema();
    assert_eq!(
        schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
        "the published schema targets draft 2020-12"
    );
    assert!(schema["$id"].is_string(), "the schema is identified");
    assert!(
        schema["title"]
            .as_str()
            .is_some_and(|t| t.contains("version 1")),
        "the schema says which configuration version it documents"
    );
    assert_eq!(schema["type"], "object");
    assert_eq!(
        schema["additionalProperties"],
        Value::Bool(false),
        "a key the loader does not know must not be accepted"
    );
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
        "only the settings the loader cannot default may be required"
    );

    for section in ["project", "tests", "policy"] {
        let node = &schema["properties"][section];
        assert_eq!(node["type"], "object", "`[{section}]` is a table");
        assert_eq!(
            node["additionalProperties"],
            Value::Bool(false),
            "`{section}` rejects unknown keys, as `deny_unknown_fields` does"
        );
    }
}

#[test]
fn the_schema_uses_only_keywords_the_test_validator_interprets() {
    // Walks schema positions only: each value of `properties`, each `not`, and
    // a schema-valued `additionalProperties` are themselves schemas, while key
    // names, enums and required lists are data.
    fn walk(node: &Value) {
        let Some(map) = node.as_object() else {
            return;
        };
        for (keyword, value) in map {
            assert!(
                schema_check::SUPPORTED_KEYWORDS.contains(&keyword.as_str()),
                "the schema uses keyword `{keyword}` these suites do not interpret; \
                 teach tests/support/schema_check.rs about it (and extend these tests)"
            );
            match keyword.as_str() {
                "properties" => value
                    .as_object()
                    .into_iter()
                    .flat_map(|properties| properties.values())
                    .for_each(walk),
                "not" | "items" => walk(value),
                "additionalProperties" if value.is_object() => walk(value),
                _ => {}
            }
        }
    }
    walk(&schema());
}

#[test]
fn every_pattern_is_anchored_and_inside_the_supported_subset() {
    let mut patterns = Vec::new();
    schema_pattern::collect_patterns(&schema(), &mut patterns);
    assert_eq!(
        patterns.len(),
        3,
        "the three run settings are the string shapes this schema pins"
    );
    for text in &patterns {
        let pattern = schema_pattern::parse(text)
            .unwrap_or_else(|error| panic!("the schema pattern `{text}` cannot be read: {error}"));
        assert!(
            pattern.is_anchored(),
            "the schema pattern `{text}` is not anchored at both ends, so it would constrain a \
             substring rather than the whole value"
        );
    }
}

#[test]
fn the_published_patterns_accept_and_reject_the_documented_shapes() {
    // Exercises the matcher against the values the schema descriptions name,
    // so a pattern that silently stops matching anything cannot pass the
    // structural tests above by accident.
    let schema = schema();
    let cases: [(&str, &[&str], &[&str]); 3] = [
        (
            "network",
            &["testnet", "mainnet", "futurenet", "stg-2.canary.dev"],
            &["", " testnet", "test net", "_leading", "net!"],
        ),
        (
            "rpc_url",
            &[
                "https://soroban-testnet.stellar.org",
                "http://localhost:8000/soroban/rpc?key=x",
                "https://example.org/f?x=1#frag",
            ],
            &[
                "",
                "ftp://example.org/rpc",
                "https://",
                "soroban-testnet.stellar.org",
                "https://example.org has a space",
            ],
        ),
        (
            "fixtures_dir",
            &["fixtures", "tests/fixtures", r"fixtures\pack", "p 28"],
            &["", "/etc/fixtures", r"C:\fixtures", " leading"],
        ),
    ];
    for (key, accepted, rejected) in cases {
        let text = schema["properties"][key]["pattern"]
            .as_str()
            .unwrap_or_else(|| panic!("`{key}` must publish a pattern"))
            .to_string();
        for value in accepted {
            assert!(
                schema_pattern::matches(&text, value)
                    .unwrap_or_else(|error| panic!("the published pattern must parse: {error}")),
                "`{key} = \"{value}\"` is documented as valid, but the published pattern rejects it"
            );
        }
        for value in rejected {
            assert!(
                !schema_pattern::matches(&text, value).expect("the published pattern parses"),
                "`{key} = \"{value}\"` breaks the documented shape, but the published pattern \
                 accepts it"
            );
        }
    }
}

#[test]
fn a_default_or_fully_populated_configuration_validates() {
    let default = serde_json::to_value(ConfigFile::default()).expect("a config has a JSON form");
    validate_instance(&default).expect("the built-in defaults must validate");

    let full = serde_json::to_value(full_config()).expect("a config has a JSON form");
    validate_instance(&full).expect("a fully populated configuration must validate");
}

#[test]
fn the_example_configuration_files_validate() {
    for (name, text) in [
        ("examples/minimal/.stellar-canary.toml", EXAMPLE_MINIMAL),
        (
            "examples/configuration/.stellar-canary.toml",
            EXAMPLE_CONFIGURATION,
        ),
    ] {
        validate_toml(text).unwrap_or_else(|error| panic!("{name} rejected: {error}\n{text}"));
    }
}

#[test]
fn the_readme_examples_validate_and_load() {
    let blocks = readme_examples();
    assert!(
        !blocks.is_empty(),
        "README.md must mark its configuration example with `{README_EXAMPLE_MARKER}` so the \
         published schema is tested against the text users copy"
    );
    for block in blocks {
        validate_toml(&block)
            .unwrap_or_else(|error| panic!("documented example rejected: {error}"));
        let _: ConfigFile =
            canary_config::parse_config(&block, std::path::Path::new(".stellar-canary.toml"))
                .expect("a documented example must also load");
    }
}

#[test]
fn optional_settings_and_sections_may_be_absent() {
    validate_toml("version = 1\nprotocol = 28\n").expect("the two required settings alone work");
    validate_toml("version = 1\nprotocol = 29\n\n[tests]\nrpc = false\n")
        .expect("a partial [tests] works; its other keys default to true");
    validate_toml("version = 1\nprotocol = 28\n\n[project]\n").expect("an empty table is allowed");
}

#[test]
fn an_unknown_top_level_key_is_rejected() {
    // Issue #455 sketched a `[runner]` table and timeout integers;
    // configuration version 1 has neither, and the loader exits 2 on them, so
    // a schema that accepted one would validate a file the CLI refuses.
    for (key, text) in [
        ("timeout", "version = 1\nprotocol = 28\ntimeout = 30\n"),
        (
            "networks",
            "version = 1\nprotocol = 28\nnetworks = \"testnet\"\n",
        ),
    ] {
        let error = validate_toml(text).expect_err("`{key}` is not a key of version 1");
        assert!(
            error.starts_with(&format!("$.{key} is not")),
            "the rejection names the unknown root key: {error}"
        );
    }
}

#[test]
fn an_unknown_top_level_section_is_rejected() {
    for section in ["network", "fixtures", "runner", "test"] {
        let text = format!("version = 1\nprotocol = 28\n\n[{section}]\nname = \"x\"\n");
        let error = validate_toml(&text).expect_err("`[{section}]` is not a section of version 1");
        assert!(error.contains(&format!("$.{section}")), "{text}: {error}");
    }
}

#[test]
fn an_unknown_key_inside_a_section_is_rejected() {
    let error = validate_toml("version = 1\nprotocol = 28\n\n[tests]\ncache = true\n")
        .expect_err("`[tests]` has three keys");
    assert!(error.contains("$.tests.cache"), "{error}");

    let error =
        validate_toml("version = 1\nprotocol = 28\n\n[policy]\nwarnings_are_failure = true\n")
            .expect_err("the key is `warnings_are_failures`");
    assert!(error.contains("$.policy"), "{error}");
}

#[test]
fn a_section_written_as_an_array_of_tables_is_rejected() {
    let error = validate_toml("version = 1\nprotocol = 28\n\n[[tests]]\nxdr = true\n")
        .expect_err("`[[tests]]` is an array, not a table");
    assert!(error.contains("$.tests"), "{error}");
}

#[test]
fn a_missing_required_setting_is_rejected() {
    let error = validate_toml("version = 1\n").expect_err("`protocol` is required");
    assert!(error.contains("protocol"), "{error}");

    let error = validate_toml("protocol = 28\n").expect_err("`version` is required");
    assert!(error.contains("version"), "{error}");

    for text in ["", "# nothing here\n"] {
        let error = validate_toml(text).expect_err("an empty file is not a configuration");
        assert!(error.contains("version"), "{error}");
    }
}

#[test]
fn a_malformed_toml_document_is_rejected() {
    for text in [
        "this is not valid toml [[[\n",
        "version = 1\nversion = 1\nprotocol = 28\n",
    ] {
        let error = validate_toml(text).expect_err("the schema validates documents, not debris");
        assert!(error.contains("TOML"), "{text}: {error}");
    }
}

#[test]
fn a_version_other_than_one_is_rejected() {
    for (key, text) in [
        ("version = 2", "version = 2\nprotocol = 28\n"),
        ("version = 0", "version = 0\nprotocol = 28\n"),
        ("version = \"1\"", "version = \"1\"\nprotocol = 28\n"),
    ] {
        let error = validate_toml(text).expect_err("this schema documents version 1 only");
        assert!(error.contains("$.version"), "{key}: {error}");
    }
}

#[test]
fn a_protocol_outside_the_documented_range_is_rejected() {
    for text in [
        "version = 1\nprotocol = 0\n",
        "version = 1\nprotocol = -1\n",
        "version = 1\nprotocol = \"28\"\n",
        "version = 1\nprotocol = 28.5\n",
        "version = 1\nprotocol = 4294967296\n",
    ] {
        let error = validate_toml(text)
            .expect_err("protocol is a positive integer the loader reads as u32");
        assert!(error.contains("$.protocol"), "{text}: {error}");
    }

    for protocol in [1_u64, 20, 22, 28, 29, 30, 4294967295] {
        let text = format!("version = 1\nprotocol = {protocol}\n");
        validate_toml(&text).unwrap_or_else(|error| {
            panic!("{text} names a value inside the documented range: {error}")
        });
    }
}

#[test]
fn a_project_type_outside_the_closed_set_is_rejected() {
    let error =
        validate_toml("version = 1\nprotocol = 28\n\n[project]\ntype = \"smart-contract\"\n")
            .expect_err("`[project].type` is a closed set");
    assert!(error.contains("$.project.type"), "{error}");

    for project_type in [
        "auto",
        "soroban",
        "rpc-consumer",
        "stellar-sdk",
        "generic-stellar",
        "unknown",
    ] {
        let text = format!("version = 1\nprotocol = 28\n\n[project]\ntype = \"{project_type}\"\n");
        validate_toml(&text).unwrap_or_else(|error| {
            panic!("`{project_type}` is a documented project type: {error}")
        });
    }
}

#[test]
fn a_configuration_with_no_surface_enabled_is_rejected() {
    // The loader's cross-field rule, published as the schema's one `not`.
    let error = validate_toml(
        "version = 1\nprotocol = 28\n\n[tests]\nxdr = false\nrpc = false\nsoroban = false\n",
    )
    .expect_err("a run with nothing to check is a configuration error");
    assert!(error.contains("$.tests"), "{error}");
    assert!(error.contains("`not`"), "{error}");

    validate_toml("version = 1\nprotocol = 28\n\n[tests]\nxdr = false\nrpc = false\n")
        .expect("leaving one key absent keeps its default of true, so a surface remains");
}

#[test]
fn a_wrong_type_for_a_setting_is_rejected() {
    for (text, path) in [
        (
            "version = 1\nprotocol = 28\n\n[policy]\nwarnings_are_failures = \"yes\"\n",
            "$.policy.warnings_are_failures",
        ),
        (
            "version = 1\nprotocol = 28\n\n[tests]\nxdr = 1\n",
            "$.tests.xdr",
        ),
        (
            "version = 1\nprotocol = 28\nnetwork = [\"testnet\"]\n",
            "$.network",
        ),
        (
            "version = 1\nprotocol = 28\nproject = \"soroban\"\n",
            "$.project",
        ),
    ] {
        let error = validate_toml(text).expect_err("the declared type is the only one accepted");
        assert!(error.contains(path), "{text}: {error}");
    }
}

#[test]
fn a_date_time_value_is_rejected() {
    // The loader refuses a datetime for every key; the converter these tests
    // use renders one as JSON null so that the schema refuses it too.
    let error = validate_toml("version = 1\nprotocol = 28\nnetwork = 1979-05-27T07:32:00Z\n")
        .expect_err("no version 1 key is a date");
    assert!(error.contains("$.network"), "{error}");
}

#[test]
fn an_rpc_url_with_an_unsupported_scheme_is_rejected() {
    // The negative test issue #455 asks for by name.
    for url in [
        "ftp://example.org/rpc",
        "ws://example.org/rpc",
        "file:///srv/rpc",
        "example.org/rpc",
        "https://example.org has a space",
        "https://rpc.example.org/<script>",
    ] {
        let error = validate_toml(&config_with("rpc_url", url))
            .expect_err("only an http(s) endpoint is a configuration value");
        assert!(error.contains("$.rpc_url"), "{url}: {error}");
    }

    for url in [
        "https://soroban-testnet.stellar.org",
        "http://localhost:8000/soroban/rpc",
        "https://rpc.example.org/v2?network=futurenet",
        "https://user:pass@example.org:443/rpc",
    ] {
        validate_toml(&config_with("rpc_url", url))
            .unwrap_or_else(|error| panic!("{url} is a documented endpoint shape: {error}"));
    }
}

#[test]
fn an_rpc_url_without_an_authority_is_rejected() {
    for url in ["https://", "http://", "https:/soroban-testnet.stellar.org"] {
        let error = validate_toml(&config_with("rpc_url", url))
            .expect_err("a bare scheme names no endpoint");
        assert!(error.contains("$.rpc_url"), "{url}: {error}");
    }
}

#[test]
fn a_network_name_outside_the_documented_shape_is_rejected() {
    for value in ["", "test net", " testnet", "mainnet!", "futurenet/2"] {
        let error =
            validate_toml(&config_with("network", value)).expect_err("a network name is one token");
        assert!(error.contains("$.network"), "{value:?}: {error}");
    }

    for value in ["testnet", "mainnet", "futurenet", "canary-stg.2"] {
        validate_toml(&config_with("network", value))
            .unwrap_or_else(|error| panic!("{value:?} is a network name: {error}"));
    }
}

#[test]
fn an_absolute_or_empty_fixtures_dir_is_rejected() {
    // Values go into TOML literal strings, so a backslash means a backslash
    // rather than an escape the file never intended.
    for value in [
        "",
        "/srv/fixtures",
        r"\\server\fixtures",
        r"C:\fixtures",
        " leading",
    ] {
        let error = validate_toml(&config_with("fixtures_dir", value))
            .expect_err("fixtures_dir is documented as a path relative to the project root");
        assert!(error.contains("$.fixtures_dir"), "{value:?}: {error}");
    }

    for value in [
        "fixtures",
        "tests/fixtures",
        r"fixtures\pack",
        "p 28 packs",
        ".fixtures",
    ] {
        validate_toml(&config_with("fixtures_dir", value))
            .unwrap_or_else(|error| panic!("{value:?} is a relative path: {error}"));
    }
}
