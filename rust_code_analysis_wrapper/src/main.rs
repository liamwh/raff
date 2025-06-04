use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::instrument;

/// A wrapper for rust-code-analysis-cli to analyze all src folders in a project.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    /// Extra flags to pass directly to rust-code-analysis-cli.
    #[clap(short = 'f', long = "flag", num_args = 0..)]
    extra_flags: Vec<String>,

    /// Number of threads to use for analysis.
    #[clap(short, long, default_value_t = num_cpus::get())]
    jobs: usize,

    /// Root directory to scan for src/ folders.
    #[clap(short, long, value_parser)]
    root_dir: Option<PathBuf>,

    /// Output format for rust-code-analysis-cli.
    #[clap(short = 'O', long, default_value = "json")]
    output_format: String,

    /// Enable metrics mode for rust-code-analysis-cli (-m).
    #[clap(short, long, default_value_t = true)]
    metrics: bool,

    /// Language to analyze.
    #[clap(short = 'l', long, default_value = "rust")]
    language: String,
}

#[instrument]
fn discover_src_directories(root_dir: &PathBuf) -> Result<Vec<String>> {
    let mut src_paths = Vec::new();
    let walker = walkdir::WalkDir::new(root_dir).into_iter();

    for entry_result in walker.filter_entry(|e| {
        let path = e.path();
        let file_name = path.file_name().unwrap_or_default();
        // Prune target and frontend directories
        if e.file_type().is_dir() && (file_name == "target" || file_name == "frontend") {
            return false;
        }
        true
    }) {
        let entry =
            entry_result.map_err(|e| anyhow::anyhow!("Failed to read directory entry: {}", e))?;
        if entry.file_type().is_dir() && entry.file_name() == "src" {
            let path_str = entry.path().to_str().ok_or_else(|| {
                anyhow::anyhow!("Path contains invalid Unicode: {:?}", entry.path())
            })?;
            src_paths.push("-p".to_string());
            src_paths.push(path_str.to_string());
        }
    }

    if src_paths.is_empty() {
        return Err(anyhow::anyhow!(
            "No 'src' directories found in {}",
            root_dir.display()
        ));
    }

    Ok(src_paths)
}

#[instrument(skip(cli_args))]
fn run_wrapper(cli_args: Cli) -> Result<()> {
    let current_dir = std::env::current_dir()?;
    let root_path = cli_args.root_dir.as_ref().unwrap_or(&current_dir);

    tracing::info!("Starting directory discovery in: {}", root_path.display());
    let src_paths = discover_src_directories(root_path)?;
    tracing::debug!("Discovered src paths: {:?}", src_paths);

    let mut cmd_args = Vec::new();
    cmd_args.extend(src_paths);

    cmd_args.push("-l".to_string());
    cmd_args.push(cli_args.language.clone());

    if cli_args.metrics {
        cmd_args.push("-m".to_string());
    }

    cmd_args.push("-O".to_string());
    cmd_args.push(cli_args.output_format.clone());

    cmd_args.push("-j".to_string());
    cmd_args.push(cli_args.jobs.to_string());

    cmd_args.extend(cli_args.extra_flags.clone());

    tracing::info!(
        "Assembled arguments for rust-code-analysis-cli: {:?}",
        cmd_args
    );

    let mut command = std::process::Command::new("rust-code-analysis-cli");
    command.args(&cmd_args);

    tracing::info!("Executing command: {:?}", command);

    let output = command.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!(
                "rust-code-analysis-cli not found. Please ensure it is installed and in your PATH."
            )
        } else {
            anyhow::anyhow!("Failed to execute rust-code-analysis-cli: {}", e)
        }
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "rust-code-analysis-cli exited with error code {}:\\n{}",
            output.status,
            stderr
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    tracing::debug!("rust-code-analysis-cli stdout:\\n{}", stdout);

    if cli_args.output_format.to_lowercase() == "json" {
        // Placeholder for JSON parsing
        // For now, just print the JSON output if it's not empty
        if !stdout.trim().is_empty() {
            match serde_json::from_str::<serde_json::Value>(&stdout) {
                Ok(json_value) => {
                    // TODO: Define structs that match the shape of the JSON data
                    // and deserialize into those structs.
                    // For now, just pretty print the parsed JSON.
                    let pretty_json = serde_json::to_string_pretty(&json_value)
                        .unwrap_or_else(|_| stdout.to_string());
                    println!("{}", pretty_json);
                    tracing::info!("Successfully parsed JSON output.");
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to parse JSON output: {}. Raw output:\\n{}",
                        e,
                        stdout
                    );
                    // It's not an error for the wrapper if the output isn't valid JSON
                    // unless we strictly expect it to be. For now, just log and proceed.
                    // If stricter parsing is needed, this could return an Err.
                    println!("{}", stdout); // Print raw output if JSON parsing fails
                }
            }
        } else {
            tracing::info!("rust-code-analysis-cli produced no output.");
        }
    } else {
        // Print non-JSON output directly
        if !stdout.is_empty() {
            println!("{}", stdout);
        }
        if !output.stderr.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*; // Imports items from the parent module (main)
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir; // For creating temporary directories for tests

    #[test]
    fn test_discover_src_directories_no_src() -> Result<()> {
        let tmp_dir = tempdir()?;
        let result = discover_src_directories(tmp_dir.path());
        assert!(
            result.is_err(),
            "Should return an error if no src directory is found"
        );
        if let Err(e) = result {
            assert!(e.to_string().contains("No 'src' directories found"));
        }
        Ok(())
    }

    #[test]
    fn test_discover_src_directories_single_src() -> Result<()> {
        let tmp_dir = tempdir()?;
        fs::create_dir_all(tmp_dir.path().join("src"))?;

        let result = discover_src_directories(tmp_dir.path())?;
        assert_eq!(
            result.len(),
            2,
            "Should find one src directory, resulting in two path arguments"
        );
        assert_eq!(result[0], "-p");
        assert!(result[1].ends_with("src"), "Path should end with src");
        Ok(())
    }

    #[test]
    fn test_discover_src_directories_nested_src() -> Result<()> {
        let tmp_dir = tempdir()?;
        fs::create_dir_all(tmp_dir.path().join("project1").join("src"))?;
        fs::create_dir_all(tmp_dir.path().join("project2").join("src"))?;

        let result = discover_src_directories(tmp_dir.path())?;
        assert_eq!(result.len(), 4, "Should find two src directories");
        // The order might vary depending on filesystem iteration, so check for presence
        let expected_path1 = tmp_dir
            .path()
            .join("project1")
            .join("src")
            .to_string_lossy()
            .to_string();
        let expected_path2 = tmp_dir
            .path()
            .join("project2")
            .join("src")
            .to_string_lossy()
            .to_string();

        assert!(result.contains(&"-p".to_string()));
        assert!(result.contains(&expected_path1));
        assert!(result.contains(&expected_path2));
        Ok(())
    }

    #[test]
    fn test_discover_src_directories_prune_target_and_frontend() -> Result<()> {
        let tmp_dir = tempdir()?;
        fs::create_dir_all(tmp_dir.path().join("src"))?; // Root src
        fs::create_dir_all(tmp_dir.path().join("target").join("src"))?; // src under target
        fs::create_dir_all(tmp_dir.path().join("frontend").join("src"))?; // src under frontend
        fs::create_dir_all(tmp_dir.path().join("sub_project").join("src"))?; // legitimate nested src

        let result = discover_src_directories(tmp_dir.path())?;
        assert_eq!(
            result.len(),
            4,
            "Should find root src and sub_project/src, but not target/src or frontend/src"
        );

        let root_src_path = tmp_dir.path().join("src").to_string_lossy().to_string();
        let sub_project_src_path = tmp_dir
            .path()
            .join("sub_project")
            .join("src")
            .to_string_lossy()
            .to_string();

        assert!(
            result.contains(&root_src_path),
            "Result should contain root src path"
        );
        assert!(
            result.contains(&sub_project_src_path),
            "Result should contain sub_project src path"
        );

        let target_src_path = tmp_dir
            .path()
            .join("target")
            .join("src")
            .to_string_lossy()
            .to_string();
        let frontend_src_path = tmp_dir
            .path()
            .join("frontend")
            .join("src")
            .to_string_lossy()
            .to_string();

        assert!(
            !result.contains(&target_src_path),
            "Result should not contain target/src path"
        );
        assert!(
            !result.contains(&frontend_src_path),
            "Result should not contain frontend/src path"
        );
        Ok(())
    }

    // Helper to create an empty file, can be used if specific file checks are needed later
    #[allow(dead_code)]
    fn create_empty_file(path: &std::path::Path) -> Result<()> {
        let mut file = File::create(path)?;
        file.write_all(b"")?;
        Ok(())
    }
}

#[instrument]
fn main() -> Result<()> {
    color_eyre::install()?;
    // Initialize tracing subscriber with environment filter
    // Example: RUST_LOG=rust_code_analysis_wrapper=debug,warn
    // This will show debug logs from our crate, and warn or error from others.
    // If RUST_LOG is not set, it defaults to "info".
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let cli_args = Cli::parse();
    tracing::info!("Parsed CLI arguments: {:?}", cli_args);

    run_wrapper(cli_args)?;

    Ok(())
}
