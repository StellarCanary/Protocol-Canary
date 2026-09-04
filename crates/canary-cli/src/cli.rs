//! Command-line argument definitions.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "stellar-canary",
    version,
    about = "Rehearse Stellar protocol upgrades before they reach your production stack."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
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
pub enum OutputFormat {
    Terminal,
    Json,
    Markdown,
}

#[derive(Debug, Parser)]
pub struct CheckArgs {
    /// Target protocol version (overrides configuration).
    #[arg(long)]
    pub protocol: Option<u32>,

    /// Network to run live checks against.
    #[arg(long, default_value = "testnet")]
    pub network: String,

    /// RPC endpoint to use for live checks.
    #[arg(long = "rpc-url")]
    pub rpc_url: Option<String>,

    /// Path to a configuration file (default: .stellar-canary.toml in the
    /// project root).
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Directory containing fixture files.
    #[arg(long = "fixtures-dir", default_value = "fixtures")]
    pub fixtures_dir: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Terminal)]
    pub format: OutputFormat,

    /// Shorthand for --format json.
    #[arg(long)]
    pub json: bool,

    #[arg(long)]
    pub verbose: bool,

    #[arg(long)]
    pub quiet: bool,
}

#[derive(Debug, Parser)]
pub struct InspectArgs {
    /// Target protocol version (overrides configuration).
    #[arg(long)]
    pub protocol: Option<u32>,

    /// Directory containing fixture files to inspect.
    #[arg(long = "fixtures-dir", default_value = "fixtures")]
    pub fixtures_dir: PathBuf,

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

    /// Directory containing fixture files.
    #[arg(long = "fixtures-dir", default_value = "fixtures")]
    pub fixtures_dir: PathBuf,

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
