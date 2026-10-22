//! Serde roundtrips and `Display` formatting for the shared core model
//! (`[E-112]`).
//!
//! Everything here goes through `canary_core`'s public API and its declared
//! wire format: the JSON keys, the stable lowercase/kebab-case enum
//! spellings, and the `Display` output that reporters and the CLI print.
//! A change to any of them is a change to a user-visible contract, so these
//! tests are written against literals rather than against the types
//! themselves where the spelling matters.

use std::path::PathBuf;

use canary_core::{
    CanaryError, Capability, CompatibilityResult, ExitCode, FixtureMetadata, FixtureStore,
    GitContext, NetworkContext, NetworkName, Policy, PolicyDecision, ProjectContext, ProjectType,
    ProtocolPack, ProtocolVersion, ResultSource, RunOptions, SkipCode, Status, Surface,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

/// Serializes `value`, deserializes it back, and asserts the two are equal.
fn roundtrip<T>(value: &T) -> T
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let text = serde_json::to_string(value).expect("model serializes");
    let back: T = serde_json::from_str(&text).expect("model deserializes");
    assert_eq!(&back, value, "roundtrip changed the value: {text}");
    back
}

fn fixture(id: &str, protocol: u32, surface: Surface) -> FixtureMetadata {
    FixtureMetadata {
        id: id.to_string(),
        protocol: ProtocolVersion(protocol),
        surface,
        category: "network".to_string(),
        description: "network identity".to_string(),
        source_reference: Some("CA-01".to_string()),
        required_capabilities: vec![Capability::RpcClient],
        content_digest: "00ff".to_string(),
    }
}

fn result(test_id: &str, status: Status) -> CompatibilityResult {
    CompatibilityResult {
        test_id: test_id.to_string(),
        protocol: ProtocolVersion(28),
        surface: Surface::Rpc,
        status,
        summary: "summary".to_string(),
        details: Some("details".to_string()),
        duration_ms: 12,
        fixture_id: Some("p28-rpc-001".to_string()),
        source: ResultSource::Live,
    }
}

// --- Display -------------------------------------------------------------

#[test]
fn protocol_version_displays_as_its_number() {
    assert_eq!(ProtocolVersion(0).to_string(), "0");
    assert_eq!(ProtocolVersion(28).to_string(), "28");
    assert_eq!(ProtocolVersion(u32::MAX).to_string(), "4294967295");
}

#[test]
fn surface_displays_as_its_wire_spelling() {
    for (surface, expected) in [
        (Surface::Xdr, "xdr"),
        (Surface::Rpc, "rpc"),
        (Surface::Soroban, "soroban"),
    ] {
        assert_eq!(surface.to_string(), expected);
        // Display and the serde spelling must not drift apart.
        assert_eq!(serde_json::to_value(surface).unwrap(), json!(expected));
    }
}

#[test]
fn status_displays_as_its_reporter_name() {
    for (status, expected) in [
        (Status::Pass, "pass"),
        (Status::Warning, "warning"),
        (Status::Fail, "fail"),
        (Status::Skipped, "skipped"),
        (Status::Error, "error"),
    ] {
        assert_eq!(status.to_string(), expected);
        assert_eq!(serde_json::to_value(status).unwrap(), json!(expected));
    }
}

#[test]
fn result_source_displays_like_its_report_name() {
    for (source, expected) in [
        (ResultSource::Live, "live"),
        (ResultSource::Cache, "cache"),
        (ResultSource::Unknown, "unknown"),
    ] {
        assert_eq!(source.to_string(), expected);
        assert_eq!(source.as_str(), expected);
        assert_eq!(serde_json::to_value(source).unwrap(), json!(expected));
    }
    assert_eq!(ResultSource::default(), ResultSource::Unknown);
}

#[test]
fn project_type_displays_as_its_kebab_case_spelling() {
    for (project_type, expected) in [
        (ProjectType::Soroban, "soroban"),
        (ProjectType::RpcConsumer, "rpc-consumer"),
        (ProjectType::StellarSdk, "stellar-sdk"),
        (ProjectType::GenericStellar, "generic-stellar"),
        (ProjectType::Unknown, "unknown"),
    ] {
        assert_eq!(project_type.to_string(), expected);
        assert_eq!(serde_json::to_value(project_type).unwrap(), json!(expected));
    }
}

#[test]
fn network_name_displays_its_configuration_value() {
    for (name, expected) in [
        (NetworkName::Testnet, "testnet"),
        (NetworkName::Mainnet, "mainnet"),
        (NetworkName::Futurenet, "futurenet"),
    ] {
        assert_eq!(name.to_string(), expected);
        assert_eq!(serde_json::to_value(&name).unwrap(), json!(expected));
    }
    // A custom network is displayed by its own name, not a placeholder.
    let custom = NetworkName::Custom("Standalone Network ; February 2017".to_string());
    assert_eq!(custom.to_string(), "Standalone Network ; February 2017");
}

#[test]
fn exit_and_policy_enums_display_their_documented_names() {
    assert_eq!(ExitCode::Pass.to_string(), "pass");
    assert_eq!(
        ExitCode::CompatibilityFailure.to_string(),
        "compatibility_failure"
    );
    assert_eq!(
        ExitCode::ConfigurationError.to_string(),
        "configuration_error"
    );
    assert_eq!(ExitCode::ExecutionError.to_string(), "execution_error");
    assert_eq!(ExitCode::InvalidFixture.to_string(), "invalid_fixture");
    assert_eq!(ExitCode::InternalError.to_string(), "internal_error");

    assert_eq!(PolicyDecision::Pass.to_string(), "pass");
    assert_eq!(PolicyDecision::Warning.to_string(), "warning");
    assert_eq!(PolicyDecision::Fail.to_string(), "fail");

    assert_eq!(SkipCode::Offline.to_string(), "OFFLINE");
    assert_eq!(SkipCode::parse("FUTURE_CODE"), SkipCode::Unknown);
    assert_eq!(SkipCode::parse("OFFLINE"), SkipCode::Offline);
}

#[test]
fn canary_error_display_names_its_stage_and_category() {
    let cases = [
        (
            CanaryError::Configuration("bad toml".into()),
            "configuration error: bad toml",
            ExitCode::ConfigurationError,
        ),
        (
            CanaryError::Project("ambiguous".into()),
            "project detection error: ambiguous",
            ExitCode::ConfigurationError,
        ),
        (
            CanaryError::Fixture("duplicate id".into()),
            "invalid fixture: duplicate id",
            ExitCode::InvalidFixture,
        ),
        (
            CanaryError::Xdr("bad bytes".into()),
            "xdr error: bad bytes",
            ExitCode::ExecutionError,
        ),
        (
            CanaryError::Rpc("timeout".into()),
            "rpc error: timeout",
            ExitCode::ExecutionError,
        ),
        (
            CanaryError::Soroban("simulation failed".into()),
            "soroban error: simulation failed",
            ExitCode::ExecutionError,
        ),
        (
            CanaryError::Git("no repository".into()),
            "git error: no repository",
            ExitCode::InternalError,
        ),
        (
            CanaryError::Cache("corrupt".into()),
            "cache error: corrupt",
            ExitCode::InternalError,
        ),
        (
            CanaryError::Internal("bad state".into()),
            "internal error: bad state",
            ExitCode::InternalError,
        ),
    ];
    for (error, expected, exit_code) in cases {
        assert_eq!(error.to_string(), expected);
        assert_eq!(ExitCode::from(&error), exit_code, "{error}");
    }
}

// --- Serde: scalar wrappers and enums ------------------------------------

#[test]
fn protocol_version_serializes_transparently_as_a_number() {
    assert_eq!(serde_json::to_string(&ProtocolVersion(28)).unwrap(), "28");
    let parsed: ProtocolVersion = serde_json::from_str("28").unwrap();
    assert_eq!(parsed, ProtocolVersion(28));
    roundtrip(&ProtocolVersion(u32::MAX));

    // A string is not a protocol version; nothing is coerced.
    assert!(serde_json::from_str::<ProtocolVersion>("\"28\"").is_err());
    assert!(serde_json::from_str::<ProtocolVersion>("-1").is_err());
}

#[test]
fn surface_serde_roundtrips_and_rejects_unknown_spellings() {
    for surface in [Surface::Xdr, Surface::Rpc, Surface::Soroban] {
        roundtrip(&surface);
    }
    assert!(serde_json::from_str::<Surface>("\"XDR\"").is_err());
    assert!(serde_json::from_str::<Surface>("\"chain\"").is_err());
    // Display and parsing agree.
    for surface in [Surface::Xdr, Surface::Rpc, Surface::Soroban] {
        assert_eq!(surface.to_string().parse::<Surface>(), Ok(surface));
    }
    assert!("chain".parse::<Surface>().is_err());
}

#[test]
fn status_serde_roundtrips_and_rejects_unknown_spellings() {
    for status in [
        Status::Pass,
        Status::Warning,
        Status::Fail,
        Status::Skipped,
        Status::Error,
    ] {
        roundtrip(&status);
    }
    assert!(serde_json::from_str::<Status>("\"PASS\"").is_err());
    assert!(serde_json::from_str::<Status>("\"unknown\"").is_err());
}

#[test]
fn project_type_and_capability_use_kebab_case_keys() {
    for project_type in [
        ProjectType::Soroban,
        ProjectType::RpcConsumer,
        ProjectType::StellarSdk,
        ProjectType::GenericStellar,
        ProjectType::Unknown,
    ] {
        roundtrip(&project_type);
    }
    assert_eq!(
        serde_json::to_value(Capability::StellarSdkDependency).unwrap(),
        json!("stellar-sdk-dependency")
    );
    assert_eq!(
        serde_json::to_value(Capability::RawLedgerAccess).unwrap(),
        json!("raw-ledger-access")
    );
    for capability in Capability::all() {
        let text = serde_json::to_string(capability).unwrap();
        let back: Capability = serde_json::from_str(&text).unwrap();
        assert_eq!(&back, capability);
        // Every declared capability has a kebab-case wire name.
        assert!(text.contains('-'), "{text} should be kebab-case");
    }
}

#[test]
fn network_name_roundtrips_including_custom_networks() {
    for name in [
        NetworkName::Testnet,
        NetworkName::Mainnet,
        NetworkName::Futurenet,
        NetworkName::Custom("local".to_string()),
        // A custom name that collides with a well-known one still round-trips
        // as a custom network: the variant, not the string, decides.
        NetworkName::Custom("testnet".to_string()),
    ] {
        roundtrip(&name);
    }
    assert_eq!(
        serde_json::to_value(NetworkName::Custom("local".to_string())).unwrap(),
        json!({ "custom": "local" })
    );
}

// --- Serde: structs ------------------------------------------------------

#[test]
fn project_context_roundtrips_with_a_path_root() {
    let context = ProjectContext {
        root: PathBuf::from("examples/soroban"),
        name: "example".to_string(),
        project_type: ProjectType::Soroban,
        capabilities: vec![Capability::SorobanContract, Capability::WasmArtifact],
    };
    let back = roundtrip(&context);
    assert_eq!(back.root, PathBuf::from("examples/soroban"));
    assert!(back.has_capability(&Capability::SorobanContract));
    assert!(!back.has_capability(&Capability::RpcClient));
}

#[test]
fn network_context_roundtrips() {
    let context = NetworkContext {
        name: NetworkName::Testnet,
        rpc_url: "https://soroban-testnet.stellar.org".to_string(),
        passphrase: "Test SDF Network ; September 2015".to_string(),
        observed_protocol: Some(ProtocolVersion(28)),
    };
    let back = roundtrip(&context);
    assert_eq!(back.observed_protocol, Some(ProtocolVersion(28)));

    let unobserved = NetworkContext {
        observed_protocol: None,
        ..context
    };
    let back = roundtrip(&unobserved);
    assert_eq!(back.observed_protocol, None);
}

#[test]
fn git_context_roundtrips_with_missing_metadata() {
    let unknown = GitContext::default();
    assert_eq!(unknown.commit, None);
    let back = roundtrip(&unknown);
    assert_eq!(back, GitContext::default());
    assert_eq!(
        serde_json::to_value(&unknown).unwrap(),
        json!({ "commit": null, "branch": null, "is_dirty": null })
    );

    let known = GitContext {
        commit: Some("9b3fefc".to_string()),
        branch: Some("main".to_string()),
        is_dirty: Some(false),
    };
    assert_eq!(roundtrip(&known), known);
}

#[test]
fn run_options_roundtrip_and_keep_documented_defaults_for_legacy_payloads() {
    let options = RunOptions::default();
    assert_eq!(options.max_concurrency, 4);
    assert_eq!(options.rpc_max_retries, 3);
    assert_eq!(options.rpc_retry_delay_ms, 500);
    roundtrip(&options);
    roundtrip(&RunOptions {
        verbose: true,
        quiet: true,
        max_concurrency: 1,
        rpc_timeout: 30,
        rpc_max_retries: 0,
        rpc_retry_delay_ms: 0,
    });

    // A payload written before the retry fields existed deserializes to the
    // documented defaults rather than failing.
    let legacy = json!({
        "verbose": false,
        "quiet": false,
        "max_concurrency": 2,
        "rpc_timeout": 5
    });
    let back: RunOptions = serde_json::from_value(legacy).expect("legacy payload still parses");
    assert_eq!(back.max_concurrency, 2);
    assert_eq!(back.rpc_max_retries, 3);
    assert_eq!(back.rpc_retry_delay_ms, 500);
}

#[test]
fn fixture_metadata_roundtrips_and_defaults_a_missing_digest() {
    let metadata = fixture("p28-rpc-001", 28, Surface::Rpc);
    assert_eq!(roundtrip(&metadata), metadata);

    let mut without_digest = serde_json::to_value(&metadata).unwrap();
    without_digest
        .as_object_mut()
        .unwrap()
        .remove("content_digest");
    let back: FixtureMetadata = serde_json::from_value(without_digest).unwrap();
    assert_eq!(back.content_digest, "");
    assert_eq!(back.id, "p28-rpc-001");
}

#[test]
fn compatibility_result_roundtrips_and_defaults_a_missing_source() {
    let live = result("t1", Status::Pass);
    assert_eq!(roundtrip(&live), live);

    // A report written before `source` existed reads back as unknown, which
    // is explicitly not a claim that it was live.
    let mut without_source = serde_json::to_value(&live).unwrap();
    without_source.as_object_mut().unwrap().remove("source");
    let back: CompatibilityResult = serde_json::from_value(without_source).unwrap();
    assert_eq!(back.source, ResultSource::Unknown);
    assert_eq!(back.details.as_deref(), Some("details"));
}

#[test]
fn compatibility_result_classifies_each_status() {
    for (status, is_failure) in [
        (Status::Pass, false),
        (Status::Warning, false),
        (Status::Skipped, false),
        (Status::Fail, true),
        (Status::Error, true),
    ] {
        let outcome = result("t1", status);
        assert_eq!(
            outcome.is_required_failure(),
            is_failure,
            "{status:?} classified incorrectly"
        );
    }
}

#[test]
fn fixture_store_roundtrips_and_filters() {
    let store = FixtureStore::new(vec![
        fixture("p28-xdr-001", 28, Surface::Xdr),
        fixture("p28-rpc-001", 28, Surface::Rpc),
        fixture("p27-rpc-001", 27, Surface::Rpc),
    ]);
    assert_eq!(store.len(), 3);
    assert!(!store.is_empty());

    let back = roundtrip(&store);
    assert_eq!(back.all().len(), 3);
    assert_eq!(back.for_surface(Surface::Rpc).count(), 2);
    assert_eq!(back.for_protocol(ProtocolVersion(28)).count(), 2);
    assert_eq!(back.for_protocol(ProtocolVersion(29)).count(), 0);
    assert_eq!(
        back.by_id("p27-rpc-001").map(|meta| meta.protocol),
        Some(ProtocolVersion(27))
    );
    assert!(back.by_id("missing").is_none());

    let empty = FixtureStore::default();
    assert!(empty.is_empty());
    assert_eq!(roundtrip(&empty).len(), 0);
}

#[test]
fn protocol_pack_roundtrips() {
    let pack = ProtocolPack {
        version: ProtocolVersion(28),
        fixtures: vec![
            fixture("p28-xdr-001", 28, Surface::Xdr),
            fixture("p28-rpc-001", 28, Surface::Rpc),
        ],
    };
    let back = roundtrip(&pack);
    assert_eq!(back.version, ProtocolVersion(28));
    assert_eq!(back.fixtures.len(), 2);
}

#[test]
fn policy_types_roundtrip() {
    roundtrip(&Policy::default());
    roundtrip(&Policy {
        warnings_are_failures: true,
    });
    for decision in [
        PolicyDecision::Pass,
        PolicyDecision::Warning,
        PolicyDecision::Fail,
    ] {
        roundtrip(&decision);
        // The wire spelling matches Display.
        assert_eq!(
            serde_json::to_value(decision).unwrap(),
            json!(decision.to_string())
        );
    }
    assert!(serde_json::from_str::<PolicyDecision>("\"unknown\"").is_err());
}

#[test]
fn a_malformed_model_payload_is_an_error_not_a_panic() {
    // Wrong types and out-of-range values are reported by serde; they never
    // panic and never silently coerce.
    let cases: &[(&str, Value)] = &[
        (
            "protocol as a string",
            json!({
                "test_id": "t1",
                "protocol": "28",
                "surface": "rpc",
                "status": "pass",
                "summary": "",
                "details": null,
                "duration_ms": 0,
                "fixture_id": null
            }),
        ),
        (
            "negative duration",
            json!({
                "test_id": "t1",
                "protocol": 28,
                "surface": "rpc",
                "status": "pass",
                "summary": "",
                "details": null,
                "duration_ms": -1,
                "fixture_id": null
            }),
        ),
        (
            "unknown status",
            json!({
                "test_id": "t1",
                "protocol": 28,
                "surface": "rpc",
                "status": "exploded",
                "summary": "",
                "details": null,
                "duration_ms": 0,
                "fixture_id": null
            }),
        ),
    ];
    for (case, payload) in cases {
        let back = serde_json::from_value::<CompatibilityResult>(payload.clone());
        assert!(back.is_err(), "{case} must not deserialize");
    }
}
