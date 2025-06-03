use anyhow::Result;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::exit, // Consider if exit is appropriate here or if errors should bubble up
};
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
        let src_dir = &args.src_dir; // src_dir is already a PathBuf in StatementCountArgs

        if !src_dir.exists() {
            eprintln!("Error: src directory not found at {}", src_dir.display());
            // Consider returning an error instead of exiting directly
            // return Err(anyhow::anyhow!("src directory not found"));
            exit(1); // Or handle exit in main based on Result
        }
        if !src_dir.is_dir() {
            eprintln!(
                "Error: --src-dir '{}' is not a directory.",
                src_dir.display()
            );
            exit(1);
        }

        let mut all_rs_files: Vec<PathBuf> = Vec::new();
        collect_all_rs(src_dir, &mut all_rs_files);

        if all_rs_files.is_empty() {
            eprintln!("Error: No `.rs` files found under {}", src_dir.display());
            exit(1);
        }

        // Map each file path (String) → stmt_count (usize)
        let mut file_to_stmt: HashMap<String, usize> = HashMap::new();

        for path_buf in &all_rs_files {
            let content = match fs::read_to_string(path_buf) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error reading {}: {}", path_buf.display(), e);
                    exit(1);
                }
            };

            let ast: SynFile = match syn::parse_file(&content) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Error parsing {}: {}", path_buf.display(), e);
                    exit(1);
                }
            };

            let mut counter = StmtCounter::new();
            counter.visit_file(&ast);

            let key = path_buf.to_string_lossy().into_owned();
            file_to_stmt.insert(key, counter.count);
        }

        if file_to_stmt.is_empty() {
            eprintln!(
                "Error: Did not find any Rust AST statements under {}",
                src_dir.display()
            );
            exit(1);
        }

        // Group by top-level namespace
        // component_name → (file_count, total_statements)
        let mut component_stats: HashMap<String, (usize, usize)> = HashMap::new();

        for path_buf in &all_rs_files {
            let namespace = relative_namespace(path_buf, src_dir);
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
            eprintln!("Error: Total Rust statements = 0. Exiting.");
            exit(1);
        }

        println!("\nStatement Count Report:");
        let any_over_threshold = print_report(&component_stats, grand_total, threshold);

        if any_over_threshold {
            eprintln!(
                "\nError: At least one component exceeds {}% of total statements.",
                threshold
            );
            // Instead of exiting, return an error to be handled by main
            // This allows main to decide on the exit code or further actions.
            return Err(anyhow::anyhow!("Component threshold exceeded."));
        }

        println!(
            "\nAll components are within {}% threshold. (Total statements = {})",
            threshold, grand_total
        );

        Ok(())
    }
}
