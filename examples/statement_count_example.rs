use raff_core::error::Result;
use raff_core::{StatementCountArgs, StatementCountOutputFormat, StatementCountRule};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args = StatementCountArgs {
        path: PathBuf::from("."),
        threshold: 10,
        output: StatementCountOutputFormat::Table,
        ci_output: None,
        output_file: None,
        staged: false,
    };

    let rule = StatementCountRule::new();
    rule.run(&args)
}
