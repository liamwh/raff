//! Statement Count Rule
//!
//! This module provides the statement count analysis rule, which counts the number of
//! statements in each Rust component (top-level directory under the source path) and
//! checks whether any component exceeds a specified percentage threshold of the total
//! statements.
//!
//! # Overview
//!
//! The statement count rule helps identify components that have grown too large relative
//! to the overall codebase. It parses all `.rs` files using `syn`, counts AST statements
//! using the [`StmtCounter`] visitor, and aggregates results by component.
//!
//! # Usage
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use raff_core::statement_count_rule::StatementCountRule;
//! use raff_core::{StatementCountArgs, StatementCountOutputFormat};
//! use std::path::PathBuf;
//!
//! let rule = StatementCountRule::new();
//! let args = StatementCountArgs {
//!     path: PathBuf::from("."),
//!     threshold: 10,
//!     output: StatementCountOutputFormat::Table,
//! };
//!
//! if let Err(e) = rule.run(&args) {
//!     eprintln!("Error: {}", e);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Data Structures
//!
//! - [`StatementCountRule`]: The main rule implementation
//! - [`StatementCountData`]: Contains the analysis results including component stats and thresholds
//!
//! # Errors
//!
//! This module returns [`RaffError`] in the following cases:
//! - The provided path does not exist or is not a directory
//! - No `.rs` files are found in the analysis path
//! - No Rust statements are found in any files
//! - An error occurs during AST parsing

use bincode;
use maud::html;
use maud::Markup;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};
use syn::visit::Visit;
use syn::File as SynFile;

use crate::cache::{CacheEntry, CacheKey, CacheManager};
use crate::cli::{StatementCountArgs, StatementCountOutputFormat}; // Import the specific args struct
use crate::counter::StmtCounter; // Assuming counter.rs is at crate::counter
use crate::error::{RaffError, Result};
use crate::file_utils::{collect_all_rs, relative_namespace, top_level_component}; // Assuming file_utils.rs is at crate::file_utils
use crate::html_utils; // Now using Maud-based html_utils
use crate::reporting::print_report; // Assuming reporting.rs is at crate::reporting // Import the new HTML utilities
use crate::rule::Rule;

/// Rule to count statements in Rust components and check against a threshold.
#[derive(Debug, Default)]
pub struct StatementCountRule;

#[derive(Debug, Serialize, Deserialize)]
pub struct StatementCountData {
    pub component_stats: HashMap<String, (usize, usize)>,
    pub grand_total: usize,
    pub threshold: usize,
    pub analysis_path: PathBuf,
}

impl Rule for StatementCountRule {
    type Config = StatementCountArgs;
    type Data = StatementCountData;

    fn name() -> &'static str {
        "statement_count"
    }

    fn description() -> &'static str {
        "Counts statements in Rust components and checks against a threshold"
    }

    fn run(&self, config: &Self::Config) -> Result<()> {
        self.run_impl(config)
    }

    fn analyze(&self, config: &Self::Config) -> Result<Self::Data> {
        self.analyze_impl(config)
    }
}

impl StatementCountRule {
    pub fn new() -> Self {
        StatementCountRule
    }

    pub fn run(&self, args: &StatementCountArgs) -> Result<()> {
        self.run_impl(args)
    }

    fn run_impl(&self, args: &StatementCountArgs) -> Result<()> {
        let data = self.analyze(args)?;

        match args.output {
            StatementCountOutputFormat::Table => {
                println!(
                    "\nStatement Count Report (analyzing path: {}):",
                    data.analysis_path.display()
                );
                let any_over_threshold =
                    print_report(&data.component_stats, data.grand_total, data.threshold);
                if any_over_threshold {
                    return Err(RaffError::analysis_error(
                        "statement_count",
                        format!(
                            "At least one component exceeds {}% of total statements.",
                            data.threshold
                        ),
                    ));
                }
                println!(
                    "\nAll components are within {}% threshold. (Total statements = {})",
                    data.threshold, data.grand_total
                );
            }
            StatementCountOutputFormat::Html => {
                let html_body = self.render_statement_count_html_body(&data)?;
                let full_html = html_utils::render_html_doc(
                    &format!("Statement Count Report: {}", data.analysis_path.display()),
                    html_body,
                );
                println!("{full_html}");
                let any_over_threshold =
                    data.component_stats
                        .values()
                        .any(|&(_file_count, st_count)| {
                            if data.grand_total == 0 {
                                return false;
                            }
                            let percentage = (st_count * 100) / data.grand_total;
                            percentage > data.threshold
                        });
                if any_over_threshold {
                    return Err(RaffError::analysis_error(
                        "statement_count",
                        format!(
                            "At least one component exceeds {}% of total statements (see HTML report for details).",
                            data.threshold
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn analyze(&self, args: &StatementCountArgs) -> Result<StatementCountData> {
        self.analyze_impl(args)
    }

    fn analyze_impl(&self, args: &StatementCountArgs) -> Result<StatementCountData> {
        let threshold = args.threshold;
        let analysis_path = &args.path;

        // Create cache key from analysis path and threshold
        let cache_manager = CacheManager::new()?;
        let cache_key = CacheKey::new(
            format!("statement_count:{}", analysis_path.display()),
            None, // No git state for statement count
            vec![("threshold".to_string(), threshold.to_string())],
        );

        // Try to get cached result
        if let Some(cached_entry) = cache_manager.get(&cache_key)? {
            tracing::info!("Using cached statement count analysis result");
            let cached_data: StatementCountData = bincode::deserialize(&cached_entry.data)
                .map_err(|e| {
                    RaffError::parse_error(format!(
                        "Failed to deserialize cached statement count data: {}",
                        e
                    ))
                })?;
            return Ok(cached_data);
        }

        if !analysis_path.exists() {
            return Err(RaffError::invalid_input_with_arg(
                "Path not found",
                analysis_path.display().to_string(),
            ));
        }
        if !analysis_path.is_dir() {
            return Err(RaffError::invalid_input_with_arg(
                "Provided path is not a directory",
                analysis_path.display().to_string(),
            ));
        }

        let mut all_rs_files: Vec<PathBuf> = Vec::new();
        collect_all_rs(analysis_path, &mut all_rs_files)?;

        if all_rs_files.is_empty() {
            return Err(RaffError::analysis_error(
                "statement_count",
                format!("No `.rs` files found under {}", analysis_path.display()),
            ));
        }

        let mut file_to_stmt: HashMap<String, usize> = HashMap::new();
        for path_buf in &all_rs_files {
            let content = fs::read_to_string(path_buf)?;
            let ast: SynFile = syn::parse_file(&content)?;
            let mut counter = StmtCounter::new();
            counter.visit_file(&ast);
            let key = path_buf.to_string_lossy().into_owned();
            file_to_stmt.insert(key, counter.count);
        }

        if file_to_stmt.is_empty() {
            return Err(RaffError::analysis_error(
                "statement_count",
                format!(
                    "Did not find any Rust AST statements under {}",
                    analysis_path.display()
                ),
            ));
        }

        let mut component_stats: HashMap<String, (usize, usize)> = HashMap::new();
        for path_buf in &all_rs_files {
            let namespace = relative_namespace(path_buf, analysis_path);
            let top = top_level_component(&namespace);
            let path_str = path_buf.to_string_lossy();
            let stmt_count = *file_to_stmt.get(&path_str.into_owned()).unwrap_or(&0);
            let entry = component_stats.entry(top).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += stmt_count;
        }

        let grand_total: usize = component_stats.values().map(|&(_f, st)| st).sum();
        if grand_total == 0 {
            return Err(RaffError::analysis_error(
                "statement_count",
                format!(
                    "Total Rust statements = 0. Ensure .rs files contain statements or check parsing. Path: {}",
                    analysis_path.display()
                ),
            ));
        }

        let result = StatementCountData {
            component_stats,
            grand_total,
            threshold,
            analysis_path: analysis_path.to_path_buf(),
        };

        // Cache the result
        let serialized_data = bincode::serialize(&result).map_err(|e| {
            RaffError::parse_error(format!(
                "Failed to serialize statement count data for caching: {}",
                e
            ))
        })?;
        let cache_entry = CacheEntry::new(serialized_data);
        cache_manager.put(&cache_key, cache_entry)?;

        Ok(result)
    }

    pub fn render_statement_count_html_body(&self, data: &StatementCountData) -> Result<Markup> {
        let explanations_data = [
            ("Component", "Name of the top-level component (e.g., directory under src/, or crate name)."),
            ("File Count", "Number of .rs files within this component."),
            ("Statement Count", "Total number of Rust statements counted in this component."),
            ("Percentage", "This component's statement count as a percentage of the grand total. Cells are colored red if this exceeds the threshold."),
        ];
        let explanations_markup = html_utils::render_metric_explanation_list(&explanations_data);

        let mut sorted_components: Vec<_> = data.component_stats.iter().collect();
        sorted_components.sort_by_key(|&(name, _)| name.clone());

        let table_markup = html! {
            table class="sortable-table" {
                caption { (format!("Analysis Path: {}. Threshold: {}%", data.analysis_path.display(), data.threshold)) }
                thead {
                    tr {
                        th class="sortable-header" data-column-index="0" data-sort-type="string" { "Component" }
                        th class="sortable-header" data-column-index="1" data-sort-type="number" { "File Count" }
                        th class="sortable-header" data-column-index="2" data-sort-type="number" { "Statement Count" }
                        th class="sortable-header" data-column-index="3" data-sort-type="number" { "Percentage" }
                    }
                }
                tbody {
                    @for (name, (file_count, st_count)) in sorted_components {
                        @let percentage = if data.grand_total > 0 { (*st_count * 100) / data.grand_total } else { 0 };
                        @let percentage_style = html_utils::get_cell_style(percentage as f64, data.threshold as f64, data.threshold as f64, false);
                        tr {
                            td { (name) }
                            td { (file_count) }
                            td { (st_count) }
                            td style=(percentage_style) { (format!("{}%", percentage)) }
                        }
                    }
                }
            }
        };

        let summary_markup = html! {
            p {
                b { "Grand Total Statements: " (data.grand_total) }
            }
            @let any_over_threshold = data.component_stats.values().any(|&(_file_count, st_count)| {
                if data.grand_total == 0 { return false; }
                let percentage = (st_count * 100) / data.grand_total;
                percentage > data.threshold
            });
            @if any_over_threshold {
                p style="color: red;" {
                    b { "Warning: At least one component exceeds the " (data.threshold) "% threshold." }
                }
            } @else {
                p style="color: green;" {
                    "All components are within the " (data.threshold) "% threshold."
                }
            }
        };

        Ok(html! {
            (explanations_markup)
            (table_markup)
            (summary_markup)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{StatementCountArgs, StatementCountOutputFormat};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Helper function to create a test directory with Rust files
    fn create_test_directory() -> TempDir {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src directory");

        // Create a simple main.rs file
        let main_rs = r#"
fn main() {
    let x = 5;
    let y = 10;
    println!("Hello, world!");
}
"#;
        fs::write(src_dir.join("main.rs"), main_rs).expect("Failed to write main.rs");

        // Create a lib.rs file
        let lib_rs = r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
"#;
        fs::write(src_dir.join("lib.rs"), lib_rs).expect("Failed to write lib.rs");

        temp_dir
    }

    /// Helper function to create test directory with separate top-level components
    fn create_multi_component_test_directory() -> TempDir {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        // Create component_a directory at top level
        let comp_a_dir = temp_dir.path().join("component_a");
        fs::create_dir_all(&comp_a_dir).expect("Failed to create component_a directory");

        let comp_a = r#"
pub fn func_a() {
    let x = 1;
    let y = 2;
}
"#;
        fs::write(comp_a_dir.join("mod.rs"), comp_a).expect("Failed to write component_a/mod.rs");

        // Create component_b directory at top level
        let comp_b_dir = temp_dir.path().join("component_b");
        fs::create_dir_all(&comp_b_dir).expect("Failed to create component_b directory");

        let comp_b = r#"
pub fn func_b() {
    let a = 10;
    let b = 20;
    let c = 30;
    let d = 40;
}
"#;
        fs::write(comp_b_dir.join("mod.rs"), comp_b).expect("Failed to write component_b/mod.rs");

        temp_dir
    }

    /// Helper function to create test args
    fn create_test_args(path: PathBuf) -> StatementCountArgs {
        StatementCountArgs {
            path,
            threshold: 10,
            output: StatementCountOutputFormat::Table,
        }
    }

    #[test]
    fn test_statement_count_rule_new_creates_instance() {
        let rule = StatementCountRule::new();
        // Just verify the rule can be created; struct has no fields to check
        let _ = rule;
    }

    #[test]
    fn test_statement_count_rule_default_creates_instance() {
        let _rule = StatementCountRule;
    }

    #[test]
    fn test_analyze_valid_directory_with_rust_files() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let result = rule.analyze(&args);

        assert!(
            result.is_ok(),
            "analyze should succeed with valid directory containing Rust files"
        );

        let data = result.unwrap();
        assert_eq!(data.threshold, 10, "threshold should match args");
        assert_eq!(
            data.analysis_path,
            temp_dir.path(),
            "analysis_path should match input path"
        );
        assert!(
            data.grand_total > 0,
            "grand_total should be greater than 0 for Rust files with statements"
        );
        assert!(
            !data.component_stats.is_empty(),
            "component_stats should not be empty"
        );
    }

    #[test]
    fn test_analyze_fails_with_nonexistent_path() {
        let rule = StatementCountRule::new();
        let fake_path = PathBuf::from("/nonexistent/path/that/does/not/exist");
        let args = create_test_args(fake_path);

        let result = rule.analyze(&args);

        assert!(result.is_err(), "analyze should fail with nonexistent path");
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Path not found"),
            "error message should mention path not found"
        );
    }

    #[test]
    fn test_analyze_fails_with_file_instead_of_directory() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("not_a_directory.txt");
        fs::write(&file_path, "test content").expect("Failed to write test file");

        let rule = StatementCountRule::new();
        let args = create_test_args(file_path);

        let result = rule.analyze(&args);

        assert!(
            result.is_err(),
            "analyze should fail when path is a file, not a directory"
        );
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("not a directory"),
            "error message should mention not a directory"
        );
    }

    #[test]
    fn test_analyze_fails_with_directory_containing_no_rust_files() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        // Create directory but no .rs files
        let empty_dir = temp_dir.path().join("empty");
        fs::create_dir_all(&empty_dir).expect("Failed to create empty directory");

        let rule = StatementCountRule::new();
        let args = create_test_args(empty_dir);

        let result = rule.analyze(&args);

        assert!(
            result.is_err(),
            "analyze should fail when directory contains no .rs files"
        );
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("No `.rs` files found"),
            "error message should mention no .rs files found"
        );
    }

    #[test]
    fn test_analyze_counts_statements_correctly() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let result = rule.analyze(&args);

        assert!(result.is_ok(), "analyze should succeed");
        let data = result.unwrap();

        // main.rs has 3 statements (2 lets + println)
        // lib.rs has 2 statements (2 returns)
        // Total should be 5
        assert_eq!(
            data.grand_total, 5,
            "grand_total should correctly count all statements"
        );
    }

    #[test]
    fn test_analyze_aggregates_by_component() {
        let temp_dir = create_multi_component_test_directory();
        let rule = StatementCountRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let result = rule.analyze(&args);

        assert!(result.is_ok(), "analyze should succeed");
        let data = result.unwrap();

        // Should have component_a and component_b as separate components
        // Files are at: component_a/mod.rs and component_b/mod.rs
        // relative_namespace(component_a/mod.rs, temp_dir) = "component_a"
        // top_level_component("component_a") = "component_a"
        assert!(
            data.component_stats.contains_key("component_a"),
            "component_stats should contain component_a"
        );
        assert!(
            data.component_stats.contains_key("component_b"),
            "component_stats should contain component_b"
        );

        // component_a has 2 statements
        let (file_count_a, stmt_count_a) = data.component_stats.get("component_a").unwrap();
        assert_eq!(*file_count_a, 1, "component_a should have 1 file");
        assert_eq!(*stmt_count_a, 2, "component_a should have 2 statements");

        // component_b has 4 statements
        let (file_count_b, stmt_count_b) = data.component_stats.get("component_b").unwrap();
        assert_eq!(*file_count_b, 1, "component_b should have 1 file");
        assert_eq!(*stmt_count_b, 4, "component_b should have 4 statements");
    }

    #[test]
    fn test_analyze_preserves_threshold_from_args() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();

        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.threshold = 25;

        let result = rule.analyze(&args);

        assert!(result.is_ok(), "analyze should succeed");
        let data = result.unwrap();
        assert_eq!(
            data.threshold, 25,
            "threshold in result data should match args.threshold"
        );
    }

    #[test]
    fn test_render_statement_count_html_body_succeeds() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let data = rule.analyze(&args).expect("analyze should succeed");

        let result = rule.render_statement_count_html_body(&data);

        assert!(
            result.is_ok(),
            "render_statement_count_html_body should succeed with valid data"
        );

        let markup = result.unwrap();
        let html_string = markup.into_string();
        assert!(!html_string.is_empty(), "rendered HTML should not be empty");
        assert!(
            html_string.contains("table"),
            "rendered HTML should contain a table element"
        );
    }

    #[test]
    fn test_render_statement_count_html_contains_threshold() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let data = rule.analyze(&args).expect("analyze should succeed");

        let markup = rule
            .render_statement_count_html_body(&data)
            .expect("render should succeed");
        let html_string = markup.into_string();

        assert!(
            html_string.contains("10%"),
            "rendered HTML should contain the threshold value"
        );
    }

    #[test]
    fn test_render_statement_count_html_contains_grand_total() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let data = rule.analyze(&args).expect("analyze should succeed");

        let markup = rule
            .render_statement_count_html_body(&data)
            .expect("render should succeed");
        let html_string = markup.into_string();

        assert!(
            html_string.contains("Grand Total Statements"),
            "rendered HTML should contain grand total label"
        );
        assert!(
            html_string.contains(&data.grand_total.to_string()),
            "rendered HTML should contain grand total value"
        );
    }

    #[test]
    fn test_run_with_table_output_succeeds_when_within_threshold() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = StatementCountOutputFormat::Table;
        args.threshold = 100; // High threshold to ensure success

        let result = rule.run(&args);

        assert!(
            result.is_ok(),
            "run should succeed when components are within threshold"
        );
    }

    #[test]
    fn test_run_with_html_output_succeeds_when_within_threshold() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = StatementCountOutputFormat::Html;
        args.threshold = 100; // High threshold to ensure success

        let result = rule.run(&args);

        assert!(
            result.is_ok(),
            "run with HTML output should succeed when components are within threshold"
        );
    }

    #[test]
    fn test_run_fails_with_low_threshold() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = StatementCountOutputFormat::Table;
        args.threshold = 1; // Very low threshold to trigger failure

        let result = rule.run(&args);

        assert!(
            result.is_err(),
            "run should fail when at least one component exceeds threshold"
        );
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("exceeds") && error_msg.contains("%"),
            "error message should mention exceeding threshold percentage"
        );
    }

    #[test]
    fn test_statement_count_data_is_serializable() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let data = rule.analyze(&args).expect("analyze should succeed");

        // Test that StatementCountData can be serialized to JSON
        let json = serde_json::to_string(&data);
        assert!(
            json.is_ok(),
            "StatementCountData should be serializable to JSON"
        );

        let json_str = json.unwrap();
        assert!(
            json_str.contains("grand_total"),
            "JSON should contain grand_total field"
        );
        assert!(
            json_str.contains("threshold"),
            "JSON should contain threshold field"
        );
    }

    // Tests for the Rule trait implementation
    use crate::rule::Rule;

    #[test]
    fn test_rule_name_returns_statement_count() {
        assert_eq!(
            StatementCountRule::name(),
            "statement_count",
            "Rule name should be 'statement_count'"
        );
    }

    #[test]
    fn test_rule_description_returns_meaningful_text() {
        let description = StatementCountRule::description();
        assert!(
            !description.is_empty(),
            "Rule description should not be empty"
        );
        assert!(
            description.contains("statement") || description.contains("count"),
            "Rule description should describe the rule's purpose"
        );
    }

    #[test]
    fn test_rule_trait_run_delegates_correctly() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = StatementCountOutputFormat::Table;
        args.threshold = 100; // High threshold to ensure success

        // Call the Rule trait's run method
        let result = <StatementCountRule as Rule>::run(&rule, &args);

        assert!(
            result.is_ok(),
            "Rule trait run method should succeed when components are within threshold"
        );
    }

    #[test]
    fn test_rule_trait_analyze_returns_correct_data_type() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        // Call the Rule trait's analyze method
        let result = <StatementCountRule as Rule>::analyze(&rule, &args);

        assert!(
            result.is_ok(),
            "Rule trait analyze method should succeed with valid input"
        );

        let data = result.unwrap();
        assert_eq!(
            data.threshold, 10,
            "Analyzed data should have the correct threshold"
        );
        assert!(
            data.grand_total > 0,
            "Analyzed data should have positive grand_total"
        );
    }

    #[test]
    fn test_rule_trait_run_fails_with_low_threshold_via_trait() {
        let temp_dir = create_test_directory();
        let rule = StatementCountRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = StatementCountOutputFormat::Table;
        args.threshold = 1; // Very low threshold to trigger failure

        // Call the Rule trait's run method
        let result = <StatementCountRule as Rule>::run(&rule, &args);

        assert!(
            result.is_err(),
            "Rule trait run method should fail when threshold is exceeded"
        );
    }

    #[test]
    fn test_rule_associated_types_match() {
        // This test verifies that the associated types are correctly set
        // It's a compile-time check; if it compiles, the types are correct
        let rule = StatementCountRule::new();

        // Verify Config type is StatementCountArgs
        let config = StatementCountArgs {
            path: PathBuf::from("."),
            threshold: 10,
            output: StatementCountOutputFormat::Table,
        };

        // Verify Data type is StatementCountData
        let _config_check: <StatementCountRule as Rule>::Config = config;
        // We can't directly check Data type without an instance, but the
        // analyze method returning Result<StatementCountData> confirms it

        // Verify run and analyze work with these types
        let _ = rule;
    }
}
