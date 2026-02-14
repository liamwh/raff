//! rust-ff: A collection of Rust code analysis tools and fitness functions.

use clap::Parser;
use raff_core::{
    CacheManager, Cli, Commands, ConfigSourceType, ContributorReportRule, CouplingRule,
    RustCodeAnalysisRule, StatementCountRule, VolatilityRule, all_rules, apply_pre_commit_profile,
    error::Result, load_hierarchical_config, merge_all_args, merge_contributor_report_args,
    merge_coupling_args, merge_rust_code_analysis_args, merge_statement_count_args,
    merge_volatility_args,
};
use std::process::exit;

fn main() -> Result<()> {
    // Initialize color-eyre for better error reporting
    color_eyre::install().map_err(|e| {
        raff_core::error::RaffError::analysis_error(
            "main",
            format!("Failed to install color-eyre: {}", e),
        )
    })?;

    // Initialize tracing subscriber with environment filter
    // Example: RUST_LOG=aff=debug,warn (aff is the binary name)
    // If RUST_LOG is not set, it defaults to "info".
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber).map_err(|e| {
        raff_core::error::RaffError::analysis_error(
            "main",
            format!("Failed to set global default tracing subscriber: {}", e),
        )
    })?;

    let cli_args = Cli::parse();
    tracing::debug!("Parsed CLI arguments: {:?}", cli_args);

    // Handle cache CLI flags
    let mut cache_manager = CacheManager::new()?;
    if cli_args.clear_cache {
        tracing::info!("Clearing cache as requested by --clear-cache flag");
        cache_manager.clear()?;
    }
    if cli_args.no_cache {
        tracing::info!("Caching disabled for this run as requested by --no-cache flag");
        cache_manager.set_enabled(false);
    }

    // Load configuration hierarchically from user, repo, and local sources
    let hierarchical_result = load_hierarchical_config(cli_args.config.as_deref())?;

    // Log all loaded configuration sources
    for source in &hierarchical_result.sources {
        tracing::info!(
            "Loaded {} config from: {}",
            match source.source_type {
                ConfigSourceType::User => "user-level",
                ConfigSourceType::RepoLocal => "repo-local",
                ConfigSourceType::TraditionalLocal => "traditional local",
                ConfigSourceType::CliExplicit => "CLI-specified",
            },
            source.path.display()
        );
    }

    // Apply profile if requested via --profile flag
    let (config, profile_fast, profile_staged, profile_quiet) =
        if cli_args.profile.as_deref() == Some("pre-commit") {
            tracing::info!("Applying pre-commit profile configuration");
            let settings = apply_pre_commit_profile(&hierarchical_result.merged);
            (
                settings.config,
                settings.fast,
                settings.staged,
                settings.quiet,
            )
        } else {
            (hierarchical_result.merged.clone(), false, false, false)
        };

    let run_result = match cli_args.command {
        Commands::StatementCount(mut args) => {
            // Apply profile staged setting
            if profile_staged {
                args.staged = true;
            }
            let mut merged_args = merge_statement_count_args(&args, &config);
            // Propagate global staged flag and profile staged setting
            merged_args.staged = cli_args.staged || profile_staged || args.staged;
            let rule = StatementCountRule::new();
            tracing::info!("Running StatementCount rule with args: {:?}", merged_args);
            rule.run(&merged_args)
        }
        Commands::Volatility(args) => {
            let merged_args = merge_volatility_args(&args, &config);
            let rule = VolatilityRule::new();
            tracing::info!("Running Volatility rule with args: {:?}", merged_args);
            rule.run(&merged_args)
        }
        Commands::Coupling(mut args) => {
            // Apply profile staged setting
            if profile_staged {
                args.staged = true;
            }
            let mut merged_args = merge_coupling_args(&args, &config);
            // Propagate global staged flag and profile staged setting
            merged_args.staged = cli_args.staged || profile_staged || args.staged;
            let rule = CouplingRule::new();
            tracing::info!("Running Coupling rule with args: {:?}", merged_args);
            rule.run(&merged_args)
        }
        Commands::RustCodeAnalysis(args) => {
            let merged_args = merge_rust_code_analysis_args(&args, &config);
            let rule = RustCodeAnalysisRule::new();
            tracing::info!("Running RustCodeAnalysis rule with args: {:?}", merged_args);
            rule.run(&merged_args)
        }
        Commands::All(mut args) => {
            // Apply profile settings if pre-commit profile is active
            if profile_fast {
                args.fast = true;
            }
            if profile_quiet {
                args.quiet = true;
            }
            if profile_staged {
                args.staged = true;
            }
            let mut merged_args = merge_all_args(&args, &config);
            // Propagate global staged flag and profile staged setting
            merged_args.staged = cli_args.staged || profile_staged || args.staged;
            tracing::info!("Running all rules with args: {:?}", merged_args);
            all_rules::run_all(&merged_args)
        }
        Commands::ContributorReport(args) => {
            let merged_args = merge_contributor_report_args(&args, &config);
            let rule = ContributorReportRule::new();
            tracing::info!(
                "Running ContributorReport rule with args: {:?}",
                merged_args
            );
            rule.run(&merged_args)
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
