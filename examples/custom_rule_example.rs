//! Example of implementing a custom analysis rule using the Raff Rule trait.
//!
//! This example demonstrates how users can create their own fitness functions
//! by implementing the `Rule` trait from the `raff_core` crate.
//!
//! The custom rule implemented here is a "File Size Rule" that analyzes
//! the size of source files in a codebase and reports any that exceed
//! a configured threshold.

use raff_core::error::{RaffError, Result};
use raff_core::file_utils;
use raff_core::rule::Rule;
use serde::Serialize;
use std::path::PathBuf;

/// Configuration for the FileSizeRule.
///
/// This struct defines the configuration parameters that our custom rule accepts.
#[derive(Clone, Debug)]
pub struct FileSizeConfig {
    /// The path to analyze.
    pub path: PathBuf,
    /// Maximum allowed file size in bytes.
    pub max_size_bytes: u64,
}

/// Analysis data produced by the FileSizeRule.
///
/// This struct contains the results of our analysis and can be serialized
/// to JSON or other formats.
#[derive(Debug, Serialize, PartialEq)]
#[serde(crate = "serde")]
pub struct FileSizeData {
    /// Total number of files analyzed.
    pub total_files: usize,
    /// Files that exceed the size threshold.
    pub oversized_files: Vec<OversizedFile>,
}

/// Information about a file that exceeds the size threshold.
#[derive(Debug, Serialize, PartialEq)]
#[serde(crate = "serde")]
pub struct OversizedFile {
    /// Path to the file relative to the analysis root.
    pub path: String,
    /// Size of the file in bytes.
    pub size_bytes: u64,
    /// How much the file exceeds the threshold.
    pub excess_bytes: u64,
}

/// A custom rule that checks file sizes in the codebase.
///
/// This rule demonstrates the minimum requirements for implementing
/// the `Rule` trait: defining Config and Data associated types,
/// providing name and description, and implementing run() and analyze().
#[derive(Debug, Default)]
pub struct FileSizeRule;

impl Rule for FileSizeRule {
    type Config = FileSizeConfig;
    type Data = FileSizeData;

    fn name() -> &'static str {
        "file_size"
    }

    fn description() -> &'static str {
        "Analyzes file sizes and reports files exceeding the threshold"
    }

    fn run(&self, config: &Self::Config) -> Result<()> {
        let data = self.analyze(config)?;

        // Print results in a human-readable format
        println!("File Size Analysis");
        println!("==================");
        println!("Path: {}", config.path.display());
        println!("Max size: {} bytes", config.max_size_bytes);
        println!("Total files: {}", data.total_files);
        println!();

        if data.oversized_files.is_empty() {
            println!("✓ All files are within the size limit.");
        } else {
            println!(
                "✗ {} files exceed the size limit:",
                data.oversized_files.len()
            );
            println!();

            for file in &data.oversized_files {
                println!(
                    "  {} ({} bytes, {} over limit)",
                    file.path, file.size_bytes, file.excess_bytes
                );
            }

            return Err(RaffError::analysis_error(
                Self::name(),
                format!(
                    "{} files exceed the maximum size limit",
                    data.oversized_files.len()
                ),
            ));
        }

        Ok(())
    }

    fn analyze(&self, config: &Self::Config) -> Result<Self::Data> {
        let mut source_files = Vec::new();
        file_utils::collect_all_rs(&config.path, &mut source_files, None)?;

        let mut oversized_files = Vec::new();

        for file_path in source_files {
            // Get the file metadata
            let metadata = std::fs::metadata(&file_path).map_err(|e| {
                RaffError::io_error_with_source(
                    format!("Failed to read file metadata for {}", file_path.display()),
                    file_path.clone(),
                    e,
                )
            })?;

            let file_size = metadata.len();

            if file_size > config.max_size_bytes {
                // Get the relative path for cleaner output
                let relative_path = file_path
                    .strip_prefix(&config.path)
                    .unwrap_or(&file_path)
                    .to_string_lossy()
                    .to_string();

                oversized_files.push(OversizedFile {
                    path: relative_path,
                    size_bytes: file_size,
                    excess_bytes: file_size - config.max_size_bytes,
                });
            }
        }

        // Sort by excess (largest first)
        oversized_files.sort_by(|a, b| b.excess_bytes.cmp(&a.excess_bytes));

        let mut all_files = Vec::new();
        file_utils::collect_all_rs(&config.path, &mut all_files, None)?;

        Ok(FileSizeData {
            total_files: all_files.len(),
            oversized_files,
        })
    }
}

fn main() -> Result<()> {
    let config = FileSizeConfig {
        path: PathBuf::from("."),
        max_size_bytes: 10_000, // 10 KB
    };

    let rule = FileSizeRule;
    rule.run(&config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_name_returns_correct_name() {
        assert_eq!(FileSizeRule::name(), "file_size");
    }

    #[test]
    fn test_rule_description_returns_description() {
        assert_eq!(
            FileSizeRule::description(),
            "Analyzes file sizes and reports files exceeding the threshold"
        );
    }

    #[test]
    fn test_rule_run_succeeds_with_valid_path() {
        let rule = FileSizeRule;
        let config = FileSizeConfig {
            path: PathBuf::from("./src"),
            max_size_bytes: 100_000, // High threshold to avoid errors
        };

        let result = rule.run(&config);
        assert!(result.is_ok(), "run should succeed with valid path");
    }

    #[test]
    fn test_rule_analyze_succeeds_with_valid_path() {
        let rule = FileSizeRule;
        let config = FileSizeConfig {
            path: PathBuf::from("./src"),
            max_size_bytes: 100_000,
        };

        let result = rule.analyze(&config);
        assert!(result.is_ok(), "analyze should succeed with valid path");

        let data = result.unwrap();
        assert!(data.total_files > 0, "should analyze at least one file");
    }

    #[test]
    fn test_rule_analyze_fails_with_nonexistent_path() {
        let rule = FileSizeRule;
        let config = FileSizeConfig {
            path: PathBuf::from("/nonexistent/path/that/does/not/exist"),
            max_size_bytes: 100_000,
        };

        let result = rule.analyze(&config);
        assert!(result.is_err(), "analyze should fail with nonexistent path");
    }

    #[test]
    fn test_rule_run_fails_when_files_exceed_threshold() {
        let rule = FileSizeRule;
        let config = FileSizeConfig {
            path: PathBuf::from("./src"),
            max_size_bytes: 1, // Very low threshold to trigger failure
        };

        let result = rule.run(&config);
        assert!(
            result.is_err(),
            "run should fail when files exceed threshold"
        );

        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("file_size"),
            "error should mention rule name"
        );
        assert!(
            error_msg.contains("exceed") || error_msg.contains("limit"),
            "error should mention threshold exceeded"
        );
    }

    #[test]
    fn test_file_size_data_is_serializable() {
        let data = FileSizeData {
            total_files: 10,
            oversized_files: vec![OversizedFile {
                path: "src/main.rs".to_string(),
                size_bytes: 15_000,
                excess_bytes: 5_000,
            }],
        };

        let json = serde_json::to_string(&data);
        assert!(json.is_ok(), "FileSizeData should be serializable to JSON");

        let json_str = json.unwrap();
        assert!(
            json_str.contains("total_files"),
            "JSON should contain total_files field"
        );
        assert!(
            json_str.contains("oversized_files"),
            "JSON should contain oversized_files field"
        );
    }

    #[test]
    fn test_oversized_file_is_serializable() {
        let file = OversizedFile {
            path: "src/lib.rs".to_string(),
            size_bytes: 20_000,
            excess_bytes: 10_000,
        };

        let json = serde_json::to_string(&file);
        assert!(json.is_ok(), "OversizedFile should be serializable to JSON");

        let json_str = json.unwrap();
        assert!(
            json_str.contains("src/lib.rs"),
            "JSON should contain file path"
        );
        assert!(json_str.contains("20000"), "JSON should contain file size");
    }

    #[test]
    fn test_file_size_config_is_cloneable() {
        let config = FileSizeConfig {
            path: PathBuf::from("./src"),
            max_size_bytes: 10_000,
        };

        let cloned = config.clone();
        assert_eq!(config.path, cloned.path);
        assert_eq!(config.max_size_bytes, cloned.max_size_bytes);
    }
}
