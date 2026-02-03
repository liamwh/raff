//! Golden (snapshot) tests for CLI table output.
//!
//! These tests use insta to capture the exact CLI table output
//! for various finding scenarios. This ensures that CLI output remains
//! stable and doesn't break unexpectedly when making changes.
//!
//! Run `cargo insta review` to review changes after modifying the code.

use raff_core::ci_report::{Finding, Location, Severity};
use raff_core::cli_report::render_cli_table;

#[test]
fn test_cli_empty() {
    let output = render_cli_table(&[]);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_single_error_finding() {
    let findings = vec![Finding {
        rule_id: "statement-count".to_string(),
        rule_name: "Statement Count Rule".to_string(),
        severity: Severity::Error,
        message: "Component 'src' has 5000 statements (25%), exceeding threshold of 20%"
            .to_string(),
        location: Some(Location::new("src/main.rs".to_string())),
        help_uri: Some("https://github.com/liamwh/raff/docs/statement-count".to_string()),
        fingerprint: Some("statement-count:src:20:5000".to_string()),
    }];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_warning_finding() {
    let findings = vec![Finding {
        rule_id: "volatility".to_string(),
        rule_name: "Volatility Rule".to_string(),
        severity: Severity::Warning,
        message: "Crate 'my-crate' has high volatility: raw_score=0.85 (alpha=0.01)".to_string(),
        location: None,
        help_uri: Some("https://github.com/liamwh/raff/docs/volatility".to_string()),
        fingerprint: Some("volatility:my-crate:0.01:0.85".to_string()),
    }];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_note_finding() {
    let findings = vec![Finding {
        rule_id: "rust-code-analysis".to_string(),
        rule_name: "Rust Code Analysis".to_string(),
        severity: Severity::Note,
        message: "File 'src/lib.rs' metrics: SLOC=150, Cyclomatic Avg=2.5, Halstead Volume=4500"
            .to_string(),
        location: Some(Location::new("src/lib.rs".to_string())),
        help_uri: None,
        fingerprint: Some("rca:src/lib.rs:150:4500".to_string()),
    }];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_multiple_findings_same_rule() {
    let findings = vec![
        Finding {
            rule_id: "statement-count".to_string(),
            rule_name: "Statement Count Rule".to_string(),
            severity: Severity::Error,
            message: "Component 'src' has 5000 statements (25%), exceeding threshold of 20%"
                .to_string(),
            location: Some(Location::new("src/main.rs".to_string())),
            help_uri: Some("https://github.com/liamwh/raff/docs/statement-count".to_string()),
            fingerprint: Some("statement-count:src:20:5000".to_string()),
        },
        Finding {
            rule_id: "statement-count".to_string(),
            rule_name: "Statement Count Rule".to_string(),
            severity: Severity::Error,
            message: "Component 'tests' has 3000 statements (15%), exceeding threshold of 10%"
                .to_string(),
            location: Some(Location::new("tests/integration_test.rs".to_string())),
            help_uri: Some("https://github.com/liamwh/raff/docs/statement-count".to_string()),
            fingerprint: Some("statement-count:tests:10:3000".to_string()),
        },
    ];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_multiple_findings_different_rules() {
    let findings = vec![
        Finding {
            rule_id: "statement-count".to_string(),
            rule_name: "Statement Count Rule".to_string(),
            severity: Severity::Error,
            message: "Component 'src' has 5000 statements (25%), exceeding threshold of 20%"
                .to_string(),
            location: Some(Location::new("src/main.rs".to_string())),
            help_uri: Some("https://github.com/liamwh/raff/docs/statement-count".to_string()),
            fingerprint: Some("statement-count:src:20:5000".to_string()),
        },
        Finding {
            rule_id: "volatility".to_string(),
            rule_name: "Volatility Rule".to_string(),
            severity: Severity::Warning,
            message: "Crate 'my-crate' has high volatility: raw_score=0.85 (alpha=0.01)"
                .to_string(),
            location: None,
            help_uri: Some("https://github.com/liamwh/raff/docs/volatility".to_string()),
            fingerprint: Some("volatility:my-crate:0.01:0.85".to_string()),
        },
        Finding {
            rule_id: "coupling".to_string(),
            rule_name: "Coupling Rule".to_string(),
            severity: Severity::Warning,
            message: "Crate 'api' has high instability: Ce=15, Ca=5, I=0.75".to_string(),
            location: None,
            help_uri: Some("https://github.com/liamwh/raff/docs/coupling".to_string()),
            fingerprint: Some("coupling:api:15:5".to_string()),
        },
    ];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_all_severity_levels() {
    let findings = vec![
        Finding {
            rule_id: "statement-count".to_string(),
            rule_name: "Statement Count Rule".to_string(),
            severity: Severity::Error,
            message: "Component 'src' exceeds threshold".to_string(),
            location: Some(Location::new("src/main.rs".to_string())),
            help_uri: None,
            fingerprint: Some("error-fingerprint".to_string()),
        },
        Finding {
            rule_id: "volatility".to_string(),
            rule_name: "Volatility Rule".to_string(),
            severity: Severity::Warning,
            message: "Crate 'my-crate' has high volatility".to_string(),
            location: None,
            help_uri: None,
            fingerprint: Some("warning-fingerprint".to_string()),
        },
        Finding {
            rule_id: "rust-code-analysis".to_string(),
            rule_name: "Rust Code Analysis".to_string(),
            severity: Severity::Note,
            message: "File metrics collected".to_string(),
            location: Some(Location::new("src/lib.rs".to_string())),
            help_uri: None,
            fingerprint: Some("note-fingerprint".to_string()),
        },
    ];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_finding_with_line_range() {
    let findings = vec![Finding {
        rule_id: "rust-code-analysis".to_string(),
        rule_name: "Rust Code Analysis".to_string(),
        severity: Severity::Note,
        message: "Function 'process_data' has high cyclomatic complexity: 15 (threshold: 10)"
            .to_string(),
        location: Some(Location::with_lines("src/processor.rs".to_string(), 42, 89)),
        help_uri: Some("https://github.com/liamwh/raff/docs/rust-code-analysis".to_string()),
        fingerprint: Some("rca:process_data:15".to_string()),
    }];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_long_message_truncation() {
    let long_message = "This is a very long message that should be truncated because it exceeds the maximum message column width in the CLI table output format and needs to be shortened to fit within the terminal display.";
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
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_finding_without_help_uri() {
    let findings = vec![Finding {
        rule_id: "custom-rule".to_string(),
        rule_name: "Custom Rule".to_string(),
        severity: Severity::Error,
        message: "Custom rule detected an issue".to_string(),
        location: Some(Location::new("src/custom.rs".to_string())),
        help_uri: None,
        fingerprint: None,
    }];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_finding_without_location() {
    let findings = vec![Finding {
        rule_id: "volatility".to_string(),
        rule_name: "Volatility Rule".to_string(),
        severity: Severity::Warning,
        message: "Crate 'utils' has high volatility".to_string(),
        location: None,
        help_uri: Some("https://github.com/liamwh/raff/docs/volatility".to_string()),
        fingerprint: None,
    }];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}

#[test]
fn test_cli_summary_counts() {
    let findings = vec![
        Finding {
            rule_id: "error-rule-1".to_string(),
            rule_name: "Error Rule 1".to_string(),
            severity: Severity::Error,
            message: "First error".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        },
        Finding {
            rule_id: "error-rule-2".to_string(),
            rule_name: "Error Rule 2".to_string(),
            severity: Severity::Error,
            message: "Second error".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        },
        Finding {
            rule_id: "warning-rule-1".to_string(),
            rule_name: "Warning Rule 1".to_string(),
            severity: Severity::Warning,
            message: "First warning".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        },
        Finding {
            rule_id: "warning-rule-2".to_string(),
            rule_name: "Warning Rule 2".to_string(),
            severity: Severity::Warning,
            message: "Second warning".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        },
        Finding {
            rule_id: "warning-rule-3".to_string(),
            rule_name: "Warning Rule 3".to_string(),
            severity: Severity::Warning,
            message: "Third warning".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        },
        Finding {
            rule_id: "note-rule".to_string(),
            rule_name: "Note Rule".to_string(),
            severity: Severity::Note,
            message: "A note".to_string(),
            location: None,
            help_uri: None,
            fingerprint: None,
        },
    ];

    let output = render_cli_table(&findings);
    insta::assert_snapshot!(output);
}
