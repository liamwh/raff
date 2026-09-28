//! Configuration file support for Raff.
//!
//! This module provides functionality to load configuration from TOML files
//! and merge them with command-line arguments. CLI arguments take precedence
//! over config file values.

use crate::error::Result;
use clap::parser::ValueSource;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Default configuration file names to search for.
const DEFAULT_CONFIG_FILES: &[&str] = &["Raff.toml", ".raff.toml", "raff.toml"];

/// Main configuration structure representing a Raff configuration file.
///
/// Configuration files use a merge strategy where:
/// 1. CLI arguments (highest priority)
/// 2. Config file values
/// 3. Default values (lowest priority)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct RaffConfig {
    /// General settings that apply to all commands.
    #[serde(default)]
    pub general: GeneralConfig,

    /// Statement count rule configuration.
    #[serde(default)]
    pub statement_count: StatementCountConfig,

    /// Volatility rule configuration.
    #[serde(default)]
    pub volatility: VolatilityConfig,

    /// Coupling rule configuration.
    #[serde(default)]
    pub coupling: CouplingConfig,

    /// Rust code analysis rule configuration.
    #[serde(default)]
    pub rust_code_analysis: RustCodeAnalysisConfig,

    /// Contributor report configuration.
    #[serde(default)]
    pub contributor_report: ContributorReportConfig,

    /// Profile configurations for different usage scenarios.
    #[serde(default)]
    pub profile: ProfileConfig,
}

/// General configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct GeneralConfig {
    /// Default path to analyze if not specified via CLI.
    pub path: Option<PathBuf>,

    /// Enable verbose output.
    #[serde(default)]
    pub verbose: bool,

    /// Output file path for the report.
    /// When specified, writes output to the file instead of stdout.
    pub output_file: Option<PathBuf>,

    /// Directory patterns to exclude from analysis.
    /// Supports glob patterns (e.g., "target", "node_modules", "**/target").
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
}

fn default_exclude() -> Vec<String> {
    vec!["target".to_string()]
}

/// Statement count rule configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StatementCountConfig {
    /// Default path for statement count analysis.
    pub path: Option<PathBuf>,

    /// Percentage threshold for component size (0-100).
    #[serde(default = "default_statement_count_threshold")]
    pub threshold: usize,

    /// Output format for the report.
    pub output: Option<String>,
}

impl Default for StatementCountConfig {
    fn default() -> Self {
        Self {
            path: None,
            threshold: 10,
            output: None,
        }
    }
}

fn default_statement_count_threshold() -> usize {
    10
}

/// Volatility rule configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VolatilityConfig {
    /// Default path for volatility analysis.
    pub path: Option<PathBuf>,

    /// Weight applied to lines changed (churn) relative to commit touches: raw score = touches + alpha * churn.
    #[serde(default = "default_volatility_alpha")]
    pub alpha: f64,

    /// Analyze commits since this date (YYYY-MM-DD).
    pub since: Option<String>,

    /// Normalize volatility scores by total lines of code.
    #[serde(default)]
    pub normalize: bool,

    /// Skip merge commits.
    #[serde(default)]
    pub skip_merges: bool,

    /// Output format for the report.
    pub output: Option<String>,
}

impl Default for VolatilityConfig {
    fn default() -> Self {
        Self {
            path: None,
            alpha: 0.01,
            since: None,
            normalize: false,
            skip_merges: false,
            output: None,
        }
    }
}

fn default_volatility_alpha() -> f64 {
    0.01
}

/// Coupling rule configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct CouplingConfig {
    /// Default path for coupling analysis.
    pub path: Option<PathBuf>,

    /// Output format for the report.
    pub output: Option<String>,

    /// Granularity of the coupling report.
    pub granularity: Option<String>,
}

/// Rust code analysis rule configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RustCodeAnalysisConfig {
    /// Default path for rust-code-analysis.
    pub path: Option<PathBuf>,

    /// Extra flags to pass directly to rust-code-analysis-cli.
    #[serde(default)]
    pub extra_flags: Vec<String>,

    /// Number of threads to use for analysis.
    pub jobs: Option<usize>,

    /// Output format for the report.
    pub output: Option<String>,

    /// Enable metrics mode.
    #[serde(default = "default_rca_metrics")]
    pub metrics: bool,

    /// Language to analyze.
    #[serde(default = "default_rca_language")]
    pub language: String,
}

impl Default for RustCodeAnalysisConfig {
    fn default() -> Self {
        Self {
            path: None,
            extra_flags: Vec::new(),
            jobs: None,
            output: None,
            metrics: true,
            language: "rust".to_string(),
        }
    }
}

fn default_rca_metrics() -> bool {
    true
}

fn default_rca_language() -> String {
    "rust".to_string()
}

/// Contributor report configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContributorReportConfig {
    /// Default path for contributor report.
    pub path: Option<PathBuf>,

    /// Analyze commits since this date (YYYY-MM-DD).
    pub since: Option<String>,

    /// Exponential decay factor for recency weighting.
    #[serde(default = "default_contributor_decay")]
    pub decay: f64,

    /// Output format for the report.
    pub output: Option<String>,
}

/// Profile configuration for different usage scenarios.
///
/// Profiles allow pre-configured sets of options for common use cases,
/// such as pre-commit hooks which need fast, minimal analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct ProfileConfig {
    /// Pre-commit hook profile configuration.
    #[serde(default)]
    pub pre_commit: Option<PreCommitProfile>,
}

/// Pre-commit hook profile configuration.
///
/// This profile is optimized for use in pre-commit hooks, where
/// speed and minimal output are important.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct PreCommitProfile {
    /// Run only fast rules: statement count and coupling, or coupling alone when `staged`.
    #[serde(default)]
    pub fast: Option<bool>,

    /// Analyze only git-staged files.
    #[serde(default)]
    pub staged: Option<bool>,

    /// Minimal output (summary line only).
    #[serde(default)]
    pub quiet: Option<bool>,

    /// Statement count threshold. Unused while `fast` and `staged` are set, as the
    /// profile then skips statement count.
    #[serde(default)]
    pub sc_threshold: Option<usize>,
}

impl Default for ContributorReportConfig {
    fn default() -> Self {
        Self {
            path: None,
            since: None,
            decay: 0.01,
            output: None,
        }
    }
}

fn default_contributor_decay() -> f64 {
    0.01
}

/// Pre-commit profile runtime settings.
///
/// This struct contains both the modified configuration and runtime flags
/// that should be applied when using the pre-commit profile.
#[derive(Debug, Clone)]
pub struct PreCommitSettings {
    /// The modified configuration with profile settings applied.
    pub config: RaffConfig,
    /// Run only fast rules: statement count and coupling, or coupling alone when `staged`.
    pub fast: bool,
    /// Analyze only git-staged files.
    pub staged: bool,
    /// Minimal output (summary line only).
    pub quiet: bool,
}

/// Apply the pre-commit profile configuration to a merged config.
///
/// This function applies the pre-commit profile settings from the configuration
/// to create a modified config with profile defaults, and returns runtime flags
/// that should be applied. CLI flags can still override these values.
///
/// # Arguments
///
/// * `config` - The merged configuration to apply the profile to
///
/// # Returns
///
/// A `PreCommitSettings` struct containing the modified config and runtime flags.
/// If no pre-commit profile is defined, returns the config unchanged with all
/// runtime flags set to false.
///
/// # Examples
///
/// ```
/// use raff_core::config::{RaffConfig, apply_pre_commit_profile};
///
/// let config = RaffConfig::default();
/// let settings = apply_pre_commit_profile(&config);
/// ```
#[must_use]
pub fn apply_pre_commit_profile(config: &RaffConfig) -> PreCommitSettings {
    let default_fast = true;
    let default_staged = true;
    let default_quiet = true;
    let default_sc_threshold = 25;
    let pc = config.profile.pre_commit.as_ref();

    let mut result = config.clone();

    // Apply statement count threshold from profile if set
    result.statement_count.threshold = pc
        .and_then(|profile| profile.sc_threshold)
        .unwrap_or(default_sc_threshold);

    // Return profile settings for runtime behavior
    PreCommitSettings {
        config: result,
        fast: pc.and_then(|profile| profile.fast).unwrap_or(default_fast),
        staged: pc
            .and_then(|profile| profile.staged)
            .unwrap_or(default_staged),
        quiet: pc
            .and_then(|profile| profile.quiet)
            .unwrap_or(default_quiet),
    }
}

/// Load configuration from a specific file path.
///
/// # Arguments
///
/// * `path` - Path to the configuration file.
///
/// # Returns
///
/// Returns a `RaffConfig` if the file exists and can be parsed.
/// Returns `Ok(None)` if the file doesn't exist.
/// Returns an error if the file exists but cannot be parsed.
pub fn load_config_from_path(path: &Path) -> Result<Option<RaffConfig>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;

    let config: RaffConfig = toml::from_str(&content)?;

    Ok(Some(config))
}

/// Discover and load configuration from default locations.
///
/// Searches for configuration files in the current directory and parent directories,
/// using the default config file names: `Raff.toml`, `.raff.toml`, `raff.toml`.
///
/// Also checks for `.raff/raff.toml` at the git repository root.
///
/// # Returns
///
/// Returns `Some(RaffConfig)` if a config file is found and can be parsed.
/// Returns `None` if no config file is found.
pub fn discover_and_load_config() -> Result<Option<(PathBuf, RaffConfig)>> {
    let mut current_dir = std::env::current_dir()?;

    // Search up the directory tree for a config file
    loop {
        for config_name in DEFAULT_CONFIG_FILES {
            let config_path = current_dir.join(config_name);
            if let Some(config) = load_config_from_path(&config_path)? {
                return Ok(Some((config_path, config)));
            }
        }

        // Move to parent directory
        if !current_dir.pop() {
            // Reached the root without finding a config file
            break;
        }
    }

    // Try to find .raff/raff.toml at git repository root
    if let Ok(Some(repo_root)) = crate::git_utils::get_repo_root() {
        let raff_config_path = repo_root.join(".raff").join("raff.toml");
        if let Some(config) = load_config_from_path(&raff_config_path)? {
            return Ok(Some((raff_config_path, config)));
        }
    }

    Ok(None)
}

/// Load configuration from a specified path or discover from default locations.
///
/// If `config_path` is `Some`, loads from that specific path.
/// If `config_path` is `None`, searches for default config files.
///
/// # Arguments
///
/// * `config_path` - Optional path to a specific configuration file.
///
/// # Returns
///
/// Returns `Some((PathBuf, RaffConfig))` if a config file is found.
/// Returns `None` if no config file is found.
pub fn load_config(config_path: Option<&Path>) -> Result<Option<(PathBuf, RaffConfig)>> {
    if let Some(path) = config_path {
        load_config_from_path(path).map(|opt| opt.map(|config| (path.to_path_buf(), config)))
    } else {
        discover_and_load_config()
    }
}

/// The subcommand arguments the user passed on the command line.
///
/// Merging uses this to tell an explicitly passed value apart from a clap default,
/// even when both are equal (e.g. `--threshold 10`).
#[derive(Debug, Default, Clone)]
pub struct ExplicitCliArgs(HashSet<String>);

impl ExplicitCliArgs {
    /// Collects the arguments of the invoked subcommand (or of `matches` itself when
    /// there is none) whose value did not come from a clap default.
    pub fn from_matches(matches: &clap::ArgMatches) -> Self {
        let matches = matches.subcommand().map_or(matches, |(_, sub)| sub);
        Self(
            matches
                .ids()
                .filter(|id| {
                    matches
                        .value_source(id.as_str())
                        .is_some_and(|source| source != ValueSource::DefaultValue)
                })
                .map(|id| id.as_str().to_owned())
                .collect(),
        )
    }

    /// Whether the argument with the given clap id was passed explicitly.
    pub fn contains(&self, id: &str) -> bool {
        self.0.contains(id)
    }
}

/// Merge statement count CLI args with config file values.
///
/// Priority order:
/// 1. CLI arguments passed explicitly (highest priority)
/// 2. Config file values
/// 3. Default values (lowest priority)
pub fn merge_statement_count_args(
    cli_args: &crate::cli::StatementCountArgs,
    config: &RaffConfig,
    explicit: &ExplicitCliArgs,
) -> crate::cli::StatementCountArgs {
    let mut merged = cli_args.clone();

    if !explicit.contains("path")
        && let Some(config_path) = &config.statement_count.path
    {
        merged.path = config_path.clone();
    }

    // The config default equals the CLI default, so the config value always applies.
    if !explicit.contains("threshold") {
        merged.threshold = config.statement_count.threshold;
    }

    if !explicit.contains("output")
        && let Some(config_output) = &config.statement_count.output
    {
        merged.output = parse_statement_count_output_format(config_output)
            .unwrap_or(crate::cli::StatementCountOutputFormat::Table);
    }

    // Merge output_file: CLI arg OR general config output_file
    if merged.output_file.is_none() {
        merged.output_file = config.general.output_file.clone();
    }

    merged
}

/// Parse output format string for statement count.
fn parse_statement_count_output_format(s: &str) -> Option<crate::cli::StatementCountOutputFormat> {
    match s.to_lowercase().as_str() {
        "table" => Some(crate::cli::StatementCountOutputFormat::Table),
        "html" => Some(crate::cli::StatementCountOutputFormat::Html),
        _ => None,
    }
}

/// Merge volatility CLI args with config file values.
pub fn merge_volatility_args(
    cli_args: &crate::cli::VolatilityArgs,
    config: &RaffConfig,
    explicit: &ExplicitCliArgs,
) -> crate::cli::VolatilityArgs {
    let mut merged = cli_args.clone();

    if !explicit.contains("path")
        && let Some(config_path) = &config.volatility.path
    {
        merged.path = config_path.clone();
    }

    // The config default equals the CLI default, so the config value always applies.
    if !explicit.contains("alpha") {
        merged.alpha = config.volatility.alpha;
    }

    // Merge since: optional, use CLI if set, otherwise config
    if merged.since.is_none() {
        merged.since = config.volatility.since.clone();
    }

    // Merge normalize: CLI default is false
    if config.volatility.normalize && !merged.normalize {
        merged.normalize = true;
    }

    // Merge skip_merges: CLI default is false
    if config.volatility.skip_merges && !merged.skip_merges {
        merged.skip_merges = true;
    }

    if !explicit.contains("output")
        && let Some(config_output) = &config.volatility.output
    {
        merged.output = parse_volatility_output_format(config_output)
            .unwrap_or(crate::cli::VolatilityOutputFormat::Table);
    }

    // Merge output_file: CLI takes precedence if set, otherwise use config
    if merged.output_file.is_none() {
        merged.output_file = config.general.output_file.clone();
    }

    merged
}

/// Parse output format string for volatility.
fn parse_volatility_output_format(s: &str) -> Option<crate::cli::VolatilityOutputFormat> {
    match s.to_lowercase().as_str() {
        "table" => Some(crate::cli::VolatilityOutputFormat::Table),
        "csv" => Some(crate::cli::VolatilityOutputFormat::Csv),
        "json" => Some(crate::cli::VolatilityOutputFormat::Json),
        "yaml" => Some(crate::cli::VolatilityOutputFormat::Yaml),
        "html" => Some(crate::cli::VolatilityOutputFormat::Html),
        _ => None,
    }
}

/// Merge coupling CLI args with config file values.
pub fn merge_coupling_args(
    cli_args: &crate::cli::CouplingArgs,
    config: &RaffConfig,
    explicit: &ExplicitCliArgs,
) -> crate::cli::CouplingArgs {
    let mut merged = cli_args.clone();

    if !explicit.contains("path")
        && let Some(config_path) = &config.coupling.path
    {
        merged.path = config_path.clone();
    }

    if !explicit.contains("output")
        && let Some(config_output) = &config.coupling.output
    {
        merged.output = parse_coupling_output_format(config_output)
            .unwrap_or(crate::cli::CouplingOutputFormat::Table);
    }

    if !explicit.contains("granularity")
        && let Some(config_granularity) = &config.coupling.granularity
    {
        merged.granularity = parse_coupling_granularity(config_granularity)
            .unwrap_or(crate::cli::CouplingGranularity::Both);
    }

    // Merge output_file: CLI takes precedence if set, otherwise use config
    if merged.output_file.is_none() {
        merged.output_file = config.general.output_file.clone();
    }

    merged
}

/// Parse output format string for coupling.
fn parse_coupling_output_format(s: &str) -> Option<crate::cli::CouplingOutputFormat> {
    match s.to_lowercase().as_str() {
        "table" => Some(crate::cli::CouplingOutputFormat::Table),
        "json" => Some(crate::cli::CouplingOutputFormat::Json),
        "yaml" => Some(crate::cli::CouplingOutputFormat::Yaml),
        "html" => Some(crate::cli::CouplingOutputFormat::Html),
        "dot" => Some(crate::cli::CouplingOutputFormat::Dot),
        _ => None,
    }
}

/// Parse granularity string for coupling.
fn parse_coupling_granularity(s: &str) -> Option<crate::cli::CouplingGranularity> {
    match s.to_lowercase().as_str() {
        "both" => Some(crate::cli::CouplingGranularity::Both),
        "crate" => Some(crate::cli::CouplingGranularity::Crate),
        "module" => Some(crate::cli::CouplingGranularity::Module),
        _ => None,
    }
}

/// Merge rust-code-analysis CLI args with config file values.
pub fn merge_rust_code_analysis_args(
    cli_args: &crate::cli::RustCodeAnalysisArgs,
    config: &RaffConfig,
    explicit: &ExplicitCliArgs,
) -> crate::cli::RustCodeAnalysisArgs {
    let mut merged = cli_args.clone();

    if !explicit.contains("path")
        && let Some(config_path) = &config.rust_code_analysis.path
    {
        merged.path = config_path.clone();
    }

    // Merge extra_flags: CLI flags should append to config, not replace
    if !config.rust_code_analysis.extra_flags.is_empty() {
        let mut combined_flags = config.rust_code_analysis.extra_flags.clone();
        combined_flags.extend(merged.extra_flags.clone());
        merged.extra_flags = combined_flags;
    }

    if !explicit.contains("jobs")
        && let Some(config_jobs) = config.rust_code_analysis.jobs
    {
        merged.jobs = config_jobs;
    }

    if !explicit.contains("output")
        && let Some(config_output) = &config.rust_code_analysis.output
    {
        merged.output = parse_rca_output_format(config_output)
            .unwrap_or(crate::cli::RustCodeAnalysisOutputFormat::Table);
    }

    // The config defaults equal the CLI defaults, so the config values always apply.
    if !explicit.contains("metrics") {
        merged.metrics = config.rust_code_analysis.metrics;
    }
    if !explicit.contains("language") {
        merged.language = config.rust_code_analysis.language.clone();
    }

    // Merge output_file: Use general.output_file if CLI arg is not set
    if merged.output_file.is_none() {
        merged.output_file = config.general.output_file.clone();
    }

    merged
}

/// Parse output format string for rust-code-analysis.
fn parse_rca_output_format(s: &str) -> Option<crate::cli::RustCodeAnalysisOutputFormat> {
    match s.to_lowercase().as_str() {
        "table" => Some(crate::cli::RustCodeAnalysisOutputFormat::Table),
        "json" => Some(crate::cli::RustCodeAnalysisOutputFormat::Json),
        "yaml" => Some(crate::cli::RustCodeAnalysisOutputFormat::Yaml),
        "html" => Some(crate::cli::RustCodeAnalysisOutputFormat::Html),
        _ => None,
    }
}

/// Merge contributor-report CLI args with config file values.
pub fn merge_contributor_report_args(
    cli_args: &crate::cli::ContributorReportArgs,
    config: &RaffConfig,
    explicit: &ExplicitCliArgs,
) -> crate::cli::ContributorReportArgs {
    let mut merged = cli_args.clone();

    if !explicit.contains("path")
        && let Some(config_path) = &config.contributor_report.path
    {
        merged.path = config_path.clone();
    }

    // Merge since: optional
    if merged.since.is_none() {
        merged.since = config.contributor_report.since.clone();
    }

    // The config default equals the CLI default, so the config value always applies.
    if !explicit.contains("decay") {
        merged.decay = config.contributor_report.decay;
    }

    if !explicit.contains("output")
        && let Some(config_output) = &config.contributor_report.output
    {
        merged.output = parse_contributor_report_output_format(config_output)
            .unwrap_or(crate::cli::ContributorReportOutputFormat::Table);
    }

    // Merge output_file: from general config if not set on CLI
    if merged.output_file.is_none() {
        merged.output_file = config.general.output_file.clone();
    }

    merged
}

/// Parse output format string for contributor report.
fn parse_contributor_report_output_format(
    s: &str,
) -> Option<crate::cli::ContributorReportOutputFormat> {
    match s.to_lowercase().as_str() {
        "table" => Some(crate::cli::ContributorReportOutputFormat::Table),
        "html" => Some(crate::cli::ContributorReportOutputFormat::Html),
        "json" => Some(crate::cli::ContributorReportOutputFormat::Json),
        "yaml" => Some(crate::cli::ContributorReportOutputFormat::Yaml),
        _ => None,
    }
}

/// Merge all-rules CLI args with config file values.
///
/// This merges into each sub-command's config section.
pub fn merge_all_args(
    cli_args: &crate::cli::AllArgs,
    config: &RaffConfig,
    explicit: &ExplicitCliArgs,
) -> crate::cli::AllArgs {
    let mut merged = cli_args.clone();

    if !explicit.contains("path") {
        // Check all config paths, use general path as fallback
        let config_path = config
            .general
            .path
            .as_ref()
            .or(config.statement_count.path.as_ref())
            .or(config.volatility.path.as_ref())
            .or(config.coupling.path.as_ref())
            .or(config.rust_code_analysis.path.as_ref())
            .or(config.contributor_report.path.as_ref());
        if let Some(cp) = config_path {
            merged.path = cp.clone();
        }
    }

    // Config defaults equal the CLI defaults, so config values always apply to
    // arguments that were not passed explicitly.
    if !explicit.contains("sc_threshold") {
        merged.sc_threshold = config.statement_count.threshold;
    }
    if !explicit.contains("vol_alpha") {
        merged.vol_alpha = config.volatility.alpha;
    }

    // Merge volatility since
    if merged.vol_since.is_none() {
        merged.vol_since = config.volatility.since.clone();
    }

    // Merge volatility normalize
    if config.volatility.normalize && !merged.vol_normalize {
        merged.vol_normalize = true;
    }

    // Merge volatility skip_merges
    if config.volatility.skip_merges && !merged.vol_skip_merges {
        merged.vol_skip_merges = true;
    }

    if !explicit.contains("coup_granularity")
        && let Some(config_granularity) = &config.coupling.granularity
    {
        merged.coup_granularity = parse_coupling_granularity(config_granularity)
            .unwrap_or(crate::cli::CouplingGranularity::Both);
    }

    // Merge RCA extra_flags
    if !config.rust_code_analysis.extra_flags.is_empty() {
        let mut combined_flags = config.rust_code_analysis.extra_flags.clone();
        combined_flags.extend(merged.rca_extra_flags.clone());
        merged.rca_extra_flags = combined_flags;
    }

    if !explicit.contains("rca_jobs")
        && let Some(config_jobs) = config.rust_code_analysis.jobs
    {
        merged.rca_jobs = config_jobs;
    }
    if !explicit.contains("rca_metrics") {
        merged.rca_metrics = config.rust_code_analysis.metrics;
    }
    if !explicit.contains("rca_language") {
        merged.rca_language = config.rust_code_analysis.language.clone();
    }

    // Merge output_file: Use general.output_file if CLI arg is not set
    if merged.output_file.is_none() {
        merged.output_file = config.general.output_file.clone();
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;
    use std::process::Command;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    /// Helper to create a temporary config file with given content
    fn create_temp_config_file(content: &str) -> NamedTempFile {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        temp_file
            .write_all(content.as_bytes())
            .expect("Failed to write to temp file");
        temp_file
    }

    #[test]
    fn test_raft_config_default_creates_valid_config() {
        let config = RaffConfig::default();

        assert!(!config.general.verbose, "verbose should default to false");
        assert_eq!(
            config.statement_count.threshold, 10,
            "statement_count threshold should default to 10"
        );
        assert_eq!(
            config.volatility.alpha, 0.01,
            "volatility alpha should default to 0.01"
        );
    }

    #[test]
    fn test_general_config_default() {
        let config = GeneralConfig::default();

        assert!(config.path.is_none(), "path should be None by default");
        assert!(!config.verbose, "verbose should be false by default");
    }

    #[test]
    fn test_statement_count_config_default() {
        let config = StatementCountConfig::default();

        assert!(config.path.is_none(), "path should be None by default");
        assert_eq!(config.threshold, 10, "threshold should default to 10");
        assert!(config.output.is_none(), "output should be None by default");
    }

    #[test]
    fn test_volatility_config_default() {
        let config = VolatilityConfig::default();

        assert!(config.path.is_none(), "path should be None by default");
        assert_eq!(config.alpha, 0.01, "alpha should default to 0.01");
        assert!(config.since.is_none(), "since should be None by default");
        assert!(!config.normalize, "normalize should default to false");
        assert!(!config.skip_merges, "skip_merges should default to false");
        assert!(config.output.is_none(), "output should be None by default");
    }

    #[test]
    fn test_load_config_from_valid_toml_file() {
        let content = r#"
[general]
verbose = true

[statement_count]
threshold = 25

[volatility]
alpha = 0.05
since = "2024-01-01"
normalize = true
"#;
        let temp_file = create_temp_config_file(content);

        let result = load_config_from_path(temp_file.path());

        assert!(
            result.is_ok(),
            "load_config_from_path should succeed with valid TOML"
        );

        let config = result.unwrap().expect("config should be present");
        assert!(config.general.verbose, "verbose should be true");
        assert_eq!(
            config.statement_count.threshold, 25,
            "threshold should be 25"
        );
        assert_eq!(config.volatility.alpha, 0.05, "alpha should be 0.05");
        assert_eq!(
            config.volatility.since.as_ref().unwrap(),
            "2024-01-01",
            "since should be 2024-01-01"
        );
        assert!(config.volatility.normalize, "normalize should be true");
    }

    #[test]
    fn test_load_config_from_nonexistent_file_returns_none() {
        let fake_path = PathBuf::from("/nonexistent/path/to/config.toml");

        let result = load_config_from_path(&fake_path);

        assert!(
            result.is_ok(),
            "load_config_from_path should not error for nonexistent file"
        );
        assert!(
            result.unwrap().is_none(),
            "should return None for nonexistent file"
        );
    }

    #[test]
    fn test_load_config_from_invalid_toml_file_fails() {
        let content = r#"
[general
verbose = true
"#; // Missing closing bracket
        let temp_file = create_temp_config_file(content);

        let result = load_config_from_path(temp_file.path());

        assert!(
            result.is_err(),
            "load_config_from_path should fail with invalid TOML"
        );
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("parse") || error_msg.contains("Failed to parse"),
            "error message should mention parsing failure"
        );
    }

    #[test]
    fn test_load_config_with_all_sections() {
        let content = r#"
[general]
verbose = true

[statement_count]
threshold = 30

[volatility]
alpha = 0.02
since = "2024-01-01"
normalize = true
skip_merges = true

[coupling]
granularity = "crate"

[rust_code_analysis]
jobs = 8
language = "rust"

[contributor_report]
decay = 0.05
"#;
        let temp_file = create_temp_config_file(content);

        let result = load_config_from_path(temp_file.path());

        assert!(result.is_ok(), "load should succeed");

        let config = result.unwrap().expect("config should be present");
        assert!(config.general.verbose);
        assert_eq!(config.statement_count.threshold, 30);
        assert_eq!(config.volatility.alpha, 0.02);
        assert!(config.volatility.skip_merges);
        assert_eq!(config.coupling.granularity.as_ref().unwrap(), "crate");
        assert_eq!(config.rust_code_analysis.jobs.unwrap(), 8);
        assert_eq!(config.contributor_report.decay, 0.05);
    }

    #[test]
    #[serial(cwd)]
    fn test_discover_and_load_config_finds_raff_toml() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join("Raff.toml");

        let content = r#"
[general]
verbose = true
"#;
        fs::write(&config_path, content).expect("Failed to write config file");

        // Use a known valid path since current_dir() might fail if previous test deleted its temp dir
        let fallback_path = std::env::var("HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                tempfile::TempDir::new()
                    .ok()
                    .map(|d| d.path().to_path_buf())
            })
            .expect("Failed to get fallback path");
        let original_path = std::env::current_dir().unwrap_or(fallback_path.clone());

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory - use fallback if original path no longer exists
        let _ = std::env::set_current_dir(&original_path)
            .or_else(|_| std::env::set_current_dir(&fallback_path));

        assert!(result.is_ok(), "discover_and_load_config should succeed");

        let (path, config) = result.unwrap().expect("should find config file");
        // Use file_name() to compare just the filename, avoiding symlink issues
        assert_eq!(
            path.file_name(),
            config_path.file_name(),
            "should return correct config file name"
        );
        assert!(config.general.verbose, "verbose should be true");
    }

    #[test]
    #[serial(cwd)]
    fn test_discover_and_load_config_finds_dot_raff_toml() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join(".raff.toml");

        let content = r#"
[statement_count]
threshold = 50
"#;
        fs::write(&config_path, content).expect("Failed to write config file");

        // Use a known valid path since current_dir() might fail if previous test deleted its temp dir
        let fallback_path = std::env::var("HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                tempfile::TempDir::new()
                    .ok()
                    .map(|d| d.path().to_path_buf())
            })
            .expect("Failed to get fallback path");
        let original_path = std::env::current_dir().unwrap_or(fallback_path.clone());

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory - use fallback if original path no longer exists
        let _ = std::env::set_current_dir(&original_path)
            .or_else(|_| std::env::set_current_dir(&fallback_path));

        assert!(result.is_ok(), "discover_and_load_config should succeed");

        let (_path, config) = result.unwrap().expect("should find config file");
        assert_eq!(
            config.statement_count.threshold, 50,
            "should load config from .raff.toml"
        );
    }

    #[test]
    #[serial(cwd)]
    fn test_discover_and_load_config_returns_none_when_no_config() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Use a known valid path since current_dir() might fail if previous test deleted its temp dir
        let fallback_path = std::env::var("HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                tempfile::TempDir::new()
                    .ok()
                    .map(|d| d.path().to_path_buf())
            })
            .expect("Failed to get fallback path");
        let original_path = std::env::current_dir().unwrap_or(fallback_path.clone());

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory - use fallback if original path no longer exists
        let _ = std::env::set_current_dir(&original_path)
            .or_else(|_| std::env::set_current_dir(&fallback_path));

        assert!(result.is_ok(), "discover_and_load_config should succeed");
        assert!(
            result.unwrap().is_none(),
            "should return None when no config file exists"
        );
    }

    #[test]
    fn test_load_config_with_specific_path() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join("custom-config.toml");

        let content = r#"
[volatility]
alpha = 0.1
"#;
        fs::write(&config_path, content).expect("Failed to write config file");

        let result = load_config(Some(&config_path));

        assert!(result.is_ok(), "load_config should succeed");

        let (_path, config) = result.unwrap().expect("should load config");
        assert_eq!(
            config.volatility.alpha, 0.1,
            "should load config from specified path"
        );
    }

    #[test]
    #[serial(cwd)]
    fn test_load_config_with_none_path_discovers_config() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join("Raff.toml");

        let content = r#"
[general]
verbose = true
"#;
        fs::write(&config_path, content).expect("Failed to write config file");

        // Use a known valid path (home directory or a temp location)
        // since current_dir() might fail if previous test deleted its temp dir
        let fallback_path = std::env::var("HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                tempfile::TempDir::new()
                    .ok()
                    .map(|d| d.path().to_path_buf())
            })
            .expect("Failed to get fallback path");
        let original_path = std::env::current_dir().unwrap_or(fallback_path.clone());

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = load_config(None);

        // Restore directory - use fallback if original path no longer exists
        let _ = std::env::set_current_dir(&original_path)
            .or_else(|_| std::env::set_current_dir(&fallback_path));

        assert!(result.is_ok(), "load_config should succeed");
        assert!(
            result.unwrap().is_some(),
            "should discover config when path is None"
        );
    }

    #[test]
    fn test_coupling_config_default() {
        let config = CouplingConfig::default();

        assert!(config.path.is_none(), "path should be None by default");
        assert!(config.output.is_none(), "output should be None by default");
        assert!(
            config.granularity.is_none(),
            "granularity should be None by default"
        );
    }

    #[test]
    fn test_rust_code_analysis_config_default() {
        let config = RustCodeAnalysisConfig::default();

        assert!(config.path.is_none(), "path should be None by default");
        assert!(
            config.extra_flags.is_empty(),
            "extra_flags should be empty by default"
        );
        assert!(config.jobs.is_none(), "jobs should be None by default");
        assert!(config.output.is_none(), "output should be None by default");
        assert!(config.metrics, "metrics should default to true");
        assert_eq!(config.language, "rust", "language should default to rust");
    }

    #[test]
    fn test_contributor_report_config_default() {
        let config = ContributorReportConfig::default();

        assert!(config.path.is_none(), "path should be None by default");
        assert!(config.since.is_none(), "since should be None by default");
        assert_eq!(config.decay, 0.01, "decay should default to 0.01");
        assert!(config.output.is_none(), "output should be None by default");
    }

    #[test]
    fn test_config_is_serializable() {
        let config = RaffConfig::default();

        let toml_str = toml::to_string(&config).expect("RaffConfig should be serializable");
        assert!(!toml_str.is_empty(), "serialized TOML should not be empty");
    }

    // Tests for merge functions

    /// Parses `argv` exactly as the binary does and returns the subcommand args
    /// together with the arguments passed explicitly.
    fn parse_cli(argv: &[&str]) -> (crate::cli::Commands, ExplicitCliArgs) {
        use clap::{CommandFactory, FromArgMatches};
        let matches = crate::cli::Cli::command()
            .try_get_matches_from(argv)
            .expect("argv should parse");
        let cli = crate::cli::Cli::from_arg_matches(&matches).expect("matches should convert");
        (cli.command, ExplicitCliArgs::from_matches(&matches))
    }

    fn merged_statement_count(
        argv: &[&str],
        config: &RaffConfig,
    ) -> crate::cli::StatementCountArgs {
        let (crate::cli::Commands::StatementCount(args), explicit) = parse_cli(argv) else {
            panic!("expected statement-count");
        };
        merge_statement_count_args(&args, config, &explicit)
    }

    fn merged_volatility(argv: &[&str], config: &RaffConfig) -> crate::cli::VolatilityArgs {
        let (crate::cli::Commands::Volatility(args), explicit) = parse_cli(argv) else {
            panic!("expected volatility");
        };
        merge_volatility_args(&args, config, &explicit)
    }

    fn merged_all(argv: &[&str], config: &RaffConfig) -> crate::cli::AllArgs {
        let (crate::cli::Commands::All(args), explicit) = parse_cli(argv) else {
            panic!("expected all");
        };
        merge_all_args(&args, config, &explicit)
    }

    #[test]
    fn test_merge_statement_count_threshold_precedence() {
        let mut config = RaffConfig::default();
        config.statement_count.threshold = 15;

        let cli_beats_config =
            merged_statement_count(&["raff", "statement-count", "--threshold", "100"], &config);
        assert_eq!(cli_beats_config.threshold, 100);

        let config_beats_default = merged_statement_count(&["raff", "statement-count"], &config);
        assert_eq!(config_beats_default.threshold, 15);

        let default_only =
            merged_statement_count(&["raff", "statement-count"], &RaffConfig::default());
        assert_eq!(default_only.threshold, 10);

        // An explicit value equal to the built-in default still beats the config.
        let explicit_default =
            merged_statement_count(&["raff", "statement-count", "--threshold", "10"], &config);
        assert_eq!(explicit_default.threshold, 10);
    }

    #[test]
    fn test_merge_statement_count_args_with_config_values() {
        let mut config = RaffConfig::default();
        config.statement_count.threshold = 25;
        config.statement_count.path = Some(PathBuf::from("/custom/path"));
        config.statement_count.output = Some("html".to_string());

        let merged = merged_statement_count(&["raff", "statement-count"], &config);

        assert_eq!(merged.path, PathBuf::from("/custom/path"));
        assert_eq!(merged.threshold, 25);
        assert!(matches!(
            merged.output,
            crate::cli::StatementCountOutputFormat::Html
        ));
    }

    #[test]
    fn test_merge_statement_count_args_cli_overrides_config() {
        let mut config = RaffConfig::default();
        config.statement_count.threshold = 25;
        config.statement_count.path = Some(PathBuf::from("/custom/path"));
        config.statement_count.output = Some("html".to_string());

        let merged = merged_statement_count(
            &[
                "raff",
                "statement-count",
                "--path",
                ".",
                "--threshold",
                "50",
                "--output",
                "table",
            ],
            &config,
        );

        assert_eq!(merged.path, PathBuf::from("."));
        assert_eq!(merged.threshold, 50);
        assert!(matches!(
            merged.output,
            crate::cli::StatementCountOutputFormat::Table
        ));
    }

    #[test]
    fn test_merge_volatility_args_with_config_values() {
        let mut config = RaffConfig::default();
        config.volatility.alpha = 0.05;
        config.volatility.since = Some("2024-01-01".to_string());
        config.volatility.normalize = true;
        config.volatility.skip_merges = true;
        config.volatility.output = Some("csv".to_string());

        let merged = merged_volatility(&["raff", "volatility"], &config);

        assert_eq!(merged.alpha, 0.05);
        assert_eq!(merged.since, Some("2024-01-01".to_string()));
        assert!(merged.normalize);
        assert!(merged.skip_merges);
        assert!(matches!(
            merged.output,
            crate::cli::VolatilityOutputFormat::Csv
        ));
    }

    #[test]
    fn test_merge_volatility_args_cli_overrides_config() {
        let mut config = RaffConfig::default();
        config.volatility.alpha = 0.05;
        config.volatility.since = Some("2024-01-01".to_string());
        config.volatility.output = Some("csv".to_string());

        let merged = merged_volatility(
            &[
                "raff",
                "volatility",
                "--alpha",
                "0.01",
                "--since",
                "2023-01-01",
                "--output",
                "table",
            ],
            &config,
        );

        assert_eq!(merged.alpha, 0.01);
        assert_eq!(merged.since, Some("2023-01-01".to_string()));
        assert!(matches!(
            merged.output,
            crate::cli::VolatilityOutputFormat::Table
        ));
    }

    #[test]
    fn test_merge_coupling_args_cli_granularity_overrides_config() {
        let mut config = RaffConfig::default();
        config.coupling.granularity = Some("module".to_string());
        config.coupling.output = Some("json".to_string());
        let (crate::cli::Commands::Coupling(args), explicit) =
            parse_cli(&["raff", "coupling", "--granularity", "both"])
        else {
            panic!("expected coupling");
        };

        let merged = merge_coupling_args(&args, &config, &explicit);

        assert!(matches!(
            merged.granularity,
            crate::cli::CouplingGranularity::Both
        ));
        assert!(matches!(
            merged.output,
            crate::cli::CouplingOutputFormat::Json
        ));
    }

    #[test]
    fn test_merge_rust_code_analysis_args_with_config_values() {
        let mut config = RaffConfig::default();
        config.rust_code_analysis.extra_flags = vec!["--flag1".to_string(), "--flag2".to_string()];
        config.rust_code_analysis.jobs = Some(4);
        config.rust_code_analysis.metrics = false;
        config.rust_code_analysis.language = "python".to_string();
        let (crate::cli::Commands::RustCodeAnalysis(args), explicit) = parse_cli(&[
            "raff",
            "rust-code-analysis",
            "-f",
            "cli-flag",
            "--jobs",
            "2",
        ]) else {
            panic!("expected rust-code-analysis");
        };

        let merged = merge_rust_code_analysis_args(&args, &config, &explicit);

        // Config flags come first, then CLI flags
        assert_eq!(merged.extra_flags, vec!["--flag1", "--flag2", "cli-flag"]);
        assert_eq!(merged.jobs, 2);
        assert!(!merged.metrics);
        assert_eq!(merged.language, "python");
    }

    #[test]
    fn test_merge_contributor_report_args_with_config_values() {
        let mut config = RaffConfig::default();
        config.contributor_report.decay = 0.02;
        config.contributor_report.since = Some("2023-01-01".to_string());
        config.contributor_report.output = Some("html".to_string());
        let (crate::cli::Commands::ContributorReport(args), explicit) =
            parse_cli(&["raff", "contributor-report"])
        else {
            panic!("expected contributor-report");
        };

        let merged = merge_contributor_report_args(&args, &config, &explicit);

        assert_eq!(merged.decay, 0.02);
        assert_eq!(merged.since, Some("2023-01-01".to_string()));
        assert!(matches!(
            merged.output,
            crate::cli::ContributorReportOutputFormat::Html
        ));
    }

    #[test]
    fn test_merge_all_args_with_config_values() {
        let mut config = RaffConfig::default();
        config.general.path = Some(PathBuf::from("/general/path"));
        config.statement_count.threshold = 30;
        config.volatility.alpha = 0.03;
        config.volatility.normalize = true;
        config.coupling.granularity = Some("crate".to_string());
        config.rust_code_analysis.extra_flags = vec!["--rca-flag".to_string()];

        let merged = merged_all(&["raff", "all"], &config);

        assert_eq!(merged.path, PathBuf::from("/general/path"));
        assert_eq!(merged.sc_threshold, 30);
        assert_eq!(merged.vol_alpha, 0.03);
        assert!(merged.vol_normalize);
        assert!(matches!(
            merged.coup_granularity,
            crate::cli::CouplingGranularity::Crate
        ));
        assert_eq!(merged.rca_extra_flags, vec!["--rca-flag"]);
    }

    #[test]
    fn test_merge_all_args_cli_overrides_config() {
        let mut config = RaffConfig::default();
        config.statement_count.threshold = 15;
        config.volatility.alpha = 0.03;

        let merged = merged_all(
            &[
                "raff",
                "all",
                "--sc-threshold",
                "100",
                "--vol-alpha",
                "0.01",
            ],
            &config,
        );

        assert_eq!(merged.sc_threshold, 100);
        assert_eq!(merged.vol_alpha, 0.01);
    }

    #[test]
    fn test_parse_statement_count_output_format() {
        assert!(matches!(
            parse_statement_count_output_format("table"),
            Some(crate::cli::StatementCountOutputFormat::Table)
        ));
        assert!(matches!(
            parse_statement_count_output_format("html"),
            Some(crate::cli::StatementCountOutputFormat::Html)
        ));
        assert!(parse_statement_count_output_format("invalid").is_none());
    }

    #[test]
    fn test_parse_volatility_output_format() {
        assert!(matches!(
            parse_volatility_output_format("csv"),
            Some(crate::cli::VolatilityOutputFormat::Csv)
        ));
        assert!(matches!(
            parse_volatility_output_format("json"),
            Some(crate::cli::VolatilityOutputFormat::Json)
        ));
        assert!(matches!(
            parse_volatility_output_format("yaml"),
            Some(crate::cli::VolatilityOutputFormat::Yaml)
        ));
        assert!(parse_volatility_output_format("invalid").is_none());
    }

    #[test]
    fn test_parse_coupling_granularity() {
        assert!(matches!(
            parse_coupling_granularity("both"),
            Some(crate::cli::CouplingGranularity::Both)
        ));
        assert!(matches!(
            parse_coupling_granularity("crate"),
            Some(crate::cli::CouplingGranularity::Crate)
        ));
        assert!(matches!(
            parse_coupling_granularity("module"),
            Some(crate::cli::CouplingGranularity::Module)
        ));
        assert!(parse_coupling_granularity("invalid").is_none());
    }

    // Profile configuration tests

    #[test]
    fn test_pre_commit_profile_default() {
        let profile = PreCommitProfile::default();

        assert!(profile.fast.is_none(), "fast should be None by default");
        assert!(profile.staged.is_none(), "staged should be None by default");
        assert!(profile.quiet.is_none(), "quiet should be None by default");
        assert!(
            profile.sc_threshold.is_none(),
            "sc_threshold should be None by default"
        );
    }

    #[test]
    fn test_profile_config_default() {
        let config = ProfileConfig::default();

        assert!(
            config.pre_commit.is_none(),
            "pre_commit should be None by default"
        );
    }

    #[test]
    fn test_raff_config_default_includes_profile() {
        let config = RaffConfig::default();

        assert!(
            config.profile.pre_commit.is_none(),
            "profile.pre_commit should be None by default"
        );
    }

    #[test]
    fn test_load_config_with_pre_commit_profile() {
        let content = r#"
[profile.pre_commit]
fast = true
staged = true
quiet = true
sc_threshold = 15
"#;
        let temp_file = create_temp_config_file(content);

        let result = load_config_from_path(temp_file.path());

        assert!(result.is_ok(), "load_config_from_path should succeed");

        let config = result.unwrap().expect("config should be present");
        assert!(
            config.profile.pre_commit.is_some(),
            "pre_commit profile should be present"
        );

        let pc = config.profile.pre_commit.as_ref().unwrap();
        assert_eq!(pc.fast, Some(true), "fast should be true");
        assert_eq!(pc.staged, Some(true), "staged should be true");
        assert_eq!(pc.quiet, Some(true), "quiet should be true");
        assert_eq!(pc.sc_threshold, Some(15), "sc_threshold should be 15");
    }

    #[test]
    fn test_load_config_with_partial_pre_commit_profile() {
        let content = r#"
[profile.pre_commit]
fast = true
sc_threshold = 20
"#;
        let temp_file = create_temp_config_file(content);

        let result = load_config_from_path(temp_file.path());

        assert!(result.is_ok(), "load_config_from_path should succeed");

        let config = result.unwrap().expect("config should be present");
        let pc = config.profile.pre_commit.as_ref().unwrap();

        assert_eq!(pc.fast, Some(true));
        assert!(pc.staged.is_none(), "staged should be None when not set");
        assert!(pc.quiet.is_none(), "quiet should be None when not set");
        assert_eq!(pc.sc_threshold, Some(20));
    }

    #[test]
    fn test_config_with_profile_and_other_sections() {
        let content = r#"
[general]
verbose = true

[statement_count]
threshold = 25

[profile.pre_commit]
fast = true
staged = true
quiet = true
sc_threshold = 15
"#;
        let temp_file = create_temp_config_file(content);

        let result = load_config_from_path(temp_file.path());

        assert!(result.is_ok());

        let config = result.unwrap().expect("config should be present");
        assert!(config.general.verbose);
        assert_eq!(config.statement_count.threshold, 25);

        let pc = config.profile.pre_commit.as_ref().unwrap();
        assert_eq!(pc.fast, Some(true));
        assert_eq!(pc.staged, Some(true));
        assert_eq!(pc.quiet, Some(true));
        assert_eq!(pc.sc_threshold, Some(15));
    }

    #[test]
    fn test_profile_config_serialization() {
        let profile = PreCommitProfile {
            fast: Some(true),
            staged: Some(false),
            quiet: Some(true),
            sc_threshold: Some(15),
        };

        let toml_str = toml::to_string(&profile).expect("serialization should succeed");

        assert!(toml_str.contains("fast = true"));
        assert!(toml_str.contains("staged = false"));
        assert!(toml_str.contains("quiet = true"));
        assert!(toml_str.contains("sc_threshold = 15"));
    }

    #[test]
    fn test_profile_config_deserialization() {
        let toml_str = r#"
fast = true
staged = false
quiet = true
sc_threshold = 15
"#;

        let profile: PreCommitProfile =
            toml::from_str(toml_str).expect("deserialization should succeed");

        assert_eq!(profile.fast, Some(true));
        assert_eq!(profile.staged, Some(false));
        assert_eq!(profile.quiet, Some(true));
        assert_eq!(profile.sc_threshold, Some(15));
    }

    #[test]
    fn test_apply_pre_commit_profile_with_no_profile_returns_built_in_defaults() {
        let config = RaffConfig::default();

        let result = apply_pre_commit_profile(&config);

        assert_eq!(
            result.config.statement_count.threshold, 25,
            "threshold should fall back to the built-in pre-commit default"
        );
        assert!(result.fast, "fast should default to true for pre-commit");
        assert!(
            result.staged,
            "staged should default to true for pre-commit"
        );
        assert!(result.quiet, "quiet should default to true for pre-commit");
    }

    #[test]
    fn test_apply_pre_commit_profile_applies_sc_threshold() {
        let mut config = RaffConfig::default();
        config.statement_count.threshold = 10; // Default
        config.profile.pre_commit = Some(PreCommitProfile {
            fast: Some(true),
            staged: Some(true),
            quiet: Some(true),
            sc_threshold: Some(20),
        });

        let result = apply_pre_commit_profile(&config);

        assert_eq!(
            result.config.statement_count.threshold, 20,
            "threshold should be updated to profile value"
        );
        assert!(result.fast, "fast should be true");
        assert!(result.staged, "staged should be true");
        assert!(result.quiet, "quiet should be true");
    }

    #[test]
    fn test_apply_pre_commit_profile_preserves_other_config_values() {
        let mut config = RaffConfig::default();
        config.statement_count.threshold = 25;
        config.volatility.alpha = 0.05;
        config.general.verbose = true;
        config.profile.pre_commit = Some(PreCommitProfile {
            fast: Some(true),
            staged: Some(true),
            quiet: Some(true),
            sc_threshold: Some(15),
        });

        let result = apply_pre_commit_profile(&config);

        assert_eq!(
            result.config.statement_count.threshold, 15,
            "sc_threshold should be applied"
        );
        assert_eq!(
            result.config.volatility.alpha, 0.05,
            "volatility alpha should be preserved"
        );
        assert!(
            result.config.general.verbose,
            "general verbose should be preserved"
        );
        assert!(result.fast, "fast should be true");
        assert!(result.staged, "staged should be true");
        assert!(result.quiet, "quiet should be true");
    }

    #[test]
    fn test_apply_pre_commit_profile_with_partial_profile() {
        let mut config = RaffConfig::default();
        config.statement_count.threshold = 10;
        config.profile.pre_commit = Some(PreCommitProfile {
            fast: None,
            staged: None,
            quiet: None,
            sc_threshold: Some(30),
        });

        let result = apply_pre_commit_profile(&config);

        assert_eq!(
            result.config.statement_count.threshold, 30,
            "only sc_threshold should be applied"
        );
        assert!(result.fast, "fast should default to true when not set");
        assert!(result.staged, "staged should default to true when not set");
        assert!(result.quiet, "quiet should default to true when not set");
    }

    #[test]
    fn test_apply_pre_commit_profile_does_not_modify_original_config() {
        let mut config = RaffConfig::default();
        config.statement_count.threshold = 10;
        config.profile.pre_commit = Some(PreCommitProfile {
            fast: Some(true),
            staged: Some(true),
            quiet: Some(true),
            sc_threshold: Some(25),
        });

        let original_threshold = config.statement_count.threshold;
        let _ = apply_pre_commit_profile(&config);

        assert_eq!(
            config.statement_count.threshold, original_threshold,
            "original config should not be modified"
        );
    }

    #[test]
    fn test_apply_pre_commit_profile_with_zero_threshold() {
        let mut config = RaffConfig::default();
        config.statement_count.threshold = 10;
        config.profile.pre_commit = Some(PreCommitProfile {
            fast: Some(true),
            staged: Some(true),
            quiet: Some(true),
            sc_threshold: Some(0),
        });

        let result = apply_pre_commit_profile(&config);

        assert_eq!(
            result.config.statement_count.threshold, 0,
            "zero threshold should be applied"
        );
        assert!(result.fast, "fast should be true");
        assert!(result.staged, "staged should be true");
        assert!(result.quiet, "quiet should be true");
    }

    #[test]
    #[serial(cwd)]
    fn test_discover_and_load_config_finds_raff_directory_config() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Initialize git repo
        let status = Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .status();

        // Only run test if git is available
        if status.is_err() || !status.unwrap().success() {
            return; // Skip test if git is not available
        }

        // Create .raff directory and config file
        let raff_dir = temp_dir.path().join(".raff");
        fs::create_dir(&raff_dir).expect("Failed to create .raff directory");

        let raff_config_path = raff_dir.join("raff.toml");
        let content = r#"
[general]
verbose = true

[statement_count]
threshold = 15
"#;
        fs::write(&raff_config_path, content).expect("Failed to write config file");

        // Use a known valid path since current_dir() might fail if previous test deleted its temp dir
        let fallback_path = std::env::var("HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                tempfile::TempDir::new()
                    .ok()
                    .map(|d| d.path().to_path_buf())
            })
            .expect("Failed to get fallback path");
        let original_path = std::env::current_dir().unwrap_or(fallback_path.clone());

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory - use fallback if original path no longer exists
        let _ = std::env::set_current_dir(&original_path)
            .or_else(|_| std::env::set_current_dir(&fallback_path));

        assert!(result.is_ok(), "discover_and_load_config should succeed");

        let (path, config) = result.unwrap().expect("should find config file");
        // Check that the config was found at .raff/raff.toml
        assert!(
            path.ends_with(".raff/raff.toml") || path.ends_with(".raff/raff.toml"),
            "should find config at .raff/raff.toml, got: {:?}",
            path
        );
        assert!(config.general.verbose, "verbose should be true");
        assert_eq!(
            config.statement_count.threshold, 15,
            "threshold should be 15"
        );
    }

    #[test]
    #[serial(cwd)]
    fn test_discover_and_load_config_prioritizes_local_config_over_raff_directory() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Initialize git repo
        let status = Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .status();

        // Only run test if git is available
        if status.is_err() || !status.unwrap().success() {
            return; // Skip test if git is not available
        }

        // Create local Raff.toml
        let local_config_path = temp_dir.path().join("Raff.toml");
        let local_content = r#"
[general]
verbose = false
"#;
        fs::write(&local_config_path, local_content).expect("Failed to write local config");

        // Create .raff directory config (should not be used since local config exists)
        let raff_dir = temp_dir.path().join(".raff");
        fs::create_dir(&raff_dir).expect("Failed to create .raff directory");

        let raff_config_path = raff_dir.join("raff.toml");
        let raff_content = r#"
[general]
verbose = true

[statement_count]
threshold = 50
"#;
        fs::write(&raff_config_path, raff_content).expect("Failed to write .raff config");

        // Use a known valid path
        let fallback_path = std::env::var("HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                tempfile::TempDir::new()
                    .ok()
                    .map(|d| d.path().to_path_buf())
            })
            .expect("Failed to get fallback path");
        let original_path = std::env::current_dir().unwrap_or(fallback_path.clone());

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory
        let _ = std::env::set_current_dir(&original_path)
            .or_else(|_| std::env::set_current_dir(&fallback_path));

        assert!(result.is_ok(), "discover_and_load_config should succeed");

        let (path, config) = result.unwrap().expect("should find config file");
        // Local config should take priority
        assert!(
            path.ends_with("Raff.toml") || path.ends_with("Raff.toml"),
            "should find local config, got: {:?}",
            path
        );
        assert!(
            !config.general.verbose,
            "should use local config (verbose=false)"
        );
    }

    #[test]
    #[serial(cwd)]
    fn test_discover_and_load_config_finds_raff_directory_from_subdirectory() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Initialize git repo
        let status = Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .status();

        // Only run test if git is available
        if status.is_err() || !status.unwrap().success() {
            return; // Skip test if git is not available
        }

        // Create .raff directory and config file at repo root
        let raff_dir = temp_dir.path().join(".raff");
        fs::create_dir(&raff_dir).expect("Failed to create .raff directory");

        let raff_config_path = raff_dir.join("raff.toml");
        let content = r#"
[coupling]
granularity = "module"
"#;
        fs::write(&raff_config_path, content).expect("Failed to write config file");

        // Create a subdirectory
        let subdir = temp_dir.path().join("nested/subdir");
        fs::create_dir_all(&subdir).expect("Failed to create subdir");

        // Use a known valid path
        let fallback_path = std::env::var("HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                tempfile::TempDir::new()
                    .ok()
                    .map(|d| d.path().to_path_buf())
            })
            .expect("Failed to get fallback path");
        let original_path = std::env::current_dir().unwrap_or(fallback_path.clone());

        std::env::set_current_dir(&subdir).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory
        let _ = std::env::set_current_dir(&original_path)
            .or_else(|_| std::env::set_current_dir(&fallback_path));

        assert!(result.is_ok(), "discover_and_load_config should succeed");

        let (_path, config) = result.unwrap().expect("should find config file");
        assert_eq!(
            config.coupling.granularity.as_ref().unwrap(),
            "module",
            "should load .raff/raff.toml from git root when in subdirectory"
        );
    }

    #[test]
    #[serial(cwd)]
    fn test_discover_and_load_config_returns_none_when_only_raff_dir_exists_but_no_config() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Initialize git repo
        let status = Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .status();

        // Only run test if git is available
        if status.is_err() || !status.unwrap().success() {
            return; // Skip test if git is not available
        }

        // Create .raff directory but no config file
        let raff_dir = temp_dir.path().join(".raff");
        fs::create_dir(&raff_dir).expect("Failed to create .raff directory");

        // Use a known valid path
        let fallback_path = std::env::var("HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                tempfile::TempDir::new()
                    .ok()
                    .map(|d| d.path().to_path_buf())
            })
            .expect("Failed to get fallback path");
        let original_path = std::env::current_dir().unwrap_or(fallback_path.clone());

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory
        let _ = std::env::set_current_dir(&original_path)
            .or_else(|_| std::env::set_current_dir(&fallback_path));

        assert!(result.is_ok(), "discover_and_load_config should succeed");
        assert!(
            result.unwrap().is_none(),
            "should return None when .raff directory exists but no config file"
        );
    }
}
