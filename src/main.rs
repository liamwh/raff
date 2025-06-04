//! rust-ff: A collection of Rust code analysis tools and fitness functions.

mod cli;
mod counter;
mod coupling_rule;
mod file_utils;
mod reporting;
mod rust_code_analysis_rule;
mod statement_count_rule;
mod table_utils;
mod volatility_rule;

use anyhow::Result;
use clap::Parser;
use std::process::exit;

use crate::cli::{Cli, Commands};
use crate::coupling_rule::CouplingRule;
use crate::rust_code_analysis_rule::RustCodeAnalysisRule;
use crate::statement_count_rule::StatementCountRule;
use crate::volatility_rule::VolatilityRule;

fn main() -> Result<()> {
    // Initialize color-eyre for better error reporting
    color_eyre::install().map_err(|e| anyhow::anyhow!("Failed to install color-eyre: {}", e))?;

    // Initialize tracing subscriber with environment filter
    // Example: RUST_LOG=aff=debug,warn (aff is the binary name)
    // If RUST_LOG is not set, it defaults to "info".
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| anyhow::anyhow!("Failed to set global default tracing subscriber: {}", e))?;

    let cli_args = Cli::parse();
    tracing::debug!("Parsed CLI arguments: {:?}", cli_args);

    let run_result = match cli_args.command {
        Commands::StatementCount(args) => {
            let rule = StatementCountRule::new();
            tracing::info!("Running StatementCount rule with args: {:?}", args);
            rule.run(&args)
        }
        Commands::Volatility(args) => {
            let rule = VolatilityRule::new();
            tracing::info!("Running Volatility rule with args: {:?}", args);
            rule.run(&args)
        }
        Commands::Coupling(args) => {
            let rule = CouplingRule::new();
            tracing::info!("Running Coupling rule with args: {:?}", args);
            rule.run(&args)
        }
        Commands::RustCodeAnalysis(args) => {
            let rule = RustCodeAnalysisRule::new();
            tracing::info!("Running RustCodeAnalysis rule with args: {:?}", args);
            rule.run(&args)
        }
    };

    if let Err(e) = run_result {
        // Using color-eyre's report format
        eprintln!("{:?}", e);
        exit(1);
    }

    tracing::info!("Command completed successfully.");
    Ok(())
}
