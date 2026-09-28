//! Code Volatility Rule
//!
//! This module provides the volatility analysis rule, which measures how frequently
//! and extensively each crate in a workspace has changed over time. It uses Git
//! history to track commit touches, lines added/deleted, and calculates volatility
//! scores that can be normalized by lines of code.
//!
//! # Overview
//!
//! The volatility rule helps identify "hot spots" in the codebase—crates that are
//! undergoing frequent or extensive changes. This can indicate areas of active
//! development, technical debt, or instability that may warrant attention.
//!
//! # Volatility Scoring
//!
//! The raw volatility score is calculated as:
//! ```text
//! raw_score = commit_touches + α * (lines_added + lines_deleted)
//! ```
//!
//! Where α (alpha) is the weight applied to code churn (lines added plus lines
//! deleted) relative to commit touches, which always carry a weight of 1. Small
//! values (the default is 0.01) focus the score on how often a crate is touched;
//! larger values give lines changed more influence.
//!
//! When normalization is enabled, the score is divided by the total lines of code:
//! ```text
//! normalized_score = raw_score / total_loc
//! ```
//!
//! # Usage
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use raff_core::volatility_rule::VolatilityRule;
//! use raff_core::{VolatilityArgs, VolatilityOutputFormat};
//! use std::path::PathBuf;
//!
//! let rule = VolatilityRule::new();
//! let args = VolatilityArgs {
//!     path: PathBuf::from("."),
//!     alpha: 0.5,
//!     since: Some("2023-01-01".to_string()),
//!     normalize: true,
//!     output: VolatilityOutputFormat::Table,
//!     skip_merges: false,
//!     ci_output: None,
//!     output_file: None,
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
//! - [`VolatilityRule`]: The main rule implementation
//! - [`CrateStats`]: Statistics for a single crate including commit counts, churn, and scores
//! - [`VolatilityData`]: Container for all crate statistics and analysis parameters
//!
//! # Output Formats
//!
//! The rule supports multiple output formats:
//! - `Table`: Human-readable table with colored output
//! - `Json`: Machine-readable JSON
//! - `Yaml`: Machine-readable YAML
//! - `Csv`: Spreadsheet-compatible CSV
//! - `Html`: Interactive HTML report with sortable tables
//!
//! # Errors
//!
//! This module returns [`RaffError`] in the following cases:
//! - The provided path is not inside a Git working tree
//! - No crate (a `Cargo.toml` with `[package].name`) lies under or encloses the path
//! - Git operations fail (e.g., corrupted repository)

use git2::{DiffOptions, Repository, Sort, TreeWalkMode, TreeWalkResult};
use jiff::{Timestamp, civil::Date, tz::TimeZone};
use maud::{Markup, html};
use prettytable::{Cell, Row, Table, format}; // Added for table output
use serde::{Deserialize, Serialize}; // Added for custom output struct
// Ensure serde_json is explicitly imported
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write}; // For reading files line by line in LoC calculation and for writing output files
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;
use tracing::instrument; // Added import for tracing
use walkdir::WalkDir; // For recursively finding Cargo.toml files // For parsing Cargo.toml

use crate::cache::{CacheKey, CacheManager};
use crate::ci_report::{Finding, Severity, ToFindings};
use crate::cli::{CiOutputFormat, VolatilityArgs, VolatilityOutputFormat}; // Ensure VolatilityOutputFormat is imported
use crate::error::{RaffError, Result};
use crate::html_utils; // Import the new HTML utilities
use crate::rule::Rule;

/// Represents the statistics gathered for a single crate.
#[derive(Debug, Default, Clone, Serialize, Deserialize)] // Clone is useful for initialization, Deserialize for testing
pub struct CrateStats {
    /// The root directory path of the crate, relative to the repository root.
    pub root_path: PathBuf,
    /// Number of commits that touched this crate at least once.
    pub commit_touch_count: usize,
    /// Total lines inserted into this crate across all relevant commits.
    pub lines_added: usize,
    /// Total lines removed from this crate across all relevant commits.
    pub lines_deleted: usize,
    /// Raw volatility score.
    pub raw_score: f64,
    // No `skip_serializing_if` here: the cache stores this with bincode, which is not
    // self-describing, so skipped fields would make cached entries undecodable.
    /// (Optional) Total lines of code, used for normalization.
    pub total_loc: Option<usize>,
    /// (Optional) Normalized volatility score.
    pub normalized_score: Option<f64>,
    /// (Optional) Timestamp of the first commit where this crate appeared.
    pub birth_commit_time: Option<i64>,
}

/// Holds information about a discovered crate.
#[allow(dead_code)] // Will be used later
pub struct CrateInfo {
    name: String,
    root_path: PathBuf,
}

/// A map from crate name (String) to its `CrateStats`.
pub type CrateStatsMap = HashMap<String, CrateStats>;

/// Formats a commit time (Unix seconds) as a UTC `YYYY-MM-DD` date for reports.
fn format_commit_date(commit_time: Option<i64>) -> String {
    match commit_time.map(Timestamp::from_second) {
        None => "N/A".to_string(),
        Some(Ok(ts)) => ts.strftime("%Y-%m-%d").to_string(),
        Some(Err(_)) => "Invalid Date".to_string(),
    }
}

/// Cache version for volatility data.
/// Increment this when the serialisation format or scoring semantics change to invalidate old cache entries.
const VOLATILITY_CACHE_VERSION: &str = "4";

/// libgit2 pathspec restricting diffs to Rust sources (`*` also matches `/`).
const RUST_SOURCE_PATHSPEC: &str = "*.rs";

/// Rule to calculate code volatility for each crate in a Git repository.
#[derive(Debug, Default)]
pub struct VolatilityRule;

/// Data structure for JSON/YAML output, deriving Serialize.
#[derive(Serialize, Debug)]
struct CrateVolatilityDataForOutput<'a> {
    crate_name: &'a str,
    birth_date: String, // Formatted as YYYY-MM-DD
    commit_touch_count: usize,
    lines_added: usize,
    lines_deleted: usize,
    #[serde(skip_serializing_if = "Option::is_none")] // Only include if normalize was true
    total_loc: Option<usize>,
    raw_score: f64,
    #[serde(skip_serializing_if = "Option::is_none")] // Only include if normalize was true
    normalized_score: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VolatilityData {
    pub crate_stats_map: CrateStatsMap,
    pub normalize: bool,
    pub alpha: f64,
    pub analysis_path: PathBuf,
}

impl ToFindings for VolatilityData {
    #[instrument(skip(self), fields(rule_id = "volatility", alpha = self.alpha))]
    fn to_findings(&self) -> Vec<Finding> {
        let mut findings = Vec::new();

        // A crate is "highly volatile" relative to the others: the top quartile
        // of raw scores. With a single crate there is nothing to compare
        // against, and flagging it would fail every commit of a one-crate repo
        // under fail_on_warnings.
        if self.crate_stats_map.len() < 2 {
            return findings;
        }

        // Calculate threshold as 75th percentile of raw scores
        let mut scores: Vec<f64> = self
            .crate_stats_map
            .values()
            .map(|stats| stats.raw_score)
            .collect();
        scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let threshold_idx = (scores.len() * 3 / 4).min(scores.len() - 1);
        let threshold = scores.get(threshold_idx).copied().unwrap_or(0.0);

        for (crate_name, stats) in &self.crate_stats_map {
            // Only generate findings for crates with volatility at or above threshold
            // This ensures at least the top 25% of crates (by volatility) get flagged
            if stats.raw_score >= threshold && threshold > 0.0 {
                findings.push(Finding {
                    rule_id: "volatility".to_string(),
                    rule_name: "Code Volatility Rule".to_string(),
                    severity: Severity::Warning,
                    message: format!(
                        "Crate '{}' shows high volatility: raw score {:.2} ({} commits, {} lines added, {} lines deleted){}",
                        crate_name,
                        stats.raw_score,
                        stats.commit_touch_count,
                        stats.lines_added,
                        stats.lines_deleted,
                        if let Some(norm) = stats.normalized_score {
                            format!(", normalized score {:.4}", norm)
                        } else {
                            String::new()
                        }
                    ),
                    location: None, // Volatility is crate-level, no specific file location
                    help_uri: Some(
                        "https://github.com/liamwh/raff#volatility".to_string(),
                    ),
                    fingerprint: Some(format!(
                        "volatility:{}:{}:{}",
                        crate_name, self.alpha, stats.raw_score as i64
                    )),
                });
            }
        }
        findings
    }
}

impl Rule for VolatilityRule {
    type Config = VolatilityArgs;
    type Data = VolatilityData;

    fn name() -> &'static str {
        "volatility"
    }

    fn description() -> &'static str {
        "Analyzes code volatility in Git repositories by tracking commit frequency and code churn"
    }

    fn run(&self, config: &Self::Config) -> Result<()> {
        self.run_impl(config)
    }

    fn analyze(&self, config: &Self::Config) -> Result<Self::Data> {
        self.analyze_impl(config)
    }
}

impl VolatilityRule {
    pub fn new() -> Self {
        VolatilityRule
    }

    pub fn run(&self, args: &VolatilityArgs) -> Result<()> {
        self.run_impl(args)
    }

    pub fn analyze(&self, args: &VolatilityArgs) -> Result<VolatilityData> {
        self.analyze_impl(args)
    }

    /// Step 2 & 3: Identify crates and initialize their statistics.
    ///
    /// Finds every `Cargo.toml` with a `[package]` in the repository. All of them are
    /// needed to attribute each changed file to the crate that owns it, even when
    /// only some are reported (see [`Self::crates_in_scope`]). Crate root paths are
    /// relative to the repository root, matching the paths Git reports in diffs.
    ///
    /// # Arguments
    /// * `repo_root` - The canonical working directory of the Git repository.
    ///
    /// # Returns
    /// A `Result` containing a map from crate name to its initialized `CrateStats`,
    /// or an error if a manifest cannot be read or parsed.
    fn discover_crates_and_init_stats(&self, repo_root: &Path) -> Result<CrateStatsMap> {
        let mut crate_stats_map = CrateStatsMap::new();
        tracing::debug!(
            "Discovering crates by finding Cargo.toml files in {}",
            repo_root.display()
        );

        for entry in WalkDir::new(repo_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy() == "Cargo.toml")
        {
            let cargo_toml_path_abs = entry.path();
            let Some(crate_root_abs) = cargo_toml_path_abs.parent() else {
                continue;
            };

            let crate_root_relative = crate_root_abs
                .strip_prefix(repo_root)
                .map_err(|e| {
                    RaffError::parse_error_with_file(
                        crate_root_abs.to_path_buf(),
                        format!("Failed to make crate root path relative: {}", e),
                    )
                })?
                .to_path_buf();

            let content = fs::read_to_string(cargo_toml_path_abs)?;

            let toml_value = content.parse::<TomlValue>()?;

            let crate_name_opt = toml_value
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .map(String::from);

            if let Some(name) = crate_name_opt {
                if crate_stats_map.contains_key(&name) {
                    tracing::warn!(
                        crate_name = name,
                        new_path_relative = %crate_root_relative.display(),
                        "Duplicate crate name found. Overwriting with new path."
                    );
                }
                tracing::debug!(
                    "  Found crate: '{}' at (relative) {}",
                    name,
                    crate_root_relative.display()
                );
                crate_stats_map.insert(
                    name.clone(),
                    CrateStats {
                        root_path: crate_root_relative,
                        commit_touch_count: 0,
                        lines_added: 0,
                        lines_deleted: 0,
                        raw_score: 0.0,
                        total_loc: None,
                        normalized_score: None,
                        birth_commit_time: None,
                    },
                );
            } else {
                tracing::warn!(
                    path = %cargo_toml_path_abs.display(),
                    "Could not extract [package].name from Cargo.toml. Skipping."
                );
            }
        }
        Ok(crate_stats_map)
    }

    /// The crates a run over `analysis_path_relative` reports: every crate rooted
    /// inside it, plus the crate enclosing it, so `--path src` in a single-crate
    /// repository still reports that crate.
    fn crates_in_scope(
        &self,
        crate_stats_map: &CrateStatsMap,
        analysis_path_relative: &Path,
    ) -> HashSet<String> {
        let mut in_scope: HashSet<String> = crate_stats_map
            .iter()
            .filter(|(_, stats)| stats.root_path.starts_with(analysis_path_relative))
            .map(|(name, _)| name.clone())
            .collect();
        if let Some((enclosing, _)) =
            self.find_owning_crate(analysis_path_relative, crate_stats_map)
        {
            in_scope.insert(enclosing);
        }
        in_scope
    }

    /// Finds the owning crate for a given file path.
    /// The owning crate is the one whose root_path is the longest prefix of the file_path.
    /// Paths are expected to be canonicalized or consistently relative to the repo root.
    fn find_owning_crate(
        &self,
        file_path_in_repo: &Path,
        crate_stats_map: &CrateStatsMap,
    ) -> Option<(String, PathBuf)> {
        crate_stats_map
            .iter()
            .filter(|(_, stats)| file_path_in_repo.starts_with(&stats.root_path))
            // A crate at the repository root has depth 0 and must still match.
            .max_by_key(|(_, stats)| stats.root_path.components().count())
            .map(|(name, stats)| (name.clone(), stats.root_path.clone()))
    }

    /// Calculates the lines of code (LoC) for a given crate directory.
    /// Only considers `.rs` files and counts non-blank lines.
    #[tracing::instrument(level = "debug", skip(self), fields(crate_relative_path = %crate_relative_path.display()))]
    fn calculate_loc_for_crate(
        &self,
        crate_relative_path: &Path,
        repo_root: &Path,
    ) -> Result<usize> {
        let crate_abs_path = repo_root.join(crate_relative_path);
        tracing::debug!(path = %crate_abs_path.display(), "Calculating LoC for crate at absolute path");
        let mut total_loc = 0;
        for entry in WalkDir::new(crate_abs_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "rs"))
        {
            let file_path = entry.path();
            tracing::trace!(file = %file_path.display(), "Counting LoC for file");
            let file = fs::File::open(file_path)?;
            let reader = BufReader::new(file);
            for line_result in reader.lines() {
                let line = line_result?;
                if !line.trim().is_empty() {
                    total_loc += 1;
                }
            }
        }
        tracing::debug!(loc = total_loc, "Calculated LoC for crate");
        Ok(total_loc)
    }

    /// Prints the volatility report as a formatted table.
    fn print_volatility_table(
        &self,
        sorted_crates: &[(&String, &CrateStats)],
        normalize: bool,
        alpha: f64,
    ) {
        println!("\nVolatility Report Interpretation:");
        println!("-----------------------------------");
        println!("- Volatility: Higher scores indicate more frequent or larger changes.");
        println!("- Crate Name: The name of the crate as defined in its Cargo.toml.");
        println!("- Birth Date: Approx. date (YYYY-MM-DD) the crate first appeared in history.");
        println!(
            "- Touches: Number of commits that modified this crate within the analysis window."
        );
        println!("- Added: Total lines of code added to this crate.");
        println!("- Deleted: Total lines of code deleted from this crate.");
        if normalize {
            println!(
                "- Total LoC: Total non-blank lines of Rust code in the crate (used for normalization)."
            );
        }
        println!(
            "- Raw Score: Calculated as 'Touches + (alpha * (Added + Deleted))'. Alpha = {alpha:.4}. A higher score indicates more recent change activity (commits and/or lines changed)."
        );
        if normalize {
            println!(
                "- Norm Score: 'Raw Score / Total LoC'. Shows volatility relative to crate size. A higher score indicates more change activity relative to the crate's size."
            );
        }
        println!("-----------------------------------");

        let mut table = Table::new();
        let table_format = format::FormatBuilder::new()
            .column_separator('|')
            .borders('|')
            .separators(
                &[format::LinePosition::Top],
                format::LineSeparator::new('─', '┬', '┌', '┐'),
            )
            .separators(
                &[format::LinePosition::Intern],
                format::LineSeparator::new('─', '┼', '├', '┤'),
            )
            .separators(
                &[format::LinePosition::Bottom],
                format::LineSeparator::new('─', '┴', '└', '┘'),
            )
            .padding(1, 1)
            .build();
        table.set_format(table_format);

        // Header row
        let mut header_cells = vec![
            Cell::new("Crate Name"),
            Cell::new("Birth Date"),
            Cell::new("Touches"),
            Cell::new("Added"),
            Cell::new("Deleted"),
        ];
        if normalize {
            header_cells.push(Cell::new("Total LoC"));
        }
        header_cells.push(Cell::new("Raw Score"));
        if normalize {
            header_cells.push(Cell::new("Norm Score"));
        }
        table.add_row(Row::new(header_cells));

        // Data rows
        for (name, stats) in sorted_crates {
            let birth_date_str = format_commit_date(stats.birth_commit_time);

            let mut row_cells = vec![
                Cell::new(name),
                Cell::new(&birth_date_str),
                Cell::new(&stats.commit_touch_count.to_string()),
                Cell::new(&stats.lines_added.to_string()),
                Cell::new(&stats.lines_deleted.to_string()),
            ];
            if normalize {
                row_cells.push(Cell::new(
                    &stats
                        .total_loc
                        .map_or_else(|| "N/A".to_string(), |loc| loc.to_string()),
                ));
            }
            row_cells.push(Cell::new(&format!("{:.2}", stats.raw_score))); // Format score to 2 decimal places
            if normalize {
                row_cells.push(Cell::new(
                    &stats
                        .normalized_score
                        .map_or_else(|| "N/A".to_string(), |ns| format!("{ns:.2}")),
                ));
            }
            table.add_row(Row::new(row_cells));
        }
        println!("\nVolatility Report:");
        table.printstd();
    }

    /// Populates the `birth_commit_time` for each crate in the `crate_stats_map`.
    /// This method iterates through commits from oldest to newest.
    #[tracing::instrument(level = "debug", skip(self, repo, crate_stats_map), err)]
    fn populate_crate_birth_times(
        &self,
        repo: &Repository,
        crate_stats_map: &mut CrateStatsMap,
    ) -> Result<()> {
        tracing::info!(
            "Populating crate birth times by walking repository history (oldest first)..."
        );

        if crate_stats_map.is_empty() {
            tracing::debug!("No crates to populate birth times for. Skipping.");
            return Ok(());
        }

        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(Sort::TIME | Sort::REVERSE)?;

        let mut crates_needing_birth_time = crate_stats_map.len();

        for oid_result in revwalk {
            let oid = oid_result?;
            let commit = repo.find_commit(oid)?;
            let commit_time = commit.time().seconds();
            let tree = commit.tree()?;

            tracing::trace!(commit_oid = %commit.id(), commit_time, "Scanning commit for crate births");

            for (crate_name, stats) in crate_stats_map.iter_mut() {
                if stats.birth_commit_time.is_none() {
                    let mut found_birth = false;
                    tree.walk(TreeWalkMode::PreOrder, |path_from_tree_root, entry| {
                        let entry_path_relative_to_repo = Path::new(path_from_tree_root).join(entry.name().unwrap_or_default());

                        if entry_path_relative_to_repo.starts_with(&stats.root_path) {
                            stats.birth_commit_time = Some(commit_time);
                            tracing::debug!(%crate_name, commit_oid = %commit.id(), %commit_time, path_found = %entry_path_relative_to_repo.display(), "Set birth time for crate");
                            found_birth = true;
                            return TreeWalkResult::Skip;
                        }
                        TreeWalkResult::Ok
                    })
                    .map_err(|e| {
                        RaffError::git_error(format!(
                            "walk tree for commit {} to find birth of crate {}: {}",
                            commit.id(),
                            crate_name,
                            e
                        ))
                    })?;

                    if found_birth {
                        crates_needing_birth_time -= 1;
                    }
                }
            }

            if crates_needing_birth_time == 0 {
                tracing::debug!(
                    "All crate birth times have been populated. Stopping birth-time revwalk."
                );
                break;
            }
        }

        for (crate_name, stats) in crate_stats_map.iter() {
            if stats.birth_commit_time.is_none() {
                tracing::warn!(%crate_name, path = %stats.root_path.display(), "Could not determine birth time for crate. It will be considered active since the beginning of the analysis window.");
            }
        }

        tracing::info!("Finished populating crate birth times.");
        Ok(())
    }

    pub fn render_volatility_html_body(
        &self,
        sorted_crates: &[(&String, &CrateStats)],
        normalize: bool,
        alpha: f64,
    ) -> Result<Markup> {
        let explanations_data = [
            (
                "Crate Name",
                "The name of the crate as defined in its Cargo.toml.",
            ),
            (
                "Birth Date",
                "The date of the first commit where this crate's Cargo.toml appeared. 'N/A' if not found in history.",
            ),
            (
                "Commit Touches",
                "The number of commits (since the specified date) that modified any file within this crate.",
            ),
            (
                "Lines Added",
                "Total number of lines added to .rs files in this crate.",
            ),
            (
                "Lines Deleted",
                "Total number of lines deleted from .rs files in this crate.",
            ),
            (
                "Raw Score",
                "A combined metric calculated as: Commit Touches + α * (Lines Added + Lines Deleted). A higher score indicates higher churn/activity.",
            ),
        ];
        let mut explanations_data_vec = explanations_data.to_vec();

        if normalize {
            explanations_data_vec.extend(&[
                ("Total LoC", "Total lines of code in the crate's .rs files (non-empty lines)."),
                ("Normalized Score", "The Raw Score divided by the Total LoC. Provides a size-independent measure of volatility."),
            ]);
        }
        let explanations_markup =
            html_utils::render_metric_explanation_list(&explanations_data_vec);

        let added_values: Vec<f64> = sorted_crates
            .iter()
            .map(|s| s.1.lines_added as f64)
            .collect();
        let deleted_values: Vec<f64> = sorted_crates
            .iter()
            .map(|s| s.1.lines_deleted as f64)
            .collect();
        let touches_values: Vec<f64> = sorted_crates
            .iter()
            .map(|s| s.1.commit_touch_count as f64)
            .collect();
        let raw_score_values: Vec<f64> = sorted_crates.iter().map(|s| s.1.raw_score).collect();
        let normalized_score_values: Vec<f64> = sorted_crates
            .iter()
            .filter_map(|s| s.1.normalized_score)
            .collect();

        let added_ranges = html_utils::MetricRanges::from_values(&added_values, false);
        let deleted_ranges = html_utils::MetricRanges::from_values(&deleted_values, false);
        let touches_ranges = html_utils::MetricRanges::from_values(&touches_values, false);
        let raw_score_ranges = html_utils::MetricRanges::from_values(&raw_score_values, false);
        let norm_score_ranges =
            html_utils::MetricRanges::from_values(&normalized_score_values, false);

        let table_markup = html! {
            table class="sortable-table" {
                caption { (format!("Volatility calculated with α (churn weight) = {}", alpha)) }
                thead {
                    tr {
                        th class="sortable-header" data-column-index="0" data-sort-type="string" { "Crate Name" }
                        th class="sortable-header" data-column-index="1" data-sort-type="string" { "Birth Date" }
                        th class="sortable-header" data-column-index="2" data-sort-type="number" { "Commit Touches" }
                        th class="sortable-header" data-column-index="3" data-sort-type="number" { "Lines Added" }
                        th class="sortable-header" data-column-index="4" data-sort-type="number" { "Lines Deleted" }
                        @if normalize {
                            th class="sortable-header" data-column-index="5" data-sort-type="number" { "Total LoC" }
                            th class="sortable-header" data-column-index="6" data-sort-type="number" { "Raw Score" }
                            th class="sortable-header" data-column-index="7" data-sort-type="number" { "Normalized Score" }
                        } @else {
                            th class="sortable-header" data-column-index="5" data-sort-type="number" { "Raw Score" }
                        }
                    }
                }
                tbody {
                    @for (name, stats) in sorted_crates {
                        @let birth_date_str = format_commit_date(stats.birth_commit_time);
                        tr {
                            td { (name) }
                            td { (birth_date_str) }
                            td style=({touches_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(stats.commit_touch_count as f64, r))}) { (stats.commit_touch_count) }
                            td style=({added_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(stats.lines_added as f64, r))}) { (stats.lines_added) }
                            td style=({deleted_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(stats.lines_deleted as f64, r))}) { (stats.lines_deleted) }
                            @if normalize {
                                td { (stats.total_loc.map_or_else(|| "N/A".to_string(), |loc| loc.to_string())) }
                                td style=({raw_score_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(stats.raw_score, r))}) { (format!("{:.2}", stats.raw_score)) }
                                td style=({norm_score_ranges.as_ref().map_or_else(String::new, |r| stats.normalized_score.map_or_else(String::new, |ns_val| html_utils::get_metric_cell_style(ns_val,r)))})
                                   { (stats.normalized_score.map_or_else(|| "N/A".to_string(), |ns| format!("{ns:.2}"))) }
                            } @else {
                                td style=({raw_score_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(stats.raw_score, r))}) { (format!("{:.2}", stats.raw_score)) }
                            }
                        }
                    }
                }
            }
        };

        Ok(html! {
            (explanations_markup)
            (table_markup)
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    fn run_impl(&self, args: &VolatilityArgs) -> Result<()> {
        let data = self.analyze(args)?;

        // Check for CI output first (takes precedence)
        if let Some(ci_format) = &args.ci_output {
            let findings = data.to_findings();

            let output = match ci_format {
                CiOutputFormat::Sarif => crate::ci_report::to_sarif(&findings)?,
                CiOutputFormat::JUnit => crate::ci_report::to_junit(&findings, "volatility")?,
            };

            // Write to file if specified, otherwise stdout
            if let Some(ref output_file) = args.output_file {
                let mut file = fs::File::create(output_file).map_err(|e| {
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

            // Note: Volatility uses Severity::Warning, which doesn't fail CI
            // We always return Ok for volatility warnings
            return Ok(());
        }

        // Sort crates by raw_score (descending) for display
        let mut sorted_crates: Vec<_> = data.crate_stats_map.iter().collect();
        sorted_crates.sort_by(|a, b| {
            b.1.raw_score
                .partial_cmp(&a.1.raw_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Print output based on format
        match &args.output {
            VolatilityOutputFormat::Table => {
                self.print_volatility_table(&sorted_crates, data.normalize, data.alpha)
            }
            output_format @ (VolatilityOutputFormat::Json
            | VolatilityOutputFormat::Yaml
            | VolatilityOutputFormat::Csv) => {
                let output_data: Vec<CrateVolatilityDataForOutput> = sorted_crates
                    .iter()
                    .map(|(name, stats)| CrateVolatilityDataForOutput {
                        crate_name: name,
                        birth_date: format_commit_date(stats.birth_commit_time),
                        commit_touch_count: stats.commit_touch_count,
                        lines_added: stats.lines_added,
                        lines_deleted: stats.lines_deleted,
                        total_loc: stats.total_loc,
                        raw_score: stats.raw_score,
                        normalized_score: stats.normalized_score,
                    })
                    .collect();

                match output_format {
                    VolatilityOutputFormat::Json => {
                        println!("{}", serde_json::to_string_pretty(&output_data)?);
                    }
                    VolatilityOutputFormat::Yaml => {
                        println!("{}", serde_yaml::to_string(&output_data)?);
                    }
                    VolatilityOutputFormat::Csv => {
                        let mut wtr = csv::WriterBuilder::new()
                            .has_headers(true)
                            .from_writer(vec![]);
                        // Write header conditionally
                        let mut headers_vec = vec![
                            "crate_name",
                            "birth_date",
                            "commit_touch_count",
                            "lines_added",
                            "lines_deleted",
                            "raw_score",
                        ];
                        if data.normalize {
                            headers_vec.push("total_loc");
                            headers_vec.push("normalized_score");
                        }
                        wtr.write_record(&headers_vec)?;

                        for record in &output_data {
                            let mut row = vec![
                                record.crate_name.to_string(),
                                record.birth_date.clone(),
                                record.commit_touch_count.to_string(),
                                record.lines_added.to_string(),
                                record.lines_deleted.to_string(),
                                record.raw_score.to_string(),
                            ];
                            if data.normalize {
                                row.push(
                                    record
                                        .total_loc
                                        .map_or("N/A".to_string(), |v| v.to_string()),
                                );
                                row.push(
                                    record
                                        .normalized_score
                                        .map_or("N/A".to_string(), |v| v.to_string()),
                                );
                            }
                            wtr.write_record(&row)?;
                        }

                        let csv_string = String::from_utf8(wtr.into_inner().map_err(|e| {
                            RaffError::parse_error(format!("Failed to get CSV bytes: {}", e))
                        })?)
                        .map_err(|e| {
                            RaffError::parse_error(format!("Failed to convert CSV to UTF-8: {}", e))
                        })?;
                        println!("{csv_string}");
                    }
                    _ => unreachable!(), // Should not happen given the parent match arm
                }
            }
            VolatilityOutputFormat::Html => {
                let html_body =
                    self.render_volatility_html_body(&sorted_crates, data.normalize, data.alpha)?;
                let full_html = html_utils::render_html_doc(
                    &format!("Volatility Report: {}", data.analysis_path.display()),
                    html_body,
                );
                println!("{full_html}");
            }
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    fn analyze_impl(&self, args: &VolatilityArgs) -> Result<VolatilityData> {
        let analysis_path = &args.path;
        let analysis_path_canonical = analysis_path.canonicalize()?;

        // Discover the repository first to get the HEAD hash for the cache key. The
        // analysis path may be any directory inside the working tree.
        let repo = Repository::discover(&analysis_path_canonical).map_err(|e| {
            RaffError::git_error_with_repo(
                format!("open Git repository: {}", e),
                analysis_path_canonical.clone(),
            )
        })?;
        let repo_root = repo
            .workdir()
            .ok_or_else(|| {
                RaffError::git_error_with_repo(
                    "analyse a bare repository",
                    analysis_path_canonical.clone(),
                )
            })?
            .canonicalize()?;

        // Get HEAD commit hash for cache key
        let head_hash = repo
            .head()
            .and_then(|head| {
                head.target()
                    .ok_or_else(|| git2::Error::from_str("No HEAD target"))
            })
            .map(|oid| oid.to_string())
            .ok();

        // Build cache parameters from analysis arguments
        let mut cache_params = vec![
            (
                "cache_version".to_string(),
                VOLATILITY_CACHE_VERSION.to_string(),
            ),
            ("alpha".to_string(), args.alpha.to_string()),
            ("normalize".to_string(), args.normalize.to_string()),
        ];
        if let Some(since) = &args.since {
            cache_params.push(("since".to_string(), since.clone()));
        }
        if args.skip_merges {
            cache_params.push(("skip_merges".to_string(), "true".to_string()));
        }

        // Create cache manager and try to get cached result
        let cache_manager = CacheManager::new()?;
        let cache_key = CacheKey::new(
            format!("volatility:{}", analysis_path_canonical.display()),
            head_hash,
            cache_params,
        );

        if let Some(cached_data) = cache_manager.get_decoded::<VolatilityData>(&cache_key)? {
            tracing::info!("Using cached volatility analysis result");
            return Ok(cached_data);
        }
        tracing::info!(path = %analysis_path_canonical.display(), "Running volatility analysis on repository");
        tracing::debug!("Successfully opened Git repository.");

        let mut crate_stats_map = self.discover_crates_and_init_stats(&repo_root)?;
        let analysis_path_relative = analysis_path_canonical
            .strip_prefix(&repo_root)
            .unwrap_or(Path::new(""));
        let in_scope = self.crates_in_scope(&crate_stats_map, analysis_path_relative);
        if in_scope.is_empty() {
            return Err(RaffError::analysis_error(
                "volatility",
                format!(
                    "No crates (Cargo.toml with [package].name) found under or enclosing {}. Ensure you are running in a Rust project with crates.",
                    analysis_path_canonical.display()
                ),
            ));
        }

        let since_timestamp = args.since.as_ref().map_or(Ok(0_i64), |date_str| {
            date_str
                .parse::<Date>()
                .and_then(|date| date.to_zoned(TimeZone::UTC))
                .map(|midnight| midnight.timestamp().as_second())
                .map_err(|e| {
                    RaffError::invalid_input_with_arg(
                        format!(
                            "Invalid --since date format '{}': {}. Please use YYYY-MM-DD.",
                            date_str, e
                        ),
                        date_str.to_string(),
                    )
                })
        })?;
        tracing::debug!(
            since_timestamp = since_timestamp,
            "Processing commits since"
        );

        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(Sort::TIME | Sort::REVERSE)?;

        let mut processed_commits = 0;
        let mut pending_stat_updates: Vec<(String, char)> = Vec::new(); // For deferred updates

        for oid_result in revwalk {
            let oid = oid_result?;
            let commit = repo.find_commit(oid)?;
            let commit_time = commit.time().seconds();

            let parents: Vec<_> = commit.parents().collect();
            if args.skip_merges && parents.len() > 1 {
                tracing::trace!(commit_id = %oid, "Skipping merge commit.");
                continue;
            }

            let tree = commit.tree()?;
            let parent_tree_opt = if !parents.is_empty() {
                parents[0].tree().ok()
            } else {
                None
            };

            let mut diff_opts = DiffOptions::new();
            diff_opts.context_lines(0);
            diff_opts.interhunk_lines(0);
            // Only Rust sources count towards touches and churn; fixtures, lockfiles and
            // other assets inside a crate would otherwise dominate the score.
            diff_opts.pathspec(RUST_SOURCE_PATHSPEC);

            let diff = repo.diff_tree_to_tree(
                parent_tree_opt.as_ref(),
                Some(&tree),
                Some(&mut diff_opts),
            )?;

            if commit_time < since_timestamp {
                tracing::trace!(commit_id = %oid, commit_date = %format_commit_date(Some(commit_time)), "Commit is older than --since date, skipping for volatility calculation (but was considered for birth date).");
                continue;
            }

            processed_commits += 1;
            let mut touched_crates_in_commit = HashSet::new();
            pending_stat_updates.clear(); // Clear for each commit

            diff.foreach(
                &mut |delta, _progress| {
                    if let Some(delta_path) =
                        delta.new_file().path().or_else(|| delta.old_file().path())
                    {
                        // This immutable borrow of crate_stats_map is fine
                        if let Some((crate_name, _)) =
                            self.find_owning_crate(delta_path, &crate_stats_map)
                        {
                            touched_crates_in_commit.insert(crate_name.clone());
                        }
                    }
                    true
                },
                None, // binary_callback
                None, // hunk_callback
                Some(&mut |delta, _hunk, line| {
                    // line_callback
                    if let Some(delta_path) =
                        delta.new_file().path().or_else(|| delta.old_file().path())
                    {
                        // This immutable borrow of crate_stats_map is fine
                        if let Some((crate_name, _)) =
                            self.find_owning_crate(delta_path, &crate_stats_map)
                        {
                            // Defer mutation by pushing to pending_stat_updates
                            pending_stat_updates.push((crate_name.clone(), line.origin()));
                        }
                    }
                    true
                }),
            )
            .map_err(|e| RaffError::git_error(format!("process diff lines: {}", e)))?;

            // Apply pending updates for the current commit
            for (crate_name, origin) in &pending_stat_updates {
                // Iterate immutably here
                if let Some(stats) = crate_stats_map.get_mut(crate_name) {
                    match origin {
                        '+' | '>' => stats.lines_added += 1,
                        '-' | '<' => stats.lines_deleted += 1,
                        _ => {}
                    }
                }
            }

            for crate_name in touched_crates_in_commit {
                if let Some(stats) = crate_stats_map.get_mut(&crate_name) {
                    stats.commit_touch_count += 1;
                }
            }
        }
        tracing::info!(
            count = processed_commits,
            "Finished processing commits for volatility stats."
        );

        // Every crate took part in attributing changes; only those in scope are reported.
        crate_stats_map.retain(|name, _| in_scope.contains(name));
        self.populate_crate_birth_times(&repo, &mut crate_stats_map)?;

        for (name, stats) in crate_stats_map.iter_mut() {
            if args.normalize {
                match self.calculate_loc_for_crate(&stats.root_path, &repo_root) {
                    Ok(loc) => stats.total_loc = Some(loc),
                    Err(e) => {
                        tracing::warn!(
                            crate_name = name,
                            path = %stats.root_path.display(),
                            error = %e,
                            "Failed to calculate LoC for crate. Normalization might be affected."
                        );
                        stats.total_loc = None;
                    }
                }
            }
            stats.raw_score = stats.commit_touch_count as f64
                + args.alpha * (stats.lines_added + stats.lines_deleted) as f64;
            if let Some(loc) = stats.total_loc
                && loc > 0
            {
                stats.normalized_score = Some(stats.raw_score / loc as f64);
            }
        }

        let result = VolatilityData {
            crate_stats_map,
            normalize: args.normalize,
            alpha: args.alpha,
            analysis_path: analysis_path_canonical,
        };

        cache_manager.put_encoded(&cache_key, &result)?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{CiOutputFormat, VolatilityArgs, VolatilityOutputFormat};
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::TempDir;

    /// Helper to initialize git repository in a directory
    fn init_git_repo(dir: &PathBuf) -> Result<()> {
        let output = Command::new("git").arg("init").current_dir(dir).output()?;
        if !output.status.success() {
            return Err(RaffError::io_error_with_source(
                "init git repo",
                dir.clone(),
                std::io::Error::other(format!("Git init failed with status: {:?}", output.status)),
            ));
        }
        Ok(())
    }

    /// Helper to create a commit in a git repository
    fn create_commit(dir: &PathBuf, message: &str) -> Result<()> {
        // Set git config
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(dir)
            .output()?;

        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .output()?;

        // Add all files
        Command::new("git")
            .arg("add")
            .arg(".")
            .current_dir(dir)
            .output()?;

        // Commit
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(dir)
            .output()?;

        if !output.status.success() {
            return Err(RaffError::io_error_with_source(
                "create commit",
                dir.clone(),
                std::io::Error::other(format!(
                    "Failed to create commit: {:?}",
                    String::from_utf8_lossy(&output.stderr)
                )),
            ));
        }
        Ok(())
    }

    /// Helper to create a test directory with Rust crates and git history
    fn create_test_repo_with_crates() -> Result<TempDir> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path();

        // Initialize git repo
        init_git_repo(&repo_path.to_path_buf())?;

        // Create src directory
        let src_dir = repo_path.join("src");
        fs::create_dir_all(&src_dir)?;

        // Create Cargo.toml for main crate
        let cargo_toml = r#"
[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(repo_path.join("Cargo.toml"), cargo_toml)?;

        // Create a simple main.rs
        let main_rs = r#"
fn main() {
    let x = 5;
    println!("Hello, world!");
}
"#;
        fs::write(src_dir.join("main.rs"), main_rs)?;

        // Create initial commit
        create_commit(&repo_path.to_path_buf(), "Initial commit")?;

        Ok(temp_dir)
    }

    /// Helper to create test args
    fn create_test_args(path: PathBuf) -> VolatilityArgs {
        VolatilityArgs {
            path,
            alpha: 0.5,
            since: None,
            normalize: false,
            output: VolatilityOutputFormat::Table,
            skip_merges: false,
            ci_output: None,
            output_file: None,
        }
    }

    // Constructor tests

    #[test]
    fn test_volatility_rule_new_creates_instance() {
        let rule = VolatilityRule::new();
        // Just verify the rule can be created; struct has no fields to check
        let _ = rule;
    }

    #[test]
    fn test_volatility_rule_default_creates_instance() {
        let _rule = VolatilityRule;
    }

    // Data structure tests

    #[test]
    fn test_crate_stats_default_creates_empty_stats() {
        let stats = CrateStats::default();
        assert_eq!(
            stats.root_path,
            PathBuf::new(),
            "root_path should be empty by default"
        );
        assert_eq!(
            stats.commit_touch_count, 0,
            "commit_touch_count should be 0 by default"
        );
        assert_eq!(stats.lines_added, 0, "lines_added should be 0 by default");
        assert_eq!(
            stats.lines_deleted, 0,
            "lines_deleted should be 0 by default"
        );
        assert_eq!(stats.raw_score, 0.0, "raw_score should be 0.0 by default");
        assert!(
            stats.total_loc.is_none(),
            "total_loc should be None by default"
        );
        assert!(
            stats.normalized_score.is_none(),
            "normalized_score should be None by default"
        );
        assert!(
            stats.birth_commit_time.is_none(),
            "birth_commit_time should be None by default"
        );
    }

    #[test]
    fn test_crate_stats_clone_creates_independent_copy() {
        let mut stats = CrateStats {
            root_path: PathBuf::from("test/path"),
            commit_touch_count: 5,
            lines_added: 100,
            lines_deleted: 50,
            raw_score: 75.0,
            ..Default::default()
        };

        let cloned = stats.clone();

        // Verify all fields match
        assert_eq!(cloned.root_path, stats.root_path);
        assert_eq!(cloned.commit_touch_count, stats.commit_touch_count);
        assert_eq!(cloned.lines_added, stats.lines_added);
        assert_eq!(cloned.lines_deleted, stats.lines_deleted);
        assert_eq!(cloned.raw_score, stats.raw_score);

        // Modify original and verify clone is independent
        stats.commit_touch_count = 10;
        assert_eq!(
            cloned.commit_touch_count, 5,
            "clone should be independent of original"
        );
    }

    #[test]
    fn test_volatility_data_is_serializable() {
        let mut crate_stats_map = CrateStatsMap::new();
        crate_stats_map.insert(
            "test-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("src"),
                commit_touch_count: 10,
                lines_added: 100,
                lines_deleted: 50,
                raw_score: 60.0,
                total_loc: Some(200),
                normalized_score: Some(0.3),
                birth_commit_time: Some(1234567890),
            },
        );

        let data = VolatilityData {
            crate_stats_map,
            normalize: true,
            alpha: 0.5,
            analysis_path: PathBuf::from("/test/path"),
        };

        // Test serialization
        let json = serde_json::to_string(&data);
        assert!(
            json.is_ok(),
            "VolatilityData should be serializable to JSON"
        );

        let json_str = json.unwrap();
        assert!(
            json_str.contains("test-crate"),
            "JSON should contain crate name"
        );
        assert!(
            json_str.contains("commit_touch_count"),
            "JSON should contain commit_touch_count"
        );
    }

    #[test]
    fn test_crate_stats_json_roundtrip() {
        let stats = CrateStats {
            root_path: PathBuf::from("src"),
            commit_touch_count: 10,
            lines_added: 100,
            lines_deleted: 50,
            raw_score: 60.0,
            total_loc: Some(200),
            normalized_score: Some(0.3),
            birth_commit_time: Some(1234567890),
        };

        // Serialize to JSON
        let json = serde_json::to_string(&stats).expect("serialization should succeed");

        // Deserialize back
        let deserialized: CrateStats =
            serde_json::from_str(&json).expect("deserialization should succeed");

        assert_eq!(deserialized.root_path, stats.root_path);
        assert_eq!(deserialized.commit_touch_count, stats.commit_touch_count);
        assert_eq!(deserialized.lines_added, stats.lines_added);
        assert_eq!(deserialized.lines_deleted, stats.lines_deleted);
        assert_eq!(deserialized.raw_score, stats.raw_score);
        assert_eq!(deserialized.total_loc, stats.total_loc);
        assert_eq!(deserialized.normalized_score, stats.normalized_score);
        assert_eq!(deserialized.birth_commit_time, stats.birth_commit_time);
    }

    // Pure function tests

    #[test]
    fn test_find_owning_crate_returns_exact_match() {
        let rule = VolatilityRule::new();
        let mut crate_stats_map = CrateStatsMap::new();

        crate_stats_map.insert(
            "crate-a".to_string(),
            CrateStats {
                root_path: PathBuf::from("crates/a"),
                ..Default::default()
            },
        );

        crate_stats_map.insert(
            "crate-b".to_string(),
            CrateStats {
                root_path: PathBuf::from("crates/b"),
                ..Default::default()
            },
        );

        // Test exact match
        let result =
            rule.find_owning_crate(&PathBuf::from("crates/a/src/main.rs"), &crate_stats_map);
        assert!(
            result.is_some(),
            "should find owning crate for file in crate-a"
        );
        let (name, path) = result.unwrap();
        assert_eq!(name, "crate-a");
        assert_eq!(path, PathBuf::from("crates/a"));
    }

    #[test]
    fn test_find_owning_crate_returns_longest_prefix_match() {
        let rule = VolatilityRule::new();
        let mut crate_stats_map = CrateStatsMap::new();

        // Create nested crate structure
        crate_stats_map.insert(
            "root-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from(""),
                ..Default::default()
            },
        );

        crate_stats_map.insert(
            "nested-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("crates/nested"),
                ..Default::default()
            },
        );

        // Test that nested crate wins over root crate
        let result =
            rule.find_owning_crate(&PathBuf::from("crates/nested/src/lib.rs"), &crate_stats_map);
        assert!(result.is_some(), "should find owning crate for nested file");
        let (name, path) = result.unwrap();
        assert_eq!(name, "nested-crate");
        assert_eq!(path, PathBuf::from("crates/nested"));
    }

    #[test]
    fn test_find_owning_crate_returns_none_for_no_match() {
        let rule = VolatilityRule::new();
        let mut crate_stats_map = CrateStatsMap::new();

        crate_stats_map.insert(
            "crate-a".to_string(),
            CrateStats {
                root_path: PathBuf::from("crates/a"),
                ..Default::default()
            },
        );

        // Test file not in any crate
        let result = rule.find_owning_crate(&PathBuf::from("other/path/file.rs"), &crate_stats_map);
        assert!(
            result.is_none(),
            "should return None for file not in any crate"
        );
    }

    #[test]
    fn test_find_owning_crate_with_empty_map_returns_none() {
        let rule = VolatilityRule::new();
        let crate_stats_map = CrateStatsMap::new();

        let result = rule.find_owning_crate(&PathBuf::from("any/path/file.rs"), &crate_stats_map);
        assert!(
            result.is_none(),
            "should return None when crate map is empty"
        );
    }

    // HTML rendering tests

    #[test]
    fn test_render_volatility_html_body_produces_valid_markup() {
        let rule = VolatilityRule::new();
        let mut crate_stats_map = CrateStatsMap::new();

        crate_stats_map.insert(
            "test-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("src"),
                commit_touch_count: 10,
                lines_added: 100,
                lines_deleted: 50,
                raw_score: 60.0,
                total_loc: Some(200),
                normalized_score: Some(0.3),
                birth_commit_time: Some(1234567890),
            },
        );

        let sorted_crates: Vec<_> = crate_stats_map.iter().collect();

        let result = rule.render_volatility_html_body(&sorted_crates, true, 0.5);

        assert!(
            result.is_ok(),
            "render_volatility_html_body should succeed with valid data"
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
    fn test_render_volatility_html_body_with_normalize_false() {
        let rule = VolatilityRule::new();
        let mut crate_stats_map = CrateStatsMap::new();

        crate_stats_map.insert(
            "test-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("src"),
                commit_touch_count: 10,
                lines_added: 100,
                lines_deleted: 50,
                raw_score: 60.0,
                total_loc: None,
                normalized_score: None,
                birth_commit_time: Some(1234567890),
            },
        );

        let sorted_crates: Vec<_> = crate_stats_map.iter().collect();

        let result = rule.render_volatility_html_body(&sorted_crates, false, 0.5);

        assert!(
            result.is_ok(),
            "render_volatility_html_body should succeed with normalize=false"
        );

        let markup = result.unwrap();
        let html_string = markup.into_string();

        // When normalize is false, Total LoC and Norm Score columns should not be in header
        // The header still has the columns for consistency but we can verify alpha is shown
        assert!(
            html_string.contains("0.5"),
            "rendered HTML should contain alpha value"
        );
    }

    #[test]
    fn test_render_volatility_html_body_with_empty_crates() {
        let rule = VolatilityRule::new();
        let crate_stats_map = CrateStatsMap::new();
        let sorted_crates: Vec<_> = crate_stats_map.iter().collect();

        let result = rule.render_volatility_html_body(&sorted_crates, true, 0.5);

        assert!(
            result.is_ok(),
            "render_volatility_html_body should succeed even with empty crates"
        );

        let markup = result.unwrap();
        let html_string = markup.into_string();
        assert!(
            html_string.contains("table"),
            "rendered HTML should contain table element even when empty"
        );
    }

    #[test]
    fn test_render_volatility_html_body_contains_crate_name() {
        let rule = VolatilityRule::new();
        let mut crate_stats_map = CrateStatsMap::new();

        crate_stats_map.insert(
            "my-awesome-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("src"),
                commit_touch_count: 5,
                lines_added: 50,
                lines_deleted: 25,
                raw_score: 30.0,
                total_loc: Some(100),
                normalized_score: Some(0.3),
                birth_commit_time: Some(1234567890),
            },
        );

        let sorted_crates: Vec<_> = crate_stats_map.iter().collect();

        let markup = rule
            .render_volatility_html_body(&sorted_crates, true, 0.5)
            .expect("render should succeed");
        let html_string = markup.into_string();

        assert!(
            html_string.contains("my-awesome-crate"),
            "rendered HTML should contain the crate name"
        );
    }

    // Integration tests with git repository

    #[test]
    fn test_discover_crates_finds_single_crate() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");

        let rule = VolatilityRule::new();
        let result = rule.discover_crates_and_init_stats(temp_dir.path());

        assert!(
            result.is_ok(),
            "discover_crates_and_init_stats should find the test crate"
        );

        let crate_map = result.unwrap();
        assert!(
            crate_map.contains_key("test-crate"),
            "should find the test-crate"
        );

        let stats = crate_map.get("test-crate").unwrap();
        assert_eq!(
            stats.root_path,
            PathBuf::from(""),
            "root_path should be repo root for this crate"
        );
        assert_eq!(
            stats.commit_touch_count, 0,
            "initial touch count should be 0"
        );
        assert_eq!(stats.lines_added, 0, "initial lines_added should be 0");
        assert_eq!(stats.lines_deleted, 0, "initial lines_deleted should be 0");
    }

    #[test]
    fn test_analyze_fails_when_no_crate_is_in_scope() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();
        init_git_repo(&repo_path).expect("Failed to init git repo");
        fs::write(repo_path.join("notes.rs"), "fn f() {}\n").expect("Failed to write file");
        create_commit(&repo_path, "No crates").expect("Failed to commit");

        let error = VolatilityRule::new()
            .analyze(&create_test_args(repo_path))
            .expect_err("analysis without crates should fail");

        assert!(
            error.to_string().contains("No crates"),
            "unexpected error: {error}"
        );
    }

    /// Commits `contents` to `relative` in the repository at `repo_path`.
    fn commit_file(repo_path: &PathBuf, relative: &str, contents: &str) {
        let path = repo_path.join(relative);
        fs::create_dir_all(path.parent().expect("file has a parent"))
            .expect("Failed to create directory");
        fs::write(&path, contents).expect("Failed to write file");
        create_commit(repo_path, &format!("Change {relative}")).expect("Failed to commit");
    }

    #[test]
    fn test_analyze_subdirectory_reports_the_enclosing_crate() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");
        let repo_path = temp_dir.path().to_path_buf();
        commit_file(
            &repo_path,
            "src/main.rs",
            "fn main() {\n    let y = 1;\n}\n",
        );
        // A nested crate outside `src` must keep its changes to itself.
        commit_file(
            &repo_path,
            "tools/helper/Cargo.toml",
            "[package]\nname = \"helper\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        commit_file(&repo_path, "tools/helper/src/lib.rs", "pub fn h() {}\n");

        let data = VolatilityRule::new()
            .analyze(&create_test_args(repo_path.join("src")))
            .expect("analysing a subdirectory of the repository should succeed");

        assert_eq!(
            data.crate_stats_map.keys().collect::<Vec<_>>(),
            ["test-crate"],
            "only the crate enclosing src is reported"
        );
        assert_eq!(data.crate_stats_map["test-crate"].commit_touch_count, 2);
    }

    #[test]
    fn test_analyze_with_valid_git_repository() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");

        let rule = VolatilityRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let result = rule.analyze(&args);

        assert!(
            result.is_ok(),
            "analyze should succeed with valid git repository containing crates"
        );

        let data = result.unwrap();
        assert_eq!(data.alpha, 0.5, "alpha should match args");
        assert!(!data.normalize, "normalize should match args");
        assert_eq!(
            data.analysis_path,
            temp_dir.path().canonicalize().unwrap(),
            "analysis_path should be canonicalized"
        );
        assert!(
            !data.crate_stats_map.is_empty(),
            "should find at least one crate"
        );
    }

    #[test]
    fn test_analyze_fails_with_non_git_repository() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src directory");

        // Create Cargo.toml without git repo
        let cargo_toml = r#"
[package]
name = "no-git-crate"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml)
            .expect("Failed to write Cargo.toml");

        let rule = VolatilityRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let result = rule.analyze(&args);

        assert!(
            result.is_err(),
            "analyze should fail with non-git repository"
        );
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Git repository") || error_msg.contains("git"),
            "error message should mention git repository issue"
        );
    }

    #[test]
    fn test_analyze_with_normalize_enabled_calculates_loc() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");

        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.normalize = true;

        let result = rule.analyze(&args);

        assert!(result.is_ok(), "analyze should succeed with normalize=true");

        let data = result.unwrap();
        assert!(data.normalize, "normalize should be true in result");

        // Check that LoC was calculated for at least one crate
        let crate_has_loc = data
            .crate_stats_map
            .values()
            .any(|stats| stats.total_loc.is_some() && stats.total_loc.unwrap() > 0);

        assert!(
            crate_has_loc,
            "at least one crate should have total_loc calculated"
        );
    }

    #[test]
    fn test_analyze_calculates_raw_score_correctly() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");

        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        // A non-unit alpha distinguishes touches + alpha * churn from churn + alpha * touches
        args.alpha = 0.5;

        let result = rule.analyze(&args);

        assert!(result.is_ok(), "analyze should succeed");

        let data = result.unwrap();
        assert!(
            data.crate_stats_map
                .values()
                .any(|stats| stats.lines_added + stats.lines_deleted != stats.commit_touch_count),
            "fixture needs a crate whose churn differs from its touch count: {:?}",
            data.crate_stats_map
        );
        for stats in data.crate_stats_map.values() {
            // raw_score = commit_touch_count + alpha * (lines_added + lines_deleted)
            let expected_raw_score = stats.commit_touch_count as f64
                + args.alpha * (stats.lines_added + stats.lines_deleted) as f64;
            assert!(
                (stats.raw_score - expected_raw_score).abs() < 0.01,
                "raw_score should be calculated correctly: expected {}, got {}",
                expected_raw_score,
                stats.raw_score
            );
        }
    }

    #[test]
    fn test_analyze_ignores_commits_touching_only_non_rust_files() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");
        let repo_path = temp_dir.path().to_path_buf();
        let rule = VolatilityRule::new();
        let args = create_test_args(repo_path.clone());
        let before = rule
            .analyze(&args)
            .expect("analyze should succeed")
            .crate_stats_map["test-crate"]
            .clone();

        let fixtures_dir = repo_path.join("tests/fixtures");
        fs::create_dir_all(&fixtures_dir).expect("Failed to create fixtures directory");
        fs::write(fixtures_dir.join("encoding.txt"), "line\n".repeat(20_000))
            .expect("Failed to write fixture");
        create_commit(&repo_path, "Add large text fixture").expect("Failed to commit fixture");

        let after = rule
            .analyze(&args)
            .expect("analyze should succeed")
            .crate_stats_map["test-crate"]
            .clone();

        assert_eq!(after.commit_touch_count, before.commit_touch_count);
        assert_eq!(after.lines_added, before.lines_added);
        assert_eq!(after.lines_deleted, before.lines_deleted);
    }

    #[test]
    #[serial_test::serial]
    fn test_analyze_reads_its_own_cache_and_treats_undecodable_entry_as_miss() {
        let cache_dir = TempDir::new().expect("Failed to create cache directory");
        // SAFETY: serialised against other tests that touch the environment.
        unsafe { std::env::set_var("RAFF_CACHE_DIR", cache_dir.path()) };
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");
        let rule = VolatilityRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        let manifest = temp_dir.path().join("Cargo.toml");
        let manifest_content = fs::read(&manifest).expect("Failed to read Cargo.toml");

        let fresh = rule.analyze(&args);
        // Without the manifest a recomputation fails, so success here proves a cache hit.
        fs::remove_file(&manifest).expect("Failed to remove Cargo.toml");
        let cached = rule.analyze(&args);
        fs::write(&manifest, manifest_content).expect("Failed to restore Cargo.toml");

        let cache_entries: Vec<PathBuf> = fs::read_dir(cache_dir.path())
            .expect("cache directory should be readable")
            .map(|entry| entry.expect("cache entry").path())
            .collect();
        assert_eq!(cache_entries.len(), 1, "one entry for this analysis");
        let undecodable = bincode::serialize(&crate::cache::CacheEntry::new(vec![10, 255, 0, 7]))
            .expect("entry should serialise");
        fs::write(&cache_entries[0], undecodable).expect("Failed to overwrite cache entry");
        let after_corruption = rule.analyze(&args);
        unsafe { std::env::remove_var("RAFF_CACHE_DIR") };

        let raw_score = |data: Result<VolatilityData>, run: &str| {
            data.unwrap_or_else(|e| panic!("{run} run failed: {e}"))
                .crate_stats_map["test-crate"]
                .raw_score
        };
        let expected = raw_score(fresh, "fresh");
        assert_eq!(raw_score(cached, "cached"), expected);
        assert_eq!(raw_score(after_corruption, "post-corruption"), expected);
    }

    // Output format tests

    #[test]
    fn test_run_with_table_output_succeeds() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");

        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = VolatilityOutputFormat::Table;

        let result = rule.run(&args);

        assert!(
            result.is_ok(),
            "run with Table output should succeed with valid repository"
        );
    }

    #[test]
    fn test_run_with_html_output_succeeds() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");

        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = VolatilityOutputFormat::Html;

        let result = rule.run(&args);

        assert!(
            result.is_ok(),
            "run with Html output should succeed with valid repository"
        );
    }

    #[test]
    fn test_run_with_json_output_succeeds() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");

        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = VolatilityOutputFormat::Json;

        let result = rule.run(&args);

        assert!(
            result.is_ok(),
            "run with Json output should succeed with valid repository"
        );
    }

    #[test]
    fn test_run_with_yaml_output_succeeds() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");

        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = VolatilityOutputFormat::Yaml;

        let result = rule.run(&args);

        assert!(
            result.is_ok(),
            "run with Yaml output should succeed with valid repository"
        );
    }

    #[test]
    fn test_run_with_csv_output_succeeds() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");

        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.output = VolatilityOutputFormat::Csv;

        let result = rule.run(&args);

        assert!(
            result.is_ok(),
            "run with Csv output should succeed with valid repository"
        );
    }

    // Edge case tests

    #[test]
    fn test_crate_stats_with_all_optional_fields_none() {
        let stats = CrateStats {
            root_path: PathBuf::from("test"),
            commit_touch_count: 0,
            lines_added: 0,
            lines_deleted: 0,
            raw_score: 0.0,
            total_loc: None,
            normalized_score: None,
            birth_commit_time: None,
        };

        // Should serialize correctly
        let json =
            serde_json::to_string(&stats).expect("CrateStats with None fields should serialize");
        assert!(json.contains("test"), "JSON should contain path");
        assert!(json.contains("0"), "JSON should contain zero values");
    }

    #[test]
    fn test_crate_stats_with_zero_values() {
        let stats = CrateStats {
            root_path: PathBuf::from("test"),
            commit_touch_count: 0,
            lines_added: 0,
            lines_deleted: 0,
            raw_score: 0.0,
            total_loc: Some(0),
            normalized_score: Some(0.0),
            birth_commit_time: Some(0),
        };

        assert_eq!(stats.commit_touch_count, 0);
        assert_eq!(stats.lines_added, 0);
        assert_eq!(stats.lines_deleted, 0);
        assert_eq!(stats.raw_score, 0.0);
    }

    #[test]
    fn test_find_owning_crate_with_multiple_nested_crates() {
        let rule = VolatilityRule::new();
        let mut crate_stats_map = CrateStatsMap::new();

        // Create deeply nested structure
        crate_stats_map.insert(
            "workspace".to_string(),
            CrateStats {
                root_path: PathBuf::from(""),
                ..Default::default()
            },
        );

        crate_stats_map.insert(
            "crates-level".to_string(),
            CrateStats {
                root_path: PathBuf::from("crates"),
                ..Default::default()
            },
        );

        crate_stats_map.insert(
            "nested-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("crates/nested"),
                ..Default::default()
            },
        );

        crate_stats_map.insert(
            "deeply-nested".to_string(),
            CrateStats {
                root_path: PathBuf::from("crates/nested/deep"),
                ..Default::default()
            },
        );

        // Test that deepest match wins
        let result = rule.find_owning_crate(
            &PathBuf::from("crates/nested/deep/src/lib.rs"),
            &crate_stats_map,
        );

        assert!(result.is_some());
        let (name, _path) = result.unwrap();
        assert_eq!(name, "deeply-nested");
    }

    // Tests for the Rule trait implementation
    use crate::rule::Rule;

    #[test]
    fn test_rule_name_returns_volatility() {
        assert_eq!(
            VolatilityRule::name(),
            "volatility",
            "Rule name should be 'volatility'"
        );
    }

    #[test]
    fn test_rule_description_returns_meaningful_text() {
        let description = VolatilityRule::description();
        assert!(
            !description.is_empty(),
            "Rule description should not be empty"
        );
        assert!(
            description.contains("volatility")
                || description.contains("Git")
                || description.contains("churn"),
            "Rule description should describe the rule's purpose"
        );
    }

    #[test]
    fn test_rule_trait_run_delegates_correctly() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");
        let rule = VolatilityRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        // Call the Rule trait's run method
        let result = <VolatilityRule as Rule>::run(&rule, &args);

        assert!(
            result.is_ok(),
            "Rule trait run method should succeed with valid git repository"
        );
    }

    #[test]
    fn test_rule_trait_analyze_returns_correct_data_type() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");
        let rule = VolatilityRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        // Call the Rule trait's analyze method
        let result = <VolatilityRule as Rule>::analyze(&rule, &args);

        assert!(
            result.is_ok(),
            "Rule trait analyze method should succeed with valid input"
        );

        let data = result.unwrap();
        assert_eq!(
            data.alpha, 0.5,
            "Analyzed data should have the correct alpha"
        );
        assert!(
            !data.crate_stats_map.is_empty(),
            "Analyzed data should have crate stats"
        );
    }

    #[test]
    fn test_rule_trait_analyze_fails_with_non_git_repository() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src directory");

        // Create Cargo.toml without git repo
        let cargo_toml = r#"
[package]
name = "no-git-crate"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml)
            .expect("Failed to write Cargo.toml");

        let rule = VolatilityRule::new();
        let args = create_test_args(temp_dir.path().to_path_buf());

        // Call the Rule trait's analyze method
        let result = <VolatilityRule as Rule>::analyze(&rule, &args);

        assert!(
            result.is_err(),
            "Rule trait analyze method should fail with non-git repository"
        );
    }

    #[test]
    fn test_rule_associated_types_match() {
        // This test verifies that the associated types are correctly set
        // It's a compile-time check; if it compiles, the types are correct
        let rule = VolatilityRule::new();

        // Verify Config type is VolatilityArgs
        let config = VolatilityArgs {
            path: PathBuf::from("."),
            alpha: 0.5,
            since: None,
            normalize: false,
            output: VolatilityOutputFormat::Table,
            skip_merges: false,
            ci_output: None,
            output_file: None,
        };

        // Verify Data type is VolatilityData
        let _config_check: <VolatilityRule as Rule>::Config = config;
        // We can't directly check Data type without an instance, but the
        // analyze method returning Result<VolatilityData> confirms it

        // Verify run and analyze work with these types
        let _ = rule;
    }

    // Tests for CI output functionality

    #[test]
    fn test_to_findings_with_empty_crate_stats() {
        let data = VolatilityData {
            crate_stats_map: CrateStatsMap::new(),
            normalize: false,
            alpha: 0.5,
            analysis_path: PathBuf::from("/test"),
        };
        let findings = data.to_findings();
        assert!(
            findings.is_empty(),
            "to_findings should return empty when crate stats is empty"
        );
    }

    #[test]
    fn test_to_findings_with_no_high_volatility_crates() {
        let mut crate_stats_map = CrateStatsMap::new();
        // All crates have low volatility (raw_score = 0)
        crate_stats_map.insert(
            "stable-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("stable"),
                commit_touch_count: 0,
                lines_added: 0,
                lines_deleted: 0,
                raw_score: 0.0,
                total_loc: None,
                normalized_score: None,
                birth_commit_time: None,
            },
        );

        let data = VolatilityData {
            crate_stats_map,
            normalize: false,
            alpha: 0.5,
            analysis_path: PathBuf::from("/test"),
        };
        let findings = data.to_findings();
        assert!(
            findings.is_empty(),
            "to_findings should return empty when no crates exceed threshold"
        );
    }

    #[test]
    fn test_to_findings_with_high_volatility_crates() {
        let mut crate_stats_map = CrateStatsMap::new();
        // Add one crate with high volatility and one with low volatility
        crate_stats_map.insert(
            "high-volatility-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("high"),
                commit_touch_count: 100,
                lines_added: 500,
                lines_deleted: 200,
                raw_score: 750.0,
                total_loc: Some(1000),
                normalized_score: Some(0.75),
                birth_commit_time: Some(1234567890),
            },
        );
        crate_stats_map.insert(
            "low-volatility-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("low"),
                commit_touch_count: 1,
                lines_added: 5,
                lines_deleted: 2,
                raw_score: 7.05,
                total_loc: None,
                normalized_score: None,
                birth_commit_time: None,
            },
        );

        let data = VolatilityData {
            crate_stats_map,
            normalize: false,
            alpha: 0.5,
            analysis_path: PathBuf::from("/test"),
        };
        let findings = data.to_findings();

        assert!(
            !findings.is_empty(),
            "to_findings should return findings when crates exceed threshold"
        );

        // Verify the first finding has the expected properties
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "volatility");
        assert_eq!(finding.rule_name, "Code Volatility Rule");
        assert_eq!(finding.severity, Severity::Warning);
        assert!(
            finding.message.contains("high volatility"),
            "finding message should mention high volatility"
        );
        assert!(
            finding.fingerprint.is_some(),
            "finding should have a fingerprint for deduplication"
        );
        assert!(
            finding.location.is_none(),
            "finding location should be None since volatility is crate-level"
        );
    }

    #[test]
    fn test_to_findings_fingerprint_includes_crate_and_alpha() {
        let mut crate_stats_map = CrateStatsMap::new();
        crate_stats_map.insert(
            "test-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("test"),
                commit_touch_count: 100,
                lines_added: 500,
                lines_deleted: 200,
                raw_score: 750.0,
                total_loc: None,
                normalized_score: None,
                birth_commit_time: None,
            },
        );
        crate_stats_map.insert(
            "calm-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("calm"),
                commit_touch_count: 1,
                lines_added: 1,
                lines_deleted: 0,
                raw_score: 1.5,
                total_loc: None,
                normalized_score: None,
                birth_commit_time: None,
            },
        );

        let data = VolatilityData {
            crate_stats_map,
            normalize: false,
            alpha: 0.5,
            analysis_path: PathBuf::from("/test"),
        };
        let findings = data.to_findings();

        assert!(!findings.is_empty(), "should have at least one finding");

        let fingerprint = findings[0]
            .fingerprint
            .as_ref()
            .expect("should have fingerprint");
        assert!(
            fingerprint.contains("volatility:"),
            "fingerprint should contain rule ID"
        );
        assert!(
            fingerprint.contains(":test-crate:"),
            "fingerprint should contain crate name"
        );
        assert!(
            fingerprint.contains(":0.5:"),
            "fingerprint should contain alpha value"
        );
    }

    #[test]
    fn test_to_findings_single_crate_has_nothing_to_compare_against() {
        let mut crate_stats_map = CrateStatsMap::new();
        crate_stats_map.insert(
            "only-crate".to_string(),
            CrateStats {
                root_path: PathBuf::from("."),
                commit_touch_count: 50,
                lines_added: 200,
                lines_deleted: 100,
                raw_score: 350.0,
                total_loc: None,
                normalized_score: None,
                birth_commit_time: None,
            },
        );

        let data = VolatilityData {
            crate_stats_map,
            normalize: false,
            alpha: 0.5,
            analysis_path: PathBuf::from("/test"),
        };
        assert!(
            data.to_findings().is_empty(),
            "a lone crate is not volatile relative to anything"
        );
    }

    #[test]
    fn test_run_with_ci_output_sarif_succeeds() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");
        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.ci_output = Some(CiOutputFormat::Sarif);

        let result = rule.run(&args);

        assert!(result.is_ok(), "run with SARIF CI output should succeed");
    }

    #[test]
    fn test_run_with_ci_output_junit_succeeds() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");
        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.ci_output = Some(CiOutputFormat::JUnit);

        let result = rule.run(&args);

        assert!(result.is_ok(), "run with JUnit CI output should succeed");
    }

    #[test]
    fn test_run_with_ci_output_does_not_fail_on_warnings() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");
        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.ci_output = Some(CiOutputFormat::Sarif);

        let result = rule.run(&args);

        assert!(
            result.is_ok(),
            "run with CI output should not fail on warnings (Warning severity doesn't fail CI)"
        );
    }

    #[test]
    fn test_run_with_ci_output_and_output_file_succeeds() {
        let temp_dir =
            create_test_repo_with_crates().expect("Failed to create test repo with crates");
        let rule = VolatilityRule::new();
        let mut args = create_test_args(temp_dir.path().to_path_buf());
        args.ci_output = Some(CiOutputFormat::Sarif);
        args.output_file = Some(temp_dir.path().join("output.sarif.json"));

        let result = rule.run(&args);

        assert!(
            result.is_ok(),
            "run with CI output and output file should succeed"
        );

        // Verify the file was created
        assert!(
            temp_dir.path().join("output.sarif.json").exists(),
            "output file should be created"
        );
    }
}
