//! Rule Trait
//!
//! This module defines the [`Rule`] trait, which provides a common interface for all
//! analysis rules in Raff. The trait enables custom rules to be implemented by users
//! and integrated into the Raff framework.
//!
//! # Overview
//!
//! The `Rule` trait abstracts the common pattern followed by all Raff analysis rules:
//!
//! - Each rule has a name for identification
//! - Each rule can analyze code and produce data
//! - Each rule can render results in multiple formats
//!
//! # Implementing a Custom Rule
//!
//! To create a custom rule, implement the `Rule` trait. Your data type must
//! implement `serde::Serialize`:
//!
//! ```rust,no_run
//! use raff_core::rule::Rule;
//! use raff_core::error::Result;
//! use serde::Serialize;
//! use std::path::PathBuf;
//!
//! # #[derive(Debug, Serialize)]
//! # #[serde(crate = "serde")]
//! # struct MyCustomData {
//! #     pub file_count: usize,
//! # }
//! #
//! # #[derive(Clone, Debug)]
//! # struct MyCustomConfig {
//! #     pub path: PathBuf,
//! # }
//! #
//! # struct MyCustomRule;
//! #
//! impl Rule for MyCustomRule {
//!     type Data = MyCustomData;
//!     type Config = MyCustomConfig;
//!
//!     fn name() -> &'static str {
//!         "my_custom_rule"
//!     }
//!
//!     fn description() -> &'static str {
//!         "Analyzes custom metrics in the codebase"
//!     }
//!
//!     fn run(&self, _config: &MyCustomConfig) -> Result<()> {
//!         // Output results
//!         println!("Analysis complete");
//!         Ok(())
//!     }
//!
//!     fn analyze(&self, _config: &MyCustomConfig) -> Result<MyCustomData> {
//!         // Perform analysis and return data
//!         Ok(MyCustomData { file_count: 42 })
//!     }
//! }
//! ```

use crate::error::Result;
use serde::Serialize;
use std::fmt::Debug;

/// Common trait for all analysis rules in Raff.
///
/// This trait defines the interface that all rules must implement to be
/// compatible with the Raff framework. Each rule specifies its own
/// configuration and data types via associated types.
pub trait Rule: Sized {
    /// The type of configuration data this rule accepts.
    ///
    /// This is typically a struct containing CLI arguments or config file
    /// settings specific to this rule.
    type Config: Clone + Debug + Send + Sync;

    /// The type of analysis data this rule produces.
    ///
    /// This is the structured data returned by the `analyze` method,
    /// which can be serialized to JSON/YAML or used for HTML rendering.
    type Data: Debug + Send + Sync + Serialize;

    /// Returns the name of this rule.
    ///
    /// This is used for identifying the rule in error messages, logs,
    /// and configuration files. Should be a unique, snake_case string.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use raff_core::rule::Rule;
    /// # struct MyRule;
    /// # impl Rule for MyRule {
    /// # type Config = ();
    /// # type Data = ();
    /// # fn description() -> &'static str { "" }
    /// fn name() -> &'static str {
    ///     "my_custom_rule"
    /// }
    /// # fn run(&self, _config: &Self::Config) -> raff_core::error::Result<()> { Ok(()) }
    /// # fn analyze(&self, _config: &Self::Config) -> raff_core::error::Result<Self::Data> { Ok(()) }
    /// # }
    /// ```
    fn name() -> &'static str;

    /// Returns a human-readable description of this rule.
    ///
    /// This should briefly explain what the rule does and what metrics
    /// it analyzes. Used for help text and documentation.
    fn description() -> &'static str;

    /// Runs the rule with the given configuration and outputs results.
    ///
    /// This method performs the full analysis pipeline:
    /// 1. Calls `analyze` to gather data
    /// 2. Formats and outputs results based on the config
    /// 3. Returns an error if the analysis fails or if thresholds are exceeded
    ///
    /// # Errors
    ///
    /// Returns a [`RaffError`] if:
    /// - The analysis path does not exist
    /// - Required tools are not available
    /// - Analysis fails to complete
    /// - Output formatting fails
    fn run(&self, config: &Self::Config) -> Result<()>;

    /// Analyzes the codebase and returns structured data.
    ///
    /// This is the core analysis method that performs the actual work
    /// of the rule. It should not produce any output directly; instead,
    /// it returns data that can be formatted by `run` or used programmatically.
    ///
    /// # Errors
    ///
    /// Returns a [`RaffError`] if the analysis cannot be completed.
    fn analyze(&self, config: &Self::Config) -> Result<Self::Data>;
}

/// Helper macro for creating error messages with rule context.
///
/// This macro is used by rule implementations to create consistent error
/// messages that include the rule name for better debugging.
///
/// # Examples
///
/// ```rust
/// # use raff_core::rule_error;
/// let error = rule_error!("my_rule", "Failed to parse file: {}", "main.rs");
/// ```
#[macro_export]
macro_rules! rule_error {
    ($rule_name:expr, $msg:expr) => {
        $crate::error::RaffError::analysis_error($rule_name, $msg)
    };
    ($rule_name:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::error::RaffError::analysis_error($rule_name, format!($fmt, $($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// A simple test rule implementation for testing the trait.
    #[derive(Debug, Default)]
    struct TestRule;

    /// Configuration for the test rule.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct TestConfig {
        pub value: usize,
    }

    /// Data produced by the test rule.
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestData {
        pub result: String,
    }

    impl Default for TestData {
        fn default() -> Self {
            TestData {
                result: "test".to_string(),
            }
        }
    }

    impl Rule for TestRule {
        type Config = TestConfig;
        type Data = TestData;

        fn name() -> &'static str {
            "test_rule"
        }

        fn description() -> &'static str {
            "A test rule for validating the Rule trait"
        }

        fn run(&self, config: &Self::Config) -> Result<()> {
            let _data = self.analyze(config)?;
            Ok(())
        }

        fn analyze(&self, config: &Self::Config) -> Result<Self::Data> {
            if config.value == 0 {
                return Err(rule_error!(Self::name(), "Config value cannot be zero"));
            }
            Ok(TestData {
                result: format!("value is {}", config.value),
            })
        }
    }

    #[test]
    fn test_rule_name_returns_correct_name() {
        assert_eq!(TestRule::name(), "test_rule");
    }

    #[test]
    fn test_rule_description_returns_description() {
        assert_eq!(
            TestRule::description(),
            "A test rule for validating the Rule trait"
        );
    }

    #[test]
    fn test_rule_run_succeeds_with_valid_config() {
        let rule = TestRule;
        let config = TestConfig { value: 42 };

        let result = rule.run(&config);
        assert!(result.is_ok(), "run should succeed with valid config");
    }

    #[test]
    fn test_rule_analyze_succeeds_with_valid_config() {
        let rule = TestRule;
        let config = TestConfig { value: 42 };

        let result = rule.analyze(&config);
        assert!(result.is_ok(), "analyze should succeed with valid config");

        let data = result.unwrap();
        assert_eq!(data.result, "value is 42");
    }

    #[test]
    fn test_rule_analyze_fails_with_zero_config_value() {
        let rule = TestRule;
        let config = TestConfig { value: 0 };

        let result = rule.analyze(&config);
        assert!(result.is_err(), "analyze should fail with zero value");

        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("test_rule"),
            "error should mention rule name"
        );
        assert!(
            error_msg.contains("cannot be zero"),
            "error should mention specific failure reason"
        );
    }

    #[test]
    fn test_rule_data_is_serializable() {
        let data = TestData {
            result: "test".to_string(),
        };

        let json = serde_json::to_string(&data);
        assert!(json.is_ok(), "TestData should be serializable to JSON");

        let json_str = json.unwrap();
        assert!(
            json_str.contains("test"),
            "JSON should contain result value"
        );
    }

    #[test]
    fn test_rule_data_can_be_deserialized() {
        let json = r#"{"result":"deserialized"}"#;
        let result: std::result::Result<TestData, _> = serde_json::from_str(json);

        assert!(result.is_ok(), "TestData should deserialize from JSON");

        let data = result.unwrap();
        assert_eq!(data.result, "deserialized");
    }
}
