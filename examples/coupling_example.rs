use raff_core::{CouplingArgs, CouplingGranularity, CouplingOutputFormat, CouplingRule};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args = CouplingArgs {
        path: PathBuf::from("."),
        output: CouplingOutputFormat::Table,
        granularity: CouplingGranularity::Module,
    };

    let rule = CouplingRule::new();
    rule.run(&args)
}
