use canary_core::{
    CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext, NetworkName,
    ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_xdr::{DefaultXdrRunner, XdrError, XdrFixture, XdrRunner};
use stellar_xdr::{Limits, StellarValue, WriteXdr};

fn valid_stellar_value_base64() -> String {
    StellarValue::default()
        .to_xdr_base64(Limits::none())
        .unwrap()
}

fn context() -> ExecutionContext {
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
        fixtures: FixtureStore::default(),
        git: GitContext::default(),
        cache: CacheStore::new(std::env::temp_dir().join("canary-xdr-json-tests-cache")),
        options: RunOptions::default(),
    }
}

fn parse_xdr_fixture(toml: &str) -> Result<XdrFixture, XdrError> {
    let loaded = parse_fixture_str(toml, std::path::Path::new("test.toml")).unwrap();
    XdrFixture::from_loaded(&loaded)
}

#[test]
fn decoded_xdr_matching_expected_json_passes() {
    let base64 = valid_stellar_value_base64();
    let toml = format!(
        r#"
id = "p28-xdr-json-match"
protocol = 28
surface = "xdr"
category = "test"
description = "test"
type = "StellarValue"
kind = "decode-json-equals"
value_base64 = "{base64}"

[expected_json]
tx_set_hash = "0000000000000000000000000000000000000000000000000000000000000000"
close_time = "0"
upgrades = []
ext = "basic"
"#
    );

    let fixture = parse_xdr_fixture(&toml).unwrap();
    let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
    assert_eq!(result.status, Status::Pass);
}

#[test]
fn nested_object_mismatched_field_value_fails_with_path_diagnostic() {
    let base64 = valid_stellar_value_base64();
    let toml = format!(
        r#"
id = "p28-xdr-json-mismatch"
protocol = 28
surface = "xdr"
category = "test"
description = "test"
type = "StellarValue"
kind = "decode-json-equals"
value_base64 = "{base64}"

[expected_json]
tx_set_hash = "0000000000000000000000000000000000000000000000000000000000000000"
close_time = "99999"
upgrades = []
ext = "basic"
"#
    );

    let fixture = parse_xdr_fixture(&toml).unwrap();
    let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
    assert_eq!(result.status, Status::Fail);
    let details = result.details.expect("details must be present");
    assert!(
        details.contains("at $.close_time"),
        "diagnostic must identify JSON path $.close_time: {details}"
    );
    assert!(
        details.contains("expected \"99999\", found \"0\""),
        "diagnostic must state expected value: {details}"
    );
}

#[test]
fn missing_expected_field_fails_with_diagnostic() {
    let base64 = valid_stellar_value_base64();
    let toml = format!(
        r#"
id = "p28-xdr-missing-field"
protocol = 28
surface = "xdr"
category = "test"
description = "test"
type = "StellarValue"
kind = "decode-json-equals"
value_base64 = "{base64}"

[expected_json]
tx_set_hash = "0000000000000000000000000000000000000000000000000000000000000000"
close_time = "0"
upgrades = []
ext = "basic"
nonexistent_field = "expected_something"
"#
    );

    let fixture = parse_xdr_fixture(&toml).unwrap();
    let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
    assert_eq!(result.status, Status::Fail);
    let details = result.details.expect("details must be present");
    assert!(
        details.contains("at $.nonexistent_field: missing expected field"),
        "diagnostic must report missing field: {details}"
    );
}

#[test]
fn unexpected_actual_field_fails_with_diagnostic() {
    let base64 = valid_stellar_value_base64();
    // expected_json omits `upgrades`
    let toml = format!(
        r#"
id = "p28-xdr-unexpected-field"
protocol = 28
surface = "xdr"
category = "test"
description = "test"
type = "StellarValue"
kind = "decode-json-equals"
value_base64 = "{base64}"

[expected_json]
tx_set_hash = "0000000000000000000000000000000000000000000000000000000000000000"
close_time = "0"
ext = "basic"
"#
    );

    let fixture = parse_xdr_fixture(&toml).unwrap();
    let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
    assert_eq!(result.status, Status::Fail);
    let details = result.details.expect("details must be present");
    assert!(
        details.contains("at $.upgrades: unexpected field in actual value"),
        "diagnostic must report unexpected field: {details}"
    );
}

#[test]
fn type_mismatch_fails_with_diagnostic() {
    let base64 = valid_stellar_value_base64();
    let toml = format!(
        r#"
id = "p28-xdr-type-mismatch"
protocol = 28
surface = "xdr"
category = "test"
description = "test"
type = "StellarValue"
kind = "decode-json-equals"
value_base64 = "{base64}"

[expected_json]
tx_set_hash = "0000000000000000000000000000000000000000000000000000000000000000"
close_time = 0
upgrades = []
ext = "basic"
"#
    );

    let fixture = parse_xdr_fixture(&toml).unwrap();
    let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
    assert_eq!(result.status, Status::Fail);
    let details = result.details.expect("details must be present");
    assert!(
        details.contains("at $.close_time: type mismatch (expected number, found string)"),
        "diagnostic must report type mismatch: {details}"
    );
}

#[test]
fn object_key_order_difference_compares_equal() {
    let base64 = valid_stellar_value_base64();
    // Reversed key order in TOML vs default struct field order
    let toml = format!(
        r#"
id = "p28-xdr-key-order"
protocol = 28
surface = "xdr"
category = "test"
description = "test"
type = "StellarValue"
kind = "decode-json-equals"
value_base64 = "{base64}"

[expected_json]
ext = "basic"
upgrades = []
close_time = "0"
tx_set_hash = "0000000000000000000000000000000000000000000000000000000000000000"
"#
    );

    let fixture = parse_xdr_fixture(&toml).unwrap();
    let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
    assert_eq!(result.status, Status::Pass);
}

#[test]
fn missing_expected_json_field_in_fixture_fails_parsing() {
    let base64 = valid_stellar_value_base64();
    let toml = format!(
        r#"
id = "p28-xdr-missing-expected-json"
protocol = 28
surface = "xdr"
category = "test"
description = "test"
type = "StellarValue"
kind = "decode-json-equals"
value_base64 = "{base64}"
"#
    );

    let err = parse_xdr_fixture(&toml).expect_err("should fail parsing");
    match err {
        XdrError::InvalidFixtureBody { reason, .. } => {
            assert!(
                reason.contains("missing \"expected_json\""),
                "reason: {reason}"
            );
        }
    }
}

#[test]
fn backward_compatibility_decode_success_without_expected_json_passes() {
    let base64 = valid_stellar_value_base64();
    let toml = format!(
        r#"
id = "p28-xdr-compat"
protocol = 28
surface = "xdr"
category = "test"
description = "test"
type = "StellarValue"
kind = "decode-success"
value_base64 = "{base64}"
"#
    );

    let fixture = parse_xdr_fixture(&toml).unwrap();
    let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
    assert_eq!(result.status, Status::Pass);
}

#[test]
fn decode_success_with_expected_json_field_performs_comparison() {
    let base64 = valid_stellar_value_base64();
    let toml = format!(
        r#"
id = "p28-xdr-decode-success-with-json"
protocol = 28
surface = "xdr"
category = "test"
description = "test"
type = "StellarValue"
kind = "decode-success"
value_base64 = "{base64}"

[expected_json]
tx_set_hash = "0000000000000000000000000000000000000000000000000000000000000000"
close_time = "0"
upgrades = []
ext = "basic"
"#
    );

    let fixture = parse_xdr_fixture(&toml).unwrap();
    let result = DefaultXdrRunner.run(&fixture, &context()).unwrap();
    assert_eq!(result.status, Status::Pass);
}
