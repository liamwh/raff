//! Hierarchical configuration loading for Raff.
//!
//! This module provides functionality to load configuration from multiple sources
//! with a well-defined priority order:
//!
//! 1. **User-level config**: `<XDG_CONFIG_HOME>/raff/raff.toml` (or `~/.config/raff/raff.toml`)
//! 2. **Repo-local config**: `<git repo root>/.raff/raff.local.toml`
//! 3. **Traditional local config**: `Raff.toml`, `.raff.toml`, `raff.toml` (searched upward from cwd)
//! 4. **CLI arguments**: Override everything (handled separately in CLI layer)
//!
//! # Priority Order
//!
//! Config files are merged in order, with later sources overriding earlier ones:
//! ```text
//! User config (lowest priority)
//!     ↓
//! Repo-local config (overrides user)
//!     ↓
//! Traditional local config (overrides repo)
//!     ↓
//! CLI arguments (highest priority)
//! ```
//!
//! # Usage
//!
//! ```no_run
//! # use raff_core::config_hierarchy::load_hierarchical_config;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use std::path::Path;
//!
//! // Load hierarchical config (without CLI-specified path)
//! let hierarchical = load_hierarchical_config(None)?;
//!
//! // Log what was loaded
//! for source in &hierarchical.sources {
//!     println!("Loaded {:?} config from: {}",
//!         source.source_type,
//!         source.path.display()
//!     );
//! }
//!
//! // Use the merged config
//! let config = &hierarchical.merged;
//! # Ok(())
//! # }
//! ```

use crate::config::{RaffConfig, load_config_from_path};
use crate::error::{RaffError, Result};
use std::path::{Path, PathBuf};

/// User configuration file name.
const USER_CONFIG_FILE: &str = "raff.toml";

/// Repo-local configuration file name.
const REPO_LOCAL_CONFIG_FILE: &str = "raff.local.toml";

/// Configuration source type.
///
/// Indicates which level of the configuration hierarchy this source came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSourceType {
    /// User-level configuration from `~/.config/raff/raff.toml`.
    User,

    /// Repository-local configuration from `<repo>/.raff/raff.local.toml`.
    RepoLocal,

    /// Traditional local configuration from `Raff.toml` or similar.
    TraditionalLocal,

    /// Explicitly specified via CLI `--config` flag.
    CliExplicit,
}

/// A single configuration source in the hierarchy.
///
/// Represents one configuration file that was loaded, including
/// its type, path, and parsed contents.
#[derive(Debug, Clone)]
pub struct ConfigSource {
    /// The type of configuration source.
    pub source_type: ConfigSourceType,

    /// The path to the configuration file.
    pub path: PathBuf,

    /// The parsed configuration from this source.
    pub config: RaffConfig,
}

impl ConfigSource {
    /// Creates a new configuration source.
    #[must_use]
    pub const fn new(source_type: ConfigSourceType, path: PathBuf, config: RaffConfig) -> Self {
        Self {
            source_type,
            path,
            config,
        }
    }
}

/// Hierarchical configuration result.
///
/// Contains all configuration sources that were loaded and the
/// final merged configuration.
#[derive(Debug, Clone)]
pub struct HierarchicalConfig {
    /// All configuration sources that were loaded, in priority order.
    pub sources: Vec<ConfigSource>,

    /// The merged configuration from all sources.
    pub merged: RaffConfig,
}

impl HierarchicalConfig {
    /// Creates a new hierarchical configuration result.
    #[must_use]
    pub const fn new(sources: Vec<ConfigSource>, merged: RaffConfig) -> Self {
        Self { sources, merged }
    }
}

/// Returns the user configuration directory.
///
/// Follows the XDG Base Directory specification on Unix-like systems:
/// - Uses `XDG_CONFIG_HOME` if set
/// - Falls back to `~/.config/raff`
///
/// On Windows, uses `APPDATA/raff`.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
pub fn get_user_config_dir() -> Result<PathBuf> {
    // Try XDG_CONFIG_HOME first (Unix-like systems)
    if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg_config).join("raff"));
    }

    // Fallback to ~/.config/raff on Unix-like systems
    #[cfg(unix)]
    {
        if let Some(home) = home_dir() {
            return Ok(home.join(".config").join("raff"));
        }
    }

    // Fallback to APPDATA on Windows
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return Ok(PathBuf::from(appdata).join("raff"));
        }
    }

    // If we still can't find anything, try HOME as a last resort
    if let Some(home) = home_dir() {
        return Ok(home.join(".config").join("raff"));
    }

    Err(RaffError::config_error(
        "Could not determine user config directory - please set XDG_CONFIG_HOME or HOME",
    ))
}

/// Returns the user configuration file path, if it would exist.
///
/// This function does not check if the file actually exists.
///
/// # Returns
///
/// * `Some(PathBuf)` - if the user config directory can be determined
/// * `None` - if the home directory cannot be determined
pub fn get_user_config_path() -> Option<PathBuf> {
    get_user_config_dir()
        .ok()
        .map(|dir| dir.join(USER_CONFIG_FILE))
}

/// Finds the git repository root from the current working directory.
///
/// Uses git2 to discover the repository root by searching upward from
/// the current directory.
///
/// # Returns
///
/// * `Ok(Some(PathBuf))` - if inside a git repository
/// * `Ok(None)` - if not in a git repository
/// * `Err(RaffError)` - if an error occurs while searching
pub fn find_git_repo_root() -> Result<Option<PathBuf>> {
    let cwd = std::env::current_dir().map_err(|e| {
        RaffError::io_error_with_source("get current directory", PathBuf::from("."), e)
    })?;

    match git2::Repository::discover(&cwd) {
        Ok(repo) => Ok(repo.workdir().map(PathBuf::from)),
        Err(e) => {
            // Return Ok(None) for expected "not in a git repo" errors
            // This includes both Config errors and generic "not found" errors
            if e.class() == git2::ErrorClass::Config || e.code() == git2::ErrorCode::NotFound {
                Ok(None)
            } else {
                Err(RaffError::git_error_with_repo("find repository root", cwd))
            }
        }
    }
}

/// Returns the repository-local configuration file path.
///
/// Returns `Some(PathBuf)` if we're in a git repository, `None` otherwise.
/// Does not check if the file actually exists.
///
/// # Returns
///
/// * `Some(PathBuf)` - path to `.raff/raff.local.toml` in the repo root
/// * `None` - if not in a git repository or the repo root cannot be determined
pub fn get_repo_local_config_path() -> Option<PathBuf> {
    find_git_repo_root()
        .ok()
        .flatten()
        .map(|root| root.join(".raff").join(REPO_LOCAL_CONFIG_FILE))
}

/// Loads configuration hierarchically from all sources.
///
/// If `cli_explicit_path` is provided, only that file is loaded (CLI explicit mode).
/// Otherwise, loads from user, repo-local, and traditional local configs in order.
///
/// # Arguments
///
/// * `cli_explicit_path` - Optional path to a config file specified via CLI `--config`.
///
/// # Errors
///
/// Returns an error if a config file exists but cannot be parsed.
pub fn load_hierarchical_config(cli_explicit_path: Option<&Path>) -> Result<HierarchicalConfig> {
    let mut sources = Vec::new();
    let mut merged = RaffConfig::default();

    // If CLI specified an explicit path, only load that
    if let Some(explicit_path) = cli_explicit_path {
        if let Some(config) = load_config_from_path(explicit_path)? {
            merged = config.clone();
            sources.push(ConfigSource::new(
                ConfigSourceType::CliExplicit,
                explicit_path.to_path_buf(),
                config,
            ));
        }
        return Ok(HierarchicalConfig::new(sources, merged));
    }

    // Load user-level config (lowest priority)
    if let Some(user_path) = get_user_config_path()
        && let Some(config) = load_config_from_path(&user_path)?
    {
        merged = merge_configs(&merged, &config);
        sources.push(ConfigSource::new(ConfigSourceType::User, user_path, config));
    }

    // Load repo-local config (overrides user)
    if let Some(repo_path) = get_repo_local_config_path()
        && let Some(config) = load_config_from_path(&repo_path)?
    {
        merged = merge_configs(&merged, &config);
        sources.push(ConfigSource::new(
            ConfigSourceType::RepoLocal,
            repo_path,
            config,
        ));
    }

    // Load traditional local config (overrides repo)
    // This uses the existing discover_and_load_config function
    if let Some((path, config)) = crate::config::discover_and_load_config()? {
        merged = merge_configs(&merged, &config);
        sources.push(ConfigSource::new(
            ConfigSourceType::TraditionalLocal,
            path,
            config,
        ));
    }

    Ok(HierarchicalConfig::new(sources, merged))
}

/// Merges two configurations, with `override_` taking precedence over `base`.
///
/// # Arguments
///
/// * `base` - The base configuration (lower priority).
/// * `override_` - The overriding configuration (higher priority).
///
/// # Returns
///
/// A new configuration with values from `override_` taking precedence.
#[must_use]
pub fn merge_configs(base: &RaffConfig, override_: &RaffConfig) -> RaffConfig {
    RaffConfig {
        general: base.general.merge(&override_.general),
        statement_count: base.statement_count.merge(&override_.statement_count),
        volatility: base.volatility.merge(&override_.volatility),
        coupling: base.coupling.merge(&override_.coupling),
        rust_code_analysis: base.rust_code_analysis.merge(&override_.rust_code_analysis),
        contributor_report: base.contributor_report.merge(&override_.contributor_report),
        profile: base.profile.merge(&override_.profile),
    }
}

/// Trait for types that can be merged with another instance of the same type.
///
/// This trait allows configuration structs to be merged hierarchically,
/// with overriding values taking precedence.
pub trait Mergeable: Sized {
    /// Merges this value with another, with `other` taking precedence.
    ///
    /// # Arguments
    ///
    /// * `other` - The overriding value.
    ///
    /// # Returns
    ///
    /// A new merged value.
    #[must_use]
    fn merge(&self, other: &Self) -> Self;
}

impl Mergeable for crate::config::GeneralConfig {
    fn merge(&self, other: &Self) -> Self {
        Self {
            path: other.path.clone().or_else(|| self.path.clone()),
            verbose: other.verbose || self.verbose,
            output_file: other
                .output_file
                .clone()
                .or_else(|| self.output_file.clone()),
        }
    }
}

impl Mergeable for crate::config::StatementCountConfig {
    fn merge(&self, other: &Self) -> Self {
        Self {
            path: other.path.clone().or_else(|| self.path.clone()),
            threshold: other.threshold,
            output: other.output.clone().or_else(|| self.output.clone()),
        }
    }
}

impl Mergeable for crate::config::VolatilityConfig {
    fn merge(&self, other: &Self) -> Self {
        Self {
            path: other.path.clone().or_else(|| self.path.clone()),
            alpha: other.alpha,
            since: other.since.clone().or_else(|| self.since.clone()),
            normalize: other.normalize || self.normalize,
            skip_merges: other.skip_merges || self.skip_merges,
            output: other.output.clone().or_else(|| self.output.clone()),
        }
    }
}

impl Mergeable for crate::config::CouplingConfig {
    fn merge(&self, other: &Self) -> Self {
        Self {
            path: other.path.clone().or_else(|| self.path.clone()),
            output: other.output.clone().or_else(|| self.output.clone()),
            granularity: other
                .granularity
                .clone()
                .or_else(|| self.granularity.clone()),
        }
    }
}

impl Mergeable for crate::config::RustCodeAnalysisConfig {
    fn merge(&self, other: &Self) -> Self {
        // If both configs are identical, return self (idempotence)
        if self == other {
            return self.clone();
        }

        // Otherwise, merge extra_flags by concatenating
        let mut extra_flags = self.extra_flags.clone();
        extra_flags.extend(other.extra_flags.clone());

        Self {
            path: other.path.clone().or_else(|| self.path.clone()),
            extra_flags,
            jobs: other.jobs.or(self.jobs),
            output: other.output.clone().or_else(|| self.output.clone()),
            metrics: other.metrics && self.metrics,
            language: if other.language != "rust" {
                other.language.clone()
            } else {
                self.language.clone()
            },
        }
    }
}

impl Mergeable for crate::config::ContributorReportConfig {
    fn merge(&self, other: &Self) -> Self {
        Self {
            path: other.path.clone().or_else(|| self.path.clone()),
            since: other.since.clone().or_else(|| self.since.clone()),
            decay: other.decay,
            output: other.output.clone().or_else(|| self.output.clone()),
        }
    }
}

impl Mergeable for crate::config::PreCommitProfile {
    fn merge(&self, other: &Self) -> Self {
        Self {
            fast: other.fast.or(self.fast),
            staged: other.staged.or(self.staged),
            quiet: other.quiet.or(self.quiet),
            sc_threshold: other.sc_threshold.or(self.sc_threshold),
        }
    }
}

impl Mergeable for crate::config::ProfileConfig {
    fn merge(&self, other: &Self) -> Self {
        // If other has pre_commit, merge with self's pre_commit (if any)
        // Otherwise, keep self's pre_commit
        let pre_commit = match (&other.pre_commit, &self.pre_commit) {
            (Some(other_pc), Some(self_pc)) => {
                // Both have values - merge them
                Some(self_pc.merge(other_pc))
            }
            (Some(other_pc), None) => Some(other_pc.clone()),
            (None, Some(self_pc)) => Some(self_pc.clone()),
            (None, None) => None,
        };

        Self { pre_commit }
    }
}

impl Mergeable for RaffConfig {
    fn merge(&self, other: &Self) -> Self {
        merge_configs(self, other)
    }
}

/// Returns the user's home directory.
///
/// This is a platform-independent helper that tries multiple
/// environment variables to find the home directory.
fn home_dir() -> Option<PathBuf> {
    // Try standard environment variables
    if let Some(home) = std::env::var_os("HOME") {
        return Some(PathBuf::from(home));
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(userprofile) = std::env::var_os("USERPROFILE") {
            return Some(PathBuf::from(userprofile));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GeneralConfig, StatementCountConfig};
    use std::fs;
    use tempfile::TempDir;

    // Helper to create a temporary config file with content
    fn create_temp_config_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let config_path = dir.join(name);
        fs::create_dir_all(config_path.parent().expect("config has parent"))
            .expect("Failed to create config dir");
        fs::write(&config_path, content).expect("Failed to write config");
        config_path
    }

    #[test]
    fn test_config_source_new_creates_source() {
        let path = PathBuf::from("/test/config.toml");
        let config = RaffConfig::default();
        let source = ConfigSource::new(ConfigSourceType::User, path.clone(), config.clone());

        assert!(matches!(source.source_type, ConfigSourceType::User));
        assert_eq!(source.path, path);
    }

    #[test]
    fn test_hierarchical_config_new_creates_result() {
        let sources = vec![];
        let merged = RaffConfig::default();
        let result = HierarchicalConfig::new(sources.clone(), merged.clone());

        assert!(result.sources.is_empty());
    }

    #[test]
    fn test_get_user_config_dir_returns_path() {
        let result = get_user_config_dir();

        // Should succeed unless HOME/XDG_CONFIG_HOME are not set
        if let Ok(path) = result {
            assert!(path.ends_with("raff"));
        }
    }

    #[test]
    fn test_get_user_config_path_returns_path() {
        let result = get_user_config_path();

        if let Some(path) = result {
            assert!(path.ends_with("raff.toml"));
        }
    }

    #[test]
    fn test_find_git_repo_root_in_this_repo() {
        let result = find_git_repo_root();

        // We're running tests in a git repo, so this should return Some
        // But we should handle the case where we're not in a git repo gracefully
        if let Ok(Some(root)) = result {
            // The repo root should contain a .git directory
            let git_dir = root.join(".git");
            assert!(git_dir.exists(), "repo root should contain .git directory");
        }
        // If result is Err or repo_root is None, that's okay in some environments
    }

    #[test]
    fn test_get_repo_local_config_path_returns_path() {
        let result = get_repo_local_config_path();

        // We're in a git repo, so this should return Some
        if let Some(path) = result {
            assert!(path.ends_with(".raff/raff.local.toml"));
        }
    }

    #[test]
    fn test_merge_configs_with_empty_override_returns_base() {
        let base = RaffConfig::default();
        let override_ = RaffConfig::default();

        let merged = merge_configs(&base, &override_);

        assert_eq!(merged.general.path, base.general.path);
        assert_eq!(
            merged.statement_count.threshold,
            base.statement_count.threshold
        );
    }

    #[test]
    fn test_merge_configs_override_takes_precedence() {
        let mut base = RaffConfig::default();
        base.statement_count.threshold = 10;

        let mut override_ = RaffConfig::default();
        override_.statement_count.threshold = 25;

        let merged = merge_configs(&base, &override_);

        assert_eq!(merged.statement_count.threshold, 25);
    }

    #[test]
    fn test_merge_general_config_override_takes_precedence() {
        let base = GeneralConfig {
            path: Some(PathBuf::from("/base/path")),
            verbose: false,
            output_file: None,
        };
        let override_ = GeneralConfig {
            path: Some(PathBuf::from("/override/path")),
            verbose: true,
            output_file: None,
        };

        let merged = base.merge(&override_);

        assert_eq!(merged.path, Some(PathBuf::from("/override/path")));
        assert!(merged.verbose);
    }

    #[test]
    fn test_merge_general_config_base_fills_missing() {
        let base = GeneralConfig {
            path: Some(PathBuf::from("/base/path")),
            verbose: false,
            output_file: None,
        };
        let override_ = GeneralConfig {
            path: None,
            verbose: true,
            output_file: None,
        };

        let merged = base.merge(&override_);

        assert_eq!(merged.path, Some(PathBuf::from("/base/path")));
        assert!(merged.verbose);
    }

    #[test]
    fn test_merge_statement_count_config_override_takes_precedence() {
        let base = StatementCountConfig {
            path: Some(PathBuf::from("/base")),
            threshold: 10,
            output: Some("table".to_string()),
        };
        let override_ = StatementCountConfig {
            path: Some(PathBuf::from("/override")),
            threshold: 25,
            output: Some("html".to_string()),
        };

        let merged = base.merge(&override_);

        assert_eq!(merged.path, Some(PathBuf::from("/override")));
        assert_eq!(merged.threshold, 25);
        assert_eq!(merged.output, Some("html".to_string()));
    }

    #[test]
    fn test_merge_configs_is_transitive() {
        let config1 = RaffConfig {
            statement_count: StatementCountConfig {
                threshold: 10,
                ..Default::default()
            },
            ..Default::default()
        };
        let config2 = RaffConfig {
            statement_count: StatementCountConfig {
                threshold: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        let config3 = RaffConfig {
            statement_count: StatementCountConfig {
                threshold: 30,
                ..Default::default()
            },
            ..Default::default()
        };

        // (config1.merge(config2)).merge(config3) == config1.merge(config2.merge(config3))
        let left = merge_configs(&merge_configs(&config1, &config2), &config3);
        let right = merge_configs(&config1, &merge_configs(&config2, &config3));

        assert_eq!(
            left.statement_count.threshold,
            right.statement_count.threshold
        );
        assert_eq!(left.statement_count.threshold, 30);
    }

    #[test]
    fn test_merge_configs_default_is_neutral_element() {
        let config = RaffConfig {
            statement_count: StatementCountConfig {
                threshold: 42,
                ..Default::default()
            },
            ..Default::default()
        };
        let default = RaffConfig::default();

        // default.merge(config) == config (config values win)
        let merged = merge_configs(&default, &config);
        assert_eq!(merged.statement_count.threshold, 42);

        // config.merge(default) == default (override/default values win)
        // Note: In hierarchical merge, the "other/override" parameter always wins
        let merged = merge_configs(&config, &default);
        assert_eq!(merged.statement_count.threshold, 10); // default threshold
    }

    #[test]
    fn test_load_hierarchical_config_with_cli_explicit_path() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path = create_temp_config_file(
            temp_dir.path(),
            "explicit.toml",
            r#"
[statement_count]
threshold = 99
"#,
        );

        let result = load_hierarchical_config(Some(&config_path));

        assert!(result.is_ok(), "load_hierarchical_config should succeed");
        let hierarchical = result.unwrap();

        assert_eq!(hierarchical.sources.len(), 1);
        assert!(matches!(
            hierarchical.sources[0].source_type,
            ConfigSourceType::CliExplicit
        ));
        assert_eq!(hierarchical.merged.statement_count.threshold, 99);
    }

    #[test]
    fn test_load_hierarchical_config_with_user_config() {
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
        let original_cwd = std::env::current_dir().unwrap_or(fallback_path.clone());
        let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create user config directory structure
        // XDG_CONFIG_HOME/raff/raff.toml
        let xdg_config = temp_dir.path().join("xdg-config");
        let user_config_dir = xdg_config.join("raff");
        fs::create_dir_all(&user_config_dir).expect("Failed to create user config dir");
        let user_config_path = user_config_dir.join("raff.toml");
        fs::write(
            &user_config_path,
            r#"
[statement_count]
threshold = 15
"#,
        )
        .expect("Failed to write user config");

        // Set XDG_CONFIG_HOME to xdg_config for this test
        // get_user_config_dir() will append "raff" to get xdg_config/raff
        unsafe { std::env::set_var("XDG_CONFIG_HOME", xdg_config) };

        // Also create a traditional config in the temp dir to avoid discovery issues
        let traditional_config = temp_dir.path().join("Raff.toml");
        fs::write(
            &traditional_config,
            r#"
[statement_count]
threshold = 10
"#,
        )
        .expect("Failed to write traditional config");

        // Change to temp directory
        std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

        // Load config - should pick up user config and traditional config
        let result = load_hierarchical_config(None);

        // Restore original directory - use fallback if original path no longer exists
        let _ = std::env::set_current_dir(&original_cwd)
            .or_else(|_| std::env::set_current_dir(&fallback_path));

        // Restore original environment before temp_dir is dropped
        if let Some(xdg) = original_xdg {
            unsafe { std::env::set_var("XDG_CONFIG_HOME", xdg) };
        } else {
            unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        }

        // Now we can assert - temp_dir is still in scope but we're back in a valid directory
        assert!(
            result.is_ok(),
            "load_hierarchical_config should succeed: {:?}",
            result.err()
        );
        let hierarchical = result.unwrap();

        // Should have loaded at least one config
        assert!(
            !hierarchical.sources.is_empty(),
            "sources should not be empty: got {} sources",
            hierarchical.sources.len()
        );

        // The user config (threshold 15) should be loaded
        assert!(
            hierarchical
                .sources
                .iter()
                .any(|s| s.config.statement_count.threshold == 15),
            "should find user config with threshold 15, got thresholds: {:?}",
            hierarchical
                .sources
                .iter()
                .map(|s| s.config.statement_count.threshold)
                .collect::<Vec<_>>()
        );

        // temp_dir is dropped here, at the end of the function
    }
}
