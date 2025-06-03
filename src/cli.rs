use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Main CLI structure for `rust-ff`.
/// This structure will be augmented by subcommands provided by different rules.
#[derive(Parser, Debug)]
#[command(
    name = "rust-ff",
    bin_name = "aff",
    about = "A collection of Rust code analysis tools and fitness functions.",
    version = env!("CARGO_PKG_VERSION"),
    long_about = "rust-ff provides various rules to analyze Rust codebases, such as statement counting and volatility analysis. Use subcommands to select a rule."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Enum representing the available subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Counts statements in Rust components and checks against a threshold.
    StatementCount(StatementCountArgs),
    /// Calculates code volatility for each crate based on Git history.
    Volatility(VolatilityArgs),
}

/// Arguments for the `statement-count` subcommand.
#[derive(Parser, Debug)]
pub struct StatementCountArgs {
    /// Percentage threshold for component size (0-100).
    /// If any component > this percent, exit non-zero.
    #[arg(long, default_value_t = 10)]
    pub threshold: usize,

    /// Path to the 'src' directory to scan (e.g., ./src).
    #[arg(long, default_value = "src")]
    pub src_dir: PathBuf,
}

/// Arguments for the `volatility` subcommand.
#[derive(Parser, Debug)]
pub struct VolatilityArgs {
    /// Weighting factor for lines changed (churn) vs. commit touch count.
    /// E.g., 0.01 means ~100 lines of churn count as much as one commit.
    #[arg(long, default_value_t = 0.01)]
    pub alpha: f64,

    /// Analyze commits since this date (YYYY-MM-DD).
    /// If not provided, analyzes all reachable history from HEAD.
    #[arg(long)]
    pub since: Option<String>,

    /// Normalize volatility scores by the total lines of code in each crate.
    #[arg(long)]
    pub normalize: bool,

    /// Skip merge commits (commits with more than one parent).
    #[arg(long)]
    pub skip_merges: bool,

    /// Path to the Git repository to analyze.
    #[arg(long, default_value = ".")]
    pub repo_path: PathBuf,

    /// Output format for the report.
    #[arg(long, value_parser = ["table", "csv", "json", "yaml"], default_value = "table")]
    pub output: String,
}
