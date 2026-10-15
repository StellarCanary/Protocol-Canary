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

    /// Request timeout for the RPC client in seconds.
    #[arg(long = "rpc-timeout", default_value = "10")]
    pub rpc_timeout: u64,

    /// Directory containing fixture files (overrides `fixtures_dir` in
    /// .stellar-canary.toml; default: fixtures).
    #[arg(long = "fixtures-dir", value_name = "PATH")]
    pub fixtures_dir: Option<PathBuf>,

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
}

#[derive(Debug, Parser)]
pub struct ReportArgs {
    /// Path to a JSON report produced by `stellar-canary check --json`.
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
}
