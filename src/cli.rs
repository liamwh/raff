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
    /// Perform extended code analysis using rust-code-analysis-cli on all src/ folders.
    RustCodeAnalysis(RustCodeAnalysisArgs),
}

/// Arguments for the `statement-count` subcommand.
#[derive(Args, Debug)]
pub struct StatementCountArgs {
    /// Path to the directory/project to analyze.
    #[clap(long, short, default_value = ".")]
    pub path: std::path::PathBuf,

    /// Percentage threshold for component size (0-100).
    /// If any component > this percent, exit non-zero.
    #[clap(long, default_value_t = 10)]
    pub threshold: usize,
}

/// Arguments for the `volatility` subcommand.
#[derive(Args, Debug)]
pub struct VolatilityArgs {
    /// Path to the Git repository to analyze.
    #[clap(long, short, default_value = ".")]
    pub path: std::path::PathBuf,

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
    /// Path to the directory/project to analyze.
    #[clap(long, short, default_value = ".")]
    pub path: std::path::PathBuf,

    /// Output format for the coupling report.
    #[clap(long, value_enum, default_value_t = CouplingOutputFormat::Table)]
    pub output: CouplingOutputFormat,

    /// Granularity of the coupling report.
    #[clap(long, value_enum, default_value_t = CouplingGranularity::default())]
    pub granularity: CouplingGranularity,
}

/// Output format for the rust-code-analysis subcommand.
#[derive(ValueEnum, Clone, Debug, Default)]
pub enum RustCodeAnalysisOutputFormat {
    #[default] // Make 'table' the default
    Table,
    Json,
    Yaml,
}

/// Arguments for the `rust-code-analysis` subcommand.
#[derive(Args, Debug)]
pub struct RustCodeAnalysisArgs {
    /// Path to the directory/project to analyze.
    #[clap(long, short, default_value = ".")]
    pub path: std::path::PathBuf,

    /// Extra flags to pass directly to rust-code-analysis-cli.
    #[clap(short = 'f', long = "flag", num_args = 0..)]
    pub extra_flags: Vec<String>,

    /// Number of threads to use for analysis.
    #[clap(short, long, default_value_t = num_cpus::get())]
    pub jobs: usize,

    /// Output format for the report.
    #[clap(long, value_enum, default_value_t = RustCodeAnalysisOutputFormat::default())]
    pub output: RustCodeAnalysisOutputFormat,

    /// Enable metrics mode for rust-code-analysis-cli (-m).
    /// Note: This wrapper always requests detailed metrics from the underlying tool for processing.
    #[clap(short, long, default_value_t = true)]
    pub metrics: bool,

    /// Language to analyze.
    /// Note: This wrapper primarily targets Rust analysis.
    #[clap(short = 'l', long, default_value = "rust")]
    pub language: String,
}
