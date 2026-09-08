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

use crate::cli::{CheckArgs, FixturesArgs, InspectArgs, OutputFormat, ReportArgs};
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

pub fn run_inspect(args: InspectArgs) -> ExitCode {
    match run_inspect_inner(args) {
        Ok(exit_code) => exit_code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(&err)
        }
    }
}

fn run_inspect_inner(args: InspectArgs) -> Result<ExitCode, CanaryError> {
    let root = std::env::current_dir()
        .map_err(|e| CanaryError::Internal(format!("failed to read current directory: {e}")))?;

    let config = match &args.config {
        Some(path) => canary_config::load(path)?,
        None => canary_config::load_from_root(&root)?.unwrap_or_default(),
    };

    let detected = canary_project::detect(&root);
    let explicit_type = match config.project.project_type {
        canary_config::ProjectTypeSetting::Auto => None,
        canary_config::ProjectTypeSetting::Explicit(t) => Some(t),
    };
    let resolved_type = canary_project::resolve_project_type(detected.project_type, explicit_type);
    let mut project = detected.clone();
    project.project_type = resolved_type;

    println!("Project root: {}", root.display());
    println!("Project type: {resolved_type}");
    if explicit_type.is_some() && resolved_type != detected.project_type {
        println!(
            "  (detected as {} by auto-detection; overridden by configuration)",
            detected.project_type
        );
    }
    println!();

    println!(
        "Detected Stellar SDK/XDR dependency: {}",
        detected.has_capability(&canary_core::Capability::StellarSdkDependency)
    );
    println!(
        "Detected Soroban contract usage: {}",
        detected.has_capability(&canary_core::Capability::SorobanContract)
    );
    println!(
        "Detected RPC client dependency: {}",
        detected.has_capability(&canary_core::Capability::RpcClient)
    );
    println!(
        "Detected WASM artifact: {}",
        detected.has_capability(&canary_core::Capability::WasmArtifact)
    );
    println!();

    println!("Configured protocol: {}", config.protocol);
    let target_protocol = ProtocolVersion(args.protocol.unwrap_or(config.protocol));
    println!("Target protocol for fixture plan: {target_protocol}");
    println!("Available compatibility surfaces:");
    println!(
        "  xdr:     {}",
        if config.tests.xdr {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  rpc:     {}",
        if config.tests.rpc {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  soroban: {}",
        if config.tests.soroban {
            "enabled"
        } else {
            "disabled"
        }
    );

    let loaded_fixtures = if args.fixtures_dir.is_dir() {
        canary_fixtures::load_directory(&args.fixtures_dir)?
    } else {
        Vec::new()
    };
    let _fixture_store = canary_fixtures::validate(&loaded_fixtures)?;
    let enabled = EnabledSurfaces {
        xdr: config.tests.xdr,
        rpc: config.tests.rpc,
        soroban: config.tests.soroban,
    };
    let plan = canary_runner::build_plan(&loaded_fixtures, target_protocol, enabled, &project)?;

    println!();
    println!("Fixture compatibility plan (offline):");
    println!("  Directory: {}", args.fixtures_dir.display());
    println!("  Loaded fixtures that would run:");
    if plan.applicable_count() == 0 {
        println!("    (none)");
    } else {
        for fixture in &plan.xdr {
            println!("    xdr: {}", fixture.metadata.id);
        }
        for fixture in &plan.rpc {
            println!("    rpc: {}", fixture.metadata.id);
        }
        for fixture in &plan.soroban {
            println!("    soroban: {}", fixture.metadata.id);
        }
    }
    println!("  Skipped fixtures:");
    if plan.skipped.is_empty() {
        println!("    (none)");
    } else {
        for skipped in &plan.skipped {
            println!(
                "    {} [{}]: {}",
                skipped.fixture_id, skipped.surface, skipped.reason
            );
        }
    }

    Ok(ExitCode::Pass)
}

pub fn run_fixtures(args: FixturesArgs) -> ExitCode {
    match run_fixtures_inner(args) {
        Ok(exit_code) => exit_code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(&err)
        }
    }
}

fn run_fixtures_inner(args: FixturesArgs) -> Result<ExitCode, CanaryError> {
    let root = std::env::current_dir()
        .map_err(|e| CanaryError::Internal(format!("failed to read current directory: {e}")))?;

    let protocol = match args.protocol {
        Some(p) => p,
        None => {
            let config = match &args.config {
                Some(path) => canary_config::load(path)?,
                None => canary_config::load_from_root(&root)?.unwrap_or_default(),
            };
            config.protocol
        }
    };

    let loaded_fixtures = if args.fixtures_dir.is_dir() {
        canary_fixtures::load_directory(&args.fixtures_dir)?
    } else {
        Vec::new()
    };
    let store = canary_fixtures::validate(&loaded_fixtures)?;

    println!("Protocol {protocol} fixtures");
    println!();

    let target = canary_core::ProtocolVersion(protocol);
    let mut any = false;
    for surface in canary_report::SURFACE_ORDER {
        let ids: Vec<&str> = store
            .for_protocol(target)
            .filter(|f| f.surface == surface)
            .map(|f| f.id.as_str())
            .collect();
        if ids.is_empty() {
            continue;
        }
        any = true;
        println!("{}", canary_report::surface_heading(surface));
        for id in ids {
            println!("  {id}");
        }
        println!();
    }

    if !any {
        println!("(no fixtures found in {})", args.fixtures_dir.display());
    }

    Ok(ExitCode::Pass)
}

pub fn run_report(args: ReportArgs) -> ExitCode {
    match run_report_inner(args) {
        Ok(exit_code) => exit_code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(&err)
        }
    }
}

fn run_report_inner(args: ReportArgs) -> Result<ExitCode, CanaryError> {
    let json_text = std::fs::read_to_string(&args.path).map_err(|e| {
        CanaryError::Configuration(format!(
            "failed to read report file {}: {e}",
            args.path.display()
        ))
    })?;
    let report_input = canary_report::JsonReporter::parse(&json_text)
        .map_err(|e| CanaryError::Configuration(format!("invalid report file: {e}")))?;

    let rendered = match args.format {
        OutputFormat::Terminal => TerminalReporter::render(&report_input),
        OutputFormat::Json => JsonReporter::render(&report_input),
        OutputFormat::Markdown => MarkdownReporter::render(&report_input),
    };
    println!("{rendered}");

    let exit_code = canary_core::exit_code_for_run(&report_input.results, report_input.decision);
    Ok(exit_code)
}

pub fn run_version() -> ExitCode {
    println!("stellar-canary {}", env!("CARGO_PKG_VERSION"));
    ExitCode::Pass
}
