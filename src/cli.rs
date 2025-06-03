use clap::{Args, Parser, Subcommand, ValueEnum};

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
    /// Count statements in Rust components and checks against a threshold.
    StatementCount(StatementCountArgs),
    /// Calculates code volatility for each crate based on Git history.
    Volatility(VolatilityArgs),
    /// Analyzes code coupling between components.
    Coupling(CouplingArgs),
}

/// Arguments for the `statement-count` subcommand.
#[derive(Args, Debug)]
pub struct StatementCountArgs {
    /// Path to the 'src' directory to scan (e.g., ./src).
    #[clap(long, default_value = "src")]
    pub src_dir: std::path::PathBuf,

    /// Percentage threshold for component size (0-100).
    /// If any component > this percent, exit non-zero.
    #[clap(long, default_value_t = 10)]
    pub threshold: usize,
}

/// Arguments for the `volatility` subcommand.
#[derive(Args, Debug)]
pub struct VolatilityArgs {
    /// Weighting factor for lines changed (churn) vs. commit touch count.
    #[clap(long, default_value_t = 0.01)]
    pub alpha: f64,

    /// Analyze commits since this date (YYYY-MM-DD).
    #[clap(long)]
    pub since: Option<String>,

    /// Normalize volatility scores by the total lines of code in each crate.
    #[clap(long)]
    pub normalize: bool,

    /// Skip merge commits (commits with more than one parent).
    #[clap(long)]
    pub skip_merges: bool,

    /// Path to the Git repository to analyze.
    #[clap(long, default_value = ".")]
    pub repo_path: std::path::PathBuf,

    /// Output format for the report.
    #[clap(long, value_parser = ["table", "csv", "json", "yaml"], default_value = "table")]
    pub output: String,
}

/// Enum representing the supported output formats for the coupling report.
#[derive(ValueEnum, Clone, Debug, Default)]
pub enum CouplingOutputFormat {
    #[default] // Make 'table' the default
    Table,
    Json,
    Yaml,
}

/// Defines the granularity level for the coupling report.
#[derive(ValueEnum, Clone, Debug, Default)]
pub enum CouplingGranularity {
    /// Show both crate-level and module-level coupling.
    #[default]
    Both,
    /// Show only crate-level coupling.
    Crate,
    /// Show only module-level coupling.
    Module,
}

/// Arguments for the `coupling` subcommand.
#[derive(Args, Debug)]
pub struct CouplingArgs {
    /// Path to the codebase to analyze.
    #[clap(short, long)]
    pub path: Option<String>,

    /// Output format for the coupling report.
    #[clap(long, value_enum, default_value_t = CouplingOutputFormat::Table)]
    pub output: CouplingOutputFormat,

    /// Granularity of the coupling report.
    #[clap(long, value_enum, default_value_t = CouplingGranularity::default())]
    pub granularity: CouplingGranularity,
}
