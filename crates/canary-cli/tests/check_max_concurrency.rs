//! Propagation tests for the `--max-concurrency` CLI flag (issue #23).
//!
//! The unit tests in `cli.rs` cover flag *parsing*; these tests verify the
//! parsed value actually reaches the runner's concurrency window instead of
//! the previously hardcoded `max_concurrency: 4` in `commands.rs`.
//!
//! The observable signal is wall-clock time: the mock endpoint delays every
//! response, so with `--max-concurrency 1` the RPC fixtures must run
//! strictly one after another, while a higher value overlaps them. Only a
//! lower bound on the serialized runtime is asserted — a loaded CI machine
//! can slow a run down but cannot make sequential requests overlap, so the
//! assertion has no flaky direction. The hardcoded-4 regression (flag
//! ignored, all fixtures overlapped) finishes far below the floor and fails
//! the assertion.

mod support;

use std::time::{Duration, Instant};

use support::{run_in, stderr, stdout, TempProject};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";

const RPC_CONFIG: &str = r#"
version = 1
protocol = 28

[tests]
xdr = false
rpc = true
soroban = false
"#;

const FIXTURE_COUNT: usize = 8;

/// Artificial per-response latency. Long enough that a serialized
/// (`--max-concurrency 1`) run is clearly distinguishable from an
/// overlapped (concurrency 4) one.
const RESPONSE_DELAY: Duration = Duration::from_millis(250);

/// A run honoring `--max-concurrency 1` must take at least the probe
/// response plus every fixture response in sequence (~2.25 s of pure
/// response delay). A run that ignores the flag overlaps the fixtures and
/// finishes well below this floor.
const SERIALIZED_FLOOR: Duration = Duration::from_millis(1300);

fn rpc_fixture(id: &str) -> String {
    format!(
        "id = \"{id}\"\n\
         protocol = 28\n\
         surface = \"rpc\"\n\
         category = \"network\"\n\
         description = \"test\"\n\
         method = \"get-network\"\n\
         \n\
         [[assert]]\n\
         kind = \"field-type\"\n\
         field = \"passphrase\"\n\
         expected_type = \"string\"\n"
    )
}

/// Starts a mock Stellar RPC endpoint that answers every JSON-RPC request
/// with a valid `getNetwork` result after an artificial delay.
fn start_delayed_mock() -> (tokio::runtime::Runtime, MockServer) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let server = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(RESPONSE_DELAY)
                    .set_body_json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "passphrase": TESTNET_PASSPHRASE,
                            "protocolVersion": 28
                        }
                    })),
            )
            .mount(&server)
            .await;
        server
    });
    (rt, server)
}

/// Writes a fresh temp project whose config enables RPC checks, with
/// `FIXTURE_COUNT` get-network fixtures. The endpoint is passed per-run
/// via `--rpc-url`.
fn setup(project_name: &str) -> TempProject {
    let dir = TempProject::new(project_name);
    dir.write(".stellar-canary.toml", &format!("{RPC_CONFIG}\n"));
    for i in 1..=FIXTURE_COUNT {
        dir.write(
            &format!("fixtures/p28-rpc-{i:02}.toml"),
            &rpc_fixture(&format!("p28-rpc-{i:02}")),
        );
    }
    dir
}

/// The acceptance case for issue #23: the flag value must actually govern
/// execution. With `--max-concurrency 1` and a delayed endpoint, the
/// fixtures run one after another; a run that kept the previously hardcoded
/// concurrency of 4 would overlap them and finish below the floor.
#[test]
fn max_concurrency_one_serializes_rpc_fixture_requests() {
    let (_rt, server) = start_delayed_mock();
    let dir = setup("check-max-concurrency-1");

    let start = Instant::now();
    let output = run_in(
        &dir.path,
        &[
            "check",
            "--rpc-url",
            &server.uri(),
            "--max-concurrency",
            "1",
        ],
    );
    let elapsed = start.elapsed();

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("8/8 applicable checks passed."),
        "every fixture must still run and pass; stdout: {}",
        stdout(&output)
    );
    assert!(
        elapsed >= SERIALIZED_FLOOR,
        "with --max-concurrency 1 the {FIXTURE_COUNT} delayed RPC fixtures \
         must run sequentially (expected at least {SERIALIZED_FLOOR:?}), but \
         the run finished in {elapsed:?} — the flag value is not reaching \
         the execution plan"
    );
}

/// Control: the same suite at a higher concurrency still runs every fixture
/// and passes — the flag changes scheduling, not outcomes.
#[test]
fn max_concurrency_four_still_runs_every_fixture() {
    let (_rt, server) = start_delayed_mock();
    let dir = setup("check-max-concurrency-4");

    let output = run_in(
        &dir.path,
        &[
            "check",
            "--rpc-url",
            &server.uri(),
            "--max-concurrency",
            "4",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("8/8 applicable checks passed."),
        "stdout: {}",
        stdout(&output)
    );
}
