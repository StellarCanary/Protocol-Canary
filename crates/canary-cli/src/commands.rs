//! Command implementations.

use std::path::{Path, PathBuf};

use canary_core::{
    CachePolicy, CacheStore, CanaryError, DefaultPolicyEvaluator, ExecutionContext, ExitCode,
    NetworkContext, Policy, PolicyEvaluator, ProtocolVersion, RunOptions,
};
use canary_git::{collect_git_context, CliGitRepository};
use canary_report::{
    JUnitReporter, JsonReporter, MarkdownReporter, NetworkSummary, ProjectSummary, ReportInput,
    TerminalReporter,
};
use canary_rpc::{validate_network_info, HttpRpcClient, RpcClient, RpcError};
use canary_runner::EnabledSurfaces;

use crate::cli::{
    CheckArgs, FixturesArgs, FixturesFormat, InspectArgs, InspectFormat, OutputFormat, ReportArgs,
};
use crate::error::{hint_for, print_error};
use crate::fixtures_json;
use crate::inspect_json;
use crate::network::{default_rpc_url, parse_network_name, resolve_passphrase};

const CACHE_DIR_NAME: &str = ".stellar-canary-cache";

/// The Git commit this binary was built from, captured by `build.rs`, or
/// `None` when the build had no usable Git history.
///
/// The value is validated here as well as in `build.rs`: the compile-time
/// variable is the only input, and anything that is not a full lowercase hex
/// object name is treated as unknown rather than copied into a report.
fn build_commit() -> Option<String> {
    normalized_commit(option_env!("CANARY_BUILD_COMMIT").unwrap_or(""))
}

fn normalized_commit(raw: &str) -> Option<String> {
    let commit = raw.trim();
    let full_length = commit.len() == 40 || commit.len() == 64;
    let lower_hex = commit
        .bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    (full_length && lower_hex).then(|| commit.to_string())
}

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

/// The directory a command operates on (CF-07 section 2).
///
/// It is the current directory unless `--project-root` is given, and it is
/// never found by searching parent directories (D-08): a monorepo member is
/// selected by naming it, so the engine can never silently check a different
/// project than the one the caller pointed at.
struct ProjectRoot {
    path: PathBuf,
    /// `true` when the root came from `--project-root`. Relative input paths
    /// are then resolved against it instead of the caller's working
    /// directory; without the flag they are left exactly as given, which is
    /// the same file because the root is the working directory.
    explicit: bool,
}

impl ProjectRoot {
    fn resolve(flag: Option<&Path>) -> Result<ProjectRoot, CanaryError> {
        let Some(requested) = flag else {
            let path = std::env::current_dir().map_err(|e| {
                CanaryError::Internal(format!("failed to read current directory: {e}"))
            })?;
            return Ok(ProjectRoot {
                path,
                explicit: false,
            });
        };

        let metadata = std::fs::metadata(requested).map_err(|e| {
            let reason = if e.kind() == std::io::ErrorKind::NotFound {
                "does not exist".to_string()
            } else {
                format!("cannot be read: {e}")
            };
            CanaryError::Configuration(format!("--project-root {} {reason}", requested.display()))
        })?;
        if !metadata.is_dir() {
            return Err(CanaryError::Configuration(format!(
                "--project-root {} is not a directory",
                requested.display()
            )));
        }

        // Resolved once, here, so every later join and the reported project
        // name agree on one absolute location.
        let canonical = std::fs::canonicalize(requested).map_err(|e| {
            CanaryError::Configuration(format!(
                "--project-root {} could not be resolved: {e}",
                requested.display()
            ))
        })?;
        Ok(ProjectRoot {
            path: without_verbatim_prefix(canonical),
            explicit: true,
        })
    }

    /// Resolves a caller-supplied input path (`--config`, `--fixtures-dir`,
    /// `--output`) against the project root when one was given explicitly.
    fn resolve_input(&self, path: &Path) -> PathBuf {
        if self.explicit && path.is_relative() {
            self.path.join(path)
        } else {
            path.to_path_buf()
        }
    }
}

/// `std::fs::canonicalize` returns `\\?\C:\...` on Windows. That form is
/// valid but is shown to users in `inspect` and is not what they typed, so
/// plain drive paths are converted back.
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(rest) = path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
            if rest.as_bytes().get(1) == Some(&b':') {
                return PathBuf::from(rest);
            }
        }
    }
    path
}

pub async fn run_check(args: CheckArgs) -> ExitCode {
    match run_check_inner(args).await {
        Ok(exit_code) => exit_code,
        Err(err) => {
            print_error(&err);
            ExitCode::from(&err)
        }
    }
}

async fn run_check_inner(args: CheckArgs) -> Result<ExitCode, CanaryError> {
    let project_root = ProjectRoot::resolve(args.project_root.as_deref())?;
    let root = project_root.path.clone();

    let config = match &args.config {
        Some(path) => canary_config::load(&project_root.resolve_input(path))?,
        None => canary_config::load_from_root(&root)?.unwrap_or_default(),
    };

    // Cascade per key: an explicitly passed CLI flag beats the matching
    // `.stellar-canary.toml` setting, which beats the built-in default
    // (issue #474).
    let settings = canary_config::resolve_settings(
        &config,
        canary_config::CliOverrides {
            network: args.network.clone(),
            rpc_url: args.rpc_url.clone(),
            fixtures_dir: args.fixtures_dir.clone(),
        },
    );
    let fixtures_dir = project_root.resolve_input(&settings.fixtures_dir);

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
    let network_name = parse_network_name(&settings.network);
    let (network_context, network_summary) = if live_checks_needed {
        let rpc_url = settings
            .rpc_url
            .clone()
            .or_else(|| default_rpc_url(&network_name).map(str::to_string))
            .ok_or_else(|| {
                CanaryError::Configuration(format!(
                    "no RPC URL for network {network_name}: pass --rpc-url or set rpc_url in .stellar-canary.toml (no built-in default)"
                ))
            })?;
        let passphrase = resolve_passphrase(&network_name, args.network_passphrase.as_deref());

        // Fetch the endpoint's network identity and compare it against
        // what this run assumed (see `canary_rpc::validate_network_info`,
        // the constructor of RpcError::NetworkMismatch/ProtocolMismatch):
        //
        // - A transport-level failure leaves `observed_protocol` as `None`
        //   (rendered as "protocol not observed"); it does not block the
        //   run — individual RPC/Soroban fixtures still report the outage
        //   as execution errors.
        // - A passphrase mismatch means `--network` (or an explicit
        //   `--network-passphrase`) and `--rpc-url` refer to different
        //   networks: abort as a configuration error rather
        //   than attribute results to the wrong network.
        // - A protocol mismatch is a warning, not a failure: running a
        //   target-protocol run against a not-yet-upgraded network is a
        //   core upgrade-rehearsal use case, but it must be visible
        //   rather than only an annotation in the report.
        // In --verbose mode every attempt of this probe (and, through
        // `RunOptions`, of every fixture call) is logged to stderr with
        // its attempt number and duration; the endpoint URL is scrubbed
        // inside the client, so credentials never reach the log.
        let client = HttpRpcClient::new(rpc_url.clone()).with_verbose(args.verbose);
        let (observed_protocol, network_error) = match client.get_network().await {
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
                (Some(ProtocolVersion(info.protocol_version)), None)
            }
            // A transport failure does not abort the run — each fixture
            // still reports the outage as an execution error — and the
            // matching `Hint:` is printed once at the end of the run, when
            // (and only when) that outage is the run's outcome.
            Err(e) => (None, Some(e.to_string())),
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
            error: network_error,
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

    let loaded_fixtures = if fixtures_dir.is_dir() {
        canary_fixtures::load_directory(&fixtures_dir)?
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

    // A run that executes nothing proves nothing, so it must not look like a
    // pass. This is checked before any check runs and before a report exists,
    // which is why it is a configuration error (exit 2) rather than a result.
    if plan.applicable_count() == 0 {
        let explanation = canary_runner::explain_empty_plan(
            &plan,
            &loaded_fixtures,
            target_protocol,
            &fixtures_dir,
        );
        if !args.allow_empty {
            return Err(CanaryError::Configuration(format!(
                "{explanation} Executing nothing is not evidence of compatibility; pass --allow-empty if an empty run is intended."
            )));
        }
        eprintln!("warning: {explanation} Continuing because --allow-empty was given; this result is not evidence of compatibility.");
    }

    let git = collect_git_context(&CliGitRepository::new(&root));

    let context = ExecutionContext {
        protocol: target_protocol,
        project: project.clone(),
        network: network_context,
        fixtures: fixture_store,
        git: git.clone(),
        cache: CacheStore::new(root.join(CACHE_DIR_NAME)).with_policy(CachePolicy {
            enabled: !args.no_cache,
            live_max_age: args.live_cache_ttl.map(std::time::Duration::from_secs),
        }),
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

    // The fixtures' own CF-04 evidence, so the terminal report can show
    // which results are backed by a verification record. A fixture with
    // no record is simply absent here, which reporters render as
    // `[unverified]`.
    let verification: Vec<canary_report::VerificationSummary> = loaded_fixtures
        .iter()
        .filter_map(|fixture| {
            fixture
                .verification
                .as_ref()
                .map(|record| canary_report::VerificationSummary {
                    fixture_id: fixture.metadata.id.clone(),
                    method: record.method.as_str().to_string(),
                    outcome: record.outcome.as_str().to_string(),
                    checked_at: record.checked_at.clone(),
                })
        })
        .collect();

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
        verification,
        decision,
        git,
        verbose: args.verbose,
        run: Some(canary_report::RunSummary {
            tool_build_commit: build_commit(),
        }),
    };

    let format = if args.json {
        OutputFormat::Json
    } else {
        args.format
    };
    let rendered = if format == OutputFormat::Terminal && args.quiet {
        format!(
            "Status: {}",
            match report_input.overall_status() {
                canary_report::ReportStatus::Pass => "PASS",
                canary_report::ReportStatus::Warning => "WARNING",
                canary_report::ReportStatus::Fail => "NOT READY",
                canary_report::ReportStatus::Error => "ERROR",
            }
        )
    } else {
        match format {
            OutputFormat::Terminal => TerminalReporter::render(&report_input),
            OutputFormat::Json => JsonReporter::render(&report_input),
            OutputFormat::Markdown => MarkdownReporter::render(&report_input),
            OutputFormat::JUnit => JUnitReporter::render(&report_input),
        }
    };

    // `--output` writes the report *as well as* printing it, so shell
    // redirection keeps working for existing callers. The file is written
    // before anything reaches stdout: a path that cannot be written fails
    // loudly rather than after a report has already been printed.
    if let Some(path) = &args.output {
        write_report_file(&project_root.resolve_input(path), &rendered)?;
    }
    println!("{rendered}");

    // A transport failure surfaces as execution errors in the report
    // (exit 3), not as an `error:` line, so this is where its hint
    // belongs: after the report, and only when the unreachable endpoint
    // is the run's outcome. A different headline — an empty plan, a
    // configuration error — already says enough about itself.
    if exit_code == ExitCode::ExecutionError {
        if let Some(hint) = report_input
            .network
            .as_ref()
            .and_then(|summary| summary.error.as_deref())
            .and_then(hint_for)
        {
            eprintln!("Hint: {hint}");
        }
    }

    Ok(exit_code)
}

/// Writes the rendered report to `path` with exactly the bytes
/// `println!` puts on stdout — trailing newline included — so `--output`
/// and shell redirection produce identical files.
///
/// The write is atomic ([`canary_report::atomic_write_report`]): the bytes
/// go to a `.tmp` sibling that is renamed over `path`, so a crash or a
/// full disk can never leave a truncated report behind, and an existing
/// file's permissions are preserved.
///
/// An unwritable path is a configuration error (the same exit code as any
/// other unusable caller-supplied path, e.g. a missing `--config` file),
/// not a compatibility result. The underlying error already names the
/// path, so it is passed through unchanged.
fn write_report_file(path: &Path, rendered: &str) -> Result<(), CanaryError> {
    canary_report::atomic_write_report(path, &format!("{rendered}\n"))
        .map_err(|err| CanaryError::Configuration(err.to_string()))
}

/// Prints offline project diagnostics and the planned fixture run.
///
/// Inspects the project in the current working directory, printing detected
/// project capabilities, active configuration settings, and an offline
/// compatibility plan for the target protocol to stdout:
/// - Project root and resolved project type (highlighting configuration overrides).
/// - Detected capabilities (Stellar SDK/XDR dependencies, Soroban contracts,
///   RPC client usage, and WASM artifacts).
/// - Configured and target protocol versions.
/// - Status of compatibility surfaces (XDR, RPC, Soroban).
/// - Planned fixtures loaded from [`InspectArgs::fixtures_dir`], showing which
///   fixtures would run on each surface and which would be skipped (with reasons).
///
/// Like [`run_report`], this command operates entirely offline: it performs no
/// network requests and does not execute any test fixtures. An absent fixture
/// directory is tolerated and simply yields an empty plan rather than failing.
///
/// Returns [`ExitCode::Pass`] when inspection completes successfully.
///
/// # Errors
///
/// This function returns an [`ExitCode`] rather than a `Result`. Any error
/// encountered during inspection is printed to stderr with an `error:` prefix
/// and mapped to an appropriate exit code:
/// - [`ExitCode::ConfigurationError`] if `--protocol 0` is supplied, an
///   explicit configuration file cannot be read, or configuration is malformed.
/// - [`ExitCode::InvalidFixture`] if a fixture file is unreadable, contains
///   malformed TOML, has duplicate IDs, or references missing companion files.
/// - [`ExitCode::InternalError`] if the current working directory cannot be
///   read.
///
/// # Examples
///
/// ```text
/// # Inspect the current project using default configuration.
/// stellar-canary inspect
///
/// # Inspect against an explicit target protocol version.
/// stellar-canary inspect --protocol 28
///
/// # Inspect with an explicit configuration file and custom fixture directory.
/// stellar-canary inspect --config .custom-canary.toml --fixtures-dir ./fixtures
/// ```
pub fn run_inspect(args: InspectArgs) -> ExitCode {
    match run_inspect_inner(args) {
        Ok(exit_code) => exit_code,
        Err(err) => {
            print_error(&err);
            ExitCode::from(&err)
        }
    }
}

/// Everything `inspect` resolves before choosing a renderer, so the
/// terminal and JSON paths agree on one set of facts.
struct InspectSetup {
    root: std::path::PathBuf,
    config: canary_config::ConfigFile,
    /// The configuration file that was read, if any. Decides
    /// `config.found`, `config.path` and `target.source` in the JSON
    /// document, so "no file found" stays `None` rather than being
    /// collapsed into a default config.
    config_file: Option<std::path::PathBuf>,
    /// The fixture directory after the `--fixtures-dir` / configuration
    /// cascade (issue #474), resolved against `--project-root` when one
    /// was given (CF-07).
    fixtures_dir: std::path::PathBuf,
    detected: canary_core::ProjectContext,
    /// [`InspectSetup::detected`] with the resolved (effective) type.
    project: canary_core::ProjectContext,
    explicit_type: Option<canary_core::ProjectType>,
    target_protocol: ProtocolVersion,
    /// `flag`, `config` or `default` (CA-03).
    target_source: &'static str,
}

fn inspect_setup(args: &InspectArgs) -> Result<InspectSetup, CanaryError> {
    let project_root = ProjectRoot::resolve(args.project_root.as_deref())?;
    let root = project_root.path.clone();

    let (config, config_file) = match &args.config {
        Some(path) => {
            let resolved = project_root.resolve_input(path);
            (canary_config::load(&resolved)?, Some(resolved))
        }
        None => match canary_config::load_from_root(&root)? {
            Some(config) => (config, Some(root.join(canary_config::CONFIG_FILE_NAME))),
            None => (canary_config::ConfigFile::default(), None),
        },
    };

    // `--fixtures-dir` overrides the configured `fixtures_dir`, which
    // overrides the built-in default (issue #474).
    let settings = canary_config::resolve_settings(
        &config,
        canary_config::CliOverrides {
            fixtures_dir: args.fixtures_dir.clone(),
            ..canary_config::CliOverrides::default()
        },
    );
    let fixtures_dir = project_root.resolve_input(&settings.fixtures_dir);

    let detected = canary_project::detect(&root);
    let explicit_type = match config.project.project_type {
        canary_config::ProjectTypeSetting::Auto => None,
        canary_config::ProjectTypeSetting::Explicit(t) => Some(t),
    };
    let resolved_type = canary_project::resolve_project_type(detected.project_type, explicit_type);
    let mut project = detected.clone();
    project.project_type = resolved_type;

    let target_source = if args.protocol.is_some() {
        "flag"
    } else if config_file.is_some() {
        "config"
    } else {
        "default"
    };
    let target_protocol =
        ProtocolVersion(validated_protocol_flag(args.protocol)?.unwrap_or(config.protocol));

    Ok(InspectSetup {
        root,
        config,
        config_file,
        fixtures_dir,
        detected,
        project,
        explicit_type,
        target_protocol,
        target_source,
    })
}

fn run_inspect_inner(args: InspectArgs) -> Result<ExitCode, CanaryError> {
    let setup = inspect_setup(&args)?;

    let format = if args.json {
        InspectFormat::Json
    } else {
        args.format
    };
    match format {
        InspectFormat::Terminal => run_inspect_terminal(setup)?,
        InspectFormat::Json => run_inspect_json(setup)?,
    }

    Ok(ExitCode::Pass)
}

/// Loads the fixture directory and builds the plan, the one step both
/// renderers share.
fn inspect_plan(
    setup: &InspectSetup,
) -> Result<
    (
        Vec<canary_fixtures::LoadedFixture>,
        canary_runner::CompatibilityPlan,
    ),
    CanaryError,
> {
    let loaded_fixtures = if setup.fixtures_dir.is_dir() {
        canary_fixtures::load_directory(&setup.fixtures_dir)?
    } else {
        Vec::new()
    };
    let _fixture_store = canary_fixtures::validate(&loaded_fixtures)?;
    let enabled = EnabledSurfaces {
        xdr: setup.config.tests.xdr,
        rpc: setup.config.tests.rpc,
        soroban: setup.config.tests.soroban,
    };
    let plan = canary_runner::build_plan(
        &loaded_fixtures,
        setup.target_protocol,
        enabled,
        &setup.project,
    )?;
    Ok((loaded_fixtures, plan))
}

/// The original human-readable output, unchanged byte for byte
/// (CA-03 section 2 makes `terminal` the documented default).
fn run_inspect_terminal(setup: InspectSetup) -> Result<(), CanaryError> {
    let InspectSetup {
        root,
        config,
        detected,
        explicit_type,
        target_protocol,
        ..
    } = &setup;
    let resolved_type = setup.project.project_type;

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

    let (_loaded_fixtures, plan) = inspect_plan(&setup)?;

    println!();
    println!("Fixture compatibility plan (offline):");
    println!("  Directory: {}", setup.fixtures_dir.display());
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

    Ok(())
}

/// Prints the CA-03 document to stdout and nothing else.
fn run_inspect_json(setup: InspectSetup) -> Result<(), CanaryError> {
    let (loaded_fixtures, plan) = inspect_plan(&setup)?;

    let check_would_refuse = plan.applicable_count() == 0;
    let refusal = check_would_refuse.then(|| {
        canary_runner::explain_empty_plan(
            &plan,
            &loaded_fixtures,
            setup.target_protocol,
            &setup.fixtures_dir,
        )
    });

    // Execution order is XDR, then RPC, then Soroban, each in planned
    // order (see `canary_runner::execute`); `wouldRun` mirrors it so a
    // consumer can compare it against a report's `results`.
    let would_run = plan
        .xdr
        .iter()
        .map(|fixture| inspect_json::PlannedFixture {
            id: fixture.metadata.id.clone(),
            surface: canary_core::Surface::Xdr.to_string(),
        })
        .chain(plan.rpc.iter().map(|fixture| inspect_json::PlannedFixture {
            id: fixture.metadata.id.clone(),
            surface: canary_core::Surface::Rpc.to_string(),
        }))
        .chain(
            plan.soroban
                .iter()
                .map(|fixture| inspect_json::PlannedFixture {
                    id: fixture.metadata.id.clone(),
                    surface: canary_core::Surface::Soroban.to_string(),
                }),
        )
        .collect();

    let skipped = plan
        .skipped
        .iter()
        .map(|skipped| inspect_json::PlannedSkip {
            id: skipped.fixture_id.clone(),
            surface: skipped.surface.to_string(),
            code: skipped.cause.code().to_string(),
            reason: skipped.reason.clone(),
        })
        .collect();

    let mut capabilities: Vec<String> = setup
        .detected
        .capabilities
        .iter()
        .map(|capability| inspect_json::capability_name(capability).to_string())
        .collect();
    capabilities.sort();

    let document = inspect_json::InspectDocument {
        inspect_version: inspect_json::INSPECT_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        offline: true,
        project: inspect_json::InspectProject {
            // The command inspects the directory it was started in, so
            // the root is `.` relative to where a consumer stands.
            root: ".".to_string(),
            root_selected_by: "current-directory".to_string(),
            project_type: setup.project.project_type.to_string(),
            type_source: if setup.explicit_type.is_some() {
                "config"
            } else {
                "detected"
            }
            .to_string(),
            detected_type: setup.detected.project_type.to_string(),
            capabilities,
        },
        config: inspect_json::InspectConfig {
            found: setup.config_file.is_some(),
            path: setup
                .config_file
                .as_deref()
                .and_then(|path| relative_to_root(&setup.root, path)),
            version: setup.config.version,
            protocol: setup.config.protocol,
            project_type: match setup.config.project.project_type {
                canary_config::ProjectTypeSetting::Auto => "auto".to_string(),
                canary_config::ProjectTypeSetting::Explicit(t) => t.to_string(),
            },
            tests: inspect_json::InspectTests {
                xdr: setup.config.tests.xdr,
                rpc: setup.config.tests.rpc,
                soroban: setup.config.tests.soroban,
            },
            policy: inspect_json::InspectPolicy {
                warnings_are_failures: setup.config.policy.warnings_are_failures,
            },
        },
        target: inspect_json::InspectTarget {
            protocol: setup.target_protocol.0,
            source: setup.target_source.to_string(),
        },
        fixtures: inspect_json::InspectFixtures {
            directory_found: setup.fixtures_dir.is_dir(),
            loaded: loaded_fixtures.len(),
        },
        plan: inspect_json::InspectPlan {
            would_run,
            skipped,
            check_would_refuse,
            refusal,
        },
    };

    println!("{}", inspect_json::render(&document));
    Ok(())
}

/// Renders `path` relative to `root` with `/` separators, or `None`
/// when it lies outside the root (the location is not disclosed, CA-03
/// section 3).
fn relative_to_root(root: &Path, path: &Path) -> Option<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let relative = absolute.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_str()?.to_string()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// Lists available compatibility fixtures for a protocol version.
///
/// Discovers and validates fixtures in [`FixturesArgs::fixtures_dir`], filters
/// them by the target protocol version, and prints matching fixture IDs grouped
/// by surface (XDR assertions, RPC responses, and Soroban invocations) to
/// stdout.
///
/// The target protocol is determined from [`FixturesArgs::protocol`], falling
/// back to the configured protocol in [`FixturesArgs::config`] (or
/// `.stellar-canary.toml`), and defaulting to version 28 if no configuration
/// exists. If the fixture directory does not exist or contains no fixtures for
/// the requested protocol, the command reports `(no fixtures found in <path>)`
/// and returns [`ExitCode::Pass`] without failing the run.
///
/// Unlike [`run_check`], this command is entirely offline: it does not contact
/// any RPC endpoint or execute tests.
///
/// # Errors
///
/// This function returns an [`ExitCode`] rather than a `Result`. Any error
/// encountered while reading configuration or loading fixtures is printed to
/// stderr with an `error:` prefix and mapped to an appropriate exit code:
/// - [`ExitCode::ConfigurationError`] if `--protocol 0` is supplied, an
///   explicit configuration file cannot be read, or configuration is malformed.
/// - [`ExitCode::InvalidFixture`] if a fixture file is unreadable, contains
///   malformed TOML, has duplicate IDs, or references missing companion files.
/// - [`ExitCode::InternalError`] if the current working directory cannot be
///   read.
///
/// # Examples
///
/// ```text
/// # List fixtures for the default or configured protocol.
/// stellar-canary fixtures
///
/// # List fixtures targeting an explicit protocol.
/// stellar-canary fixtures --protocol 28
///
/// # Inspect fixtures in a custom directory.
/// stellar-canary fixtures --fixtures-dir ./custom-fixtures --protocol 28
/// ```
pub fn run_fixtures(args: FixturesArgs) -> ExitCode {
    match run_fixtures_inner(args) {
        Ok(exit_code) => exit_code,
        Err(err) => {
            print_error(&err);
            ExitCode::from(&err)
        }
    }
}

fn run_fixtures_inner(args: FixturesArgs) -> Result<ExitCode, CanaryError> {
    let project_root = ProjectRoot::resolve(args.project_root.as_deref())?;
    let root = project_root.path.clone();

    let config = match &args.config {
        Some(path) => canary_config::load(&project_root.resolve_input(path))?,
        None => canary_config::load_from_root(&root)?.unwrap_or_default(),
    };

    // `--fixtures-dir` overrides the configured `fixtures_dir`, which
    // overrides the built-in default (issue #474). The configuration file
    // is read even when `--protocol` is passed so the cascade always has
    // the file to fall back to.
    let settings = canary_config::resolve_settings(
        &config,
        canary_config::CliOverrides {
            fixtures_dir: args.fixtures_dir.clone(),
            ..canary_config::CliOverrides::default()
        },
    );
    let fixtures_dir = project_root.resolve_input(&settings.fixtures_dir);

    let protocol = validated_protocol_flag(args.protocol)?.unwrap_or(config.protocol);

    let loaded_fixtures = if fixtures_dir.is_dir() {
        canary_fixtures::load_directory(&fixtures_dir)?
    } else {
        Vec::new()
    };
    let store = canary_fixtures::validate(&loaded_fixtures)?;

    let format = if args.json {
        FixturesFormat::Json
    } else {
        args.format
    };
    match format {
        FixturesFormat::Terminal => {
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
                println!("(no fixtures found in {})", fixtures_dir.display());
            }
        }
        FixturesFormat::Json => {
            let inventory = fixtures_json::build(
                protocol,
                fixtures_dir.is_dir(),
                &fixtures_dir,
                &loaded_fixtures,
            );
            println!("{}", fixtures_json::render(&inventory));
        }
    }

    Ok(ExitCode::Pass)
}

/// Renders a previously stored JSON report to the console.
///
/// Reads the report file named by [`ReportArgs::path`] — the JSON written by
/// `stellar-canary check --json`, as described in
/// `docs/json-report-contract.md` — re-renders it in [`ReportArgs::format`],
/// and prints the result to stdout. When the path is `"-"`, the report is
/// read from standard input until EOF instead, which chains it onto another
/// command without touching disk:
/// `stellar-canary check --format json | stellar-canary report -`.
/// Standard input is capped at 10 MiB (D-06); a larger stream is rejected
/// rather than truncated. Unlike [`run_check`], it never touches the
/// network or re-runs any fixture, so it is safe to call offline, on CI, or
/// against a report produced by another machine.
///
/// Returns the [`ExitCode`] implied by the stored run, matching what
/// `check --json` would have returned for the same run:
/// [`ExitCode::ExecutionError`] if any stored result recorded an execution
/// error, otherwise [`ExitCode::CompatibilityFailure`] when the stored
/// decision is a failure, and [`ExitCode::Pass`] for a pass or warning.
///
/// # Errors
///
/// This function returns an [`ExitCode`] rather than a `Result`. A report
/// file that is missing or cannot be read, and a file that is not valid JSON
/// in the expected report shape, are both reported on stderr and mapped to
/// [`ExitCode::ConfigurationError`].
///
/// # Examples
///
/// ```text
/// # Render a stored report as Markdown (the default format).
/// stellar-canary report results.json
///
/// # Re-render the same report for a terminal or as JSON.
/// stellar-canary report results.json --format terminal
/// stellar-canary report results.json --format json
///
/// # Read the report from standard input instead of a file.
/// stellar-canary check --format json | stellar-canary report -
/// ```
pub fn run_report(args: ReportArgs) -> ExitCode {
    match run_report_inner(args) {
        Ok(exit_code) => exit_code,
        Err(err) => {
            print_error(&err);
            ExitCode::from(&err)
        }
    }
}

fn run_report_inner(args: ReportArgs) -> Result<ExitCode, CanaryError> {
    // `report -` reads the report from standard input until EOF (E-053),
    // so a report can be piped straight through without a temporary file.
    let json_text = if args.path == Path::new("-") {
        canary_report::read_report_json(std::io::stdin()).map_err(|e| {
            CanaryError::Configuration(format!("failed to read report from standard input: {e}"))
        })?
    } else {
        std::fs::read_to_string(&args.path).map_err(|e| {
            CanaryError::Configuration(format!(
                "failed to read report file {}: {e}",
                args.path.display()
            ))
        })?
    };
    let report_input = canary_report::JsonReporter::parse(&json_text)
        .map_err(|e| CanaryError::Configuration(format!("invalid report file: {e}")))?;

    let rendered = match args.format {
        OutputFormat::Terminal => TerminalReporter::render(&report_input),
        OutputFormat::Json => JsonReporter::render(&report_input),
        OutputFormat::Markdown => MarkdownReporter::render(&report_input),
        OutputFormat::JUnit => JUnitReporter::render(&report_input),
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

    fn fixtures_args(
        protocol: Option<u32>,
        fixtures_dir: &str,
        config: Option<&str>,
    ) -> FixturesArgs {
        FixturesArgs {
            protocol,
            fixtures_dir: Some(PathBuf::from(fixtures_dir)),
            project_root: None,
            config: config.map(PathBuf::from),
            format: crate::cli::FixturesFormat::Terminal,
            json: false,
        }
    }

    fn inspect_args(
        protocol: Option<u32>,
        fixtures_dir: &str,
        config: Option<&str>,
    ) -> InspectArgs {
        InspectArgs {
            protocol,
            fixtures_dir: Some(PathBuf::from(fixtures_dir)),
            project_root: None,
            config: config.map(PathBuf::from),
            format: crate::cli::InspectFormat::Terminal,
            json: false,
        }
    }

    #[test]
    fn without_a_flag_the_root_is_the_working_directory_and_inputs_are_untouched() {
        let root = ProjectRoot::resolve(None).unwrap();
        assert_eq!(root.path, std::env::current_dir().unwrap());
        assert!(!root.explicit);
        assert_eq!(
            root.resolve_input(Path::new("fixtures")),
            PathBuf::from("fixtures")
        );
    }

    #[test]
    fn an_explicit_root_is_canonicalized_and_resolves_relative_inputs() {
        let dir = create_unique_temp_dir();
        let nested = dir.join("member");
        std::fs::create_dir_all(&nested).unwrap();
        // A `..` segment proves the path is canonicalized, not just joined.
        let requested = nested.join("..").join("member");

        let root = ProjectRoot::resolve(Some(&requested)).unwrap();
        assert!(root.explicit);
        assert_eq!(
            root.path,
            without_verbatim_prefix(std::fs::canonicalize(&nested).unwrap())
        );
        assert!(root.path.is_absolute());
        assert_eq!(
            root.resolve_input(Path::new("fixtures")),
            root.path.join("fixtures")
        );

        let absolute = std::env::temp_dir().join("elsewhere");
        assert_eq!(root.resolve_input(&absolute), absolute);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_root_that_does_not_exist_is_a_configuration_error() {
        let missing = create_unique_temp_dir().join("no-such-directory");
        let err = ProjectRoot::resolve(Some(&missing))
            .err()
            .expect("an error");
        assert!(matches!(err, CanaryError::Configuration(_)));
        assert_eq!(ExitCode::from(&err), ExitCode::ConfigurationError);
        assert!(err.to_string().contains("does not exist"), "{err}");
        assert!(err.to_string().contains("--project-root"), "{err}");
    }

    #[test]
    fn a_project_root_that_is_a_file_is_a_configuration_error() {
        let dir = create_unique_temp_dir();
        let file = dir.join("not-a-directory");
        std::fs::write(&file, "x").unwrap();
        let err = ProjectRoot::resolve(Some(&file)).err().expect("an error");
        assert!(matches!(err, CanaryError::Configuration(_)));
        assert!(err.to_string().contains("is not a directory"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalized_commit_accepts_only_full_lowercase_hex_object_names() {
        let sha1 = "0123456789abcdef0123456789abcdef01234567";
        let sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(normalized_commit(sha1).as_deref(), Some(sha1));
        assert_eq!(normalized_commit(sha256).as_deref(), Some(sha256));
        assert_eq!(
            normalized_commit(&format!("{sha1}\n")).as_deref(),
            Some(sha1)
        );

        assert_eq!(normalized_commit(""), None);
        assert_eq!(normalized_commit("unknown"), None);
        assert_eq!(normalized_commit(&sha1[..39]), None);
        assert_eq!(normalized_commit(&sha1.to_uppercase()), None);
        assert_eq!(normalized_commit(&sha1.replace('0', "g")), None);
        assert_eq!(normalized_commit(&format!("{sha1}0")), None);
    }

    #[test]
    fn the_embedded_build_commit_is_unknown_or_a_full_object_name() {
        // Built from a Git checkout this is `Some`; built from a source
        // archive it is `None`. Either way it is never a placeholder.
        if let Some(commit) = build_commit() {
            assert!(commit.len() == 40 || commit.len() == 64, "{commit:?}");
            assert!(commit.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn run_version_returns_the_success_exit_code() {
        assert_eq!(run_version(), ExitCode::Pass);
    }

    #[test]
    fn run_fixtures_passes_when_the_fixture_directory_is_absent() {
        // An absent --fixtures-dir lists nothing rather than failing: fixtures
        // live in a separate repository, so a checkout without one is normal.
        let code = run_fixtures_inner(fixtures_args(Some(28), "no-such-fixtures-dir", None))
            .expect("an absent fixture directory is not an error");
        assert_eq!(code, ExitCode::Pass);
    }

    #[test]
    fn run_fixtures_rejects_a_zero_protocol_before_touching_the_filesystem() {
        let err = run_fixtures_inner(fixtures_args(Some(0), "no-such-fixtures-dir", None))
            .expect_err("--protocol 0 must not be accepted");
        assert!(
            matches!(err, CanaryError::Configuration(_)),
            "unexpected error: {err:?}"
        );
        assert!(
            err.to_string()
                .contains("--protocol must be a positive protocol version number"),
            "unexpected error text: {err}"
        );
    }

    #[test]
    fn run_fixtures_reports_a_missing_explicit_configuration_file() {
        // Only reached when no --protocol was given, since an explicit protocol
        // skips configuration loading entirely.
        let err = run_fixtures_inner(fixtures_args(
            None,
            "no-such-fixtures-dir",
            Some("no-such-config.stellar-canary.toml"),
        ))
        .expect_err("an explicit --config that does not exist must fail");
        assert!(
            matches!(err, CanaryError::Configuration(_)),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn run_inspect_reports_a_plan_for_an_empty_fixture_set() {
        // Inspect prints the resolved project, configuration and fixture plan
        // and succeeds even when the fixture directory holds nothing: the plan
        // is simply empty.
        let code = run_inspect_inner(inspect_args(Some(28), "no-such-fixtures-dir", None))
            .expect("an absent fixture directory is not an error");
        assert_eq!(code, ExitCode::Pass);
    }

    #[test]
    fn run_inspect_rejects_a_zero_protocol() {
        let err = run_inspect_inner(inspect_args(Some(0), "no-such-fixtures-dir", None))
            .expect_err("--protocol 0 must not be accepted");
        assert!(
            matches!(err, CanaryError::Configuration(_)),
            "unexpected error: {err:?}"
        );
        assert!(
            err.to_string()
                .contains("--protocol must be a positive protocol version number"),
            "unexpected error text: {err}"
        );
    }

    #[test]
    fn run_inspect_reports_a_missing_explicit_configuration_file() {
        let err = run_inspect_inner(inspect_args(
            None,
            "no-such-fixtures-dir",
            Some("no-such-config.stellar-canary.toml"),
        ))
        .expect_err("an explicit --config that does not exist must fail");
        assert!(
            matches!(err, CanaryError::Configuration(_)),
            "unexpected error: {err:?}"
        );
    }

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
            source: canary_core::ResultSource::Live,
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
            // Verification evidence lives with the fixture pack, not
            // with a stored run.
            verification: Vec::new(),
            decision,
            git: GitContext::default(),
            verbose: false,
            run: None,
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
