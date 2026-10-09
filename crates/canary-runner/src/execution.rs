//! Executing a [`CompatibilityPlan`].
//!
//! XDR fixtures are offline and run synchronously, in order. RPC and
//! Soroban fixtures are network-bound and run concurrently within their
//! own surface (bounded by [`ExecutionContext::options`]'s
//! `max_concurrency`), then reordered back to fixture order so the overall
//! result list stays deterministic regardless of which network call
//! happened to finish first.

use canary_core::{
    CompatibilityResult, ExecutionContext, FixtureMetadata, ProtocolVersion, Status, Surface,
};
use canary_rpc::{HttpRpcClient, RpcFixture, RpcRunner};
use canary_soroban::{SorobanFixture, SorobanRunner};
use canary_xdr::{DefaultXdrRunner, XdrFixture, XdrRunner};
use futures::stream::{self, StreamExt};

use crate::scheduler::CompatibilityPlan;

/// Runs every fixture in `plan` and returns results in a deterministic,
/// surface-grouped order: all XDR results, then all RPC results, then all
/// Soroban results, each in the fixture's original planned order.
pub async fn execute(
    plan: &CompatibilityPlan,
    context: &ExecutionContext,
    rpc_endpoint: &str,
) -> Vec<CompatibilityResult> {
    let concurrency = context.options.max_concurrency.max(1) as usize;
    let client = HttpRpcClient::new(rpc_endpoint.to_string())
        .with_timeout(std::time::Duration::from_secs(context.options.rpc_timeout));

    let mut results = run_xdr(&plan.xdr, context);
    results.extend(run_rpc(&plan.rpc, context, client.clone(), concurrency).await);
    results.extend(run_soroban(&plan.soroban, context, client, concurrency).await);
    results
}

fn run_xdr(fixtures: &[XdrFixture], context: &ExecutionContext) -> Vec<CompatibilityResult> {
    let runner = DefaultXdrRunner;
    fixtures
        .iter()
        .map(|fixture| {
            let cache_key = cache_key(context, &fixture.metadata);
            if let Some(cached) = context.cache.get(&cache_key, Surface::Xdr) {
                return cached;
            }

            let result = runner.run(fixture, context).unwrap_or_else(|e| {
                error_result(
                    &fixture.metadata.id,
                    fixture.metadata.protocol,
                    Surface::Xdr,
                    &e.to_string(),
                )
            });

            let _ = context.cache.put(&cache_key, Surface::Xdr, &result);
            result
        })
        .collect()
}

async fn run_rpc(
    fixtures: &[RpcFixture],
    context: &ExecutionContext,
    client: HttpRpcClient,
    concurrency: usize,
) -> Vec<CompatibilityResult> {
    let runner = canary_rpc::DefaultRpcRunner::new(client);

    let mut indexed: Vec<(usize, CompatibilityResult)> = stream::iter(fixtures.iter().enumerate())
        .map(|(index, fixture)| {
            let runner = &runner;
            async move {
                let cache_key = cache_key(context, &fixture.metadata);
                if let Some(cached) = context.cache.get(&cache_key, Surface::Rpc) {
                    return (index, cached);
                }

                let result = runner.run(fixture, context).await.unwrap_or_else(|e| {
                    error_result(
                        &fixture.metadata.id,
                        fixture.metadata.protocol,
                        Surface::Rpc,
                        &e.to_string(),
                    )
                });
                let _ = context.cache.put(&cache_key, Surface::Rpc, &result);
                (index, result)
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    indexed.sort_by_key(|(index, _)| *index);
    indexed.into_iter().map(|(_, result)| result).collect()
}

async fn run_soroban(
    fixtures: &[SorobanFixture],
    context: &ExecutionContext,
    client: HttpRpcClient,
    concurrency: usize,
) -> Vec<CompatibilityResult> {
    let runner = canary_soroban::DefaultSorobanRunner::new(client);

    let mut indexed: Vec<(usize, CompatibilityResult)> = stream::iter(fixtures.iter().enumerate())
        .map(|(index, fixture)| {
            let runner = &runner;
            async move {
                let cache_key = cache_key(context, &fixture.metadata);
                if let Some(cached) = context.cache.get(&cache_key, Surface::Soroban) {
                    return (index, cached);
                }

                let result = runner.run(fixture, context).await.unwrap_or_else(|e| {
                    error_result(
                        &fixture.metadata.id,
                        fixture.metadata.protocol,
                        Surface::Soroban,
                        &e.to_string(),
                    )
                });
                let _ = context.cache.put(&cache_key, Surface::Soroban, &result);
                (index, result)
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    indexed.sort_by_key(|(index, _)| *index);
    indexed.into_iter().map(|(_, result)| result).collect()
}

fn error_result(
    fixture_id: &str,
    protocol: ProtocolVersion,
    surface: Surface,
    message: &str,
) -> CompatibilityResult {
    CompatibilityResult {
        test_id: fixture_id.to_string(),
        protocol,
        surface,
        status: Status::Error,
        summary: "failed to execute fixture".to_string(),
        details: Some(message.to_string()),
        duration_ms: 0,
        fixture_id: Some(fixture_id.to_string()),
        source: canary_core::ResultSource::Live,
    }
}

/// Path-free identity of the project for cache purposes: its type, its
/// sorted capabilities and its Git state. The absolute project path and the
/// directory name are deliberately absent so a moved or renamed checkout
/// keeps its entries, and a different project cannot collide on a shared
/// name.
fn project_fingerprint(context: &ExecutionContext) -> String {
    let mut capabilities: Vec<String> = context
        .project
        .capabilities
        .iter()
        .map(|c| format!("{c:?}"))
        .collect();
    capabilities.sort();
    let git = match (&context.git.commit, context.git.is_dirty) {
        (Some(commit), Some(true)) => format!("{commit}+dirty"),
        (Some(commit), _) => commit.clone(),
        (None, _) => "no-commit".to_string(),
    };
    format!(
        "{:?}|{}|{git}",
        context.project.project_type,
        capabilities.join(",")
    )
}

fn cache_key(context: &ExecutionContext, fixture: &FixtureMetadata) -> canary_core::CacheKey {
    canary_core::CacheKey {
        fixture_id: fixture.id.clone(),
        fixture_digest: fixture.content_digest.clone(),
        protocol: context.protocol,
        project_fingerprint: project_fingerprint(context),
        network: context.network.name.to_string(),
        rpc_endpoint: context.network.rpc_url.clone(),
        observed_protocol: context.network.observed_protocol,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canary_core::{
        CacheStore, FixtureStore, GitContext, NetworkContext, NetworkName, ProjectContext,
        ProjectType, RunOptions,
    };
    use canary_fixtures::parse_fixture_str;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn context() -> ExecutionContext {
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
            cache: CacheStore::new(
                std::env::temp_dir().join("canary-runner-execution-tests-cache"),
            ),
            options: RunOptions::default(),
        }
    }

    /// [`context()`] backed by a cache directory nothing has written to.
    ///
    /// Every test here leaves `network.rpc_url` as the literal `"unused"` and
    /// uses fixed fixture ids, so the shared temp cache can hand back entries
    /// written by earlier runs of this suite. A cache hit
    /// returns the stored result *without contacting the mock server*, which
    /// is precisely the concurrent execution the out-of-order tests below
    /// exist to observe, so they have to start from an empty cache.
    fn uncached_context() -> ExecutionContext {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let mut context = context();
        // pid + nanos + counter: the timestamp alone cannot separate the
        // threads the harness runs in parallel.
        let unique = format!(
            "canary-runner-uncached-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        );
        context.cache = CacheStore::new(std::env::temp_dir().join(unique));
        context
    }

    fn rpc_fixture(id: &str, method: &str) -> RpcFixture {
        RpcFixture::from_loaded(
            &parse_fixture_str(
                &format!(
                    "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"c\"\ndescription = \"d\"\nmethod = \"{method}\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"protocolVersion\"\n"
                ),
                std::path::Path::new("r.toml"),
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn xdr_fixtures_run_synchronously_in_order() {
        let f1 = XdrFixture::from_loaded(
            &parse_fixture_str(
                "id = \"a\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"c\"\ndescription = \"d\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n",
                std::path::Path::new("a.toml"),
            )
            .unwrap(),
        )
        .unwrap();
        let f2 = XdrFixture::from_loaded(
            &parse_fixture_str(
                "id = \"b\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"c\"\ndescription = \"d\"\ntype = \"StellarValue\"\nkind = \"decode-failure\"\nvalue_base64 = \"AAAA\"\n",
                std::path::Path::new("b.toml"),
            )
            .unwrap(),
        )
        .unwrap();

        let results = run_xdr(&[f1, f2], &context());
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].test_id, "a");
        assert_eq!(results[1].test_id, "b");
    }

    #[tokio::test]
    async fn execute_returns_results_grouped_by_surface_in_fixture_order() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "passphrase": "Test SDF Network ; September 2015", "protocolVersion": 28 }
            })))
            .mount(&server)
            .await;

        let mut plan = CompatibilityPlan::default();
        plan.xdr.push(
            XdrFixture::from_loaded(
                &parse_fixture_str(
                    "id = \"x\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"c\"\ndescription = \"d\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"AAAA\"\n",
                    std::path::Path::new("x.toml"),
                )
                .unwrap(),
            )
            .unwrap(),
        );
        plan.rpc.push(
            RpcFixture::from_loaded(
                &parse_fixture_str(
                    "id = \"r\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"c\"\ndescription = \"d\"\nmethod = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n",
                    std::path::Path::new("r.toml"),
                )
                .unwrap(),
            )
            .unwrap(),
        );

        let results = execute(&plan, &context(), &server.uri()).await;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].test_id, "x");
        assert_eq!(results[1].test_id, "r");
    }

    fn soroban_fixture(id: &str, sequence_number: i64) -> SorobanFixture {
        use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
        let source_account = StrkeyPublicKey([0u8; 32]).to_string();
        let contract_id = StrkeyContract([0u8; 32]).to_string();
        SorobanFixture::from_loaded(
            &parse_fixture_str(
                &format!(
                    "id = \"{id}\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"cap-85\"\ndescription = \"test\"\nsource_account = \"{source_account}\"\ncontract_id = \"{contract_id}\"\nfunction = \"hello\"\nsequence_number = {sequence_number}\n\n[[args]]\nkind = \"symbol\"\nvalue = \"world\"\n\n[expect]\nkind = \"simulation-success\"\n"
                ),
                std::path::Path::new("s.toml"),
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn soroban_results_are_grouped_after_rpc_results() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "latestLedger": 1000, "transactionData": "AAAA" }
            })))
            .mount(&server)
            .await;

        let mut plan = CompatibilityPlan::default();
        plan.rpc.push(
            RpcFixture::from_loaded(
                &parse_fixture_str(
                    "id = \"r\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"c\"\ndescription = \"d\"\nmethod = \"get-network\"\n\n[[assert]]\nkind = \"field-exists\"\nfield = \"passphrase\"\n",
                    std::path::Path::new("r.toml"),
                )
                .unwrap(),
            )
            .unwrap(),
        );
        plan.soroban.push(soroban_fixture("s1", 1));
        plan.soroban.push(soroban_fixture("s2", 2));

        let results = execute(&plan, &context(), &server.uri()).await;
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].test_id, "r");
        assert_eq!(results[0].surface, Surface::Rpc);
        assert_eq!(results[1].test_id, "s1");
        assert_eq!(results[1].surface, Surface::Soroban);
        assert_eq!(results[2].test_id, "s2");
        assert_eq!(results[2].surface, Surface::Soroban);
        assert_eq!(results[2].status, Status::Pass);
    }

    #[tokio::test]
    async fn soroban_results_keep_fixture_order_when_responses_finish_out_of_order() {
        let server = MockServer::start().await;

        let first = soroban_fixture("s1", 1);
        let second = soroban_fixture("s2", 2);
        // The two fixtures differ only in sequence number, so their
        // simulation request bodies are unique and the envelope itself is
        // a reliable request matcher.
        let first_envelope =
            canary_soroban::build_invoke_transaction_envelope(&first.invocation).unwrap();
        let second_envelope =
            canary_soroban::build_invoke_transaction_envelope(&second.invocation).unwrap();

        // The first fixture's response is delayed so the second fixture's
        // finishes first, completing out of fixture order.
        Mock::given(wiremock::matchers::body_string_contains(first_envelope))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": { "latestLedger": 1000, "transactionData": "AAAA" }
                    }))
                    .set_delay(std::time::Duration::from_millis(500)),
            )
            .mount(&server)
            .await;
        Mock::given(wiremock::matchers::body_string_contains(second_envelope))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "latestLedger": 1000, "transactionData": "AAAA" }
            })))
            .mount(&server)
            .await;

        let mut plan = CompatibilityPlan::default();
        plan.soroban.push(first);
        plan.soroban.push(second);

        // A cache hit returns the stored result without contacting the mock,
        // so with the shared cache directory these fixtures would be served
        // from a previous run's entries and never complete out of order.
        let context = uncached_context();
        let results = execute(&plan, &context, &server.uri()).await;
        let _ = context.cache.clear();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].test_id, "s1");
        assert_eq!(results[1].test_id, "s2");
        assert_eq!(results[0].status, Status::Pass);
        assert_eq!(results[1].status, Status::Pass);
    }

    /// Issue #282: `run_rpc` uses the same collect-into-`(index, result)`
    /// pairs then `sort_by_key` pattern as `run_soroban`, but had no
    /// equivalent test, so RPC's determinism guarantee under concurrent
    /// completion was unverified.
    ///
    /// The two fixtures call different RPC methods, so the JSON-RPC `method`
    /// name in the request body identifies each one: the first fixture's
    /// response is delayed and the second's returns immediately, making
    /// completion order the reverse of fixture order.
    #[tokio::test]
    async fn rpc_results_keep_fixture_order_when_responses_finish_out_of_order() {
        let server = MockServer::start().await;

        Mock::given(wiremock::matchers::body_string_contains("getNetwork"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "passphrase": "Test SDF Network ; September 2015",
                            "protocolVersion": 28
                        }
                    }))
                    .set_delay(std::time::Duration::from_millis(500)),
            )
            .mount(&server)
            .await;
        Mock::given(wiremock::matchers::body_string_contains("getLatestLedger"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "id": "AAAA", "protocolVersion": 28, "sequence": 1000 }
            })))
            .mount(&server)
            .await;

        let mut plan = CompatibilityPlan::default();
        plan.rpc.push(rpc_fixture("r1", "get-network"));
        plan.rpc.push(rpc_fixture("r2", "get-latest-ledger"));

        let context = uncached_context();
        let results = execute(&plan, &context, &server.uri()).await;
        let _ = context.cache.clear();

        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].test_id, "r1",
            "the fixture whose response arrived last must still be reported first"
        );
        assert_eq!(results[1].test_id, "r2");
        assert_eq!(results[0].status, Status::Pass);
        assert_eq!(results[1].status, Status::Pass);
    }

    const VALID_STELLAR_VALUE: &str =
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn xdr_fixture(id: &str, value_base64: &str) -> XdrFixture {
        XdrFixture::from_loaded(
            &parse_fixture_str(
                &format!(
                    "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"c\"\ndescription = \"d\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{value_base64}\"\n"
                ),
                std::path::Path::new("f.toml"),
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn editing_a_fixture_cannot_reuse_the_previous_result() {
        let context = uncached_context();

        let passing = xdr_fixture("same-id", VALID_STELLAR_VALUE);
        let first = run_xdr(std::slice::from_ref(&passing), &context);
        assert_eq!(first[0].status, Status::Pass);
        assert!(
            context
                .cache
                .get(&cache_key(&context, &passing.metadata), Surface::Xdr)
                .is_some(),
            "the passing result was stored"
        );

        // Same id, same project, same network, but the fixture changed.
        let edited = xdr_fixture("same-id", "not-valid-xdr!!");
        let second = run_xdr(std::slice::from_ref(&edited), &context);
        assert_eq!(
            second[0].status,
            Status::Fail,
            "the edited fixture must be evaluated, not served the old pass"
        );

        // Going back to the original content is a hit on the original entry.
        let third = run_xdr(std::slice::from_ref(&passing), &context);
        assert_eq!(third[0].status, Status::Pass);
    }

    #[test]
    fn an_unchanged_fixture_is_served_from_the_cache() {
        let context = uncached_context();
        let fixture = xdr_fixture("cached", VALID_STELLAR_VALUE);
        let key = cache_key(&context, &fixture.metadata);

        let mut planted = run_xdr(std::slice::from_ref(&fixture), &context).remove(0);
        planted.summary = "planted marker".into();
        context.cache.put(&key, Surface::Xdr, &planted).unwrap();

        let result = run_xdr(std::slice::from_ref(&fixture), &context).remove(0);
        assert_eq!(result.summary, "planted marker");
    }

    #[test]
    fn cache_key_does_not_depend_on_the_project_path_or_name() {
        let mut a = uncached_context();
        let mut b = uncached_context();
        a.project.root = "/home/one/project".into();
        a.project.name = "project".into();
        b.project.root = "C:\\work\\other-checkout".into();
        b.project.name = "renamed".into();
        let fixture = xdr_fixture("path-free", VALID_STELLAR_VALUE);
        assert_eq!(
            cache_key(&a, &fixture.metadata),
            cache_key(&b, &fixture.metadata)
        );
    }

    #[test]
    fn cache_key_changes_with_project_network_and_observation() {
        let fixture = xdr_fixture("axes", VALID_STELLAR_VALUE);
        let base = cache_key(&uncached_context(), &fixture.metadata);

        let mut context = uncached_context();
        context.project.capabilities = vec![canary_core::Capability::SorobanContract];
        assert_ne!(cache_key(&context, &fixture.metadata), base, "capabilities");

        let mut context = uncached_context();
        context.git.commit = Some("abc123".into());
        assert_ne!(cache_key(&context, &fixture.metadata), base, "commit");
        context.git.is_dirty = Some(true);
        let dirty = cache_key(&context, &fixture.metadata);
        context.git.is_dirty = Some(false);
        assert_ne!(dirty, cache_key(&context, &fixture.metadata), "dirty flag");

        let mut context = uncached_context();
        context.network.name = NetworkName::Mainnet;
        assert_ne!(cache_key(&context, &fixture.metadata), base, "network");

        let mut context = uncached_context();
        context.network.rpc_url = "https://other.example".into();
        assert_ne!(cache_key(&context, &fixture.metadata), base, "endpoint");

        let mut context = uncached_context();
        context.network.observed_protocol = Some(ProtocolVersion(29));
        assert_ne!(cache_key(&context, &fixture.metadata), base, "observed");

        let mut context = uncached_context();
        context.protocol = ProtocolVersion(29);
        assert_ne!(cache_key(&context, &fixture.metadata), base, "target");
    }
}
