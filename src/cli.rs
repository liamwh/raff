use clap::Parser;
use std::path::PathBuf;

/// CLI arguments for `rust-ff`.
#[derive(Parser)]
#[command(
    name = "rust-ff",
    about = "Fail build if any Rust component exceeds a given % of total statements"
)]
pub struct Cli {
    /// Percentage threshold (integer). If any component > this percent, exit nonzero.
    #[arg(long, default_value_t = 10)]
    pub threshold: usize,

    /// Path to the `src` directory to scan. Defaults to `./src`.
    #[arg(long, default_value = "src")]
    pub src_dir: PathBuf,
}
