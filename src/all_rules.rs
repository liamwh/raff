//! Orchestration for running all analysis rules.
//!
//! This module provides [`run_all`], which executes all configured analysis rules
//! and produces consolidated reports. It is used by the CLI's "all" command to
//! run multiple analyses in a single invocation.
//!
//! # Failing closed
//!
//! A rule that runs but returns an error becomes an error-severity finding
//! ("Could not run: ..."), so the run exits non-zero instead of reporting a
//! clean result. The one exception is a missing external tool
//! ([`RaffError::ToolNotFound`]), which becomes a note ("Skipped: ...") and does
//! not fail the run.
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
//!     quiet: false,
//!     fail_on_warnings: false,
//!     staged: false,
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

use crate::ci_report::{Finding, Severity, ToFindings};
use crate::cli_report::render_summary_line;
use crate::error::{RaffError, Result};
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

    /// Every finding from the rules that ran, plus one finding per rule that
    /// returned an error (see the module documentation).
    #[must_use]
    pub fn findings(&self) -> Vec<Finding> {
        let mut findings = Vec::new();
        collect_findings(
            &mut findings,
            self.statement_count.as_ref(),
            &STATEMENT_COUNT,
        );
        collect_findings(&mut findings, self.volatility.as_ref(), &VOLATILITY);
        collect_findings(&mut findings, self.coupling.as_ref(), &COUPLING);
        collect_findings(
            &mut findings,
            self.rust_code_analysis.as_ref(),
            &RUST_CODE_ANALYSIS,
        );
        findings
    }

    /// One finding per rule that returned an error instead of data.
    fn rule_errors(&self) -> Vec<Finding> {
        [
            rule_error(self.statement_count.as_ref(), &STATEMENT_COUNT),
            rule_error(self.volatility.as_ref(), &VOLATILITY),
            rule_error(self.coupling.as_ref(), &COUPLING),
            rule_error(self.rust_code_analysis.as_ref(), &RUST_CODE_ANALYSIS),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// How a rule identifies itself in findings.
struct RuleIdentity {
    id: &'static str,
    name: &'static str,
    help_uri: &'static str,
}

const STATEMENT_COUNT: RuleIdentity = RuleIdentity {
    id: "statement-count",
    name: "Statement Count Rule",
    help_uri: "https://github.com/liamwh/raff#statement-count",
};
const VOLATILITY: RuleIdentity = RuleIdentity {
    id: "volatility",
    name: "Code Volatility Rule",
    help_uri: "https://github.com/liamwh/raff#volatility",
};
const COUPLING: RuleIdentity = RuleIdentity {
    id: "coupling",
    name: "Code Coupling Rule",
    help_uri: "https://github.com/liamwh/raff#module-coupling",
};
const RUST_CODE_ANALYSIS: RuleIdentity = RuleIdentity {
    id: "rust-code-analysis",
    name: "Rust Code Analysis Rule",
    help_uri: "https://github.com/liamwh/raff#rust-code-analysis",
};

fn collect_findings<T: ToFindings>(
    findings: &mut Vec<Finding>,
    result: Option<&Result<T>>,
    rule: &RuleIdentity,
) {
    match result {
        Some(Ok(data)) => findings.extend(data.to_findings()),
        Some(Err(error)) => findings.push(rule_error_finding(rule, error)),
        None => {}
    }
}

fn rule_error<T>(result: Option<&Result<T>>, rule: &RuleIdentity) -> Option<Finding> {
    match result {
        Some(Err(error)) => Some(rule_error_finding(rule, error)),
        _ => None,
    }
}

/// The finding reported for a rule that returned `error` instead of data.
fn rule_error_finding(rule: &RuleIdentity, error: &RaffError) -> Finding {
    let (severity, message, outcome) = match error {
        RaffError::ToolNotFound { tool } => (
            Severity::Note,
            format!("Skipped: {tool} is not installed"),
            "skipped",
        ),
        _ => (
            Severity::Error,
            format!("Could not run: {error}"),
            "not-run",
        ),
    };
    Finding {
        rule_id: rule.id.to_string(),
        rule_name: rule.name.to_string(),
        severity,
        message,
        location: None,
        help_uri: Some(rule.help_uri.to_string()),
        fingerprint: Some(format!("{}:{outcome}", rule.id)),
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
        // In staged fast mode, skip statement-count because component-percentage
        // metrics over a staged subset are not meaningful.
        AllReportData {
            statement_count: if args.staged {
                None
            } else {
                Some(sc_rule.analyze(&sc_args))
            },
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
        let all_findings = all_data.findings();

        let output = match ci_format {
            CiOutputFormat::Sarif => crate::ci_report::to_sarif(&all_findings)?,
            CiOutputFormat::JUnit => crate::ci_report::to_junit(&all_findings, "raff-all-rules")?,
        };

        // Write to file if specified, otherwise stdout
        if let Some(output_file) = &args.output_file {
            let mut file = File::create(output_file).map_err(|e| {
                RaffError::io_error(format!(
                    "Failed to create output file {}: {}",
                    output_file.display(),
                    e
                ))
            })?;
            file.write_all(output.as_bytes()).map_err(|e| {
                RaffError::io_error(format!(
                    "Failed to write to output file {}: {}",
                    output_file.display(),
                    e
                ))
            })?;
        } else {
            println!("{output}");
        }

        return check_findings(&all_findings, false);
    }

    match args.output {
        AllOutputFormat::Cli => {
            let mut all_findings = all_data.findings();

            // Sort by severity (Error first) then rule
            all_findings.sort_by_key(|f| (!f.severity.is_error(), f.rule_id.clone()));

            if args.quiet && all_findings.is_empty() {
                println!("{}", render_summary_line(&all_findings));
            } else {
                let output = crate::cli_report::render_cli_table(&all_findings);
                println!("{output}");
            }

            check_findings(&all_findings, args.fail_on_warnings)
        }
        AllOutputFormat::Json => {
            let rule_errors = all_data.rule_errors();
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
                errors: rule_errors
                    .iter()
                    .map(|finding| format!("{}: {}", finding.rule_name, finding.message))
                    .collect(),
            };

            let json = serde_json::to_string_pretty(&json_report)?;
            println!("{json}");
            check_findings(&rule_errors, false)
        }
        AllOutputFormat::Html => {
            let rule_errors = all_data.rule_errors();
            let mut html_body_parts: Vec<Markup> = vec![];

            if !rule_errors.is_empty() {
                html_body_parts.push(maud::html! {
                    h2 { "Rules that did not run" }
                    ul {
                        @for finding in &rule_errors {
                            li { b { (finding.rule_name) } ": " (finding.message) }
                        }
                    }
                });
            }
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
            check_findings(&rule_errors, false)
        }
    }
}

/// Fails the run when any finding is an error, or, with `fail_on_warnings`, a
/// warning. Notes never fail the run.
fn check_findings(findings: &[Finding], fail_on_warnings: bool) -> Result<()> {
    let fails = |severity: &Severity| {
        *severity == Severity::Error || (fail_on_warnings && *severity == Severity::Warning)
    };
    if findings.iter().any(|finding| fails(&finding.severity)) {
        return Err(RaffError::analysis_error(
            "all",
            format!("Found {}", render_summary_line(findings)),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn finding(severity: Severity) -> Finding {
        Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity,
            message: "message".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        }
    }

    #[test]
    fn test_rule_that_returns_an_error_becomes_an_error_finding_and_fails_the_run() {
        let data = AllReportData::with_results(
            None,
            Some(Err(RaffError::git_error("open Git repository"))),
            None,
            None,
        );

        let findings = data.findings();

        assert_eq!(findings.len(), 1);
        let failure = &findings[0];
        assert_eq!(failure.rule_id, "volatility");
        assert_eq!(failure.severity, Severity::Error);
        assert_eq!(
            failure.message,
            "Could not run: Git error during 'open Git repository': operation failed"
        );
        assert_eq!(
            failure.help_uri.as_deref(),
            Some("https://github.com/liamwh/raff#volatility")
        );
        assert!(check_findings(&findings, false).is_err());
    }

    #[test]
    fn test_missing_tool_becomes_a_note_and_does_not_fail_the_run() {
        let data = AllReportData::with_results(
            None,
            None,
            None,
            Some(Err(RaffError::tool_not_found("rust-code-analysis-cli"))),
        );

        let findings = data.findings();

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "rust-code-analysis");
        assert_eq!(findings[0].severity, Severity::Note);
        assert_eq!(
            findings[0].message,
            "Skipped: rust-code-analysis-cli is not installed"
        );
        assert!(check_findings(&findings, true).is_ok());
    }

    #[test]
    fn test_warnings_fail_the_run_only_with_fail_on_warnings() {
        let findings = [finding(Severity::Warning), finding(Severity::Note)];

        assert!(check_findings(&findings, false).is_ok());
        let error = check_findings(&findings, true).expect_err("warnings should fail");
        assert!(
            error
                .to_string()
                .ends_with("Found 2 findings (0 errors, 1 warning, 1 note)"),
            "unexpected message: {error}"
        );
    }

    fn write_single_crate(root: &Path) {
        fs::create_dir_all(root.join("src")).expect("Failed to create src");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("Failed to write Cargo.toml");
        fs::write(root.join("src/main.rs"), "fn main() {\n    let x = 1;\n}\n")
            .expect("Failed to write main.rs");
    }

    fn cli_args(path: &Path) -> AllArgs {
        AllArgs {
            path: path.to_path_buf(),
            output: AllOutputFormat::Cli,
            fast: false,
            quiet: true,
            fail_on_warnings: false,
            sc_threshold: 100,
            vol_alpha: 0.01,
            vol_since: None,
            vol_normalize: false,
            vol_skip_merges: false,
            coup_granularity: crate::cli::CouplingGranularity::Crate,
            rca_extra_flags: vec![],
            rca_jobs: 1,
            rca_metrics: true,
            rca_language: "rust".to_string(),
            ci_output: None,
            output_file: None,
            staged: false,
        }
    }

    #[test]
    fn test_run_all_fails_when_a_rule_cannot_run() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        write_single_crate(temp_dir.path());
        let args = cli_args(temp_dir.path());

        // Outside a Git repository volatility cannot run; every other rule passes.
        assert!(run_all(&args).is_err());

        let git = |git_args: &[&str]| {
            let status = Command::new("git")
                .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
                .args(["-c", "commit.gpgsign=false"])
                .args(git_args)
                .current_dir(temp_dir.path())
                .status()
                .expect("Failed to run git");
            assert!(status.success(), "git {git_args:?} failed");
        };
        git(&["init", "--quiet"]);
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "Initial commit"]);

        run_all(&args).expect("every rule can run inside a Git repository");
    }
}
