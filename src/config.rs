//! Configuration file support for Raff.
//!
//! This module provides functionality to load configuration from TOML files
//! and merge them with command-line arguments. CLI arguments take precedence
//! over config file values.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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

    /// Weighting factor for lines changed (churn) vs. commit touch count.
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

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read configuration file from {}", path.display()))?;

    let config: RaffConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse configuration file from {}", path.display()))?;

    Ok(Some(config))
}

/// Discover and load configuration from default locations.
///
/// Searches for configuration files in the current directory and parent directories,
/// using the default config file names: `Raff.toml`, `.raff.toml`, `raff.toml`.
///
/// # Returns
///
/// Returns `Some(RaffConfig)` if a config file is found and can be parsed.
/// Returns `None` if no config file is found.
pub fn discover_and_load_config() -> Result<Option<(PathBuf, RaffConfig)>> {
    let mut current_dir = std::env::current_dir().context("Failed to get current directory")?;

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

/// Get the path from config if set, otherwise return the default.
pub fn resolve_path(config_path: &Option<PathBuf>, default: &PathBuf) -> PathBuf {
    config_path.as_ref().unwrap_or(default).clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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
    fn test_discover_and_load_config_finds_raff_toml() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join("Raff.toml");

        let content = r#"
[general]
verbose = true
"#;
        fs::write(&config_path, content).expect("Failed to write config file");

        let original_path = std::env::current_dir().expect("Failed to get current dir");

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory - temp_dir is still in scope here
        let _ = std::env::set_current_dir(&original_path);

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
    fn test_discover_and_load_config_finds_dot_raff_toml() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join(".raff.toml");

        let content = r#"
[statement_count]
threshold = 50
"#;
        fs::write(&config_path, content).expect("Failed to write config file");

        let original_path = std::env::current_dir().expect("Failed to get current dir");

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory - ignore errors if temp dir was already cleaned up
        let _ = std::env::set_current_dir(&original_path);

        assert!(result.is_ok(), "discover_and_load_config should succeed");

        let (_path, config) = result.unwrap().expect("should find config file");
        assert_eq!(
            config.statement_count.threshold, 50,
            "should load config from .raff.toml"
        );
    }

    #[test]
    fn test_discover_and_load_config_returns_none_when_no_config() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let original_path = std::env::current_dir().expect("Failed to get current dir");

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = discover_and_load_config();

        // Restore directory - ignore errors if temp dir was already cleaned up
        let _ = std::env::set_current_dir(&original_path);

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
    fn test_load_config_with_none_path_discovers_config() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join("Raff.toml");

        let content = r#"
[general]
verbose = true
"#;
        fs::write(&config_path, content).expect("Failed to write config file");

        let original_path = std::env::current_dir().expect("Failed to get current dir");

        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        let result = load_config(None);

        // Restore directory - ignore errors if temp dir was already cleaned up
        let _ = std::env::set_current_dir(&original_path);

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
}
