use raff_core::{VolatilityArgs, VolatilityOutputFormat, VolatilityRule};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args = VolatilityArgs {
        path: PathBuf::from("."),
        alpha: 0.01,
        since: None,
        normalize: false,
        skip_merges: false,
        output: VolatilityOutputFormat::Table,
    };

    let rule = VolatilityRule::new();
    rule.run(&args)
}
