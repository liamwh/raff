use anyhow::{Context, Result};
use prettytable::{Cell, Row, Table};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::{visit::Visit, ExprPath, Item, ItemMod, ItemUse, PatType};
use walkdir::WalkDir;

use crate::cli::{CouplingArgs, CouplingGranularity, CouplingOutputFormat};
use crate::table_utils::get_default_table_format;

#[derive(Debug, Deserialize, Clone)]
struct CargoMetadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Option<Resolve>, // Optional: only present if --no-deps is NOT used
}

#[derive(Debug, Deserialize, Clone)]
struct Package {
    id: String,
    name: String,
    dependencies: Vec<Dependency>,
    manifest_path: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Hash, Clone)]
struct PkgId(String); // To use package id string in HashSet

#[derive(Debug, Deserialize, Clone)]
struct Dependency {
    name: String,
    // The actual package id this dependency resolves to is not directly here with --no-deps.
    // We need to match by name with workspace_members or rely on resolve graph if available.
    // For inter-workspace dependencies, pkg_id is usually not present in this dependency struct itself.
    // Instead, cargo metadata --no-deps gives a flat list of packages.
    // We will iterate over a package's dependencies and check if the dependency *name*
    // corresponds to another workspace package.
    // The `pkg` field in dependency is often not populated with --no-deps.
    // We'll need to look up the dependency name in the list of all packages.
    // However, cargo metadata's `dependencies` objects do not have a direct `id` field to link
    // to the `id` field in the top-level `packages` array when using `--no-deps`.
    // The `id` field of a package is of the form "pkg_name version (path/url)".
    // The `workspace_members` array contains these full IDs.
    // For a dependency listed in a package, we typically get its `name`.
    // We need to find a workspace package whose `name` matches the dependency's `name`.
}

// Only used if we don't use --no-deps, which is not the primary strategy here for Step A.1.
// However, if --no-deps proves insufficient for resolving package IDs for dependencies,
// we might need to parse the resolve graph.
#[derive(Debug, Deserialize, Clone)]
struct Resolve {
    nodes: Vec<ResolveNode>,
}

#[derive(Debug, Deserialize, Clone)]
struct ResolveNode {
    id: String, // Package ID
    dependencies: Vec<String>, // List of Package IDs this node depends on
                // deps: Vec<ResolveDep>, // Alternative structure for dependencies with names
}

// --- New Data Structures for Coupling Report ---

#[derive(Debug)]
struct CrateLevelAnalysisResult {
    crate_couplings_map: HashMap<String, CrateCoupling>,
    workspace_packages_map: HashMap<String, Package>,
    package_id_to_name: HashMap<String, String>,
    workspace_member_ids: HashSet<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct CrateCoupling {
    name: String,
    ce: usize, // Efferent Couplings
    ca: usize, // Afferent Couplings
    modules: Vec<ModuleCoupling>,
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct ModuleCoupling {
    path: String, // Module path, e.g., "crate_root", "utils", "services::auth"
    ce_m: usize,  // Module Efferent Couplings
    ca_m: usize,  // Module Afferent Couplings
}

#[derive(Serialize, Debug, Default)]
pub struct CouplingReport {
    crates: Vec<CrateCoupling>,
}

pub struct CouplingRule;

impl CouplingRule {
    pub fn new() -> Self {
        Self
    }

    #[tracing::instrument(level = "debug", skip(self, args), ret)]
    pub fn run(&self, args: &CouplingArgs) -> Result<()> {
        let analysis_result = self.analyze_crate_level_coupling(args)?;
        let crate_couplings_map = analysis_result.crate_couplings_map;
        let workspace_packages_map = analysis_result.workspace_packages_map;
        let package_id_to_name = analysis_result.package_id_to_name;
        let workspace_member_ids = analysis_result.workspace_member_ids;

        let mut full_report = CouplingReport { crates: Vec::new() };

        let mut sorted_workspace_pkg_ids: Vec<_> = workspace_member_ids.iter().cloned().collect();
        sorted_workspace_pkg_ids
            .sort_by_key(|id| package_id_to_name.get(id).cloned().unwrap_or_default());

        for pkg_id in sorted_workspace_pkg_ids {
            if let Some(pkg_data) = workspace_packages_map.get(&pkg_id) {
                let crate_name = &pkg_data.name;
                let mut current_crate_coupling = crate_couplings_map
                    .get(crate_name)
                    .cloned()
                    .unwrap_or_else(|| CrateCoupling {
                        name: crate_name.clone(),
                        ce: 0,
                        ca: 0,
                        modules: Vec::new(),
                    });

                // Conditionally run module analysis
                if matches!(
                    args.granularity,
                    CouplingGranularity::Module | CouplingGranularity::Both
                ) {
                    let manifest_path = PathBuf::from(&pkg_data.manifest_path);
                    if let Some(crate_root_dir) = manifest_path.parent() {
                        let src_path = crate_root_dir.join("src");
                        if src_path.exists() {
                            let mut module_couplings = self
                                .analyze_module_level_coupling_for_crate(
                                    crate_name, &src_path, pkg_data,
                                )?;
                            // Sort modules by total coupling (ce_m + ca_m) descending
                            module_couplings
                                .sort_by(|a, b| (b.ce_m + b.ca_m).cmp(&(a.ce_m + a.ca_m)));
                            current_crate_coupling.modules = module_couplings;
                        }
                    }
                }
                full_report.crates.push(current_crate_coupling);
            }
        }

        // Sort crates by total coupling (ce + ca) descending
        full_report
            .crates
            .sort_by(|a, b| (b.ce + b.ca).cmp(&(a.ce + a.ca)));

        match args.output {
            CouplingOutputFormat::Table => {
                self.print_table_report(&full_report, &args.granularity)?;
            }
            CouplingOutputFormat::Json => {
                let json = serde_json::to_string_pretty(&full_report)?;
                println!("{}", json);
            }
            CouplingOutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(&full_report)?;
                println!("{}", yaml);
            }
        }
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, args), ret)]
    fn analyze_crate_level_coupling(
        &self,
        args: &CouplingArgs,
    ) -> Result<CrateLevelAnalysisResult> {
        let analysis_path = &args.path;

        if !analysis_path.exists() {
            anyhow::bail!("Provided path does not exist: {}", analysis_path.display());
        }
        if !analysis_path.is_dir() {
            anyhow::bail!(
                "Provided path is not a directory: {}",
                analysis_path.display()
            );
        }

        tracing::info!(
            "Analyzing coupling in directory: {}",
            analysis_path.display()
        );

        let metadata_output = Command::new("cargo")
            .arg("metadata")
            .arg("--format-version")
            .arg("1")
            .current_dir(analysis_path)
            .output()
            .context("Failed to execute cargo metadata")?;

        if !metadata_output.status.success() {
            let stderr = String::from_utf8_lossy(&metadata_output.stderr);
            anyhow::bail!("cargo metadata failed: {}", stderr);
        }

        let metadata_json = String::from_utf8_lossy(&metadata_output.stdout);
        let metadata: CargoMetadata =
            serde_json::from_str(&metadata_json).context("Failed to parse cargo metadata JSON")?;

        let workspace_member_ids: HashSet<_> = metadata.workspace_members.iter().cloned().collect();
        let mut package_id_to_name: HashMap<String, String> = HashMap::new();
        let mut workspace_packages_map: HashMap<String, Package> = HashMap::new();

        for pkg in &metadata.packages {
            package_id_to_name.insert(pkg.id.clone(), pkg.name.clone());
            if workspace_member_ids.contains(&pkg.id) {
                workspace_packages_map.insert(pkg.id.clone(), pkg.clone());
            }
        }

        let mut efferent_couplings: HashMap<String, usize> = HashMap::new();
        let mut afferent_couplings: HashMap<String, usize> = HashMap::new();
        let mut crate_couplings_map: HashMap<String, CrateCoupling> = HashMap::new();

        for pkg_id in &workspace_member_ids {
            if let Some(name) = package_id_to_name.get(pkg_id) {
                efferent_couplings.insert(name.clone(), 0);
                afferent_couplings.insert(name.clone(), 0);
                // Initialize CrateCoupling entry
                crate_couplings_map.insert(
                    name.clone(),
                    CrateCoupling {
                        name: name.clone(),
                        ce: 0,
                        ca: 0,
                        modules: Vec::new(),
                    },
                );
            }
        }

        if let Some(resolve_data) = &metadata.resolve {
            let resolve_nodes_map: HashMap<_, _> = resolve_data
                .nodes
                .iter()
                .map(|n| (n.id.clone(), n))
                .collect();

            for origin_pkg_id_str in workspace_packages_map.keys() {
                let origin_pkg_name = match package_id_to_name.get(origin_pkg_id_str) {
                    Some(name) => name,
                    None => continue,
                };

                if let Some(resolve_node) = resolve_nodes_map.get(origin_pkg_id_str) {
                    for dep_pkg_id_str in &resolve_node.dependencies {
                        if workspace_member_ids.contains(dep_pkg_id_str) {
                            let target_pkg_name = match package_id_to_name.get(dep_pkg_id_str) {
                                Some(name) => name,
                                None => continue,
                            };

                            if origin_pkg_name != target_pkg_name {
                                // Avoid self-dependency for Ce/Ca
                                *efferent_couplings
                                    .entry(origin_pkg_name.clone())
                                    .or_insert(0) += 1;
                                *afferent_couplings
                                    .entry(target_pkg_name.clone())
                                    .or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
        } else {
            eprintln!("Warning: 'resolve' graph not found in cargo metadata. Crate coupling might be inaccurate using fallback.");
            let workspace_package_names_to_ids: HashMap<String, String> = workspace_packages_map
                .values()
                .map(|p| (p.name.clone(), p.id.clone()))
                .collect();

            for (origin_pkg_id_str, origin_pkg_data) in &workspace_packages_map {
                let origin_pkg_name = &origin_pkg_data.name;
                for dep in &origin_pkg_data.dependencies {
                    if let Some(target_pkg_id_str) = workspace_package_names_to_ids.get(&dep.name) {
                        if origin_pkg_id_str != target_pkg_id_str {
                            let target_pkg_name = workspace_packages_map
                                .values()
                                .find(|p| &p.id == target_pkg_id_str)
                                .map(|p| &p.name)
                                .unwrap_or_else(|| &dep.name);

                            *efferent_couplings
                                .entry(origin_pkg_name.clone())
                                .or_insert(0) += 1;
                            *afferent_couplings
                                .entry(target_pkg_name.clone())
                                .or_insert(0) += 1;
                        }
                    }
                }
            }
        }

        // Populate the CrateCoupling structs
        for (name, coupling_data) in crate_couplings_map.iter_mut() {
            coupling_data.ce = *efferent_couplings.get(name).unwrap_or(&0);
            coupling_data.ca = *afferent_couplings.get(name).unwrap_or(&0);
        }

        Ok(CrateLevelAnalysisResult {
            crate_couplings_map,
            workspace_packages_map,
            package_id_to_name,
            workspace_member_ids,
        })
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

        // BTreeMap for initial path-sorted processing before final coupling sort
        let mut module_results_map: BTreeMap<String, ModuleCoupling> = BTreeMap::new();

        for mod_path_str in module_map.keys() {
            module_efferent_couplings.insert(mod_path_str.clone(), HashSet::new());
            module_afferent_couplings.insert(mod_path_str.clone(), HashSet::new());
            module_results_map.insert(mod_path_str.clone(), ModuleCoupling::default());
            // Store in BTreeMap
        }

        for (current_module_path_str, source_file_path) in &module_map {
            let content = fs::read_to_string(source_file_path).with_context(|| {
                format!("Failed to read module file: {}", source_file_path.display())
            })?;

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

                    for referenced_module_path in visitor.dependencies {
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
                        current_module_path_str, source_file_path.display(), err
                    );
                }
            }
        }

        // Populate coupling data into the BTreeMap values
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

        // Collect into Vec for sorting by coupling (original BTreeMap was by path)
        Ok(module_results_map.into_values().collect())
    }

    fn print_table_report(
        &self,
        report: &CouplingReport,
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
                        println!(); // Add a newline if crate table was printed and this isn't the first module table
                    } else if first_module_table
                        && matches!(granularity, CouplingGranularity::Module)
                    {
                        // No newline if only module view and it's the first table
                    } else if !first_module_table {
                        println!(); // Add a newline between module tables when only module view
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
            let entry = entry.with_context(|| {
                format!(
                    "Failed to read directory entry in {}",
                    current_dir.display()
                )
            })?;
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
                    format!("crate::{}", mod_name)
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
            {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let mod_name = stem.to_string();
                    let mod_path_str = if base_mod_path.to_string_lossy() == "crate" {
                        format!("crate::{}", mod_name)
                    } else {
                        format!("{}::{}", base_mod_path.to_string_lossy(), mod_name)
                    };
                    if !module_map.contains_key(&mod_path_str) {
                        module_map.insert(mod_path_str.clone(), path.to_path_buf());
                        self.discover_inline_modules(path, &mod_path_str, module_map)?;
                    }
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
        let content = fs::read_to_string(file_path).with_context(|| {
            format!(
                "Failed to read file for inline module discovery: {}",
                file_path.display()
            )
        })?;
        match syn::parse_file(&content) {
            Ok(ast) => {
                for item in ast.items {
                    if let Item::Mod(item_mod) = item {
                        if item_mod.content.is_some() {
                            let mod_name = item_mod.ident.to_string();
                            let _inline_mod_path_str = if base_module_path_str == "crate" {
                                format!("crate::{}", mod_name)
                            } else {
                                format!("{}::{}", base_module_path_str, mod_name)
                            };
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to parse {} for inline modules: {}. Skipping inline scan for this file.",
                    file_path.display(), e
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
            if self
                .current_module_path
                .last()
                .is_some_and(|last| last == "crate")
                && self.current_module_path.len() == 1
            {
                self.current_module_path.push(mod_name);
            } else {
                // This logic might need to be smarter if current_module_path is already deep
                // For file-level modules, current_module_path is already set correctly before visitor is called.
                // This is for inline `mod foo { ... }`
                // If current_module_path is ["crate", "parent"], new one is ["crate", "parent", "foo"]
                self.current_module_path.push(mod_name);
            }
            syn::visit::visit_item_mod(self, item_mod);
            self.current_module_path = original_path; // Restore path after visiting inline module
        } else {
            // For `mod foo;` declarations, we don't need to change path or visit content here,
            // as `discover_modules` handles them by finding the respective file/directory.
            // syn::visit::visit_item_mod(self, item_mod); // Default visit might still be useful for attrs etc.
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
        if let syn::Type::Path(type_path) = &*pt.ty {
            if let Some(resolved_path) = self.resolve_path(&type_path.path) {
                self.dependencies.insert(resolved_path);
            }
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
                self.add_dependency_from_path_tree(&use_path.tree);
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
        self.module_map
            .keys()
            .any(|known_mod_path| known_mod_path.starts_with(&query_prefix))
    }
}
