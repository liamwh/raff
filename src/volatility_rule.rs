use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc}; // For parsing --since date
use git2::{DiffFormat, DiffOptions, Repository, Sort, TreeWalkMode, TreeWalkResult};
use prettytable::{format, Cell, Row, Table}; // Added for table output
use serde::Serialize; // Added for custom output struct
                      // Ensure serde_json is explicitly imported
use std::collections::{HashMap, HashSet};
use std::fmt::Write; // Added for html_buffer
use std::fs;
use std::io::{BufRead, BufReader}; // For reading files line by line in LoC calculation
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;
// Added import for tracing
use maud::{html, Markup};
use walkdir::WalkDir; // For recursively finding Cargo.toml files // For parsing Cargo.toml

use crate::cli::{VolatilityArgs, VolatilityOutputFormat}; // Ensure VolatilityOutputFormat is imported
use crate::html_utils; // Import the new HTML utilities

/// Represents the statistics gathered for a single crate.
#[derive(Debug, Default, Clone)] // Clone is useful for initialization
pub struct CrateStats {
    /// The root directory path of the crate, relative to the repository root.
    root_path: PathBuf,
    /// Number of commits that touched this crate at least once.
    commit_touch_count: usize,
    /// Total lines inserted into this crate across all relevant commits.
    lines_added: usize,
    /// Total lines removed from this crate across all relevant commits.
    lines_deleted: usize,
    /// Raw volatility score.
    raw_score: f64,
    /// (Optional) Total lines of code, used for normalization.
    total_loc: Option<usize>,
    /// (Optional) Normalized volatility score.
    normalized_score: Option<f64>,
    /// (Optional) Timestamp of the first commit where this crate appeared.
    birth_commit_time: Option<i64>,
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
                .with_context(|| {
                    format!(
                        "Failed to make crate root path '{}' relative to repo root '{}'",
                        crate_root_abs.display(),
                        analysis_path_canonical.display()
                    )
                })?
                .to_path_buf();

            let content = fs::read_to_string(cargo_toml_path_abs).with_context(|| {
                format!(
                    "Failed to read Cargo.toml at {}",
                    cargo_toml_path_abs.display()
                )
            })?;

            let toml_value = content.parse::<TomlValue>().with_context(|| {
                format!(
                    "Failed to parse Cargo.toml at {}",
                    cargo_toml_path_abs.display()
                )
            })?;

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
            return Err(anyhow::anyhow!(
                "No crates (Cargo.toml with [package].name) found under {}. Ensure you are running in a Rust project with crates.",
                analysis_path_canonical.display()
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
            let file = fs::File::open(file_path).with_context(|| {
                format!("Failed to open file for LoC count: {}", file_path.display())
            })?;
            let reader = BufReader::new(file);
            for line_result in reader.lines() {
                let line = line_result.with_context(|| {
                    format!("Failed to read line from file: {}", file_path.display())
                })?;
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
            "- Raw Score: Calculated as 'Touches + (alpha * (Added + Deleted))'. Alpha = {:.4}. A higher score indicates more recent change activity (commits and/or lines changed).",
            alpha
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
                        .map_or_else(|| "N/A".to_string(), |ns| format!("{:.2}", ns)),
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
            let oid = oid_result.with_context(|| "Failed to get OID from birth-time revwalk")?;
            let commit = repo
                .find_commit(oid)
                .with_context(|| "Failed to find commit from OID in birth-time revwalk")?;
            let commit_time = commit.time().seconds();
            let tree = commit
                .tree()
                .with_context(|| "Failed to get tree for commit in birth-time revwalk")?;

            tracing::trace!(commit_oid = %commit.id(), commit_time, "Scanning commit for crate births");

            for (crate_name, stats) in crate_stats_map.iter_mut() {
                if stats.birth_commit_time.is_none() {
                    tree.walk(TreeWalkMode::PreOrder, |path_from_tree_root, entry| {
                        let entry_path_relative_to_repo = Path::new(path_from_tree_root).join(entry.name().unwrap_or_default());

                        if entry_path_relative_to_repo.starts_with(&stats.root_path) {
                            stats.birth_commit_time = Some(commit_time);
                            tracing::debug!(%crate_name, commit_oid = %commit.id(), %commit_time, path_found = %entry_path_relative_to_repo.display(), "Set birth time for crate");
                            crates_needing_birth_time -= 1;
                            return TreeWalkResult::Skip;
                        }
                        TreeWalkResult::Ok
                    }).with_context(|| format!("Error walking tree for commit {} to find birth of crate {}", commit.id(), crate_name))?;
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

    fn render_volatility_html_report(
        &self,
        sorted_crates: &[(&String, &CrateStats)],
        normalize: bool,
        alpha: f64,
        analysis_path_canonical: &Path,
    ) -> Result<String> {
        let title = format!("Volatility Report: {}", analysis_path_canonical.display());

        let mut metric_explanations_data = vec![
            ("Crate Name", "The name of the crate as defined in its Cargo.toml."),
            ("Birth Date", "Approx. date (YYYY-MM-DD) the crate first appeared in history."),
            ("Touches", "Number of commits that modified this crate within the analysis window. Higher is more volatile."),
            ("Added", "Total lines of code added to this crate. Higher is more volatile."),
            ("Deleted", "Total lines of code deleted from this crate. Higher is more volatile."),
        ];
        if normalize {
            metric_explanations_data.push((
                "Total LoC",
                "Total non-blank lines of Rust code in the crate (used for normalization).",
            ));
        }
        let raw_score_explanation_string = format!("Calculated as 'Touches + (alpha * (Added + Deleted))'. Alpha = {:.4}. Higher score indicates more change activity.", alpha);
        metric_explanations_data.push(("Raw Score", &raw_score_explanation_string));
        if normalize {
            metric_explanations_data.push(("Norm Score", "'Raw Score / Total LoC'. Volatility relative to crate size. Higher is more volatile."));
        }
        let explanations_markup =
            html_utils::render_metric_explanation_list(&metric_explanations_data);

        // Prepare MetricRanges for color scaling
        let touches_values: Vec<f64> = sorted_crates
            .iter()
            .map(|(_, s)| s.commit_touch_count as f64)
            .collect();
        let added_values: Vec<f64> = sorted_crates
            .iter()
            .map(|(_, s)| s.lines_added as f64)
            .collect();
        let deleted_values: Vec<f64> = sorted_crates
            .iter()
            .map(|(_, s)| s.lines_deleted as f64)
            .collect();
        let raw_score_values: Vec<f64> = sorted_crates.iter().map(|(_, s)| s.raw_score).collect();
        let norm_score_values: Vec<f64> = sorted_crates
            .iter()
            .filter_map(|(_, s)| s.normalized_score)
            .collect();

        let touches_ranges = html_utils::MetricRanges::from_values(&touches_values, false);
        let added_ranges = html_utils::MetricRanges::from_values(&added_values, false);
        let deleted_ranges = html_utils::MetricRanges::from_values(&deleted_values, false);
        let raw_score_ranges = html_utils::MetricRanges::from_values(&raw_score_values, false);
        let norm_score_ranges = html_utils::MetricRanges::from_values(&norm_score_values, false);

        let table_markup = html! {
            table class="sortable-table" {
                caption { (format!("Volatility Metrics (Alpha: {:.4}, Normalized: {})", alpha, normalize)) }
                thead {
                    tr {
                        th class="sortable-header" data-column-index="0" data-sort-type="string" { "Crate Name" }
                        th class="sortable-header" data-column-index="1" data-sort-type="string" { "Birth Date" }
                        th class="sortable-header" data-column-index="2" data-sort-type="number" { "Touches" }
                        th class="sortable-header" data-column-index="3" data-sort-type="number" { "Added" }
                        th class="sortable-header" data-column-index="4" data-sort-type="number" { "Deleted" }
                        @if normalize {
                            th class="sortable-header" data-column-index="5" data-sort-type="number" { "Total LoC" }
                            th class="sortable-header" data-column-index="6" data-sort-type="number" { "Raw Score" }
                            th class="sortable-header" data-column-index="7" data-sort-type="number" { "Norm Score" }
                        } @else {
                            th class="sortable-header" data-column-index="5" data-sort-type="number" { "Raw Score" }
                        }
                    }
                }
                tbody {
                    @for (name, stats) in sorted_crates {
                        @let birth_date_str = stats.birth_commit_time.map_or_else(
                            || "N/A".to_string(),
                            |ts| DateTime::from_timestamp(ts, 0).map_or_else(
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
                                   { (stats.normalized_score.map_or_else(|| "N/A".to_string(), |ns| format!("{:.2}", ns))) }
                            } @else {
                                td style=({raw_score_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(stats.raw_score, r))}) { (format!("{:.2}", stats.raw_score)) }
                            }
                        }
                    }
                }
            }
        };

        let body_content = html! {
            (explanations_markup)
            (table_markup)
        };

        Ok(html_utils::render_html_doc(&title, body_content))
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    pub fn run(&self, args: &VolatilityArgs) -> Result<()> {
        let analysis_path = &args.path;
        let analysis_path_canonical = analysis_path
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize path: {}", analysis_path.display()))?;
        tracing::info!(path = %analysis_path_canonical.display(), "Running volatility analysis on repository");

        let repo = Repository::open(&analysis_path_canonical).with_context(|| {
            format!(
                "Could not open Git repository at '{}'",
                analysis_path_canonical.display()
            )
        })?;
        tracing::debug!("Successfully opened Git repository.");

        let mut crate_stats_map = self.discover_crates_and_init_stats(&analysis_path_canonical)?;
        self.populate_crate_birth_times(&repo, &mut crate_stats_map)?;

        let since_timestamp = args.since.as_ref().map_or(Ok(0_i64), |date_str| {
            NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .map(|naive_date| {
                    Utc.from_local_date(&naive_date)
                        .unwrap()
                        .and_hms_opt(0, 0, 0)
                        .unwrap()
                        .timestamp()
                })
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Invalid --since date format '{}': {}. Please use YYYY-MM-DD.",
                        date_str,
                        e
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
            let oid = oid_result.with_context(|| "Failed to walk revisions")?;
            let commit = repo
                .find_commit(oid)
                .with_context(|| "Failed to find commit")?;
            let commit_time = commit.time().seconds();

            let parents: Vec<_> = commit.parents().collect();
            if args.skip_merges && parents.len() > 1 {
                tracing::trace!(commit_id = %oid, "Skipping merge commit.");
                continue;
            }

            let tree = commit.tree().with_context(|| "Failed to get commit tree")?;
            let parent_tree_opt = if !parents.is_empty() {
                parents[0].tree().ok()
            } else {
                None
            };

            let mut diff_opts = DiffOptions::new();
            diff_opts.context_lines(0);
            diff_opts.interhunk_lines(0);

            let diff = repo
                .diff_tree_to_tree(parent_tree_opt.as_ref(), Some(&tree), Some(&mut diff_opts))
                .with_context(|| "Failed to compute diff")?;

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
            .with_context(|| "Error processing diff lines")?;

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

        // Sort crates by raw_score (descending) for display
        let mut sorted_crates: Vec<_> = crate_stats_map.iter().collect();
        sorted_crates.sort_by(|a, b| {
            b.1.raw_score
                .partial_cmp(&a.1.raw_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Print output based on format
        match &args.output {
            VolatilityOutputFormat::Table => {
                self.print_volatility_table(&sorted_crates, args.normalize, args.alpha)
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
                        if args.normalize {
                            headers_vec.insert(5, "total_loc");
                            headers_vec.push("normalized_score");
                        }
                        wtr.write_record(&headers_vec)?;
                        for item in output_data {
                            let mut record = vec![
                                item.crate_name.to_string(),
                                item.birth_date.to_string(),
                                item.commit_touch_count.to_string(),
                                item.lines_added.to_string(),
                                item.lines_deleted.to_string(),
                            ];
                            if args.normalize {
                                record.push(
                                    item.total_loc.map_or_else(String::new, |v| v.to_string()),
                                );
                            }
                            record.push(format!("{:.2}", item.raw_score));
                            if args.normalize {
                                record.push(
                                    item.normalized_score
                                        .map_or_else(String::new, |v| format!("{:.2}", v)),
                                );
                            }
                            wtr.write_record(&record)?;
                        }
                        let csv_string = String::from_utf8(wtr.into_inner()?)?;
                        println!("{}", csv_string);
                    }
                    _ => unreachable!(), // Other variants handled by outer match
                }
            }
            VolatilityOutputFormat::Html => {
                let html_output = self.render_volatility_html_report(
                    &sorted_crates,
                    args.normalize,
                    args.alpha,
                    &analysis_path_canonical,
                )?;
                println!("{}", html_output);
            }
        }
        Ok(())
    }
}
