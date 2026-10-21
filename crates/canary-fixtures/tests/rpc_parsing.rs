use canary_fixtures::parse_fixture_str;
use canary_rpc::{RpcFixture, RpcFixtureError};
use std::path::Path;

fn parse_rpc_fixture(toml_str: &str) -> Result<RpcFixture, RpcFixtureError> {
    let loaded = parse_fixture_str(toml_str, Path::new("test_rpc.toml"))
        .expect("common fixture metadata parsing succeeded");
    RpcFixture::from_loaded(&loaded)
}

#[test]
fn missing_required_method_fails_parsing() {
    let toml = r#"
id = "p28-rpc-missing-method"
protocol = 28
surface = "rpc"
category = "network"
description = "missing method field"

[[assert]]
kind = "field-exists"
field = "passphrase"
"#;

    let err = parse_rpc_fixture(toml).expect_err("should fail due to missing method");
    let RpcFixtureError::InvalidFixtureBody {
        source_path,
        reason,
    } = &err;

    assert_eq!(source_path, Path::new("test_rpc.toml"));
    assert!(
        reason.contains("missing required string field \"method\""),
        "unexpected reason: {reason}"
    );
}

#[test]
fn malformed_or_invalid_params_fails_parsing() {
    let toml_scalar = r#"
id = "p28-rpc-bad-params-scalar"
protocol = 28
surface = "rpc"
category = "network"
description = "scalar params"
method = "get-network"
params = "invalid-string-params"
"#;

    let err = parse_rpc_fixture(toml_scalar).expect_err("should fail due to scalar params");
    let RpcFixtureError::InvalidFixtureBody { reason, .. } = &err;
    assert!(
        reason.contains("params"),
        "diagnostic should mention 'params': {reason}"
    );

    let toml_unsupported = r#"
id = "p28-rpc-unsupported-params"
protocol = 28
surface = "rpc"
category = "network"
description = "params provided for method that takes none"
method = "get-network"
params = { key = "val" }
"#;

    let err = parse_rpc_fixture(toml_unsupported).expect_err("should fail due to params");
    let RpcFixtureError::InvalidFixtureBody { reason, .. } = &err;
    assert!(
        reason.contains("params"),
        "diagnostic should mention 'params': {reason}"
    );
    assert!(
        reason.contains("get-network"),
        "diagnostic should mention method: {reason}"
    );
}

#[test]
fn invalid_or_unknown_assertion_operator_fails_parsing() {
    let toml = r#"
id = "p28-rpc-unknown-operator"
protocol = 28
surface = "rpc"
category = "network"
description = "unknown assertion operator"
method = "get-network"

[[assert]]
kind = "field-matches-regex"
field = "passphrase"
"#;

    let err = parse_rpc_fixture(toml).expect_err("should fail due to unknown assertion operator");
    let RpcFixtureError::InvalidFixtureBody { reason, .. } = &err;
    assert!(
        reason.contains("field-matches-regex"),
        "diagnostic should identify unknown kind: {reason}"
    );
    assert!(
        reason.contains("unsupported assertion kind"),
        "diagnostic should be actionable: {reason}"
    );
}

#[test]
fn invalid_or_unknown_assertion_type_fails_parsing() {
    let toml = r#"
id = "p28-rpc-unknown-type"
protocol = 28
surface = "rpc"
category = "network"
description = "unknown expected json type"
method = "get-network"

[[assert]]
kind = "field-type"
field = "passphrase"
expected_type = "integer"
"#;

    let err = parse_rpc_fixture(toml).expect_err("should fail due to unknown json type");
    let RpcFixtureError::InvalidFixtureBody { reason, .. } = &err;
    assert!(
        reason.contains("integer"),
        "diagnostic should identify unknown type 'integer': {reason}"
    );
}

#[test]
fn unknown_fixture_body_key_fails_parsing() {
    let toml = r#"
id = "p28-rpc-unknown-key"
protocol = 28
surface = "rpc"
category = "network"
description = "unknown key in body"
method = "get-network"
unknown_field = "unexpected"
"#;

    let err = parse_rpc_fixture(toml).expect_err("should fail due to unknown fixture body key");
    let RpcFixtureError::InvalidFixtureBody { reason, .. } = &err;
    assert!(
        reason.contains("unknown_field"),
        "diagnostic should identify offending key 'unknown_field': {reason}"
    );
}

#[test]
fn unknown_assertion_key_fails_parsing() {
    let toml = r#"
id = "p28-rpc-unknown-assert-key"
protocol = 28
surface = "rpc"
category = "network"
description = "unknown key in assertion"
method = "get-network"

[[assert]]
kind = "field-exists"
field = "passphrase"
unexpected_attr = true
"#;

    let err = parse_rpc_fixture(toml).expect_err("should fail due to unknown assertion key");
    let RpcFixtureError::InvalidFixtureBody { reason, .. } = &err;
    assert!(
        reason.contains("unexpected_attr"),
        "diagnostic should identify offending assertion key: {reason}"
    );
}

#[test]
fn invalid_assertion_block_structure_fails_parsing() {
    let toml_scalar_assert = r#"
id = "p28-rpc-scalar-assert"
protocol = 28
surface = "rpc"
category = "network"
description = "assert is a scalar string instead of an array"
method = "get-network"
assert = "not-an-array"
"#;

    let err = parse_rpc_fixture(toml_scalar_assert)
        .expect_err("should fail due to non-array assertion block");
    let RpcFixtureError::InvalidFixtureBody { reason, .. } = &err;
    assert!(
        reason.contains("assert"),
        "diagnostic should mention 'assert': {reason}"
    );

    let toml_non_table_entry = r#"
id = "p28-rpc-non-table-entry"
protocol = 28
surface = "rpc"
category = "network"
description = "assertion entry is a scalar"
method = "get-network"
assert = ["not-a-table"]
"#;

    let err = parse_rpc_fixture(toml_non_table_entry)
        .expect_err("should fail due to scalar assertion entry");
    let RpcFixtureError::InvalidFixtureBody { reason, .. } = &err;
    assert!(
        reason.contains("[[assert]]"),
        "diagnostic should mention [[assert]] table entry: {reason}"
    );
}
