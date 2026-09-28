//! Cargo Workspace Metadata
//!
//! Shared access to `cargo metadata` for rules that reason about the packages in a
//! Cargo workspace. [`CargoMetadata::load`] runs
//! `cargo metadata --no-deps --format-version 1 --locked` once and deserialises the
//! parts raff uses; [`MemberOwnership`] maps source files under a workspace root to
//! the member package that owns them.
//!
//! # Errors
//!
//! [`CargoMetadata::load`] returns [`RaffError`] when `cargo` cannot be spawned, exits
//! unsuccessfully, or prints output that is not valid metadata JSON.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{RaffError, Result};

/// Component name for files under a workspace root that no member package owns.
pub const OUTSIDE_WORKSPACE_MEMBERS: &str = "(outside workspace members)";

/// The subset of `cargo metadata --no-deps` output raff relies on.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CargoMetadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub workspace_root: PathBuf,
}

/// A package from `cargo metadata`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub dependencies: Vec<Dependency>,
    pub manifest_path: PathBuf,
    pub targets: Vec<Target>,
}

/// A dependency declared by a [`Package`].
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Dependency {
    pub name: String,
    /// `None` for normal dependencies, otherwise `"dev"` or `"build"`.
    #[serde(default)]
    pub kind: Option<String>,
}

/// A build target (lib, bin, test, bench, example, build script) of a [`Package`].
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Target {
    pub src_path: PathBuf,
}

impl Package {
    /// The directory containing this package's `Cargo.toml`.
    #[must_use]
    pub fn manifest_dir(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(Path::new(""))
    }
}

impl CargoMetadata {
    /// Runs `cargo metadata --no-deps` for the workspace containing `dir`.
    ///
    /// `--no-deps` never resolves dependencies, so no network access is needed and
    /// no `Cargo.lock` is written.
    ///
    /// # Errors
    ///
    /// Returns an error if `cargo` cannot be run, exits unsuccessfully, or its output
    /// cannot be deserialised.
    #[tracing::instrument(level = "debug")]
    pub fn load(dir: &Path) -> Result<Self> {
        let output = Command::new("cargo")
            .args(["metadata", "--format-version", "1", "--locked", "--no-deps"])
            .current_dir(dir)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RaffError::parse_error(format!(
                "cargo metadata failed: {stderr}"
            )));
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    /// The workspace member packages, in `workspace_members` order.
    pub fn workspace_packages(&self) -> impl Iterator<Item = &Package> {
        self.workspace_members
            .iter()
            .filter_map(|id| self.packages.iter().find(|package| &package.id == id))
    }
}

/// Maps files under a Cargo workspace root to the member package that owns them.
///
/// Each member owns its manifest directory and the directory of every target's
/// `src_path`. A file belongs to the member owning its deepest ancestor directory,
/// so a root package whose binary lives in `crates/core/main.rs` owns `crates/core`
/// while `crates/printer` stays with the package whose manifest lives there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberOwnership {
    /// Owned directories relative to the workspace root, mapped to package names.
    owned_dirs: HashMap<PathBuf, String>,
}

impl MemberOwnership {
    /// Builds the ownership map when `root` is the root of a Cargo workspace with at
    /// least two members.
    ///
    /// Returns `None` when `root` has no `Cargo.toml`, `cargo metadata` fails, `root`
    /// is not the workspace root, or the workspace has fewer than two members.
    #[must_use]
    pub fn at_workspace_root(root: &Path) -> Option<Self> {
        if !root.join("Cargo.toml").is_file() {
            return None;
        }
        let metadata = match CargoMetadata::load(root) {
            Ok(metadata) => metadata,
            Err(err) => {
                tracing::debug!(
                    "Not treating {} as a Cargo workspace: {err}",
                    root.display()
                );
                return None;
            }
        };
        let root = canonical(root);
        if canonical(&metadata.workspace_root) != root {
            return None;
        }
        Self::from_metadata(&metadata, &root)
    }

    /// Builds the ownership map from `metadata` for files under the canonical `root`.
    fn from_metadata(metadata: &CargoMetadata, root: &Path) -> Option<Self> {
        let members: Vec<&Package> = metadata.workspace_packages().collect();
        if members.len() < 2 {
            return None;
        }

        let relative_to_root = |dir: &Path| {
            canonical(dir)
                .strip_prefix(root)
                .ok()
                .map(Path::to_path_buf)
        };
        let mut owned_dirs = HashMap::new();
        // Manifest directories are claimed first so a target path can never take a
        // directory away from the package whose manifest lives there.
        for package in &members {
            if let Some(dir) = relative_to_root(package.manifest_dir()) {
                owned_dirs
                    .entry(dir)
                    .or_insert_with(|| package.name.clone());
            }
        }
        for package in &members {
            for target in &package.targets {
                if let Some(dir) = target.src_path.parent().and_then(relative_to_root) {
                    owned_dirs
                        .entry(dir)
                        .or_insert_with(|| package.name.clone());
                }
            }
        }
        Some(Self { owned_dirs })
    }

    /// The package owning `relative_file`, a path relative to the workspace root, or
    /// `None` when no member owns any of its ancestor directories.
    #[must_use]
    pub fn owner(&self, relative_file: &Path) -> Option<&str> {
        relative_file
            .ancestors()
            .skip(1)
            .find_map(|dir| self.owned_dirs.get(dir))
            .map(String::as_str)
    }

    /// A stable description of the ownership map, for use in cache keys.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let mut entries: Vec<_> = self
            .owned_dirs
            .iter()
            .map(|(dir, name)| format!("{}={name}", dir.display()))
            .collect();
        entries.sort_unstable();
        entries.join("\n")
    }
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
