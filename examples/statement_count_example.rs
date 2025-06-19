use raff_core::{StatementCountArgs, StatementCountOutputFormat, StatementCountRule};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args = StatementCountArgs {
        path: PathBuf::from("."),
        threshold: 10,
        output: StatementCountOutputFormat::Table,
    };

    let rule = StatementCountRule::new();
    rule.run(&args)
}
