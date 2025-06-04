use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Recursively collect every `.rs` file under `dir` into `out_files`.
///
/// # Parameters
/// - `dir`: the directory (e.g. "./src") to walk.
/// - `out_files`: a `Vec<PathBuf>` to push each discovered `.rs` file into.
///
/// # Returns
/// - `Ok(())` if successful.
/// - `Err` if directory traversal fails.
pub fn collect_all_rs(dir: &Path, out_files: &mut Vec<PathBuf>) -> Result<()> {
    for entry_result in WalkDir::new(dir).into_iter() {
        let entry = entry_result
            .with_context(|| format!("Error walking directory entry in '{}'", dir.display()))?;
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext == "rs" {
                    out_files.push(entry.into_path());
                }
            }
        }
    }
    Ok(())
}

/// Given a full file path (e.g. "/.../mycrate/src/foo/bar.rs") and the `src_dir` (e.g. "src"),
/// return a "namespace" string:
/// 1) Strip the `src_dir` prefix, including the path separator.
/// 2) Drop the file extension `.rs`.
/// 3) If the file name is `mod.rs`, treat the namespace as its parent folder name.
/// 4) Replace path separators `/` or `\` with `::`.
/// 5) Special‐case `lib.rs` and `main.rs` at top level as `"lib"` and `"main"`.
///
/// Examples:
/// - `src/foo/bar.rs`    → `"foo::bar"`
/// - `src/foo/mod.rs`    → `"foo"`
/// - `src/lib.rs`        → `"lib"`
/// - `src/main.rs`       → `"main"`
pub fn relative_namespace(file_path: &Path, src_dir: &Path) -> String {
    let rel = match file_path.strip_prefix(src_dir) {
        Ok(r) => r,
        Err(_) => file_path,
    };

    let rel_str = rel.to_string_lossy();
    let no_ext = rel_str.trim_end_matches(".rs");

    let mut parts: Vec<&str> = no_ext.split(std::path::MAIN_SEPARATOR).collect();
    if let Some(last) = parts.last() {
        if *last == "mod" && parts.len() > 1 {
            parts.pop();
        }
    }

    if parts.is_empty() {
        return "root".to_owned();
    }

    let joined = parts.join("::");
    if joined == "lib" || joined == "main" {
        return joined;
    }

    joined
}

/// Given a namespace like `"foo::bar::baz"`, return the top‐level component `"foo"`.
/// If there is no `"::"` in the string, return the entire string.
pub fn top_level_component(namespace: &str) -> String {
    match namespace.split("::").next() {
        Some(first) => first.to_owned(),
        None => namespace.to_owned(),
    }
}
