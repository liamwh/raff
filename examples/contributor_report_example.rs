use raff_core::error::Result;
use raff_core::{ContributorReportArgs, ContributorReportOutputFormat, ContributorReportRule};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args = ContributorReportArgs {
        path: PathBuf::from("."),
        since: None,
        decay: 0.01,
        output: ContributorReportOutputFormat::Table,
    };

    let rule = ContributorReportRule::new();
    rule.run(&args)
}
