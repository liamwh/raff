//! CLI-friendly table output for consolidated analysis results.
//!
//! Provides concise, actionable tables that show:
//! - What rule detected the issue
//! - Severity level (with color coding)
//! - The issue message
//! - Where to find it (file location)
//! - Suggested action / documentation link

use crate::ci_report::{Finding, Severity};
use prettytable::{format, Attr, Cell, Row, Table};

/// Maximum width for the message column before truncation.
const MAX_MESSAGE_WIDTH: usize = 60;

/// Truncates a message to fit within the maximum width.
///
/// # Arguments
///
/// * `message` - The message to truncate
///
/// # Returns
///
/// A truncated message with "..." appended if it was too long.
#[must_use]
fn truncate_message(message: &str) -> String {
    if message.len() > MAX_MESSAGE_WIDTH {
        format!("{}...", &message[..MAX_MESSAGE_WIDTH.saturating_sub(3)])
    } else {
        message.to_string()
    }
}

/// Returns a shorthand severity label for display.
///
/// # Arguments
///
/// * `severity` - The severity level
///
/// # Returns
///
/// A short string label for the severity.
#[must_use]
const fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "ERR",
        Severity::Warning => "WARN",
        Severity::Note => "NOTE",
    }
}

/// Renders a single-line summary of findings.
///
/// This function produces a minimal one-line output format suitable for
/// pre-commit hooks and other scenarios where verbose output is undesirable.
///
/// # Arguments
///
/// * `findings` - Slice of findings to summarize
///
/// # Returns
///
/// A single-line formatted string with the summary.
///
/// # Example
///
/// ```rust,no_run
/// use raff_core::cli_report::render_summary_line;
/// use raff_core::ci_report::{Finding, Severity, Location};
///
/// let findings = vec![
///     Finding {
///         rule_id: "statement-count".to_string(),
///         rule_name: "Statement Count".to_string(),
///         severity: Severity::Error,
///         message: "Component too large".to_string(),
///         location: Some(Location::new("src/main.rs".to_string())),
///         help_uri: Some("https://example.com/docs".to_string()),
///         fingerprint: None,
///     }
/// ];
///
/// let summary = render_summary_line(&findings);
/// assert_eq!(summary, "1 findings (1 error, 0 warnings, 0 notes)");
/// ```
#[must_use]
pub fn render_summary_line(findings: &[Finding]) -> String {
    let total = findings.len();
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    let notes = findings
        .iter()
        .filter(|f| f.severity == Severity::Note)
        .count();

    format!(
        "{} finding{} ({} error{}, {} warning{}, {} note{})",
        total,
        if total == 1 { "" } else { "s" },
        errors,
        if errors == 1 { "" } else { "s" },
        warnings,
        if warnings == 1 { "" } else { "s" },
        notes,
        if notes == 1 { "" } else { "s" },
    )
}

/// Returns the color attribute for a given severity level.
///
/// # Arguments
///
/// * `severity` - The severity level
///
/// # Returns
///
/// A `prettytable::Attr` with the appropriate foreground color.
#[must_use]
const fn severity_color(severity: Severity) -> Attr {
    match severity {
        Severity::Error => Attr::ForegroundColor(prettytable::color::RED),
        Severity::Warning => Attr::ForegroundColor(prettytable::color::YELLOW),
        Severity::Note => Attr::ForegroundColor(prettytable::color::BLUE),
    }
}

/// Renders findings as an actionable CLI table.
///
/// Output format:
/// - One row per finding
/// - Columns: Severity | Rule | Location | Message | Action
/// - Color-coded by severity (red=Error, yellow=Warning, blue=Note)
/// - Truncates long messages to fit terminal width
///
/// # Arguments
///
/// * `findings` - Slice of findings to render
///
/// # Returns
///
/// A formatted string containing the table output.
///
/// # Example
///
/// ```rust,no_run
/// use raff_core::cli_report::render_cli_table;
/// use raff_core::ci_report::{Finding, Severity, Location};
///
/// let findings = vec![
///     Finding {
///         rule_id: "statement-count".to_string(),
///         rule_name: "Statement Count".to_string(),
///         severity: Severity::Error,
///         message: "Component too large".to_string(),
///         location: Some(Location::new("src/main.rs".to_string())),
///         help_uri: Some("https://example.com/docs".to_string()),
///         fingerprint: None,
///     }
/// ];
///
/// let table = render_cli_table(&findings);
/// println!("{table}");
/// ```
#[must_use]
pub fn render_cli_table(findings: &[Finding]) -> String {
    let mut table = Table::new();
    table.set_format(
        format::FormatBuilder::new()
            .separator(
                format::LinePosition::Top,
                format::LineSeparator::new('─', '┬', '┌', '┐'),
            )
            .separator(
                format::LinePosition::Title,
                format::LineSeparator::new('═', '╪', '╞', '╡'),
            )
            .separator(
                format::LinePosition::Intern,
                format::LineSeparator::new('─', '┼', '├', '┤'),
            )
            .separator(
                format::LinePosition::Bottom,
                format::LineSeparator::new('─', '┴', '└', '┘'),
            )
            .padding(1, 1)
            .build(),
    );

    // Header row
    table.set_titles(Row::new(vec![
        Cell::new("Severity").with_style(Attr::Bold),
        Cell::new("Rule").with_style(Attr::Bold),
        Cell::new("Location").with_style(Attr::Bold),
        Cell::new("Issue").with_style(Attr::Bold),
        Cell::new("Action").with_style(Attr::Bold),
    ]));

    // Sort findings by severity (Error first), then by rule_id
    let mut sorted_findings = findings.to_vec();
    sorted_findings.sort_by(|a, b| {
        // First sort by severity (Error before Warning before Note)
        match (a.severity, b.severity) {
            (Severity::Error, Severity::Error | Severity::Warning | Severity::Note) => {
                std::cmp::Ordering::Less
            }
            (Severity::Warning | Severity::Note, Severity::Error) => std::cmp::Ordering::Greater,
            (Severity::Warning, Severity::Warning | Severity::Note) => std::cmp::Ordering::Less,
            (Severity::Note, Severity::Warning) => std::cmp::Ordering::Greater,
            (Severity::Note, Severity::Note) => std::cmp::Ordering::Equal,
        }
        // Then by rule_id for deterministic ordering
        .then_with(|| a.rule_id.cmp(&b.rule_id))
    });

    for finding in &sorted_findings {
        let location = finding
            .location
            .as_ref()
            .map(|loc| loc.uri.as_str())
            .unwrap_or("-");

        let action = finding.help_uri.as_deref().unwrap_or("See rule docs");

        table.add_row(Row::new(vec![
            Cell::new(severity_label(finding.severity))
                .with_style(severity_color(finding.severity)),
            Cell::new(&finding.rule_id),
            Cell::new(location),
            Cell::new(&truncate_message(&finding.message)),
            Cell::new(action),
        ]));
    }

    // Add summary row
    let error_count = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warning_count = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    let note_count = findings
        .iter()
        .filter(|f| f.severity == Severity::Note)
        .count();
    let total = findings.len();

    if total > 0 {
        table.add_row(Row::new(vec![
            Cell::new(""),
            Cell::new(""),
            Cell::new(""),
            Cell::new(""),
            Cell::new(""),
        ]));
        table.add_row(Row::new(vec![
            Cell::new(&format!(
                "Summary: {} issue{} ({} error{}, {} warning{}, {} note{})",
                total,
                if total == 1 { "" } else { "s" },
                error_count,
                if error_count == 1 { "" } else { "s" },
                warning_count,
                if warning_count == 1 { "" } else { "s" },
                note_count,
                if note_count == 1 { "" } else { "s" },
            ))
            .with_style(Attr::Bold),
            Cell::new(""),
            Cell::new(""),
            Cell::new(""),
            Cell::new(""),
        ]));
    }

    table.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_report::Location;

    #[test]
    fn test_truncate_message_short() {
        let message = "Short message";
        let truncated = truncate_message(message);
        assert_eq!(
            truncated, "Short message",
            "Short messages should not be truncated"
        );
    }

    #[test]
    fn test_truncate_message_at_limit() {
        let message = "a".repeat(MAX_MESSAGE_WIDTH);
        let truncated = truncate_message(&message);
        assert_eq!(
            truncated.len(),
            MAX_MESSAGE_WIDTH,
            "Message at limit should not be truncated"
        );
        assert!(
            !truncated.contains("..."),
            "Message at limit should not have ellipsis"
        );
    }

    #[test]
    fn test_truncate_message_over_limit() {
        let message = "a".repeat(MAX_MESSAGE_WIDTH + 10);
        let truncated = truncate_message(&message);
        assert!(
            truncated.len() <= MAX_MESSAGE_WIDTH,
            "Truncated message should be at most MAX_MESSAGE_WIDTH"
        );
        assert!(
            truncated.ends_with("..."),
            "Truncated message should end with ellipsis"
        );
    }

    #[test]
    fn test_severity_label() {
        assert_eq!(severity_label(Severity::Error), "ERR");
        assert_eq!(severity_label(Severity::Warning), "WARN");
        assert_eq!(severity_label(Severity::Note), "NOTE");
    }

    #[test]
    fn test_render_cli_table_empty() {
        let output = render_cli_table(&[]);
        assert!(output.contains("Severity"), "Table should have header");
        assert!(output.contains("Rule"), "Table should have header");
        assert!(output.contains("Location"), "Table should have header");
        assert!(output.contains("Issue"), "Table should have header");
        assert!(output.contains("Action"), "Table should have header");
    }

    #[test]
    fn test_render_cli_table_single_finding() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Error,
            message: "Test error message".to_string(),
            location: Some(Location::new("src/test.rs".to_string())),
            help_uri: Some("https://example.com/docs".to_string()),
            fingerprint: None,
        }];

        let output = render_cli_table(&findings);
        assert!(output.contains("ERR"), "Output should contain error label");
        assert!(
            output.contains("test-rule"),
            "Output should contain rule ID"
        );
        assert!(
            output.contains("src/test.rs"),
            "Output should contain location"
        );
        assert!(
            output.contains("Test error"),
            "Output should contain message"
        );
        assert!(
            output.contains("Summary: 1 issue"),
            "Output should contain summary"
        );
    }

    #[test]
    fn test_render_cli_table_all_severities() {
        let findings = vec![
            Finding {
                rule_id: "error-rule".to_string(),
                rule_name: "Error Rule".to_string(),
                severity: Severity::Error,
                message: "Error message".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "warning-rule".to_string(),
                rule_name: "Warning Rule".to_string(),
                severity: Severity::Warning,
                message: "Warning message".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "note-rule".to_string(),
                rule_name: "Note Rule".to_string(),
                severity: Severity::Note,
                message: "Note message".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
        ];

        let output = render_cli_table(&findings);
        assert!(output.contains("ERR"), "Output should contain ERR label");
        assert!(output.contains("WARN"), "Output should contain WARN label");
        assert!(output.contains("NOTE"), "Output should contain NOTE label");
        assert!(
            output.contains("Summary: 3 issues"),
            "Output should contain correct summary"
        );
        assert!(
            output.contains("1 error"),
            "Output should contain error count"
        );
        assert!(
            output.contains("1 warning"),
            "Output should contain warning count"
        );
        assert!(
            output.contains("1 note"),
            "Output should contain note count"
        );
    }

    #[test]
    fn test_render_cli_table_long_message_truncated() {
        let long_message = "This is a very long message that should be truncated because it exceeds the maximum width of the message column in the CLI table output format.";
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Warning,
            message: long_message.to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        }];

        let output = render_cli_table(&findings);
        assert!(
            output.contains("..."),
            "Long message should be truncated with ellipsis"
        );
    }

    #[test]
    fn test_render_cli_table_sorting() {
        let findings = vec![
            Finding {
                rule_id: "warning-rule".to_string(),
                rule_name: "Warning Rule".to_string(),
                severity: Severity::Warning,
                message: "Warning".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "error-rule".to_string(),
                rule_name: "Error Rule".to_string(),
                severity: Severity::Error,
                message: "Error".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "note-rule".to_string(),
                rule_name: "Note Rule".to_string(),
                severity: Severity::Note,
                message: "Note".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
        ];

        let output = render_cli_table(&findings);
        // Find positions of each severity label
        let err_pos = output.find("ERR");
        let warn_pos = output.find("WARN");
        let note_pos = output.find("NOTE");

        // Errors should come before warnings, which come before notes
        assert!(
            err_pos < warn_pos && warn_pos < note_pos,
            "Findings should be sorted by severity: Error, Warning, Note"
        );
    }

    #[test]
    fn test_render_cli_table_with_help_uri() {
        let findings = vec![Finding {
            rule_id: "statement-count".to_string(),
            rule_name: "Statement Count".to_string(),
            severity: Severity::Error,
            message: "Component too large".to_string(),
            location: Some(Location::new("src/main.rs".to_string())),
            help_uri: Some("https://github.com/liamwh/raff/docs/statement-count".to_string()),
            fingerprint: None,
        }];

        let output = render_cli_table(&findings);
        assert!(
            output.contains("https://github.com/liamwh/raff/docs/statement-count"),
            "Output should contain help URI in Action column"
        );
    }

    #[test]
    fn test_render_cli_table_without_help_uri() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Warning,
            message: "Test warning".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        }];

        let output = render_cli_table(&findings);
        assert!(
            output.contains("See rule docs"),
            "Output should show default action text when no help URI"
        );
    }

    // Tests for render_summary_line

    #[test]
    fn test_render_summary_line_empty() {
        let findings = vec![];
        let summary = render_summary_line(&findings);
        assert_eq!(summary, "0 findings (0 errors, 0 warnings, 0 notes)");
    }

    #[test]
    fn test_render_summary_line_single_error() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Error,
            message: "Test error".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        }];

        let summary = render_summary_line(&findings);
        assert_eq!(summary, "1 finding (1 error, 0 warnings, 0 notes)");
    }

    #[test]
    fn test_render_summary_line_single_warning() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Warning,
            message: "Test warning".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        }];

        let summary = render_summary_line(&findings);
        assert_eq!(summary, "1 finding (0 errors, 1 warning, 0 notes)");
    }

    #[test]
    fn test_render_summary_line_single_note() {
        let findings = vec![Finding {
            rule_id: "test-rule".to_string(),
            rule_name: "Test Rule".to_string(),
            severity: Severity::Note,
            message: "Test note".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        }];

        let summary = render_summary_line(&findings);
        assert_eq!(summary, "1 finding (0 errors, 0 warnings, 1 note)");
    }

    #[test]
    fn test_render_summary_line_multiple_findings() {
        let findings = vec![
            Finding {
                rule_id: "error-rule".to_string(),
                rule_name: "Error Rule".to_string(),
                severity: Severity::Error,
                message: "Error".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "error-rule-2".to_string(),
                rule_name: "Error Rule 2".to_string(),
                severity: Severity::Error,
                message: "Error 2".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "warning-rule".to_string(),
                rule_name: "Warning Rule".to_string(),
                severity: Severity::Warning,
                message: "Warning".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "note-rule".to_string(),
                rule_name: "Note Rule".to_string(),
                severity: Severity::Note,
                message: "Note".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
        ];

        let summary = render_summary_line(&findings);
        assert_eq!(summary, "4 findings (2 errors, 1 warning, 1 note)");
    }

    #[test]
    fn test_render_summary_line_pluralization() {
        let findings = vec![
            Finding {
                rule_id: "error-1".to_string(),
                rule_name: "Error 1".to_string(),
                severity: Severity::Error,
                message: "Error 1".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "error-2".to_string(),
                rule_name: "Error 2".to_string(),
                severity: Severity::Error,
                message: "Error 2".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "warning-1".to_string(),
                rule_name: "Warning 1".to_string(),
                severity: Severity::Warning,
                message: "Warning 1".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "warning-2".to_string(),
                rule_name: "Warning 2".to_string(),
                severity: Severity::Warning,
                message: "Warning 2".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "note-1".to_string(),
                rule_name: "Note 1".to_string(),
                severity: Severity::Note,
                message: "Note 1".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
            Finding {
                rule_id: "note-2".to_string(),
                rule_name: "Note 2".to_string(),
                severity: Severity::Note,
                message: "Note 2".to_string(),
                location: None,
                help_uri: None,
                fingerprint: None,
            },
        ];

        let summary = render_summary_line(&findings);
        assert_eq!(summary, "6 findings (2 errors, 2 warnings, 2 notes)");
    }
}
