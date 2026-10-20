//! `--verbose` RPC attempt logging and URL redaction (E-098).
//!
//! In verbose mode every RPC attempt must be logged to stderr with its
//! attempt count and elapsed time, and the logged URL must have its
//! basic-auth userinfo and query tokens scrubbed (CF-08 section 3.5).
//! Outside verbose mode the client must stay silent — no new stderr
//! output, which also means no URL at all.

mod support;

use support::{run_in, stderr, TempProject};
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

/// A mock endpoint that answers `getNetwork` with a valid Protocol 28
/// result, so the probe succeeds on its first attempt.
fn working_server() -> (Runtime, MockServer) {
    let rt = Runtime::new().expect("tokio runtime");
    let server = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "passphrase": "Test SDF Network ; September 2015",
                    "protocolVersion": 28
                }
            })))
            .mount(&server)
            .await;
        server
    });
    (rt, server)
}

/// Positive case: one line per attempt, naming the attempt count, the
/// elapsed time and the called method — exactly as the client documents.
#[test]
fn verbose_mode_logs_each_rpc_attempt_with_count_and_duration() {
    let (_rt, server) = working_server();
    let dir = TempProject::new("check-verbose-rpc");
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
            "--verbose",
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} | stderr: {}",
        support::stdout(&output),
        stderr(&output)
    );
    let err = stderr(&output);
    // The budget is the attempt count: the default `--rpc-max-retries 3`
    // allows three retries on top of the first request, so 4 attempts.
    assert!(
        err.contains("[RPC attempt 1/4: "),
        "attempt count and budget must be logged; stderr: {err}"
    );
    assert!(err.contains("ms] getNetwork "), "stderr: {err}");
    assert!(
        err.contains(&format!("] getNetwork {}", server.uri())),
        "the called endpoint must be named; stderr: {err}"
    );
    // The attempt line belongs on stderr, not in the report on stdout.
    assert!(
        !support::stdout(&output).contains("[RPC attempt"),
        "attempt logs must not pollute the report"
    );
}

/// Negative case: without `--verbose` the run must emit no attempt lines
/// at all — the feature adds nothing to existing stderr output.
#[test]
fn non_verbose_mode_emits_no_rpc_attempt_logs() {
    let (_rt, server) = working_server();
    let dir = TempProject::new("check-non-verbose-rpc");
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
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let err = stderr(&output);
    assert!(
        !err.contains("[RPC attempt"),
        "non-verbose runs must not log attempts; stderr: {err}"
    );
}

/// Redaction: a `--rpc-url` carrying basic-auth credentials and a query
/// token must never appear verbatim in verbose logs.
#[test]
fn verbose_logs_scrub_credentials_and_query_tokens_from_the_url() {
    let (_rt, server) = working_server();
    let dir = TempProject::new("check-verbose-redaction");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);

    let credentialed = format!(
        "{}/?token=supersecret",
        server.uri().replacen("http://", "http://user:pass@", 1)
    );
    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &credentialed,
            "--network",
            "testnet",
            "--verbose",
        ],
    );

    let err = stderr(&output);
    // Default retry settings (`--rpc-max-retries 3`) budget 4 attempts.
    assert!(
        err.contains("[RPC attempt 1/4: "),
        "the attempt must still be logged; stderr: {err}"
    );
    assert!(
        err.contains("***:***@"),
        "userinfo must be masked; stderr: {err}"
    );
    assert!(
        err.contains("token=***"),
        "query token must be masked; stderr: {err}"
    );
    assert!(
        !err.contains("user:pass") && !err.contains("supersecret"),
        "raw credentials must never reach stderr: {err}"
    );
    assert!(
        !support::stdout(&output).contains("supersecret"),
        "raw credentials must never reach the report"
    );
}

/// The same URL without `--verbose`: no attempt lines and no URL at all.
#[test]
fn non_verbose_mode_never_prints_the_rpc_url() {
    let (_rt, server) = working_server();
    let dir = TempProject::new("check-non-verbose-redaction");
    dir.write(".stellar-canary.toml", LIVE_CONFIG);

    let credentialed = format!(
        "{}/?token=supersecret",
        server.uri().replacen("http://", "http://user:pass@", 1)
    );
    let output = run_in(
        &dir.path,
        &[
            "check",
            "--allow-empty",
            "--rpc-url",
            &credentialed,
            "--network",
            "testnet",
        ],
    );

    let err = stderr(&output);
    assert!(!err.contains("[RPC attempt"), "stderr: {err}");
    assert!(!err.contains("supersecret"), "stderr: {err}");
    assert!(!err.contains("user:pass"), "stderr: {err}");
}
