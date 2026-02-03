use raff_core::error::Result;
use raff_core::{all_rules, AllArgs, AllOutputFormat, CouplingGranularity};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args = AllArgs {
        path: PathBuf::from("."),
        output: AllOutputFormat::Html,
        fast: false,
        sc_threshold: 10,
        vol_alpha: 0.01,
        vol_since: None,
        vol_normalize: false,
        vol_skip_merges: false,
        coup_granularity: CouplingGranularity::Module,
        rca_extra_flags: vec![],
        rca_jobs: num_cpus::get(),
        rca_metrics: true,
        rca_language: "rust".to_string(),
        ci_output: None,
        output_file: None,
    };

    all_rules::run_all(&args)
}
