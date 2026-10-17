//! Command-line argument definitions.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "stellar-canary",
    version,
    about = "Rehearse Stellar protocol upgrades before they reach your production stack."
)]
/// Top-level command-line interface for the `stellar-canary` binary.
///
/// `Cli` is the `clap` entry point: it holds the one required subcommand the
/// user typed — [`Command::Check`], [`Command::Inspect`],
/// [`Command::Fixtures`], [`Command::Report`] or [`Command::Version`] —
/// together with that subcommand's own argument struct. The binary name and
/// version banner come from the `#[command(...)]` attributes above, and flags
/// that apply to a single run live on the per-command `*Args` types rather
/// than here.
///
/// Building a `Cli` parses process arguments only: it performs no I/O of its
/// own and does not panic. Invalid input is a failure condition rather than
/// an error value — `clap` prints the usage message to stderr and exits the
/// process with status 2 before a `Cli` is ever constructed, so any `Cli`
/// value that exists represents a successfully parsed invocation.
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
/// Top-level commands accepted by the `stellar-canary` CLI.
///
/// `Check` runs compatibility assertions, while `Inspect` and `Fixtures`
/// provide offline project and fixture information. `Report` renders a
/// previously generated JSON report, and `Version` prints the tool version.
/// Parsing this enum is handled by `clap`; the variants themselves do not
/// perform I/O or panic during argument parsing.
pub enum Command {
    /// Run compatibility checks against the current project.
    Check(CheckArgs),
    /// Print offline project diagnostics.
    Inspect(InspectArgs),
    /// List fixtures available for a protocol.
    Fixtures(FixturesArgs),
    /// Render a previously generated JSON report.
    Report(ReportArgs),
    /// Manage the local result cache.
    Cache(CacheArgs),
    /// Print the tool version.
    Version,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
/// Selects the renderer used for a compatibility report.
///
/// The `--format` flag accepts these variants using their kebab-case names.
/// `Terminal` is the human-readable default, `Json` produces the versioned
/// machine-readable report consumed by automation, and `Markdown` produces
/// the report shape used in job summaries and pull-request comments. The
/// `--json` shorthand on `check` selects [`OutputFormat::Json`] explicitly.
pub enum OutputFormat {
    /// Human-readable output for an interactive terminal.
    Terminal,
    /// Versioned JSON output for automation and report persistence.
    Json,
    /// Markdown output for summaries and pull-request comments.
    Markdown,
    /// JUnit XML output for CI test-summary dashboards.
    #[value(name = "junit")]
    JUnit,
}

#[derive(Debug, Parser)]
pub struct CheckArgs {
    /// Target protocol version (overrides configuration).
    #[arg(long)]
    pub protocol: Option<u32>,

    /// Network to run live checks against (overrides `network` in
    /// .stellar-canary.toml; default: testnet).
    #[arg(long)]
    pub network: Option<String>,

    /// RPC endpoint to use for live checks.
    #[arg(long = "rpc-url")]
    pub rpc_url: Option<String>,

    /// Network passphrase used to verify the endpoint's identity and to
    /// sign transactions for this run. Defaults to the well-known
    /// passphrase for --network; custom networks are only verified when
    /// this flag is given. Empty values are rejected.
    #[arg(
        long = "network-passphrase",
        value_name = "PASSPHRASE",
        value_parser = parse_network_passphrase,
    )]
    pub network_passphrase: Option<String>,

    /// Directory to treat as the project root (default: the current
    /// directory). Relative `--config`, `--fixtures-dir`, `--cache-dir` and
    /// `--output` paths are resolved against it. The root is never searched
    /// for in parent directories.
    #[arg(long = "project-root", value_name = "PATH")]
    pub project_root: Option<PathBuf>,

    /// Path to a configuration file (default: .stellar-canary.toml in the
    /// project root).
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Request timeout for the RPC client in seconds.
    #[arg(long = "rpc-timeout", default_value = "10")]
    pub rpc_timeout: u64,

    /// Maximum number of retries after a failed RPC attempt. `0` disables
    /// retries: the call fails on the first network error. Each retry is a
    /// full request, so the total number of attempts is n + 1.
    #[arg(long = "rpc-max-retries", default_value_t = 3)]
    pub rpc_max_retries: u32,

    /// Base delay between RPC retry attempts, in milliseconds. The backoff
    /// is linear: attempt n waits n times this value.
    #[arg(long = "rpc-retry-delay-ms", default_value_t = 500)]
    pub rpc_retry_delay_ms: u64,

    /// Directory containing fixture files (overrides `fixtures_dir` in
    /// .stellar-canary.toml; default: fixtures).
    #[arg(long = "fixtures-dir", value_name = "PATH")]
    pub fixtures_dir: Option<PathBuf>,

    /// Directory for cached fixture execution results (default:
    /// .stellar-canary-cache in the project root). The directory is created
    /// if it does not exist. Relative paths are resolved against
    /// --project-root when it is given.
    #[arg(long = "cache-dir", value_name = "PATH")]
    pub cache_dir: Option<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Terminal)]
    pub format: OutputFormat,

    /// Shorthand for --format json.
    #[arg(long)]
    pub json: bool,

    /// Write the rendered report to this path, in addition to stdout.
    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Allow a run in which no fixture applies. Without this flag such a run
    /// fails with exit code 2, because executing nothing is not evidence of
    /// compatibility.
    #[arg(long)]
    pub allow_empty: bool,

    /// Include skip reasons in Markdown/terminal output and populate the JSON report's verbose field.
    #[arg(long)]
    pub verbose: bool,

    /// Shorten terminal-format output to a single status line.
    #[arg(long)]
    pub quiet: bool,

    /// Maximum number of concurrent network requests for RPC/Soroban fixtures.
    #[arg(long, default_value_t = 4)]
    pub max_concurrency: u32,

    /// Do not read or write the local result cache. Every fixture runs fresh.
    #[arg(long, conflicts_with = "live_cache_ttl")]
    pub no_cache: bool,

    /// Allow RPC and Soroban results from the local cache to be replayed for
    /// up to this many seconds. Off by default: a cached live result says
    /// nothing about what the network does now. Replayed results are marked
    /// `"source": "cache"` in the JSON report. XDR results are offline and
    /// are always cacheable.
    #[arg(
        long,
        value_name = "SECONDS",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub live_cache_ttl: Option<u64>,
}

/// Arguments for the `cache` command group.
#[derive(Debug, Subcommand)]
pub enum CacheArgs {
    /// Delete cached fixture execution results and report reclaimed space.
    Clear(CacheClearArgs),
}

/// Arguments for `stellar-canary cache clear`.
#[derive(Debug, Parser)]
pub struct CacheClearArgs {
    /// Directory to treat as the project root (default: the current
    /// directory). Relative `--cache-dir` paths are resolved against it.
    #[arg(long = "project-root", value_name = "PATH")]
    pub project_root: Option<PathBuf>,

    /// Directory for cached fixture execution results (default:
    /// .stellar-canary-cache in the project root).
    #[arg(long = "cache-dir", value_name = "PATH")]
    pub cache_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
/// Selects the renderer used by `inspect`.
///
/// `Terminal` is today's human-readable output, byte for byte. `Json`
/// prints the versioned CA-03 document for CI pipelines and tooling.
/// `markdown` is deliberately not accepted here: `inspect` renders no
/// report.
pub enum InspectFormat {
    /// Human-readable output for an interactive terminal.
    Terminal,
    /// Versioned JSON output (CA-03) for automation.
    Json,
}

/// Rejects `--network-passphrase ""` at parse time.
///
/// An empty expected passphrase is how `canary_rpc::validate_network_info`
/// *skips* the network-identity comparison, so an empty flag value would
/// silently disable the very check the flag exists to strengthen. Failing
/// the argument instead makes the mistake loud: clap reports an argument
/// validation error (exit code 2) before anything runs.
fn parse_network_passphrase(value: &str) -> Result<String, String> {
    if value.is_empty() {
        Err("the passphrase cannot be empty".to_string())
    } else {
        Ok(value.to_string())
    }
}

#[derive(Debug, Parser)]
pub struct InspectArgs {
    /// Target protocol version (overrides configuration).
    #[arg(long)]
    pub protocol: Option<u32>,

    /// Directory containing fixture files to inspect (overrides
    /// `fixtures_dir` in .stellar-canary.toml; default: fixtures).
    #[arg(long = "fixtures-dir", value_name = "PATH")]
    pub fixtures_dir: Option<PathBuf>,

    /// Directory to treat as the project root (default: the current
    /// directory). Relative `--config`, `--fixtures-dir` and `--output`
    /// paths are resolved against it. The root is never searched for in
    /// parent directories.
    #[arg(long = "project-root", value_name = "PATH")]
    pub project_root: Option<PathBuf>,

    /// Path to a configuration file (default: .stellar-canary.toml in the
    /// project root).
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = InspectFormat::Terminal)]
    pub format: InspectFormat,

    /// Shorthand for --format json.
    #[arg(long)]
    pub json: bool,

    /// Include verbose diagnostic details in output.
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
/// Selects the renderer used by `fixtures`.
///
/// `Terminal` is today's human-readable listing, byte for byte. `Json`
/// prints the versioned CA-02 inventory for CI pipelines and tooling.
/// `markdown` is deliberately not accepted: `fixtures` renders no
/// report.
pub enum FixturesFormat {
    /// Human-readable output for an interactive terminal.
    Terminal,
    /// Versioned JSON inventory (CA-02) for automation.
    Json,
}

#[derive(Debug, Parser)]
pub struct FixturesArgs {
    /// Protocol version to list fixtures for (default: the configured
    /// protocol, or 28 if there is no configuration file).
    #[arg(long)]
    pub protocol: Option<u32>,

    /// Directory containing fixture files (overrides `fixtures_dir` in
    /// .stellar-canary.toml; default: fixtures).
    #[arg(long = "fixtures-dir", value_name = "PATH")]
    pub fixtures_dir: Option<PathBuf>,

    /// Directory to treat as the project root (default: the current
    /// directory). Relative `--config`, `--fixtures-dir` and `--output`
    /// paths are resolved against it. The root is never searched for in
    /// parent directories.
    #[arg(long = "project-root", value_name = "PATH")]
    pub project_root: Option<PathBuf>,

    /// Path to a configuration file (default: .stellar-canary.toml in the
    /// project root).
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = FixturesFormat::Terminal)]
    pub format: FixturesFormat,

    /// Shorthand for --format json.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Parser)]
/// Arguments for the `stellar-canary report` subcommand ([`Command::Report`]).
///
/// `report` re-renders a JSON report that an earlier `check` run stored (the
/// contract in `docs/json-report-contract.md`) into another format, without
/// touching the network, the fixtures, or the project. It exists so a report
/// captured on one machine — or in CI — can be re-rendered elsewhere, and so
/// `check --format json | stellar-canary report -` can pipe a run straight
/// into a Markdown job summary.
///
/// The two fields map one-to-one onto the subcommand's arguments:
///
/// - [`ReportArgs::path`] is the positional report file, or `-` to read the
///   report from standard input until EOF (capped at 10 MiB).
/// - [`ReportArgs::format`] is the `--format` flag: `markdown` by default,
///   with `terminal`, `json` and `junit` available.
///
/// Parsing is performed by `clap` and performs no I/O and cannot panic: a
/// missing path, an unknown format, or an unreadable or malformed report is
/// a failure of the *run*, not of parsing — `run_report` reports it on
/// stderr and maps it to a configuration error (exit code 2). The parse
/// itself only fails with clap's usage message and exit status 2, before a
/// `ReportArgs` value ever exists.
///
/// # Examples
///
/// ```text
/// stellar-canary report results.json
/// stellar-canary report results.json --format json
/// stellar-canary check --format json | stellar-canary report -
/// ```
pub struct ReportArgs {
    /// Path to a JSON report produced by `stellar-canary check --json`, or
    /// `-` to read the report from standard input until EOF (10 MiB max).
    pub path: PathBuf,

    /// Output format to render the stored report as.
    #[arg(long, value_enum, default_value_t = OutputFormat::Markdown)]
    pub format: OutputFormat,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn test_max_concurrency_flag_default() {
        let cli = Cli::parse_from(["stellar-canary", "check"]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.max_concurrency, 4);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn test_max_concurrency_flag_custom() {
        let cli = Cli::parse_from(["stellar-canary", "check", "--max-concurrency", "10"]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.max_concurrency, 10);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn project_root_defaults_to_none_for_every_command() {
        for command in ["check", "inspect", "fixtures"] {
            let cli = Cli::parse_from(["stellar-canary", command]);
            let root = match cli.command {
                Command::Check(args) => args.project_root,
                Command::Inspect(args) => args.project_root,
                Command::Fixtures(args) => args.project_root,
                other => panic!("unexpected command {other:?}"),
            };
            assert_eq!(root, None, "{command}");
        }
    }

    #[test]
    fn project_root_flag_takes_a_path_on_every_command() {
        for command in ["check", "inspect", "fixtures"] {
            let cli = Cli::parse_from(["stellar-canary", command, "--project-root", "member"]);
            let root = match cli.command {
                Command::Check(args) => args.project_root,
                Command::Inspect(args) => args.project_root,
                Command::Fixtures(args) => args.project_root,
                other => panic!("unexpected command {other:?}"),
            };
            assert_eq!(root, Some(PathBuf::from("member")), "{command}");
        }
    }

    #[test]
    fn test_output_flag_defaults_to_none() {
        let cli = Cli::parse_from(["stellar-canary", "check"]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.output, None);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn test_output_flag_takes_a_path() {
        let cli = Cli::parse_from(["stellar-canary", "check", "--output", "result.json"]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.output, Some(PathBuf::from("result.json")));
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn cache_dir_flag_defaults_to_none() {
        let cli = Cli::parse_from(["stellar-canary", "check"]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.cache_dir, None);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn cache_dir_flag_takes_a_path() {
        let cli = Cli::parse_from([
            "stellar-canary",
            "check",
            "--cache-dir",
            "build/canary-cache",
        ]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.cache_dir, Some(PathBuf::from("build/canary-cache")));
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn network_passphrase_flag_is_absent_by_default() {
        let cli = Cli::parse_from(["stellar-canary", "check"]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.network_passphrase, None);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn network_passphrase_flag_accepts_a_custom_value() {
        let cli = Cli::parse_from([
            "stellar-canary",
            "check",
            "--network-passphrase",
            "Local Standalone Network ; October 2026",
        ]);
        if let Command::Check(args) = cli.command {
            assert_eq!(
                args.network_passphrase.as_deref(),
                Some("Local Standalone Network ; October 2026")
            );
        } else {
            panic!("Expected Check command");
        }
    }

    /// An empty passphrase is not "skip the check" — it is a mistake, so
    /// clap must reject it as an argument validation error (exit code 2)
    /// rather than let it parse.
    #[test]
    fn network_passphrase_flag_rejects_an_empty_value() {
        let err = Cli::try_parse_from(["stellar-canary", "check", "--network-passphrase", ""])
            .expect_err("an empty passphrase must fail argument validation");
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
        let text = err.to_string();
        assert!(text.contains("--network-passphrase"), "{text}");
        assert!(text.contains("the passphrase cannot be empty"), "{text}");
    }

    #[test]
    fn rpc_retry_flags_default_to_three_retries_and_500_milliseconds() {
        let cli = Cli::parse_from(["stellar-canary", "check"]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.rpc_max_retries, 3);
            assert_eq!(args.rpc_retry_delay_ms, 500);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn rpc_retry_flags_take_custom_values() {
        let cli = Cli::parse_from([
            "stellar-canary",
            "check",
            "--rpc-max-retries",
            "0",
            "--rpc-retry-delay-ms",
            "250",
        ]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.rpc_max_retries, 0);
            assert_eq!(args.rpc_retry_delay_ms, 250);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn cache_clear_subcommand_defaults_to_none() {
        let cli = Cli::parse_from(["stellar-canary", "cache", "clear"]);
        if let Command::Cache(CacheArgs::Clear(args)) = cli.command {
            assert_eq!(args.cache_dir, None);
            assert_eq!(args.project_root, None);
        } else {
            panic!("Expected Cache::Clear command");
        }
    }

    #[test]
    fn cache_clear_subcommand_takes_a_cache_dir() {
        let cli = Cli::parse_from([
            "stellar-canary",
            "cache",
            "clear",
            "--cache-dir",
            "build/canary-cache",
        ]);
        if let Command::Cache(CacheArgs::Clear(args)) = cli.command {
            assert_eq!(args.cache_dir, Some(PathBuf::from("build/canary-cache")));
        } else {
            panic!("Expected Cache::Clear command");
        }
    }

    #[test]
    fn cache_clear_subcommand_takes_a_project_root() {
        let cli = Cli::parse_from([
            "stellar-canary",
            "cache",
            "clear",
            "--project-root",
            "member",
        ]);
        if let Command::Cache(CacheArgs::Clear(args)) = cli.command {
            assert_eq!(args.project_root, Some(PathBuf::from("member")));
        } else {
            panic!("Expected Cache::Clear command");
        }
    }
}
