//! Snapshot tests for hierarchical configuration loading.
//!
//! These tests use insta to capture the behavior of hierarchical configuration
//! loading across various scenarios.
//!
//! Note: These tests modify the current directory and environment variables,
//! so they should be run with `--test-threads=1` for reliable results.

use raff_core::config_hierarchy::{load_hierarchical_config, ConfigSourceType, HierarchicalConfig};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Helper to create a temporary config file with content.
fn create_temp_config_file(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
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

    assert!(status.success(), "git init failed");

    // Configure git to allow commits
    let status = Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .expect("Failed to configure git user.email");

    assert!(status.success(), "git config user.email failed");

    let status = Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .status()
        .expect("Failed to configure git user.name");

    assert!(status.success(), "git config user.name failed");
}

/// Helper to format a HierarchicalConfig for snapshot testing.
fn format_hierarchical_config(hierarchical: &HierarchicalConfig) -> String {
    let mut output = String::new();

    output.push_str("## Sources\n");
    for (i, source) in hierarchical.sources.iter().enumerate() {
        // Use only the filename for stable snapshots
        let filename = source
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>");

        output.push_str(&format!(
            "  {}. {:?} from {}\n",
            i + 1,
            source.source_type,
            filename
        ));
        output.push_str(&format!(
            "     statement_count.threshold: {}\n",
            source.config.statement_count.threshold
        ));
        output.push_str(&format!(
            "     volatility.alpha: {}\n",
            source.config.volatility.alpha
        ));
        output.push_str(&format!(
            "     general.verbose: {}\n",
            source.config.general.verbose
        ));
    }

    output.push_str("\n## Merged Configuration\n");
    output.push_str(&format!(
        "  statement_count.threshold: {}\n",
        hierarchical.merged.statement_count.threshold
    ));
    output.push_str(&format!(
        "  volatility.alpha: {}\n",
        hierarchical.merged.volatility.alpha
    ));
    output.push_str(&format!(
        "  general.verbose: {}\n",
        hierarchical.merged.general.verbose
    ));
    output.push_str(&format!(
        "  volatility.normalize: {}\n",
        hierarchical.merged.volatility.normalize
    ));
    output.push_str(&format!(
        "  coupling.granularity: {:?}\n",
        hierarchical.merged.coupling.granularity
    ));

    output
}

#[test]
fn snapshot_full_hierarchy_loading() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let original_cwd = std::env::current_dir().expect("Failed to get cwd");
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Initialize git repo so repo-local config can be found
    init_git_repo(temp_dir.path());

    // Create user config directory structure
    let xdg_config = temp_dir.path().join("xdg-config");
    let _user_config_path = create_temp_config_file(
        &xdg_config,
        "raff/raff.toml",
        r#"
[general]
verbose = true

[statement_count]
threshold = 15

[volatility]
alpha = 0.02
"#,
    );

    // Set XDG_CONFIG_HOME
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Create repo-local config
    let _repo_local_config = create_temp_config_file(
        temp_dir.path(),
        ".raff/raff.local.toml",
        r#"
[statement_count]
threshold = 20

[volatility]
alpha = 0.03
normalize = true
"#,
    );

    // Create traditional config
    let _traditional_config = create_temp_config_file(
        temp_dir.path(),
        "Raff.toml",
        r#"
[statement_count]
threshold = 25

[coupling]
granularity = "module"
"#,
    );

    // Change to temp directory
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    // Load config - should pick up user, repo-local, and traditional configs
    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd);

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical = result.expect("load_hierarchical_config should succeed");

    // Verify all three sources were loaded
    assert_eq!(
        hierarchical.sources.len(),
        3,
        "should load 3 config sources"
    );

    // Verify source types
    assert!(matches!(
        hierarchical.sources[0].source_type,
        ConfigSourceType::User
    ));
    assert!(matches!(
        hierarchical.sources[1].source_type,
        ConfigSourceType::RepoLocal
    ));
    assert!(matches!(
        hierarchical.sources[2].source_type,
        ConfigSourceType::TraditionalLocal
    ));

    // Snapshot the full hierarchical configuration
    let formatted = format_hierarchical_config(&hierarchical);
    insta::assert_snapshot!(formatted);
}

#[test]
fn snapshot_partial_hierarchy_user_and_repo_with_possible_traditional() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let original_cwd = std::env::current_dir().expect("Failed to get cwd");
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Initialize git repo so repo-local config can be found
    init_git_repo(temp_dir.path());

    // Create user config directory structure
    let xdg_config = temp_dir.path().join("xdg-config");
    let _user_config_path = create_temp_config_file(
        &xdg_config,
        "raff/raff.toml",
        r#"
[general]
verbose = false

[statement_count]
threshold = 12

[volatility]
alpha = 0.015
"#,
    );

    // Set XDG_CONFIG_HOME
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Create repo-local config (no traditional config)
    let _repo_local_config = create_temp_config_file(
        temp_dir.path(),
        ".raff/raff.local.toml",
        r#"
[statement_count]
threshold = 18
"#,
    );

    // Change to temp directory
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    // Load config - should pick up user and repo-local configs only
    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd);

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical = result.expect("load_hierarchical_config should succeed");

    // Verify at least two sources were loaded (user + repo-local)
    // Note: May also discover traditional config from parent directories
    assert!(
        hierarchical.sources.len() >= 2,
        "should load at least 2 config sources, got {}",
        hierarchical.sources.len()
    );

    // Verify user and repo-local sources are present
    assert!(hierarchical
        .sources
        .iter()
        .any(|s| matches!(s.source_type, ConfigSourceType::User)));
    assert!(hierarchical
        .sources
        .iter()
        .any(|s| matches!(s.source_type, ConfigSourceType::RepoLocal)));

    // Snapshot the partial hierarchical configuration
    let formatted = format_hierarchical_config(&hierarchical);
    insta::assert_snapshot!(formatted);
}

#[test]
fn snapshot_cli_explicit_path_override() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let original_cwd = std::env::current_dir().expect("Failed to get cwd");
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Create user config (should be ignored when CLI specifies explicit path)
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

    // Create repo-local config (should also be ignored)
    let _repo_local_config = create_temp_config_file(
        temp_dir.path(),
        ".raff/raff.local.toml",
        r#"
[statement_count]
threshold = 200
"#,
    );

    // Create explicit CLI config
    let explicit_config = create_temp_config_file(
        temp_dir.path(),
        "cli-explicit.toml",
        r#"
[general]
verbose = true

[statement_count]
threshold = 99

[volatility]
alpha = 0.08
skip_merges = true

[coupling]
granularity = "crate"
"#,
    );

    // Change to temp directory
    std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");

    // Load config with explicit CLI path - should bypass hierarchy
    let result = load_hierarchical_config(Some(&explicit_config));

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd);

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
        "should load only 1 config source"
    );
    assert!(matches!(
        hierarchical.sources[0].source_type,
        ConfigSourceType::CliExplicit
    ));

    // Verify the explicit config values (not user/repo values)
    assert_eq!(
        hierarchical.merged.statement_count.threshold, 99,
        "should use CLI explicit threshold, not user (100) or repo (200)"
    );

    // Snapshot the CLI explicit hierarchical configuration
    let formatted = format_hierarchical_config(&hierarchical);
    insta::assert_snapshot!(formatted);
}

#[test]
fn snapshot_default_config_when_no_user_or_repo_config() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let original_cwd = std::env::current_dir().expect("Failed to get cwd");
    let original_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // Set XDG_CONFIG_HOME to a directory with no config
    let xdg_config = temp_dir.path().join("xdg-config");
    fs::create_dir_all(&xdg_config).expect("Failed to create xdg config dir");
    std::env::set_var("XDG_CONFIG_HOME", xdg_config);

    // Initialize a git repo in a different directory so repo-local config won't be found
    let git_temp_dir = TempDir::new().expect("Failed to create git temp dir");
    init_git_repo(git_temp_dir.path());

    // Change to git temp directory (has git repo but no config files)
    std::env::set_current_dir(git_temp_dir.path()).expect("Failed to cd to git temp dir");

    // Load config with no configs available in the immediate directory
    let result = load_hierarchical_config(None);

    // Restore original directory
    let _ = std::env::set_current_dir(&original_cwd);

    // Restore original environment
    if let Some(xdg) = original_xdg {
        std::env::set_var("XDG_CONFIG_HOME", xdg);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let hierarchical = result.expect("load_hierarchical_config should succeed");

    // Should not load repo-local config when none exists in the git repo
    assert!(
        !hierarchical
            .sources
            .iter()
            .any(|s| matches!(s.source_type, ConfigSourceType::RepoLocal)),
        "should not load repo-local config when none exists"
    );

    // Snapshot the default hierarchical configuration (should have defaults)
    let formatted = format_hierarchical_config(&hierarchical);
    insta::assert_snapshot!(formatted);
}
