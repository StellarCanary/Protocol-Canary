//! Concurrency bounding and deterministic result ordering (issue #821).
//!
//! [`execute`] runs XDR fixtures synchronously and RPC/Soroban fixtures
//! concurrently, bounded by `RunOptions::max_concurrency`, then reorders the
//! concurrent results back to their planned sequence. These tests pin that
//! contract:
//!
//! - Completion order (simulated with response delays) never changes the
//!   returned order: all XDR results, then RPC, then Soroban, each in
//!   fixture order.
//! - `max_concurrency` genuinely bounds the number of in-flight checks and
//!   `1` does not deadlock.
//! - One failing concurrent check does not abort its siblings; every fixture
//!   still produces a result in its planned slot.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use canary_core::{
    CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext, NetworkName,
    ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status, Surface,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::RpcFixture;
use canary_runner::execution::execute;
use canary_runner::scheduler::CompatibilityPlan;
use canary_soroban::SorobanFixture;
use canary_xdr::XdrFixture;
use serde_json::json;
use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A context whose cache directory no other run has written to.
///
/// A cache hit returns a stored result without contacting the mock server,
/// which would hide the concurrent execution (and its completion order) these
/// tests exist to observe.
fn context(max_concurrency: u32) -> ExecutionContext {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let unique = format!(
        "canary-runner-execution-order-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
    );

    let mut options = RunOptions::default();
    options.max_concurrency = max_concurrency;

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
            rpc_url: "unused".into(),
            passphrase: "Test SDF Network ; September 2015".into(),
            observed_protocol: None,
        },
        fixtures: FixtureStore::default(),
        git: GitContext::default(),
        cache: CacheStore::new(std::env::temp_dir().join(unique)),
        options,
    }
}

fn xdr_fixture(id: &str) -> XdrFixture {
    XdrFixture::from_loaded(
        &parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"c\"\ndescription = \"d\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n"
            ),
            std::path::Path::new("x.toml"),
        )
        .expect("xdr fixture parses"),
    )
    .expect("xdr body parses")
}

fn rpc_fixture(id: &str, method_name: &str, field: &str) -> RpcFixture {
    RpcFixture::from_loaded(
        &parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"c\"\ndescription = \"d\"\nmethod = \"{method_name}\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"{field}\"\n"
            ),
            std::path::Path::new("r.toml"),
        )
        .expect("rpc fixture parses"),
    )
    .expect("rpc body parses")
}

fn soroban_fixture(id: &str, sequence_number: i64) -> SorobanFixture {
    SorobanFixture::from_loaded(
        &parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"cap-85\"\ndescription = \"test\"\nsource_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"hello\"\nsequence_number = {sequence_number}\n\n[[args]]\nkind = \"symbol\"\nvalue = \"world\"\n\n[expect]\nkind = \"simulation-success\"\n",
                StrkeyPublicKey([0u8; 32]),
                StrkeyContract([0u8; 32]),
            ),
            std::path::Path::new("s.toml"),
        )
        .expect("soroban fixture parses"),
    )
    .expect("soroban body parses")
}

fn network_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "passphrase": "Test SDF Network ; September 2015",
            "protocolVersion": 28
        }
    }))
}

fn ledger_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "id": "AAAA", "protocolVersion": 28, "sequence": 1000 }
    }))
}

fn simulation_success() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "latestLedger": 1000, "transactionData": "AAAA" }
    }))
}

/// Mounts a mock for a Soroban fixture's unique envelope, optionally delayed.
async fn mount_simulation(server: &MockServer, fixture: &SorobanFixture, delay: Option<Duration>) {
    let envelope = canary_soroban::build_invoke_transaction_envelope(&fixture.invocation)
        .expect("the invocation builds");
    let mut response = simulation_success();
    if let Some(delay) = delay {
        response = response.set_delay(delay);
    }
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains(envelope))
        .respond_with(response)
        .mount(server)
        .await;
}

// -- Deterministic ordering under variable latency ------------------------

#[tokio::test]
async fn results_keep_surface_grouping_and_fixture_order_when_latency_reverses_completion() {
    let server = MockServer::start().await;

    // RPC: r1's response is delayed, r2's is immediate.
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("getNetwork"))
        .respond_with(network_response().set_delay(Duration::from_millis(300)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("getLatestLedger"))
        .respond_with(ledger_response())
        .mount(&server)
        .await;

    // Soroban: s1's response is delayed, s2's is immediate. The distinct
    // sequence numbers make each request body unique.
    let first = soroban_fixture("p28-soroban-first", 1);
    let second = soroban_fixture("p28-soroban-second", 2);
    mount_simulation(&server, &first, Some(Duration::from_millis(300))).await;
    mount_simulation(&server, &second, None).await;

    let mut plan = CompatibilityPlan::default();
    plan.xdr.push(xdr_fixture("p28-xdr-order"));
    plan.rpc
        .push(rpc_fixture("p28-rpc-first", "get-network", "passphrase"));
    plan.rpc.push(rpc_fixture(
        "p28-rpc-second",
        "get-latest-ledger",
        "sequence",
    ));
    plan.soroban.push(first);
    plan.soroban.push(second);

    let context = context(4);
    let results = execute(&plan, &context, &server.uri()).await;
    let _ = context.cache.clear();

    let ids: Vec<&str> = results
        .iter()
        .map(|result| result.test_id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "p28-xdr-order",
            "p28-rpc-first",
            "p28-rpc-second",
            "p28-soroban-first",
            "p28-soroban-second",
        ],
        "later-completing fixtures must still be reported in planned order"
    );

    let surfaces: Vec<Surface> = results.iter().map(|result| result.surface).collect();
    assert_eq!(
        surfaces,
        [
            Surface::Xdr,
            Surface::Rpc,
            Surface::Rpc,
            Surface::Soroban,
            Surface::Soroban,
        ],
        "results are grouped XDR -> RPC -> Soroban"
    );

    // The slow fixtures really did run and pass, so ordering was not an
    // artefact of a cache hit or a skipped check.
    assert_eq!(results[1].status, Status::Pass);
    assert_eq!(results[2].status, Status::Pass);
    assert_eq!(results[3].status, Status::Pass);
    assert_eq!(results[4].status, Status::Pass);
}

#[tokio::test]
async fn ordering_is_stable_across_repeated_runs_with_different_latencies() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("getNetwork"))
        .respond_with(network_response().set_delay(Duration::from_millis(150)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("getLatestLedger"))
        .respond_with(ledger_response().set_delay(Duration::from_millis(50)))
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.rpc
        .push(rpc_fixture("p28-rpc-stable-1", "get-network", "passphrase"));
    plan.rpc.push(rpc_fixture(
        "p28-rpc-stable-2",
        "get-latest-ledger",
        "sequence",
    ));

    for run in 0..3 {
        let context = context(2);
        let results = execute(&plan, &context, &server.uri()).await;
        let _ = context.cache.clear();

        let ids: Vec<&str> = results
            .iter()
            .map(|result| result.test_id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["p28-rpc-stable-1", "p28-rpc-stable-2"],
            "run {run} must keep planned order"
        );
        assert!(results.iter().all(|result| result.status == Status::Pass));
    }
}

// -- Concurrency bounding --------------------------------------------------

#[tokio::test]
async fn max_concurrency_one_serializes_and_does_not_deadlock() {
    let server = MockServer::start().await;
    let fixtures: Vec<SorobanFixture> = (0..4)
        .map(|i| soroban_fixture(&format!("p28-soroban-serial-{i}"), i + 1))
        .collect();
    for fixture in &fixtures {
        mount_simulation(&server, fixture, Some(Duration::from_millis(200))).await;
    }

    let mut plan = CompatibilityPlan::default();
    plan.soroban.extend(fixtures);

    let context = context(1);
    let started = Instant::now();
    let results = execute(&plan, &context, &server.uri()).await;
    let elapsed = started.elapsed();
    let _ = context.cache.clear();

    let ids: Vec<&str> = results
        .iter()
        .map(|result| result.test_id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "p28-soroban-serial-0",
            "p28-soroban-serial-1",
            "p28-soroban-serial-2",
            "p28-soroban-serial-3",
        ],
        "concurrency 1 still returns every result in planned order"
    );
    assert!(results.iter().all(|result| result.status == Status::Pass));
    assert!(
        elapsed >= Duration::from_millis(700),
        "with max_concurrency = 1 four 200ms checks cannot overlap; took {elapsed:?}"
    );
}

#[tokio::test]
async fn a_higher_max_concurrency_runs_checks_in_parallel_and_bounds_them() {
    let server = MockServer::start().await;
    let make_fixtures = || -> Vec<SorobanFixture> {
        (0..4)
            .map(|i| soroban_fixture(&format!("p28-soroban-parallel-{i}"), i + 1))
            .collect()
    };
    let fixtures = make_fixtures();
    for fixture in &fixtures {
        mount_simulation(&server, fixture, Some(Duration::from_millis(200))).await;
    }

    let serial_context = context(1);
    let mut serial_plan = CompatibilityPlan::default();
    serial_plan.soroban = make_fixtures();
    let serial_started = Instant::now();
    let serial_results = execute(&serial_plan, &serial_context, &server.uri()).await;
    let serial_elapsed = serial_started.elapsed();
    let _ = serial_context.cache.clear();

    let parallel_context = context(4);
    let mut parallel_plan = CompatibilityPlan::default();
    parallel_plan.soroban = make_fixtures();
    let parallel_started = Instant::now();
    let parallel_results = execute(&parallel_plan, &parallel_context, &server.uri()).await;
    let parallel_elapsed = parallel_started.elapsed();
    let _ = parallel_context.cache.clear();

    assert_eq!(serial_results.len(), 4);
    assert_eq!(parallel_results.len(), 4);
    assert!(parallel_results
        .iter()
        .all(|result| result.status == Status::Pass));

    // Four 200ms checks take at least four batches serially and roughly one
    // batch at concurrency 4. Comparing the two runs in the same test keeps
    // the assertion independent of absolute machine speed.
    assert!(
        serial_elapsed >= Duration::from_millis(700),
        "serial run should take at least four 200ms batches; took {serial_elapsed:?}"
    );
    assert!(
        parallel_elapsed < serial_elapsed,
        "max_concurrency = 4 should be faster than 1: parallel {parallel_elapsed:?}, serial {serial_elapsed:?}"
    );
    assert!(
        parallel_elapsed < Duration::from_millis(700),
        "max_concurrency = 4 should finish in about one 200ms batch, not four; took {parallel_elapsed:?}"
    );

    // Bound confirmed: the same planned order comes back in both cases.
    let serial_ids: Vec<&str> = serial_results
        .iter()
        .map(|result| result.test_id.as_str())
        .collect();
    let parallel_ids: Vec<&str> = parallel_results
        .iter()
        .map(|result| result.test_id.as_str())
        .collect();
    assert_eq!(serial_ids, parallel_ids);
}

// -- Failure isolation -----------------------------------------------------

#[tokio::test]
async fn one_failing_concurrent_check_does_not_abort_its_siblings() {
    let server = MockServer::start().await;

    // A JSON-RPC error object is a deterministic, non-retried engine error,
    // so this response fails the first fixture quickly while the second
    // fixture's check succeeds.
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("getNetwork"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32602, "message": "invalid params" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("getLatestLedger"))
        .respond_with(ledger_response())
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.rpc
        .push(rpc_fixture("p28-rpc-broken", "get-network", "passphrase"));
    plan.rpc.push(rpc_fixture(
        "p28-rpc-healthy",
        "get-latest-ledger",
        "sequence",
    ));

    let context = context(2);
    let results = execute(&plan, &context, &server.uri()).await;
    let _ = context.cache.clear();

    assert_eq!(results.len(), 2, "both checks produce a result");
    assert_eq!(results[0].test_id, "p28-rpc-broken");
    assert_eq!(
        results[0].status,
        Status::Error,
        "the failing check is reported, not propagated as a panic or an Err"
    );
    assert!(results[0].details.is_some());

    assert_eq!(results[1].test_id, "p28-rpc-healthy");
    assert_eq!(
        results[1].status,
        Status::Pass,
        "the sibling check must still run and pass"
    );
}

#[tokio::test]
async fn errors_and_passes_mixed_across_surfaces_are_all_reported() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("getNetwork"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32602, "message": "invalid params" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("getLatestLedger"))
        .respond_with(ledger_response())
        .mount(&server)
        .await;
    let soroban = soroban_fixture("p28-soroban-mixed", 1);
    mount_simulation(&server, &soroban, None).await;

    let mut plan = CompatibilityPlan::default();
    plan.xdr.push(xdr_fixture("p28-xdr-mixed"));
    plan.rpc.push(rpc_fixture(
        "p28-rpc-broken-mixed",
        "get-network",
        "passphrase",
    ));
    plan.rpc.push(rpc_fixture(
        "p28-rpc-ok-mixed",
        "get-latest-ledger",
        "sequence",
    ));
    plan.soroban.push(soroban);

    let context = context(3);
    let results = execute(&plan, &context, &server.uri()).await;
    let _ = context.cache.clear();

    assert_eq!(results.len(), 4);
    let ids: Vec<&str> = results
        .iter()
        .map(|result| result.test_id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "p28-xdr-mixed",
            "p28-rpc-broken-mixed",
            "p28-rpc-ok-mixed",
            "p28-soroban-mixed",
        ]
    );
    assert_eq!(results[1].status, Status::Error);
    assert_eq!(results[2].status, Status::Pass);
    assert_eq!(results[3].status, Status::Pass);
}
