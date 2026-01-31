//! Orchestration for running all analysis rules.
//!
//! This module provides [`run_all`], which executes all configured analysis rules
//! and produces consolidated reports. It is used by the CLI's "all" command to
//! run multiple analyses in a single invocation.
//!
//! # Output Formats
//!
//! The consolidated report supports two output formats:
//!
//! - **JSON**: Combines results from all rules into a single JSON document
//! - **HTML**: Generates an HTML report with all analysis results combined
//!
//! # Example
//!
//! ```rust,no_run
//! use raff_core::{run_all, AllArgs, AllOutputFormat};
//! use std::path::PathBuf;
//!
//! # fn main() -> raff_core::error::Result<()> {
//! let args = AllArgs {
//!     path: PathBuf::from("./src"),
//!     output: AllOutputFormat::Json,
//!     // .. other fields
//!     sc_threshold: 10,
//!     vol_alpha: 0.01,
//!     vol_since: None,
//!     vol_normalize: false,
//!     vol_skip_merges: false,
//!     coup_granularity: raff_core::CouplingGranularity::Both,
//!     rca_extra_flags: vec![],
//!     rca_jobs: None,
//!     rca_metrics: true,
//!     rca_language: "rust".to_string(),
//! };
//!
//! run_all(&args)?;
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use maud::Markup;
use serde::Serialize;

use crate::{
    cli::{AllArgs, AllOutputFormat},
    coupling_rule::{CouplingData, CouplingRule},
    html_utils,
    rust_code_analysis_rule::{RustCodeAnalysisData, RustCodeAnalysisRule},
    statement_count_rule::{StatementCountData, StatementCountRule},
    volatility_rule::{VolatilityData, VolatilityRule},
};

#[derive(Debug)]
pub struct AllReportData {
    statement_count: Option<Result<StatementCountData>>,
    volatility: Option<Result<VolatilityData>>,
    coupling: Option<Result<CouplingData>>,
    rust_code_analysis: Option<Result<RustCodeAnalysisData>>,
}

#[derive(Debug, Serialize)]
struct JsonReportData<'a> {
    statement_count: Option<&'a StatementCountData>,
    volatility: Option<&'a VolatilityData>,
    coupling: Option<&'a CouplingData>,
    rust_code_analysis: Option<&'a RustCodeAnalysisData>,
    errors: Vec<String>,
}

// Public constructors for testing
impl AllReportData {
    /// Creates a new `AllReportData` with all fields set to None.
    /// Used for testing error handling scenarios.
    pub fn new() -> Self {
        Self {
            statement_count: None,
            volatility: None,
            coupling: None,
            rust_code_analysis: None,
        }
    }

    /// Creates a new `AllReportData` with the given values.
    /// Used for testing successful analysis scenarios.
    pub fn with_results(
        statement_count: Option<Result<StatementCountData>>,
        volatility: Option<Result<VolatilityData>>,
        coupling: Option<Result<CouplingData>>,
        rust_code_analysis: Option<Result<RustCodeAnalysisData>>,
    ) -> Self {
        Self {
            statement_count,
            volatility,
            coupling,
            rust_code_analysis,
        }
    }
}

impl Default for AllReportData {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_all(args: &AllArgs) -> Result<()> {
    let sc_rule = StatementCountRule::new();
    let vol_rule = VolatilityRule::new();
    let coup_rule = CouplingRule::new();
    let rca_rule = RustCodeAnalysisRule::new();

    // We need to construct the specific args for each rule from the AllArgs
    let sc_args = crate::cli::StatementCountArgs {
        path: args.path.clone(),
        threshold: args.sc_threshold,
        output: crate::cli::StatementCountOutputFormat::Table, // format is irrelevant for analyze
    };
    let vol_args = crate::cli::VolatilityArgs {
        path: args.path.clone(),
        alpha: args.vol_alpha,
        since: args.vol_since.clone(),
        normalize: args.vol_normalize,
        skip_merges: args.vol_skip_merges,
        output: crate::cli::VolatilityOutputFormat::Table, // format is irrelevant for analyze
    };
    let coup_args = crate::cli::CouplingArgs {
        path: args.path.clone(),
        granularity: args.coup_granularity.clone(),
        output: crate::cli::CouplingOutputFormat::Table, // format is irrelevant for analyze
    };
    let rca_args = crate::cli::RustCodeAnalysisArgs {
        path: args.path.clone(),
        extra_flags: args.rca_extra_flags.clone(),
        jobs: args.rca_jobs,
        metrics: args.rca_metrics,
        language: args.rca_language.clone(),
        output: crate::cli::RustCodeAnalysisOutputFormat::Table, // format is irrelevant for analyze
    };

    let all_data = AllReportData {
        statement_count: Some(sc_rule.analyze(&sc_args)),
        volatility: Some(vol_rule.analyze(&vol_args)),
        coupling: Some(coup_rule.analyze(&coup_args)),
        rust_code_analysis: Some(rca_rule.analyze(&rca_args)),
    };

    match args.output {
        AllOutputFormat::Json => {
            let mut errors = Vec::new();
            if let Some(Err(e)) = &all_data.statement_count {
                errors.push(format!("Statement Count Error: {e}"));
            }
            if let Some(Err(e)) = &all_data.volatility {
                errors.push(format!("Volatility Error: {e}"));
            }
            if let Some(Err(e)) = &all_data.coupling {
                errors.push(format!("Coupling Error: {e}"));
            }
            if let Some(Err(e)) = &all_data.rust_code_analysis {
                errors.push(format!("Rust Code Analysis Error: {e}"));
            }

            let json_report = JsonReportData {
                statement_count: all_data
                    .statement_count
                    .as_ref()
                    .and_then(|r| r.as_ref().ok()),
                volatility: all_data.volatility.as_ref().and_then(|r| r.as_ref().ok()),
                coupling: all_data.coupling.as_ref().and_then(|r| r.as_ref().ok()),
                rust_code_analysis: all_data
                    .rust_code_analysis
                    .as_ref()
                    .and_then(|r| r.as_ref().ok()),
                errors,
            };

            let json = serde_json::to_string_pretty(&json_report)?;
            println!("{json}");
        }
        AllOutputFormat::Html => {
            let mut html_body_parts: Vec<Markup> = vec![];

            if let Some(Ok(data)) = &all_data.statement_count {
                html_body_parts.push(sc_rule.render_statement_count_html_body(data)?);
            }
            if let Some(Ok(data)) = &all_data.volatility {
                let mut sorted_crates: Vec<_> = data.crate_stats_map.iter().collect();
                sorted_crates.sort_by(|a, b| {
                    b.1.raw_score
                        .partial_cmp(&a.1.raw_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                html_body_parts.push(vol_rule.render_volatility_html_body(
                    &sorted_crates,
                    data.normalize,
                    data.alpha,
                )?);
            }
            if let Some(Ok(data)) = &all_data.coupling {
                html_body_parts.push(coup_rule.render_coupling_html_body(data)?);
            }
            if let Some(Ok(data)) = &all_data.rust_code_analysis {
                html_body_parts.push(rca_rule.render_rust_code_analysis_html_body(
                    &data.analysis_results,
                    &data.analysis_path,
                )?);
            }

            let full_html = html_utils::render_html_doc(
                "Consolidated Analysis Report",
                maud::html! { @for part in &html_body_parts { (part) } },
            );
            println!("{full_html}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Helper function to create test AllArgs with minimal valid values.
    fn create_test_args(path: &str) -> AllArgs {
        use crate::cli::CouplingGranularity;

        AllArgs {
            path: PathBuf::from(path),
            output: AllOutputFormat::Json,
            sc_threshold: 10,
            vol_alpha: 0.01,
            vol_since: None,
            vol_normalize: false,
            vol_skip_merges: false,
            coup_granularity: CouplingGranularity::Both,
            rca_extra_flags: vec![],
            rca_jobs: 1,
            rca_metrics: true,
            rca_language: "rust".to_string(),
        }
    }

    /// Helper function to create test StatementCountArgs.
    #[allow(dead_code)]
    fn create_test_statement_count_args(
        path: &str,
        threshold: usize,
    ) -> crate::cli::StatementCountArgs {
        crate::cli::StatementCountArgs {
            path: PathBuf::from(path),
            threshold,
            output: crate::cli::StatementCountOutputFormat::Table,
        }
    }

    /// Helper function to create test VolatilityArgs.
    #[allow(dead_code)]
    fn create_test_volatility_args(path: &str, alpha: f64) -> crate::cli::VolatilityArgs {
        crate::cli::VolatilityArgs {
            path: PathBuf::from(path),
            alpha,
            since: None,
            normalize: false,
            skip_merges: false,
            output: crate::cli::VolatilityOutputFormat::Table,
        }
    }

    /// Helper function to create test CouplingArgs.
    #[allow(dead_code)]
    fn create_test_coupling_args(path: &str) -> crate::cli::CouplingArgs {
        use crate::cli::{CouplingGranularity, CouplingOutputFormat};
        crate::cli::CouplingArgs {
            path: PathBuf::from(path),
            granularity: CouplingGranularity::Both,
            output: CouplingOutputFormat::Table,
        }
    }

    /// Helper function to create test RustCodeAnalysisArgs.
    #[allow(dead_code)]
    fn create_test_rca_args(path: &str) -> crate::cli::RustCodeAnalysisArgs {
        crate::cli::RustCodeAnalysisArgs {
            path: PathBuf::from(path),
            extra_flags: vec![],
            jobs: 1,
            metrics: true,
            language: "rust".to_string(),
            output: crate::cli::RustCodeAnalysisOutputFormat::Table,
        }
    }

    #[test]
    fn test_all_report_data_new_creates_empty_instance() {
        let data = AllReportData::new();
        assert!(
            data.statement_count.is_none(),
            "AllReportData::new() should create instance with statement_count as None"
        );
        assert!(
            data.volatility.is_none(),
            "AllReportData::new() should create instance with volatility as None"
        );
        assert!(
            data.coupling.is_none(),
            "AllReportData::new() should create instance with coupling as None"
        );
        assert!(
            data.rust_code_analysis.is_none(),
            "AllReportData::new() should create instance with rust_code_analysis as None"
        );
    }

    #[test]
    fn test_all_report_data_default_creates_empty_instance() {
        let data = AllReportData::default();
        assert!(
            data.statement_count.is_none(),
            "AllReportData::default() should create instance with statement_count as None"
        );
        assert!(
            data.volatility.is_none(),
            "AllReportData::default() should create instance with volatility as None"
        );
        assert!(
            data.coupling.is_none(),
            "AllReportData::default() should create instance with coupling as None"
        );
        assert!(
            data.rust_code_analysis.is_none(),
            "AllReportData::default() should create instance with rust_code_analysis as None"
        );
    }

    #[test]
    fn test_all_report_data_with_results_stores_provided_values() {
        let sc_result: Result<StatementCountData> = Err(crate::error::RaffError::analysis_error(
            "statement_count",
            "test error",
        ));
        let vol_result: Result<VolatilityData> = Err(crate::error::RaffError::analysis_error(
            "volatility",
            "volatility error",
        ));
        let coup_result: Result<CouplingData> = Err(crate::error::RaffError::analysis_error(
            "coupling",
            "coupling error",
        ));
        let rca_result: Result<RustCodeAnalysisData> = Err(
            crate::error::RaffError::analysis_error("rust_code_analysis", "rca error"),
        );

        let data = AllReportData::with_results(
            Some(sc_result),
            Some(vol_result),
            Some(coup_result),
            Some(rca_result),
        );

        assert!(
            data.statement_count.is_some(),
            "with_results should store statement_count result"
        );
        assert!(
            data.volatility.is_some(),
            "with_results should store volatility result"
        );
        assert!(
            data.coupling.is_some(),
            "with_results should store coupling result"
        );
        assert!(
            data.rust_code_analysis.is_some(),
            "with_results should store rust_code_analysis result"
        );
    }

    #[test]
    fn test_all_report_data_with_results_none_values() {
        let data = AllReportData::with_results(None, None, None, None);

        assert!(
            data.statement_count.is_none(),
            "with_results should accept None for statement_count"
        );
        assert!(
            data.volatility.is_none(),
            "with_results should accept None for volatility"
        );
        assert!(
            data.coupling.is_none(),
            "with_results should accept None for coupling"
        );
        assert!(
            data.rust_code_analysis.is_none(),
            "with_results should accept None for rust_code_analysis"
        );
    }

    #[test]
    fn test_json_report_data_is_serializable() {
        // Create JsonReportData with all None values
        let report_data = JsonReportData {
            statement_count: None,
            volatility: None,
            coupling: None,
            rust_code_analysis: None,
            errors: vec![],
        };

        let json = serde_json::to_string(&report_data);
        assert!(
            json.is_ok(),
            "JsonReportData should be serializable to JSON"
        );

        let json_str = json.unwrap();
        assert!(
            json_str.contains("statement_count"),
            "JSON output should contain statement_count field"
        );
        assert!(
            json_str.contains("volatility"),
            "JSON output should contain volatility field"
        );
        assert!(
            json_str.contains("coupling"),
            "JSON output should contain coupling field"
        );
        assert!(
            json_str.contains("rust_code_analysis"),
            "JSON output should contain rust_code_analysis field"
        );
        assert!(
            json_str.contains("errors"),
            "JSON output should contain errors field"
        );
    }

    #[test]
    fn test_json_report_data_with_errors_is_serializable() {
        let report_data = JsonReportData {
            statement_count: None,
            volatility: None,
            coupling: None,
            rust_code_analysis: None,
            errors: vec!["Error 1".to_string(), "Error 2".to_string()],
        };

        let json = serde_json::to_string(&report_data);
        assert!(
            json.is_ok(),
            "JsonReportData with errors should be serializable"
        );

        let json_str = json.unwrap();
        assert!(
            json_str.contains("Error 1"),
            "JSON output should contain first error message"
        );
        assert!(
            json_str.contains("Error 2"),
            "JSON output should contain second error message"
        );
    }

    #[test]
    fn test_all_args_creates_valid_statement_count_args() {
        let all_args = create_test_args("/test/path");

        let sc_args = crate::cli::StatementCountArgs {
            path: all_args.path.clone(),
            threshold: all_args.sc_threshold,
            output: crate::cli::StatementCountOutputFormat::Table,
        };

        assert_eq!(
            sc_args.path,
            PathBuf::from("/test/path"),
            "StatementCountArgs path should match AllArgs path"
        );
        assert_eq!(
            sc_args.threshold, 10,
            "StatementCountArgs threshold should match AllArgs sc_threshold"
        );
        assert!(
            matches!(
                sc_args.output,
                crate::cli::StatementCountOutputFormat::Table
            ),
            "StatementCountArgs output should be Table"
        );
    }

    #[test]
    fn test_all_args_creates_valid_volatility_args() {
        let all_args = create_test_args("/test/path");

        let vol_args = crate::cli::VolatilityArgs {
            path: all_args.path.clone(),
            alpha: all_args.vol_alpha,
            since: all_args.vol_since.clone(),
            normalize: all_args.vol_normalize,
            skip_merges: all_args.vol_skip_merges,
            output: crate::cli::VolatilityOutputFormat::Table,
        };

        assert_eq!(
            vol_args.path,
            PathBuf::from("/test/path"),
            "VolatilityArgs path should match AllArgs path"
        );
        assert_eq!(
            vol_args.alpha, 0.01,
            "VolatilityArgs alpha should match AllArgs vol_alpha"
        );
        assert_eq!(
            vol_args.since, None,
            "VolatilityArgs since should match AllArgs vol_since"
        );
        assert!(
            !vol_args.normalize,
            "VolatilityArgs normalize should match AllArgs vol_normalize"
        );
        assert!(
            !vol_args.skip_merges,
            "VolatilityArgs skip_merges should match AllArgs vol_skip_merges"
        );
    }

    #[test]
    fn test_all_args_creates_valid_coupling_args() {
        use crate::cli::{CouplingGranularity, CouplingOutputFormat};
        let all_args = create_test_args("/test/path");

        let coup_args = crate::cli::CouplingArgs {
            path: all_args.path.clone(),
            granularity: all_args.coup_granularity.clone(),
            output: CouplingOutputFormat::Table,
        };

        assert_eq!(
            coup_args.path,
            PathBuf::from("/test/path"),
            "CouplingArgs path should match AllArgs path"
        );
        assert!(
            matches!(coup_args.granularity, CouplingGranularity::Both),
            "CouplingArgs granularity should match AllArgs coup_granularity"
        );
        assert!(
            matches!(coup_args.output, CouplingOutputFormat::Table),
            "CouplingArgs output should be Table"
        );
    }

    #[test]
    fn test_all_args_creates_valid_rca_args() {
        let all_args = create_test_args("/test/path");

        let rca_args = crate::cli::RustCodeAnalysisArgs {
            path: all_args.path.clone(),
            extra_flags: all_args.rca_extra_flags.clone(),
            jobs: all_args.rca_jobs,
            metrics: all_args.rca_metrics,
            language: all_args.rca_language.clone(),
            output: crate::cli::RustCodeAnalysisOutputFormat::Table,
        };

        assert_eq!(
            rca_args.path,
            PathBuf::from("/test/path"),
            "RustCodeAnalysisArgs path should match AllArgs path"
        );
        assert_eq!(
            rca_args.extra_flags.len(),
            0,
            "RustCodeAnalysisArgs extra_flags should match AllArgs rca_extra_flags"
        );
        assert_eq!(
            rca_args.jobs, 1,
            "RustCodeAnalysisArgs jobs should match AllArgs rca_jobs"
        );
        assert!(
            rca_args.metrics,
            "RustCodeAnalysisArgs metrics should match AllArgs rca_metrics"
        );
        assert_eq!(
            rca_args.language, "rust",
            "RustCodeAnalysisArgs language should match AllArgs rca_language"
        );
    }

    #[test]
    fn test_all_args_with_vol_since_creates_correct_volatility_args() {
        use crate::cli::VolatilityOutputFormat;
        let mut all_args = create_test_args("/test/path");
        all_args.vol_since = Some("2023-01-01".to_string());

        let vol_args = crate::cli::VolatilityArgs {
            path: all_args.path.clone(),
            alpha: all_args.vol_alpha,
            since: all_args.vol_since.clone(),
            normalize: all_args.vol_normalize,
            skip_merges: all_args.vol_skip_merges,
            output: VolatilityOutputFormat::Table,
        };

        assert_eq!(
            vol_args.since,
            Some("2023-01-01".to_string()),
            "VolatilityArgs since should match AllArgs vol_since when set"
        );
    }

    #[test]
    fn test_all_args_with_vol_normalize_creates_correct_volatility_args() {
        let mut all_args = create_test_args("/test/path");
        all_args.vol_normalize = true;

        let vol_args = crate::cli::VolatilityArgs {
            path: all_args.path.clone(),
            alpha: all_args.vol_alpha,
            since: all_args.vol_since.clone(),
            normalize: all_args.vol_normalize,
            skip_merges: all_args.vol_skip_merges,
            output: crate::cli::VolatilityOutputFormat::Table,
        };

        assert!(
            vol_args.normalize,
            "VolatilityArgs normalize should match AllArgs vol_normalize when true"
        );
    }

    #[test]
    fn test_all_args_with_rca_extra_flags_creates_correct_rca_args() {
        let mut all_args = create_test_args("/test/path");
        all_args.rca_extra_flags = vec!["--flag1".to_string(), "--flag2".to_string()];

        let rca_args = crate::cli::RustCodeAnalysisArgs {
            path: all_args.path.clone(),
            extra_flags: all_args.rca_extra_flags.clone(),
            jobs: all_args.rca_jobs,
            metrics: all_args.rca_metrics,
            language: all_args.rca_language.clone(),
            output: crate::cli::RustCodeAnalysisOutputFormat::Table,
        };

        assert_eq!(
            rca_args.extra_flags.len(),
            2,
            "RustCodeAnalysisArgs extra_flags should contain the extra flags"
        );
        assert_eq!(
            rca_args.extra_flags,
            vec!["--flag1".to_string(), "--flag2".to_string()],
            "RustCodeAnalysisArgs extra_flags should match AllArgs rca_extra_flags"
        );
    }

    #[test]
    fn test_statement_count_rule_new_creates_instance() {
        let rule = StatementCountRule::new();
        // Just verify the rule can be created without panicking
        let _ = rule;
    }

    #[test]
    fn test_volatility_rule_new_creates_instance() {
        let rule = VolatilityRule::new();
        // Just verify the rule can be created without panicking
        let _ = rule;
    }

    #[test]
    fn test_coupling_rule_new_creates_instance() {
        let rule = CouplingRule::new();
        // Just verify the rule can be created without panicking
        let _ = rule;
    }

    #[test]
    fn test_rust_code_analysis_rule_new_creates_instance() {
        let rule = RustCodeAnalysisRule::new();
        // Just verify the rule can be created without panicking
        let _ = rule;
    }

    #[test]
    fn test_all_output_format_variants_exist() {
        // Test that Json variant exists
        let json_format = AllOutputFormat::Json;
        assert!(
            matches!(json_format, AllOutputFormat::Json),
            "AllOutputFormat::Json should exist"
        );

        // Test that Html variant exists
        let html_format = AllOutputFormat::Html;
        assert!(
            matches!(html_format, AllOutputFormat::Html),
            "AllOutputFormat::Html should exist"
        );
    }

    #[test]
    fn test_json_report_data_lifetime_annotation() {
        // This test verifies that JsonReportData can hold references to data
        // with the correct lifetime annotation
        let errors = vec!["test error".to_string()];
        let report_data = JsonReportData {
            statement_count: None,
            volatility: None,
            coupling: None,
            rust_code_analysis: None,
            errors,
        };

        assert_eq!(
            report_data.errors.len(),
            1,
            "errors vector should contain 1 error"
        );
        assert_eq!(
            report_data.errors[0], "test error",
            "error message should match"
        );
    }
}
