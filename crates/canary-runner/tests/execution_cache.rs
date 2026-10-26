//! Execution result categories and result-cache branches (`[E-109]`,
//! issue #503).
//!
//! Every test drives the public [`execute`] entry point against a local
//! `wiremock` server (or none, for XDR), with a fresh cache directory per
//! test, so nothing depends on the network, on test order, or on files an
//! earlier run left behind.
//!
//! A cache hit is reported as [`ResultSource::Cache`]; that is the runner's
//! "replayed" marker. What the cache may hold is decided by
//! [`CachePolicy`]: XDR results are always cacheable, RPC and Soroban
//! results only when `live_max_age` is set and only while younger than it,
//! and an [`Status::Error`] result is never stored.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use canary_core::{
    CachePolicy, CacheStore, CompatibilityResult, ExecutionContext, FixtureStore, GitContext,
    NetworkContext, NetworkName, ProjectContext, ProjectType, ProtocolVersion, ResultSource,
    RunOptions, Status, Surface,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::RpcFixture;
use canary_runner::{execute, CompatibilityPlan};
use canary_soroban::SorobanFixture;
use canary_xdr::XdrFixture;
use serde_json::{json, Value};
use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
use wiremock::matchers::{body_string_contains, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

// --- harness ----------------------------------------------------------------

/// A cache directory unique to one test, removed when dropped.
struct TempCache {
    root: PathBuf,
}

impl TempCache {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        // pid + nanos + counter: the timestamp alone cannot separate the
        // threads the harness runs in parallel.
        let root = std::env::temp_dir().join(format!(
            "canary-execution-cache-{label}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        TempCache { root }
    }

    fn store(&self, policy: CachePolicy) -> CacheStore {
        CacheStore::new(&self.root).with_policy(policy)
    }

    /// The cache entry files currently on disk.
    fn entries(&self) -> Vec<PathBuf> {
        let Ok(dir) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut entries: Vec<PathBuf> = dir
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        entries.sort();
        entries
    }

    /// The single entry a one-fixture run wrote.
    fn only_entry(&self) -> PathBuf {
        let entries = self.entries();
        assert_eq!(entries.len(), 1, "expected one cache entry: {entries:?}");
        entries.into_iter().next().unwrap()
    }
}

impl Drop for TempCache {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// XDR results only: the default policy.
fn default_policy() -> CachePolicy {
    CachePolicy::default()
}

/// Live (RPC and Soroban) results may be replayed for an hour.
fn live_policy() -> CachePolicy {
    CachePolicy {
        enabled: true,
        live_max_age: Some(Duration::from_secs(3600)),
    }
}

/// `--no-cache`.
fn disabled_policy() -> CachePolicy {
    CachePolicy {
        enabled: false,
        live_max_age: Some(Duration::from_secs(3600)),
    }
}

fn context(cache: CacheStore) -> ExecutionContext {
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
            rpc_url: "http://mock.invalid".into(),
            passphrase: "Test SDF Network ; September 2015".into(),
            observed_protocol: Some(ProtocolVersion(28)),
        },
        fixtures: FixtureStore::default(),
        git: GitContext::default(),
        cache,
        // No retries and a short timeout, so the timeout test is quick and
        // every other test sends exactly one request per execution.
        options: RunOptions {
            rpc_timeout: 1,
            rpc_max_retries: 0,
            rpc_retry_delay_ms: 0,
            ..RunOptions::default()
        },
    }
}

const METADATA: &str =
    "protocol = 28\ncategory = \"test\"\ndescription = \"execution cache test\"\n";

fn xdr_fixture(id: &str, kind: &str, value_base64: &str) -> XdrFixture {
    let text = format!(
        "id = \"{id}\"\nsurface = \"xdr\"\n{METADATA}type = \"StellarValue\"\nkind = \"{kind}\"\nvalue_base64 = \"{value_base64}\"\n"
    );
    XdrFixture::from_loaded(&parse_fixture_str(&text, Path::new("xdr.toml")).unwrap()).unwrap()
}

fn rpc_fixture(id: &str) -> RpcFixture {
    let text = format!(
        "id = \"{id}\"\nsurface = \"rpc\"\n{METADATA}method = \"get-network\"\n\n[[assert]]\nkind = \"field-equals\"\nfield = \"protocolVersion\"\nvalue = 28\n"
    );
    RpcFixture::from_loaded(&parse_fixture_str(&text, Path::new("rpc.toml")).unwrap()).unwrap()
}

fn soroban_fixture(id: &str, expect: &str) -> SorobanFixture {
    let text = format!(
        "id = \"{id}\"\nsurface = \"soroban\"\n{METADATA}source_account = \"{}\"\ncontract_id = \"{}\"\nfunction = \"hello\"\nsequence_number = 1\n\n[[args]]\nkind = \"symbol\"\nvalue = \"world\"\n\n[expect]\n{expect}",
        StrkeyPublicKey([0u8; 32]),
        StrkeyContract([0u8; 32]),
    );
    SorobanFixture::from_loaded(&parse_fixture_str(&text, Path::new("soroban.toml")).unwrap())
        .unwrap()
}

const VALID_STELLAR_VALUE: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// Canonical base64 that is not a complete `StellarValue`, so the fixture
/// parses and then fails the decode assertion at run time.
const DECODE_FAILURE_BASE64: &str = "AAAA";

fn xdr_plan(fixture: XdrFixture) -> CompatibilityPlan {
    let mut plan = CompatibilityPlan::default();
    plan.xdr.push(fixture);
    plan
}

fn rpc_plan(fixture: RpcFixture) -> CompatibilityPlan {
    let mut plan = CompatibilityPlan::default();
    plan.rpc.push(fixture);
    plan
}

fn soroban_plan(fixture: SorobanFixture) -> CompatibilityPlan {
    let mut plan = CompatibilityPlan::default();
    plan.soroban.push(fixture);
    plan
}

/// Runs a one-fixture plan and returns its only result.
async fn run_one(
    plan: &CompatibilityPlan,
    context: &ExecutionContext,
    endpoint: &str,
) -> CompatibilityResult {
    let mut results = execute(plan, context, endpoint).await;
    assert_eq!(results.len(), 1);
    results.remove(0)
}

/// An endpoint nothing listens on; only XDR plans, which never call it,
/// use it.
const NO_ENDPOINT: &str = "http://127.0.0.1:0";

fn jsonrpc_result(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "jsonrpc": "2.0", "id": 1, "result": result }))
}

async fn server_answering(result: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(jsonrpc_result(result))
        .mount(&server)
        .await;
    server
}

fn get_network(protocol: u32) -> Value {
    json!({ "passphrase": "Test SDF Network ; September 2015", "protocolVersion": protocol })
}

fn simulation_ok() -> Value {
    json!({ "latestLedger": 1000, "transactionData": "AAAA" })
}

fn simulation_error(message: &str) -> Value {
    json!({ "latestLedger": 1000, "error": message })
}

async fn request_count(server: &MockServer) -> usize {
    server.received_requests().await.unwrap_or_default().len()
}

/// The parts of a result a replay must reproduce: everything except
/// `source` and the timing.
fn outcome(result: &CompatibilityResult) -> (String, Surface, Status, String, Option<String>) {
    (
        result.test_id.clone(),
        result.surface,
        result.status,
        result.summary.clone(),
        result.details.clone(),
    )
}

/// Rewrites one field of a stored entry in place.
fn edit_entry(path: &Path, field: &str, value: Value) {
    let mut entry: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    entry[field] = value;
    std::fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}

// --- execution error categories --------------------------------------------

#[tokio::test]
async fn rpc_assertion_mismatch_is_a_fail_with_the_reason() {
    let server = server_answering(get_network(27)).await;
    let cache = TempCache::new("rpc-fail");

    let result = run_one(
        &rpc_plan(rpc_fixture("rpc-fail")),
        &context(cache.store(live_policy())),
        &server.uri(),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    assert_eq!(result.source, ResultSource::Live);
    let details = result.details.expect("a failed assertion explains itself");
    assert!(
        details.contains("protocolVersion") && details.contains("27"),
        "{details}"
    );
}

#[tokio::test]
async fn rpc_timeout_is_an_error_not_a_fail() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(jsonrpc_result(get_network(28)).set_delay(Duration::from_secs(10)))
        .mount(&server)
        .await;
    let cache = TempCache::new("rpc-timeout");

    let result = run_one(
        &rpc_plan(rpc_fixture("rpc-timeout")),
        &context(cache.store(live_policy())),
        &server.uri(),
    )
    .await;

    assert_eq!(result.status, Status::Error);
    assert_eq!(result.surface, Surface::Rpc);
    let details = result.details.expect("an error carries its cause");
    assert!(details.contains("timed out"), "{details}");
}

#[tokio::test]
async fn rpc_transport_failure_is_an_error() {
    let cache = TempCache::new("rpc-transport");

    let result = run_one(
        &rpc_plan(rpc_fixture("rpc-transport")),
        &context(cache.store(live_policy())),
        NO_ENDPOINT,
    )
    .await;

    assert_eq!(result.status, Status::Error);
    assert!(
        result.summary.contains("failed to call"),
        "{}",
        result.summary
    );
}

#[tokio::test]
async fn rpc_json_rpc_error_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32603, "message": "internal error" }
        })))
        .mount(&server)
        .await;
    let cache = TempCache::new("rpc-jsonrpc-error");

    let result = run_one(
        &rpc_plan(rpc_fixture("rpc-jsonrpc-error")),
        &context(cache.store(live_policy())),
        &server.uri(),
    )
    .await;

    assert_eq!(result.status, Status::Error);
    let details = result.details.unwrap();
    assert!(
        details.contains("-32603") && details.contains("internal error"),
        "{details}"
    );
}

#[tokio::test]
async fn rpc_method_not_available_is_skipped_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32601, "message": "method not found" }
        })))
        .mount(&server)
        .await;
    let cache = TempCache::new("rpc-skip");

    let result = run_one(
        &rpc_plan(rpc_fixture("rpc-skip")),
        &context(cache.store(live_policy())),
        &server.uri(),
    )
    .await;

    assert_eq!(result.status, Status::Skipped);
    assert!(result.summary.contains("-32601"), "{}", result.summary);
}

#[tokio::test]
async fn soroban_unexpected_simulation_failure_is_a_fail() {
    let server = server_answering(simulation_error("HostError: Error(Contract, #1)")).await;
    let cache = TempCache::new("soroban-fail");

    let result = run_one(
        &soroban_plan(soroban_fixture(
            "soroban-fail",
            "kind = \"simulation-success\"\n",
        )),
        &context(cache.store(live_policy())),
        &server.uri(),
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    assert_eq!(result.surface, Surface::Soroban);
    assert_eq!(
        result.details.as_deref(),
        Some("HostError: Error(Contract, #1)"),
        "the simulation error is reported as the failure detail"
    );
}

#[tokio::test]
async fn soroban_expected_simulation_failure_is_a_pass() {
    let server = server_answering(simulation_error("HostError: Error(Contract, #1)")).await;
    let cache = TempCache::new("soroban-expected-error");

    let result = run_one(
        &soroban_plan(soroban_fixture(
            "soroban-expected-error",
            "kind = \"simulation-error\"\nmessage_contains = \"Contract\"\n",
        )),
        &context(cache.store(live_policy())),
        &server.uri(),
    )
    .await;

    assert_eq!(result.status, Status::Pass);
}

#[tokio::test]
async fn soroban_timeout_is_an_error_not_a_fail() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(jsonrpc_result(simulation_ok()).set_delay(Duration::from_secs(10)))
        .mount(&server)
        .await;
    let cache = TempCache::new("soroban-timeout");

    let result = run_one(
        &soroban_plan(soroban_fixture(
            "soroban-timeout",
            "kind = \"simulation-success\"\n",
        )),
        &context(cache.store(live_policy())),
        &server.uri(),
    )
    .await;

    assert_eq!(result.status, Status::Error);
    assert_eq!(result.summary, "failed to call simulateTransaction");
    let details = result.details.unwrap();
    assert!(details.contains("timed out"), "{details}");
}

#[tokio::test]
async fn xdr_decode_failure_is_a_fail() {
    let cache = TempCache::new("xdr-fail");

    let result = run_one(
        &xdr_plan(xdr_fixture(
            "xdr-fail",
            "decode-success",
            DECODE_FAILURE_BASE64,
        )),
        &context(cache.store(default_policy())),
        NO_ENDPOINT,
    )
    .await;

    assert_eq!(result.status, Status::Fail);
    assert_eq!(result.surface, Surface::Xdr);
}

#[tokio::test]
async fn an_error_result_is_never_cached_and_the_next_run_executes() {
    let cache = TempCache::new("error-not-cached");
    let plan = rpc_plan(rpc_fixture("error-not-cached"));

    // First run: nothing listening, so the call fails.
    let first = run_one(&plan, &context(cache.store(live_policy())), NO_ENDPOINT).await;
    assert_eq!(first.status, Status::Error);
    assert!(
        cache.entries().is_empty(),
        "an error result must not be stored"
    );

    // Second run: a healthy endpoint is actually called.
    let server = server_answering(get_network(28)).await;
    let second = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(second.status, Status::Pass);
    assert_eq!(second.source, ResultSource::Live);
    assert_eq!(request_count(&server).await, 1);
}

// --- cache hits -------------------------------------------------------------

#[tokio::test]
async fn rpc_cache_hit_replays_without_calling_the_endpoint() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-hit");
    let plan = rpc_plan(rpc_fixture("rpc-hit"));

    let first = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(first.status, Status::Pass);
    assert_eq!(first.source, ResultSource::Live);
    assert_eq!(request_count(&server).await, 1);

    let second = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(
        second.source,
        ResultSource::Cache,
        "a hit is marked replayed"
    );
    assert_eq!(outcome(&second), outcome(&first));
    assert_eq!(second.duration_ms, first.duration_ms);
    assert_eq!(request_count(&server).await, 1, "a hit must not re-execute");
}

#[tokio::test]
async fn rpc_cached_fail_is_replayed_as_a_fail() {
    let server = server_answering(get_network(27)).await;
    let cache = TempCache::new("rpc-hit-fail");
    let plan = rpc_plan(rpc_fixture("rpc-hit-fail"));

    let first = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(first.status, Status::Fail);

    let second = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(second.source, ResultSource::Cache);
    assert_eq!(outcome(&second), outcome(&first));
    assert_eq!(request_count(&server).await, 1);
}

#[tokio::test]
async fn soroban_cache_hit_replays_without_simulating_again() {
    let server = server_answering(simulation_ok()).await;
    let cache = TempCache::new("soroban-hit");
    let plan = soroban_plan(soroban_fixture(
        "soroban-hit",
        "kind = \"simulation-success\"\n",
    ));

    let first = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(first.status, Status::Pass);
    assert_eq!(first.source, ResultSource::Live);

    let second = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(second.source, ResultSource::Cache);
    assert_eq!(outcome(&second), outcome(&first));
    assert_eq!(request_count(&server).await, 1);
}

#[tokio::test]
async fn xdr_cache_hit_is_marked_replayed() {
    let cache = TempCache::new("xdr-hit");
    let plan = xdr_plan(xdr_fixture(
        "xdr-hit",
        "decode-success",
        VALID_STELLAR_VALUE,
    ));

    let first = run_one(&plan, &context(cache.store(default_policy())), NO_ENDPOINT).await;
    assert_eq!(first.source, ResultSource::Live);
    let second = run_one(&plan, &context(cache.store(default_policy())), NO_ENDPOINT).await;
    assert_eq!(second.source, ResultSource::Cache);
    assert_eq!(outcome(&second), outcome(&first));
}

#[tokio::test]
async fn a_hit_returns_the_stored_content_not_a_fresh_evaluation() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-hit-content");
    let plan = rpc_plan(rpc_fixture("rpc-hit-content"));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    let entry = cache.only_entry();
    let mut stored: Value = serde_json::from_slice(&std::fs::read(&entry).unwrap()).unwrap();
    assert_eq!(
        stored["result"]["source"], "live",
        "entries record a live run"
    );
    stored["result"]["summary"] = json!("planted marker");
    std::fs::write(&entry, serde_json::to_vec(&stored).unwrap()).unwrap();

    let replayed = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(replayed.summary, "planted marker");
    assert_eq!(replayed.source, ResultSource::Cache);
}

// --- bypass and invalidation ------------------------------------------------

#[tokio::test]
async fn live_results_are_not_cached_under_the_default_policy() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-default-policy");
    let plan = rpc_plan(rpc_fixture("rpc-default-policy"));

    for _ in 0..2 {
        let result = run_one(
            &plan,
            &context(cache.store(default_policy())),
            &server.uri(),
        )
        .await;
        assert_eq!(result.source, ResultSource::Live);
    }
    assert!(cache.entries().is_empty());
    assert_eq!(request_count(&server).await, 2);
}

#[tokio::test]
async fn disabled_cache_ignores_a_valid_entry_and_executes() {
    let rpc_server = server_answering(get_network(28)).await;
    let soroban_server = server_answering(simulation_ok()).await;
    let cache = TempCache::new("disabled");
    let rpc = rpc_plan(rpc_fixture("disabled-rpc"));
    let soroban = soroban_plan(soroban_fixture(
        "disabled-soroban",
        "kind = \"simulation-success\"\n",
    ));
    let xdr = xdr_plan(xdr_fixture(
        "disabled-xdr",
        "decode-success",
        VALID_STELLAR_VALUE,
    ));

    // Populate valid entries for all three surfaces.
    let enabled = context(cache.store(live_policy()));
    run_one(&rpc, &enabled, &rpc_server.uri()).await;
    run_one(&soroban, &enabled, &soroban_server.uri()).await;
    run_one(&xdr, &enabled, NO_ENDPOINT).await;
    assert_eq!(cache.entries().len(), 3);

    let disabled = context(cache.store(disabled_policy()));
    assert_eq!(
        run_one(&rpc, &disabled, &rpc_server.uri()).await.source,
        ResultSource::Live
    );
    assert_eq!(
        run_one(&soroban, &disabled, &soroban_server.uri())
            .await
            .source,
        ResultSource::Live
    );
    assert_eq!(
        run_one(&xdr, &disabled, NO_ENDPOINT).await.source,
        ResultSource::Live
    );
    assert_eq!(request_count(&rpc_server).await, 2);
    assert_eq!(request_count(&soroban_server).await, 2);
}

#[tokio::test]
async fn a_stale_live_entry_is_not_replayed_and_is_refreshed() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-stale");
    let plan = rpc_plan(rpc_fixture("rpc-stale"));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    // Backdate the entry far past the one-hour `live_max_age`.
    edit_entry(&cache.only_entry(), "created_at", json!(0));

    let stale = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(stale.source, ResultSource::Live, "a stale entry is a miss");
    assert_eq!(request_count(&server).await, 2);

    // The re-execution replaced the stale entry with a fresh one.
    let fresh = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(fresh.source, ResultSource::Cache);
    assert_eq!(request_count(&server).await, 2);
}

#[tokio::test]
async fn a_live_entry_stamped_in_the_future_is_not_replayed() {
    let server = server_answering(simulation_ok()).await;
    let cache = TempCache::new("soroban-future");
    let plan = soroban_plan(soroban_fixture(
        "soroban-future",
        "kind = \"simulation-success\"\n",
    ));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    let tomorrow = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 24 * 3600;
    edit_entry(&cache.only_entry(), "created_at", json!(tomorrow));

    let result = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(result.source, ResultSource::Live);
    assert_eq!(request_count(&server).await, 2);
}

#[tokio::test]
async fn a_live_entry_with_an_unrepresentable_timestamp_re_executes() {
    // `u64::MAX` seconds is past the end of `SystemTime` on every platform
    // (Windows' range is far narrower than Unix's). Reading the entry must
    // be a miss, not an overflow panic that takes the run down.
    let server = server_answering(simulation_ok()).await;
    let cache = TempCache::new("soroban-overflow");
    let plan = soroban_plan(soroban_fixture(
        "soroban-overflow",
        "kind = \"simulation-success\"\n",
    ));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    edit_entry(&cache.only_entry(), "created_at", json!(u64::MAX));

    let result = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(result.source, ResultSource::Live);
    assert_eq!(result.status, Status::Pass);
    assert_eq!(request_count(&server).await, 2);
}

#[tokio::test]
async fn an_xdr_entry_does_not_go_stale() {
    let cache = TempCache::new("xdr-old");
    let plan = xdr_plan(xdr_fixture(
        "xdr-old",
        "decode-success",
        VALID_STELLAR_VALUE,
    ));

    run_one(&plan, &context(cache.store(live_policy())), NO_ENDPOINT).await;
    edit_entry(&cache.only_entry(), "created_at", json!(0));

    let result = run_one(&plan, &context(cache.store(live_policy())), NO_ENDPOINT).await;
    assert_eq!(result.source, ResultSource::Cache);
}

#[tokio::test]
async fn a_changed_run_context_invalidates_the_entry() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-context-change");
    let plan = rpc_plan(rpc_fixture("rpc-context-change"));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;

    let mut moved_network = context(cache.store(live_policy()));
    moved_network.network.observed_protocol = Some(ProtocolVersion(29));
    let result = run_one(&plan, &moved_network, &server.uri()).await;
    assert_eq!(result.source, ResultSource::Live);
    assert_eq!(request_count(&server).await, 2);
    assert_eq!(cache.entries().len(), 2, "each context has its own entry");
}

#[tokio::test]
async fn a_cleared_cache_executes_again() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-cleared");
    let plan = rpc_plan(rpc_fixture("rpc-cleared"));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    cache.store(live_policy()).clear().unwrap();

    let result = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(result.source, ResultSource::Live);
    assert_eq!(request_count(&server).await, 2);
}

// --- corrupted and unreadable entries ---------------------------------------

#[tokio::test]
async fn a_corrupted_rpc_entry_is_a_miss_and_is_rewritten() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-corrupt");
    let plan = rpc_plan(rpc_fixture("rpc-corrupt"));

    let first = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    let entry = cache.only_entry();
    std::fs::write(&entry, b"{ not json").unwrap();

    let second = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(second.source, ResultSource::Live);
    assert_eq!(outcome(&second), outcome(&first));
    assert_eq!(
        request_count(&server).await,
        2,
        "a corrupt entry re-executes"
    );

    // The re-execution wrote a valid entry over the corrupt one.
    let third = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(third.source, ResultSource::Cache);
    assert_eq!(request_count(&server).await, 2);
}

#[tokio::test]
async fn a_corrupted_soroban_entry_is_a_miss() {
    let server = server_answering(simulation_ok()).await;
    let cache = TempCache::new("soroban-corrupt");
    let plan = soroban_plan(soroban_fixture(
        "soroban-corrupt",
        "kind = \"simulation-success\"\n",
    ));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    std::fs::write(cache.only_entry(), b"").unwrap();

    let result = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(result.source, ResultSource::Live);
    assert_eq!(result.status, Status::Pass);
    assert_eq!(request_count(&server).await, 2);
}

#[tokio::test]
async fn a_corrupted_xdr_entry_is_a_miss() {
    let cache = TempCache::new("xdr-corrupt");
    let plan = xdr_plan(xdr_fixture(
        "xdr-corrupt",
        "decode-success",
        VALID_STELLAR_VALUE,
    ));

    run_one(&plan, &context(cache.store(default_policy())), NO_ENDPOINT).await;
    std::fs::write(cache.only_entry(), b"\x00\xffgarbage").unwrap();

    let result = run_one(&plan, &context(cache.store(default_policy())), NO_ENDPOINT).await;
    assert_eq!(result.source, ResultSource::Live);
    assert_eq!(result.status, Status::Pass);
}

#[tokio::test]
async fn an_entry_from_another_cache_format_is_a_miss() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-old-format");
    let plan = rpc_plan(rpc_fixture("rpc-old-format"));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    edit_entry(&cache.only_entry(), "format", json!(1));

    let result = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(result.source, ResultSource::Live);
    assert_eq!(request_count(&server).await, 2);
}

#[tokio::test]
async fn an_entry_written_for_a_different_key_is_a_miss() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-wrong-key");
    let plan = rpc_plan(rpc_fixture("rpc-wrong-key"));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    edit_entry(
        &cache.only_entry(),
        "key",
        json!("canary-cache-key/3\nsomething else\n"),
    );

    let result = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(result.source, ResultSource::Live);
    assert_eq!(request_count(&server).await, 2);
}

#[tokio::test]
async fn an_unreadable_entry_is_a_miss_and_does_not_fail_the_run() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-unreadable");
    let plan = rpc_plan(rpc_fixture("rpc-unreadable"));

    run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    // A directory where the entry file should be: reading it fails, and so
    // does storing the new result over it. Neither may surface as a result.
    let entry = cache.only_entry();
    std::fs::remove_file(&entry).unwrap();
    std::fs::create_dir(&entry).unwrap();

    let result = run_one(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    assert_eq!(result.source, ResultSource::Live);
    assert_eq!(result.status, Status::Pass);
    assert_eq!(request_count(&server).await, 2);
    assert!(entry.is_dir(), "the unreadable entry is left alone");
}

#[tokio::test]
async fn one_corrupt_entry_does_not_affect_another_fixture() {
    let server = server_answering(get_network(28)).await;
    let cache = TempCache::new("rpc-corrupt-neighbour");
    let mut plan = CompatibilityPlan::default();
    plan.rpc.push(rpc_fixture("neighbour-a"));
    plan.rpc.push(rpc_fixture("neighbour-b"));

    execute(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    let entries = cache.entries();
    assert_eq!(entries.len(), 2);
    std::fs::write(&entries[0], b"corrupt").unwrap();

    let results = execute(&plan, &context(cache.store(live_policy())), &server.uri()).await;
    let sources: Vec<ResultSource> = results.iter().map(|r| r.source).collect();
    assert_eq!(
        sources
            .iter()
            .filter(|s| **s == ResultSource::Cache)
            .count(),
        1,
        "{sources:?}"
    );
    assert_eq!(
        sources.iter().filter(|s| **s == ResultSource::Live).count(),
        1,
        "{sources:?}"
    );
    assert!(results.iter().all(|r| r.status == Status::Pass));
    assert_eq!(request_count(&server).await, 3);
}

/// The Soroban requests counted above are `simulateTransaction` calls: a
/// mock that only answers that method is enough for a passing run.
#[tokio::test]
async fn soroban_runs_call_simulate_transaction() {
    let server = MockServer::start().await;
    Mock::given(body_string_contains("simulateTransaction"))
        .respond_with(jsonrpc_result(simulation_ok()))
        .expect(1)
        .mount(&server)
        .await;
    let cache = TempCache::new("soroban-method");

    let result = run_one(
        &soroban_plan(soroban_fixture(
            "soroban-method",
            "kind = \"simulation-success\"\n",
        )),
        &context(cache.store(live_policy())),
        &server.uri(),
    )
    .await;
    assert_eq!(result.status, Status::Pass);
}
