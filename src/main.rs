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

use clap::Parser;
use prettytable::{format, Cell, Row, Table};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{exit, Command, Stdio},
};
use syn::{visit::Visit, File as SynFile, Stmt};
use walkdir::WalkDir;

/// CLI arguments for `rust-ff`.
#[derive(Parser)]
#[command(
    name = "rust-ff",
    about = "Fail build if any Rust component exceeds a given % of total statements"
)]
struct Cli {
    /// Percentage threshold (integer). If any component > this percent, exit nonzero.
    #[arg(long, default_value_t = 10)]
    threshold: usize,

    /// Path to the `src` directory to scan. Defaults to `./src`.
    #[arg(long, default_value = "src")]
    src_dir: PathBuf,
}

/// A simple visitor that counts every `syn::Stmt` node in an AST.
struct StmtCounter {
    count: usize,
}

impl StmtCounter {
    /// Create a new, empty `StmtCounter`.
    fn new() -> Self {
        StmtCounter { count: 0 }
    }
}

impl<'ast> Visit<'ast> for StmtCounter {
    /// Called for each `Stmt` in the AST.
    /// We increment `count` and continue walking nested statements.
    fn visit_stmt(&mut self, node: &'ast Stmt) {
        self.count += 1;
        syn::visit::visit_stmt(self, node);
    }
}

fn main() {
    let cli = Cli::parse();

    if !cli.src_dir.exists() {
        eprintln!(
            "Error: src directory not found at {}",
            cli.src_dir.display()
        );
        exit(1);
    }

    let mut all_rs_files = Vec::new();
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

    for path in &all_rs_files {
        let content = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", path.display(), e);
                exit(1);
            }
        };

        let ast: SynFile = match syn::parse_file(&content) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error parsing {}: {}", path.display(), e);
                exit(1);
            }
        };

        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);

        let key = path.to_string_lossy().into_owned();
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

    for path in &all_rs_files {
        let namespace = relative_namespace(path, &cli.src_dir);
        let top = top_level_component(&namespace);

        let path_str = path.to_string_lossy();
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

    // Print Markdown table header
    // println!("| Component | Percent | Statements | Files |");
    // println!("| --------- | ------: | ---------: | ----: |");

    let mut table = Table::new();
    // table.set_format(*format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR); // Using a common format
    let mut format = format::FormatBuilder::new()
        .column_separator('|')
        .borders('|')
        .separators(
            &[format::LinePosition::Top, format::LinePosition::Bottom],
            format::LineSeparator::new('-', '+', '+', '+'),
        )
        .separator(
            format::LinePosition::Intern,
            format::LineSeparator::new('-', '|', '|', '|'),
        )
        .padding(1, 1)
        .build();
    table.set_format(format);

    table.add_row(Row::new(vec![
        Cell::new("Component"),
        Cell::new("Percent"),
        Cell::new("Statements"),
        Cell::new("Files"),
    ]));

    // Sort components by descending statement count
    let mut sorted: Vec<_> = component_stats.iter().collect();
    sorted.sort_unstable_by_key(|(_, &(_f, st))| std::cmp::Reverse(st));

    let mut any_over_threshold = false;
    for (component, &(files, stmts)) in &sorted {
        let percent = ((stmts as f64 / grand_total as f64) * 100.0).round() as usize;
        if percent > cli.threshold {
            any_over_threshold = true;
        }
        // println!("| {} | {} % | {} | {} |", component, percent, stmts, files);
        table.add_row(Row::new(vec![
            Cell::new(component),
            Cell::new(&format!("{} %", percent)),
            Cell::new(&stmts.to_string()),
            Cell::new(&files.to_string()),
        ]));
    }

    table.printstd(); // Print the table to stdout

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

/// Recursively collect every `.rs` file under `dir` into `out_files`.
///
/// # Parameters
/// - `dir`: the directory (e.g. "./src") to walk.
/// - `out_files`: a `Vec<PathBuf>` to push each discovered `.rs` file into.
fn collect_all_rs(dir: &Path, out_files: &mut Vec<PathBuf>) {
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if let Some(ext) = entry.path().extension() {
            if ext == "rs" {
                out_files.push(entry.into_path());
            }
        }
    }
}

/// Given a full file path (e.g. "/.../mycrate/src/foo/bar.rs") and the `src_dir` (e.g. "src"),
/// return a "namespace" string:
/// 1) Strip the `src_dir` prefix, including the path separator.
/// 2) Drop the file extension `.rs`.
/// 3) If the file name is `mod.rs`, treat the namespace as its parent folder name.
/// 4) Replace path separators `/` or `\` with `::`.
/// 5) Special‐case `lib.rs` and `main.rs` at top level as `"lib"` and `"main"`.
///
/// Examples:
/// - `src/foo/bar.rs`    → `"foo::bar"`
/// - `src/foo/mod.rs`    → `"foo"`
/// - `src/lib.rs`        → `"lib"`
/// - `src/main.rs`       → `"main"`
fn relative_namespace(file_path: &Path, src_dir: &Path) -> String {
    let rel = match file_path.strip_prefix(src_dir) {
        Ok(r) => r,
        Err(_) => file_path,
    };

    let rel_str = rel.to_string_lossy();
    let no_ext = rel_str.trim_end_matches(".rs");

    let mut parts: Vec<&str> = no_ext.split(std::path::MAIN_SEPARATOR).collect();
    if let Some(last) = parts.last() {
        if *last == "mod" && parts.len() > 1 {
            parts.pop();
        }
    }

    if parts.is_empty() {
        return "root".to_owned();
    }

    let joined = parts.join("::");
    if joined == "lib" || joined == "main" {
        return joined;
    }

    joined
}

/// Given a namespace like `"foo::bar::baz"`, return the top‐level component `"foo"`.
/// If there is no `"::"` in the string, return the entire string.
fn top_level_component(namespace: &str) -> String {
    match namespace.split("::").next() {
        Some(first) => first.to_owned(),
        None => namespace.to_owned(),
    }
}
