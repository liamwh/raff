use raff_core::error::Result;
use raff_core::{ContributorReportArgs, ContributorReportOutputFormat, ContributorReportRule};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args = ContributorReportArgs {
        path: PathBuf::from("."),
        since: None,
        decay: 0.01,
        output: ContributorReportOutputFormat::Table,
        ci_output: None,
    };

    let rule = ContributorReportRule::new();
    rule.run(&args)
}
