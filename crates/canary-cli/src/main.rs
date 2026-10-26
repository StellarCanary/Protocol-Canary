//! Entry point for the `stellar-canary` command-line interface.

mod cli;
mod commands;
mod error;
mod fixtures_json;
mod inspect_json;
mod network;

use clap::Parser;

use cli::{CacheArgs, Cli, Command, ConfigArgs, LockCommand};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Command::Check(args) => commands::run_check(args).await,
        Command::Inspect(args) => commands::run_inspect(args),
        Command::Fixtures(args) => commands::run_fixtures(args),
        Command::Report(args) => commands::run_report(args),
        Command::Diff(args) => commands::run_diff(args),
        Command::Cache(CacheArgs::Clear(args)) => commands::run_cache_clear(args),
        Command::Config(ConfigArgs::Validate(args)) => commands::run_config_validate(args),
        Command::Lock(args) => match args.command {
            Some(LockCommand::Verify(verify)) => commands::run_lock_verify(verify),
            None => commands::run_lock(args),
        },
        Command::Completions(args) => commands::run_completions(args),
        Command::Version => commands::run_version(),
    };

    std::process::exit(exit_code.code());
}
