//! Command-line argument definitions.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use canary_core::Surface;

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
/// [`Command::Fixtures`], [`Command::Report`], [`Command::Diff`],
/// [`Command::Cache`], [`Command::Lock`], [`Command::Completions`] or
/// [`Command::Version`] — together with that subcommand's own argument
/// struct. The binary name and
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
// `CheckArgs` is intentionally passed through unchanged from clap parsing;
// boxing it would alter the public command enum shape for a small size gain.
#[allow(clippy::large_enum_variant)]
/// Top-level commands accepted by the `stellar-canary` CLI.
///
/// `Check` runs compatibility assertions, while `Inspect` and `Fixtures`
/// provide offline project and fixture information. `Report` renders a
/// previously generated JSON report, `Diff` compares two stored reports
/// (CF-05), `Cache` manages cached execution results, `Lock` writes and
/// verifies the fixture-pack pin, `Completions` prints a shell
/// completion script, and `Version` prints the tool version. Parsing
/// this enum is handled by `clap`; the variants themselves do not perform
/// I/O or panic during argument parsing.
pub enum Command {
    /// Run compatibility checks against the current project.
    Check(CheckArgs),
    /// Print offline project diagnostics.
    Inspect(InspectArgs),
    /// List fixtures available for a protocol.
    Fixtures(FixturesArgs),
    /// Render a previously generated JSON report.
    Report(ReportArgs),
    /// Compare two stored JSON reports and render the diff (CF-05).
    Diff(DiffArgs),
    /// Manage the local result cache.
    #[command(subcommand)]
    Cache(CacheArgs),
    /// Manage the fixture-pack lockfile: bare `lock` writes the pin
    /// (CF-03-B), `lock verify` re-checks it (CF-03-D).
    Lock(LockArgs),
    /// Print a shell completion script.
    Completions(CompletionsArgs),
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
/// Arguments for the `stellar-canary check` subcommand ([`Command::Check`]).
///
/// `check` is the compatibility run itself: it loads the project and its
/// fixtures, executes the XDR, RPC and Soroban surfaces selected by the
/// configuration file, and renders a pass/warn/fail report. Every flag here
/// is an override or a qualifier for that one run; the resolved settings
/// cascade as CLI flag > `.stellar-canary.toml` > built-in default, per key
/// (see `canary_config::resolve_settings`).
///
/// The fields map one-to-one onto the subcommand's flags. Notably:
///
/// - [`CheckArgs::protocol`], [`CheckArgs::network`] and
///   [`CheckArgs::fixtures_dir`] override the configuration file without
///   changing it.
/// - [`CheckArgs::format`] selects the renderer, and [`CheckArgs::json`] is a
///   shorthand for `--format json`; [`CheckArgs::output`] also writes the
///   rendered report to a file.
/// - [`CheckArgs::offline`] forbids network access, [`CheckArgs::allow_empty`]
///   permits a run that selects no fixture, and
///   [`CheckArgs::fail_on_warning`] turns warnings into a failing exit code.
///
/// Parsing is performed by `clap`: it performs no I/O and does not panic.
/// Invalid input is reported as a usage message on stderr with exit status 2
/// before a `CheckArgs` value exists. Failures of the run itself — a missing
/// configuration file, an unreachable RPC endpoint, fixture errors — are
/// reported by the command implementation with the exit codes documented in
/// the report contract.
///
/// # Examples
///
/// ```text
/// stellar-canary check
/// stellar-canary check --offline --format json --output report.json
/// stellar-canary check --surface soroban --fixture p28-soroban-native-asset-name
/// ```
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

    /// Run only the fixture with this exact ID.
    #[arg(long, value_name = "ID")]
    pub fixture: Option<String>,

    /// Run fixtures from only this compatibility surface.
    #[arg(long, value_enum)]
    pub surface: Option<SurfaceFilter>,

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

    /// Run offline only: execute local XDR checks and skip every fixture
    /// that requires network access (RPC and Soroban). No network socket is
    /// opened. Skipped network fixtures are reported with the skip code
    /// `OFFLINE`.
    #[arg(long)]
    pub offline: bool,

    /// Treat fixture warnings as compatibility failures (exit code 1).
    #[arg(long)]
    pub fail_on_warning: bool,

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

/// Surface choices accepted by `check --surface`.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum SurfaceFilter {
    Xdr,
    Rpc,
    Soroban,
}

impl From<SurfaceFilter> for Surface {
    fn from(value: SurfaceFilter) -> Self {
        match value {
            SurfaceFilter::Xdr => Surface::Xdr,
            SurfaceFilter::Rpc => Surface::Rpc,
            SurfaceFilter::Soroban => Surface::Soroban,
        }
    }
}

/// Arguments for the `cache` command group.
#[derive(Debug, Subcommand)]
pub enum CacheArgs {
    /// Delete cached fixture execution results and report reclaimed space.
    Clear(CacheClearArgs),
}

/// Subcommands of the `lock` command group (CF-03).
///
/// `lock verify` re-checks the pin a `.stellar-canary.lock` file holds: it
/// loads the lock under the parser rules of CF-03 section 3, computes the
/// `canary-pack-v1` digest (CF-02) of the fixture directory the
/// configuration resolves to, and compares the two. It never writes: the
/// fixture pack, the lockfile, and the cache are left exactly as they are.
/// Without a subcommand, the parent [`LockArgs`] performs CF-03-B's
/// write instead.
#[derive(Debug, Subcommand)]
pub enum LockCommand {
    /// Verify that the lockfile still matches the fixture pack.
    Verify(LockVerifyArgs),
}

/// Arguments for `stellar-canary lock verify`.
///
/// # Examples
///
/// ```text
/// stellar-canary lock verify
/// stellar-canary lock verify --project-root path/to/project
/// stellar-canary lock verify --fixtures-dir ci/fixtures
/// ```
#[derive(Debug, Parser)]
pub struct LockVerifyArgs {
    /// Directory to treat as the project root (default: the current
    /// directory). Relative `--config` and `--fixtures-dir` paths are
    /// resolved against it. The root is never searched for in parent
    /// directories.
    #[arg(long = "project-root", value_name = "PATH")]
    pub project_root: Option<PathBuf>,

    /// Directory containing fixture files (overrides `fixtures_dir` in
    /// .stellar-canary.toml; default: fixtures). Overriding the location
    /// does not waive the pin: the chosen directory is still verified
    /// against `packDigest`.
    #[arg(long = "fixtures-dir", value_name = "PATH")]
    pub fixtures_dir: Option<PathBuf>,

    /// Path to a configuration file (default: .stellar-canary.toml in the
    /// project root).
    #[arg(long)]
    pub config: Option<PathBuf>,
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
/// Arguments for the `stellar-canary inspect` subcommand ([`Command::Inspect`]).
///
/// `inspect` prints offline diagnostics about the project and the fixtures it
/// would run: the detected project type, the effective protocol, and the
/// fixture inventory. It never opens a network socket, so it is safe to run
/// in sandboxes and before credentials are configured.
///
/// The fields map one-to-one onto the subcommand's flags. Relative paths for
/// [`InspectArgs::config`] and [`InspectArgs::fixtures_dir`] are resolved
/// against [`InspectArgs::project_root`] when it is given, and the project
/// root is never discovered by walking parent directories.
/// [`InspectArgs::format`] selects terminal or versioned JSON output, with
/// [`InspectArgs::json`] as the shorthand for JSON.
///
/// Parsing is performed by `clap`: it performs no I/O and does not panic.
/// Invalid input is a usage error (exit status 2) reported before an
/// `InspectArgs` value exists; failures of the command itself, such as an
/// unreadable configuration file, are reported by the command implementation
/// as configuration errors (exit status 2).
///
/// # Examples
///
/// ```text
/// stellar-canary inspect
/// stellar-canary inspect --json --project-root path/to/project
/// ```
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
/// Arguments for the `stellar-canary fixtures` subcommand ([`Command::Fixtures`]).
///
/// `fixtures` lists the fixtures available for a protocol, optionally as the
/// versioned CA-02 JSON inventory for tooling and CI. It reads fixture files
/// from disk only: no network request is made and no fixture is executed.
///
/// The fields map one-to-one onto the subcommand's flags.
/// [`FixturesArgs::protocol`] overrides the configured protocol for the
/// listing, and relative [`FixturesArgs::config`] and
/// [`FixturesArgs::fixtures_dir`] paths are resolved against
/// [`FixturesArgs::project_root`] when it is given. [`FixturesArgs::format`]
/// selects terminal or versioned JSON output, with [`FixturesArgs::json`] as
/// the shorthand for JSON.
///
/// Parsing is performed by `clap`: it performs no I/O and does not panic.
/// Invalid input is a usage error (exit status 2) reported before a
/// `FixturesArgs` value exists; failures of the command itself, such as a
/// missing fixture directory or an unreadable configuration file, are
/// reported by the command implementation as configuration errors (exit
/// status 2).
///
/// # Examples
///
/// ```text
/// stellar-canary fixtures --protocol 28
/// stellar-canary fixtures --json --fixtures-dir path/to/fixtures
/// ```
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

    /// Filter fixtures by compatibility surface.
    #[arg(long, value_enum)]
    pub surface: Option<SurfaceFilter>,

    /// Filter fixtures by category.
    #[arg(long)]
    pub category: Option<String>,
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

#[derive(Debug, Parser)]
/// Arguments for the `stellar-canary diff` subcommand ([`Command::Diff`]).
///
/// `diff` compares two previously stored JSON reports offline and renders
/// the comparison as the deterministic `diffVersion: 1` document of
/// CF-05. Nothing is re-run and no network call is made. The argument
/// order is **current first, baseline second**, matching the question
/// "what changed since the baseline".
///
/// `diff` gates CI on the shared exit-code contract as read through
/// CF-05 section 5: `0` ([`canary_core::ExitCode::Pass`]) for verdict
/// `no-change` or `improved`, `1`
/// ([`canary_core::ExitCode::CompatibilityFailure`]) for verdict
/// `regression` or `coverage-lost`, and `2`
/// ([`canary_core::ExitCode::ConfigurationError`]) when the reports
/// are not comparable or an input file is missing or invalid.
///
/// Parsing is performed by `clap` and performs no I/O and cannot panic: a
/// missing path is a usage error (exit status 2) before a `DiffArgs`
/// value ever exists.
///
/// # Examples
///
/// ```text
/// stellar-canary diff current.json baseline.json
/// ```
pub struct DiffArgs {
    /// Path to the current report (produced by `stellar-canary check --json`).
    pub current_report: PathBuf,

    /// Path to the baseline report to compare the current report against.
    pub baseline_report: PathBuf,
}

#[derive(Debug, Parser)]
/// Arguments for the `stellar-canary lock` command ([`Command::Lock`]).
///
/// Bare `lock` validates the fixture pack, computes its canonical
/// `canary-pack-v1` digest (CF-02) and writes a `.stellar-canary.lock`
/// (CF-03) at the project root pinning the digest, the target protocol
/// and the tool version (CF-03-B). The serializer is deterministic: the
/// same pack always yields a byte-identical file. The pack is fully
/// validated before the lockfile is touched, so a failure never leaves a
/// partial file behind.
///
/// `lock verify` (CF-03-D) is the read-only counterpart, defined on
/// [`LockCommand`].
///
/// Parsing is performed by `clap` and performs no I/O and cannot panic.
///
/// # Examples
///
/// ```text
/// stellar-canary lock
/// stellar-canary lock --protocol 29 --fixtures-dir fixtures
/// stellar-canary lock verify
/// ```
pub struct LockArgs {
    /// The read-only verification subcommand; absent, `lock` writes.
    #[command(subcommand)]
    pub command: Option<LockCommand>,

    /// Target protocol version (overrides configuration).
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

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
/// The shells `stellar-canary completions` accepts.
///
/// The four variants cover every shell this release generates scripts for
/// and map one-to-one onto the matching [`clap_complete::Shell`] generator
/// (see the `From` impl below). clap validates the positional argument
/// against these variants, so an unsupported name — or a misspelling — is
/// a usage error (exit status 2) whose message lists the accepted input
/// spellings, `bash`, `zsh`, `fish` and `powershell`. There is no default
/// shell: input is never silently mapped to one.
pub enum CompletionShell {
    /// Bourne-Again shell (`bash`).
    Bash,
    /// Z shell (`zsh`).
    Zsh,
    /// Friendly interactive shell (`fish`).
    Fish,
    /// Microsoft PowerShell (`powershell`).
    #[value(name = "powershell")]
    PowerShell,
}

impl From<CompletionShell> for clap_complete::Shell {
    fn from(value: CompletionShell) -> Self {
        match value {
            CompletionShell::Bash => clap_complete::Shell::Bash,
            CompletionShell::Zsh => clap_complete::Shell::Zsh,
            CompletionShell::Fish => clap_complete::Shell::Fish,
            CompletionShell::PowerShell => clap_complete::Shell::PowerShell,
        }
    }
}

#[derive(Debug, Parser)]
/// Arguments for the `stellar-canary completions` subcommand
/// ([`Command::Completions`]).
///
/// `completions` prints a shell completion script for the requested shell
/// on stdout. The script is generated from the same [`Cli`] command
/// definition the binary parses with, so it always describes the real
/// command tree — every subcommand, option and flag this build accepts.
/// Stdout carries only the script, which makes it safe to redirect or pipe:
///
/// ```text
/// stellar-canary completions bash > /etc/bash_completion.d/stellar-canary
/// stellar-canary completions zsh > "${fpath[1]}/_stellar-canary"
/// stellar-canary completions fish > ~/.config/fish/completions/stellar-canary.fish
/// stellar-canary completions powershell > stellar-canary.ps1
/// ```
///
/// Parsing is performed by `clap` and does not perform I/O or panic. The
/// single positional [`CompletionsArgs::shell`] is required and is
/// validated against [`CompletionShell`]: a missing, unknown or misspelled
/// shell is a usage error reported on stderr with exit status 2 — and with
/// the supported shells listed — before a `CompletionsArgs` value exists,
/// so an error can never contaminate the completion script on stdout.
pub struct CompletionsArgs {
    /// Shell to generate a completion script for: bash, zsh, fish or
    /// powershell.
    #[arg(value_enum, value_name = "SHELL")]
    pub shell: CompletionShell,
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
    fn test_offline_flag_defaults_to_false() {
        let cli = Cli::parse_from(["stellar-canary", "check"]);
        if let Command::Check(args) = cli.command {
            assert!(!args.offline);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn fail_on_warning_flag_defaults_to_false() {
        let cli = Cli::parse_from(["stellar-canary", "check"]);
        if let Command::Check(args) = cli.command {
            assert!(!args.fail_on_warning);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn fail_on_warning_flag_can_be_enabled() {
        let cli = Cli::parse_from(["stellar-canary", "check", "--fail-on-warning"]);
        if let Command::Check(args) = cli.command {
            assert!(args.fail_on_warning);
        } else {
            panic!("Expected Check command");
        }
    }

    #[test]
    fn check_accepts_fixture_and_surface_filters() {
        let cli = Cli::parse_from([
            "stellar-canary",
            "check",
            "--fixture",
            "p28-xdr-1",
            "--surface",
            "xdr",
        ]);
        if let Command::Check(args) = cli.command {
            assert_eq!(args.fixture.as_deref(), Some("p28-xdr-1"));
            assert_eq!(args.surface, Some(SurfaceFilter::Xdr));
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
    fn test_offline_flag_parses() {
        let cli = Cli::parse_from(["stellar-canary", "check", "--offline"]);
        if let Command::Check(args) = cli.command {
            assert!(args.offline);
        } else {
            panic!("Expected Check command");
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

    #[test]
    fn completions_parses_every_supported_shell_name() {
        for shell in ["bash", "zsh", "fish", "powershell"] {
            let cli = Cli::parse_from(["stellar-canary", "completions", shell]);
            if let Command::Completions(args) = cli.command {
                assert_eq!(args.shell.to_possible_value().unwrap().get_name(), shell);
            } else {
                panic!("Expected Completions command");
            }
        }
    }

    /// An unsupported shell must fail argument validation (never fall back
    /// to a default shell), and the error must list every accepted name so
    /// the user can correct it without reading the docs.
    #[test]
    fn completions_rejects_an_unsupported_shell_and_lists_the_supported_ones() {
        let err = Cli::try_parse_from(["stellar-canary", "completions", "unsupported"])
            .expect_err("an unsupported shell must fail argument validation");
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
        let text = err.to_string();
        for shell in ["bash", "zsh", "fish", "powershell"] {
            assert!(
                text.contains(shell),
                "the error should list {shell:?}: {text}"
            );
        }
    }

    /// The shell is the one required argument of `completions`; asking for
    /// it without giving it is a usage error on stderr, not a panic or a
    /// script for a guessed shell.
    #[test]
    fn completions_requires_a_shell() {
        let err = Cli::try_parse_from(["stellar-canary", "completions"])
            .expect_err("a missing shell must fail argument validation");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        let text = err.to_string();
        assert!(text.contains("SHELL"), "{text}");
    }

    /// `ValueEnum` must render the PowerShell input name as the single
    /// word `powershell` (not the derived kebab-case `power-shell`), since
    /// that spelling is what the docs, the help text and users use.
    #[test]
    fn completion_shell_input_names_are_the_documented_ones() {
        let names: Vec<String> = CompletionShell::value_variants()
            .iter()
            .map(|shell| shell.to_possible_value().unwrap().get_name().to_string())
            .collect();
        assert_eq!(names, ["bash", "zsh", "fish", "powershell"]);
    }

    #[test]
    fn fixtures_flags_parse_surface_and_category() {
        let cli = Cli::parse_from([
            "stellar-canary",
            "fixtures",
            "--surface",
            "rpc",
            "--category",
            "network",
        ]);
        if let Command::Fixtures(args) = cli.command {
            assert_eq!(args.surface, Some(SurfaceFilter::Rpc));
            assert_eq!(args.category.as_deref(), Some("network"));
        } else {
            panic!("Expected Fixtures command");
        }
    }
}
