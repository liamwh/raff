use raff_core::error::Result;
use raff_core::{CouplingArgs, CouplingGranularity, CouplingOutputFormat, CouplingRule};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args = CouplingArgs {
        path: PathBuf::from("."),
        output: CouplingOutputFormat::Table,
        granularity: CouplingGranularity::Module,
        ci_output: None,
    };

    let rule = CouplingRule::new();
    rule.run(&args)
}
