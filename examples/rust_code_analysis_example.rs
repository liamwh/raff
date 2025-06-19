use raff_core::{RustCodeAnalysisArgs, RustCodeAnalysisOutputFormat, RustCodeAnalysisRule};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args = RustCodeAnalysisArgs {
        path: PathBuf::from("."),
        extra_flags: vec![],
        jobs: num_cpus::get(),
        output: RustCodeAnalysisOutputFormat::Table,
        metrics: true,
        language: "rust".to_string(),
    };

    let rule = RustCodeAnalysisRule::new();
    rule.run(&args)
}
