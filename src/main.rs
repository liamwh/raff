//! rust-ff: A collection of Rust code analysis tools and fitness functions.

mod cli;
mod counter;
mod coupling_rule;
mod file_utils;
mod reporting;
mod statement_count_rule;
mod table_utils;
mod volatility_rule;

use anyhow::Result;
use clap::Parser;
use std::process::exit;

use crate::cli::{Cli, Commands};
use crate::coupling_rule::CouplingRule;
use crate::statement_count_rule::StatementCountRule;
use crate::volatility_rule::VolatilityRule;

fn main() -> Result<()> {
    let cli_args = Cli::parse();

    let run_result = match cli_args.command {
        Commands::StatementCount(args) => {
            let rule = StatementCountRule::new();
            rule.run(&args)
        }
        Commands::Volatility(args) => {
            let rule = VolatilityRule::new();
            rule.run(&args)
        }
        Commands::Coupling(args) => {
            let rule = CouplingRule::new();
            rule.run(&args)
        }
    };

    if let Err(e) = run_result {
        eprintln!("Error: {}", e);
        for cause in e.chain().skip(1) {
            eprintln!("  Caused by: {}", cause);
        }
        exit(1);
    }

    Ok(())
}
