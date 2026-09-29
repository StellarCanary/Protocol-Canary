//! Issue #274: no test drove `check` against an endpoint that is simply not
//! there. Every existing RPC integration test points at a live `wiremock`
//! server, so the whole unreachable-network branch was unexercised:
//!
//! - the network probe in `commands.rs` treats a transport failure as
//!   "protocol not observed" and deliberately does *not* abort the run,
//! - each fixture then surfaces the outage as `Status::Error`,
//! - and `exit_code_for_run` turns any `Status::Error` into
//!   `ExitCode::ExecutionError` (exit 3) — a code no CLI test had ever
//!   asserted.
//!
//! This test pins that contract: a dead endpoint fails loudly and
//! distinctly from a compatibility failure (1) or a configuration error (2).

mod support;

use support::{run_in, stderr, stdout, TempProject};

const UNREACHABLE_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = false
rpc = true
soroban = false
"#;

/// The URI of a TCP port nothing is listening on.
///
/// Binding an ephemeral listener and dropping it hands back a port that is
/// almost certainly free; a stray process claiming it in between would only
/// turn connection-refused into a different transport error, and the run
/// still ends in `Status::Error`.
fn closed_port_uri() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port must be bindable on loopback");
    let port = listener
        .local_addr()
        .expect("a bound listener has a local address")
        .port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

/// The acceptance case for #274: an unreachable endpoint must be reported as
/// an execution error, not silently treated as a passing or skipped run.
#[test]
fn check_against_an_unreachable_endpoint_exits_with_an_execution_error() {
    let dir = TempProject::new("check-rpc-unreachable");
    dir.write(".stellar-canary.toml", UNREACHABLE_CONFIG);
    dir.write(
        "fixtures/p28-rpc-1.toml",
        "id = \"p28-rpc-1\"\n\
         protocol = 28\n\
         surface = \"rpc\"\n\
         category = \"network\"\n\
         description = \"test\"\n\
         method = \"get-network\"\n\
         \n\
         [[assert]]\n\
         kind = \"field-type\"\n\
         field = \"passphrase\"\n\
         expected_type = \"string\"\n",
    );

    let output = run_in(&dir.path, &["check", "--rpc-url", &closed_port_uri()]);

    // 3 == ExitCode::ExecutionError: the run happened, the network did not.
    assert_eq!(
        output.status.code(),
        Some(3),
        "an unreachable endpoint must exit with the execution-error code; \
         stdout: {} | stderr: {}",
        stdout(&output),
        stderr(&output)
    );

    let text = stdout(&output);
    assert!(
        text.contains("protocol not observed"),
        "the probe's transport failure must be recorded as an unobserved \
         protocol rather than hiding the network line; stdout: {text}"
    );
    assert!(
        text.contains("‼ ERROR"),
        "the fixture must be reported as an error, not a pass or a skip; \
         stdout: {text}"
    );
    assert!(
        text.contains("Status: ERROR"),
        "the terminal verdict must be ERROR for an execution failure; \
         stdout: {text}"
    );
    assert!(
        text.contains("0/1 applicable checks passed."),
        "stdout: {text}"
    );
}
