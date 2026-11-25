//! `--rpc-max-retries` / `--rpc-retry-delay-ms` reach the RPC client the
//! `check` command actually uses (E-097).
//!
//! The flags are parsed in `cli.rs` and converted by
//! `RetryPolicy::from_retries`; these tests drive the whole binary against
//! a mock endpoint that always fails, so the number of requests the server
//! receives *is* the number of attempts the client made — end-to-end proof
//! that `0` disables retries and that a configured count is honored.

mod support;

use support::{run_in, stderr, stdout, TempProject};
use tokio::runtime::Runtime;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LIVE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = false
rpc = true
soroban = false
"#;

/// Starts a mock RPC endpoint that answers every request with 503 — a
/// retryable transport-level failure — and returns its request count so
/// far.
fn failing_server() -> (Runtime, MockServer) {
    let rt = Runtime::new().expect("tokio runtime");
    let server = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        server
    });
    (rt, server)
}

fn request_count(rt: &Runtime, server: &MockServer) -> usize {
    rt.block_on(server.received_requests())
        .expect("the mock server records requests")
        .len()
}

/// `--rpc-max-retries 0` must fail the network probe on its first
/// attempt: exactly one request reaches the server. (No fixtures are
/// configured, so every request the run makes comes from the probe.)
#[test]
fn zero_retries_makes_the_network_probe_fail_after_a_single_request() {
    let (rt, server) = failing_server();
    let dir = TempProject::new("check-rpc-retries-zero");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &server.uri(),
            "--network",
            "testnet",
            "--rpc-max-retries",
            "0",
            "--rpc-retry-delay-ms",
            "0",
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "a failed probe does not abort a fixture-less run; stdout: {} | stderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(
        stdout(&output).contains("protocol not observed"),
        "stdout: {}",
        stdout(&output)
    );
    assert_eq!(
        request_count(&rt, &server),
        1,
        "--rpc-max-retries 0 must send exactly one request"
    );
}

/// A configured retry count is honored end-to-end: two retries after the
/// first attempt means exactly three requests reach the server.
#[test]
fn configured_retries_are_honored_by_the_check_run() {
    let (rt, server) = failing_server();
    let dir = TempProject::new("check-rpc-retries-two");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &server.uri(),
            "--network",
            "testnet",
            "--rpc-max-retries",
            "2",
            "--rpc-retry-delay-ms",
            "0",
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} | stderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert_eq!(
        request_count(&rt, &server),
        3,
        "--rpc-max-retries 2 must produce three attempts"
    );
}

/// Persistent server errors (5xx) must stop retrying after exactly
/// max_attempts requests.
#[test]
fn persistent_5xx_errors_stop_after_max_attempts() {
    let (rt, server) = failing_server();
    let dir = TempProject::new("check-rpc-retries-persistent");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);

    // Create a minimal fixtures directory so the binary does not warn about
    // missing fixtures. An empty directory with a placeholder file is sufficient.
    let fixtures_dir = dir.path.join("fixtures");
    std::fs::create_dir_all(&fixtures_dir).expect("failed to create fixtures dir");
    std::fs::write(
        fixtures_dir.join("p28-xdr-1.toml"),
        // Minimal valid Protocol 28 XDR fixture (surface = "xdr").
        "id = \"p28-xdr-1\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"cap-83\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"\n",
    )
    .expect("failed to write fixture");

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &server.uri(),
            "--network",
            "testnet",
            "--rpc-max-retries",
            "2",
            "--rpc-retry-delay-ms",
            "0",
        ],
    );

    // The check should still exit 0 because with --allow-empty and no fixtures,
    // a failed probe does not abort the run (see zero_retries test).
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} | stderr: {}",
        stdout(&output),
        stderr(&output)
    );

    // The mock server should have received exactly max_attempts + 1 = 3 requests
    // (the initial attempt plus 2 retries).
    assert_eq!(
        request_count(&rt, &server),
        3,
        "persistent 5xx must stop after max_attempts; server received {} requests",
        request_count(&rt, &server)
    );
}