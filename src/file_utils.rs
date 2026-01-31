//! File system utilities for code analysis.
//!
//! This module provides utility functions for working with file paths and
//! discovering Rust source files in a directory tree. It includes functions for:
//!
//! - Collecting all `.rs` files from a directory recursively
//! - Converting file paths to Rust namespace notation
//! - Extracting top-level module names from namespaces
//!
//! # Example
//!
//! ```rust,no_run
//! use raff_core::file_utils::{collect_all_rs, relative_namespace};
//! use std::path::Path;
//!
//! # fn main() -> raff_core::error::Result<()> {
//! let src_dir = Path::new("./src");
//! let mut files = Vec::new();
//!
//! // Collect all Rust source files
//! collect_all_rs(src_dir, &mut files)?;
//!
//! // Convert a file path to a namespace
//! let namespace = relative_namespace(
//!     &Path::new("./src/foo/bar.rs"),
//!     &Path::new("./src")
//! );
//! assert_eq!(namespace, "foo::bar");
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
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
        let entry = entry_result?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    #[test]
    fn test_collect_all_rs_returns_empty_for_empty_directory() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut files = Vec::new();
        let result = collect_all_rs(temp_dir.path(), &mut files);
        assert!(
            result.is_ok(),
            "collect_all_rs should succeed for empty directory"
        );
        assert!(
            files.is_empty(),
            "collect_all_rs should return empty vec for empty directory"
        );
    }

    #[test]
    fn test_collect_all_rs_collects_single_rs_file() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        File::create(temp_dir.path().join("test.rs")).expect("Failed to create test file");

        let mut files = Vec::new();
        collect_all_rs(temp_dir.path(), &mut files).expect("Failed to collect .rs files");

        assert_eq!(
            files.len(),
            1,
            "collect_all_rs should find 1 .rs file in directory"
        );
        assert_eq!(
            files[0].file_name().unwrap(),
            "test.rs",
            "collect_all_rs should return the correct file name"
        );
    }

    #[test]
    fn test_collect_all_rs_collects_multiple_rs_files() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        File::create(temp_dir.path().join("foo.rs")).expect("Failed to create foo.rs");
        File::create(temp_dir.path().join("bar.rs")).expect("Failed to create bar.rs");
        File::create(temp_dir.path().join("baz.rs")).expect("Failed to create baz.rs");

        let mut files = Vec::new();
        collect_all_rs(temp_dir.path(), &mut files).expect("Failed to collect .rs files");

        assert_eq!(files.len(), 3, "collect_all_rs should find all 3 .rs files");
    }

    #[test]
    fn test_collect_all_rs_ignores_non_rs_files() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        File::create(temp_dir.path().join("test.rs")).expect("Failed to create test.rs");
        File::create(temp_dir.path().join("test.txt")).expect("Failed to create test.txt");
        File::create(temp_dir.path().join("test.toml")).expect("Failed to create test.toml");

        let mut files = Vec::new();
        collect_all_rs(temp_dir.path(), &mut files).expect("Failed to collect .rs files");

        assert_eq!(
            files.len(),
            1,
            "collect_all_rs should only collect .rs files, ignoring .txt and .toml"
        );
        assert_eq!(
            files[0].file_name().unwrap(),
            "test.rs",
            "collect_all_rs should return only the .rs file"
        );
    }

    #[test]
    fn test_collect_all_rs_recursively_collects_from_subdirectories() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let subdir = temp_dir.path().join("subdir");
        fs::create_dir(&subdir).expect("Failed to create subdir");

        File::create(temp_dir.path().join("root.rs")).expect("Failed to create root.rs");
        File::create(subdir.join("nested.rs")).expect("Failed to create nested.rs");

        let mut files = Vec::new();
        collect_all_rs(temp_dir.path(), &mut files).expect("Failed to collect .rs files");

        assert_eq!(
            files.len(),
            2,
            "collect_all_rs should recursively collect files from subdirectories"
        );
    }

    #[test]
    fn test_collect_all_rs_handles_deeply_nested_directories() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let level1 = temp_dir.path().join("level1");
        let level2 = level1.join("level2");
        let level3 = level2.join("level3");
        fs::create_dir_all(&level3).expect("Failed to create nested dirs");

        File::create(level3.join("deep.rs")).expect("Failed to create deep.rs");

        let mut files = Vec::new();
        collect_all_rs(temp_dir.path(), &mut files).expect("Failed to collect .rs files");

        assert_eq!(
            files.len(),
            1,
            "collect_all_rs should find files in deeply nested directories"
        );
        assert!(
            files[0].ends_with("level3/deep.rs") || files[0].ends_with("level3\\deep.rs"),
            "collect_all_rs should preserve the full path to the nested file"
        );
    }

    #[test]
    fn test_relative_namespace_basic_path() {
        let file_path = PathBuf::from("/project/src/foo/bar.rs");
        let src_dir = PathBuf::from("/project/src");

        let result = relative_namespace(&file_path, &src_dir);

        assert_eq!(
            result, "foo::bar",
            "relative_namespace should convert 'src/foo/bar.rs' to 'foo::bar'"
        );
    }

    #[test]
    fn test_relative_namespace_mod_rs_becomes_parent_name() {
        let file_path = PathBuf::from("/project/src/foo/mod.rs");
        let src_dir = PathBuf::from("/project/src");

        let result = relative_namespace(&file_path, &src_dir);

        assert_eq!(
            result, "foo",
            "relative_namespace should convert 'src/foo/mod.rs' to 'foo'"
        );
    }

    #[test]
    fn test_relative_namespace_lib_rs_becomes_lib() {
        let file_path = PathBuf::from("/project/src/lib.rs");
        let src_dir = PathBuf::from("/project/src");

        let result = relative_namespace(&file_path, &src_dir);

        assert_eq!(
            result, "lib",
            "relative_namespace should convert 'src/lib.rs' to 'lib'"
        );
    }

    #[test]
    fn test_relative_namespace_main_rs_becomes_main() {
        let file_path = PathBuf::from("/project/src/main.rs");
        let src_dir = PathBuf::from("/project/src");

        let result = relative_namespace(&file_path, &src_dir);

        assert_eq!(
            result, "main",
            "relative_namespace should convert 'src/main.rs' to 'main'"
        );
    }

    #[test]
    fn test_relative_namespace_deeply_nested_path() {
        let file_path = PathBuf::from("/project/src/a/b/c/d.rs");
        let src_dir = PathBuf::from("/project/src");

        let result = relative_namespace(&file_path, &src_dir);

        assert_eq!(
            result, "a::b::c::d",
            "relative_namespace should convert deeply nested paths with '::' separators"
        );
    }

    #[test]
    fn test_relative_namespace_mod_rs_at_nested_level() {
        let file_path = PathBuf::from("/project/src/foo/bar/baz/mod.rs");
        let src_dir = PathBuf::from("/project/src");

        let result = relative_namespace(&file_path, &src_dir);

        assert_eq!(
            result, "foo::bar::baz",
            "relative_namespace should strip 'mod.rs' at any nesting level"
        );
    }

    #[test]
    fn test_relative_namespace_file_not_in_src_dir_returns_full_namespace() {
        let file_path = PathBuf::from("/other/project/src/foo.rs");
        let src_dir = PathBuf::from("/project/src");

        let result = relative_namespace(&file_path, &src_dir);

        // When file_path doesn't start with src_dir, it returns the full path without extension
        assert!(
            result.contains("other") && result.contains("project") && result.contains("foo"),
            "relative_namespace should handle files not in src_dir gracefully: got {}",
            result
        );
    }

    #[test]
    fn test_relative_namespace_with_backslash_path_on_unix() {
        // On Unix systems, backslashes are valid filename characters, not path separators
        // The function uses MAIN_SEPARATOR which is platform-dependent
        let file_path = PathBuf::from("C:\\project\\src\\foo\\bar.rs");
        let src_dir = PathBuf::from("C:\\project\\src");

        let result = relative_namespace(&file_path, &src_dir);

        // On Unix, backslashes don't split, so we get the whole path minus extension
        assert!(
            result.contains("bar"),
            "relative_namespace should handle path, though result varies by platform: got {}",
            result
        );
    }

    #[test]
    fn test_top_level_component_simple_namespace() {
        let namespace = "foo::bar::baz";

        let result = top_level_component(namespace);

        assert_eq!(
            result, "foo",
            "top_level_component should extract 'foo' from 'foo::bar::baz'"
        );
    }

    #[test]
    fn test_top_level_component_single_component() {
        let namespace = "foo";

        let result = top_level_component(namespace);

        assert_eq!(
            result, "foo",
            "top_level_component should return 'foo' when there's no '::' separator"
        );
    }

    #[test]
    fn test_top_level_component_two_components() {
        let namespace = "std::collections";

        let result = top_level_component(namespace);

        assert_eq!(
            result, "std",
            "top_level_component should extract 'std' from 'std::collections'"
        );
    }

    #[test]
    fn test_top_level_component_empty_string() {
        let namespace = "";

        let result = top_level_component(namespace);

        assert_eq!(
            result, "",
            "top_level_component should return empty string for empty input"
        );
    }

    #[test]
    fn test_top_level_component_many_components() {
        let namespace = "a::b::c::d::e::f";

        let result = top_level_component(namespace);

        assert_eq!(
            result, "a",
            "top_level_component should extract first component from deeply nested namespace"
        );
    }

    #[test]
    fn test_relative_namespace_empty_file_path() {
        let file_path = PathBuf::from("");
        let src_dir = PathBuf::from("/project/src");

        let result = relative_namespace(&file_path, &src_dir);

        // Empty path after strip_prefix fails returns empty string, not "root"
        // "root" is returned when parts becomes empty after splitting, but empty string doesn't split
        assert_eq!(
            result, "",
            "relative_namespace should return empty string for empty file path"
        );
    }
}
