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
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &rpc_url,
            "--network",
            "testnet",
        ],
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

/// Issue #721: an endpoint reporting a protocol *newer* than the run's
/// target. Protocol 99 is not a version this engine knows about at all, so
/// the mismatch can only be resolved by upgrading the engine (or by
/// pointing the run at an endpoint that speaks the target protocol) — never
/// by downgrading the endpoint. The diagnostic must therefore name both
/// versions in one actionable sentence that makes the direction of the gap
/// obvious, and the run must still complete rather than abort.
///
/// Exit code: `0`. The engine's documented exit-code contract
/// (`docs-site/src/exit-codes.md`, enforced by `exit_code_docs`) has no
/// `ProtocolMismatch` code — `4` is `InvalidFixture`, reserved for a fixture
/// file that fails to load — and a protocol mismatch is deliberately a
/// warning rather than a failure, because rehearsing a target protocol
/// against a network that is ahead *or* behind is the tool's core use case.
/// This test pins that contract for the future-protocol direction; the
/// sibling test above pins it for the backward direction.
#[test]
fn a_future_protocol_reported_by_the_rpc_endpoint_is_surfaced_as_an_actionable_warning() {
    let (_rt, server) = mock_get_network(TESTNET_PASSPHRASE, 99);
    let dir = TempProject::new("check-observed-future-protocol-mismatch");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);

    let rpc_url = server.uri();
    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &rpc_url,
            "--network",
            "testnet",
        ],
    );

    let err = stderr(&output);
    assert!(
        err.contains("warning:"),
        "a future-protocol mismatch is a warning, not a silent report annotation; stderr: {err}"
    );
    // The single sentence carries the whole diagnosis: which protocol the
    // endpoint reports (99), which protocol the engine is running (28), and
    // therefore which side is behind and needs upgrading.
    assert!(
        err.contains("the RPC endpoint reports protocol 99, but this run targets protocol 28"),
        "stderr: {err}"
    );
    assert!(
        !err.contains("configuration error"),
        "an endpoint ahead of the engine is not a configuration error; stderr: {err}"
    );

    // A protocol mismatch is evidence to surface, not a reason to abort: the
    // run completes (see the module docs on exit codes above).
    assert_eq!(
        output.status.code(),
        Some(0),
        "a protocol warning must not change the exit code; stderr: {err}"
    );

    // The observation is retained, so the report still records that this run
    // was measured against a Protocol 99 endpoint.
    let text = stdout(&output);
    assert!(
        text.contains("Network: testnet (observed protocol 99)"),
        "stdout: {text}"
    );
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
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &rpc_url,
            "--network",
            "testnet",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let err = stderr(&output);
    assert!(
        !err.contains("RPC endpoint reports protocol"),
        "stderr: {err}"
    );
    let text = stdout(&output);
    assert!(text.contains("Network: testnet (observed protocol 28)"));
}
