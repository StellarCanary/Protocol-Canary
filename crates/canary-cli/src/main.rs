//! Entry point for the `stellar-canary` command-line interface.

mod cli;
mod commands;
mod network;

use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Command::Check(args) => commands::run_check(args).await,
        Command::Inspect(args) => commands::run_inspect(args),
        Command::Fixtures(args) => commands::run_fixtures(args),
        Command::Report(args) => commands::run_report(args),
        Command::Version => commands::run_version(),
    };

    std::process::exit(exit_code.code());
}
