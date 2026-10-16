//! `stellar-canary check --network-passphrase` (E-096) against a mocked
//! Stellar RPC endpoint.
//!
//! The flag supplies the passphrase a run verifies `getNetwork` against —
//! the thing that makes private and standalone networks verifiable at all,
//! and the passphrase a signing consumer would use. These tests cover:
//! verifying a custom network with the flag, a wrong passphrase aborting,
//! the flag *replacing* the well-known `--network` default rather than
//! being ignored, the empty-value argument validation error, and the
//! untouched no-flag behavior for custom networks.

mod support;

use support::{run_in, stderr, stdout, TempProject};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CUSTOM_PASSPHRASE: &str = "Local Standalone Network ; October 2026";
const OTHER_PASSPHRASE: &str = "Some Other Net ; October 2026";
const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";

const LIVE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = false
rpc = true
soroban = false
"#;

/// Starts a mock RPC endpoint whose `getNetwork` result reports the given
/// passphrase and protocol version.
fn mock_get_network(
    passphrase: &str,
    protocol_version: u32,
) -> (tokio::runtime::Runtime, MockServer) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let server = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "passphrase": passphrase,
                    "protocolVersion": protocol_version
                }
            })))
            .mount(&server)
            .await;
        server
    });
    (rt, server)
}

fn project(name: &str) -> TempProject {
    let dir = TempProject::new(name);
    dir.write(".stellar-canary.toml", LIVE_CONFIG);
    dir
}

/// Positive case: a custom network is verified when `--network-passphrase`
/// supplies its identity — without the flag this comparison is skipped,
/// so the flag is what makes the verification happen.
#[test]
fn the_passphrase_flag_verifies_a_private_network() {
    let (_rt, server) = mock_get_network(CUSTOM_PASSPHRASE, 28);
    let dir = project("network-passphrase-custom-match");

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &server.uri(),
            "--network",
            "my-standalone-network",
            "--network-passphrase",
            CUSTOM_PASSPHRASE,
        ],
    );

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("Network: my-standalone-network (observed protocol 28)"),
        "stdout: {text}"
    );
    let err = stderr(&output);
    assert!(
        !err.contains("does not match the expected passphrase"),
        "matching passphrase must not mismatch; stderr: {err}"
    );
}

/// Negative case: a `--network-passphrase` that differs from what the
/// endpoint reports aborts as a configuration error before any check runs,
/// naming both passphrases.
#[test]
fn a_wrong_custom_passphrase_aborts_as_a_configuration_error() {
    let (_rt, server) = mock_get_network(OTHER_PASSPHRASE, 28);
    let dir = project("network-passphrase-custom-mismatch");

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &server.uri(),
            "--network",
            "my-standalone-network",
            "--network-passphrase",
            CUSTOM_PASSPHRASE,
        ],
    );

    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(err.contains("configuration error"), "stderr: {err}");
    assert!(
        err.contains(CUSTOM_PASSPHRASE) && err.contains(OTHER_PASSPHRASE),
        "stderr must name both passphrases; stderr: {err}"
    );
}

/// The flag must *override* the well-known `--network` passphrase, not be
/// silently ignored: with `--network testnet` and a custom flag value, an
/// endpoint reporting the real Testnet passphrase has to mismatch against
/// the flag value. If the flag were ignored the run would compare
/// Testnet-vs-Testnet and pass.
#[test]
fn the_passphrase_flag_replaces_the_well_known_default_passphrase() {
    let (_rt, server) = mock_get_network(TESTNET_PASSPHRASE, 28);
    let dir = project("network-passphrase-overrides-default");

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &server.uri(),
            "--network",
            "testnet",
            "--network-passphrase",
            OTHER_PASSPHRASE,
        ],
    );

    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(
        err.contains(OTHER_PASSPHRASE) && err.contains(TESTNET_PASSPHRASE),
        "the flag value must be the expected passphrase; stderr: {err}"
    );
}

/// An empty `--network-passphrase ""` is an argument validation error:
/// clap rejects it before anything runs, with exit code 2.
#[test]
fn an_empty_passphrase_is_an_argument_validation_error() {
    let dir = project("network-passphrase-empty");

    let output = run_in(&dir.path, &["check", "--network-passphrase", ""]);

    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty(), "nothing must run or render");
    let err = stderr(&output);
    assert!(err.contains("--network-passphrase"), "stderr: {err}");
    assert!(
        err.contains("the passphrase cannot be empty"),
        "stderr: {err}"
    );
}

/// Control: without the flag, a custom network still skips the passphrase
/// comparison (the historical behavior this feature extends, not replaces).
#[test]
fn without_the_flag_a_custom_network_still_skips_the_passphrase_check() {
    let (_rt, server) = mock_get_network(OTHER_PASSPHRASE, 28);
    let dir = project("network-passphrase-absent-custom");

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &server.uri(),
            "--network",
            "my-standalone-network",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("Network: my-standalone-network (observed protocol 28)"),
        "stdout: {text}"
    );
    assert!(
        !stderr(&output).contains("does not match the expected passphrase"),
        "no passphrase means no comparison; stderr: {}",
        stderr(&output)
    );
}
