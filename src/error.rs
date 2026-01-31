//! Error types for Raff.
//!
//! This module defines a comprehensive error type for the Raff CLI tool,
//! providing specific error variants for different failure modes and enabling
//! programmatic error handling.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// The main error type for Raff operations.
///
/// `RaffError` provides specific error variants for different failure modes,
/// making it possible to programmatically handle different error cases.
#[derive(Debug)]
pub enum RaffError {
    /// An error occurred while parsing or analyzing source code.
    ParseError {
        /// The file that failed to parse.
        file: Option<PathBuf>,
        /// Context about what was being parsed.
        context: String,
        /// The underlying error.
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// An error occurred during a Git operation.
    GitError {
        /// Context about what Git operation was being performed.
        operation: String,
        /// Additional context about the repository.
        repo_path: Option<PathBuf>,
        /// The underlying error.
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// An error occurred during file system operations.
    IoError {
        /// The operation being performed.
        operation: String,
        /// The path involved in the error.
        path: Option<PathBuf>,
        /// The underlying IO error.
        source: Option<io::Error>,
    },

    /// An error occurred while loading or parsing configuration.
    ConfigError {
        /// Description of the configuration issue.
        message: String,
        /// The config file path, if applicable.
        path: Option<PathBuf>,
        /// The underlying error.
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// An error occurred during analysis.
    AnalysisError {
        /// The analysis rule that failed.
        rule: String,
        /// Description of what went wrong.
        message: String,
        /// The underlying error.
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// An error indicating an invalid argument or input.
    InvalidInput {
        /// Description of the invalid input.
        message: String,
        /// The argument or value that was invalid.
        argument: Option<String>,
    },
}

impl RaffError {
    /// Creates a new `ParseError` with the given context.
    ///
    /// # Arguments
    /// * `context` - A description of what was being parsed.
    ///
    /// # Examples
    /// ```
    /// use raff::error::RaffError;
    ///
    /// let err = RaffError::parse_error("Failed to parse Rust file");
    /// ```
    pub fn parse_error(context: impl Into<String>) -> Self {
        Self::ParseError {
            file: None,
            context: context.into(),
            source: None,
        }
    }

    /// Creates a new `ParseError` with a file path.
    ///
    /// # Arguments
    /// * `file` - The path to the file that failed to parse.
    /// * `context` - A description of what was being parsed.
    pub fn parse_error_with_file(file: PathBuf, context: impl Into<String>) -> Self {
        Self::ParseError {
            file: Some(file),
            context: context.into(),
            source: None,
        }
    }

    /// Creates a new `GitError` with the given operation description.
    ///
    /// # Arguments
    /// * `operation` - A description of the Git operation being performed.
    pub fn git_error(operation: impl Into<String>) -> Self {
        Self::GitError {
            operation: operation.into(),
            repo_path: None,
            source: None,
        }
    }

    /// Creates a new `GitError` with a repository path.
    ///
    /// # Arguments
    /// * `operation` - A description of the Git operation being performed.
    /// * `repo_path` - The path to the repository.
    pub fn git_error_with_repo(operation: impl Into<String>, repo_path: PathBuf) -> Self {
        Self::GitError {
            operation: operation.into(),
            repo_path: Some(repo_path),
            source: None,
        }
    }

    /// Creates a new `IoError` with the given operation description.
    ///
    /// # Arguments
    /// * `operation` - A description of the IO operation being performed.
    pub fn io_error(operation: impl Into<String>) -> Self {
        Self::IoError {
            operation: operation.into(),
            path: None,
            source: None,
        }
    }

    /// Creates a new `IoError` with a path and underlying error.
    ///
    /// # Arguments
    /// * `operation` - A description of the IO operation being performed.
    /// * `path` - The path involved in the error.
    /// * `source` - The underlying IO error.
    pub fn io_error_with_source(
        operation: impl Into<String>,
        path: PathBuf,
        source: io::Error,
    ) -> Self {
        Self::IoError {
            operation: operation.into(),
            path: Some(path),
            source: Some(source),
        }
    }

    /// Creates a new `ConfigError` with the given message.
    ///
    /// # Arguments
    /// * `message` - A description of the configuration issue.
    pub fn config_error(message: impl Into<String>) -> Self {
        Self::ConfigError {
            message: message.into(),
            path: None,
            source: None,
        }
    }

    /// Creates a new `ConfigError` with a file path.
    ///
    /// # Arguments
    /// * `message` - A description of the configuration issue.
    /// * `path` - The path to the config file.
    pub fn config_error_with_path(message: impl Into<String>, path: PathBuf) -> Self {
        Self::ConfigError {
            message: message.into(),
            path: Some(path),
            source: None,
        }
    }

    /// Creates a new `AnalysisError` for the given rule.
    ///
    /// # Arguments
    /// * `rule` - The name of the analysis rule that failed.
    /// * `message` - A description of what went wrong.
    pub fn analysis_error(rule: impl Into<String>, message: impl Into<String>) -> Self {
        Self::AnalysisError {
            rule: rule.into(),
            message: message.into(),
            source: None,
        }
    }

    /// Creates a new `InvalidInput` error.
    ///
    /// # Arguments
    /// * `message` - A description of the invalid input.
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
            argument: None,
        }
    }

    /// Creates a new `InvalidInput` error with an argument name.
    ///
    /// # Arguments
    /// * `message` - A description of the invalid input.
    /// * `argument` - The argument or value that was invalid.
    pub fn invalid_input_with_arg(message: impl Into<String>, argument: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
            argument: Some(argument.into()),
        }
    }

    /// Returns the name of the error variant.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ParseError { .. } => "ParseError",
            Self::GitError { .. } => "GitError",
            Self::IoError { .. } => "IoError",
            Self::ConfigError { .. } => "ConfigError",
            Self::AnalysisError { .. } => "AnalysisError",
            Self::InvalidInput { .. } => "InvalidInput",
        }
    }

    /// Returns suggested recovery actions for the error.
    pub fn suggestions(&self) -> Vec<String> {
        match self {
            Self::ParseError { file, .. } => {
                let mut s = vec![
                    "Ensure the file contains valid Rust code".to_string(),
                    "Check that the file is not corrupted".to_string(),
                ];
                if file.is_some() {
                    s.push("Verify the file compiles with `cargo check`".to_string());
                }
                s
            }
            Self::GitError { .. } => vec![
                "Ensure the path is a valid Git repository".to_string(),
                "Check that you have permissions to access the repository".to_string(),
                "Verify Git is installed and accessible".to_string(),
            ],
            Self::IoError { operation, .. } => {
                let mut s = vec![
                    "Check that the path exists and is accessible".to_string(),
                    "Verify you have the necessary permissions".to_string(),
                ];
                if operation.contains("read") || operation.contains("open") {
                    s.push("Ensure the file is not locked by another process".to_string());
                }
                s
            }
            Self::ConfigError { .. } => vec![
                "Check the configuration file syntax".to_string(),
                "Verify all required fields are present".to_string(),
                "Ensure the file is valid TOML format".to_string(),
                "Review the documentation for configuration options".to_string(),
            ],
            Self::AnalysisError { rule, .. } => {
                vec![
                    format!("Ensure the '{}' rule has valid inputs", rule),
                    "Check that the source code is well-formed".to_string(),
                    "Review the rule-specific documentation".to_string(),
                ]
            }
            Self::InvalidInput { .. } => vec![
                "Review the command-line arguments".to_string(),
                "Check the documentation for valid input formats".to_string(),
                "Verify all required arguments are provided".to_string(),
            ],
        }
    }
}

impl fmt::Display for RaffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseError { file, context, .. } => {
                if let Some(file) = file {
                    write!(f, "Parse error in '{}': {}", file.display(), context)
                } else {
                    write!(f, "Parse error: {}", context)
                }
            }
            Self::GitError {
                operation,
                repo_path,
                ..
            } => {
                if let Some(path) = repo_path {
                    write!(
                        f,
                        "Git error during '{}' at '{}': operation failed",
                        operation,
                        path.display()
                    )
                } else {
                    write!(f, "Git error during '{}': operation failed", operation)
                }
            }
            Self::IoError {
                operation, path, ..
            } => {
                if let Some(p) = path {
                    write!(
                        f,
                        "IO error during '{}' at '{}': operation failed",
                        operation,
                        p.display()
                    )
                } else {
                    write!(f, "IO error during '{}': operation failed", operation)
                }
            }
            Self::ConfigError { message, path, .. } => {
                if let Some(p) = path {
                    write!(f, "Configuration error in '{}': {}", p.display(), message)
                } else {
                    write!(f, "Configuration error: {}", message)
                }
            }
            Self::AnalysisError { rule, message, .. } => {
                write!(f, "Analysis error in rule '{}': {}", rule, message)
            }
            Self::InvalidInput { message, argument } => {
                if let Some(arg) = argument {
                    write!(f, "Invalid input '{}': {}", arg, message)
                } else {
                    write!(f, "Invalid input: {}", message)
                }
            }
        }
    }
}

impl std::error::Error for RaffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ParseError { source, .. } => source.as_ref().map(|s| s.as_ref() as _),
            Self::GitError { source, .. } => source.as_ref().map(|s| s.as_ref() as _),
            Self::IoError { source, .. } => source.as_ref().map(|e| e as _),
            Self::ConfigError { source, .. } => source.as_ref().map(|s| s.as_ref() as _),
            Self::AnalysisError { source, .. } => source.as_ref().map(|s| s.as_ref() as _),
            Self::InvalidInput { .. } => None,
        }
    }
}

// Implement From conversions for common error types

impl From<io::Error> for RaffError {
    fn from(err: io::Error) -> Self {
        Self::IoError {
            operation: "file operation".to_string(),
            path: None,
            source: Some(err),
        }
    }
}

impl From<toml::de::Error> for RaffError {
    fn from(err: toml::de::Error) -> Self {
        Self::ConfigError {
            message: format!("Failed to parse TOML: {}", err),
            path: None,
            source: Some(Box::new(err)),
        }
    }
}

impl From<toml::ser::Error> for RaffError {
    fn from(err: toml::ser::Error) -> Self {
        Self::ConfigError {
            message: format!("Failed to serialize TOML: {}", err),
            path: None,
            source: Some(Box::new(err)),
        }
    }
}

impl From<serde_json::Error> for RaffError {
    fn from(err: serde_json::Error) -> Self {
        Self::ConfigError {
            message: format!("Failed to parse/serialize JSON: {}", err),
            path: None,
            source: Some(Box::new(err)),
        }
    }
}

impl From<csv::Error> for RaffError {
    fn from(err: csv::Error) -> Self {
        Self::AnalysisError {
            rule: "csv_output".to_string(),
            message: format!("Failed to write CSV: {}", err),
            source: Some(Box::new(err)),
        }
    }
}

impl From<syn::Error> for RaffError {
    fn from(err: syn::Error) -> Self {
        Self::ParseError {
            file: None,
            context: format!("Failed to parse Rust source: {}", err),
            source: Some(Box::new(err)),
        }
    }
}

impl From<git2::Error> for RaffError {
    fn from(err: git2::Error) -> Self {
        Self::GitError {
            operation: "git operation".to_string(),
            repo_path: None,
            source: Some(Box::new(err)),
        }
    }
}

impl From<walkdir::Error> for RaffError {
    fn from(err: walkdir::Error) -> Self {
        Self::IoError {
            operation: "directory traversal".to_string(),
            path: err.path().map(PathBuf::from),
            source: None,
        }
    }
}

/// A type alias for `Result<T, RaffError>`.
///
/// This is the recommended return type for functions that can fail with Raff-specific errors.
pub type Result<T> = std::result::Result<T, RaffError>;

#[cfg(test)]
mod tests {
    use super::*;

    // Constructor tests

    #[test]
    fn test_parse_error_creates_basic_error() {
        let err = RaffError::parse_error("test context");
        assert!(matches!(err, RaffError::ParseError { .. }));
        assert_eq!(err.name(), "ParseError");
    }

    #[test]
    fn test_parse_error_with_file_creates_error_with_path() {
        let path = PathBuf::from("/test/file.rs");
        let err = RaffError::parse_error_with_file(path.clone(), "test context");
        assert!(matches!(err, RaffError::ParseError { file, .. } if file == Some(path)));
    }

    #[test]
    fn test_git_error_creates_basic_error() {
        let err = RaffError::git_error("open repository");
        assert!(matches!(err, RaffError::GitError { .. }));
        assert_eq!(err.name(), "GitError");
    }

    #[test]
    fn test_git_error_with_repo_creates_error_with_path() {
        let path = PathBuf::from("/repo");
        let err = RaffError::git_error_with_repo("open", path.clone());
        assert!(matches!(err, RaffError::GitError { repo_path, .. } if repo_path == Some(path)));
    }

    #[test]
    fn test_io_error_creates_basic_error() {
        let err = RaffError::io_error("read file");
        assert!(matches!(err, RaffError::IoError { .. }));
        assert_eq!(err.name(), "IoError");
    }

    #[test]
    fn test_io_error_with_source_creates_error_with_path_and_source() {
        let path = PathBuf::from("/test/file.txt");
        let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
        let err = RaffError::io_error_with_source("read", path.clone(), io_err);
        assert!(matches!(err, RaffError::IoError { path: p, .. } if p == Some(path)));
    }

    #[test]
    fn test_config_error_creates_basic_error() {
        let err = RaffError::config_error("invalid format");
        assert!(matches!(err, RaffError::ConfigError { .. }));
        assert_eq!(err.name(), "ConfigError");
    }

    #[test]
    fn test_config_error_with_path_creates_error_with_path() {
        let path = PathBuf::from("/config.toml");
        let err = RaffError::config_error_with_path("invalid", path.clone());
        assert!(matches!(err, RaffError::ConfigError { path: p, .. } if p == Some(path)));
    }

    #[test]
    fn test_analysis_error_creates_basic_error() {
        let err = RaffError::analysis_error("volatility", "no crates found");
        assert!(matches!(err, RaffError::AnalysisError { .. }));
        assert_eq!(err.name(), "AnalysisError");
        let display = format!("{}", err);
        assert!(display.contains("volatility"));
        assert!(display.contains("no crates found"));
    }

    #[test]
    fn test_invalid_input_creates_basic_error() {
        let err = RaffError::invalid_input("invalid path");
        assert!(matches!(err, RaffError::InvalidInput { .. }));
        assert_eq!(err.name(), "InvalidInput");
    }

    #[test]
    fn test_invalid_input_with_arg_creates_error_with_argument() {
        let err = RaffError::invalid_input_with_arg("invalid path", "/bad/path");
        assert!(
            matches!(err, RaffError::InvalidInput { argument, .. } if argument == Some("/bad/path".to_string()))
        );
    }

    // Display tests

    #[test]
    fn test_display_parse_error_without_file() {
        let err = RaffError::parse_error("test context");
        let display = format!("{}", err);
        assert!(display.contains("Parse error"));
        assert!(display.contains("test context"));
    }

    #[test]
    fn test_display_parse_error_with_file() {
        let path = PathBuf::from("/test/file.rs");
        let err = RaffError::parse_error_with_file(path.clone(), "test context");
        let display = format!("{}", err);
        assert!(display.contains("Parse error"));
        assert!(display.contains("test context"));
        assert!(display.contains("file.rs"));
    }

    #[test]
    fn test_display_git_error_without_repo() {
        let err = RaffError::git_error("open repository");
        let display = format!("{}", err);
        assert!(display.contains("Git error"));
        assert!(display.contains("open repository"));
    }

    #[test]
    fn test_display_git_error_with_repo() {
        let path = PathBuf::from("/repo");
        let err = RaffError::git_error_with_repo("open", path.clone());
        let display = format!("{}", err);
        assert!(display.contains("Git error"));
        assert!(display.contains("repo"));
    }

    #[test]
    fn test_display_io_error_without_path() {
        let err = RaffError::io_error("read file");
        let display = format!("{}", err);
        assert!(display.contains("IO error"));
        assert!(display.contains("read file"));
    }

    #[test]
    fn test_display_io_error_with_path() {
        let path = PathBuf::from("/test/file.txt");
        let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
        let err = RaffError::io_error_with_source("read", path, io_err);
        let display = format!("{}", err);
        assert!(display.contains("IO error"));
        assert!(display.contains("read"));
    }

    #[test]
    fn test_display_config_error_without_path() {
        let err = RaffError::config_error("invalid format");
        let display = format!("{}", err);
        assert!(display.contains("Configuration error"));
        assert!(display.contains("invalid format"));
    }

    #[test]
    fn test_display_config_error_with_path() {
        let path = PathBuf::from("/config.toml");
        let err = RaffError::config_error_with_path("invalid", path);
        let display = format!("{}", err);
        assert!(display.contains("Configuration error"));
        assert!(display.contains("config.toml"));
    }

    #[test]
    fn test_display_analysis_error() {
        let err = RaffError::analysis_error("volatility", "no crates found");
        let display = format!("{}", err);
        assert!(display.contains("Analysis error"));
        assert!(display.contains("volatility"));
        assert!(display.contains("no crates found"));
    }

    #[test]
    fn test_display_invalid_input_without_argument() {
        let err = RaffError::invalid_input("invalid path");
        let display = format!("{}", err);
        assert!(display.contains("Invalid input"));
        assert!(display.contains("invalid path"));
    }

    #[test]
    fn test_display_invalid_input_with_argument() {
        let err = RaffError::invalid_input_with_arg("invalid path", "/bad/path");
        let display = format!("{}", err);
        assert!(display.contains("Invalid input"));
        assert!(display.contains("/bad/path"));
    }

    // Suggestions tests

    #[test]
    fn test_suggestions_parse_error_without_file() {
        let err = RaffError::parse_error("test context");
        let suggestions = err.suggestions();
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains("valid Rust code")));
    }

    #[test]
    fn test_suggestions_parse_error_with_file() {
        let path = PathBuf::from("/test/file.rs");
        let err = RaffError::parse_error_with_file(path, "test context");
        let suggestions = err.suggestions();
        assert!(suggestions.iter().any(|s| s.contains("cargo check")));
    }

    #[test]
    fn test_suggestions_git_error() {
        let err = RaffError::git_error("open repository");
        let suggestions = err.suggestions();
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains("Git repository")));
    }

    #[test]
    fn test_suggestions_io_error() {
        let err = RaffError::io_error("read file");
        let suggestions = err.suggestions();
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains("permissions")));
    }

    #[test]
    fn test_suggestions_config_error() {
        let err = RaffError::config_error("invalid format");
        let suggestions = err.suggestions();
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains("TOML")));
    }

    #[test]
    fn test_suggestions_analysis_error() {
        let err = RaffError::analysis_error("volatility", "no crates found");
        let suggestions = err.suggestions();
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains("volatility")));
    }

    #[test]
    fn test_suggestions_invalid_input() {
        let err = RaffError::invalid_input("invalid path");
        let suggestions = err.suggestions();
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains("arguments")));
    }

    // From conversion tests

    #[test]
    fn test_from_io_error_creates_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let raff_err: RaffError = io_err.into();
        assert!(matches!(raff_err, RaffError::IoError { .. }));
    }

    #[test]
    fn test_from_toml_de_error_creates_config_error() {
        // Parse invalid TOML to get a toml::de::Error
        let toml_err = toml::from_str::<toml::Value>("invalid = [unclosed").unwrap_err();
        let raff_err: RaffError = toml_err.into();
        assert!(matches!(raff_err, RaffError::ConfigError { .. }));
        let display = format!("{}", raff_err);
        assert!(display.contains("TOML"));
    }

    #[test]
    fn test_from_toml_ser_error_creates_config_error() {
        // Create a value that can't be serialized to TOML (non-string key in map)
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(123, "value");
        let toml_err = toml::to_string(&map).unwrap_err();
        let raff_err: RaffError = toml_err.into();
        assert!(matches!(raff_err, RaffError::ConfigError { .. }));
        let display = format!("{}", raff_err);
        assert!(display.contains("TOML"));
    }

    #[test]
    fn test_from_serde_json_error_creates_config_error() {
        // Parse invalid JSON to get a serde_json::Error
        let json_err = serde_json::from_str::<serde_json::Value>("{invalid}").unwrap_err();
        let raff_err: RaffError = json_err.into();
        assert!(matches!(raff_err, RaffError::ConfigError { .. }));
        let display = format!("{}", raff_err);
        assert!(display.contains("JSON"));
    }

    #[test]
    fn test_from_csv_error_creates_analysis_error() {
        let csv_err = csv::Error::from(io::Error::other("csv error"));
        let raff_err: RaffError = csv_err.into();
        assert!(matches!(raff_err, RaffError::AnalysisError { .. }));
    }

    #[test]
    fn test_from_syn_error_creates_parse_error() {
        // Create a syn error by parsing invalid code
        let result: std::result::Result<syn::File, _> = syn::parse_str("fn invalid {");
        let syn_err = match result {
            Ok(_) => panic!("Expected syn error"),
            Err(e) => e,
        };
        let raff_err: RaffError = syn_err.into();
        assert!(matches!(raff_err, RaffError::ParseError { .. }));
        let display = format!("{}", raff_err);
        assert!(display.contains("Rust source"));
    }

    #[test]
    fn test_from_git2_error_creates_git_error() {
        let git_err = git2::Error::from_str("git operation failed");
        let raff_err: RaffError = git_err.into();
        assert!(matches!(raff_err, RaffError::GitError { .. }));
    }

    #[test]
    fn test_from_walkdir_error_creates_io_error() {
        // walkdir::Error is created during directory iteration failures
        // We verify the From impl by triggering a walkdir error on a non-existent path
        let non_existent = PathBuf::from("/tmp/.raff_test_nonexistent_12345");
        let mut found_error = false;
        for entry in walkdir::WalkDir::new(&non_existent).into_iter() {
            if let Err(walk_err) = entry {
                let raff_err: RaffError = walk_err.into();
                assert!(matches!(raff_err, RaffError::IoError { .. }));
                found_error = true;
                break;
            }
        }
        assert!(
            found_error,
            "Expected walkdir to produce an error for non-existent path"
        );
    }

    // Result type alias tests

    #[test]
    fn test_result_type_alias_works() {
        fn returns_ok() -> Result<i32> {
            Ok(42)
        }
        fn returns_err() -> Result<i32> {
            Err(RaffError::invalid_input("test"))
        }

        // Use matches! since RaffError doesn't implement PartialEq
        assert!(matches!(returns_ok(), Ok(42)));
        assert!(returns_err().is_err());
    }

    #[test]
    fn test_question_mark_operator_works_with_result() {
        fn may_fail(should_fail: bool) -> Result<i32> {
            if should_fail {
                Err(RaffError::invalid_input("failed"))
            } else {
                Ok(42)
            }
        }

        fn uses_question_mark(should_fail: bool) -> Result<i32> {
            let val = may_fail(should_fail)?;
            Ok(val + 8)
        }

        // Use matches! since RaffError doesn't implement PartialEq
        assert!(matches!(uses_question_mark(false), Ok(50)));
        assert!(uses_question_mark(true).is_err());
    }
}
