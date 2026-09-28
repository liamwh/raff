//! Code Coupling Rule
//!
//! This module provides the coupling analysis rule, which measures dependencies
//! between components in a Rust codebase. It supports both crate-level and
//! module-level coupling analysis.
//!
//! # Overview
//!
//! The coupling rule analyzes how different parts of a codebase depend on each other.
//! It calculates:
//!
//! - **Ce (Efferent Coupling)**: The number of other components this component depends on
//! - **Ca (Afferent Coupling)**: The number of other components that depend on this component
//! - **I (Instability)**: Ce / (Ce + Ca) — ranges from 0 (stable) to 1 (unstable)
//!
//! # Findings: the Stable Dependencies Principle
//!
//! Instability on its own is not a problem: binaries and other entry-point crates
//! are expected to sit at or near `I = 1`. What matters is the direction of each
//! dependency. Following Robert C. Martin's Stable Dependencies Principle, a crate
//! should only depend on crates that are at least as stable as itself. For every
//! workspace-internal edge `A -> B`, the rule emits a warning when `I(B) > I(A)`.
//! Crates with no coupling at all (`Ce + Ca == 0`) are treated as `I = 0`.
//! With `--staged`, instability still comes from the whole workspace graph, but only
//! edges where either crate has staged changes are reported.
//!
//! # Granularity Levels
//!
//! The analysis can be performed at three levels:
//!
//! - **Crate**: Analyzes dependencies between workspace crates using `cargo metadata`
//! - **Module**: Analyzes dependencies between Rust modules within each crate using AST analysis
//! - **Both**: Reports both crate-level and module-level coupling
//!
//! # Usage
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use raff_core::coupling_rule::CouplingRule;
//! use raff_core::{CouplingArgs, CouplingGranularity, CouplingOutputFormat};
//! use std::path::PathBuf;
//!
//! let rule = CouplingRule::new();
//! let args = CouplingArgs {
//!     path: PathBuf::from("."),
//!     output: CouplingOutputFormat::Table,
//!     granularity: CouplingGranularity::Both,
//!     ci_output: None,
//!     output_file: None,
//!     staged: false,
//! };
//!
//! if let Err(e) = rule.run(&args) {
//!     eprintln!("Error: {}", e);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Data Structures
//!
//! - [`CouplingRule`]: The main rule implementation
//! - [`CrateCoupling`]: Coupling data for a single crate
//! - [`ModuleCoupling`]: Coupling data for a single module
//! - [`CouplingData`]: Container for all coupling analysis results
//!
//! # Output Formats
//!
//! The rule supports multiple output formats:
//! - `Table`: Human-readable table format
//! - `Json`: Machine-readable JSON
//! - `Yaml`: Machine-readable YAML
//! - `Html`: Interactive HTML report with sortable tables
//! - `Dot`: Graphviz DOT format for visualization
//!
//! # Crate-Level Analysis
//!
//! At the crate level, the rule uses `cargo metadata` to analyze the dependency graph
//! of workspace crates. It identifies which crates depend on which other crates within
//! the workspace. Normal and build dependencies count as edges; dev-dependencies do
//! not, because they only serve tests, examples and benches.
//!
//! # Module-Level Analysis
//!
//! At the module level, the rule performs AST analysis to identify:
//! - `use` statements and their paths
//! - Type references in function signatures and struct fields
//! - Expression paths that reference other modules
//!
//! # Errors
//!
//! This module returns [`RaffError`] in the following cases:
//! - The provided path does not exist or is not a directory
//! - `cargo metadata` fails to execute or returns invalid output
//! - Git operations fail for repository-level analysis

use crate::cargo_workspace::{CargoMetadata, Package};
use crate::ci_report::{Finding, Severity, ToFindings};
use crate::cli::{CiOutputFormat, CouplingArgs, CouplingGranularity, CouplingOutputFormat};
use crate::error::{RaffError, Result};
use crate::html_utils;
use crate::rule::Rule;
use crate::table_utils::get_default_table_format;
use maud::{Markup, html};
use prettytable::{Cell, Row, Table};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use syn::{ExprPath, Item, ItemMod, ItemUse, PatType, visit::Visit};
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize)]
struct CrateLevelAnalysisResult {
    crate_couplings_map: HashMap<String, CrateCoupling>,
    workspace_packages_map: HashMap<String, Package>,
    package_id_to_name: HashMap<String, String>,
    workspace_member_ids: HashSet<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CrateCoupling {
    pub name: String,
    pub ce: usize,
    pub ca: usize,
    pub modules: Vec<ModuleCoupling>,
    pub dependencies: HashSet<String>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ModuleCoupling {
    pub path: String,
    pub ce_m: usize,
    pub ca_m: usize,
    pub module_dependencies: HashSet<String>,
}

#[derive(Serialize, Debug, Default)]
pub struct CouplingData {
    /// Crates shown in the report; in staged mode only the affected ones.
    pub crates: Vec<CrateCoupling>,
    pub granularity: CouplingGranularity,
    pub analysis_path: PathBuf,
    /// Stable Dependencies Principle violations, evaluated over the whole workspace
    /// graph (see [`find_sdp_violations`]).
    #[serde(skip)]
    pub sdp_violations: Vec<SdpViolation>,
}

impl CrateCoupling {
    /// Instability `I = Ce / (Ce + Ca)`; a crate with no coupling at all has `I = 0`.
    pub fn instability(&self) -> f64 {
        let total_coupling = self.ce + self.ca;
        if total_coupling == 0 {
            0.0
        } else {
            self.ce as f64 / total_coupling as f64
        }
    }
}

/// A workspace edge `dependant -> dependency` where the dependency is less stable
/// than the crate depending on it.
#[derive(Debug, Clone, PartialEq)]
pub struct SdpViolation {
    pub dependant: String,
    pub dependant_instability: f64,
    pub dependency: String,
    pub dependency_instability: f64,
}

/// Finds violations of the Stable Dependencies Principle: for every workspace-internal
/// edge `A -> B`, `B` should be at least as stable as `A` (`I(B) <= I(A)`). A high
/// instability on its own is not reported, as entry points such as binaries are
/// expected to be maximally unstable.
///
/// `crates` must be the whole workspace graph so that instability uses the full Ce and
/// Ca. When `affected` is set (staged mode), only edges where `A` or `B` is affected
/// are kept. Violations are sorted by dependant, then dependency.
pub fn find_sdp_violations<'a>(
    crates: impl IntoIterator<Item = &'a CrateCoupling>,
    affected: Option<&HashSet<String>>,
) -> Vec<SdpViolation> {
    let crates: Vec<&CrateCoupling> = crates.into_iter().collect();
    let instability_by_crate: HashMap<&str, f64> = crates
        .iter()
        .map(|krate| (krate.name.as_str(), krate.instability()))
        .collect();
    let is_reported = |dependant: &str, dependency: &str| {
        affected
            .is_none_or(|affected| affected.contains(dependant) || affected.contains(dependency))
    };

    let mut violations = Vec::new();
    for krate in crates {
        let dependant_instability = instability_by_crate[krate.name.as_str()];
        for dependency in &krate.dependencies {
            let Some(&dependency_instability) = instability_by_crate.get(dependency.as_str())
            else {
                continue;
            };
            if dependency_instability > dependant_instability + 1e-9
                && is_reported(&krate.name, dependency)
            {
                violations.push(SdpViolation {
                    dependant: krate.name.clone(),
                    dependant_instability,
                    dependency: dependency.clone(),
                    dependency_instability,
                });
            }
        }
    }
    violations.sort_by(|a, b| (&a.dependant, &a.dependency).cmp(&(&b.dependant, &b.dependency)));
    violations
}

impl ToFindings for CouplingData {
    #[tracing::instrument(skip(self), fields(rule_id = "coupling"))]
    fn to_findings(&self) -> Vec<Finding> {
        self.sdp_violations
            .iter()
            .map(|violation| Finding {
                rule_id: "coupling".to_string(),
                rule_name: "Code Coupling Rule".to_string(),
                severity: Severity::Warning,
                message: format!(
                    "Crate '{}' (I={:.2}) depends on less stable crate '{}' (I={:.2}), violating the Stable Dependencies Principle",
                    violation.dependant,
                    violation.dependant_instability,
                    violation.dependency,
                    violation.dependency_instability
                ),
                location: None, // Coupling is crate-level, no specific file location
                help_uri: Some("https://github.com/liamwh/raff#module-coupling".to_string()),
                fingerprint: Some(format!(
                    "coupling-sdp:{}:{}",
                    violation.dependant, violation.dependency
                )),
            })
            .collect()
    }
}

pub struct CouplingRule;

impl Rule for CouplingRule {
    type Config = CouplingArgs;
    type Data = CouplingData;

    fn name() -> &'static str {
        "coupling"
    }

    fn description() -> &'static str {
        "Analyzes code coupling between components at crate and module levels"
    }

    fn run(&self, config: &Self::Config) -> Result<()> {
        self.run_impl(config)
    }

    fn analyze(&self, config: &Self::Config) -> Result<Self::Data> {
        self.analyze_impl(config)
    }
}

impl Default for CouplingRule {
    fn default() -> Self {
        Self::new()
    }
}

impl CouplingRule {
    pub fn new() -> Self {
        Self
    }

    #[tracing::instrument(level = "debug", skip(self, args), ret)]
    fn run_impl(&self, args: &CouplingArgs) -> Result<()> {
        let full_report = self.analyze(args)?;

        // Check for CI output first (takes precedence)
        if let Some(ci_format) = &args.ci_output {
            let findings = full_report.to_findings();

            let output = match ci_format {
                CiOutputFormat::Sarif => crate::ci_report::to_sarif(&findings)?,
                CiOutputFormat::JUnit => crate::ci_report::to_junit(&findings, "coupling")?,
            };

            // Write to file if specified, otherwise stdout
            if let Some(ref output_file) = args.output_file {
                let mut file = fs::File::create(output_file).map_err(|e| {
                    RaffError::io_error(format!(
                        "Failed to create output file {}: {}",
                        output_file.display(),
                        e
                    ))
                })?;
                file.write_all(output.as_bytes()).map_err(|e| {
                    RaffError::io_error(format!(
                        "Failed to write to output file {}: {}",
                        output_file.display(),
                        e
                    ))
                })?;
            } else {
                println!("{output}");
            }

            // Note: Coupling uses Severity::Warning, which doesn't fail CI
            // We always return Ok for coupling warnings
            return Ok(());
        }

        match args.output {
            CouplingOutputFormat::Table => {
                self.print_table_report(&full_report, &full_report.granularity)?;
            }
            CouplingOutputFormat::Json => {
                let json = serde_json::to_string_pretty(&full_report)?;
                println!("{json}");
            }
            CouplingOutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(&full_report)?;
                println!("{yaml}");
            }
            CouplingOutputFormat::Html => {
                let html_body = self.render_coupling_html_body(&full_report)?;
                let full_html = html_utils::render_html_doc(
                    &format!("Coupling Report: {}", full_report.analysis_path.display()),
                    html_body,
                );
                println!("{full_html}");
            }
            CouplingOutputFormat::Dot => {
                self.print_dot_report(&full_report, &full_report.granularity)?;
            }
        }
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, args), ret)]
    fn analyze_impl(&self, args: &CouplingArgs) -> Result<CouplingData> {
        let analysis_result = self.analyze_crate_level_coupling(args)?;
        let crate_couplings_map = analysis_result.crate_couplings_map;
        let workspace_packages_map = analysis_result.workspace_packages_map;
        let package_id_to_name = analysis_result.package_id_to_name;
        let workspace_member_ids = analysis_result.workspace_member_ids;
        let affected_crates = if args.staged {
            Some(self.detect_affected_crates(&workspace_packages_map)?)
        } else {
            None
        };

        let mut full_report = CouplingData {
            crates: Vec::new(),
            granularity: args.granularity.clone(),
            analysis_path: args.path.clone(),
            // Instability needs the full Ce/Ca, so the check always sees every crate.
            sdp_violations: find_sdp_violations(
                crate_couplings_map.values(),
                affected_crates.as_ref(),
            ),
        };

        if let Some(affected_crates) = affected_crates.as_ref()
            && affected_crates.is_empty()
        {
            return Ok(full_report);
        }

        let mut sorted_workspace_pkg_ids: Vec<_> = workspace_member_ids.iter().cloned().collect();
        sorted_workspace_pkg_ids
            .sort_by_key(|id| package_id_to_name.get(id).cloned().unwrap_or_default());

        for pkg_id in sorted_workspace_pkg_ids {
            if let Some(pkg_data) = workspace_packages_map.get(&pkg_id) {
                let crate_name = &pkg_data.name;
                if let Some(affected_crates) = affected_crates.as_ref()
                    && !affected_crates.contains(crate_name)
                {
                    continue;
                }
                let mut current_crate_coupling = crate_couplings_map
                    .get(crate_name)
                    .cloned()
                    .unwrap_or_else(|| CrateCoupling {
                        name: crate_name.clone(),
                        ce: 0,
                        ca: 0,
                        modules: Vec::new(),
                        dependencies: HashSet::new(),
                    });

                if matches!(
                    args.granularity,
                    CouplingGranularity::Module | CouplingGranularity::Both
                ) {
                    let src_path = pkg_data.manifest_dir().join("src");
                    if src_path.exists() {
                        let mut module_couplings = self.analyze_module_level_coupling_for_crate(
                            crate_name, &src_path, pkg_data,
                        )?;
                        module_couplings
                            .sort_by_key(|item| std::cmp::Reverse(item.ce_m + item.ca_m));
                        current_crate_coupling.modules = module_couplings;
                    }
                }
                full_report.crates.push(current_crate_coupling);
            }
        }

        full_report
            .crates
            .sort_by_key(|item| std::cmp::Reverse(item.ce + item.ca));

        Ok(full_report)
    }

    /// Public wrapper that delegates to the Rule trait's run method
    pub fn run(&self, args: &CouplingArgs) -> Result<()> {
        self.run_impl(args)
    }

    /// Public wrapper that delegates to the Rule trait's analyze method
    pub fn analyze(&self, args: &CouplingArgs) -> Result<CouplingData> {
        self.analyze_impl(args)
    }

    #[tracing::instrument(level = "debug", skip(self, args), ret)]
    fn analyze_crate_level_coupling(
        &self,
        args: &CouplingArgs,
    ) -> Result<CrateLevelAnalysisResult> {
        let analysis_path = &args.path;
        if !analysis_path.exists() {
            return Err(RaffError::invalid_input(format!(
                "Provided path does not exist: {}",
                analysis_path.display()
            )));
        }
        if !analysis_path.is_dir() {
            return Err(RaffError::invalid_input(format!(
                "Provided path is not a directory: {}",
                analysis_path.display()
            )));
        }
        tracing::info!(
            "Analyzing coupling in directory: {}",
            analysis_path.display()
        );

        let metadata = CargoMetadata::load(analysis_path)?;

        let workspace_member_ids: HashSet<_> = metadata.workspace_members.iter().cloned().collect();
        let mut package_id_to_name: HashMap<String, String> = HashMap::new();
        let mut workspace_packages_map: HashMap<String, Package> = HashMap::new();

        for pkg in &metadata.packages {
            package_id_to_name.insert(pkg.id.clone(), pkg.name.clone());
            if workspace_member_ids.contains(&pkg.id) {
                workspace_packages_map.insert(pkg.id.clone(), pkg.clone());
            }
        }

        let workspace_package_names: HashSet<&str> = workspace_packages_map
            .values()
            .map(|package| package.name.as_str())
            .collect();
        let mut crate_couplings_map: HashMap<String, CrateCoupling> = HashMap::new();
        for package in workspace_packages_map.values() {
            // Dev-dependencies only serve tests, examples and benches; they are not
            // part of the architecture, so only normal and build edges count.
            let dependencies = package
                .dependencies
                .iter()
                .filter(|dep| dep.kind.as_deref() != Some("dev"))
                .filter(|dep| {
                    dep.name != package.name && workspace_package_names.contains(dep.name.as_str())
                })
                .map(|dep| dep.name.clone())
                .collect();
            crate_couplings_map.insert(
                package.name.clone(),
                CrateCoupling {
                    name: package.name.clone(),
                    ce: 0,
                    ca: 0,
                    modules: Vec::new(),
                    dependencies,
                },
            );
        }

        let mut afferent_couplings: HashMap<String, usize> = HashMap::new();
        for coupling_data in crate_couplings_map.values() {
            for dependency in &coupling_data.dependencies {
                *afferent_couplings.entry(dependency.clone()).or_insert(0) += 1;
            }
        }
        for (name, coupling_data) in crate_couplings_map.iter_mut() {
            coupling_data.ce = coupling_data.dependencies.len();
            coupling_data.ca = afferent_couplings.get(name).copied().unwrap_or(0);
        }

        Ok(CrateLevelAnalysisResult {
            crate_couplings_map,
            workspace_packages_map,
            package_id_to_name,
            workspace_member_ids,
        })
    }

    fn detect_affected_crates(
        &self,
        workspace_packages_map: &HashMap<String, Package>,
    ) -> Result<HashSet<String>> {
        let staged_files = crate::git_utils::get_staged_files()?;
        if staged_files.is_empty() {
            return Ok(HashSet::new());
        }

        let mut affected = HashSet::new();
        for package in workspace_packages_map.values() {
            let package_dir = package.manifest_dir();
            let canonical_package_dir = package_dir
                .canonicalize()
                .unwrap_or_else(|_| package_dir.to_path_buf());
            if staged_files
                .iter()
                .any(|file| file.starts_with(&canonical_package_dir))
            {
                affected.insert(package.name.clone());
            }
        }

        Ok(affected)
    }

    #[tracing::instrument(level = "debug", skip(self, _pkg_data), ret)]
    fn analyze_module_level_coupling_for_crate(
        &self,
        crate_name: &str,
        src_path: &Path,
        _pkg_data: &Package,
    ) -> Result<Vec<ModuleCoupling>> {
        let mut module_map: HashMap<String, PathBuf> = HashMap::new();
        self.discover_modules(src_path, PathBuf::from("crate"), &mut module_map)?;
        let mut module_efferent_couplings: HashMap<String, HashSet<String>> = HashMap::new();
        let mut module_afferent_couplings: HashMap<String, HashSet<String>> = HashMap::new();
        let mut module_results_map: BTreeMap<String, ModuleCoupling> = module_map
            .keys()
            .map(|mod_path_str| (mod_path_str.clone(), ModuleCoupling::default()))
            .collect();

        for (current_module_path_str, source_file_path) in &module_map {
            let content = fs::read_to_string(source_file_path)?;
            match syn::parse_file(&content) {
                Ok(ast) => {
                    let mut visitor = ModuleDependencyVisitor::new(
                        crate_name,
                        current_module_path_str
                            .split("::")
                            .map(String::from)
                            .collect(),
                        &module_map,
                        HashSet::new(),
                    );
                    visitor.visit_file(&ast);
                    let collected_module_dependencies = visitor.dependencies;

                    if let Some(coupling_data) = module_results_map.get_mut(current_module_path_str)
                    {
                        coupling_data.module_dependencies = collected_module_dependencies.clone();
                    }

                    for referenced_module_path in collected_module_dependencies {
                        if let Some(efferent_set) =
                            module_efferent_couplings.get_mut(current_module_path_str)
                        {
                            efferent_set.insert(referenced_module_path.clone());
                        }
                        if let Some(afferent_set) =
                            module_afferent_couplings.get_mut(&referenced_module_path)
                        {
                            afferent_set.insert(current_module_path_str.clone());
                        }
                    }
                }
                Err(err) => {
                    eprintln!(
                        "Warning: Failed to parse module {} at {}: {}. Skipping for module analysis.",
                        current_module_path_str,
                        source_file_path.display(),
                        err
                    );
                }
            }
        }

        for (mod_path, coupling_data) in module_results_map.iter_mut() {
            coupling_data.path = if mod_path == "crate" {
                "crate_root".to_string()
            } else {
                mod_path.trim_start_matches("crate::").to_string()
            };
            coupling_data.ce_m = module_efferent_couplings
                .get(mod_path)
                .map_or(0, |s| s.len());
            coupling_data.ca_m = module_afferent_couplings
                .get(mod_path)
                .map_or(0, |s| s.len());
        }
        Ok(module_results_map.into_values().collect())
    }

    fn print_table_report(
        &self,
        report: &CouplingData,
        granularity: &CouplingGranularity,
    ) -> Result<()> {
        if matches!(
            granularity,
            CouplingGranularity::Crate | CouplingGranularity::Both
        ) {
            println!("[Crate level]");
            let mut crate_table = Table::new();
            crate_table.set_format(get_default_table_format());
            crate_table.set_titles(Row::new(vec![
                Cell::new("Crate Name"),
                Cell::new("Ce (Efferent)"),
                Cell::new("Ca (Afferent)"),
            ]));
            for crate_data in &report.crates {
                crate_table.add_row(Row::new(vec![
                    Cell::new(&crate_data.name),
                    Cell::new(&crate_data.ce.to_string()),
                    Cell::new(&crate_data.ca.to_string()),
                ]));
            }
            crate_table.printstd();
        }

        if matches!(
            granularity,
            CouplingGranularity::Module | CouplingGranularity::Both
        ) {
            let mut first_module_table = true;
            for crate_data in &report.crates {
                if !crate_data.modules.is_empty() {
                    if matches!(
                        granularity,
                        CouplingGranularity::Crate | CouplingGranularity::Both
                    ) && !first_module_table
                    {
                        println!();
                    } else if first_module_table
                        && matches!(granularity, CouplingGranularity::Module)
                    {
                    } else if !first_module_table {
                        println!();
                    }
                    println!("[Module level: {}]", crate_data.name);
                    first_module_table = false;
                    let mut module_table = Table::new();
                    module_table.set_format(get_default_table_format());
                    module_table.set_titles(Row::new(vec![
                        Cell::new("  Module Path"),
                        Cell::new("Ce_m (Efferent)"),
                        Cell::new("Ca_m (Afferent)"),
                    ]));
                    for module_data in &crate_data.modules {
                        module_table.add_row(Row::new(vec![
                            Cell::new(&format!("  {}", module_data.path)),
                            Cell::new(&module_data.ce_m.to_string()),
                            Cell::new(&module_data.ca_m.to_string()),
                        ]));
                    }
                    module_table.printstd();
                }
            }
        }
        Ok(())
    }

    pub fn render_coupling_html_body(&self, report: &CouplingData) -> Result<Markup> {
        let granularity = &report.granularity;
        let analysis_path = &report.analysis_path;
        let mut explanations = vec![
            (
                "Ce (Efferent Coupling)",
                "The number of other components that this component depends on.",
            ),
            (
                "Ca (Afferent Coupling)",
                "The number of other components that depend on this component.",
            ),
            (
                "I (Instability)",
                "Ce / (Ce + Ca). Ranges from 0 (completely stable) to 1 (completely unstable).",
            ),
            ("A (Abstractness)", "Not implemented in this version."),
            (
                "D (Distance)",
                "The perpendicular distance from the main sequence. |A + I - 1|. A value of 0 is ideal, 1 is the furthest away.",
            ),
        ];
        if matches!(
            granularity,
            CouplingGranularity::Module | CouplingGranularity::Both
        ) {
            explanations.push(("Ce_M (Module Efferent Coupling)", "Number of other modules this module depends on (within the same crate or other workspace crates)."));
            explanations.push((
                "Ca_M (Module Afferent Coupling)",
                "Number of other modules that depend on this module.",
            ));
        }

        let explanations_markup = html_utils::render_metric_explanation_list(&explanations);

        let max_coupling = report.crates.iter().map(|c| c.ce + c.ca).max().unwrap_or(1) as f64;

        let table_markup = html! {
            @if matches!(granularity, CouplingGranularity::Crate | CouplingGranularity::Both) {
                h2 { "Crate Level Coupling" }
                table class="sortable-table" {
                    caption { (format!("Analysis Path: {}", analysis_path.display())) }
                    thead {
                        tr {
                            th class="sortable-header" data-column-index="0" data-sort-type="string" { "Crate" }
                            th class="sortable-header" data-column-index="1" data-sort-type="number" { "Ce" }
                            th class="sortable-header" data-column-index="2" data-sort-type="number" { "Ca" }
                            th class="sortable-header" data-column-index="3" data-sort-type="number" { "I" }
                            th class="sortable-header" data-column-index="4" data-sort-type="string" { "A" }
                            th class="sortable-header" data-column-index="5" data-sort-type="number" { "D" }
                        }
                    }
                    tbody {
                        @for krate in &report.crates {
                            @let instability = krate.instability();
                            @let distance = (instability - 1.0).abs();
                            @let ce_style = html_utils::get_cell_style(krate.ce as f64, max_coupling / 2.0, max_coupling, false);
                            @let ca_style = html_utils::get_cell_style(krate.ca as f64, max_coupling / 2.0, max_coupling, false);
                             @let i_style = html_utils::get_cell_style(instability, 0.5, 0.8, false);
                            @let d_style = html_utils::get_cell_style(distance, 0.5, 0.8, false);

                            tr {
                                td { (krate.name) }
                                td style=(ce_style) { (krate.ce) }
                                td style=(ca_style) { (krate.ca) }
                                td style=(i_style) { (format!("{:.2}", instability)) }
                                td { "N/A" }
                                td style=(d_style) { (format!("{:.2}", distance)) }
                            }
                        }
                    }
                }
            }

            @if matches!(granularity, CouplingGranularity::Module | CouplingGranularity::Both) {
                h2 { "Module Level Coupling" }
                @for krate in &report.crates {
                    @if !krate.modules.is_empty() {
                        h3 { "Crate: " (krate.name) }
                        table class="sortable-table" {
                            thead {
                                tr {
                                    th class="sortable-header" data-column-index="0" data-sort-type="string" { "Module" }
                                    th class="sortable-header" data-column-index="1" data-sort-type="number" { "Ce_M" }
                                    th class="sortable-header" data-column-index="2" data-sort-type="number" { "Ca_M" }
                                }
                            }
                            tbody {
                                @for module in &krate.modules {
                                    tr {
                                        td { (module.path) }
                                        td { (module.ce_m) }
                                        td { (module.ca_m) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };

        Ok(html! {
            (explanations_markup)
            (table_markup)
        })
    }

    // New method for DOT output
    #[tracing::instrument(level = "debug", skip(self, report, granularity))]
    fn print_dot_report(
        &self,
        report: &CouplingData,
        granularity: &CouplingGranularity,
    ) -> Result<()> {
        if matches!(
            granularity,
            CouplingGranularity::Crate | CouplingGranularity::Both
        ) {
            let dot_string = self.generate_crate_dot(report)?;
            println!("{dot_string}");
        }

        if matches!(
            granularity,
            CouplingGranularity::Module | CouplingGranularity::Both
        ) {
            if matches!(granularity, CouplingGranularity::Both) {
                println!(
                    "

"
                ); // Separator if printing both
            }
            let dot_string = self.generate_module_dot(report)?;
            println!("{dot_string}");
        }
        Ok(())
    }

    // Helper to get color for DOT nodes
    fn get_dot_color(value: f64, max_value: f64) -> String {
        if max_value == 0.0 {
            return "0.33,1.0,0.7".to_string(); // Light green for no coupling or single element
        }
        let normalized = (value / max_value).clamp(0.0, 1.0); // Clamp between 0 and 1
        let hue = 0.33 * (1.0 - normalized); // 0.33 (green) to 0.0 (red)
        format!("{:.2},{:.1},{:.1}", hue, 1.0, 0.5) // HSL format
    }

    #[tracing::instrument(level = "debug", skip(self, report))]
    fn generate_crate_dot(&self, report: &CouplingData) -> Result<String> {
        let mut dot = String::from("digraph CrateCoupling {\n");
        dot.push_str("  rankdir=\"LR\";\n");
        dot.push_str("  node [shape=box, style=filled];\n\n");

        let max_coupling = report.crates.iter().map(|c| c.ce + c.ca).max().unwrap_or(0) as f64;

        for crate_data in &report.crates {
            let total_coupling = (crate_data.ce + crate_data.ca) as f64;
            let color = Self::get_dot_color(total_coupling, max_coupling);
            dot.push_str(&format!(
                "  \"{}\" [label=\"{}\nCe: {}\nCa: {}\", fillcolor=\"{}\"];\n",
                crate_data.name, crate_data.name, crate_data.ce, crate_data.ca, color
            ));
        }
        dot.push('\n');

        for crate_data in &report.crates {
            for dep_name in &crate_data.dependencies {
                dot.push_str(&format!("  \"{}\" -> \"{}\";\n", crate_data.name, dep_name));
            }
        }

        dot.push_str("}\n");
        Ok(dot)
    }

    #[tracing::instrument(level = "debug", skip(self, report))]
    fn generate_module_dot(&self, report: &CouplingData) -> Result<String> {
        let mut dot = String::from("digraph ModuleCoupling {\n");
        dot.push_str("  rankdir=\"LR\";\n");
        dot.push_str("  node [shape=box, style=filled];\n");
        dot.push_str("  compound=true; // Allow edges to clusters\n\n");

        let max_module_coupling = report
            .crates
            .iter()
            .flat_map(|c| &c.modules)
            .map(|m| m.ce_m + m.ca_m)
            .max()
            .unwrap_or(0) as f64;

        for crate_data in &report.crates {
            if crate_data.modules.is_empty() {
                continue;
            }
            dot.push_str(&format!("  subgraph \"cluster_{}\" {{\n", crate_data.name));
            dot.push_str(&format!("    label = \"{}\";\n", crate_data.name));
            dot.push_str("    style=filled;\n");
            dot.push_str("    color=lightgrey;\n\n");

            for module_data in &crate_data.modules {
                let total_coupling = (module_data.ce_m + module_data.ca_m) as f64;
                let color = Self::get_dot_color(total_coupling, max_module_coupling);
                // Ensure module paths are suitable as DOT IDs (they should be if no spaces/special chars beyond ::)
                // Full path for module: crate_name::module_path (if module_data.path is relative to crate)
                // Current module_data.path is already like "crate_root" or "module::submodule"
                // For DOT ID, ensure uniqueness. Prefix with crate name if path is not global.
                // Module path seems to be "crate" or "crate::module" from discover_modules
                // ModuleCoupling.path is "crate_root" or "module_name" (relative to crate)
                // Let's form a globally unique ID for module nodes: CrateName::ModulePath
                let module_node_id = if module_data.path == "crate_root" {
                    format!("{}::ROOT", crate_data.name) // Ensure crate_root is unique per crate
                } else {
                    format!("{}::{}", crate_data.name, module_data.path)
                };

                dot.push_str(&format!(
                    "    \"{}\" [label=\"{}\nCe_m: {}\nCa_m: {}\", fillcolor=\"{}\"];\n",
                    module_node_id, module_data.path, module_data.ce_m, module_data.ca_m, color
                ));
            }
            dot.push_str("  }\n\n");
        }

        for crate_data in &report.crates {
            for module_data in &crate_data.modules {
                let current_module_node_id = if module_data.path == "crate_root" {
                    format!("{}::ROOT", crate_data.name)
                } else {
                    format!("{}::{}", crate_data.name, module_data.path)
                };

                for dep_mod_path_str in &module_data.module_dependencies {
                    // dep_mod_path_str is a fully qualified path like "crate::module::sub"
                    // We need to map this to the DOT node ID format (CrateName::ModulePath or CrateName::ROOT)
                    // This requires knowing which crate dep_mod_path_str belongs to if it's an external dependency.
                    // The current ModuleDependencyVisitor resolves paths within the *same* crate.
                    // For cross-crate module dependencies, this DOT generation might be tricky without more info.
                    // For now, assume module_dependencies are within the same crate or are self-contained FQNs.
                    // If `dep_mod_path_str` starts with "crate::", it implies it's within the *current* crate being processed.
                    // The ModuleDependencyVisitor resolves paths like `crate::foo`, `super::foo`, `self::foo`.
                    // These are resolved to a path like "crate::actual_module_path".
                    // So, `dep_mod_path_str` is effectively "crate::module_name_in_current_crate".
                    // We need to replace the leading "crate" with the actual crate_data.name for the DOT ID.

                    let target_module_node_id = if dep_mod_path_str.starts_with("crate::") {
                        let path_suffix = dep_mod_path_str.trim_start_matches("crate::");
                        if path_suffix.is_empty() || dep_mod_path_str == "crate" {
                            // Dependency on the crate root
                            format!("{}::ROOT", crate_data.name)
                        } else {
                            format!("{}::{}", crate_data.name, path_suffix)
                        }
                    } else if dep_mod_path_str == "crate" {
                        // also crate root
                        format!("{}::ROOT", crate_data.name)
                    } else {
                        // This case implies a path that isn't "crate::foo" or "crate".
                        // It might be a fully qualified path from another crate if the visitor was extended,
                        // or an unresolvable/external path. For now, we'll assume it's resolvable within the current crate.
                        // If module paths are complex, this might need adjustment.
                        // Let's assume dep_mod_path_str is relative to its crate root if not starting with "crate::"
                        // However, ModuleDependencyVisitor normalizes them to start with "crate::" or be "crate"
                        // So this 'else' branch should ideally not be hit for valid internal deps.
                        // For robustness, we could try to find its original crate if we had a global module map.
                        // Sticking to the assumption: dep_mod_path_str is like "crate::path" or "crate"
                        // which is already handled by the above 'if'.
                        // If it's something else, it might be an error or an external unhandled crate.
                        // For now, we will assume all module dependencies are within the same crate.
                        // This is a limitation of the current module dependency visitor.
                        // Let's make it skip if it's not clearly from the current crate context:
                        continue; // Or log a warning.
                    };

                    // Avoid self-loops in visualization if path resolves to same node id
                    if current_module_node_id != target_module_node_id {
                        dot.push_str(&format!(
                            "  \"{current_module_node_id}\" -> \"{target_module_node_id}\";\n"
                        ));
                    }
                }
            }
        }

        dot.push_str("}\n");
        Ok(dot)
    }

    #[tracing::instrument(level = "debug", skip(self, current_dir, base_mod_path, module_map))]
    fn discover_modules(
        &self,
        current_dir: &Path,
        base_mod_path: PathBuf,
        module_map: &mut HashMap<String, PathBuf>,
    ) -> Result<()> {
        if base_mod_path.to_string_lossy() == "crate" {
            let lib_rs = current_dir.join("lib.rs");
            let main_rs = current_dir.join("main.rs");
            if lib_rs.exists() {
                module_map.insert("crate".to_string(), lib_rs.clone());
                self.discover_inline_modules(&lib_rs, "crate", module_map)?;
            } else if main_rs.exists() {
                module_map.insert("crate".to_string(), main_rs.clone());
                self.discover_inline_modules(&main_rs, "crate", module_map)?;
            }
        }
        for entry in WalkDir::new(current_dir).min_depth(1).max_depth(1) {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy();
            if path.is_dir() {
                let mod_name = file_name.into_owned();
                let mod_rs_path = path.join("mod.rs");
                let new_base_mod_path = if base_mod_path.to_string_lossy() == "crate" {
                    PathBuf::from(mod_name.clone())
                } else {
                    base_mod_path.join(mod_name.clone())
                };
                let new_base_mod_path_str = if base_mod_path.to_string_lossy() == "crate" {
                    format!("crate::{mod_name}")
                } else {
                    format!("{}::{}", base_mod_path.to_string_lossy(), mod_name)
                };
                if mod_rs_path.exists() {
                    module_map.insert(new_base_mod_path_str.clone(), mod_rs_path.clone());
                    self.discover_inline_modules(&mod_rs_path, &new_base_mod_path_str, module_map)?;
                    self.discover_modules(path, new_base_mod_path, module_map)?;
                } else {
                    self.discover_modules(path, new_base_mod_path, module_map)?;
                }
            } else if file_name.ends_with(".rs")
                && file_name != "mod.rs"
                && file_name != "lib.rs"
                && file_name != "main.rs"
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                let mod_name = stem.to_string();
                let mod_path_str = if base_mod_path.to_string_lossy() == "crate" {
                    format!("crate::{mod_name}")
                } else {
                    format!("{}::{}", base_mod_path.to_string_lossy(), mod_name)
                };
                if !module_map.contains_key(&mod_path_str) {
                    module_map.insert(mod_path_str.clone(), path.to_path_buf());
                    self.discover_inline_modules(path, &mod_path_str, module_map)?;
                }
            }
        }
        Ok(())
    }

    #[tracing::instrument(
        level = "debug",
        skip(self, file_path, base_module_path_str, _module_map)
    )]
    fn discover_inline_modules(
        &self,
        file_path: &Path,
        base_module_path_str: &str,
        _module_map: &mut HashMap<String, PathBuf>,
    ) -> Result<()> {
        let content = fs::read_to_string(file_path)?;
        match syn::parse_file(&content) {
            Ok(ast) => {
                for item in ast.items {
                    if let Item::Mod(item_mod) = item
                        && item_mod.content.is_some()
                    {
                        let mod_name = item_mod.ident.to_string();
                        let _inline_mod_path_str = if base_module_path_str == "crate" {
                            format!("crate::{mod_name}")
                        } else {
                            format!("{base_module_path_str}::{mod_name}")
                        };
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to parse {} for inline modules: {}. Skipping inline scan for this file.",
                    file_path.display(),
                    e
                );
            }
        }
        Ok(())
    }
}

struct ModuleDependencyVisitor<'a> {
    _crate_name: &'a str,
    current_module_path: Vec<String>,
    module_map: &'a HashMap<String, PathBuf>,
    dependencies: HashSet<String>,
}

impl<'a> Visit<'a> for ModuleDependencyVisitor<'a> {
    fn visit_item_mod(&mut self, item_mod: &'a ItemMod) {
        if item_mod.content.is_some() {
            let mod_name = item_mod.ident.to_string();
            let original_path = self.current_module_path.clone();

            if self.current_module_path.len() == 1 && self.current_module_path[0] == "crate" {
                self.current_module_path = vec!["crate".to_string(), mod_name];
            } else {
                self.current_module_path.push(mod_name);
            }

            syn::visit::visit_item_mod(self, item_mod);
            self.current_module_path = original_path;
        }
    }
    fn visit_item_use(&mut self, i: &'a ItemUse) {
        self.add_dependency_from_path_tree(&i.tree);
        syn::visit::visit_item_use(self, i);
    }
    fn visit_expr_path(&mut self, expr: &'a ExprPath) {
        if let Some(resolved_path) = self.resolve_path(&expr.path) {
            self.dependencies.insert(resolved_path);
        }
        syn::visit::visit_expr_path(self, expr);
    }
    fn visit_pat_type(&mut self, pt: &'a PatType) {
        if let syn::Type::Path(type_path) = &*pt.ty
            && let Some(resolved_path) = self.resolve_path(&type_path.path)
        {
            self.dependencies.insert(resolved_path);
        }
        syn::visit::visit_pat_type(self, pt);
    }
}

impl<'a> ModuleDependencyVisitor<'a> {
    fn new(
        _crate_name: &'a str,
        current_module_path: Vec<String>,
        module_map: &'a HashMap<String, PathBuf>,
        dependencies: HashSet<String>,
    ) -> Self {
        Self {
            _crate_name,
            current_module_path,
            module_map,
            dependencies,
        }
    }
    fn add_dependency_from_path_tree(&mut self, tree: &'a syn::UseTree) {
        match tree {
            syn::UseTree::Path(use_path) => {
                if let Some(resolved_path) = self.resolve_path_from_segments(
                    std::iter::once(&use_path.ident).chain(
                        self.collect_path_segments_from_tree(use_path.tree.as_ref())
                            .iter()
                            .copied(),
                    ),
                ) {
                    self.dependencies.insert(resolved_path);
                }
                match use_path.tree.as_ref() {
                    syn::UseTree::Path(_) | syn::UseTree::Group(_) => {
                        self.add_dependency_from_path_tree(&use_path.tree);
                    }
                    _ => {}
                }
            }
            syn::UseTree::Name(_use_name) => {}
            syn::UseTree::Rename(_use_rename) => {}
            syn::UseTree::Glob(_use_glob) => {
                if let Some(_resolved_path) = self.resolve_path_from_segments(std::iter::empty()) {}
            }
            syn::UseTree::Group(use_group) => {
                for item_tree in &use_group.items {
                    self.add_dependency_from_path_tree(item_tree);
                }
            }
        }
    }
    fn collect_path_segments_from_tree(&self, tree: &'a syn::UseTree) -> Vec<&'a syn::Ident> {
        let mut segments = Vec::new();
        let mut current_tree = tree;
        loop {
            match current_tree {
                syn::UseTree::Path(p) => {
                    segments.push(&p.ident);
                    current_tree = &p.tree;
                }
                syn::UseTree::Name(n) => {
                    segments.push(&n.ident);
                    break;
                }
                _ => break,
            }
        }
        segments
    }
    fn resolve_path(&self, path: &'a syn::Path) -> Option<String> {
        if path.leading_colon.is_some()
            && (path.segments.is_empty() || path.segments[0].ident != "crate")
        {
            return None;
        }
        let segments: Vec<&syn::Ident> = path.segments.iter().map(|s| &s.ident).collect();
        self.resolve_path_from_segments(segments.into_iter())
    }
    fn resolve_path_from_segments<I>(&self, segments_iter: I) -> Option<String>
    where
        I: Iterator<Item = &'a syn::Ident> + Clone,
    {
        let segments: Vec<String> = segments_iter.map(|s| s.to_string()).collect();
        if segments.is_empty() {
            return None;
        }
        let mut resolved_path_parts: Vec<String> = Vec::new();
        let first_segment = &segments[0];
        match first_segment.as_str() {
            "crate" => {
                resolved_path_parts.push("crate".to_string());
                resolved_path_parts.extend(segments.iter().skip(1).cloned());
            }
            "self" => {
                resolved_path_parts.extend(self.current_module_path.iter().cloned());
                resolved_path_parts.extend(segments.iter().skip(1).cloned());
            }
            "super" => {
                if self.current_module_path.len() > 1 {
                    resolved_path_parts.extend(
                        self.current_module_path
                            .iter()
                            .take(self.current_module_path.len() - 1)
                            .cloned(),
                    );
                    resolved_path_parts.extend(segments.iter().skip(1).cloned());
                } else {
                    return None;
                }
            }
            _ => {
                let mut temp_path_parts = self.current_module_path.clone();
                temp_path_parts.extend(segments.iter().cloned());
                if self.is_valid_module_prefix(&temp_path_parts) {
                    resolved_path_parts = temp_path_parts;
                } else {
                    let mut crate_root_path = vec!["crate".to_string()];
                    crate_root_path.extend(segments.iter().cloned());
                    if self.is_valid_module_prefix(&crate_root_path) {
                        resolved_path_parts = crate_root_path;
                    } else {
                        return None;
                    }
                }
            }
        }
        let mut current_check_path = resolved_path_parts.clone();
        while !current_check_path.is_empty() {
            let path_str = current_check_path.join("::");
            if self.module_map.contains_key(&path_str) {
                if path_str != self.current_module_path.join("::") {
                    return Some(path_str);
                }
                return None;
            }
            current_check_path.pop();
        }
        None
    }
    fn is_valid_module_prefix(&self, path_parts: &[String]) -> bool {
        let query_prefix = path_parts.join("::");
        if path_parts.is_empty() {
            return false;
        }
        self.module_map.keys().any(|known_mod_path| {
            known_mod_path == &query_prefix
                || known_mod_path.starts_with(&(query_prefix.clone() + "::"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Constructor tests
    #[test]
    fn test_coupling_rule_new_creates_instance() {
        let rule = CouplingRule::new();
        // CouplingRule is a zero-sized struct, so we just verify it can be created
        let _ = rule;
    }

    #[test]
    fn test_coupling_rule_default_creates_instance() {
        let _rule = CouplingRule;
    }

    // Data structure tests
    #[test]
    fn test_crate_coupling_new_with_default_fields() {
        let coupling = CrateCoupling {
            name: "test_crate".to_string(),
            ce: 5,
            ca: 3,
            modules: Vec::new(),
            dependencies: HashSet::new(),
        };
        assert_eq!(coupling.name, "test_crate");
        assert_eq!(coupling.ce, 5);
        assert_eq!(coupling.ca, 3);
        assert!(coupling.modules.is_empty());
        assert!(coupling.dependencies.is_empty());
    }

    #[test]
    fn test_crate_coupling_with_modules() {
        let module = ModuleCoupling {
            path: "test::module".to_string(),
            ce_m: 2,
            ca_m: 1,
            module_dependencies: HashSet::new(),
        };
        let coupling = CrateCoupling {
            name: "test_crate".to_string(),
            ce: 5,
            ca: 3,
            modules: vec![module.clone()],
            dependencies: HashSet::new(),
        };
        assert_eq!(coupling.modules.len(), 1);
        assert_eq!(coupling.modules[0].path, "test::module");
    }

    #[test]
    fn test_crate_coupling_with_dependencies() {
        let mut deps = HashSet::new();
        deps.insert("dep1".to_string());
        deps.insert("dep2".to_string());
        let coupling = CrateCoupling {
            name: "test_crate".to_string(),
            ce: 2,
            ca: 0,
            modules: Vec::new(),
            dependencies: deps.clone(),
        };
        assert_eq!(coupling.dependencies.len(), 2);
        assert!(coupling.dependencies.contains("dep1"));
        assert!(coupling.dependencies.contains("dep2"));
    }

    #[test]
    fn test_module_coupling_default_creates_empty_instance() {
        let coupling = ModuleCoupling::default();
        assert_eq!(coupling.path, "");
        assert_eq!(coupling.ce_m, 0);
        assert_eq!(coupling.ca_m, 0);
        assert!(coupling.module_dependencies.is_empty());
    }

    #[test]
    fn test_module_coupling_with_values() {
        let mut deps = HashSet::new();
        deps.insert("crate::other".to_string());
        let coupling = ModuleCoupling {
            path: "crate::test_module".to_string(),
            ce_m: 3,
            ca_m: 2,
            module_dependencies: deps.clone(),
        };
        assert_eq!(coupling.path, "crate::test_module");
        assert_eq!(coupling.ce_m, 3);
        assert_eq!(coupling.ca_m, 2);
        assert_eq!(coupling.module_dependencies.len(), 1);
    }

    #[test]
    fn test_coupling_data_new() {
        let data = CouplingData {
            crates: Vec::new(),
            granularity: CouplingGranularity::Crate,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        assert!(data.crates.is_empty());
        assert_eq!(data.granularity, CouplingGranularity::Crate);
        assert_eq!(data.analysis_path, PathBuf::from("/test/path"));
    }

    // Serialization tests
    #[test]
    fn test_crate_coupling_is_serializable() {
        let coupling = CrateCoupling {
            name: "test_crate".to_string(),
            ce: 5,
            ca: 3,
            modules: vec![ModuleCoupling {
                path: "test::module".to_string(),
                ce_m: 2,
                ca_m: 1,
                module_dependencies: HashSet::new(),
            }],
            dependencies: {
                let mut deps = HashSet::new();
                deps.insert("dep1".to_string());
                deps
            },
        };
        let json = serde_json::to_string(&coupling);
        assert!(json.is_ok(), "CrateCoupling should be serializable to JSON");
    }

    #[test]
    fn test_module_coupling_is_serializable() {
        let coupling = ModuleCoupling {
            path: "test::module".to_string(),
            ce_m: 2,
            ca_m: 1,
            module_dependencies: {
                let mut deps = HashSet::new();
                deps.insert("crate::other".to_string());
                deps
            },
        };
        let json = serde_json::to_string(&coupling);
        assert!(
            json.is_ok(),
            "ModuleCoupling should be serializable to JSON"
        );
    }

    #[test]
    fn test_coupling_data_is_serializable() {
        let data = CouplingData {
            crates: vec![CrateCoupling {
                name: "test_crate".to_string(),
                ce: 5,
                ca: 3,
                modules: Vec::new(),
                dependencies: HashSet::new(),
            }],
            granularity: CouplingGranularity::Crate,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let json = serde_json::to_string(&data);
        assert!(json.is_ok(), "CouplingData should be serializable to JSON");
    }

    #[test]
    fn test_crate_coupling_json_roundtrip() {
        let original = CrateCoupling {
            name: "test_crate".to_string(),
            ce: 5,
            ca: 3,
            modules: vec![ModuleCoupling {
                path: "test::module".to_string(),
                ce_m: 2,
                ca_m: 1,
                module_dependencies: {
                    let mut deps = HashSet::new();
                    deps.insert("crate::other".to_string());
                    deps
                },
            }],
            dependencies: {
                let mut deps = HashSet::new();
                deps.insert("dep1".to_string());
                deps.insert("dep2".to_string());
                deps
            },
        };
        let json = serde_json::to_string(&original).expect("Serialization should succeed");
        let deserialized: CrateCoupling =
            serde_json::from_str(&json).expect("Deserialization should succeed");
        assert_eq!(deserialized.name, original.name);
        assert_eq!(deserialized.ce, original.ce);
        assert_eq!(deserialized.ca, original.ca);
        assert_eq!(deserialized.modules.len(), original.modules.len());
        assert_eq!(deserialized.dependencies.len(), original.dependencies.len());
    }

    #[test]
    fn test_coupling_data_yaml_serialization() {
        let data = CouplingData {
            crates: vec![CrateCoupling {
                name: "test_crate".to_string(),
                ce: 5,
                ca: 3,
                modules: Vec::new(),
                dependencies: HashSet::new(),
            }],
            granularity: CouplingGranularity::Module,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let yaml = serde_yaml::to_string(&data);
        assert!(yaml.is_ok(), "CouplingData should be serializable to YAML");
    }

    // Pure function tests
    #[test]
    fn test_get_dot_color_with_zero_max_value() {
        let color = CouplingRule::get_dot_color(100.0, 0.0);
        assert_eq!(color, "0.33,1.0,0.7".to_string());
    }

    #[test]
    fn test_get_dot_color_with_zero_value() {
        let color = CouplingRule::get_dot_color(0.0, 100.0);
        assert_eq!(color, "0.33,1.0,0.5".to_string());
    }

    #[test]
    fn test_get_dot_color_with_half_max_value() {
        let color = CouplingRule::get_dot_color(50.0, 100.0);
        assert_eq!(color, "0.17,1.0,0.5".to_string());
    }

    #[test]
    fn test_get_dot_color_with_max_value() {
        let color = CouplingRule::get_dot_color(100.0, 100.0);
        assert_eq!(color, "0.00,1.0,0.5".to_string());
    }

    #[test]
    fn test_get_dot_color_clamps_high_values() {
        let color = CouplingRule::get_dot_color(150.0, 100.0);
        // Should be clamped to 1.0, resulting in red
        assert_eq!(color, "0.00,1.0,0.5".to_string());
    }

    #[test]
    fn test_get_dot_color_green_for_low_coupling() {
        let color = CouplingRule::get_dot_color(1.0, 100.0);
        // Should be close to green
        assert!(color.starts_with("0.3"));
    }

    #[test]
    fn test_get_dot_color_red_for_high_coupling() {
        let color = CouplingRule::get_dot_color(99.0, 100.0);
        // Should be close to red
        assert!(color.starts_with("0.0") || color.starts_with("0.1"));
    }

    // HTML rendering tests
    #[test]
    fn test_render_coupling_html_body_produces_valid_markup() {
        let rule = CouplingRule::new();
        let report = CouplingData {
            crates: vec![CrateCoupling {
                name: "test_crate".to_string(),
                ce: 5,
                ca: 3,
                modules: Vec::new(),
                dependencies: {
                    let mut deps = HashSet::new();
                    deps.insert("dep1".to_string());
                    deps
                },
            }],
            granularity: CouplingGranularity::Crate,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let result = rule.render_coupling_html_body(&report);
        assert!(result.is_ok(), "HTML rendering should succeed");
        let markup = result.unwrap();
        let html_string = markup.into_string();
        assert!(
            html_string.contains("<table"),
            "Should contain a table element"
        );
        assert!(
            html_string.contains("test_crate"),
            "Should contain crate name"
        );
    }

    #[test]
    fn test_render_coupling_html_body_with_module_granularity() {
        let rule = CouplingRule::new();
        let report = CouplingData {
            crates: vec![CrateCoupling {
                name: "test_crate".to_string(),
                ce: 5,
                ca: 3,
                modules: vec![ModuleCoupling {
                    path: "test_module".to_string(),
                    ce_m: 2,
                    ca_m: 1,
                    module_dependencies: HashSet::new(),
                }],
                dependencies: HashSet::new(),
            }],
            granularity: CouplingGranularity::Module,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let result = rule.render_coupling_html_body(&report);
        assert!(result.is_ok());
        let html_string = result.unwrap().into_string();
        assert!(html_string.contains("test_module"));
    }

    #[test]
    fn test_render_coupling_html_body_with_both_granularity() {
        let rule = CouplingRule::new();
        let report = CouplingData {
            crates: vec![CrateCoupling {
                name: "test_crate".to_string(),
                ce: 5,
                ca: 3,
                modules: vec![ModuleCoupling {
                    path: "test_module".to_string(),
                    ce_m: 2,
                    ca_m: 1,
                    module_dependencies: HashSet::new(),
                }],
                dependencies: HashSet::new(),
            }],
            granularity: CouplingGranularity::Both,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let result = rule.render_coupling_html_body(&report);
        assert!(result.is_ok());
        let html_string = result.unwrap().into_string();
        // Should have both crate and module level headers
        assert!(html_string.contains("Crate Level"));
        assert!(html_string.contains("Module Level"));
    }

    #[test]
    fn test_render_coupling_html_body_with_empty_crates() {
        let rule = CouplingRule::new();
        let report = CouplingData {
            crates: Vec::new(),
            granularity: CouplingGranularity::Crate,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let result = rule.render_coupling_html_body(&report);
        assert!(result.is_ok());
    }

    // DOT generation tests
    #[test]
    fn test_generate_crate_dot_produces_valid_graph() {
        let rule = CouplingRule::new();
        let report = CouplingData {
            crates: vec![CrateCoupling {
                name: "test_crate".to_string(),
                ce: 5,
                ca: 3,
                modules: Vec::new(),
                dependencies: {
                    let mut deps = HashSet::new();
                    deps.insert("other_crate".to_string());
                    deps
                },
            }],
            granularity: CouplingGranularity::Crate,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let result = rule.generate_crate_dot(&report);
        assert!(result.is_ok(), "DOT generation should succeed");
        let dot = result.unwrap();
        assert!(dot.contains("digraph CrateCoupling"), "Should be a digraph");
        assert!(dot.contains("test_crate"), "Should contain crate name");
        assert!(dot.contains("other_crate"), "Should contain dependency");
    }

    #[test]
    fn test_generate_crate_dot_with_empty_crates() {
        let rule = CouplingRule::new();
        let report = CouplingData {
            crates: Vec::new(),
            granularity: CouplingGranularity::Crate,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let result = rule.generate_crate_dot(&report);
        assert!(result.is_ok());
        let dot = result.unwrap();
        assert!(dot.contains("digraph CrateCoupling"));
        assert!(dot.contains("}"));
    }

    #[test]
    fn test_generate_module_dot_produces_valid_graph() {
        let rule = CouplingRule::new();
        let report = CouplingData {
            crates: vec![CrateCoupling {
                name: "test_crate".to_string(),
                ce: 0,
                ca: 0,
                modules: vec![ModuleCoupling {
                    path: "test_module".to_string(),
                    ce_m: 1,
                    ca_m: 0,
                    module_dependencies: {
                        let mut deps = HashSet::new();
                        deps.insert("crate::other_module".to_string());
                        deps
                    },
                }],
                dependencies: HashSet::new(),
            }],
            granularity: CouplingGranularity::Module,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let result = rule.generate_module_dot(&report);
        assert!(result.is_ok(), "Module DOT generation should succeed");
        let dot = result.unwrap();
        assert!(dot.contains("digraph ModuleCoupling"));
        assert!(dot.contains("test_module"));
    }

    #[test]
    fn test_generate_module_dot_with_empty_modules() {
        let rule = CouplingRule::new();
        let report = CouplingData {
            crates: vec![CrateCoupling {
                name: "test_crate".to_string(),
                ce: 0,
                ca: 0,
                modules: Vec::new(),
                dependencies: HashSet::new(),
            }],
            granularity: CouplingGranularity::Module,
            analysis_path: PathBuf::from("/test/path"),
            sdp_violations: Vec::new(),
        };
        let result = rule.generate_module_dot(&report);
        assert!(result.is_ok());
        let dot = result.unwrap();
        assert!(dot.contains("digraph ModuleCoupling"));
        // Should not have subgraphs for empty modules
        assert!(!dot.contains("subgraph"));
    }

    // Clone tests for data structures
    #[test]
    fn test_module_coupling_clone_creates_independent_copy() {
        let mut deps = HashSet::new();
        deps.insert("crate::dep1".to_string());
        let original = ModuleCoupling {
            path: "test::module".to_string(),
            ce_m: 2,
            ca_m: 1,
            module_dependencies: deps.clone(),
        };
        let cloned = original.clone();
        assert_eq!(cloned.path, original.path);
        assert_eq!(cloned.ce_m, original.ce_m);
        assert_eq!(cloned.ca_m, original.ca_m);
        assert_eq!(cloned.module_dependencies, original.module_dependencies);
    }

    #[test]
    fn test_crate_coupling_clone_creates_independent_copy() {
        let mut deps = HashSet::new();
        deps.insert("dep1".to_string());
        let original = CrateCoupling {
            name: "test_crate".to_string(),
            ce: 5,
            ca: 3,
            modules: vec![ModuleCoupling {
                path: "test::module".to_string(),
                ce_m: 2,
                ca_m: 1,
                module_dependencies: HashSet::new(),
            }],
            dependencies: deps.clone(),
        };
        let cloned = original.clone();
        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.ce, original.ce);
        assert_eq!(cloned.ca, original.ca);
        assert_eq!(cloned.modules.len(), original.modules.len());
        assert_eq!(cloned.dependencies, original.dependencies);
    }

    // Edge case tests
    #[test]
    fn test_crate_coupling_with_zero_coupling() {
        let coupling = CrateCoupling {
            name: "stable_crate".to_string(),
            ce: 0,
            ca: 0,
            modules: Vec::new(),
            dependencies: HashSet::new(),
        };
        assert_eq!(coupling.ce, 0);
        assert_eq!(coupling.ca, 0);
    }

    #[test]
    fn test_module_coupling_with_zero_coupling() {
        let coupling = ModuleCoupling {
            path: "isolated_module".to_string(),
            ce_m: 0,
            ca_m: 0,
            module_dependencies: HashSet::new(),
        };
        assert_eq!(coupling.ce_m, 0);
        assert_eq!(coupling.ca_m, 0);
        assert!(coupling.module_dependencies.is_empty());
    }

    #[test]
    fn test_coupling_data_with_both_granularity() {
        let data = CouplingData {
            crates: vec![CrateCoupling {
                name: "test_crate".to_string(),
                ce: 5,
                ca: 3,
                modules: vec![ModuleCoupling {
                    path: "test::module".to_string(),
                    ce_m: 2,
                    ca_m: 1,
                    module_dependencies: HashSet::new(),
                }],
                dependencies: HashSet::new(),
            }],
            granularity: CouplingGranularity::Both,
            analysis_path: PathBuf::from("/test"),
            sdp_violations: Vec::new(),
        };
        assert_eq!(data.granularity, CouplingGranularity::Both);
        assert_eq!(data.crates.len(), 1);
        assert_eq!(data.crates[0].modules.len(), 1);
    }

    // Output format tests
    #[test]
    fn test_coupling_granularity_variants() {
        // Test that all granularity variants can be created
        let _ = CouplingGranularity::Crate;
        let _ = CouplingGranularity::Module;
        let _ = CouplingGranularity::Both;
    }

    // Tests for the Rule trait implementation
    use crate::rule::Rule;

    #[test]
    fn test_rule_name_returns_coupling() {
        assert_eq!(
            CouplingRule::name(),
            "coupling",
            "Rule name should be 'coupling'"
        );
    }

    #[test]
    fn test_rule_description_returns_meaningful_text() {
        let description = CouplingRule::description();
        assert!(
            !description.is_empty(),
            "Rule description should not be empty"
        );
        assert!(
            description.contains("coupling") || description.contains("coupl"),
            "Rule description should describe the rule's purpose"
        );
    }

    #[test]
    fn test_rule_trait_run_delegates_correctly() {
        // Create a minimal test directory structure
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp directory");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src directory");

        // Create a minimal Cargo.toml
        let cargo_toml = r#"
[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"

[workspace]
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml)
            .expect("Failed to write Cargo.toml");

        // Create a simple main.rs file
        let main_rs = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        fs::write(src_dir.join("main.rs"), main_rs).expect("Failed to write main.rs");

        let rule = CouplingRule::new();
        let args = CouplingArgs {
            path: temp_dir.path().to_path_buf(),
            output: CouplingOutputFormat::Table,
            granularity: CouplingGranularity::Crate,
            ci_output: None,
            output_file: None,
            staged: false,
        };

        // Call the Rule trait's run method
        let result = <CouplingRule as Rule>::run(&rule, &args);

        assert!(
            result.is_ok(),
            "Rule trait run method should succeed with valid input: {:?}",
            result
        );
    }

    #[test]
    fn test_rule_trait_analyze_returns_correct_data_type() {
        // Create a minimal test directory structure
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp directory");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src directory");

        // Create a minimal Cargo.toml
        let cargo_toml = r#"
[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"

[workspace]
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml)
            .expect("Failed to write Cargo.toml");

        // Create a simple main.rs file
        let main_rs = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        fs::write(src_dir.join("main.rs"), main_rs).expect("Failed to write main.rs");

        let rule = CouplingRule::new();
        let args = CouplingArgs {
            path: temp_dir.path().to_path_buf(),
            output: CouplingOutputFormat::Table,
            granularity: CouplingGranularity::Crate,
            ci_output: None,
            output_file: None,
            staged: false,
        };

        // Call the Rule trait's analyze method
        let result = <CouplingRule as Rule>::analyze(&rule, &args);

        assert!(
            result.is_ok(),
            "Rule trait analyze method should succeed with valid input: {:?}",
            result
        );

        let data = result.unwrap();
        assert_eq!(
            data.granularity,
            CouplingGranularity::Crate,
            "Analyzed data should have the correct granularity"
        );
        assert_eq!(
            data.analysis_path,
            temp_dir.path(),
            "Analyzed data should have the correct analysis path"
        );
    }

    #[test]
    fn test_rule_trait_analyze_fails_with_nonexistent_directory() {
        let rule = CouplingRule::new();
        let fake_path = PathBuf::from("/nonexistent/path/that/does/not/exist");
        let args = CouplingArgs {
            path: fake_path,
            output: CouplingOutputFormat::Table,
            granularity: CouplingGranularity::Crate,
            ci_output: None,
            output_file: None,
            staged: false,
        };

        // Call the Rule trait's analyze method
        let result = <CouplingRule as Rule>::analyze(&rule, &args);

        assert!(
            result.is_err(),
            "Rule trait analyze method should fail with nonexistent path"
        );
    }

    #[test]
    fn test_rule_associated_types_match() {
        // This test verifies that the associated types are correctly set
        // It's a compile-time check; if it compiles, the types are correct
        let rule = CouplingRule::new();

        // Verify Config type is CouplingArgs
        let config = CouplingArgs {
            path: PathBuf::from("."),
            output: CouplingOutputFormat::Table,
            granularity: CouplingGranularity::Crate,
            ci_output: None,
            output_file: None,
            staged: false,
        };

        // Verify Data type is CouplingData
        let _config_check: <CouplingRule as Rule>::Config = config;
        // We can't directly check Data type without an instance, but the
        // analyze method returning Result<CouplingData> confirms it

        // Verify run and analyze work with these types
        let _ = rule;
    }

    // Tests for CI output functionality

    fn crate_coupling(name: &str, ce: usize, ca: usize, dependencies: &[&str]) -> CrateCoupling {
        CrateCoupling {
            name: name.to_string(),
            ce,
            ca,
            modules: Vec::new(),
            dependencies: dependencies.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn sdp_findings(crates: &[CrateCoupling], affected: Option<&HashSet<String>>) -> Vec<Finding> {
        CouplingData {
            crates: Vec::new(),
            granularity: CouplingGranularity::Crate,
            analysis_path: PathBuf::from("/test"),
            sdp_violations: find_sdp_violations(crates, affected),
        }
        .to_findings()
    }

    #[test]
    fn test_to_findings_ignores_maximally_unstable_binary_depending_on_stable_crate() {
        let crates = [
            crate_coupling("server", 1, 0, &["core"]), // I = 1.0
            crate_coupling("core", 0, 1, &[]),         // I = 0.0
        ];

        assert!(
            sdp_findings(&crates, None).is_empty(),
            "high instability alone must not produce a finding"
        );
    }

    #[test]
    fn test_to_findings_reports_stable_crate_depending_on_less_stable_crate() {
        let crates = [
            crate_coupling("core", 1, 4, &["plugin"]), // I = 0.20
            crate_coupling("plugin", 3, 1, &[]),       // I = 0.75
        ];

        let findings = sdp_findings(&crates, None);

        assert_eq!(findings.len(), 1, "one SDP violation expected");
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "coupling");
        assert_eq!(finding.rule_name, "Code Coupling Rule");
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(
            finding.message,
            "Crate 'core' (I=0.20) depends on less stable crate 'plugin' (I=0.75), violating the Stable Dependencies Principle"
        );
        assert_eq!(
            finding.fingerprint.as_deref(),
            Some("coupling-sdp:core:plugin")
        );
        assert!(finding.location.is_none());
    }

    #[test]
    fn test_to_findings_ignores_dependency_with_equal_instability() {
        let crates = [
            crate_coupling("left", 1, 1, &["right"]), // I = 0.5
            crate_coupling("right", 2, 2, &[]),       // I = 0.5
        ];

        assert!(
            sdp_findings(&crates, None).is_empty(),
            "equally stable dependencies do not violate the SDP"
        );
    }

    #[test]
    fn test_to_findings_orders_violations_by_dependant_then_dependency() {
        let crates = [
            crate_coupling("zeta", 1, 9, &["unstable_b", "unstable_a"]), // I = 0.10
            crate_coupling("alpha", 1, 9, &["unstable_b", "unstable_a"]), // I = 0.10
            crate_coupling("unstable_a", 9, 1, &[]),                     // I = 0.90
            crate_coupling("unstable_b", 9, 1, &[]),                     // I = 0.90
        ];

        let fingerprints: Vec<String> = sdp_findings(&crates, None)
            .into_iter()
            .filter_map(|finding| finding.fingerprint)
            .collect();

        assert_eq!(
            fingerprints,
            [
                "coupling-sdp:alpha:unstable_a",
                "coupling-sdp:alpha:unstable_b",
                "coupling-sdp:zeta:unstable_a",
                "coupling-sdp:zeta:unstable_b",
            ]
        );
    }

    #[test]
    fn test_to_findings_in_staged_mode_reports_edges_touching_an_affected_crate() {
        // stable_core (I = 0.25) -> volatile_ui (I = 0.75) violates the SDP;
        // unrelated is part of the graph but not of that edge.
        let crates = [
            crate_coupling("stable_core", 1, 3, &["volatile_ui"]),
            crate_coupling("volatile_ui", 3, 1, &[]),
            crate_coupling("unrelated", 0, 0, &[]),
        ];
        let affected = |names: &[&str]| -> HashSet<String> {
            names.iter().map(|name| name.to_string()).collect()
        };

        for staged in [["stable_core"], ["volatile_ui"]] {
            let findings = sdp_findings(&crates, Some(&affected(&staged)));
            assert_eq!(
                findings
                    .iter()
                    .filter_map(|finding| finding.fingerprint.as_deref())
                    .collect::<Vec<_>>(),
                ["coupling-sdp:stable_core:volatile_ui"],
                "staging {staged:?} should report the edge"
            );
        }
        assert!(sdp_findings(&crates, Some(&affected(&["unrelated"]))).is_empty());
    }

    #[test]
    fn test_analyze_ignores_dev_dependency_edges() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp directory");
        let root = temp_dir.path();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\", \"fixtures\"]\nresolver = \"2\"\n",
        )
        .expect("Failed to write workspace Cargo.toml");
        for (name, manifest_extra) in [
            (
                "app",
                "[dev-dependencies]\nfixtures = { path = \"../fixtures\" }\n",
            ),
            ("fixtures", ""),
        ] {
            let crate_dir = root.join(name);
            fs::create_dir_all(crate_dir.join("src")).expect("Failed to create crate src");
            fs::write(
                crate_dir.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n{manifest_extra}"
                ),
            )
            .expect("Failed to write crate Cargo.toml");
            fs::write(crate_dir.join("src/lib.rs"), "").expect("Failed to write lib.rs");
        }

        let data = CouplingRule::new()
            .analyze(&CouplingArgs {
                path: root.to_path_buf(),
                output: CouplingOutputFormat::Json,
                granularity: CouplingGranularity::Crate,
                ci_output: None,
                output_file: None,
                staged: false,
            })
            .expect("coupling analysis should succeed");

        for krate in &data.crates {
            assert!(
                krate.dependencies.is_empty(),
                "'{}' should have no edges, got {:?}",
                krate.name,
                krate.dependencies
            );
            assert_eq!((krate.ce, krate.ca), (0, 0), "'{}' coupling", krate.name);
        }
        assert_eq!(data.crates.len(), 2);
        assert!(data.to_findings().is_empty());
    }

    #[test]
    fn test_run_with_ci_output_sarif_succeeds() {
        // Create a minimal test directory structure
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp directory");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src directory");

        // Create a minimal Cargo.toml
        let cargo_toml = r#"
[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"

[workspace]
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml)
            .expect("Failed to write Cargo.toml");

        // Create a simple main.rs file
        let main_rs = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        fs::write(src_dir.join("main.rs"), main_rs).expect("Failed to write main.rs");

        let rule = CouplingRule::new();
        let args = CouplingArgs {
            path: temp_dir.path().to_path_buf(),
            output: CouplingOutputFormat::Table,
            granularity: CouplingGranularity::Crate,
            ci_output: Some(CiOutputFormat::Sarif),
            output_file: None,
            staged: false,
        };

        let result = rule.run(&args);

        assert!(result.is_ok(), "run with SARIF CI output should succeed");
    }

    #[test]
    fn test_run_with_ci_output_junit_succeeds() {
        // Create a minimal test directory structure
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp directory");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src directory");

        // Create a minimal Cargo.toml
        let cargo_toml = r#"
[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"

[workspace]
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml)
            .expect("Failed to write Cargo.toml");

        // Create a simple main.rs file
        let main_rs = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        fs::write(src_dir.join("main.rs"), main_rs).expect("Failed to write main.rs");

        let rule = CouplingRule::new();
        let args = CouplingArgs {
            path: temp_dir.path().to_path_buf(),
            output: CouplingOutputFormat::Table,
            granularity: CouplingGranularity::Crate,
            ci_output: Some(CiOutputFormat::JUnit),
            output_file: None,
            staged: false,
        };

        let result = rule.run(&args);

        assert!(result.is_ok(), "run with JUnit CI output should succeed");
    }

    #[test]
    fn test_run_with_ci_output_does_not_fail_on_warnings() {
        // Create a minimal test directory structure
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp directory");
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).expect("Failed to create src directory");

        // Create a minimal Cargo.toml
        let cargo_toml = r#"
[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"

[workspace]
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml)
            .expect("Failed to write Cargo.toml");

        // Create a simple main.rs file
        let main_rs = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        fs::write(src_dir.join("main.rs"), main_rs).expect("Failed to write main.rs");

        let rule = CouplingRule::new();
        let args = CouplingArgs {
            path: temp_dir.path().to_path_buf(),
            output: CouplingOutputFormat::Table,
            granularity: CouplingGranularity::Crate,
            ci_output: Some(CiOutputFormat::Sarif),
            output_file: None,
            staged: false,
        };

        let result = rule.run(&args);

        // Coupling uses Warning severity, so it should not fail even with findings
        assert!(
            result.is_ok(),
            "run with CI output should succeed even with warnings"
        );
    }
}
