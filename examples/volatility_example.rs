use raff_core::error::Result;
use raff_core::{VolatilityArgs, VolatilityOutputFormat, VolatilityRule};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args = VolatilityArgs {
        path: PathBuf::from("."),
        alpha: 0.01,
        since: None,
        normalize: false,
        skip_merges: false,
        output: VolatilityOutputFormat::Table,
        ci_output: None,
    };

    let rule = VolatilityRule::new();
    rule.run(&args)
}
