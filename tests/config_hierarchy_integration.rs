//! Integration tests for hierarchical configuration loading.
//!
//! These tests verify end-to-end behavior of the hierarchical configuration
//! system, including:
//! - End-to-end hierarchy with temporary directories
//! - Real git repository scenarios
//! - Backward compatibility with Raff.toml

use raff_core::config_hierarchy::{find_git_repo_root, load_hierarchical_config, ConfigSourceType};
use serial_test::serial;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Helper to get a safe fallback directory for restoring cwd.
fn get_fallback_dir() -> PathBuf {
    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            tempfile::TempDir::new()
                .ok()
                .map(|d| d.path().to_path_buf())
        })
        .expect("Failed to get fallback directory")
}

/// Helper to create a temporary config file with content.
fn create_temp_config_file(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let config_path = dir.join(name);
    fs::create_dir_all(config_path.parent().expect("config has parent"))
        .expect("Failed to create config dir");
    fs::write(&config_path, content).expect("Failed to write config");
    config_path
}

/// Helper to initialize a git repository in the given directory.
fn init_git_repo(dir: &Path) {
    let status = Command::new("git")
        .arg("init")
        .current_dir(dir)
        .status()
        .expect("Failed to run git init");

    assert!(status.success(), "git init failed with status: {status:?}");

    // Configure git to allow commits
    let status = Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .expect("Failed to configure git user.email");

    assert!(
        status.success(),
        "git config user.email failed with status: {status:?}"
    );

    let status = Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .status()
        .expect("Failed to configure git user.name");

    assert!(
        status.success(),
        "git config user.name failed with status: {status:?}"
    );
}

/// Test end-to-end hierarchy loading with temporary directories.
///
/// This test creates a complete hierarchy (user, repo-local, traditional)
/// in temporary directories and verifies that all sources are loaded and
/// merged in the correct priority order.
#[test]
#[serial]
fn integration_end_to_end_hierarchy_with_temp_directories() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let fallback_dir = get_fallback_dir();
    let original_cwd = std::env::current_dir().unwrap_or(fallback_dir.clone());
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Initialize git repo so repo-local config can be found
    init_git_repo(temp_dir.path());

    // Create user config with base values
    let xdg_config = temp_dir.path().join("xdg-config");
    let _user_config_path = create_temp_config_file(
        &xdg_config,
        "raff/raff.toml",
        r#"
[general]
verbose = false

[statement_count]
threshold = 10

[volatility]
alpha = 0.01
normalize = false
"#,
    );

    // Set XDG_CONFIG_HOME
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Create repo-local config that overrides user config
    let _repo_local_config = create_temp_config_file(
        temp_dir.path(),
        ".raff/raff.local.toml",
        r#"
[statement_count]
threshold = 20

[volatility]
alpha = 0.02
normalize = true
"#,
    );

    // Create traditional config that overrides repo-local config
    let _traditional_config = create_temp_config_file(
        temp_dir.path(),
        "Raff.toml",
        r#"
[statement_count]
threshold = 30

[volatility]
alpha = 0.03
"#,
    );

    // Change to temp directory
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    // Load hierarchical config
    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd)
        .or_else(|_| std::env::set_current_dir(&fallback_dir));

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    // Verify loading succeeded
    let hierarchical = result.expect("load_hierarchical_config should succeed");

    // Verify all three sources were loaded
    assert_eq!(
        hierarchical.sources.len(),
        3,
        "should load 3 config sources: user, repo-local, traditional"
    );

    // Verify source types are in correct priority order
    assert!(
        matches!(hierarchical.sources[0].source_type, ConfigSourceType::User),
        "first source should be User config"
    );
    assert!(
        matches!(
            hierarchical.sources[1].source_type,
            ConfigSourceType::RepoLocal
        ),
        "second source should be RepoLocal config"
    );
    assert!(
        matches!(
            hierarchical.sources[2].source_type,
            ConfigSourceType::TraditionalLocal
        ),
        "third source should be TraditionalLocal config"
    );

    // Verify merging priority: traditional > repo-local > user
    assert_eq!(
        hierarchical.merged.statement_count.threshold, 30,
        "merged threshold should be from traditional config (highest priority)"
    );
    assert_eq!(
        hierarchical.merged.volatility.alpha, 0.03,
        "merged alpha should be from traditional config (highest priority)"
    );
    assert!(
        hierarchical.merged.volatility.normalize,
        "merged normalize should be true from repo-local config"
    );
    assert!(
        !hierarchical.merged.general.verbose,
        "merged verbose should be false from user config (no override)"
    );

    // Verify individual source values
    assert_eq!(
        hierarchical.sources[0].config.statement_count.threshold, 10,
        "user config should have threshold 10"
    );
    assert_eq!(
        hierarchical.sources[1].config.statement_count.threshold, 20,
        "repo-local config should have threshold 20"
    );
    assert_eq!(
        hierarchical.sources[2].config.statement_count.threshold, 30,
        "traditional config should have threshold 30"
    );
}

/// Test real git repository scenario with repo-local config.
///
/// This test verifies that repo-local configuration is correctly detected
/// and loaded when running within a git repository.
#[test]
#[serial]
fn integration_real_git_repository_scenario() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let fallback_dir = get_fallback_dir();
    let original_cwd = std::env::current_dir().unwrap_or(fallback_dir.clone());
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Initialize a real git repository
    init_git_repo(temp_dir.path());

    // Verify git repo root is found
    let _git_root = find_git_repo_root()
        .expect("find_git_repo_root should succeed")
        .expect("should find git repository root");

    // Change to temp directory to test git repo detection
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    let git_root_in_repo = find_git_repo_root()
        .expect("find_git_repo_root should succeed in git repo")
        .expect("should find git repository root in temp dir");

    // Verify the found root matches our temp directory
    // Canonicalize both paths to handle macOS /private symlinks
    let git_root_canonical = git_root_in_repo
        .canonicalize()
        .expect("Failed to canonicalize git root");
    let temp_dir_canonical = temp_dir
        .path()
        .canonicalize()
        .expect("Failed to canonicalize temp dir");
    assert_eq!(
        git_root_canonical, temp_dir_canonical,
        "git repo root should match temp directory"
    );

    // Create repo-local config
    let _repo_local_config = create_temp_config_file(
        temp_dir.path(),
        ".raff/raff.local.toml",
        r#"
[statement_count]
threshold = 25

[coupling]
granularity = "module"
"#,
    );

    // Create traditional config to prevent upward discovery issues
    let _traditional_config = create_temp_config_file(
        temp_dir.path(),
        "Raff.toml",
        r#"
[statement_count]
threshold = 10
"#,
    );

    // Load config and verify repo-local is detected
    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd)
        .or_else(|_| std::env::set_current_dir(&fallback_dir));

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical = result.expect("load_hierarchical_config should succeed");

    // Verify repo-local config was loaded
    let repo_local_source = hierarchical
        .sources
        .iter()
        .find(|s| matches!(s.source_type, ConfigSourceType::RepoLocal));

    assert!(
        repo_local_source.is_some(),
        "should load repo-local config in git repository"
    );

    let repo_local_source = repo_local_source.unwrap();

    // Verify repo-local config values
    assert_eq!(
        repo_local_source.config.statement_count.threshold, 25,
        "repo-local config should have threshold 25"
    );
    assert_eq!(
        repo_local_source.config.coupling.granularity.as_deref(),
        Some("module"),
        "repo-local config should have module granularity"
    );
}

/// Test backward compatibility with Raff.toml.
///
/// This test verifies that traditional Raff.toml files continue to work
/// as expected, maintaining backward compatibility with existing configurations.
#[test]
#[serial]
fn integration_backward_compatibility_with_raff_toml() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let fallback_dir = get_fallback_dir();
    let original_cwd = std::env::current_dir().unwrap_or(fallback_dir.clone());
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Set XDG_CONFIG_HOME to a directory without raff config
    let xdg_config = temp_dir.path().join("xdg-config");
    fs::create_dir_all(&xdg_config).expect("Failed to create xdg config dir");
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Create a traditional Raff.toml with various settings
    let raff_toml_content = r#"
[general]
verbose = true

[statement_count]
threshold = 50

[volatility]
alpha = 0.05
since = "2023-01-01"
normalize = true

[coupling]
granularity = "function"

[rust_code_analysis]
jobs = 4
metrics = true

[contributor_report]
since = "2022-01-01"
decay = 0.9
"#;

    let _raff_toml_path = create_temp_config_file(temp_dir.path(), "Raff.toml", raff_toml_content);

    // Change to temp directory
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    // Load config without specifying explicit path (should discover Raff.toml)
    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd)
        .or_else(|_| std::env::set_current_dir(&fallback_dir));

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical = result.expect("load_hierarchical_config should succeed with Raff.toml");

    // Verify traditional local config was loaded
    let traditional_source = hierarchical
        .sources
        .iter()
        .find(|s| matches!(s.source_type, ConfigSourceType::TraditionalLocal));

    assert!(
        traditional_source.is_some(),
        "should discover and load traditional Raff.toml config"
    );

    // Verify all settings from Raff.toml are present
    assert!(
        hierarchical.merged.general.verbose,
        "should load verbose setting from Raff.toml"
    );
    assert_eq!(
        hierarchical.merged.statement_count.threshold, 50,
        "should load statement_count threshold from Raff.toml"
    );
    assert_eq!(
        hierarchical.merged.volatility.alpha, 0.05,
        "should load volatility alpha from Raff.toml"
    );
    assert_eq!(
        hierarchical.merged.volatility.since.as_deref(),
        Some("2023-01-01"),
        "should load volatility since from Raff.toml"
    );
    assert!(
        hierarchical.merged.volatility.normalize,
        "should load volatility normalize from Raff.toml"
    );
    assert_eq!(
        hierarchical.merged.coupling.granularity.as_deref(),
        Some("function"),
        "should load coupling granularity from Raff.toml"
    );
    assert_eq!(
        hierarchical.merged.rust_code_analysis.jobs,
        Some(4),
        "should load rust_code_analysis jobs from Raff.toml"
    );
    assert!(
        hierarchical.merged.rust_code_analysis.metrics,
        "should load rust_code_analysis metrics from Raff.toml"
    );
    assert_eq!(
        hierarchical.merged.contributor_report.decay, 0.9,
        "should load contributor_report decay from Raff.toml"
    );

    // Verify the config path points to Raff.toml
    if let Some(source) = traditional_source {
        assert!(
            source.path.ends_with("Raff.toml"),
            "traditional config path should end with Raff.toml, got: {}",
            source.path.display()
        );
    }
}

/// Test that CLI explicit path bypasses hierarchical discovery.
///
/// This test verifies that when a user specifies an explicit config path
/// via CLI, the hierarchical discovery is bypassed and only that file is loaded.
#[test]
#[serial]
fn integration_cli_explicit_path_bypasses_hierarchy() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let fallback_dir = get_fallback_dir();
    let original_cwd = std::env::current_dir().unwrap_or(fallback_dir.clone());
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Create user config (should be ignored)
    let xdg_config = temp_dir.path().join("xdg-config");
    let _user_config_path = create_temp_config_file(
        &xdg_config,
        "raff/raff.toml",
        r#"
[statement_count]
threshold = 100
"#,
    );
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Initialize git repo and create repo-local config (should be ignored)
    init_git_repo(temp_dir.path());
    let _repo_local_config = create_temp_config_file(
        temp_dir.path(),
        ".raff/raff.local.toml",
        r#"
[statement_count]
threshold = 200
"#,
    );

    // Create traditional config (should be ignored)
    let _traditional_config = create_temp_config_file(
        temp_dir.path(),
        "Raff.toml",
        r#"
[statement_count]
threshold = 300
"#,
    );

    // Create explicit CLI config
    let explicit_config = create_temp_config_file(
        temp_dir.path(),
        "custom.toml",
        r#"
[statement_count]
threshold = 42

[volatility]
alpha = 0.99
"#,
    );

    // Change to temp directory
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    // Load config with explicit CLI path
    let result = load_hierarchical_config(Some(&explicit_config));

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd)
        .or_else(|_| std::env::set_current_dir(&fallback_dir));

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical = result.expect("load_hierarchical_config should succeed");

    // Verify only the CLI explicit source was loaded
    assert_eq!(
        hierarchical.sources.len(),
        1,
        "should load only 1 config source when CLI path is explicit"
    );
    assert!(
        matches!(
            hierarchical.sources[0].source_type,
            ConfigSourceType::CliExplicit
        ),
        "source type should be CliExplicit"
    );

    // Verify the values from the explicit config (not user/repo/traditional)
    assert_eq!(
        hierarchical.merged.statement_count.threshold, 42,
        "should use CLI explicit threshold (42), not user (100), repo (200), or traditional (300)"
    );
    assert_eq!(
        hierarchical.merged.volatility.alpha, 0.99,
        "should use CLI explicit alpha value"
    );
}

/// Test config priority: user < repo-local < traditional.
///
/// This test explicitly verifies the priority order by creating configs
/// with different values for the same key and checking which value wins.
#[test]
#[serial]
fn integration_config_priority_order() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let fallback_dir = get_fallback_dir();
    let original_cwd = std::env::current_dir().unwrap_or(fallback_dir.clone());
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Initialize git repo
    init_git_repo(temp_dir.path());

    // User config: threshold = 10, alpha = 0.01
    let xdg_config = temp_dir.path().join("xdg-config");
    let _user_config = create_temp_config_file(
        &xdg_config,
        "raff/raff.toml",
        r#"
[statement_count]
threshold = 10

[volatility]
alpha = 0.01
"#,
    );
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Repo-local config: threshold = 20, (no alpha)
    let _repo_config = create_temp_config_file(
        temp_dir.path(),
        ".raff/raff.local.toml",
        r#"
[statement_count]
threshold = 20
"#,
    );

    // Traditional config: threshold = 30 (highest priority), alpha = 0.03
    let _traditional_config = create_temp_config_file(
        temp_dir.path(),
        "Raff.toml",
        r#"
[statement_count]
threshold = 30

[volatility]
alpha = 0.03
"#,
    );

    // Change to temp directory
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd)
        .or_else(|_| std::env::set_current_dir(&fallback_dir));

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical = result.expect("load_hierarchical_config should succeed");

    // Verify priority: traditional > repo-local > user
    // threshold: user=10, repo=20, traditional=30 -> should use traditional's 30
    assert_eq!(
        hierarchical.merged.statement_count.threshold, 30,
        "threshold should be from traditional (30), not repo (20) or user (10)"
    );

    // alpha: user=0.01, repo=(none), traditional=0.03 -> should use traditional's 0.03
    assert_eq!(
        hierarchical.merged.volatility.alpha, 0.03,
        "alpha should be from traditional (0.03), not user (0.01)"
    );
}

/// Test behavior when no config files exist.
///
/// This test verifies that the system gracefully handles the case where
/// no config files exist at any level of the hierarchy.
#[test]
#[serial]
fn integration_no_config_files_uses_defaults() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let fallback_dir = get_fallback_dir();
    let original_cwd = std::env::current_dir().unwrap_or(fallback_dir.clone());
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Set XDG_CONFIG_HOME to empty directory
    let xdg_config = temp_dir.path().join("xdg-config");
    fs::create_dir_all(&xdg_config).expect("Failed to create xdg config dir");
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Initialize git repo but don't create any config files
    init_git_repo(temp_dir.path());

    // Change to temp directory (no config files exist)
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd)
        .or_else(|_| std::env::set_current_dir(&fallback_dir));

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical =
        result.expect("load_hierarchical_config should succeed even with no configs");

    // Should have no sources (or only discovered from parent dirs, which we'll ignore)
    // The important thing is that it succeeds and returns default config
    assert!(
        hierarchical.merged.general.path.is_none(),
        "default config should have no path set"
    );

    // Verify default values are present
    assert_eq!(
        hierarchical.merged.statement_count.threshold, 10,
        "default statement_count threshold should be 10"
    );
    assert_eq!(
        hierarchical.merged.volatility.alpha, 0.01,
        "default volatility alpha should be 0.01"
    );
}

/// Test .raff.toml alternative file name.
///
/// This test verifies backward compatibility with the .raff.toml file name.
#[test]
#[serial]
fn integration_backward_compatibility_with_dot_raff_toml() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let fallback_dir = get_fallback_dir();
    let original_cwd = std::env::current_dir().unwrap_or(fallback_dir.clone());
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Set XDG_CONFIG_HOME to empty directory
    let xdg_config = temp_dir.path().join("xdg-config");
    fs::create_dir_all(&xdg_config).expect("Failed to create xdg config dir");
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Create .raff.toml file
    let _dot_raff_toml = create_temp_config_file(
        temp_dir.path(),
        ".raff.toml",
        r#"
[statement_count]
threshold = 77

[volatility]
alpha = 0.07
"#,
    );

    // Change to temp directory
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd)
        .or_else(|_| std::env::set_current_dir(&fallback_dir));

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical = result.expect("load_hierarchical_config should discover .raff.toml");

    // Verify .raff.toml was loaded by checking for a source with that filename
    let dot_raff_source = hierarchical
        .sources
        .iter()
        .find(|s| s.path.file_name() == Some(std::ffi::OsStr::new(".raff.toml")));
    assert!(
        dot_raff_source.is_some(),
        "should discover and load .raff.toml, found sources: {:?}",
        hierarchical
            .sources
            .iter()
            .map(|s| s.path.file_name())
            .collect::<Vec<_>>()
    );

    assert_eq!(
        hierarchical.merged.statement_count.threshold, 77,
        "should load threshold from .raff.toml"
    );
    assert_eq!(
        hierarchical.merged.volatility.alpha, 0.07,
        "should load alpha from .raff.toml"
    );
}

/// Test behavior when outside a git repository.
///
/// This test verifies that repo-local config is not loaded when
/// the current directory is not in a git repository.
#[test]
#[serial]
fn integration_outside_git_repo_no_repo_local_config() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let fallback_dir = get_fallback_dir();
    let original_cwd = std::env::current_dir().unwrap_or(fallback_dir.clone());
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Set XDG_CONFIG_HOME to empty directory
    let xdg_config = temp_dir.path().join("xdg-config");
    fs::create_dir_all(&xdg_config).expect("Failed to create xdg config dir");
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Do NOT initialize git repo (we're testing outside git)

    // Change to temp directory (not a git repo)
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    // Verify git repo root is not found
    let git_root = find_git_repo_root().expect("find_git_repo_root should succeed");
    assert!(
        git_root.is_none(),
        "git repo root should be None when outside git repository"
    );

    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd)
        .or_else(|_| std::env::set_current_dir(&fallback_dir));

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical = result.expect("load_hierarchical_config should succeed outside git repo");

    // Verify repo-local config was NOT loaded
    assert!(
        !hierarchical
            .sources
            .iter()
            .any(|s| matches!(s.source_type, ConfigSourceType::RepoLocal)),
        "should not load repo-local config when outside git repository"
    );
}
