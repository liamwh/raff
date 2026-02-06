//! # Raff - Rust Architecture Fitness Functions
//!
//! Raff is a CLI tool and library for measuring and verifying architectural goals
//! in Rust codebases. It provides fitness functions for:
//!
//! - **Statement Counting**: Measures the size of components by counting statements
//! - **Volatility Analysis**: Tracks how frequently code changes based on git history
//! - **Coupling Analysis**: Measures dependencies between modules and crates
//! - **Contributor Reporting**: Reports on contributor activity across the codebase
//!
//! ## Architecture
//!
//! Raff is organized into several modules:
//!
//! - [`cli`] - Command-line argument parsing and configuration
//! - [`config`] - Configuration file loading and management
//! - [`error`] - Centralized error types for the crate
//! - [`counter`] - AST statement counting utilities
//! - [`file_utils`] - File system operations and path handling
//! - [`statement_count_rule`] - Statement count analysis rule
//! - [`volatility_rule`] - Code volatility analysis based on git history
//! - [`coupling_rule`] - Dependency coupling analysis
//! - [`rust_code_analysis_rule`] - Wrapper for rust-code-analysis
//! - [`contributor_report`] - Contributor activity reporting
//! - [`all_rules`] - Orchestration for running all rules
//! - [`cache`] - Result caching for improved performance
//! - [`ci_report`] - CI/CD platform report generation (SARIF, JUnit)
//! - [`cli_report`] - CLI-friendly table output for terminal consumption
//!
//! ## Usage as a Library
//!
//! Raff can be used as a library to programmatically analyze Rust code:
//!
//! ```rust,no_run
//! use raff_core::{StatementCountRule, Cli, StatementCountArgs, StatementCountOutputFormat};
//! use std::path::PathBuf;
//!
//! # fn main() -> raff_core::error::Result<()> {
//! // Create a new rule instance
//! let rule = StatementCountRule::new();
//!
//! // Configure the analysis
//! let args = StatementCountArgs {
//!     path: PathBuf::from("./src"),
//!     threshold: 1000,
//!     output: StatementCountOutputFormat::Table,
//!     ci_output: None,
//!     output_file: None,
//!     staged: false,
//! };
//!
//! // Run the analysis
//! rule.run(&args)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Configuration
//!
//! Raff supports configuration files via [`RaffConfig`]. See the [`config`]
//! module for details on configuration file format and loading.
//!
//! ## Error Handling
//!
//! All functions that can fail return [`Result<T>`], which is a type alias for
//! `std::result::Result<T, RaffError>`. See the [`error`] module for details on
//! error types and handling.

// Module declarations
pub mod all_rules;
pub mod cache;
pub mod ci_report;
pub mod cli;
pub mod cli_report;
pub mod config;
pub mod config_hierarchy;
pub mod contributor_report;
pub mod counter;
pub mod coupling_rule;
pub mod error;
pub mod file_utils;
pub mod git_utils;
pub mod html_utils;
pub mod reporting;
pub mod rule;
pub mod rust_code_analysis_rule;
pub mod statement_count_rule;
pub mod table_utils;
pub mod volatility_rule;

// Public API exports
pub use crate::all_rules::run_all;
pub use crate::cli::{
    AllArgs, AllOutputFormat, CiOutputFormat, Cli, Commands, ContributorReportArgs,
    ContributorReportOutputFormat, CouplingArgs, CouplingGranularity, CouplingOutputFormat,
    RustCodeAnalysisArgs, RustCodeAnalysisOutputFormat, StatementCountArgs,
    StatementCountOutputFormat, VolatilityArgs, VolatilityOutputFormat,
};
pub use crate::contributor_report::ContributorReportRule;
pub use crate::coupling_rule::CouplingRule;
pub use crate::rust_code_analysis_rule::RustCodeAnalysisRule;
pub use crate::statement_count_rule::StatementCountRule;
pub use crate::volatility_rule::VolatilityRule;

// Config exports
pub use crate::config::{
    apply_pre_commit_profile, load_config, load_config_from_path, merge_all_args,
    merge_contributor_report_args, merge_coupling_args, merge_rust_code_analysis_args,
    merge_statement_count_args, merge_volatility_args, ContributorReportConfig, CouplingConfig,
    GeneralConfig, PreCommitProfile, PreCommitSettings, ProfileConfig, RaffConfig,
    RustCodeAnalysisConfig, StatementCountConfig, VolatilityConfig,
};

// Config hierarchy exports
pub use crate::config_hierarchy::{
    find_git_repo_root, get_repo_local_config_path, get_user_config_path, load_hierarchical_config,
    merge_configs, ConfigSource, ConfigSourceType, HierarchicalConfig, Mergeable,
};

// Cache exports
pub use crate::cache::{CacheEntry, CacheKey, CacheManager};

// Error exports
pub use crate::error::{RaffError as Error, Result};

// Git utils exports
pub use crate::git_utils::{filter_rust_files, get_staged_files};

// CLI report exports
pub use crate::cli_report::render_summary_line;

// Rule trait exports
pub use crate::rule::Rule;

// CI report exports
pub use crate::ci_report::{to_junit, to_sarif, Finding, Location, Severity, ToFindings};
