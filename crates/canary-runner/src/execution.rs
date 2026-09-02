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
            runner.run(fixture, context).unwrap_or_else(|e| {
                error_result(
                    &fixture.metadata.id,
                    fixture.metadata.protocol,
                    Surface::Xdr,
                    &e.to_string(),
                )
            })
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
                let result = runner.run(fixture, context).await.unwrap_or_else(|e| {
                    error_result(
                        &fixture.metadata.id,
                        fixture.metadata.protocol,
                        Surface::Rpc,
                        &e.to_string(),
                    )
                });
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
                let result = runner.run(fixture, context).await.unwrap_or_else(|e| {
                    error_result(
                        &fixture.metadata.id,
                        fixture.metadata.protocol,
                        Surface::Soroban,
                        &e.to_string(),
                    )
                });
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
}
