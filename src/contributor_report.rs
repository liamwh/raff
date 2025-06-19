use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use git2::{Commit, Oid, Repository, Revwalk};
use maud::{html, Markup};
use prettytable::{row, Table};
use serde::Serialize;

use crate::cli::{ContributorReportArgs, ContributorReportOutputFormat};
use crate::html_utils::{self, MetricRanges};

#[derive(Debug, Clone, Serialize)]
struct ContributorStats {
    author: String,
    commit_count: u32,
    lines_added: u32,
    lines_deleted: u32,
    files_touched: u32,
    last_commit_date: DateTime<Utc>,
    score: f64,
}

impl ContributorStats {
    fn new(author: String) -> Self {
        Self {
            author,
            commit_count: 0,
            lines_added: 0,
            lines_deleted: 0,
            files_touched: 0,
            last_commit_date: Utc::now(),
            score: 0.0,
        }
    }
}

pub struct ContributorReportRule;

impl ContributorReportRule {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&self, args: &ContributorReportArgs) -> Result<()> {
        let repo = Repository::open(&args.path)
            .with_context(|| format!("Failed to open repository at {:?}", &args.path))?;
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;

        let mut stats: HashMap<String, ContributorStats> = HashMap::new();
        let now = Utc::now();

        for oid in revwalk {
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            let author = commit.author().name().unwrap_or("Unknown").to_string();

            let mut contributor = stats
                .entry(author.clone())
                .or_insert_with(|| ContributorStats::new(author));

            let commit_time = DateTime::from_timestamp(commit.time().seconds(), 0).unwrap_or(now);
            let days_since_commit = now.signed_duration_since(commit_time).num_days() as f64;
            let weight = (-args.decay * days_since_commit).exp();

            let (lines_added, lines_deleted, files_touched) =
                self.get_commit_stats(&repo, &commit)?;

            contributor.commit_count += 1;
            contributor.lines_added += lines_added;
            contributor.lines_deleted += lines_deleted;
            contributor.files_touched += files_touched;

            let churn = (lines_added + lines_deleted) as f64;
            let commit_score = (1.0 + churn + files_touched as f64) * weight;
            contributor.score += commit_score;

            if commit_time > contributor.last_commit_date {
                contributor.last_commit_date = commit_time;
            }
        }

        let mut sorted_stats: Vec<ContributorStats> = stats.into_values().collect();
        sorted_stats.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        match args.output {
            ContributorReportOutputFormat::Table => self.print_table(&sorted_stats),
            ContributorReportOutputFormat::Html => self.print_html(&sorted_stats),
            ContributorReportOutputFormat::Json => self.print_json(&sorted_stats),
            ContributorReportOutputFormat::Yaml => self.print_yaml(&sorted_stats),
        }
    }

    fn get_commit_stats(&self, repo: &Repository, commit: &Commit) -> Result<(u32, u32, u32)> {
        let parent = commit.parent(0);
        let tree = commit.tree()?;
        let parent_tree = parent.ok().and_then(|p| p.tree().ok());

        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        let diff_stats = diff.stats()?;

        Ok((
            diff_stats.insertions() as u32,
            diff_stats.deletions() as u32,
            diff_stats.files_changed() as u32,
        ))
    }

    fn print_table(&self, stats: &[ContributorStats]) -> Result<()> {
        let mut table = Table::new();
        table.add_row(row![
            "Author",
            "Commit Count",
            "Lines Added",
            "Lines Deleted",
            "Files Touched",
            "Score"
        ]);

        for stat in stats {
            table.add_row(row![
                stat.author,
                stat.commit_count.to_string(),
                stat.lines_added.to_string(),
                stat.lines_deleted.to_string(),
                stat.files_touched.to_string(),
                format!("{:.2}", stat.score)
            ]);
        }

        table.printstd();
        Ok(())
    }

    fn print_json(&self, stats: &[ContributorStats]) -> Result<()> {
        let json = serde_json::to_string_pretty(stats)?;
        println!("{}", json);
        Ok(())
    }

    fn print_yaml(&self, stats: &[ContributorStats]) -> Result<()> {
        let yaml = serde_yaml::to_string(stats)?;
        println!("{}", yaml);
        Ok(())
    }

    fn print_html(&self, stats: &[ContributorStats]) -> Result<()> {
        let report_body = self.generate_report_body(stats);
        let html_content = html_utils::render_html_doc("Contributor Report", report_body);
        let mut file = File::create("contributor-report.html")?;
        file.write_all(html_content.as_bytes())?;
        println!("HTML report generated: contributor-report.html");
        Ok(())
    }

    fn generate_report_body(&self, stats: &[ContributorStats]) -> Markup {
        let commit_counts: Vec<f64> = stats.iter().map(|s| s.commit_count as f64).collect();
        let lines_added: Vec<f64> = stats.iter().map(|s| s.lines_added as f64).collect();
        let lines_deleted: Vec<f64> = stats.iter().map(|s| s.lines_deleted as f64).collect();
        let files_touched: Vec<f64> = stats.iter().map(|s| s.files_touched as f64).collect();
        let scores: Vec<f64> = stats.iter().map(|s| s.score).collect();

        let commit_ranges = MetricRanges::from_values(&commit_counts, true);
        let added_ranges = MetricRanges::from_values(&lines_added, true);
        let deleted_ranges = MetricRanges::from_values(&lines_deleted, true);
        let touched_ranges = MetricRanges::from_values(&files_touched, true);
        let score_ranges = MetricRanges::from_values(&scores, true);

        html! {
            (self.render_explanation())
            table class="sortable-table" {
                thead {
                    tr {
                        th { "Author" }
                        th { "Commit Count" }
                        th { "Lines Added" }
                        th { "Lines Deleted" }
                        th { "Files Touched" }
                        th { "Score" }
                    }
                }
                tbody {
                    @for stat in stats {
                        tr {
                            td { (stat.author) }
                            @if let Some(ref ranges) = commit_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.commit_count as f64, ranges)) { (stat.commit_count) }
                            } @else {
                                td { (stat.commit_count) }
                            }
                            @if let Some(ref ranges) = added_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.lines_added as f64, ranges)) { (stat.lines_added) }
                            } @else {
                                td { (stat.lines_added) }
                            }
                            @if let Some(ref ranges) = deleted_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.lines_deleted as f64, ranges)) { (stat.lines_deleted) }
                            } @else {
                                td { (stat.lines_deleted) }
                            }
                            @if let Some(ref ranges) = touched_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.files_touched as f64, ranges)) { (stat.files_touched) }
                            } @else {
                                td { (stat.files_touched) }
                            }
                            @if let Some(ref ranges) = score_ranges {
                                td style=(html_utils::get_metric_cell_style(stat.score, ranges)) { (format!("{:.2}", stat.score)) }
                            } @else {
                                td { (format!("{:.2}", stat.score)) }
                            }
                        }
                    }
                }
            }
        }
    }

    fn render_explanation(&self) -> Markup {
        let explanations = vec![
            ("Author", "The name of the contributor, as extracted from the Git commit logs."),
            ("Commit Count", "The total number of commits made by the contributor."),
            ("Lines Added", "The total number of lines of code added by the contributor. This metric is weighted positively in the score calculation."),
            ("Lines Deleted", "The total number of lines of code deleted by the contributor. This is considered a positive contribution (e.g., refactoring, removing dead code) and is weighted positively."),
            ("Files Touched", "The total number of unique files modified by the contributor."),
            ("Score", "A calculated metric representing the overall contribution. It is a weighted sum of commits, lines added, lines deleted, and files touched, with an exponential decay factor applied to give more weight to recent contributions. The formula is: `Σ((1 + churn + files_touched) * e^(-decay * days_since_commit))` for each commit."),
        ];
        html_utils::render_metric_explanation_list(&explanations)
    }
}
