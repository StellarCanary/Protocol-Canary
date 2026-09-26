//! Live-path `check` tests against a mocked Stellar RPC endpoint.
//!
//! These cover `run_check_inner`'s comparison of the endpoint-reported
//! network identity (`getNetwork`) against the run's `--network`
//! passphrase and target protocol — the path that constructs
//! `RpcError::NetworkMismatch`/`RpcError::ProtocolMismatch`.

mod support;

use support::{run_in, stderr, stdout, TempProject};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";
const MAINNET_PASSPHRASE: &str = "Public Global Stellar Network ; September 2015";

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

/// The acceptance case for issue #52: when the RPC-observed protocol
/// differs from `--protocol`, the mismatch is surfaced to the user as a
/// warning rather than silently reduced to a report annotation.
#[test]
fn a_protocol_mismatch_between_the_target_and_observed_protocol_is_surfaced_to_the_user() {
    let (_rt, server) = mock_get_network(TESTNET_PASSPHRASE, 27);
    let dir = TempProject::new("check-observed-protocol-mismatch");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);

    let rpc_url = server.uri();
    let output = run_in(
        &dir.path,
        &["check", "--rpc-url", &rpc_url, "--network", "testnet"],
    );

    let err = stderr(&output);
    assert!(
        err.contains("warning:"),
        "mismatch must be flagged as a warning; stderr: {err}"
    );
    assert!(
        err.contains("the RPC endpoint reports protocol 27, but this run targets protocol 28"),
        "stderr: {err}"
    );

    // The run still completes (protocol-ahead rehearsal is legitimate),
    // and the observed protocol remains in the report.
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("Network: testnet (observed protocol 27)"));
}

/// A `--network`/`--rpc-url` pair pointing at different networks must
/// abort as a configuration error before any checks run.
#[test]
fn a_network_passphrase_mismatch_aborts_as_a_configuration_error() {
    let (_rt, server) = mock_get_network(MAINNET_PASSPHRASE, 28);
    let dir = TempProject::new("check-network-mismatch");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);

    let rpc_url = server.uri();
    let output = run_in(
        &dir.path,
        &["check", "--rpc-url", &rpc_url, "--network", "testnet"],
    );

    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(err.contains("configuration error"), "stderr: {err}");
    assert!(
        err.contains(MAINNET_PASSPHRASE) && err.contains(TESTNET_PASSPHRASE),
        "stderr must name both passphrases; stderr: {err}"
    );
}

/// Control: a matching network and protocol produce no mismatch warning.
#[test]
fn a_matching_network_and_protocol_produce_no_mismatch_warning() {
    let (_rt, server) = mock_get_network(TESTNET_PASSPHRASE, 28);
    let dir = TempProject::new("check-network-match");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);

    let rpc_url = server.uri();
    let output = run_in(
        &dir.path,
        &["check", "--rpc-url", &rpc_url, "--network", "testnet"],
    );

    assert_eq!(output.status.code(), Some(0));
    let err = stderr(&output);
    assert!(!err.contains("warning:"), "stderr: {err}");
    let text = stdout(&output);
    assert!(text.contains("Network: testnet (observed protocol 28)"));
}
