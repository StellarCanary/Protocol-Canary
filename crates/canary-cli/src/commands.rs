//! Command implementations.

use canary_core::{
    CacheStore, CanaryError, DefaultPolicyEvaluator, ExecutionContext, ExitCode, NetworkContext,
    Policy, PolicyEvaluator, ProtocolVersion, RunOptions,
};
use canary_git::{collect_git_context, CliGitRepository};
use canary_report::{
    JsonReporter, MarkdownReporter, NetworkSummary, ProjectSummary, ReportInput, TerminalReporter,
};
use canary_rpc::{validate_network_info, HttpRpcClient, RpcClient, RpcError};
use canary_runner::EnabledSurfaces;

use crate::cli::{CheckArgs, FixturesArgs, InspectArgs, OutputFormat, ReportArgs};
use crate::network::{default_passphrase, default_rpc_url, parse_network_name};

const CACHE_DIR_NAME: &str = ".stellar-canary-cache";

/// Rejects `--protocol 0` the same way the config loader rejects
/// `protocol = 0`, so the flag can't bypass that validation.
fn validated_protocol_flag(protocol: Option<u32>) -> Result<Option<u32>, CanaryError> {
    match protocol {
        Some(0) => Err(CanaryError::Configuration(
            "--protocol must be a positive protocol version number".to_string(),
        )),
        other => Ok(other),
    }
}

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

    let target_protocol =
        ProtocolVersion(validated_protocol_flag(args.protocol)?.unwrap_or(config.protocol));

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

        // Fetch the endpoint's network identity and compare it against
        // what this run assumed (see `canary_rpc::validate_network_info`,
        // the constructor of RpcError::NetworkMismatch/ProtocolMismatch):
        //
        // - A transport-level failure leaves `observed_protocol` as `None`
        //   (rendered as "protocol not observed"); it does not block the
        //   run — individual RPC/Soroban fixtures still report the outage
        //   as execution errors.
        // - A passphrase mismatch means `--network` and `--rpc-url` refer
        //   to different networks: abort as a configuration error rather
        //   than attribute results to the wrong network.
        // - A protocol mismatch is a warning, not a failure: running a
        //   target-protocol run against a not-yet-upgraded network is a
        //   core upgrade-rehearsal use case, but it must be visible
        //   rather than only an annotation in the report.
        let client = HttpRpcClient::new(rpc_url.clone());
        let observed_protocol = match client.get_network().await {
            Ok(info) => {
                if let Err(err) = validate_network_info(&info, &passphrase, target_protocol.0) {
                    match err {
                        RpcError::NetworkMismatch { .. } => {
                            return Err(CanaryError::Configuration(err.to_string()));
                        }
                        RpcError::ProtocolMismatch { .. } => {
                            eprintln!("warning: {err}");
                        }
                        other => return Err(other.into()),
                    }
                }
                Some(ProtocolVersion(info.protocol_version))
            }
            Err(_) => None,
        };

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
            max_concurrency: args.max_concurrency,
            rpc_timeout: args.rpc_timeout,
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
    let target_protocol =
        ProtocolVersion(validated_protocol_flag(args.protocol)?.unwrap_or(config.protocol));
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

    let protocol = match validated_protocol_flag(args.protocol)? {
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

#[cfg(test)]
mod tests {
    use super::*;
    use canary_core::{
        CompatibilityResult, GitContext, PolicyDecision, ProjectType, Status, Surface,
    };
    use std::path::PathBuf;

    /// Minimal temp-file helper: the repo deliberately hand-rolls these
    /// rather than adding a `tempfile` dev-dependency for a few tests.
    struct TempReport {
        dir: PathBuf,
    }

    impl TempReport {
        fn with_contents(contents: &str) -> Self {
            TempReport {
                dir: create_unique_temp_dir(),
            }
            .write(contents)
        }

        fn write(self, contents: &str) -> Self {
            std::fs::write(self.path(), contents).unwrap();
            self
        }

        fn path(&self) -> PathBuf {
            self.dir.join("result.json")
        }
    }

    /// A clock sample alone does not separate two threads that happen to
    /// read the same nanosecond, and several of these tests run at once, so
    /// the attempt counter is retried until `create_dir` — which fails on an
    /// existing path — actually claims a directory.
    fn create_unique_temp_dir() -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let dir =
                std::env::temp_dir().join(format!("canary-cli-report-{pid}-{nanos}-{attempt}"));
            match std::fs::create_dir(&dir) {
                Ok(()) => return dir,
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!("could not create a temporary directory: {err}"),
            }
        }
    }

    impl Drop for TempReport {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn stored_result(status: Status) -> CompatibilityResult {
        CompatibilityResult {
            test_id: "xdr/stored".into(),
            protocol: ProtocolVersion(28),
            surface: Surface::Xdr,
            status,
            summary: "stored result".into(),
            details: None,
            duration_ms: 1,
            fixture_id: None,
        }
    }

    /// Rendered through the real JSON reporter so this test feeds
    /// `run_report_inner` the same shape `check --json` writes out.
    fn stored_report(results: Vec<CompatibilityResult>, decision: PolicyDecision) -> String {
        JsonReporter::render(&ReportInput {
            tool_version: "0.0.0-test".into(),
            target_protocol: ProtocolVersion(28),
            project: ProjectSummary {
                name: "stored-project".into(),
                project_type: ProjectType::Soroban,
            },
            network: None,
            results,
            skipped: Vec::new(),
            decision,
            git: GitContext::default(),
            verbose: false,
        })
    }

    fn report_args(path: PathBuf, format: OutputFormat) -> ReportArgs {
        ReportArgs { path, format }
    }

    #[test]
    fn run_report_inner_returns_pass_for_a_stored_passing_run() {
        let file = TempReport::with_contents(&stored_report(
            vec![stored_result(Status::Pass)],
            PolicyDecision::Pass,
        ));
        let exit_code = run_report_inner(report_args(file.path(), OutputFormat::Markdown))
            .expect("a valid stored report must be readable");
        assert_eq!(exit_code, ExitCode::Pass);
    }

    #[test]
    fn run_report_inner_returns_a_compatibility_failure_for_a_stored_failed_run() {
        let file = TempReport::with_contents(&stored_report(
            vec![stored_result(Status::Fail)],
            PolicyDecision::Fail,
        ));
        let exit_code = run_report_inner(report_args(file.path(), OutputFormat::Markdown))
            .expect("a failed run is a result, not an error of this command");
        assert_eq!(exit_code, ExitCode::CompatibilityFailure);
    }

    #[test]
    fn run_report_inner_lets_a_stored_execution_error_override_the_decision() {
        // The stored decision is `Pass` (an execution error is excluded from
        // policy evaluation), so only the override inside `exit_code_for_run`
        // can turn this into `ExecutionError`.
        let file = TempReport::with_contents(&stored_report(
            vec![stored_result(Status::Error)],
            PolicyDecision::Pass,
        ));
        let exit_code = run_report_inner(report_args(file.path(), OutputFormat::Markdown))
            .expect("an execution error in the report is still a readable report");
        assert_eq!(exit_code, ExitCode::ExecutionError);
    }

    #[test]
    fn run_report_inner_renders_every_supported_format() {
        let contents = stored_report(vec![stored_result(Status::Pass)], PolicyDecision::Pass);
        for format in [
            OutputFormat::Terminal,
            OutputFormat::Json,
            OutputFormat::Markdown,
        ] {
            let file = TempReport::with_contents(&contents);
            let exit_code = run_report_inner(report_args(file.path(), format))
                .unwrap_or_else(|err| panic!("{format:?} output must render: {err}"));
            assert_eq!(exit_code, ExitCode::Pass);
        }
    }

    #[test]
    fn run_report_inner_reports_a_missing_file_as_a_configuration_error() {
        let missing = std::env::temp_dir().join(format!(
            "canary-cli-absent-report-{}.json",
            std::process::id()
        ));
        let err = run_report_inner(report_args(missing.clone(), OutputFormat::Markdown))
            .expect_err("an absent report file must not be reported as success");
        assert!(matches!(err, CanaryError::Configuration(_)));
        let message = err.to_string();
        assert!(
            message.contains("failed to read report file")
                && message.contains(&missing.display().to_string()),
            "error must name the unreadable file, got: {message}"
        );
    }

    #[test]
    fn run_report_inner_reports_an_unparseable_file_as_a_configuration_error() {
        let file = TempReport::with_contents("not json at all");
        let err = run_report_inner(report_args(file.path(), OutputFormat::Markdown))
            .expect_err("a file that is not a report must not be reported as success");
        assert!(matches!(err, CanaryError::Configuration(_)));
        assert!(
            err.to_string().contains("invalid report file"),
            "error must say the report is invalid, got: {err}"
        );
    }
}
