//! CI/CD Platform-Friendly Report Generation
//!
//! Provides SARIF and JUnit XML output formats for CI/CD integration.
//!
//! # Overview
//!
//! This module defines types and functions for generating analysis reports in
//! formats consumed by CI/CD platforms like GitHub Actions (SARIF) and Azure
//! DevOps (JUnit XML).
//!
//! # Usage
//!
//! Each analysis rule implements the [`ToFindings`] trait to convert its
//! output data into a list of [`Finding`] objects. These findings can then
//! be serialized to SARIF or JUnit formats.
//!
//! # Example
//!
//! ```rust,no_run
//! use raff_core::ci_report::{Finding, Severity, to_sarif, to_junit};
//!
//! let findings = vec![
//!     Finding {
//!         rule_id: "statement-count".to_string(),
//!         rule_name: "Statement Count Rule".to_string(),
//!         severity: Severity::Error,
//!         message: "Component too large".to_string(),
//!         location: None,
//!         help_uri: Some("https://github.com/liamwh/raff/docs/statement-count".to_string()),
//!         fingerprint: Some("unique-id".to_string()),
//!     }
//! ];
//!
//! let sarif = to_sarif(&findings)?;
//! let junit = to_junit(&findings, "my-test-suite")?;
//! # Ok::<(), raff_core::error::RaffError>(())
//! ```

use crate::error::{RaffError, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// Severity level for CI report findings.
///
/// Maps to CI platform conventions:
/// - **Error**: Fails the build (SARIF `error`, JUnit `<failure>`)
/// - **Warning**: Informational only (SARIF `warning`, JUnit `<system-out>`)
/// - **Note**: Informational (SARIF `note`, JUnit passed test)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Error severity - causes CI failure
    Error,
    /// Warning severity - informational, does not fail CI
    Warning,
    /// Note severity - informational, does not fail CI
    Note,
}

impl Severity {
    /// Returns the SARIF level string for this severity.
    #[must_use]
    pub const fn to_sarif_level(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        }
    }

    /// Returns whether this severity should cause a CI failure.
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// A finding from an architectural rule analysis.
///
/// Findings represent individual issues or observations detected by analysis
/// rules. Each finding includes information about what rule detected it,
/// where it occurred, and how severe it is.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Unique identifier for the rule that generated this finding.
    /// Used as `ruleId` in SARIF output.
    pub rule_id: String,

    /// Human-readable name of the rule.
    pub rule_name: String,

    /// Severity level of this finding.
    pub severity: Severity,

    /// Human-readable message describing the finding.
    pub message: String,

    /// Optional location information for the finding.
    pub location: Option<Location>,

    /// Optional URI to documentation about this rule/finding.
    pub help_uri: Option<String>,

    /// Stable fingerprint for deduplication.
    /// Format-independent identifier that maps to SARIF `partialFingerprints`.
    pub fingerprint: Option<String>,
}

/// Location information for a finding.
///
/// Provides file path and optional line number information to help
/// pinpoint the source of a finding.
#[derive(Debug, Clone, Serialize)]
pub struct Location {
    /// Repo-relative file path (e.g., "src/main.rs").
    /// Paths are normalized to use forward slashes on all platforms.
    pub uri: String,

    /// Optional start line number (1-indexed).
    pub start_line: Option<usize>,

    /// Optional end line number (1-indexed).
    pub end_line: Option<usize>,
}

impl Location {
    /// Creates a new location with just a URI.
    #[must_use]
    pub fn new(uri: String) -> Self {
        Self {
            uri,
            start_line: None,
            end_line: None,
        }
    }

    /// Creates a new location with a URI and line range.
    #[must_use]
    pub fn with_lines(uri: String, start_line: usize, end_line: usize) -> Self {
        Self {
            uri,
            start_line: Some(start_line),
            end_line: Some(end_line),
        }
    }
}

/// Trait for converting rule data to CI findings.
///
/// Each analysis rule implements this trait to convert its output data
/// into a format-agnostic list of findings that can be serialized to
/// SARIF or JUnit.
pub trait ToFindings {
    /// Converts the rule's output data into a list of findings.
    ///
    /// Each finding represents an issue or observation detected by the rule.
    /// Rules are responsible for setting appropriate severity levels.
    fn to_findings(&self) -> Vec<Finding>;
}

/// Normalizes a path to repo-relative format with forward slashes.
///
/// # Arguments
///
/// * `path` - The path to normalize
/// * `repo_root` - The repository root path to strip from `path`
///
/// # Returns
///
/// A repo-relative path string with forward slashes (e.g., "src/main.rs").
///
/// # Examples
///
/// ```
/// use raff_core::ci_report::normalize_repo_relative;
/// use std::path::Path;
///
/// let repo_root = Path::new("/Users/liam/git/raff");
/// let file_path = Path::new("/Users/liam/git/raff/src/main.rs");
/// let normalized = normalize_repo_relative(file_path, repo_root);
/// assert_eq!(normalized, "src/main.rs");
/// ```
#[must_use]
pub fn normalize_repo_relative(path: &Path, repo_root: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Converts findings to SARIF v2.1.0 format.
///
/// # Arguments
///
/// * `findings` - Slice of findings to convert
///
/// # Returns
///
/// A JSON string containing the SARIF report.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
///
/// # SARIF Format
///
/// The output follows the SARIF v2.1.0 specification:
/// - `tool.driver.name` = "raff"
/// - `tool.driver.version` = current version
/// - `tool.driver.rules[]` = de-duplicated list of unique rule IDs
/// - `results[]` = one entry per finding
/// - `results[].ruleId` = finding's rule_id
/// - `results[].level` = error/warning/note based on severity
/// - `results[].message.text` = finding's message
/// - `results[].locations[]` = location info if present
/// - `results[].partialFingerprints["primaryLocation"]` = finding's fingerprint
pub fn to_sarif(findings: &[Finding]) -> Result<String> {
    // Group findings by rule_id for de-duplicated tool.driver.rules
    let mut unique_rules: HashMap<String, SarifRule> = HashMap::new();

    for finding in findings {
        unique_rules
            .entry(finding.rule_id.clone())
            .or_insert_with(|| SarifRule {
                id: finding.rule_id.clone(),
                name: finding.rule_name.clone(),
                help_uri: finding.help_uri.clone(),
            });
    }

    let rules: Vec<SarifRule> = unique_rules.into_values().collect();

    let sarif_results: Vec<SarifResult> = findings.iter().map(SarifResult::from_finding).collect();

    let sarif_log = SarifLog {
        version: "2.1.0",
        schema: "https://json.schemastore.org/sarif-2.1.0.json",
        runs: vec![SarifRun {
            tool: SarifTool {
                driver: SarifDriver {
                    name: "raff".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    rules,
                },
            },
            results: sarif_results,
            invocations: None, // Can be added later if needed
        }],
    };

    serde_json::to_string_pretty(&sarif_log)
        .map_err(|e| RaffError::config_error(format!("Failed to serialize SARIF: {}", e)))
}

/// Converts findings to JUnit XML format.
///
/// # Arguments
///
/// * `findings` - Slice of findings to convert
/// * `suite_name` - Name for the test suite (e.g., "raff-all-rules")
///
/// # Returns
///
/// An XML string containing the JUnit report.
///
/// # Errors
///
/// Returns an error if XML serialization fails.
///
/// # JUnit Format
///
/// The output follows the JUnit XML schema:
/// - Each finding becomes a `<testcase>`
/// - Error findings add a `<failure>` element (causes test failure)
/// - Warning findings add a `<system-out>` element (informational only)
/// - Note findings produce passing tests (no failure element)
/// - Always writes a valid XML file, even with zero findings
pub fn to_junit(findings: &[Finding], suite_name: &str) -> Result<String> {
    let mut testcase_count = findings.len();
    let failure_count = findings.iter().filter(|f| f.severity.is_error()).count();

    // Ensure at least one test case (empty suite)
    if testcase_count == 0 {
        testcase_count = 1;
    }

    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    xml.push_str(&format!(
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"0\">",
        escape_xml(suite_name),
        testcase_count,
        failure_count
    ));

    xml.push_str(&format!(
        "<testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"0\">",
        escape_xml(suite_name),
        testcase_count,
        failure_count
    ));

    if findings.is_empty() {
        // Empty suite - include a placeholder test
        xml.push_str(&format!(
            "<testcase name=\"{}\" classname=\"{}\"/>",
            escape_xml(suite_name),
            escape_xml(suite_name)
        ));
    } else {
        for finding in findings {
            let classname = format!("raff.{}", finding.rule_id);
            let testcase_name = truncate_testcase_name(&finding.message);

            xml.push_str("<testcase ");
            xml.push_str(&format!("name=\"{}\" ", escape_xml(&testcase_name)));
            xml.push_str(&format!("classname=\"{}\"", escape_xml(&classname)));

            match finding.severity {
                Severity::Error => {
                    xml.push('>');
                    xml.push_str("<failure ");
                    if let Some(loc) = &finding.location {
                        xml.push_str(&format!(
                            "message=\"{}: {}\">",
                            escape_xml(&loc.uri),
                            escape_xml(&finding.message)
                        ));
                    } else {
                        xml.push_str(&format!("message=\"{}\">", escape_xml(&finding.message)));
                    }
                    if let Some(uri) = &finding.help_uri {
                        xml.push_str(&format!("\nHelp: {}\n", escape_xml(uri)));
                    }
                    xml.push_str("</failure>");
                    xml.push_str("</testcase>");
                }
                Severity::Warning => {
                    xml.push('>');
                    xml.push_str("<system-out>");
                    if let Some(loc) = &finding.location {
                        xml.push_str(&format!("{}: ", escape_xml(&loc.uri)));
                    }
                    xml.push_str(&escape_xml(&finding.message));
                    if let Some(uri) = &finding.help_uri {
                        xml.push_str(&format!("\nHelp: {}", escape_xml(uri)));
                    }
                    xml.push_str("</system-out>");
                    xml.push('>');
                }
                Severity::Note => {
                    // Note is informational - test passes
                    xml.push_str("/>");
                }
            }
        }
    }

    xml.push_str("</testsuite>");
    xml.push_str("</testsuites>");

    Ok(xml)
}

/// Escapes special XML characters.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Truncates a testcase name to a reasonable length.
/// JUnit parsers may have issues with very long names.
fn truncate_testcase_name(name: &str) -> String {
    if name.len() > 200 {
        format!("{}...", &name[..197])
    } else {
        name.to_string()
    }
}

// SARIF types for serialization

#[derive(Debug, Serialize)]
struct SarifLog {
    version: &'static str,
    #[serde(rename = "$schema")]
    schema: &'static str,
    runs: Vec<SarifRun>,
}

#[derive(Debug, Serialize)]
struct SarifRun {
    tool: SarifTool,
    results: Vec<SarifResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    invocations: Option<Vec<SarifInvocation>>,
}

#[derive(Debug, Serialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(Debug, Serialize)]
struct SarifDriver {
    name: String,
    version: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    rules: Vec<SarifRule>,
}

#[derive(Debug, Serialize)]
struct SarifRule {
    #[serde(rename = "id")]
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "helpUri")]
    help_uri: Option<String>,
}

#[derive(Debug, Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    level: String,
    message: SarifMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    locations: Option<Vec<SarifLocation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "partialFingerprints")]
    partial_fingerprints: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct SarifMessage {
    text: String,
}

#[derive(Debug, Serialize)]
struct SarifLocation {
    physical_location: SarifPhysicalLocation,
}

#[derive(Debug, Serialize)]
struct SarifPhysicalLocation {
    artifact_location: SarifArtifactLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<SarifRegion>,
}

#[derive(Debug, Serialize)]
struct SarifArtifactLocation {
    uri: String,
}

#[derive(Debug, Serialize)]
struct SarifRegion {
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<usize>,
}

#[derive(Debug, Serialize)]
struct SarifInvocation {
    #[serde(rename = "executionSuccessful")]
    execution_successful: bool,
    #[serde(rename = "endTimeUtc")]
    end_time_utc: String,
}

impl SarifResult {
    fn from_finding(finding: &Finding) -> Self {
        let locations = finding.location.as_ref().map(|loc| {
            vec![SarifLocation {
                physical_location: SarifPhysicalLocation {
                    artifact_location: SarifArtifactLocation {
                        uri: loc.uri.clone(),
                    },
                    region: if loc.start_line.is_some() || loc.end_line.is_some() {
                        Some(SarifRegion {
                            start_line: loc.start_line,
                            end_line: loc.end_line,
                        })
                    } else {
                        None
                    },
                },
            }]
        });

        let partial_fingerprints = finding.fingerprint.as_ref().map(|fp| {
            let mut map = HashMap::new();
            map.insert("primaryLocation".to_string(), fp.clone());
            map
        });

        Self {
            rule_id: finding.rule_id.clone(),
            level: finding.severity.to_sarif_level().to_string(),
            message: SarifMessage {
                text: finding.message.clone(),
            },
            locations,
            partial_fingerprints,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_to_sarif_level() {
        assert_eq!(Severity::Error.to_sarif_level(), "error");
        assert_eq!(Severity::Warning.to_sarif_level(), "warning");
        assert_eq!(Severity::Note.to_sarif_level(), "note");
    }

    #[test]
    fn test_severity_is_error() {
        assert!(Severity::Error.is_error());
        assert!(!Severity::Warning.is_error());
        assert!(!Severity::Note.is_error());
    }

    #[test]
    fn test_location_new() {
        let loc = Location::new("src/main.rs".to_string());
        assert_eq!(loc.uri, "src/main.rs");
        assert!(loc.start_line.is_none());
        assert!(loc.end_line.is_none());
    }

    #[test]
    fn test_location_with_lines() {
        let loc = Location::with_lines("src/main.rs".to_string(), 10, 20);
        assert_eq!(loc.uri, "src/main.rs");
        assert_eq!(loc.start_line, Some(10));
        assert_eq!(loc.end_line, Some(20));
    }

    #[test]
    fn test_normalize_repo_relative_with_repo_root() {
        let repo_root = Path::new("/Users/liam/git/raff");
        let file_path = Path::new("/Users/liam/git/raff/src/main.rs");
        let normalized = normalize_repo_relative(file_path, repo_root);
        assert_eq!(normalized, "src/main.rs");
    }

    #[test]
    fn test_normalize_repo_relative_without_repo_root() {
        let repo_root = Path::new("/other/repo");
        let file_path = Path::new("/Users/liam/git/raff/src/main.rs");
        let normalized = normalize_repo_relative(file_path, repo_root);
        assert_eq!(normalized, "/Users/liam/git/raff/src/main.rs");
    }

    #[test]
    fn test_normalize_repo_relative_windows_paths() {
        // On non-Windows platforms, backslashes are part of the path name
        // The test verifies that backslashes get converted to forward slashes
        let repo_root = Path::new("project/repo");
        let file_path = Path::new("project/repo/src/main.rs");
        let normalized = normalize_repo_relative(file_path, repo_root);
        assert_eq!(normalized, "src/main.rs");
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("normal"), "normal");
        assert_eq!(escape_xml("a < b"), "a &lt; b");
        assert_eq!(escape_xml("a > b"), "a &gt; b");
        assert_eq!(escape_xml("a & b"), "a &amp; b");
        assert_eq!(escape_xml("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(escape_xml("'apostrophe'"), "&apos;apostrophe&apos;");
    }

    #[test]
    fn test_truncate_testcase_name_short() {
        let short = "short name";
        assert_eq!(truncate_testcase_name(short), short);
    }

    #[test]
    fn test_truncate_testcase_name_long() {
        let long = "a".repeat(300);
        let truncated = truncate_testcase_name(&long);
        assert!(truncated.len() <= 200);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_to_sarif_empty() {
        let json = to_sarif(&[]).expect("SARIF generation should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("Generated SARIF should be valid JSON");

        assert_eq!(parsed["version"], "2.1.0");
        assert_eq!(parsed["runs"][0]["tool"]["driver"]["name"], "raff");
        assert!(parsed["runs"][0]["results"].is_array());
        assert_eq!(
            parsed["runs"][0]["results"]
                .as_array()
                .map_or(0, |v| v.len()),
            0
        );
    }

    #[test]
    fn test_to_sarif_with_findings() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Error,
            message: "Test finding".to_string(),
            location: Some(Location::new("src/test.rs".to_string())),
            help_uri: Some("https://example.com/docs".to_string()),
            fingerprint: Some("test-fingerprint".to_string()),
        }];

        let json = to_sarif(&findings).expect("SARIF generation should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("Generated SARIF should be valid JSON");

        assert_eq!(parsed["runs"][0]["results"][0]["ruleId"], "test-rule");
        assert_eq!(parsed["runs"][0]["results"][0]["level"], "error");
        assert_eq!(
            parsed["runs"][0]["results"][0]["message"]["text"],
            "Test finding"
        );
    }

    #[test]
    fn test_to_sarif_deduplicates_rules() {
        let findings = vec![
            Finding {
                rule_id: "rule-a".to_string(),
                rule_name: "Rule A".to_string(),
                severity: Severity::Error,
                message: "Finding 1".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "rule-a".to_string(),
                rule_name: "Rule A".to_string(),
                severity: Severity::Warning,
                message: "Finding 2".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
        ];

        let json = to_sarif(&findings).expect("SARIF generation should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("Generated SARIF should be valid JSON");

        // Should have exactly one rule in driver.rules
        let rules = parsed["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .expect("rules should be an array");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["id"], "rule-a");

        // But two results
        let results = parsed["runs"][0]["results"]
            .as_array()
            .expect("results should be an array");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_to_junit_empty() {
        let xml = to_junit(&[], "test-suite").expect("JUnit generation should succeed");

        assert!(xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<testsuites"));
        assert!(xml.contains("name=\"test-suite\""));
        assert!(xml.contains("tests=\"1\""));
        assert!(xml.contains("failures=\"0\""));
    }

    #[test]
    fn test_to_junit_with_error_finding() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Error,
            message: "Test error".to_string(),
            location: Some(Location::new("src/test.rs".to_string())),
            help_uri: None,
            fingerprint: None,
        }];

        let xml = to_junit(&findings, "test-suite").expect("JUnit generation should succeed");

        assert!(xml.contains("<testcase"));
        assert!(xml.contains("<failure"));
        assert!(xml.contains("Test error"));
        assert!(xml.contains("src/test.rs"));
        assert!(xml.contains("failures=\"1\""));
    }

    #[test]
    fn test_to_junit_with_warning_finding() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Warning,
            message: "Test warning".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        }];

        let xml = to_junit(&findings, "test-suite").expect("JUnit generation should succeed");

        assert!(xml.contains("<testcase"));
        assert!(xml.contains("<system-out>"));
        assert!(xml.contains("Test warning"));
        assert!(!xml.contains("<failure"));
    }

    #[test]
    fn test_to_junit_with_note_finding() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Note,
            message: "Test note".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        }];

        let xml = to_junit(&findings, "test-suite").expect("JUnit generation should succeed");

        assert!(xml.contains("<testcase"));
        // Note findings don't add any extra elements (test passes)
        assert!(!xml.contains("<failure"));
        assert!(!xml.contains("<system-out>"));
    }

    #[test]
    fn test_to_junit_escapes_special_characters() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Error,
            message: "Error: x < y & y > z".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        }];

        let xml = to_junit(&findings, "test-suite").expect("JUnit generation should succeed");

        assert!(xml.contains("&lt;"));
        assert!(xml.contains("&gt;"));
        assert!(xml.contains("&amp;"));
    }

    #[test]
    fn test_finding_serialization() {
        let finding = Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Warning,
            message: "Test message".to_string(),
            location: Some(Location::with_lines("src/test.rs".to_string(), 10, 20)),
            help_uri: Some("https://example.com".to_string()),
            fingerprint: Some("abc123".to_string()),
        };

        let json = serde_json::to_string(&finding).expect("Finding should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("Serialized JSON should be valid");

        assert_eq!(parsed["rule_id"], "test-rule");
        assert_eq!(parsed["rule_name"], "Test Rule");
        assert_eq!(parsed["severity"], "warning");
        assert_eq!(parsed["message"], "Test message");
        assert_eq!(parsed["location"]["uri"], "src/test.rs");
        assert_eq!(parsed["location"]["start_line"], 10);
        assert_eq!(parsed["location"]["end_line"], 20);
        assert_eq!(parsed["help_uri"], "https://example.com");
        assert_eq!(parsed["fingerprint"], "abc123");
    }
}
