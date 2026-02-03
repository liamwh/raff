//! Git utility functions for Raff.
//!
//! This module provides utilities for interacting with Git repositories,
//! particularly for discovering staged files for pre-commit hook analysis.

use crate::error::{RaffError, Result};
use std::path::PathBuf;
use std::process::Command;

use tracing::instrument;

/// Gets the list of staged files from the Git repository.
///
/// This function runs `git diff --name-only --cached` to discover which files
/// are currently staged for commit. If not in a Git repository or if there are
/// no staged files, returns an empty vector.
///
/// # Returns
///
/// * `Ok(Vec<PathBuf>)` - List of staged file paths
/// * `Err(RaffError)` - Git command execution failed
///
/// # Examples
///
/// ```no_run
/// use raff_core::git_utils;
///
/// # fn main() -> raff_core::error::Result<()> {
/// let staged_files = git_utils::get_staged_files()?;
/// println!("Found {} staged files", staged_files.len());
/// # Ok(())
/// # }
/// ```
#[instrument(skip(), ret, level = "debug")]
pub fn get_staged_files() -> Result<Vec<PathBuf>> {
    let output = Command::new("git")
        .args(["diff", "--name-only", "--cached"])
        .output()
        .map_err(|e| RaffError::git_error(format!("git diff failed: {}", e)))?;

    // Not in git or command failed - return empty vec (fallback to full analysis)
    if !output.status.success() {
        tracing::debug!("git diff --name-only --cached failed or not in git repo");
        return Ok(Vec::new());
    }

    let content = String::from_utf8_lossy(&output.stdout);
    let files: Vec<PathBuf> = content
        .lines()
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect();

    tracing::debug!("Found {} staged files", files.len());
    Ok(files)
}

/// Filters a list of files to only include Rust source files (`.rs` extension).
///
/// # Arguments
///
/// * `files` - Slice of file paths to filter
///
/// # Returns
///
/// A new vector containing only files with `.rs` extension.
///
/// # Examples
///
/// ```
/// use raff_core::git_utils;
/// use std::path::PathBuf;
///
/// # fn main() {
/// let files = vec![
///     PathBuf::from("src/main.rs"),
///     PathBuf::from("README.md"),
///     PathBuf::from("src/lib.rs"),
///     PathBuf::from("Cargo.toml"),
/// ];
///
/// let rust_files = git_utils::filter_rust_files(&files);
/// assert_eq!(rust_files.len(), 2);
/// # }
/// ```
#[must_use]
pub fn filter_rust_files(files: &[PathBuf]) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|p| p.extension().is_some_and(|ext| ext == "rs"))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_filter_rust_files_filters_correctly() {
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("README.md"),
            PathBuf::from("Cargo.toml"),
            PathBuf::from("tests/integration_test.rs"),
            PathBuf::from("build.rs"),
        ];

        let rust_files = filter_rust_files(&files);

        assert_eq!(rust_files.len(), 4);
        assert!(rust_files.contains(&PathBuf::from("src/main.rs")));
        assert!(rust_files.contains(&PathBuf::from("src/lib.rs")));
        assert!(rust_files.contains(&PathBuf::from("tests/integration_test.rs")));
        assert!(rust_files.contains(&PathBuf::from("build.rs")));
        assert!(!rust_files.contains(&PathBuf::from("README.md")));
        assert!(!rust_files.contains(&PathBuf::from("Cargo.toml")));
    }

    #[test]
    fn test_filter_rust_files_empty_input() {
        let files = vec![];
        let rust_files = filter_rust_files(&files);
        assert!(rust_files.is_empty());
    }

    #[test]
    fn test_filter_rust_files_no_rust_files() {
        let files = vec![
            PathBuf::from("README.md"),
            PathBuf::from("Cargo.toml"),
            PathBuf::from(".gitignore"),
        ];

        let rust_files = filter_rust_files(&files);
        assert!(rust_files.is_empty());
    }

    #[test]
    fn test_filter_rust_files_all_rust_files() {
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("tests/test.rs"),
        ];

        let rust_files = filter_rust_files(&files);
        assert_eq!(rust_files.len(), 3);
    }

    #[test]
    fn test_filter_rust_files_preserves_paths() {
        let files = vec![
            PathBuf::from("deeply/nested/path/module.rs"),
            PathBuf::from("src/ffi/c_wrapper.rs"),
        ];

        let rust_files = filter_rust_files(&files);
        assert_eq!(rust_files.len(), 2);
        assert!(rust_files.contains(&PathBuf::from("deeply/nested/path/module.rs")));
        assert!(rust_files.contains(&PathBuf::from("src/ffi/c_wrapper.rs")));
    }

    #[test]
    fn test_filter_rust_files_case_sensitive() {
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/main.RS"), // Uppercase extension
            PathBuf::from("src/main.Rs"), // Mixed case extension
        ];

        let rust_files = filter_rust_files(&files);

        // Only lowercase .rs should match
        assert_eq!(rust_files.len(), 1);
        assert!(rust_files.contains(&PathBuf::from("src/main.rs")));
    }

    #[test]
    fn test_filter_rust_files_with_no_extension() {
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/Makefile"), // No extension
            PathBuf::from("README"),       // No extension
        ];

        let rust_files = filter_rust_files(&files);

        assert_eq!(rust_files.len(), 1);
        assert!(rust_files.contains(&PathBuf::from("src/main.rs")));
    }

    #[test]
    fn test_get_staged_files_returns_empty_when_not_in_git() {
        // Create a temp directory that is not a git repo
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Change to the non-git directory
        let original_dir = std::env::current_dir().expect("Failed to get current dir");
        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        // get_staged_files should return Ok with empty vec when not in git
        let result = get_staged_files();

        // Restore original directory
        std::env::set_current_dir(original_dir).expect("Failed to restore dir");

        assert!(
            result.is_ok(),
            "get_staged_files should not error when not in a git repo"
        );
        let files = result.unwrap();
        assert!(
            files.is_empty(),
            "get_staged_files should return empty vec when not in git"
        );
    }

    #[test]
    fn test_get_staged_files_returns_empty_when_no_staged_files() {
        // Create a temp git repo with no staged files
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

        // Configure git user for commits
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp_dir.path())
            .status();

        let _ = Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(temp_dir.path())
            .status();

        // Change to the git repo directory
        let original_dir = std::env::current_dir().expect("Failed to get current dir");
        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        // get_staged_files should return empty vec when no files are staged
        let result = get_staged_files();

        // Restore original directory
        std::env::set_current_dir(original_dir).expect("Failed to restore dir");

        assert!(result.is_ok(), "get_staged_files should succeed");
        let files = result.unwrap();
        assert!(
            files.is_empty(),
            "get_staged_files should return empty vec when no files are staged"
        );
    }

    #[test]
    fn test_get_staged_files_detects_staged_files() {
        // Create a temp git repo with a staged file
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

        // Configure git user for commits
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp_dir.path())
            .status();

        let _ = Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(temp_dir.path())
            .status();

        // Create and stage a file
        let file_path = temp_dir.path().join("test.rs");
        let mut file = fs::File::create(&file_path).expect("Failed to create file");
        file.write_all(b"fn main() {}")
            .expect("Failed to write file");

        let _ = Command::new("git")
            .args(["add", "test.rs"])
            .current_dir(temp_dir.path())
            .status();

        // Change to the git repo directory
        let original_dir = std::env::current_dir().expect("Failed to get current dir");
        std::env::set_current_dir(temp_dir.path()).expect("Failed to change dir");

        // get_staged_files should detect the staged file
        let result = get_staged_files();

        // Restore original directory
        std::env::set_current_dir(original_dir).expect("Failed to restore dir");

        assert!(result.is_ok(), "get_staged_files should succeed");
        let files = result.unwrap();
        assert_eq!(
            files.len(),
            1,
            "get_staged_files should find one staged file"
        );
        assert_eq!(
            files[0],
            PathBuf::from("test.rs"),
            "get_staged_files should return the correct file name"
        );
    }
}
