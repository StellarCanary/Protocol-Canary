use canary_core::{
    CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext, NetworkName,
    ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::RpcFixture;
use canary_runner::scheduler::CompatibilityPlan;
use canary_soroban::SorobanFixture;
use std::path::{Path, PathBuf};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_temp_dir(prefix: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{pid}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn context(cache_dir: &Path, rpc_url: &str) -> ExecutionContext {
    ExecutionContext {
        protocol: ProtocolVersion(28),
        project: ProjectContext {
            root: ".".into(),
            name: "test-project".into(),
            project_type: ProjectType::Soroban,
            capabilities: vec![],
        },
        network: NetworkContext {
            name: NetworkName::Testnet,
            rpc_url: rpc_url.to_string(),
            passphrase: "Test SDF Network ; September 2015".into(),
            observed_protocol: Some(ProtocolVersion(28)),
        },
        fixtures: FixtureStore::default(),
        git: GitContext {
            commit: Some("commit1".into()),
            branch: Some("main".into()),
            is_dirty: Some(false),
        },
        cache: CacheStore::new(cache_dir),
        options: RunOptions::default(),
    }
}

fn rpc_fixture(id: &str) -> RpcFixture {
    RpcFixture::from_loaded(
        &parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"c\"\ndescription = \"d\"\nmethod = \"get-network\"\n\n[[assert]]\nkind = \"field-equals\"\nfield = \"protocolVersion\"\nvalue = 28\n"
            ),
            Path::new("rpc.toml"),
        )
        .unwrap(),
    )
    .unwrap()
}

fn soroban_fixture(id: &str) -> SorobanFixture {
    use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
    let source_account = StrkeyPublicKey([0u8; 32]).to_string();
    let contract_id = StrkeyContract([0u8; 32]).to_string();
    SorobanFixture::from_loaded(
        &parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"cap-85\"\ndescription = \"test\"\nsource_account = \"{source_account}\"\ncontract_id = \"{contract_id}\"\nfunction = \"hello\"\nsequence_number = 1\n\n[[args]]\nkind = \"symbol\"\nvalue = \"world\"\n\n[expect]\nkind = \"simulation-success\"\n"
            ),
            Path::new("soroban.toml"),
        )
        .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn execution_error_categorization_rpc() {
    let server = MockServer::start().await;

    // 1. Simulation/assertion failure: Status::Fail
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 27 } // mismatch
        })))
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.rpc.push(rpc_fixture("rpc-fail"));

    let cache_dir = create_temp_dir("execution-err-rpc-fail");
    let ctx = context(&cache_dir, &server.uri());

    let results = canary_runner::execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].status,
        Status::Fail,
        "assertion mismatch must be Status::Fail"
    );
    assert!(results[0].details.is_some());

    // 2. Transport/network error: Status::Error
    let dead_url = "http://127.0.0.1:1";
    let mut err_plan = CompatibilityPlan::default();
    err_plan.rpc.push(rpc_fixture("rpc-err"));
    let err_ctx = context(&cache_dir, dead_url);

    let err_results = canary_runner::execute(&err_plan, &err_ctx, dead_url).await;
    assert_eq!(err_results.len(), 1);
    assert_eq!(
        err_results[0].status,
        Status::Error,
        "transport error must be Status::Error"
    );
    assert!(err_results[0].details.is_some());
    let _ = std::fs::remove_dir_all(&cache_dir);
}

#[tokio::test]
async fn execution_error_categorization_soroban() {
    let server = MockServer::start().await;

    // 1. Simulation error: Status::Fail
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "latestLedger": 1000, "error": "Host error: transaction failed" }
        })))
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.soroban.push(soroban_fixture("soroban-fail"));

    let cache_dir = create_temp_dir("execution-err-soroban-fail");
    let ctx = context(&cache_dir, &server.uri());

    let results = canary_runner::execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].status,
        Status::Fail,
        "simulation error must be Status::Fail"
    );
    assert!(results[0].details.is_some());

    // 2. Transport error: Status::Error
    let dead_url = "http://127.0.0.1:1";
    let mut err_plan = CompatibilityPlan::default();
    err_plan.soroban.push(soroban_fixture("soroban-err"));
    let err_ctx = context(&cache_dir, dead_url);

    let err_results = canary_runner::execute(&err_plan, &err_ctx, dead_url).await;
    assert_eq!(err_results.len(), 1);
    assert_eq!(
        err_results[0].status,
        Status::Error,
        "transport failure must be Status::Error"
    );
    let _ = std::fs::remove_dir_all(&cache_dir);
}

#[tokio::test]
async fn rpc_and_soroban_cache_hit_sets_replayed_true() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 28, "latestLedger": 100 }
        })))
        .expect(2) // 1 RPC + 1 Soroban on first run; 0 on second run!
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.rpc.push(rpc_fixture("rpc-cache-1"));
    plan.soroban.push(soroban_fixture("soroban-cache-1"));

    let cache_dir = create_temp_dir("cache-hit-test");
    let ctx = context(&cache_dir, &server.uri());

    // First run: executes checks, populates cache
    let results_1 = canary_runner::execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(results_1.len(), 2);
    assert_eq!(results_1[0].status, Status::Pass);
    assert_eq!(results_1[1].status, Status::Pass);
    assert!(!results_1[0].replayed, "first run is not replayed");
    assert!(!results_1[1].replayed, "first run is not replayed");

    // Second run: cache hit
    let results_2 = canary_runner::execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(results_2.len(), 2);
    assert_eq!(results_2[0].status, Status::Pass);
    assert_eq!(results_2[1].status, Status::Pass);
    assert!(
        results_2[0].replayed,
        "RPC result on cache hit must set replayed: true"
    );
    assert!(
        results_2[1].replayed,
        "Soroban result on cache hit must set replayed: true"
    );
    assert_eq!(results_2[0].test_id, results_1[0].test_id);
    assert_eq!(results_2[1].test_id, results_1[1].test_id);

    let _ = std::fs::remove_dir_all(&cache_dir);
}

#[tokio::test]
async fn cache_disabled_bypasses_cache() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 28 }
        })))
        .expect(2) // executed both times because cache is disabled
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.rpc.push(rpc_fixture("rpc-no-cache"));

    let cache_dir = create_temp_dir("cache-disabled-test");
    let mut ctx = context(&cache_dir, &server.uri());

    // Run 1: with cache
    let r1 = canary_runner::execute(&plan, &ctx, &server.uri()).await;
    assert!(!r1[0].replayed);

    // Run 2: with no_cache = true
    ctx.options.no_cache = true;
    let r2 = canary_runner::execute(&plan, &ctx, &server.uri()).await;
    assert!(
        !r2[0].replayed,
        "execution with no_cache=true must bypass cache and set replayed: false"
    );

    let _ = std::fs::remove_dir_all(&cache_dir);
}

#[tokio::test]
async fn corrupted_or_invalid_cache_file_causes_reexecution() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 28 }
        })))
        .expect(2) // executed initial run + re-executed after corrupting cache file
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.rpc.push(rpc_fixture("rpc-corrupt"));

    let cache_dir = create_temp_dir("cache-corrupt-test");
    let ctx = context(&cache_dir, &server.uri());

    // Run 1: cache populated
    let r1 = canary_runner::execute(&plan, &ctx, &server.uri()).await;
    assert!(!r1[0].replayed);

    // Corrupt all json files in cache_dir
    for entry in std::fs::read_dir(&cache_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().map_or(false, |ext| ext == "json") {
            std::fs::write(entry.path(), b"NOT VALID JSON CORRUPTED").unwrap();
        }
    }

    // Run 2: corrupted cache file should be ignored as cache miss and re-executed safely
    let r2 = canary_runner::execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(r2[0].status, Status::Pass);
    assert!(
        !r2[0].replayed,
        "corrupted cache file should trigger re-execution with replayed: false"
    );

    let _ = std::fs::remove_dir_all(&cache_dir);
}

#[tokio::test]
async fn stale_cache_key_invalidation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 28 }
        })))
        .expect(2)
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.rpc.push(rpc_fixture("rpc-stale"));

    let cache_dir = create_temp_dir("cache-stale-test");
    let mut ctx1 = context(&cache_dir, &server.uri());
    ctx1.protocol = ProtocolVersion(28);

    // Run 1: protocol 28
    let r1 = canary_runner::execute(&plan, &ctx1, &server.uri()).await;
    assert!(!r1[0].replayed);

    // Run 2: changed protocol target (stale cache key)
    let mut ctx2 = context(&cache_dir, &server.uri());
    ctx2.protocol = ProtocolVersion(29);
    let r2 = canary_runner::execute(&plan, &ctx2, &server.uri()).await;
    assert!(
        !r2[0].replayed,
        "different protocol in cache key must miss cache and re-execute"
    );

    let _ = std::fs::remove_dir_all(&cache_dir);
}
