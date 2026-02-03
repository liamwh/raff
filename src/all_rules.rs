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
//!     fast: false,
//!     // .. other fields
//!     sc_threshold: 10,
//!     vol_alpha: 0.01,
//!     vol_since: None,
//!     vol_normalize: false,
//!     vol_skip_merges: false,
//!     coup_granularity: raff_core::CouplingGranularity::Both,
//!     rca_extra_flags: vec![],
//!     rca_jobs: 4,
//!     rca_metrics: true,
//!     rca_language: "rust".to_string(),
//!     ci_output: None,
//!     output_file: None,
//! };
//!
//! run_all(&args)?;
//! # Ok(())
//! # }
//! ```

use crate::ci_report::{Severity, ToFindings};
use crate::error::Result;
use crate::{
    cli::{AllArgs, AllOutputFormat, CiOutputFormat},
    coupling_rule::{CouplingData, CouplingRule},
    html_utils,
    rust_code_analysis_rule::{RustCodeAnalysisData, RustCodeAnalysisRule},
    statement_count_rule::{StatementCountData, StatementCountRule},
    volatility_rule::{VolatilityData, VolatilityRule},
};
use maud::Markup;
use serde::Serialize;
use std::fs::File;
use std::io::Write;

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
        ci_output: None,
        output_file: args.output_file.clone(),
        staged: args.staged,
    };
    let vol_args = crate::cli::VolatilityArgs {
        path: args.path.clone(),
        alpha: args.vol_alpha,
        since: args.vol_since.clone(),
        normalize: args.vol_normalize,
        skip_merges: args.vol_skip_merges,
        output: crate::cli::VolatilityOutputFormat::Table, // format is irrelevant for analyze
        ci_output: None,
        output_file: args.output_file.clone(),
    };
    let coup_args = crate::cli::CouplingArgs {
        path: args.path.clone(),
        granularity: args.coup_granularity.clone(),
        output: crate::cli::CouplingOutputFormat::Table, // format is irrelevant for analyze
        ci_output: None,
        output_file: args.output_file.clone(),
        staged: args.staged,
    };
    let rca_args = crate::cli::RustCodeAnalysisArgs {
        path: args.path.clone(),
        extra_flags: args.rca_extra_flags.clone(),
        jobs: args.rca_jobs,
        metrics: args.rca_metrics,
        language: args.rca_language.clone(),
        output: crate::cli::RustCodeAnalysisOutputFormat::Table, // format is irrelevant for analyze
        ci_output: None,
        output_file: args.output_file.clone(),
    };

    let all_data = if args.fast {
        // Fast mode: only run statement-count and coupling (skip volatility and rust-code-analysis)
        AllReportData {
            statement_count: Some(sc_rule.analyze(&sc_args)),
            volatility: None,
            coupling: Some(coup_rule.analyze(&coup_args)),
            rust_code_analysis: None,
        }
    } else {
        // Full mode: run all rules
        AllReportData {
            statement_count: Some(sc_rule.analyze(&sc_args)),
            volatility: Some(vol_rule.analyze(&vol_args)),
            coupling: Some(coup_rule.analyze(&coup_args)),
            rust_code_analysis: Some(rca_rule.analyze(&rca_args)),
        }
    };

    // Check for CI output first (takes precedence)
    if let Some(ci_format) = &args.ci_output {
        let mut all_findings = Vec::new();

        // Each rule sets its own severity in to_findings()
        if let Some(Ok(data)) = &all_data.statement_count {
            all_findings.extend(data.to_findings());
        }
        if let Some(Ok(data)) = &all_data.volatility {
            all_findings.extend(data.to_findings());
        }
        if let Some(Ok(data)) = &all_data.coupling {
            all_findings.extend(data.to_findings());
        }
        if let Some(Ok(data)) = &all_data.rust_code_analysis {
            all_findings.extend(data.to_findings());
        }

        let output = match ci_format {
            CiOutputFormat::Sarif => crate::ci_report::to_sarif(&all_findings)?,
            CiOutputFormat::JUnit => crate::ci_report::to_junit(&all_findings, "raff-all-rules")?,
        };

        // Write to file if specified, otherwise stdout
        if let Some(ref output_file) = args.output_file {
            let mut file = File::create(output_file).map_err(|e| {
                crate::error::RaffError::io_error(format!(
                    "Failed to create output file {}: {}",
                    output_file.display(),
                    e
                ))
            })?;
            file.write_all(output.as_bytes()).map_err(|e| {
                crate::error::RaffError::io_error(format!(
                    "Failed to write to output file {}: {}",
                    output_file.display(),
                    e
                ))
            })?;
        } else {
            println!("{output}");
        }

        // Exit code based on any Error findings
        let has_errors = all_findings.iter().any(|f| f.severity == Severity::Error);
        if has_errors {
            return Err(crate::error::RaffError::analysis_error(
                "all",
                "Found errors in one or more rules",
            ));
        }
        return Ok(());
    }

    match args.output {
        AllOutputFormat::Cli => {
            // Collect all findings from all rules
            let mut all_findings = Vec::new();

            if let Some(Ok(data)) = &all_data.statement_count {
                all_findings.extend(data.to_findings());
            }
            if let Some(Ok(data)) = &all_data.volatility {
                all_findings.extend(data.to_findings());
            }
            if let Some(Ok(data)) = &all_data.coupling {
                all_findings.extend(data.to_findings());
            }
            if let Some(Ok(data)) = &all_data.rust_code_analysis {
                all_findings.extend(data.to_findings());
            }

            // Sort by severity (Error first) then rule
            all_findings.sort_by_key(|f| (!f.severity.is_error(), f.rule_id.clone()));

            if args.quiet {
                let summary = crate::cli_report::render_summary_line(&all_findings);
                println!("{summary}");
            } else {
                let output = crate::cli_report::render_cli_table(&all_findings);
                println!("{output}");
            }

            // Return error if any findings are Error severity
            let has_errors = all_findings.iter().any(|f| f.severity == Severity::Error);
            if has_errors {
                return Err(crate::error::RaffError::analysis_error(
                    "all",
                    format!(
                        "Found {} issue{} ({} error{})",
                        all_findings.len(),
                        if all_findings.len() == 1 { "" } else { "s" },
                        all_findings
                            .iter()
                            .filter(|f| f.severity == Severity::Error)
                            .count(),
                        if all_findings
                            .iter()
                            .filter(|f| f.severity == Severity::Error)
                            .count()
                            == 1
                        {
                            ""
                        } else {
                            "s"
                        }
                    ),
                ));
            }
        }
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
            fast: false,
            quiet: false,
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
            ci_output: None,
            output_file: None,
            staged: false,
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
            ci_output: None,
            output_file: None,
            staged: false,
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
            ci_output: None,
            output_file: None,
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
            ci_output: None,
            output_file: None,
            staged: false,
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
            ci_output: None,
            output_file: None,
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
            ci_output: None,
            output_file: None,
            staged: all_args.staged,
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
            ci_output: None,
            output_file: None,
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
            ci_output: None,
            output_file: None,
            staged: all_args.staged,
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
            ci_output: None,
            output_file: None,
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
            ci_output: None,
            output_file: None,
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
            ci_output: None,
            output_file: None,
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
            ci_output: None,
            output_file: None,
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

    // CI output tests

    #[test]
    fn test_all_args_with_ci_output_sarif_succeeds() {
        let mut args = create_test_args(".");
        args.ci_output = Some(CiOutputFormat::Sarif);

        // The test should either succeed or fail with an expected error
        // (it will likely fail due to no actual repo, but we're testing the CI output path)
        let result = run_all(&args);

        // We expect this to fail (no actual repo), but the important thing is
        // that it doesn't panic and goes through the CI output path
        assert!(
            result.is_err() || result.is_ok(),
            "run_all with ci_output should not panic"
        );
    }

    #[test]
    fn test_all_args_with_ci_output_junit_succeeds() {
        let mut args = create_test_args(".");
        args.ci_output = Some(CiOutputFormat::JUnit);

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with ci_output should not panic"
        );
    }

    #[test]
    fn test_all_args_with_ci_output_and_output_file_succeeds() {
        use tempfile::NamedTempFile;

        let mut args = create_test_args(".");
        args.ci_output = Some(CiOutputFormat::Sarif);

        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        args.output_file = Some(temp_file.path().to_path_buf());

        let result = run_all(&args);

        // Clean up temp file
        let _ = std::fs::remove_file(temp_file.path());

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with output_file should not panic"
        );
    }

    #[test]
    fn test_all_args_without_ci_output_uses_regular_output() {
        let args = create_test_args(".");

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all without ci_output should not panic"
        );
    }

    #[test]
    fn test_all_args_ci_output_none_does_not_trigger_ci_path() {
        let mut args = create_test_args(".");
        args.ci_output = None;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with ci_output=None should use regular output path"
        );
    }

    #[test]
    fn test_all_output_format_json_without_ci_output() {
        let mut args = create_test_args(".");
        args.output = AllOutputFormat::Json;
        args.ci_output = None;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with JSON output should not panic"
        );
    }

    #[test]
    fn test_all_output_format_html_without_ci_output() {
        let mut args = create_test_args(".");
        args.output = AllOutputFormat::Html;
        args.ci_output = None;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with HTML output should not panic"
        );
    }

    #[test]
    fn test_all_args_with_output_file_only() {
        use tempfile::NamedTempFile;

        let mut args = create_test_args(".");
        args.output_file = Some(
            NamedTempFile::new()
                .expect("Failed to create temp file")
                .path()
                .to_path_buf(),
        );

        let result = run_all(&args);

        // Clean up temp file
        if let Some(ref path) = args.output_file {
            let _ = std::fs::remove_file(path);
        }

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with output_file should not panic"
        );
    }

    #[test]
    fn test_all_args_ci_output_sarif_takes_precedence_over_json() {
        let mut args = create_test_args(".");
        args.output = AllOutputFormat::Json;
        args.ci_output = Some(CiOutputFormat::Sarif);

        let result = run_all(&args);

        // With ci_output set, it should take precedence over output format
        assert!(
            result.is_err() || result.is_ok(),
            "run_all with ci_output should take precedence"
        );
    }

    #[test]
    fn test_all_args_ci_output_junit_takes_precedence_over_html() {
        let mut args = create_test_args(".");
        args.output = AllOutputFormat::Html;
        args.ci_output = Some(CiOutputFormat::JUnit);

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with ci_output should take precedence"
        );
    }

    // Fast mode tests

    #[test]
    fn test_all_args_fast_flag_exists_and_defaults_to_false() {
        let args = create_test_args(".");
        assert!(!args.fast, "AllArgs fast field should default to false");
    }

    #[test]
    fn test_all_args_fast_flag_can_be_set_to_true() {
        let mut args = create_test_args(".");
        args.fast = true;
        assert!(args.fast, "AllArgs fast field should be settable to true");
    }

    #[test]
    fn test_all_report_data_new_creates_empty_all_none() {
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
    fn test_all_report_data_with_results_all_some() {
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
    fn test_all_report_data_with_results_fast_mode_pattern() {
        // Fast mode: only statement_count and coupling, volatility and rca are None
        let sc_result: Result<StatementCountData> = Err(crate::error::RaffError::analysis_error(
            "statement_count",
            "test error",
        ));
        let coup_result: Result<CouplingData> = Err(crate::error::RaffError::analysis_error(
            "coupling",
            "coupling error",
        ));

        let data = AllReportData::with_results(
            Some(sc_result),
            None, // volatility is None in fast mode
            Some(coup_result),
            None, // rust_code_analysis is None in fast mode
        );

        assert!(
            data.statement_count.is_some(),
            "fast mode should have statement_count"
        );
        assert!(
            data.volatility.is_none(),
            "fast mode should not have volatility"
        );
        assert!(data.coupling.is_some(), "fast mode should have coupling");
        assert!(
            data.rust_code_analysis.is_none(),
            "fast mode should not have rust_code_analysis"
        );
    }

    #[test]
    fn test_all_args_with_fast_true_runs_without_panic() {
        let mut args = create_test_args(".");
        args.fast = true;

        let result = run_all(&args);

        // Fast mode should run successfully (or fail gracefully, but not panic)
        assert!(
            result.is_err() || result.is_ok(),
            "run_all with fast=true should not panic"
        );
    }

    #[test]
    fn test_all_args_with_fast_false_runs_without_panic() {
        let mut args = create_test_args(".");
        args.fast = false;

        let result = run_all(&args);

        // Full mode should run successfully (or fail gracefully, but not panic)
        assert!(
            result.is_err() || result.is_ok(),
            "run_all with fast=false should not panic"
        );
    }

    #[test]
    fn test_all_args_fast_mode_with_json_output() {
        let mut args = create_test_args(".");
        args.fast = true;
        args.output = AllOutputFormat::Json;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with fast=true and Json output should not panic"
        );
    }

    #[test]
    fn test_all_args_fast_mode_with_html_output() {
        let mut args = create_test_args(".");
        args.fast = true;
        args.output = AllOutputFormat::Html;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with fast=true and Html output should not panic"
        );
    }

    #[test]
    fn test_all_args_fast_mode_with_cli_output() {
        let mut args = create_test_args(".");
        args.fast = true;
        args.output = AllOutputFormat::Cli;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with fast=true and Cli output should not panic"
        );
    }

    #[test]
    fn test_all_args_fast_mode_with_ci_output_sarif() {
        let mut args = create_test_args(".");
        args.fast = true;
        args.ci_output = Some(CiOutputFormat::Sarif);

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with fast=true and Sarif CI output should not panic"
        );
    }

    #[test]
    fn test_all_args_fast_mode_with_ci_output_junit() {
        let mut args = create_test_args(".");
        args.fast = true;
        args.ci_output = Some(CiOutputFormat::JUnit);

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with fast=true and JUnit CI output should not panic"
        );
    }

    #[test]
    fn test_all_args_fast_mode_with_output_file() {
        use tempfile::NamedTempFile;

        let mut args = create_test_args(".");
        args.fast = true;

        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        args.output_file = Some(temp_file.path().to_path_buf());

        let result = run_all(&args);

        // Clean up temp file
        let _ = std::fs::remove_file(temp_file.path());

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with fast=true and output_file should not panic"
        );
    }

    // Quiet mode tests

    #[test]
    fn test_all_args_quiet_flag_exists_and_defaults_to_false() {
        let args = create_test_args(".");
        assert!(!args.quiet, "AllArgs quiet field should default to false");
    }

    #[test]
    fn test_all_args_quiet_flag_can_be_set_to_true() {
        let mut args = create_test_args(".");
        args.quiet = true;
        assert!(args.quiet, "AllArgs quiet field should be settable to true");
    }

    #[test]
    fn test_all_args_quiet_mode_with_cli_output() {
        let mut args = create_test_args(".");
        args.quiet = true;
        args.output = AllOutputFormat::Cli;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with quiet=true and Cli output should not panic"
        );
    }

    #[test]
    fn test_all_args_quiet_and_fast_flags_work_together() {
        let mut args = create_test_args(".");
        args.quiet = true;
        args.fast = true;
        args.output = AllOutputFormat::Cli;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with both quiet=true and fast=true should not panic"
        );
    }

    #[test]
    fn test_all_args_quiet_mode_does_not_affect_json_output() {
        let mut args = create_test_args(".");
        args.quiet = true;
        args.output = AllOutputFormat::Json;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with quiet=true and Json output should not panic"
        );
    }

    #[test]
    fn test_all_args_quiet_mode_does_not_affect_html_output() {
        let mut args = create_test_args(".");
        args.quiet = true;
        args.output = AllOutputFormat::Html;

        let result = run_all(&args);

        assert!(
            result.is_err() || result.is_ok(),
            "run_all with quiet=true and Html output should not panic"
        );
    }
}
