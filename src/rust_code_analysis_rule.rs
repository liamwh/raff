use crate::error::{RaffError, Result};
use prettytable::{format as pt_format, Attr, Cell, Row, Table};
use serde::{Deserialize, Serialize};
// use std::fmt::Write; // No longer needed for HTML buffer
use maud::html;
use std::path::{Path, PathBuf};
use tracing::instrument;

use crate::cli::{RustCodeAnalysisArgs, RustCodeAnalysisOutputFormat};
use crate::html_utils;

// --- Structs for deserializing rust-code-analysis-cli JSON output ---

#[derive(Deserialize, Serialize, Debug)]
pub struct LocMetrics {
    pub sloc: f64,
    pub ploc: f64,
    pub lloc: f64,
    pub cloc: f64,
    pub blank: f64,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct CyclomaticMetrics {
    pub sum: f64,
    pub average: f64,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct HalsteadMetrics {
    pub n1: f64,
    pub n2: f64,
    pub length: f64, // Was N before, ensure it's `length` for parsing if output changed
    pub vocabulary: f64,
    pub volume: f64,
    pub difficulty: f64,
    pub effort: f64,
    pub time: f64,
    pub bugs: f64,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ItemMetrics {
    // For simplicity, focusing on loc and cyclomatic for now. Add others as needed.
    pub loc: Option<LocMetrics>,
    pub cyclomatic: Option<CyclomaticMetrics>,
    pub halstead: Option<HalsteadMetrics>,
    // Other metrics like 'nom', 'mi', 'abc', 'cognitive' can be added here
}

#[derive(Deserialize, Serialize, Debug)]
pub struct CodeSpace {
    pub name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub metrics: ItemMetrics,
    pub spaces: Vec<CodeSpace>, // For nested items
}

#[derive(Deserialize, Serialize, Debug)]
pub struct AnalysisUnit {
    pub name: String, // Full path to the file
    pub kind: String, // "unit" for files
    pub spaces: Vec<CodeSpace>,
    pub metrics: Option<ItemMetrics>, // Top-level metrics for the file/unit itself
}

// --- Aggregated Metrics Structure ---
#[derive(Default, Debug, Serialize)]
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

#[derive(Serialize, Debug)]
pub struct RustCodeAnalysisData {
    pub analysis_results: Vec<AnalysisUnit>,
    pub analysis_path: PathBuf,
}

impl Default for RustCodeAnalysisRule {
    fn default() -> Self {
        Self::new()
    }
}

impl RustCodeAnalysisRule {
    pub fn new() -> Self {
        Self
    }

    #[instrument(skip(self, args), fields(output = ?args.output))]
    pub fn run(&self, args: &RustCodeAnalysisArgs) -> Result<()> {
        let data = self.analyze(args)?;

        if data.analysis_results.is_empty() {
            tracing::info!("Analysis produced no results.");
            println!("No analysis data produced.");
            return Ok(());
        }

        match args.output {
            RustCodeAnalysisOutputFormat::Table => {
                print_analysis_table(&data.analysis_results, &data.analysis_path)?;
            }
            RustCodeAnalysisOutputFormat::Json => {
                let json_output = serde_json::to_string_pretty(&data.analysis_results)?;
                println!("{json_output}");
            }
            RustCodeAnalysisOutputFormat::Yaml => {
                let yaml_output = serde_yaml::to_string(&data.analysis_results)?;
                println!("{yaml_output}");
            }
            RustCodeAnalysisOutputFormat::Html => {
                let body = self.render_rust_code_analysis_html_body(
                    &data.analysis_results,
                    &data.analysis_path,
                )?;
                let title = format!(
                    "Rust Code Analysis Report: {}",
                    data.analysis_path.display()
                );
                let full_html = html_utils::render_html_doc(&title, body);
                println!("{full_html}");
            }
        }

        Ok(())
    }

    #[instrument(skip(self, args))]
    pub fn analyze(&self, args: &RustCodeAnalysisArgs) -> Result<RustCodeAnalysisData> {
        let analysis_path = PathBuf::from(&args.path);

        tracing::info!(
            "Starting file discovery in: {} for language {}",
            analysis_path.display(),
            args.language
        );

        let file_path_args = discover_and_filter_files(&analysis_path, &args.language)?;

        if file_path_args.is_empty() {
            println!(
                "No files matching language '{:?}' found in path '{:?}'.",
                args.language, args.path
            );
            return Ok(RustCodeAnalysisData {
                analysis_results: vec![],
                analysis_path,
            });
        }

        tracing::debug!("Discovered files for CLI: {:?}", file_path_args);

        let mut cmd_args = Vec::new();
        cmd_args.extend(file_path_args);

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
                RaffError::analysis_error(
                    "rust_code_analysis",
                    "rust-code-analysis-cli not found. Please ensure it is installed and in your PATH."
                )
            } else {
                RaffError::analysis_error(
                    "rust_code_analysis",
                    format!("Failed to execute rust-code-analysis-cli: {}", e)
                )
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!("rust-code-analysis-cli failed. Stderr:\n{}", stderr);
            return Err(RaffError::analysis_error(
                "rust_code_analysis",
                format!(
                    "rust-code-analysis-cli exited with error code {}:\n{}",
                    output.status, stderr
                ),
            ));
        }

        let stdout_str = String::from_utf8_lossy(&output.stdout);
        tracing::debug!("rust-code-analysis-cli stdout length: {}", stdout_str.len());

        if stdout_str.trim().is_empty() {
            tracing::info!("rust-code-analysis-cli produced no output.");
            return Ok(RustCodeAnalysisData {
                analysis_results: vec![],
                analysis_path,
            });
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

        Ok(RustCodeAnalysisData {
            analysis_results,
            analysis_path,
        })
    }

    pub fn render_rust_code_analysis_html_body(
        &self,
        analysis_results: &[AnalysisUnit],
        project_root: &Path,
    ) -> Result<maud::Markup> {
        let _title = format!("Rust Code Analysis Report: {}", project_root.display());

        let explanations_data = [
            (
                "File",
                "Path to the analyzed file, relative to the project root.",
            ),
            (
                "SLOC",
                "Source Lines of Code. Higher may indicate a larger file.",
            ),
            (
                "PLOC",
                "Physical Lines of Code. Higher may indicate a larger file.",
            ),
            (
                "LLOC",
                "Logical Lines of Code. Higher may indicate more complex logic.",
            ),
            ("CLOC", "Comment Lines of Code."),
            ("Blank", "Blank Lines."),
            (
                "Cyc Sum",
                "Cyclomatic Complexity Sum. Higher is more complex (worse).",
            ),
            (
                "Cyc Avg",
                "Cyclomatic Complexity Average. Higher is more complex (worse).",
            ),
            (
                "H Len",
                "Halstead Length. Higher indicates more operators/operands.",
            ),
            (
                "H Vocab",
                "Halstead Vocabulary. Higher indicates more unique operators/operands.",
            ),
            (
                "H Vol",
                "Halstead Volume. Higher indicates larger program size (worse).",
            ),
            (
                "H Effort",
                "Halstead Effort. Higher indicates more mental effort (worse).",
            ),
            (
                "H Time",
                "Halstead Time (sec). Higher indicates longer development time (worse).",
            ),
            (
                "H Bugs",
                "Halstead Bugs. Higher indicates more potential bugs (worse).",
            ),
        ];
        let explanations_markup = html_utils::render_metric_explanation_list(&explanations_data);

        let mut aggregated_metrics_list: Vec<(String, FileAggregatedMetrics)> = Vec::new();
        for unit in analysis_results {
            let full_path = PathBuf::from(&unit.name);
            let path_for_display = full_path
                .strip_prefix(project_root)
                .map_or_else(|_| full_path.clone(), |p| p.to_path_buf());
            let relative_path_str = path_for_display.display().to_string();
            let mut aggregated_metrics = FileAggregatedMetrics::default();
            aggregate_metrics_recursive(&unit.spaces, &mut aggregated_metrics);
            aggregated_metrics_list.push((relative_path_str, aggregated_metrics));
        }

        let sloc_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.sloc)
                .collect::<Vec<f64>>(),
            false,
        );
        let ploc_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.ploc)
                .collect::<Vec<f64>>(),
            false,
        );
        let lloc_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.lloc)
                .collect::<Vec<f64>>(),
            false,
        );
        let cyc_sum_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.cyclomatic_sum)
                .collect::<Vec<f64>>(),
            false,
        );
        let cyc_avg_values: Vec<f64> = aggregated_metrics_list
            .iter()
            .map(|(_, m)| {
                if m.items_with_metrics > 0 {
                    m.cyclomatic_sum / m.items_with_metrics as f64
                } else {
                    0.0
                }
            })
            .collect();
        let cyc_avg_ranges = html_utils::MetricRanges::from_values(&cyc_avg_values, false);
        let h_len_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.halstead_length)
                .collect::<Vec<f64>>(),
            false,
        );
        let h_vocab_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.halstead_vocabulary)
                .collect::<Vec<f64>>(),
            false,
        );
        let h_vol_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.halstead_volume)
                .collect::<Vec<f64>>(),
            false,
        );
        let h_effort_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.halstead_effort)
                .collect::<Vec<f64>>(),
            false,
        );
        let h_time_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.halstead_time)
                .collect::<Vec<f64>>(),
            false,
        );
        let h_bugs_ranges = html_utils::MetricRanges::from_values(
            &aggregated_metrics_list
                .iter()
                .map(|(_, m)| m.halstead_bugs)
                .collect::<Vec<f64>>(),
            false,
        );

        let table_markup = html! {
            table class="sortable-table" {
                caption { "Detailed Metrics per File" }
                thead {
                    tr {
                        th class="sortable-header" data-column-index="0" data-sort-type="string" { "File" }
                        th class="sortable-header" data-column-index="1" data-sort-type="number" { "SLOC" }
                        th class="sortable-header" data-column-index="2" data-sort-type="number" { "PLOC" }
                        th class="sortable-header" data-column-index="3" data-sort-type="number" { "LLOC" }
                        th class="sortable-header" data-column-index="4" data-sort-type="number" { "CLOC" }
                        th class="sortable-header" data-column-index="5" data-sort-type="number" { "Blank" }
                        th class="sortable-header" data-column-index="6" data-sort-type="number" { "Cyc Sum" }
                        th class="sortable-header" data-column-index="7" data-sort-type="number" { "Cyc Avg" }
                        th class="sortable-header" data-column-index="8" data-sort-type="number" { "H Len" }
                        th class="sortable-header" data-column-index="9" data-sort-type="number" { "H Vocab" }
                        th class="sortable-header" data-column-index="10" data-sort-type="number" { "H Vol" }
                        th class="sortable-header" data-column-index="11" data-sort-type="number" { "H Effort" }
                        th class="sortable-header" data-column-index="12" data-sort-type="number" { "H Time" }
                        th class="sortable-header" data-column-index="13" data-sort-type="number" { "H Bugs" }
                    }
                }
                tbody {
                    @for (relative_path_str, metrics) in &aggregated_metrics_list {
                        @let cyclomatic_avg = if metrics.items_with_metrics > 0 { metrics.cyclomatic_sum / metrics.items_with_metrics as f64 } else { 0.0 };
                        tr {
                            td { (relative_path_str) }
                            td style=({sloc_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.sloc, r))}) { (format!("{:.0}", metrics.sloc)) }
                            td style=({ploc_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.ploc, r))}) { (format!("{:.0}", metrics.ploc)) }
                            td style=({lloc_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.lloc, r))}) { (format!("{:.0}", metrics.lloc)) }
                            td { (format!("{:.0}", metrics.cloc)) } // CLOC - no color scale
                            td { (format!("{:.0}", metrics.blank)) } // Blank - no color scale
                            td style=({cyc_sum_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.cyclomatic_sum, r))}) { (format!("{:.0}", metrics.cyclomatic_sum)) }
                            td style=({cyc_avg_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(cyclomatic_avg, r))}) { (format!("{:.1}", cyclomatic_avg)) }
                            td style=({h_len_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.halstead_length, r))}) { (format!("{:.0}", metrics.halstead_length)) }
                            td style=({h_vocab_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.halstead_vocabulary, r))}) { (format!("{:.0}", metrics.halstead_vocabulary)) }
                            td style=({h_vol_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.halstead_volume, r))}) { (format!("{:.1}", metrics.halstead_volume)) }
                            td style=({h_effort_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.halstead_effort, r))}) { (format!("{:.0}", metrics.halstead_effort)) }
                            td style=({h_time_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.halstead_time, r))}) { (format!("{:.1}", metrics.halstead_time)) }
                            td style=({h_bugs_ranges.as_ref().map_or_else(String::new, |r| html_utils::get_metric_cell_style(metrics.halstead_bugs, r))}) { (format!("{:.2}", metrics.halstead_bugs)) }
                        }
                    }
                }
            }
        };

        let body_content = html! {
            (explanations_markup)
            (table_markup)
        };

        Ok(body_content)
    }
}

// --- Updated Table Printing Logic ---
fn print_analysis_table(analysis_results: &[AnalysisUnit], project_root: &Path) -> Result<()> {
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
    table.set_format(*pt_format::consts::FORMAT_BOX_CHARS);
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
            .strip_prefix(project_root)
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
            Cell::new(&format!("{cyclomatic_avg:.1}")),
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

fn get_extension_for_language(language: &str) -> Option<&str> {
    match language {
        "rust" => Some(".rs"),
        // Future extensions for other languages can be added here.
        _ => None,
    }
}

#[instrument]
fn discover_and_filter_files(root_dir: &PathBuf, language: &str) -> Result<Vec<String>> {
    if !root_dir.exists() {
        return Err(RaffError::invalid_input(format!(
            "Root path not found: {}",
            root_dir.display()
        )));
    }

    // If a specific file extension is known for the language, walk the directory and find all matching files.
    if let Some(extension) = get_extension_for_language(language) {
        let mut discovered_files = Vec::new();
        let walker = walkdir::WalkDir::new(root_dir).into_iter();

        for entry_result in walker.filter_entry(|e| {
            if e.file_type().is_dir() {
                let file_name = e.file_name().to_string_lossy();
                if file_name == "target"
                    || file_name == "frontend"
                    || file_name == "node_modules"
                    || file_name == ".git"
                {
                    return false; // Skip these directories
                }
            }
            true
        }) {
            match entry_result {
                Ok(entry) => {
                    if entry.file_type().is_file() {
                        if let Some(path_str) = entry.path().to_str() {
                            if path_str.ends_with(extension) {
                                discovered_files.push(path_str.to_string());
                            }
                        }
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

        let mut file_path_args = Vec::new();
        for file in discovered_files {
            file_path_args.push("-p".to_string());
            file_path_args.push(file);
        }
        Ok(file_path_args)
    } else {
        // Fallback for languages without a specified extension: pass the path directly to the CLI tool.
        let path_str = root_dir.to_str().ok_or_else(|| {
            RaffError::invalid_input(format!("Path contains invalid Unicode: {:?}", root_dir))
        })?;
        Ok(vec!["-p".to_string(), path_str.to_string()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_discover_and_filter_files_rust() {
        // Setup a temporary directory with a structure for testing
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create files and directories
        fs::write(root.join("file1.rs"), b"fn main() {}").unwrap();
        fs::write(root.join("another.txt"), b"some text").unwrap();

        let src_dir = root.join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("file2.rs"), b"// rust code").unwrap();

        let nested_dir = src_dir.join("nested");
        fs::create_dir(&nested_dir).unwrap();
        fs::write(nested_dir.join("file3.rs"), b"// more rust").unwrap();

        // Create directories and files that should be ignored
        let target_dir = root.join("target");
        fs::create_dir(&target_dir).unwrap();
        fs::write(target_dir.join("ignored.rs"), b"").unwrap();

        let node_modules_dir = root.join("node_modules");
        fs::create_dir(&node_modules_dir).unwrap();
        fs::write(node_modules_dir.join("ignored.rs"), b"").unwrap();

        // Run the discovery function
        let result = discover_and_filter_files(&root.to_path_buf(), "rust").unwrap();

        // The result should be a Vec of strings like ["-p", "/path/to/file1.rs", "-p", "/path/to/file2.rs", ...]
        // So we expect 3 files, which means 6 elements in the vec.
        assert_eq!(result.len(), 6, "Should find 3 Rust files");

        // Convert to a HashSet for easier comparison, ignoring the order and the "-p" flags.
        let result_files: std::collections::HashSet<String> =
            result.chunks(2).map(|chunk| chunk[1].clone()).collect();

        let expected_files: std::collections::HashSet<String> = [
            root.join("file1.rs"),
            src_dir.join("file2.rs"),
            nested_dir.join("file3.rs"),
        ]
        .iter()
        .map(|p| p.to_str().unwrap().to_string())
        .collect();

        assert_eq!(result_files, expected_files);
    }

    #[test]
    fn test_no_matching_files_found() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("file1.txt"), b"hello").unwrap();
        fs::write(root.join("file2.js"), b"world").unwrap();

        let result = discover_and_filter_files(&root.to_path_buf(), "rust").unwrap();
        assert!(
            result.is_empty(),
            "Should return an empty vec when no .rs files are found"
        );
    }

    #[test]
    fn test_unsupported_language_fallback() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap().to_string();

        let result = discover_and_filter_files(&root.to_path_buf(), "javascript").unwrap();

        assert_eq!(
            result,
            vec!["-p".to_string(), root_str],
            "Should fall back to passing the root directory for unsupported languages"
        );
    }

    #[test]
    fn test_non_existent_path() {
        let path = PathBuf::from("/non/existent/path/that/should/not/be/real");
        let result = discover_and_filter_files(&path, "rust");
        assert!(
            result.is_err(),
            "Should return an error for a non-existent path"
        );
    }
}
