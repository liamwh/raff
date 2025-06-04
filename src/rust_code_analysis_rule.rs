use anyhow::{Context, Result};
use prettytable::{format, Attr, Cell, Row, Table};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::instrument;

use crate::cli::{RustCodeAnalysisArgs, RustCodeAnalysisOutputFormat};

// --- Structs for deserializing rust-code-analysis-cli JSON output ---

#[derive(Deserialize, Serialize, Debug)]
struct LocMetrics {
    sloc: f64,
    ploc: f64,
    lloc: f64,
    cloc: f64,
    blank: f64,
}

#[derive(Deserialize, Serialize, Debug)]
struct CyclomaticMetrics {
    sum: f64,
    average: f64,
}

#[derive(Deserialize, Serialize, Debug)]
struct HalsteadMetrics {
    n1: f64,
    n2: f64,
    length: f64,
    vocabulary: f64,
    volume: f64,
    difficulty: f64,
    effort: f64,
    time: f64,
    bugs: f64,
}

#[derive(Deserialize, Serialize, Debug)]
struct ItemMetrics {
    // For simplicity, focusing on loc and cyclomatic for now. Add others as needed.
    loc: Option<LocMetrics>,
    cyclomatic: Option<CyclomaticMetrics>,
    halstead: Option<HalsteadMetrics>,
    // Other metrics like 'nom', 'mi', 'abc', 'cognitive' can be added here
}

#[derive(Deserialize, Serialize, Debug)]
struct CodeSpace {
    name: String,
    kind: String,
    start_line: usize,
    end_line: usize,
    metrics: ItemMetrics,
    spaces: Vec<CodeSpace>, // For nested items
}

#[derive(Deserialize, Serialize, Debug)]
struct AnalysisUnit {
    name: String, // Full path to the file
    kind: String, // "unit" for files
    spaces: Vec<CodeSpace>,
    metrics: Option<ItemMetrics>, // Top-level metrics for the file/unit itself
}

// --- Aggregated Metrics Structure ---
#[derive(Default, Debug)]
struct FileAggregatedMetrics {
    sloc: f64,
    ploc: f64,
    lloc: f64,
    cloc: f64,
    blank: f64,
    cyclomatic_sum: f64,
    halstead_length: f64,
    halstead_vocabulary: f64, // Sum of vocabularies from sub-items
    halstead_volume: f64,
    halstead_effort: f64,
    halstead_time: f64,
    halstead_bugs: f64,
    items_with_metrics: usize, // Count of spaces (e.g. functions) that contributed metrics
}

// --- Recursive Aggregation Logic ---
fn aggregate_metrics_recursive(spaces: &[CodeSpace], acc: &mut FileAggregatedMetrics) {
    for space in spaces {
        let mut has_metrics_for_this_space = false;
        if let Some(loc) = &space.metrics.loc {
            acc.sloc += loc.sloc;
            acc.ploc += loc.ploc;
            acc.lloc += loc.lloc;
            acc.cloc += loc.cloc;
            acc.blank += loc.blank;
            if loc.sloc > 0.0 {
                has_metrics_for_this_space = true;
            }
        }
        if let Some(cyclo) = &space.metrics.cyclomatic {
            acc.cyclomatic_sum += cyclo.sum;
            // We count an item if it has any cyclomatic sum, typically functions/methods
            if cyclo.sum > 0.0 {
                has_metrics_for_this_space = true;
            }
        }
        if let Some(halstead) = &space.metrics.halstead {
            acc.halstead_length += halstead.length;
            acc.halstead_vocabulary += halstead.vocabulary;
            acc.halstead_volume += halstead.volume;
            acc.halstead_effort += halstead.effort;
            acc.halstead_time += halstead.time;
            acc.halstead_bugs += halstead.bugs;
            if halstead.length > 0.0 {
                has_metrics_for_this_space = true;
            }
        }

        // Heuristic: if a space of kind function/method/closure provided any non-zero primary metric, count it.
        // Or, more simply, if it had a metrics block that wasn't entirely default/empty.
        // For now, `has_metrics_for_this_space` based on positive primary metric values.
        if has_metrics_for_this_space
            && (space.kind == "function"
                || space.kind == "method"
                || space.kind == "closure"
                || space.kind == "associated_function")
        {
            acc.items_with_metrics += 1;
        }

        // Recursive call for nested spaces
        if !space.spaces.is_empty() {
            aggregate_metrics_recursive(&space.spaces, acc);
        }
    }
}

// --- Rule implementation ---

#[derive(Debug)]
pub struct RustCodeAnalysisRule;

impl RustCodeAnalysisRule {
    pub fn new() -> Self {
        Self
    }

    #[instrument(skip(self, args), fields(output = ?args.output))]
    pub fn run(&self, args: &RustCodeAnalysisArgs) -> Result<()> {
        let analysis_path = &args.path;

        tracing::info!(
            "Starting directory discovery in: {}",
            analysis_path.display()
        );
        let src_paths_args = discover_src_directories(analysis_path, args).with_context(|| {
            format!(
                "Failed to discover source directories in {}",
                analysis_path.display()
            )
        })?;

        tracing::debug!("Discovered src paths for CLI: {:?}", src_paths_args);

        let mut cmd_args = Vec::new();
        cmd_args.extend(src_paths_args);

        cmd_args.push("-l".to_string());
        cmd_args.push(args.language.clone());

        if args.metrics {
            cmd_args.push("-m".to_string());
        }

        cmd_args.push("-O".to_string());
        cmd_args.push("json".to_string());

        cmd_args.push("-j".to_string());
        cmd_args.push(args.jobs.to_string());

        cmd_args.extend(args.extra_flags.clone());

        tracing::info!(
            "Assembled arguments for rust-code-analysis-cli: {:?}",
            cmd_args
        );

        let mut command = std::process::Command::new("rust-code-analysis-cli");
        command.args(&cmd_args);

        tracing::info!("Executing command: {:?}", command);

        let output = command.output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "rust-code-analysis-cli not found. Please ensure it is installed and in your PATH."
                )
            } else {
                anyhow::anyhow!("Failed to execute rust-code-analysis-cli: {}", e)
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!("rust-code-analysis-cli failed. Stderr:\n{}", stderr);
            return Err(anyhow::anyhow!(
                "rust-code-analysis-cli exited with error code {}:\n{}",
                output.status,
                stderr
            ));
        }

        let stdout_str = String::from_utf8_lossy(&output.stdout);
        tracing::debug!("rust-code-analysis-cli stdout length: {}", stdout_str.len());

        if stdout_str.trim().is_empty() {
            tracing::info!("rust-code-analysis-cli produced no output.");
            println!("No analysis data produced.");
            return Ok(());
        }

        let mut analysis_results: Vec<AnalysisUnit> = Vec::new();
        for line in stdout_str.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<AnalysisUnit>(line) {
                Ok(unit) => analysis_results.push(unit),
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse a line of JSON output: {}. Line: '{}'",
                        e,
                        line
                    );
                }
            }
        }

        if analysis_results.is_empty() {
            tracing::info!("Successfully parsed rust-code-analysis-cli output, but it contained no valid analysis units.");
            println!("No parsable analysis data found.");
            return Ok(());
        }

        match args.output {
            RustCodeAnalysisOutputFormat::Table => {
                print_analysis_table(&analysis_results, analysis_path)?;
            }
            RustCodeAnalysisOutputFormat::Json => {
                let pretty_json = serde_json::to_string_pretty(&analysis_results)
                    .context("Failed to serialize analysis results to JSON")?;
                println!("{}", pretty_json);
            }
            RustCodeAnalysisOutputFormat::Yaml => {
                let yaml_output = serde_yaml::to_string(&analysis_results)
                    .context("Failed to serialize analysis results to YAML")?;
                println!("{}", yaml_output);
            }
        }

        Ok(())
    }
}

// --- Updated Table Printing Logic ---
fn print_analysis_table(analysis_results: &[AnalysisUnit], project_root: &PathBuf) -> Result<()> {
    if analysis_results.is_empty() {
        println!("No analysis data to display in table.");
        return Ok(());
    }

    // Print metric explanations
    println!("Metric Explanations:");
    println!("--------------------");
    println!("File       : Path to the analyzed file, relative to the project root.");
    println!("SLOC       : Source Lines of Code - Non-comment, non-blank lines.");
    println!("PLOC       : Physical Lines of Code - Total lines including comments and blanks (summed from functions).");
    println!("LLOC       : Logical Lines of Code - Number of executable statements (summed from functions).");
    println!("CLOC       : Comment Lines of Code - Lines containing only comments (summed from functions).");
    println!(
        "Blank      : Blank Lines - Lines containing only whitespace (summed from functions)."
    );
    println!(
        "Cyc Sum    : Cyclomatic Complexity Sum - Total complexity paths in all functions/methods."
    );
    println!(
        "Cyc Avg    : Cyclomatic Complexity Average - Average complexity per function/method."
    );
    println!("H Len      : Halstead Length - Total number of operators and operands (summed).");
    println!("H Vocab    : Halstead Vocabulary - Sum of unique operators/operands in functions.");
    println!(
        "H Vol      : Halstead Volume - Program size based on Length and Vocabulary (summed)."
    );
    println!(
        "H Effort   : Halstead Effort - Estimated mental effort to develop/understand (summed)."
    );
    println!("H Time     : Halstead Time (sec) - Estimated time to develop/understand (summed).");
    println!("H Bugs     : Halstead Bugs - Estimated number of delivered bugs (summed).");
    println!("\n"); // Add a newline before the table

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_BOX_CHARS);
    table.add_row(Row::new(vec![
        Cell::new("File").with_style(Attr::Bold),
        Cell::new("SLOC").with_style(Attr::Bold),
        Cell::new("PLOC").with_style(Attr::Bold),
        Cell::new("LLOC").with_style(Attr::Bold),
        Cell::new("CLOC").with_style(Attr::Bold),
        Cell::new("Blank").with_style(Attr::Bold),
        Cell::new("Cyc Sum").with_style(Attr::Bold),
        Cell::new("Cyc Avg").with_style(Attr::Bold),
        Cell::new("H Len").with_style(Attr::Bold),
        Cell::new("H Vocab").with_style(Attr::Bold),
        Cell::new("H Vol").with_style(Attr::Bold),
        Cell::new("H Effort").with_style(Attr::Bold),
        Cell::new("H Time").with_style(Attr::Bold),
        Cell::new("H Bugs").with_style(Attr::Bold),
    ]));

    for unit in analysis_results {
        let full_path = PathBuf::from(&unit.name);
        let path_for_display = full_path
            .strip_prefix(project_root.as_path())
            .map(|stripped_ref| stripped_ref.to_path_buf())
            .unwrap_or_else(|_err| full_path.clone());
        let relative_path_str = path_for_display.display().to_string();

        let mut aggregated_metrics = FileAggregatedMetrics::default();
        aggregate_metrics_recursive(&unit.spaces, &mut aggregated_metrics);

        let cyclomatic_avg = if aggregated_metrics.items_with_metrics > 0 {
            aggregated_metrics.cyclomatic_sum / aggregated_metrics.items_with_metrics as f64
        } else {
            0.0
        };

        table.add_row(Row::new(vec![
            Cell::new(&relative_path_str),
            Cell::new(&format!("{:.0}", aggregated_metrics.sloc)),
            Cell::new(&format!("{:.0}", aggregated_metrics.ploc)),
            Cell::new(&format!("{:.0}", aggregated_metrics.lloc)),
            Cell::new(&format!("{:.0}", aggregated_metrics.cloc)),
            Cell::new(&format!("{:.0}", aggregated_metrics.blank)),
            Cell::new(&format!("{:.0}", aggregated_metrics.cyclomatic_sum)),
            Cell::new(&format!("{:.1}", cyclomatic_avg)),
            Cell::new(&format!("{:.0}", aggregated_metrics.halstead_length)),
            Cell::new(&format!("{:.0}", aggregated_metrics.halstead_vocabulary)),
            Cell::new(&format!("{:.1}", aggregated_metrics.halstead_volume)),
            Cell::new(&format!("{:.0}", aggregated_metrics.halstead_effort)),
            Cell::new(&format!("{:.1}", aggregated_metrics.halstead_time)),
            Cell::new(&format!("{:.2}", aggregated_metrics.halstead_bugs)),
        ]));
    }

    table.printstd();
    Ok(())
}

#[instrument(skip(_args))]
fn discover_src_directories(
    root_dir: &PathBuf,
    _args: &RustCodeAnalysisArgs,
) -> Result<Vec<String>> {
    let mut src_paths = Vec::new();
    if !root_dir.exists() {
        return Err(anyhow::anyhow!(
            "Root directory not found: {}",
            root_dir.display()
        ));
    }
    if !root_dir.is_dir() {
        return Err(anyhow::anyhow!(
            "Root path is not a directory: {}",
            root_dir.display()
        ));
    }

    let walker = walkdir::WalkDir::new(root_dir).into_iter();

    for entry_result in walker.filter_entry(|e| {
        let path = e.path();
        if path.to_str().is_none() {
            tracing::warn!("Skipping path with invalid Unicode: {:?}", path);
            return false;
        }
        let file_name = path.file_name().unwrap_or_default();

        if e.file_type().is_dir() {
            if file_name == "target" || file_name == "frontend" {
                return false;
            }
        }
        true
    }) {
        match entry_result {
            Ok(entry) => {
                if entry.file_type().is_dir() && entry.file_name() == "src" {
                    let path_str = entry.path().to_str().ok_or_else(|| {
                        anyhow::anyhow!("Path contains invalid Unicode: {:?}", entry.path())
                    })?;
                    src_paths.push("-p".to_string());
                    src_paths.push(path_str.to_string());
                    tracing::debug!("Found 'src' directory: {}", path_str);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Error accessing entry in {}: {}. Skipping.",
                    root_dir.display(),
                    e
                );
            }
        }
    }

    if src_paths.is_empty() {
        return Err(anyhow::anyhow!(
            "No 'src' directories found (excluding 'target' and 'frontend') in {}",
            root_dir.display()
        ));
    }

    Ok(src_paths)
}
