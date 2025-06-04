use anyhow::{bail, Context, Result};
use maud::html;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use syn::visit::Visit;
use syn::File as SynFile;

use crate::cli::{StatementCountArgs, StatementCountOutputFormat}; // Import the specific args struct
use crate::counter::StmtCounter; // Assuming counter.rs is at crate::counter
use crate::file_utils::{collect_all_rs, relative_namespace, top_level_component}; // Assuming file_utils.rs is at crate::file_utils
use crate::html_utils; // Now using Maud-based html_utils
use crate::reporting::print_report; // Assuming reporting.rs is at crate::reporting // Import the new HTML utilities

/// Rule to count statements in Rust components and check against a threshold.
#[derive(Debug, Default)]
pub struct StatementCountRule;

impl StatementCountRule {
    pub fn new() -> Self {
        StatementCountRule
    }

    pub fn run(&self, args: &StatementCountArgs) -> Result<()> {
        let threshold = args.threshold;
        let analysis_path = &args.path;

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
            bail!(
                "Error: Did not find any Rust AST statements under {}",
                analysis_path.display()
            );
        }

        let mut component_stats: HashMap<String, (usize, usize)> = HashMap::new();
        for path_buf in &all_rs_files {
            let namespace = relative_namespace(path_buf, analysis_path);
            let top = top_level_component(&namespace);
            let path_str = path_buf.to_string_lossy();
            let stmt_count = *file_to_stmt.get(&path_str.into_owned()).unwrap_or(&0);
            let entry = component_stats.entry(top).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += stmt_count;
        }

        let grand_total: usize = component_stats.values().map(|&(_f, st)| st).sum();
        if grand_total == 0 {
            bail!("Error: Total Rust statements = 0. Ensure .rs files contain statements or check parsing. Path: {}", analysis_path.display());
        }

        match args.output {
            StatementCountOutputFormat::Table => {
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
            }
            StatementCountOutputFormat::Html => {
                let html_output = self.render_statement_count_html_report(
                    &component_stats,
                    grand_total,
                    threshold,
                    analysis_path,
                )?;
                println!("{}", html_output);
                let any_over_threshold =
                    component_stats.values().any(|&(_file_count, st_count)| {
                        if grand_total == 0 {
                            return false;
                        }
                        let percentage = (st_count * 100) / grand_total;
                        percentage > threshold
                    });
                if any_over_threshold {
                    bail!(
                        "At least one component exceeds {}% of total statements (see HTML report for details).",
                        threshold
                    );
                }
            }
        }
        Ok(())
    }

    fn render_statement_count_html_report(
        &self,
        component_stats: &HashMap<String, (usize, usize)>,
        grand_total: usize,
        threshold: usize,
        analysis_path: &Path,
    ) -> Result<String> {
        let title = format!("Statement Count Report: {}", analysis_path.display());

        let explanations_data = [
            ("Component", "Name of the top-level component (e.g., directory under src/, or crate name)."),
            ("File Count", "Number of .rs files within this component."),
            ("Statement Count", "Total number of Rust statements counted in this component."),
            ("Percentage", "This component's statement count as a percentage of the grand total. Cells are colored red if this exceeds the threshold."),
        ];
        let explanations_markup = html_utils::render_metric_explanation_list(&explanations_data);

        let mut sorted_components: Vec<_> = component_stats.iter().collect();
        sorted_components.sort_by_key(|&(name, _)| name.clone());

        let table_markup = html! {
            table class="sortable-table" {
                caption { (format!("Analysis Path: {}. Threshold: {}%", analysis_path.display(), threshold)) }
                thead {
                    tr {
                        th class="sortable-header" data-column-index="0" data-sort-type="string" { "Component" }
                        th class="sortable-header" data-column-index="1" data-sort-type="number" { "File Count" }
                        th class="sortable-header" data-column-index="2" data-sort-type="number" { "Statement Count" }
                        th class="sortable-header" data-column-index="3" data-sort-type="number" { "Percentage" }
                    }
                }
                tbody {
                    @for (name, (file_count, st_count)) in sorted_components {
                        @let percentage = if grand_total > 0 { (*st_count * 100) / grand_total } else { 0 };
                        @let percentage_style = html_utils::get_cell_style(percentage as f64, threshold as f64, threshold as f64, false);
                        tr {
                            td { (name) }
                            td { (file_count) }
                            td { (st_count) }
                            td style=(percentage_style) { (format!("{}%", percentage)) }
                        }
                    }
                }
            }
        };

        let summary_markup = html! {
            p {
                b { "Grand Total Statements: " (grand_total) }
            }
            @let any_over_threshold = component_stats.values().any(|&(_file_count, st_count)| {
                if grand_total == 0 { return false; }
                let percentage = (st_count * 100) / grand_total;
                percentage > threshold
            });
            @if any_over_threshold {
                p style="color: red;" {
                    b { "Warning: At least one component exceeds the " (threshold) "% threshold." }
                }
            } @else {
                p style="color: green;" {
                    "All components are within the " (threshold) "% threshold."
                }
            }
        };

        let body_content = html! {
            (explanations_markup)
            (table_markup)
            (summary_markup)
        };

        Ok(html_utils::render_html_doc(&title, body_content))
    }
}
