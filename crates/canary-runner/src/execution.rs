//! Executing a [`CompatibilityPlan`](crate::scheduler::CompatibilityPlan).
//!
//! XDR fixtures are offline and run synchronously, in order. RPC and
//! Soroban fixtures are network-bound and run concurrently within their
//! own surface (bounded by [`ExecutionContext::options`]'s
//! `max_concurrency`), then reordered back to fixture order so the overall
//! result list stays deterministic regardless of which network call
//! happened to finish first.

use canary_core::{CompatibilityResult, ExecutionContext, ProtocolVersion, Status, Surface};
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
    let client = HttpRpcClient::new(rpc_endpoint.to_string());

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
            let cache_key = cache_key(context, &fixture.metadata.id);
            if let Some(cached) = context.cache.get(&cache_key) {
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

            let _ = context.cache.put(&cache_key, &result);
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
                let cache_key = cache_key(context, &fixture.metadata.id);
                if let Some(cached) = context.cache.get(&cache_key) {
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
                let _ = context.cache.put(&cache_key, &result);
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
                let cache_key = cache_key(context, &fixture.metadata.id);
                if let Some(cached) = context.cache.get(&cache_key) {
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
                let _ = context.cache.put(&cache_key, &result);
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
    }
}

fn cache_key(context: &ExecutionContext, fixture_id: &str) -> canary_core::CacheKey {
    let project_fingerprint = match (&context.git.commit, context.git.is_dirty) {
        (Some(commit), Some(true)) => format!("{}-dirty", commit),
        (Some(commit), _) => commit.clone(),
        (None, _) => context.project.name.clone(),
    };
    canary_core::CacheKey {
        fixture_id: fixture_id.to_string(),
        protocol: context.protocol,
        project_fingerprint,
        rpc_endpoint: context.network.rpc_url.clone(),
        observed_protocol: context.network.observed_protocol,
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
                    "id = \"r\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"c\"\ndescription = \"d\"\nmethod = \"get-network\"\n",
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
                    "id = \"r\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"c\"\ndescription = \"d\"\nmethod = \"get-network\"\n",
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

        let results = execute(&plan, &context(), &server.uri()).await;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].test_id, "s1");
        assert_eq!(results[1].test_id, "s2");
        assert_eq!(results[0].status, Status::Pass);
        assert_eq!(results[1].status, Status::Pass);
    }
}
