//! Golden (snapshot) tests for JUnit XML output.
//!
//! These tests use insta to capture the exact JUnit XML output
//! for various finding scenarios. This ensures that CI output remains
//! stable and doesn't break unexpectedly when making changes.
//!
//! Run `cargo insta review` to review changes after modifying the code.

use raff_core::ci_report::{Finding, Location, Severity, to_junit};

#[test]
fn test_junit_empty() {
    let xml = to_junit(&[], "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_single_error_finding() {
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

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_warning_finding() {
    let findings = vec![Finding {
        rule_id: "volatility".to_string(),
        rule_name: "Volatility Rule".to_string(),
        severity: Severity::Warning,
        message: "Crate 'my-crate' has high volatility: raw_score=0.85 (alpha=0.01)".to_string(),
        location: None,
        help_uri: Some("https://github.com/liamwh/raff/docs/volatility".to_string()),
        fingerprint: Some("volatility:my-crate:0.01:0.85".to_string()),
    }];

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_note_finding() {
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

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_multiple_findings_same_rule() {
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

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_multiple_findings_different_rules() {
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

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_finding_with_line_range() {
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

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_finding_with_special_characters() {
    let findings = vec![Finding {
        rule_id: "statement-count".to_string(),
        rule_name: "Statement Count Rule".to_string(),
        severity: Severity::Error,
        message:
            "Component 'src/utils/helpers' has 1000 statements (10%), exceeding threshold of 5%"
                .to_string(),
        location: Some(Location::new("src/utils/helpers.rs".to_string())),
        help_uri: Some("https://github.com/liamwh/raff/docs/statement-count".to_string()),
        fingerprint: Some("statement-count:src/utils/helpers:5:1000".to_string()),
    }];

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_finding_with_xml_special_characters() {
    let findings = vec![Finding {
        rule_id: "test-rule".to_string(),
        rule_name: "Test Rule".to_string(),
        severity: Severity::Error,
        message: "Error: value < threshold & condition > expected".to_string(),
        location: Some(Location::new("src/test.rs".to_string())),
        help_uri: None,
        fingerprint: None,
    }];

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_finding_without_fingerprint() {
    let findings = vec![Finding {
        rule_id: "contributor-report".to_string(),
        rule_name: "Contributor Report".to_string(),
        severity: Severity::Note,
        message: "Contributor 'alice@example.com' statistics: 50 commits, 5000 lines added, 2000 lines deleted".to_string(),
        location: None,
        help_uri: None,
        fingerprint: None,
    }];

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_all_severity_levels() {
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

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_custom_suite_name() {
    let findings = vec![Finding {
        rule_id: "statement-count".to_string(),
        rule_name: "Statement Count Rule".to_string(),
        severity: Severity::Error,
        message: "Test finding".to_string(),
        location: None,
        help_uri: None,
        fingerprint: None,
    }];

    let xml = to_junit(&findings, "my-custom-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}

#[test]
fn test_junit_long_message_truncation() {
    let long_message = "This is a very long message that should be truncated because it exceeds the maximum testcase name length that JUnit parsers can handle reliably. ".repeat(10);

    let findings = vec![Finding {
        rule_id: "test-rule".to_string(),
        rule_name: "Test Rule".to_string(),
        severity: Severity::Error,
        message: long_message,
        location: None,
        help_uri: None,
        fingerprint: None,
    }];

    let xml = to_junit(&findings, "raff-test-suite").expect("JUnit generation should succeed");
    insta::assert_snapshot!(xml);
}
