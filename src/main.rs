//! rust-ff: Compute Rust‐statement counts per top‐level component without using `tokei`.
//!
//! This program does the following:
//! 1. Walks the `src/` directory recursively to find all `*.rs` files.
//! 2. Parses each file with `syn::parse_file`.
//! 3. Visits the AST to count every `syn::Stmt` node (Rust statement).
//! 4. Computes a "namespace" string for each file (e.g. `src/foo/bar.rs` → `foo::bar`, `src/lib.rs` → `lib`).
//! 5. Groups files by top‐level namespace (e.g. `foo::*`, `bar::*`, `lib`, `main`).
//! 6. Sums file counts and statement counts per component, computes percentages, and prints a Markdown table.
//! 7. Exits with code 1 if any component exceeds `--threshold` percent (default 10 %) of total statements.
//!
//! USAGE EXAMPLE (run from your crate root):
//!   cargo run --release -- --threshold 10
//!
//! By default, this expects to be run where `./src` exists. You can override `--src-dir` if needed.

mod cli;
mod counter;
mod file_utils;
mod reporting;

use clap::Parser;
use std::{collections::HashMap, fs, path::PathBuf, process::exit};
use syn::visit::Visit;
use syn::File as SynFile;

use cli::Cli;
use counter::StmtCounter;
use file_utils::{collect_all_rs, relative_namespace, top_level_component};
use reporting::print_report;

fn main() {
    let cli = Cli::parse();

    if !cli.src_dir.exists() {
        eprintln!(
            "Error: src directory not found at {}",
            cli.src_dir.display()
        );
        exit(1);
    }

    let mut all_rs_files: Vec<PathBuf> = Vec::new();
    collect_all_rs(&cli.src_dir, &mut all_rs_files);

    if all_rs_files.is_empty() {
        eprintln!(
            "Error: No `.rs` files found under {}",
            cli.src_dir.display()
        );
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
            cli.src_dir.display()
        );
        exit(1);
    }

    // Group by top-level namespace
    // component_name → (file_count, total_statements)
    let mut component_stats: HashMap<String, (usize, usize)> = HashMap::new();

    for path_buf in &all_rs_files {
        let namespace = relative_namespace(path_buf, &cli.src_dir);
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

    let any_over_threshold = print_report(&component_stats, grand_total, cli.threshold);

    if any_over_threshold {
        eprintln!(
            "Error: At least one component exceeds {}% of total statements.",
            cli.threshold
        );
        exit(1);
    }

    println!(
        "All components are within {}% threshold. (Total statements = {})",
        cli.threshold, grand_total
    );
}
