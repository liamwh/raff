use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc}; // For parsing --since date
use git2::{DiffFormat, DiffOptions, Repository, Sort, TreeWalkMode, TreeWalkResult};
use prettytable::{format, Cell, Row, Table}; // Added for table output
use serde::Serialize; // Added for custom output struct
                      // Ensure serde_json is explicitly imported
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader}; // For reading files line by line in LoC calculation
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;
// Added import for tracing
use walkdir::WalkDir; // For recursively finding Cargo.toml files // For parsing Cargo.toml

use crate::cli::VolatilityArgs; // Import the specific args struct

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
    /// * `repo_path` - The root path of the repository to scan.
    ///
    /// # Returns
    /// A `Result` containing a map from crate name to its initialized `CrateStats`,
    /// or an error if discovery or parsing fails.
    fn discover_crates_and_init_stats(&self, repo_canonical_path: &Path) -> Result<CrateStatsMap> {
        let mut crate_stats_map = CrateStatsMap::new();

        tracing::debug!("Discovering crates by finding Cargo.toml files...");

        for entry in WalkDir::new(repo_canonical_path)
            .into_iter()
            .filter_map(|e| e.ok()) // Filter out errors during walk
            .filter(|e| e.file_name().to_string_lossy() == "Cargo.toml")
        {
            let cargo_toml_path_abs = entry.path();
            let crate_root_abs = match cargo_toml_path_abs.parent() {
                Some(p) => p.to_path_buf(),
                None => {
                    // This case should be rare (Cargo.toml at the root of filesystem?)
                    tracing::warn!(
                        path = %cargo_toml_path_abs.display(),
                        "Cargo.toml found with no parent directory, skipping."
                    );
                    continue;
                }
            };

            // Make crate_root relative to the repository root
            let crate_root_relative = crate_root_abs
                .strip_prefix(repo_canonical_path)
                .with_context(|| {
                    format!(
                        "Failed to make crate root path '{}' relative to repo root '{}'",
                        crate_root_abs.display(),
                        repo_canonical_path.display()
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
                    // This could happen in workspaces with path dependencies if not careful,
                    // or if a crate name is duplicated. For now, we warn and overwrite,
                    // assuming the first one found at a shallower depth (if WalkDir provides that order)
                    // or the last one encountered is taken.
                    // A more robust solution might involve checking paths.
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
                        raw_score: 0.0,          // Initialize new field
                        total_loc: None,         // Initialize new field
                        normalized_score: None,  // Initialize new field
                        birth_commit_time: None, // Initialize new field
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
                repo_canonical_path.display()
            ));
        }

        tracing::info!(
            count = crate_stats_map.len(),
            "Found crate(s). Initialized stats with relative paths."
        );
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
        // Returns (crate_name, crate_root_path)
        let mut longest_match: Option<(String, PathBuf)> = None;
        let mut max_depth = 0;

        for (name, stats) in crate_stats_map {
            // stats.root_path is now relative to repo root.
            // file_path_in_repo is also relative to repo root.
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
        repo_canonical_path: &Path,
    ) -> Result<usize> {
        let crate_abs_path = repo_canonical_path.join(crate_relative_path);
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
            .column_separator('│')
            .borders('│')
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
            let oid = oid_result.context("Failed to get OID from birth-time revwalk")?;
            let commit = repo
                .find_commit(oid)
                .context("Failed to find commit from OID in birth-time revwalk")?;
            let commit_time = commit.time().seconds();
            let tree = commit
                .tree()
                .context("Failed to get tree for commit in birth-time revwalk")?;

            tracing::trace!(commit_oid = %commit.id(), commit_time, "Scanning commit for crate births");

            for (crate_name, stats) in crate_stats_map.iter_mut() {
                if stats.birth_commit_time.is_none() {
                    let mut found_in_this_commit = false;
                    tree.walk(TreeWalkMode::PreOrder, |path_from_tree_root, entry| {
                        let entry_path_relative_to_repo = Path::new(path_from_tree_root).join(entry.name().unwrap_or_default());

                        if entry_path_relative_to_repo.starts_with(&stats.root_path) {
                            found_in_this_commit = true;
                            stats.birth_commit_time = Some(commit_time);
                            tracing::debug!(%crate_name, commit_oid = %commit.id(), %commit_time, path_found = %entry_path_relative_to_repo.display(), "Set birth time for crate");
                            crates_needing_birth_time -= 1;
                            return TreeWalkResult::Skip;
                        }
                        TreeWalkResult::Ok
                    }).context(format!("Error walking tree for commit {} to find birth of crate {}", commit.id(), crate_name))?;

                    // No specific action needed here if found_in_this_commit is true,
                    // as birth_commit_time is set and crates_needing_birth_time is decremented inside closure.
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

    #[tracing::instrument(level = "debug", skip_all, err)]
    pub fn run(&self, args: &VolatilityArgs) -> Result<()> {
        let alpha = args.alpha;
        let since_date_str_opt = args.since.as_ref(); // Option<&String> from Option<String>
        let normalize = args.normalize;
        let skip_merges = args.skip_merges;
        let repo_path_arg = &args.repo_path;
        let output_format = &args.output;

        tracing::debug!(
            repo_path = %repo_path_arg.display(),
            alpha,
            since = since_date_str_opt,
            normalize,
            skip_merges,
            output_format,
            "Starting Volatility Rule execution with options"
        );

        if !repo_path_arg.is_dir() {
            return Err(anyhow::anyhow!(
                "Repository path '{}' is not a valid directory.",
                repo_path_arg.display()
            ));
        }
        let canonical_repo_path = repo_path_arg.canonicalize().with_context(|| {
            format!(
                "Failed to get canonical path for repository: {}",
                repo_path_arg.display()
            )
        })?;
        tracing::debug!(path = %canonical_repo_path.display(), "Canonical repository path resolved.");
        if !canonical_repo_path.join(".git").exists() {
            return Err(anyhow::anyhow!(
                "The specified repository path '{}' does not appear to be a Git repository (missing .git directory).",
                canonical_repo_path.display()
            ));
        }

        let mut crate_stats_map = self.discover_crates_and_init_stats(&canonical_repo_path)?;
        if crate_stats_map.is_empty() {
            tracing::info!("No crates found. Exiting.");
            return Ok(());
        }

        let repo = Repository::open(&canonical_repo_path).with_context(|| {
            format!(
                "Failed to open Git repository at {}",
                canonical_repo_path.display()
            )
        })?;
        tracing::debug!(path = ?repo.path(), "Successfully opened Git repository.");

        // Populate crate birth times
        self.populate_crate_birth_times(&repo, &mut crate_stats_map)
            .context("Failed to populate crate birth times")?;

        tracing::debug!("Setting up revision walk (revwalk).");
        let head_oid = repo.head()?.peel_to_commit()?.id();
        tracing::debug!(head_oid = %head_oid, "Resolved HEAD OID.");
        let mut revwalk = repo.revwalk()?;
        revwalk.push(head_oid)?;
        let since_timestamp_opt: Option<i64> = since_date_str_opt
            .map(|date_str| {
                NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Invalid --since date format '{}': {}. Please use YYYY-MM-DD.",
                            date_str,
                            e
                        )
                    })
                    .and_then(|naive_date| {
                        let naive_datetime = naive_date.and_hms_opt(0, 0, 0).ok_or_else(|| {
                            anyhow::anyhow!(
                                "Invalid date '{}': could not convert to NaiveDateTime at 00:00:00",
                                date_str
                            )
                        })?;
                        Ok(Utc.from_utc_datetime(&naive_datetime))
                    })
                    .map(|datetime| datetime.timestamp())
            })
            .transpose()?;
        if let Some(ts) = since_timestamp_opt {
            tracing::debug!(since_timestamp = ts, "Applying date cutoff to revwalk.");
        }
        tracing::info!("Iterating through commits for volatility analysis...");
        let mut processed_commit_count = 0;
        for oid_result in revwalk {
            let oid = oid_result.context("Failed to get OID from revwalk")?;
            let commit = repo
                .find_commit(oid)
                .context("Failed to find commit from OID")?;
            let commit_time = commit.time().seconds();

            if let Some(cutoff_ts) = since_timestamp_opt {
                if commit_time < cutoff_ts {
                    tracing::debug!(commit_oid = %commit.id(), %commit_time, cutoff_time = cutoff_ts, "Commit is older than --since cutoff. Stopping revwalk.");
                    break;
                }
            }
            if skip_merges && commit.parent_count() > 1 {
                tracing::trace!(commit_oid = %commit.id(), parent_count = commit.parent_count(), "Skipping merge commit.");
                continue;
            }
            tracing::debug!(commit_oid = %commit.id(), summary = %commit.summary().unwrap_or_default().trim(), "Processing commit.");
            let current_tree = commit
                .tree()
                .context("Failed to get tree for current commit")?;
            let parent_tree_opt = if commit.parent_count() == 0 {
                None
            } else {
                Some(
                    commit
                        .parent(0)?
                        .tree()
                        .context("Failed to get tree for parent commit (0)")?,
                )
            };
            let mut diff_opts = DiffOptions::new();
            diff_opts.context_lines(0);
            diff_opts.interhunk_lines(0);
            let diff = repo
                .diff_tree_to_tree(
                    parent_tree_opt.as_ref(),
                    Some(&current_tree),
                    Some(&mut diff_opts),
                )
                .context("Failed to compute diff between commit and its parent")?;

            let mut crates_touched_this_commit_for_stats = HashSet::new();
            diff.deltas().for_each(|delta| {
                let file_path = delta.new_file().path().or_else(|| delta.old_file().path());
                if let Some(p) = file_path {
                    if let Some((crate_name, _)) = self.find_owning_crate(p, &crate_stats_map) {
                        // Check against birth_commit_time AND global --since before adding to set
                        if let Some(stats) = crate_stats_map.get(&crate_name) {
                            let effective_since_timestamp = stats.birth_commit_time.map_or_else(
                                || since_timestamp_opt, // If no birth time, use global since
                                |bt| Some(since_timestamp_opt.map_or(bt, |st| bt.max(st))), // Use max(birth_time, global_since)
                            );
                            if effective_since_timestamp
                                .is_none_or(|eff_since| commit_time >= eff_since)
                            {
                                crates_touched_this_commit_for_stats.insert(crate_name.clone());
                            }
                        }
                    }
                }
            });

            diff.print(DiffFormat::Patch, |_delta_cb, _hunk_cb, line_cb| {
                let current_file_path = _delta_cb
                    .new_file()
                    .path()
                    .or_else(|| _delta_cb.old_file().path());
                if let Some(p) = current_file_path {
                    if let Some((crate_name, _)) = self.find_owning_crate(p, &crate_stats_map) {
                        if let Some(stats) = crate_stats_map.get_mut(&crate_name) {
                            // Check against birth_commit_time AND global --since before updating line stats
                            let effective_since_timestamp = stats.birth_commit_time.map_or_else(
                                || since_timestamp_opt, // If no birth time, use global since
                                |bt| Some(since_timestamp_opt.map_or(bt, |st| bt.max(st))), // Use max(birth_time, global_since)
                            );

                            if effective_since_timestamp
                                .is_none_or(|eff_since| commit_time >= eff_since)
                            {
                                match line_cb.origin() {
                                    '+' | '>' => stats.lines_added += 1,
                                    '-' | '<' => stats.lines_deleted += 1,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                true
            })?;

            for crate_name in &crates_touched_this_commit_for_stats {
                if let Some(stats) = crate_stats_map.get_mut(crate_name) {
                    stats.commit_touch_count += 1;
                }
            }
            processed_commit_count += 1;
        }
        tracing::info!(
            count = processed_commit_count,
            "Processed commits for volatility."
        );

        tracing::info!("Calculating volatility scores...");
        for (name, stats) in crate_stats_map.iter_mut() {
            let churn = stats.lines_added + stats.lines_deleted;
            stats.raw_score = stats.commit_touch_count as f64 + alpha * churn as f64;
            tracing::debug!(
                crate_name = name,
                raw_score = stats.raw_score,
                touches = stats.commit_touch_count,
                churn = churn,
                "Calculated raw score"
            );
            if normalize {
                match self.calculate_loc_for_crate(&stats.root_path, &canonical_repo_path) {
                    Ok(loc) => {
                        stats.total_loc = Some(loc);
                        if loc > 0 {
                            stats.normalized_score = Some(stats.raw_score / loc as f64);
                            tracing::debug!(
                                crate_name = name,
                                loc = loc,
                                normalized_score = stats.normalized_score.unwrap_or_default(),
                                "Calculated normalized score"
                            );
                        } else {
                            stats.normalized_score = Some(stats.raw_score);
                            tracing::warn!(crate_name = name, loc = loc, "Crate has 0 LoC. Normalized score set to raw score to avoid division by zero.");
                        }
                    }
                    Err(e) => {
                        tracing::error!(crate_name = name, error = %e, "Failed to calculate LoC for crate. Skipping normalization for this crate.");
                    }
                }
            }
        }

        tracing::debug!("Sorting crates by volatility score...");
        let mut sorted_crates: Vec<(&String, &CrateStats)> = crate_stats_map.iter().collect();
        if normalize {
            sorted_crates.sort_by(|a, b| {
                let score_a = a.1.normalized_score;
                let score_b = b.1.normalized_score;
                score_b
                    .partial_cmp(&score_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        } else {
            sorted_crates.sort_by(|a, b| {
                b.1.raw_score
                    .partial_cmp(&a.1.raw_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // Conditional output based on format
        if output_format == "csv" {
            println!("\nVolatility Report (CSV)");
            println!("# Interpretation:");
            println!("# - Volatility: Higher scores indicate more frequent or larger changes.");
            println!("# - Crate Name: The name of the crate as defined in its Cargo.toml.");
            println!("# - Birth Date: Approx. date (YYYY-MM-DD) the crate first appeared in history. Calculated from commit timestamp.");
            println!("# - Commit Touch Count: Number of commits that modified this crate within the analysis window (since crate birth or global --since, whichever is later).");
            println!(
                "# - Lines Added: Total lines of code added to this crate during that period."
            );
            println!("# - Lines Deleted: Total lines of code deleted from this crate during that period.");
            if normalize {
                println!("# - Total LoC: Total non-blank lines of Rust code in the crate (used for normalization).");
            }
            println!("# - Raw Score: Calculated as 'Commit Touch Count + (alpha * (Lines Added + Lines Deleted))'. Alpha = {:.4}. A higher score indicates more recent change activity (commits and/or lines changed).", alpha);
            if normalize {
                println!("# - Normalized Score: 'Raw Score / Total LoC'. Shows volatility relative to crate size. May be N/A if LoC is 0 or not calculated. A higher score indicates more change activity relative to the crate's size.");
            }

            if normalize {
                println!("crate_name,birth_date,commit_touch_count,lines_added,lines_deleted,total_loc,raw_score,normalized_score");
            } else {
                println!(
                    "crate_name,birth_date,commit_touch_count,lines_added,lines_deleted,raw_score"
                );
            }
            for (name, stats) in &sorted_crates {
                let birth_date_str = stats.birth_commit_time.map_or_else(
                    || "N/A".to_string(),
                    |ts| {
                        DateTime::from_timestamp(ts, 0).map_or_else(
                            || "N/A".to_string(), // Fallback for CSV if timestamp is invalid
                            |dt| dt.format("%Y-%m-%d").to_string(),
                        )
                    },
                );
                if normalize {
                    println!(
                        "{},{},{},{},{},{},{:.4},{:.4}",
                        name,
                        birth_date_str,
                        stats.commit_touch_count,
                        stats.lines_added,
                        stats.lines_deleted,
                        stats
                            .total_loc
                            .map_or_else(|| "N/A".to_string(), |loc| loc.to_string()),
                        stats.raw_score,
                        stats
                            .normalized_score
                            .map_or_else(|| "N/A".to_string(), |ns| format!("{:.4}", ns))
                    );
                } else {
                    println!(
                        "{},{},{},{},{},{:.4}",
                        name,
                        birth_date_str,
                        stats.commit_touch_count,
                        stats.lines_added,
                        stats.lines_deleted,
                        stats.raw_score
                    );
                }
            }
        } else if output_format == "json" || output_format == "yaml" {
            let output_data: Vec<CrateVolatilityDataForOutput> = sorted_crates
                .iter()
                .map(|(name, stats)| {
                    let birth_date_str = stats.birth_commit_time.map_or_else(
                        || "N/A".to_string(),
                        |ts| {
                            DateTime::from_timestamp(ts, 0).map_or_else(
                                || "Invalid Date".to_string(),
                                |dt| dt.format("%Y-%m-%d").to_string(),
                            )
                        },
                    );
                    CrateVolatilityDataForOutput {
                        crate_name: name,
                        birth_date: birth_date_str,
                        commit_touch_count: stats.commit_touch_count,
                        lines_added: stats.lines_added,
                        lines_deleted: stats.lines_deleted,
                        total_loc: if normalize { stats.total_loc } else { None },
                        raw_score: stats.raw_score,
                        normalized_score: if normalize {
                            stats.normalized_score
                        } else {
                            None
                        },
                    }
                })
                .collect();

            if output_format == "json" {
                let json_output = serde_json::to_string_pretty(&output_data)
                    .context("Failed to serialize data to JSON")?;
                println!("{}", json_output);
            } else {
                // output_format == "yaml"
                let yaml_output = serde_yaml::to_string(&output_data)
                    .context("Failed to serialize data to YAML")?;
                println!("{}", yaml_output);
            }
        } else {
            // Default to table output
            self.print_volatility_table(&sorted_crates, normalize, alpha);
        }

        tracing::info!("Volatility analysis complete. Report generated.");

        Ok(())
    }
}
