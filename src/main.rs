//! rust-ff: A collection of Rust code analysis tools and fitness functions.

use anyhow::Result;
use clap::Parser;
use raff_core::{
    all_rules, load_config, Cli, Commands, ContributorReportRule, CouplingRule,
    RustCodeAnalysisRule, StatementCountRule, VolatilityRule,
};
use std::process::exit;

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

    // Load configuration file if specified or discover from default locations
    let config_result = load_config(cli_args.config.as_deref())?;
    if let Some((config_path, _config)) = &config_result {
        tracing::info!("Loaded configuration from: {}", config_path.display());
    }

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
        Commands::All(args) => {
            tracing::info!("Running all rules with args: {:?}", args);
            all_rules::run_all(&args)
        }
        Commands::ContributorReport(args) => {
            let rule = ContributorReportRule::new();
            tracing::info!("Running ContributorReport rule with args: {:?}", args);
            rule.run(&args)
        }
    };

    if let Err(e) = run_result {
        // Using color-eyre's report format
        eprintln!("{e:?}");
        exit(1);
    }

    tracing::info!("Command completed successfully.");
    Ok(())
}
