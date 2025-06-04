use anyhow::{bail, Context, Result};
use std::{collections::HashMap, fs, path::PathBuf};
use syn::visit::Visit;
use syn::File as SynFile;

use crate::cli::StatementCountArgs;
use crate::counter::StmtCounter; // Assuming counter.rs is at crate::counter
use crate::file_utils::{collect_all_rs, relative_namespace, top_level_component}; // Assuming file_utils.rs is at crate::file_utils
use crate::reporting::print_report; // Assuming reporting.rs is at crate::reporting // Import the specific args struct

/// Rule to count statements in Rust components and check against a threshold.
#[derive(Debug, Default)]
pub struct StatementCountRule;

impl StatementCountRule {
    pub fn new() -> Self {
        StatementCountRule
    }

    pub fn run(&self, args: &StatementCountArgs) -> Result<()> {
        let threshold = args.threshold;
        let analysis_path = &args.path; // Changed from src_dir to path

        if !analysis_path.exists() {
            bail!("Error: Path not found at {}", analysis_path.display());
        }
        if !analysis_path.is_dir() {
            bail!(
                "Error: Provided path '{}' is not a directory.",
                analysis_path.display()
            );
        }

        let mut all_rs_files: Vec<PathBuf> = Vec::new();
        collect_all_rs(analysis_path, &mut all_rs_files).with_context(|| {
            format!(
                "Failed to collect Rust files from {}",
                analysis_path.display()
            )
        })?;

        if all_rs_files.is_empty() {
            bail!(
                "Error: No `.rs` files found under {}",
                analysis_path.display()
            );
        }

        // Map each file path (String) → stmt_count (usize)
        let mut file_to_stmt: HashMap<String, usize> = HashMap::new();

        for path_buf in &all_rs_files {
            let content = fs::read_to_string(path_buf)
                .with_context(|| format!("Error reading file {}", path_buf.display()))?;

            let ast: SynFile = syn::parse_file(&content)
                .with_context(|| format!("Error parsing file {}", path_buf.display()))?;

            let mut counter = StmtCounter::new();
            counter.visit_file(&ast);

            let key = path_buf.to_string_lossy().into_owned();
            file_to_stmt.insert(key, counter.count);
        }

        if file_to_stmt.is_empty() {
            // This case should ideally be covered by all_rs_files.is_empty() if parsing never yields statements
            // but keeping it as a safeguard, though it might indicate an issue with StmtCounter or parsing logic.
            bail!(
                "Error: Did not find any Rust AST statements under {}",
                analysis_path.display()
            );
        }

        // Group by top-level namespace
        // component_name → (file_count, total_statements)
        let mut component_stats: HashMap<String, (usize, usize)> = HashMap::new();

        for path_buf in &all_rs_files {
            let namespace = relative_namespace(path_buf, analysis_path);
            let top = top_level_component(&namespace);

            let path_str = path_buf.to_string_lossy();
            let stmt_count = *file_to_stmt.get(&path_str.into_owned()).unwrap_or(&0);

            let entry = component_stats.entry(top).or_insert((0, 0));
            entry.0 += 1; // increment file count
            entry.1 += stmt_count; // add statement count
        }

        // Sum total statements across all components
        let grand_total: usize = component_stats.values().map(|&(_f, st)| st).sum();

        if grand_total == 0 {
            // This implies .rs files were found, but no statements were counted.
            bail!("Error: Total Rust statements = 0. Ensure .rs files contain statements or check parsing. Path: {}", analysis_path.display());
        }

        println!(
            "\nStatement Count Report (analyzing path: {}):",
            analysis_path.display()
        );
        let any_over_threshold = print_report(&component_stats, grand_total, threshold);

        if any_over_threshold {
            bail!(
                "At least one component exceeds {}% of total statements.",
                threshold
            );
        }

        println!(
            "\nAll components are within {}% threshold. (Total statements = {})",
            threshold, grand_total
        );

        Ok(())
    }
}
