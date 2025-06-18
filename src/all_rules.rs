use anyhow::Result;
use maud::Markup;
use serde::Serialize;

use crate::{
    cli::{AllArgs, AllOutputFormat},
    coupling_rule::{CouplingData, CouplingRule},
    html_utils,
    rust_code_analysis_rule::{RustCodeAnalysisData, RustCodeAnalysisRule},
    statement_count_rule::{StatementCountData, StatementCountRule},
    volatility_rule::{VolatilityData, VolatilityRule},
};

#[derive(Debug)]
pub struct AllReportData {
    statement_count: Option<Result<StatementCountData>>,
    volatility: Option<Result<VolatilityData>>,
    coupling: Option<Result<CouplingData>>,
    rust_code_analysis: Option<Result<RustCodeAnalysisData>>,
}

#[derive(Debug, Serialize)]
struct JsonReportData<'a> {
    statement_count: Option<&'a StatementCountData>,
    volatility: Option<&'a VolatilityData>,
    coupling: Option<&'a CouplingData>,
    rust_code_analysis: Option<&'a RustCodeAnalysisData>,
    errors: Vec<String>,
}

pub fn run_all(args: &AllArgs) -> Result<()> {
    let sc_rule = StatementCountRule::new();
    let vol_rule = VolatilityRule::new();
    let coup_rule = CouplingRule::new();
    let rca_rule = RustCodeAnalysisRule::new();

    // We need to construct the specific args for each rule from the AllArgs
    let sc_args = crate::cli::StatementCountArgs {
        path: args.path.clone(),
        threshold: args.sc_threshold,
        output: crate::cli::StatementCountOutputFormat::Table, // format is irrelevant for analyze
    };
    let vol_args = crate::cli::VolatilityArgs {
        path: args.path.clone(),
        alpha: args.vol_alpha,
        since: args.vol_since.clone(),
        normalize: args.vol_normalize,
        skip_merges: args.vol_skip_merges,
        output: crate::cli::VolatilityOutputFormat::Table, // format is irrelevant for analyze
    };
    let coup_args = crate::cli::CouplingArgs {
        path: args.path.clone(),
        granularity: args.coup_granularity.clone(),
        output: crate::cli::CouplingOutputFormat::Table, // format is irrelevant for analyze
    };
    let rca_args = crate::cli::RustCodeAnalysisArgs {
        path: args.path.clone(),
        extra_flags: args.rca_extra_flags.clone(),
        jobs: args.rca_jobs,
        metrics: args.rca_metrics,
        language: args.rca_language.clone(),
        output: crate::cli::RustCodeAnalysisOutputFormat::Table, // format is irrelevant for analyze
    };

    let all_data = AllReportData {
        statement_count: Some(sc_rule.analyze(&sc_args)),
        volatility: Some(vol_rule.analyze(&vol_args)),
        coupling: Some(coup_rule.analyze(&coup_args)),
        rust_code_analysis: Some(rca_rule.analyze(&rca_args)),
    };

    match args.output {
        AllOutputFormat::Json => {
            let mut errors = Vec::new();
            if let Some(Err(e)) = &all_data.statement_count {
                errors.push(format!("Statement Count Error: {}", e));
            }
            if let Some(Err(e)) = &all_data.volatility {
                errors.push(format!("Volatility Error: {}", e));
            }
            if let Some(Err(e)) = &all_data.coupling {
                errors.push(format!("Coupling Error: {}", e));
            }
            if let Some(Err(e)) = &all_data.rust_code_analysis {
                errors.push(format!("Rust Code Analysis Error: {}", e));
            }

            let json_report = JsonReportData {
                statement_count: all_data
                    .statement_count
                    .as_ref()
                    .and_then(|r| r.as_ref().ok()),
                volatility: all_data.volatility.as_ref().and_then(|r| r.as_ref().ok()),
                coupling: all_data.coupling.as_ref().and_then(|r| r.as_ref().ok()),
                rust_code_analysis: all_data
                    .rust_code_analysis
                    .as_ref()
                    .and_then(|r| r.as_ref().ok()),
                errors,
            };

            let json = serde_json::to_string_pretty(&json_report)?;
            println!("{}", json);
        }
        AllOutputFormat::Html => {
            let mut html_body_parts: Vec<Markup> = vec![];

            if let Some(Ok(data)) = &all_data.statement_count {
                html_body_parts.push(sc_rule.render_statement_count_html_body(data)?);
            }
            if let Some(Ok(data)) = &all_data.volatility {
                let mut sorted_crates: Vec<_> = data.crate_stats_map.iter().collect();
                sorted_crates.sort_by(|a, b| {
                    b.1.raw_score
                        .partial_cmp(&a.1.raw_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                html_body_parts.push(vol_rule.render_volatility_html_body(
                    &sorted_crates,
                    data.normalize,
                    data.alpha,
                )?);
            }
            if let Some(Ok(data)) = &all_data.coupling {
                html_body_parts.push(coup_rule.render_coupling_html_body(data)?);
            }
            if let Some(Ok(data)) = &all_data.rust_code_analysis {
                html_body_parts.push(rca_rule.render_rust_code_analysis_html_body(
                    &data.analysis_results,
                    &data.analysis_path,
                )?);
            }

            let full_html = html_utils::render_html_doc(
                "Consolidated Analysis Report",
                maud::html! { @for part in &html_body_parts { (part) } },
            );
            println!("{}", full_html);
        }
    }

    Ok(())
}
