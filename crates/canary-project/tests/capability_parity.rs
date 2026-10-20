//! Capability parity between the Rust engine and the fixture validator
//! (E-114, issue #512).
//!
//! The engine's [`canary_core::Capability`] enum and the canonical capability
//! vector at `tests/fixtures/capabilities.json` must always describe the same
//! vocabulary: exactly the 5 capability strings approved in
//! ProtocolCanary-Fixtures#288 (F-032) — `soroban-contract`, `rpc-client`,
//! `stellar-sdk-dependency`, `wasm-artifact` and `raw-ledger-access` — which
//! is also the `CAPABILITIES` set in ProtocolCanary-Fixtures'
//! `tools/validate/validate.py` and the `required_capabilities` enum in its
//! `schemas/fixture-v1.schema.json` (CF-07 section 9).
//!
//! Run with `cargo test -p canary-project --test capability_parity`.

use std::collections::BTreeSet;

use canary_core::Capability;
use canary_project::{detect_capabilities, DetectionSignals, ProjectManifest};
use serde_json::Value;

/// The canonical capability vector, loaded from the repository fixture.
const CAPABILITIES_JSON: &str = include_str!("../../../tests/fixtures/capabilities.json");

/// The 5 capability strings approved by ProtocolCanary-Fixtures#288 (F-032).
const APPROVED_CAPABILITIES: &[&str] = &[
    "soroban-contract",
    "rpc-client",
    "stellar-sdk-dependency",
    "wasm-artifact",
    "raw-ledger-access",
];

/// Reads the canonical capability vector out of its JSON representation.
///
/// A bare JSON array of strings and an object carrying the
/// `required_capabilities` field — the exact field name the fixture schema
/// mandates — are both accepted, so the parity test keeps working whichever
/// shape ProtocolCanary-Fixtures#288 settles on.
///
/// # Panics
///
/// Panics when the file is not valid JSON, does not hold a capability array,
/// or holds an entry that is not a string; a fixture vector the test cannot
/// read must never silently compare as empty.
fn parse_capability_vector(json: &str) -> Vec<String> {
    let value: Value = serde_json::from_str(json)
        .unwrap_or_else(|error| panic!("capabilities.json is not valid JSON: {error}"));
    let vector = match value {
        Value::Array(_) => value,
        Value::Object(mut fields) => fields.remove("required_capabilities").unwrap_or_else(|| {
            panic!("capabilities.json object must carry a `required_capabilities` array")
        }),
        other => panic!("capabilities.json must be an array or an object, got {other}"),
    };
    let entries = vector
        .as_array()
        .unwrap_or_else(|| panic!("capability vector must be a JSON array, got {vector}"));
    let mut tags: Vec<String> = entries
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .unwrap_or_else(|| panic!("capability entries must be strings, got {entry}"))
                .to_string()
        })
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

/// The kebab-case wire name a capability serializes to inside fixtures.
fn capability_tag(capability: &Capability) -> String {
    serde_json::to_string(capability)
        .expect("Capability serializes to a plain string")
        .trim_matches('"')
        .to_string()
}

/// Every capability declared by the engine, as fixture-facing tags.
fn engine_tags() -> Vec<String> {
    Capability::all().iter().map(capability_tag).collect()
}

/// Compares two capability tag sets and reports every difference.
///
/// Tags present in `engine` but absent from `fixture` are *missing* from the
/// fixture vector; tags present in `fixture` but absent from `engine` are
/// *extra* — declared without a matching engine variant. Either direction
/// breaks parity, and the error names the offending tags.
fn assert_parity(engine: &[String], fixture: &[String]) -> Result<(), String> {
    let engine: BTreeSet<&str> = engine.iter().map(String::as_str).collect();
    let fixture: BTreeSet<&str> = fixture.iter().map(String::as_str).collect();

    let missing: Vec<&str> = engine.difference(&fixture).copied().collect();
    let extra: Vec<&str> = fixture.difference(&engine).copied().collect();

    if missing.is_empty() && extra.is_empty() {
        return Ok(());
    }
    Err(format!(
        "capability sets diverge: {missing:?} declared by the engine but missing from the \
         fixture vector, {extra:?} present in the fixture vector but undeclared by the engine"
    ))
}

#[test]
fn fixture_vector_holds_exactly_the_approved_capability_strings() {
    let vector = parse_capability_vector(CAPABILITIES_JSON);
    let expected: BTreeSet<String> = APPROVED_CAPABILITIES
        .iter()
        .map(|tag| (*tag).to_string())
        .collect();

    assert_eq!(
        vector.iter().cloned().collect::<BTreeSet<_>>(),
        expected,
        "tests/fixtures/capabilities.json must hold exactly the 5 approved \
         capability strings from ProtocolCanary-Fixtures#288"
    );
    assert_eq!(vector.len(), APPROVED_CAPABILITIES.len());
}

#[test]
fn engine_capability_set_matches_fixture_vector() {
    let fixture = parse_capability_vector(CAPABILITIES_JSON);

    if let Err(report) = assert_parity(&engine_tags(), &fixture) {
        panic!("{report}");
    }
}

#[test]
fn engine_serializes_to_kebab_case_fixture_tags() {
    let tags = engine_tags();

    assert_eq!(
        tags,
        vec![
            "soroban-contract",
            "rpc-client",
            "stellar-sdk-dependency",
            "wasm-artifact",
            "raw-ledger-access",
        ],
        "Capability must serialize to the kebab-case tags the fixture validator accepts"
    );
}

#[test]
fn every_capability_variant_maps_to_an_approved_tag() {
    /// Exhaustive over [`Capability`]: adding a variant fails compilation here
    /// until its fixture tag is spelled out, which points the author at the
    /// `Capability::all()` list and `tests/fixtures/capabilities.json` that
    /// must grow with it.
    fn fixture_tag(capability: &Capability) -> &'static str {
        match capability {
            Capability::SorobanContract => "soroban-contract",
            Capability::RpcClient => "rpc-client",
            Capability::StellarSdkDependency => "stellar-sdk-dependency",
            Capability::WasmArtifact => "wasm-artifact",
            Capability::RawLedgerAccess => "raw-ledger-access",
        }
    }

    for capability in Capability::all() {
        assert!(
            APPROVED_CAPABILITIES.contains(&fixture_tag(capability)),
            "{capability:?} serializes to a tag ProtocolCanary-Fixtures#288 has not approved"
        );
    }
}

#[test]
fn detector_only_emits_declared_capabilities() {
    let signals = DetectionSignals {
        manifests: vec![ProjectManifest {
            dependency_names: vec![
                "soroban-sdk".to_string(),
                "stellar-sdk".to_string(),
                "stellar-rpc-client".to_string(),
            ],
        }],
        has_stellar_toml: true,
        has_wasm_artifact: true,
    };
    let declared: BTreeSet<String> = Capability::all().iter().map(capability_tag).collect();

    for capability in detect_capabilities(&signals) {
        assert!(
            declared.contains(&capability_tag(&capability)),
            "detector emitted {capability:?}, which Capability::all() does not declare"
        );
    }
}

#[test]
fn parity_detects_a_capability_missing_from_the_fixture_vector() {
    let mut truncated = parse_capability_vector(CAPABILITIES_JSON);
    truncated.retain(|tag| tag != "raw-ledger-access");

    let report = assert_parity(&engine_tags(), &truncated)
        .expect_err("dropping a capability from the fixture vector must fail parity");
    assert!(report.contains("raw-ledger-access"));
    assert!(report.contains("missing from the fixture vector"));
}

#[test]
fn parity_detects_an_undeclared_capability_added_to_the_fixture_vector() {
    let mut extended = parse_capability_vector(CAPABILITIES_JSON);
    extended.push("fake-cap".to_string());

    let report = assert_parity(&engine_tags(), &extended)
        .expect_err("an extra capability in the fixture vector must fail parity");
    assert!(report.contains("fake-cap"));
    assert!(report.contains("undeclared by the engine"));
}

#[test]
fn parity_detects_a_synthetic_engine_capability() {
    let fixture = parse_capability_vector(CAPABILITIES_JSON);
    let mut extended = engine_tags();
    extended.push("fake-cap".to_string());

    let report = assert_parity(&extended, &fixture)
        .expect_err("an extra capability on the engine side must fail parity");
    assert!(report.contains("fake-cap"));
    assert!(report.contains("missing from the fixture vector"));
}

#[test]
fn parity_detects_an_empty_fixture_vector() {
    let report = assert_parity(&engine_tags(), &[])
        .expect_err("an empty fixture vector must fail parity against the engine");
    for tag in APPROVED_CAPABILITIES {
        assert!(
            report.contains(tag),
            "{tag} missing from the report: {report}"
        );
    }
}

#[test]
#[should_panic(expected = "`required_capabilities`")]
fn parser_rejects_an_object_without_required_capabilities() {
    parse_capability_vector(r#"{"capabilities": ["soroban-contract"]}"#);
}

#[test]
#[should_panic(expected = "must be strings")]
fn parser_rejects_non_string_entries() {
    parse_capability_vector(r#"["soroban-contract", 7]"#);
}
