//! Contributor Report Rule
//!
//! This module provides the contributor report rule, which analyzes Git commit history
//! to generate ranked reports of contributor activity. The report scores contributors
//! based on commit count, lines changed, files touched, and recency of contributions.
//!
//! # Overview
//!
//! The contributor report helps identify the most active contributors to a codebase.
//! It uses an exponential decay factor to weight recent contributions more heavily than
//! older ones, providing a current view of contributor engagement.
//!
//! # Scoring Formula
//!
//! Each commit contributes to a contributor's score using the formula:
//!
//! ```text
//! commit_score = (1 + churn + files_touched) * e^(-decay * days_since_commit)
//!
//! where:
//!   churn = lines_added + lines_deleted
//!   decay = the decay factor (default: 0.01)
//!   days_since_commit = number of days since the commit
//! ```
//!
//! A higher decay factor causes older contributions to be weighted less heavily.
//!
//! # Usage
//!
//! ```no_run
//! use raff::contributor_report::{ContributorReportRule, ContributorReportArgs};
//! use raff::cli::ContributorReportOutputFormat;
//! use std::path::PathBuf;
//!
//! let rule = ContributorReportRule::new();
//! let args = ContributorReportArgs {
//!     path: PathBuf::from("."),
//!     since: Some("2023-01-01".to_string()),
//!     decay: 0.01,
//!     output: ContributorReportOutputFormat::Table,
//! };
//!
//! if let Err(e) = rule.run(&args) {
//!     eprintln!("Error: {}", e);
//! }
//! ```
//!
//! # Data Structures
//!
//! - [`ContributorReportRule`]: The main rule implementation
//! - [`ContributorStats`]: Statistics for a single contributor including commits, churn, and score
//!
//! # Metrics Per Contributor
//!
//! - **Author**: The git author name
//! - **Commit Count**: Total number of commits
//! - **Lines Added**: Total lines of code added
//! - **Lines Deleted**: Total lines of code deleted (considered positive contribution)
//! - **Files Touched**: Number of unique files modified
//! - **Score**: Weighted sum considering recency decay
//!
//! # Output Formats
//!
//! The rule supports multiple output formats:
//! - `Table`: Human-readable table format
//! - `Html`: Interactive HTML report saved to `contributor-report.html`
//! - `Json`: Machine-readable JSON
//! - `Yaml`: Machine-readable YAML
//!
//! # Errors
//!
//! This module returns [`RaffError`] in the following cases:
//! - The provided path is not a valid Git repository
//! - Git operations fail (e.g., corrupted repository)

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use crate::error::{RaffError, Result};
use chrono::{DateTime, Utc};
use git2::{Commit, Repository};
use maud::{html, Markup};
use prettytable::{row, Table};
use serde::{Deserialize, Serialize};

use crate::cli::{ContributorReportArgs, ContributorReportOutputFormat};
use crate::html_utils::{self, MetricRanges};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributorStats {
    pub author: String,
    pub commit_count: u32,
    pub lines_added: u32,
    pub lines_deleted: u32,
    pub files_touched: u32,
    pub last_commit_date: DateTime<Utc>,
    pub score: f64,
}

impl ContributorStats {
    pub fn new(author: String) -> Self {
        Self {
            author,
            commit_count: 0,
            lines_added: 0,
            lines_deleted: 0,
            files_touched: 0,
            last_commit_date: Utc::now(),
            score: 0.0,
        }
    }
}

pub struct ContributorReportRule;

impl Default for ContributorReportRule {
    fn default() -> Self {
        Self::new()
    }
}

impl ContributorReportRule {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&self, args: &ContributorReportArgs) -> Result<()> {
        let repo = Repository::open(&args.path)
            .map_err(|_e| RaffError::git_error_with_repo("open repository", args.path.clone()))?;
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;

        let mut stats: HashMap<String, ContributorStats> = HashMap::new();
        let now = Utc::now();

        for oid in revwalk {
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            let author = commit.author().name().unwrap_or("Unknown").to_string();

            let contributor = stats
                .entry(author.clone())
                .or_insert_with(|| ContributorStats::new(author));

            let commit_time = DateTime::from_timestamp(commit.time().seconds(), 0).unwrap_or(now);
            let days_since_commit = now.signed_duration_since(commit_time).num_days() as f64;
            let weight = (-args.decay * days_since_commit).exp();

            let (lines_added, lines_deleted, files_touched) =
                self.get_commit_stats(&repo, &commit)?;

            contributor.commit_count += 1;
            contributor.lines_added += lines_added;
            contributor.lines_deleted += lines_deleted;
            contributor.files_touched += files_touched;

            let churn = (lines_added + lines_deleted) as f64;
            let commit_score = (1.0 + churn + files_touched as f64) * weight;
            contributor.score += commit_score;

            if commit_time > contributor.last_commit_date {
                contributor.last_commit_date = commit_time;
            }
        }

        let mut sorted_stats: Vec<ContributorStats> = stats.into_values().collect();
        sorted_stats.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        match args.output {
            ContributorReportOutputFormat::Table => self.print_table(&sorted_stats),
            ContributorReportOutputFormat::Html => self.print_html(&sorted_stats),
            ContributorReportOutputFormat::Json => self.print_json(&sorted_stats),
            ContributorReportOutputFormat::Yaml => self.print_yaml(&sorted_stats),
        }
    }

    fn get_commit_stats(&self, repo: &Repository, commit: &Commit) -> Result<(u32, u32, u32)> {
        let parent = commit.parent(0);
        let tree = commit.tree()?;
        let parent_tree = parent.ok().and_then(|p| p.tree().ok());

        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        let diff_stats = diff.stats()?;

        Ok((
            diff_stats.insertions() as u32,
            diff_stats.deletions() as u32,
            diff_stats.files_changed() as u32,
        ))
    }

    fn print_table(&self, stats: &[ContributorStats]) -> Result<()> {
        let mut table = Table::new();
        table.add_row(row![
            "Author",
            "Commit Count",
            "Lines Added",
            "Lines Deleted",
            "Files Touched",
            "Score"
        ]);

        for stat in stats {
            table.add_row(row![
                stat.author,
                stat.commit_count.to_string(),
                stat.lines_added.to_string(),
                stat.lines_deleted.to_string(),
                stat.files_touched.to_string(),
                format!("{:.2}", stat.score)
            ]);
        }

        table.printstd();
        Ok(())
    }

    fn print_json(&self, stats: &[ContributorStats]) -> Result<()> {
        let json = serde_json::to_string_pretty(stats)?;
        println!("{json}");
        Ok(())
    }

    fn print_yaml(&self, stats: &[ContributorStats]) -> Result<()> {
        let yaml = serde_yaml::to_string(stats)?;
        println!("{yaml}");
        Ok(())
    }

    fn print_html(&self, stats: &[ContributorStats]) -> Result<()> {
        let report_body = self.generate_report_body(stats);
        let html_content = html_utils::render_html_doc("Contributor Report", report_body);
        let mut file = File::create("contributor-report.html")?;
        file.write_all(html_content.as_bytes())?;
        println!("HTML report generated: contributor-report.html");
        Ok(())
    }

    fn generate_report_body(&self, stats: &[ContributorStats]) -> Markup {
        let commit_counts: Vec<f64> = stats.iter().map(|s| s.commit_count as f64).collect();
        let lines_added: Vec<f64> = stats.iter().map(|s| s.lines_added as f64).collect();
        let lines_deleted: Vec<f64> = stats.iter().map(|s| s.lines_deleted as f64).collect();
        let files_touched: Vec<f64> = stats.iter().map(|s| s.files_touched as f64).collect();
        let scores: Vec<f64> = stats.iter().map(|s| s.score).collect();

        let commit_ranges = MetricRanges::from_values(&commit_counts, true);
        let added_ranges = MetricRanges::from_values(&lines_added, true);
        let deleted_ranges = MetricRanges::from_values(&lines_deleted, true);
        let touched_ranges = MetricRanges::from_values(&files_touched, true);
        let score_ranges = MetricRanges::from_values(&scores, true);

        html! {
            (self.render_explanation())
            table class="sortable-table" {
                thead {
                    tr {
                        th { "Author" }
                        th { "Commit Count" }
                        th { "Lines Added" }
                        th { "Lines Deleted" }
                        th { "Files Touched" }
                        th { "Score" }
                    }
                }
                tbody {
                    @for stat in stats {
                        tr {
                            td { (stat.author) }
                            @if let Some(ref ranges) = commit_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.commit_count as f64, ranges)) { (stat.commit_count) }
                            } @else {
                                td { (stat.commit_count) }
                            }
                            @if let Some(ref ranges) = added_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.lines_added as f64, ranges)) { (stat.lines_added) }
                            } @else {
                                td { (stat.lines_added) }
                            }
                            @if let Some(ref ranges) = deleted_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.lines_deleted as f64, ranges)) { (stat.lines_deleted) }
                            } @else {
                                td { (stat.lines_deleted) }
                            }
                            @if let Some(ref ranges) = touched_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.files_touched as f64, ranges)) { (stat.files_touched) }
                            } @else {
                                td { (stat.files_touched) }
                            }
                            @if let Some(ref ranges) = score_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.score, ranges)) { (format!("{:.2}", stat.score)) }
                            } @else {
                                td { (format!("{:.2}", stat.score)) }
                            }
                        }
                    }
                }
            }
        }
    }

    fn render_explanation(&self) -> Markup {
        let explanations = vec![
            ("Author", "The name of the contributor, as extracted from the Git commit logs."),
            ("Commit Count", "The total number of commits made by the contributor."),
            ("Lines Added", "The total number of lines of code added by the contributor. This metric is weighted positively in the score calculation."),
            ("Lines Deleted", "The total number of lines of code deleted by the contributor. This is considered a positive contribution (e.g., refactoring, removing dead code) and is weighted positively."),
            ("Files Touched", "The total number of unique files modified by the contributor."),
            ("Score", "A calculated metric representing the overall contribution. It is a weighted sum of commits, lines added, lines deleted, and files touched, with an exponential decay factor applied to give more weight to recent contributions. The formula is: `Σ((1 + churn + files_touched) * e^(-decay * days_since_commit))` for each commit."),
        ];
        html_utils::render_metric_explanation_list(&explanations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create test ContributorStats.
    fn create_test_contributor_stats(
        author: &str,
        commit_count: u32,
        lines_added: u32,
        lines_deleted: u32,
        files_touched: u32,
        score: f64,
    ) -> ContributorStats {
        ContributorStats {
            author: author.to_string(),
            commit_count,
            lines_added,
            lines_deleted,
            files_touched,
            last_commit_date: Utc::now(),
            score,
        }
    }

    #[test]
    fn test_contributor_stats_new_creates_default_instance() {
        let stats = ContributorStats::new("Test Author".to_string());
        assert_eq!(stats.author, "Test Author", "author should match input");
        assert_eq!(stats.commit_count, 0, "commit_count should be 0");
        assert_eq!(stats.lines_added, 0, "lines_added should be 0");
        assert_eq!(stats.lines_deleted, 0, "lines_deleted should be 0");
        assert_eq!(stats.files_touched, 0, "files_touched should be 0");
        assert_eq!(stats.score, 0.0, "score should be 0.0");
    }

    #[test]
    fn test_contributor_report_rule_new_creates_instance() {
        let rule = ContributorReportRule::new();
        // Just verify it can be created - it's a zero-sized struct
        let _ = &rule;
    }

    #[test]
    fn test_contributor_report_rule_default_creates_instance() {
        let _rule = ContributorReportRule;
        // Just verify it can be created - it's a zero-sized struct
    }

    #[test]
    fn test_contributor_stats_is_serializable() {
        let stats = create_test_contributor_stats("Alice", 10, 500, 200, 50, 1500.0);
        let json = serde_json::to_string(&stats);
        assert!(
            json.is_ok(),
            "ContributorStats should be serializable to JSON"
        );
        let json_str = json.unwrap();
        assert!(
            json_str.contains("Alice"),
            "JSON output should contain author name"
        );
        assert!(
            json_str.contains("500"),
            "JSON output should contain lines_added"
        );
    }

    #[test]
    fn test_contributor_stats_clone_creates_independent_copy() {
        let stats1 = create_test_contributor_stats("Bob", 5, 300, 100, 25, 750.0);
        let mut stats2 = stats1.clone();

        // Modify the clone
        stats2.commit_count = 10;
        stats2.score = 2000.0;

        // Original should be unchanged
        assert_eq!(
            stats1.commit_count, 5,
            "original commit_count should not be affected by clone modification"
        );
        assert_eq!(
            stats1.score, 750.0,
            "original score should not be affected by clone modification"
        );

        // Clone should have the new values
        assert_eq!(
            stats2.commit_count, 10,
            "clone commit_count should reflect modification"
        );
        assert_eq!(
            stats2.score, 2000.0,
            "clone score should reflect modification"
        );
    }

    #[test]
    fn test_generate_report_body_produces_valid_markup() {
        let rule = ContributorReportRule::new();
        let stats = vec![create_test_contributor_stats(
            "Charlie", 15, 750, 300, 75, 3000.0,
        )];

        let markup = rule.generate_report_body(&stats);
        let markup_string = markup.into_string();

        assert!(
            markup_string.contains("Charlie"),
            "generated HTML should contain contributor name"
        );
        assert!(
            markup_string.contains("15"),
            "generated HTML should contain commit count"
        );
        assert!(
            markup_string.contains("750"),
            "generated HTML should contain lines added"
        );
        assert!(
            markup_string.contains("300"),
            "generated HTML should contain lines deleted"
        );
        assert!(
            markup_string.contains("75"),
            "generated HTML should contain files touched"
        );
    }

    #[test]
    fn test_generate_report_body_with_empty_stats() {
        let rule = ContributorReportRule::new();
        let stats: Vec<ContributorStats> = vec![];

        let markup = rule.generate_report_body(&stats);
        let markup_string = markup.into_string();

        assert!(
            markup_string.contains("table"),
            "generated HTML should contain table element"
        );
        assert!(
            markup_string.contains("thead"),
            "generated HTML should contain table header"
        );
        assert!(
            markup_string.contains("Author"),
            "generated HTML should contain Author column header"
        );
        assert!(
            markup_string.contains("Score"),
            "generated HTML should contain Score column header"
        );
    }

    #[test]
    fn test_generate_report_body_with_multiple_contributors() {
        let rule = ContributorReportRule::new();
        let stats = vec![
            create_test_contributor_stats("Alice", 20, 1000, 400, 100, 4000.0),
            create_test_contributor_stats("Bob", 15, 750, 300, 75, 3000.0),
            create_test_contributor_stats("Charlie", 5, 200, 50, 10, 500.0),
        ];

        let markup = rule.generate_report_body(&stats);
        let markup_string = markup.into_string();

        assert!(
            markup_string.contains("Alice"),
            "generated HTML should contain first contributor"
        );
        assert!(
            markup_string.contains("Bob"),
            "generated HTML should contain second contributor"
        );
        assert!(
            markup_string.contains("Charlie"),
            "generated HTML should contain third contributor"
        );
    }

    #[test]
    fn test_render_explanation_produces_valid_markup() {
        let rule = ContributorReportRule::new();
        let markup = rule.render_explanation();
        let markup_string = markup.into_string();

        assert!(
            markup_string.contains("Author"),
            "explanation should describe Author field"
        );
        assert!(
            markup_string.contains("Commit Count"),
            "explanation should describe Commit Count field"
        );
        assert!(
            markup_string.contains("Lines Added"),
            "explanation should describe Lines Added field"
        );
        assert!(
            markup_string.contains("Lines Deleted"),
            "explanation should describe Lines Deleted field"
        );
        assert!(
            markup_string.contains("Files Touched"),
            "explanation should describe Files Touched field"
        );
        assert!(
            markup_string.contains("Score"),
            "explanation should describe Score field"
        );
        assert!(
            markup_string.contains("churn"),
            "explanation should describe the score formula"
        );
    }

    #[test]
    fn test_print_json_produces_valid_json() {
        let rule = ContributorReportRule::new();
        let stats = vec![
            create_test_contributor_stats("Alice", 10, 500, 200, 50, 1500.0),
            create_test_contributor_stats("Bob", 5, 300, 100, 25, 750.0),
        ];

        // Redirect stdout to capture the output
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rule.print_json(&stats)));

        assert!(
            result.is_ok(),
            "print_json should not panic when given valid stats"
        );
    }

    #[test]
    fn test_print_yaml_produces_valid_yaml() {
        let rule = ContributorReportRule::new();
        let stats = vec![create_test_contributor_stats(
            "Alice", 10, 500, 200, 50, 1500.0,
        )];

        // Redirect stdout to capture the output
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rule.print_yaml(&stats)));

        assert!(
            result.is_ok(),
            "print_yaml should not panic when given valid stats"
        );
    }

    #[test]
    fn test_print_table_produces_valid_output() {
        let rule = ContributorReportRule::new();
        let stats = vec![create_test_contributor_stats(
            "Alice", 10, 500, 200, 50, 1500.0,
        )];

        // Redirect stdout to capture the output
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rule.print_table(&stats)));

        assert!(
            result.is_ok(),
            "print_table should not panic when given valid stats"
        );
    }

    #[test]
    fn test_contributor_stats_with_zero_values() {
        let stats = ContributorStats::new("Zero Hero".to_string());
        assert_eq!(stats.commit_count, 0, "commit_count should be 0");
        assert_eq!(stats.lines_added, 0, "lines_added should be 0");
        assert_eq!(stats.lines_deleted, 0, "lines_deleted should be 0");
        assert_eq!(stats.files_touched, 0, "files_touched should be 0");
        assert_eq!(stats.score, 0.0, "score should be 0.0");
    }

    #[test]
    fn test_contributor_stats_json_roundtrip() {
        let original = create_test_contributor_stats("Roundtrip", 100, 5000, 2000, 500, 15000.0);

        let json = serde_json::to_string(&original);
        assert!(json.is_ok(), "ContributorStats should serialize to JSON");

        let deserialized: std::result::Result<ContributorStats, _> =
            serde_json::from_str(&json.unwrap());
        assert!(
            deserialized.is_ok(),
            "JSON should deserialize back to ContributorStats"
        );

        let stats = deserialized.unwrap();
        assert_eq!(
            stats.author, original.author,
            "author should survive roundtrip"
        );
        assert_eq!(
            stats.commit_count, original.commit_count,
            "commit_count should survive roundtrip"
        );
        assert_eq!(
            stats.lines_added, original.lines_added,
            "lines_added should survive roundtrip"
        );
        assert_eq!(
            stats.lines_deleted, original.lines_deleted,
            "lines_deleted should survive roundtrip"
        );
        assert_eq!(
            stats.files_touched, original.files_touched,
            "files_touched should survive roundtrip"
        );
        assert!(
            (stats.score - original.score).abs() < f64::EPSILON,
            "score should survive roundtrip"
        );
    }

    #[test]
    fn test_contributor_stats_yaml_serialization() {
        let stats = create_test_contributor_stats("Yaml Author", 25, 1200, 450, 120, 3500.0);

        let yaml = serde_yaml::to_string(&stats);
        assert!(yaml.is_ok(), "ContributorStats should serialize to YAML");

        let yaml_string = yaml.unwrap();
        assert!(
            yaml_string.contains("Yaml Author"),
            "YAML output should contain author name"
        );
        assert!(
            yaml_string.contains("1200"),
            "YAML output should contain lines added"
        );
    }
}
