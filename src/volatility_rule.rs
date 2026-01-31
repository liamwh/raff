use chrono::{DateTime, NaiveDate, TimeZone, Utc}; // For parsing --since date
use git2::{DiffOptions, Repository, Sort, TreeWalkMode, TreeWalkResult};
use maud::{html, Markup};
use prettytable::{format, Cell, Row, Table}; // Added for table output
use serde::{Deserialize, Serialize}; // Added for custom output struct
                                     // Ensure serde_json is explicitly imported
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader}; // For reading files line by line in LoC calculation
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;
// Added import for tracing
use walkdir::WalkDir; // For recursively finding Cargo.toml files // For parsing Cargo.toml

use crate::cli::{VolatilityArgs, VolatilityOutputFormat}; // Ensure VolatilityOutputFormat is imported
use crate::error::{RaffError, Result};
use crate::html_utils; // Import the new HTML utilities

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
    /// (Optional) Total lines of code, used for normalization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_loc: Option<usize>,
    /// (Optional) Normalized volatility score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_score: Option<f64>,
    /// (Optional) Timestamp of the first commit where this crate appeared.
    #[serde(skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Serialize)]
pub struct VolatilityData {
    pub crate_stats_map: CrateStatsMap,
    pub normalize: bool,
    pub alpha: f64,
    pub analysis_path: PathBuf,
}

impl VolatilityRule {
    pub fn new() -> Self {
        VolatilityRule
    }

    /// Step 2 & 3: Identify crates and initialize their statistics.
    /// Scans the given repository path for `Cargo.toml` files, extracts crate names,
    /// and initializes their statistics.
    ///
    /// # Arguments
    /// * `analysis_path_canonical` - The root path of the repository to scan.
    ///
    /// # Returns
    /// A `Result` containing a map from crate name to its initialized `CrateStats`,
    /// or an error if discovery or parsing fails.
    fn discover_crates_and_init_stats(
        &self,
        analysis_path_canonical: &Path,
    ) -> Result<CrateStatsMap> {
        let mut crate_stats_map = CrateStatsMap::new();
        tracing::debug!(
            "Discovering crates by finding Cargo.toml files in {}",
            analysis_path_canonical.display()
        );

        for entry in WalkDir::new(analysis_path_canonical)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy() == "Cargo.toml")
        {
            let cargo_toml_path_abs = entry.path();
            let crate_root_abs = match cargo_toml_path_abs.parent() {
                Some(p) => p.to_path_buf(),
                None => {
                    tracing::warn!(
                        path = %cargo_toml_path_abs.display(),
                        "Cargo.toml found with no parent directory, skipping."
                    );
                    continue;
                }
            };

            let crate_root_relative = crate_root_abs
                .strip_prefix(analysis_path_canonical)
                .map_err(|e| {
                    RaffError::parse_error_with_file(
                        crate_root_abs.clone(),
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

        if crate_stats_map.is_empty() {
            return Err(RaffError::analysis_error(
                "volatility",
                format!(
                    "No crates (Cargo.toml with [package].name) found under {}. Ensure you are running in a Rust project with crates.",
                    analysis_path_canonical.display()
                ),
            ));
        }
        Ok(crate_stats_map)
    }

    /// Finds the owning crate for a given file path.
    /// The owning crate is the one whose root_path is the longest prefix of the file_path.
    /// Paths are expected to be canonicalized or consistently relative to the repo root.
    fn find_owning_crate(
        &self,
        file_path_in_repo: &Path,
        crate_stats_map: &CrateStatsMap,
    ) -> Option<(String, PathBuf)> {
        let mut longest_match: Option<(String, PathBuf)> = None;
        let mut max_depth = 0;

        for (name, stats) in crate_stats_map {
            if file_path_in_repo.starts_with(&stats.root_path) {
                let depth = stats.root_path.components().count();
                if depth > max_depth {
                    max_depth = depth;
                    longest_match = Some((name.clone(), stats.root_path.clone()));
                }
            }
        }
        longest_match
    }

    /// Calculates the lines of code (LoC) for a given crate directory.
    /// Only considers `.rs` files and counts non-blank lines.
    #[tracing::instrument(level = "debug", skip(self), fields(crate_relative_path = %crate_relative_path.display()))]
    fn calculate_loc_for_crate(
        &self,
        crate_relative_path: &Path,
        analysis_path_canonical: &Path,
    ) -> Result<usize> {
        let crate_abs_path = analysis_path_canonical.join(crate_relative_path);
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
            println!("- Total LoC: Total non-blank lines of Rust code in the crate (used for normalization).");
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
            let birth_date_str = stats.birth_commit_time.map_or_else(
                || "N/A".to_string(),
                |ts| {
                    DateTime::from_timestamp(ts, 0).map_or_else(
                        || "Invalid Date".to_string(),
                        |dt| dt.format("%Y-%m-%d").to_string(),
                    )
                },
            );

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
            ("Crate Name", "The name of the crate as defined in its Cargo.toml."),
            ("Birth Date", "The date of the first commit where this crate's Cargo.toml appeared. 'N/A' if not found in history."),
            ("Commit Touches", "The number of commits (since the specified date) that modified any file within this crate."),
            ("Lines Added", "Total number of lines added to .rs files in this crate."),
            ("Lines Deleted", "Total number of lines deleted from .rs files in this crate."),
            ("Raw Score", "A combined metric calculated as: (Lines Added + Lines Deleted) + α * (Commit Touches). A higher score indicates higher churn/activity."),
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
                caption { (format!("Volatility calculated with α (touch weight) = {}", alpha)) }
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
                        @let birth_date_str = stats.birth_commit_time.map_or_else(
                            || "N/A".to_string(),
                            |dt| Utc.timestamp_opt(dt, 0).single().map_or_else(
                                || "Invalid Date".to_string(),
                                |dt| dt.format("%Y-%m-%d").to_string()
                            )
                        );
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
    pub fn run(&self, args: &VolatilityArgs) -> Result<()> {
        let data = self.analyze(args)?;

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
                        birth_date: stats.birth_commit_time.map_or_else(
                            || "N/A".to_string(),
                            |ts| {
                                DateTime::from_timestamp(ts, 0)
                                    .unwrap_or_default()
                                    .format("%Y-%m-%d")
                                    .to_string()
                            },
                        ),
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
    pub fn analyze(&self, args: &VolatilityArgs) -> Result<VolatilityData> {
        let analysis_path = &args.path;
        let analysis_path_canonical = analysis_path.canonicalize()?;
        tracing::info!(path = %analysis_path_canonical.display(), "Running volatility analysis on repository");

        let repo = Repository::open(&analysis_path_canonical).map_err(|e| {
            RaffError::git_error_with_repo(
                format!("open Git repository: {}", e),
                analysis_path_canonical.clone(),
            )
        })?;
        tracing::debug!("Successfully opened Git repository.");

        let mut crate_stats_map = self.discover_crates_and_init_stats(&analysis_path_canonical)?;
        self.populate_crate_birth_times(&repo, &mut crate_stats_map)?;

        let since_timestamp = args.since.as_ref().map_or(Ok(0_i64), |date_str| {
            NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .map(|naive_date| {
                    let naive_datetime = naive_date.and_hms_opt(0, 0, 0).expect(
                        "Internal error: Failed to create NaiveDateTime from NaiveDate at midnight",
                    );
                    Utc.from_local_datetime(&naive_datetime)
                        .single()
                        .expect("Internal error: Failed to convert NaiveDateTime to DateTime<Utc>")
                        .timestamp()
                })
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

            let diff = repo.diff_tree_to_tree(
                parent_tree_opt.as_ref(),
                Some(&tree),
                Some(&mut diff_opts),
            )?;

            if commit_time < since_timestamp {
                tracing::trace!(commit_id = %oid, commit_date = %DateTime::from_timestamp(commit_time, 0).unwrap().format("%Y-%m-%d"), "Commit is older than --since date, skipping for volatility calculation (but was considered for birth date).");
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

        for (name, stats) in crate_stats_map.iter_mut() {
            if args.normalize {
                match self.calculate_loc_for_crate(&stats.root_path, &analysis_path_canonical) {
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
            stats.raw_score = (stats.lines_added + stats.lines_deleted) as f64
                + args.alpha * stats.commit_touch_count as f64;
            if let Some(loc) = stats.total_loc {
                if loc > 0 {
                    stats.normalized_score = Some(stats.raw_score / loc as f64);
                }
            }
        }

        Ok(VolatilityData {
            crate_stats_map,
            normalize: args.normalize,
            alpha: args.alpha,
            analysis_path: analysis_path_canonical,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{VolatilityArgs, VolatilityOutputFormat};
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
    fn test_discover_crates_fails_with_no_crates() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create directory without Cargo.toml
        let empty_dir = temp_dir.path().join("empty");
        fs::create_dir_all(&empty_dir).expect("Failed to create empty directory");

        let rule = VolatilityRule::new();
        let result = rule.discover_crates_and_init_stats(&empty_dir);

        assert!(
            result.is_err(),
            "discover_crates_and_init_stats should fail when no crates found"
        );
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("No crates") || error_msg.contains("Cargo.toml"),
            "error message should mention no crates found"
        );
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
        args.alpha = 1.0; // Use alpha = 1.0 for simpler calculation

        let result = rule.analyze(&args);

        assert!(result.is_ok(), "analyze should succeed");

        let data = result.unwrap();
        for stats in data.crate_stats_map.values() {
            // raw_score = (lines_added + lines_deleted) + alpha * commit_touch_count
            let expected_raw_score = (stats.lines_added + stats.lines_deleted) as f64
                + args.alpha * stats.commit_touch_count as f64;
            assert!(
                (stats.raw_score - expected_raw_score).abs() < 0.01,
                "raw_score should be calculated correctly: expected {}, got {}",
                expected_raw_score,
                stats.raw_score
            );
        }
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
}
