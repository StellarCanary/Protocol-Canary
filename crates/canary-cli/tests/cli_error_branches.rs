mod support;

use support::{run_in, stderr, TempProject};

#[test]
fn nonexistent_configuration_file_returns_configuration_error() {
    let dir = TempProject::new("err-nonexistent-config");

    for cmd in ["check", "inspect", "fixtures"] {
        let output = run_in(
            &dir.path,
            &[cmd, "--config", "nonexistent-config-file.toml"],
        );
        assert_eq!(
            output.status.code(),
            Some(2),
            "command '{cmd}' with nonexistent --config should exit with code 2"
        );
        let err_text = stderr(&output);
        assert!(
            err_text.contains("configuration error"),
            "stderr for '{cmd}' should mention 'configuration error': {err_text}"
        );
        assert!(
            err_text.contains("nonexistent-config-file.toml"),
            "stderr for '{cmd}' should mention filename: {err_text}"
        );
    }
}

#[test]
fn invalid_command_line_flags_returns_clap_usage_error() {
    let dir = TempProject::new("err-invalid-flag");

    for cmd in ["check", "inspect", "fixtures"] {
        let output = run_in(&dir.path, &[cmd, "--unknown-flag-xyz"]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "command '{cmd}' with unknown flag should exit with clap code 2"
        );
        let err_text = stderr(&output);
        assert!(
            err_text.contains("unexpected argument") || err_text.contains("unexpected"),
            "stderr for '{cmd}' should contain clap argument diagnostic: {err_text}"
        );
    }
}

#[test]
fn missing_required_rpc_url_returns_configuration_error() {
    let dir = TempProject::new("err-missing-rpc-url");

    // Custom network requires --rpc-url
    let output = run_in(&dir.path, &["check", "--network", "my-custom-net"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "check --network custom without --rpc-url should exit with code 2"
    );
    let err_text = stderr(&output);
    assert!(
        err_text.contains("--rpc-url is required"),
        "stderr should state that --rpc-url is required: {err_text}"
    );
}

#[test]
fn zero_protocol_flag_returns_configuration_error() {
    let dir = TempProject::new("err-zero-protocol");

    for cmd in ["check", "inspect", "fixtures"] {
        let output = run_in(&dir.path, &[cmd, "--protocol", "0"]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "command '{cmd}' with --protocol 0 should exit with code 2"
        );
        let err_text = stderr(&output);
        assert!(
            err_text.contains("--protocol must be a positive protocol version number"),
            "stderr for '{cmd}' should reject zero protocol: {err_text}"
        );
    }
}

#[test]
fn malformed_configuration_file_returns_configuration_error() {
    let dir = TempProject::new("err-malformed-config");
    dir.write(".stellar-canary.toml", "invalid [ [ [ toml syntax");

    for cmd in ["check", "inspect", "fixtures"] {
        let output = run_in(&dir.path, &[cmd]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "command '{cmd}' with malformed config should exit with code 2"
        );
        let err_text = stderr(&output);
        assert!(
            err_text.contains("configuration error"),
            "stderr for '{cmd}' should report configuration error: {err_text}"
        );
    }
}

#[test]
fn malformed_fixture_toml_returns_invalid_fixture_error() {
    let dir = TempProject::new("err-malformed-fixture");
    dir.write("fixtures/xdr/bad.toml", "not valid [ [ [ toml");

    for cmd in ["check", "inspect", "fixtures"] {
        let output = run_in(&dir.path, &[cmd, "--fixtures-dir", "fixtures"]);
        assert_eq!(
            output.status.code(),
            Some(4),
            "command '{cmd}' with malformed fixture TOML should exit with InvalidFixture code 4"
        );
        let err_text = stderr(&output);
        assert!(
            err_text.contains("invalid fixture"),
            "stderr for '{cmd}' should report invalid fixture error: {err_text}"
        );
    }
}

#[test]
fn duplicate_fixture_ids_returns_invalid_fixture_error() {
    let dir = TempProject::new("err-duplicate-fixture");
    let fixture_body = "id = \"dup-id\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"c\"\ndescription = \"d\"\n";
    dir.write("fixtures/a.toml", fixture_body);
    dir.write("fixtures/b.toml", fixture_body);

    for cmd in ["check", "inspect", "fixtures"] {
        let output = run_in(&dir.path, &[cmd, "--fixtures-dir", "fixtures"]);
        assert_eq!(
            output.status.code(),
            Some(4),
            "command '{cmd}' with duplicate fixture IDs should exit with InvalidFixture code 4"
        );
        let err_text = stderr(&output);
        assert!(
            err_text.contains("invalid fixture") || err_text.contains("duplicate fixture id"),
            "stderr for '{cmd}' should report duplicate fixture ID error: {err_text}"
        );
    }
}
