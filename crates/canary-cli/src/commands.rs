//! Command implementations.

use canary_core::{
    CacheStore, CanaryError, DefaultPolicyEvaluator, ExecutionContext, ExitCode, NetworkContext,
    Policy, PolicyEvaluator, ProtocolVersion, RunOptions,
};
use canary_git::{collect_git_context, CliGitRepository};
use canary_report::{
    JsonReporter, MarkdownReporter, NetworkSummary, ProjectSummary, ReportInput, TerminalReporter,
};
use canary_rpc::{HttpRpcClient, RpcClient};
use canary_runner::EnabledSurfaces;

use crate::cli::{CheckArgs, OutputFormat};
use crate::network::{default_passphrase, default_rpc_url, parse_network_name};

const CACHE_DIR_NAME: &str = ".stellar-canary-cache";

pub async fn run_check(args: CheckArgs) -> ExitCode {
    match run_check_inner(args).await {
        Ok(exit_code) => exit_code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(&err)
        }
    }
}

async fn run_check_inner(args: CheckArgs) -> Result<ExitCode, CanaryError> {
    let root = std::env::current_dir()
        .map_err(|e| CanaryError::Internal(format!("failed to read current directory: {e}")))?;

    let config = match &args.config {
        Some(path) => canary_config::load(path)?,
        None => canary_config::load_from_root(&root)?.unwrap_or_default(),
    };

    let target_protocol = ProtocolVersion(args.protocol.unwrap_or(config.protocol));

    let project = canary_project::detect(&root);
    let explicit_type = match config.project.project_type {
        canary_config::ProjectTypeSetting::Auto => None,
        canary_config::ProjectTypeSetting::Explicit(t) => Some(t),
    };
    let mut project = project;
    project.project_type =
        canary_project::resolve_project_type(project.project_type, explicit_type);

    let live_checks_needed = config.tests.rpc || config.tests.soroban;
    let network_name = parse_network_name(&args.network);
    let (network_context, network_summary) = if live_checks_needed {
        let rpc_url = args
            .rpc_url
            .clone()
            .or_else(|| default_rpc_url(&network_name).map(str::to_string))
            .ok_or_else(|| {
                CanaryError::Configuration(format!(
                    "--rpc-url is required for network {network_name} (no built-in default)"
                ))
            })?;
        let passphrase = default_passphrase(&network_name).unwrap_or("").to_string();

        let client = HttpRpcClient::new(rpc_url.clone());
        let observed_protocol = client
            .get_network()
            .await
            .ok()
            .map(|info| ProtocolVersion(info.protocol_version));

        let context = NetworkContext {
            name: network_name.clone(),
            rpc_url,
            passphrase,
            observed_protocol,
        };
        let summary = NetworkSummary {
            name: network_name,
            observed_protocol,
        };
        (context, Some(summary))
    } else {
        (
            NetworkContext {
                name: network_name,
                rpc_url: String::new(),
                passphrase: String::new(),
                observed_protocol: None,
            },
            None,
        )
    };

    let loaded_fixtures = if args.fixtures_dir.is_dir() {
        canary_fixtures::load_directory(&args.fixtures_dir)?
    } else {
        Vec::new()
    };
    let fixture_store = canary_fixtures::validate(&loaded_fixtures)?;

    let enabled = EnabledSurfaces {
        xdr: config.tests.xdr,
        rpc: config.tests.rpc,
        soroban: config.tests.soroban,
    };
    let plan = canary_runner::build_plan(&loaded_fixtures, target_protocol, enabled, &project)?;

    let git = collect_git_context(&CliGitRepository::new(&root));

    let context = ExecutionContext {
        protocol: target_protocol,
        project: project.clone(),
        network: network_context,
        fixtures: fixture_store,
        git: git.clone(),
        cache: CacheStore::new(root.join(CACHE_DIR_NAME)),
        options: RunOptions {
            verbose: args.verbose,
            quiet: args.quiet,
            max_concurrency: 4,
        },
    };

    let results = canary_runner::execute(&plan, &context, &context.network.rpc_url).await;

    let policy = Policy {
        warnings_are_failures: config.policy.warnings_are_failures,
    };
    let decision = DefaultPolicyEvaluator.evaluate(&results, &policy);
    let exit_code = canary_core::exit_code_for_run(&results, decision);

    let report_input = ReportInput {
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        target_protocol,
        project: ProjectSummary {
            name: project.name.clone(),
            project_type: project.project_type,
        },
        network: network_summary,
        results,
        skipped: plan
            .skipped
            .iter()
            .map(|s| canary_report::SkipSummary {
                fixture_id: s.fixture_id.clone(),
                surface: s.surface,
                reason: s.reason.clone(),
            })
            .collect(),
        decision,
        git,
        verbose: args.verbose,
    };

    let format = if args.json {
        OutputFormat::Json
    } else {
        args.format
    };
    if format == OutputFormat::Terminal && args.quiet {
        println!(
            "Status: {}",
            match report_input.overall_status() {
                canary_report::ReportStatus::Pass => "PASS",
                canary_report::ReportStatus::Warning => "WARNING",
                canary_report::ReportStatus::Fail => "NOT READY",
                canary_report::ReportStatus::Error => "ERROR",
            }
        );
    } else {
        let rendered = match format {
            OutputFormat::Terminal => TerminalReporter::render(&report_input),
            OutputFormat::Json => JsonReporter::render(&report_input),
            OutputFormat::Markdown => MarkdownReporter::render(&report_input),
        };
        println!("{rendered}");
    }

    Ok(exit_code)
}

pub fn run_version() -> ExitCode {
    println!("stellar-canary {}", env!("CARGO_PKG_VERSION"));
    ExitCode::Pass
}
