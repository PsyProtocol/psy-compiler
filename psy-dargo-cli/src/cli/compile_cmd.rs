use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
};

use clap::Args;
use psy_package::{ResolvedSourceWorkspace, VfsPath, Workspace};
use psy_vm::dpn::vm::def::DPNFunctionCircuitDefinition;

use crate::{
    cli::{
        doc_cmd::{extract_function_metadata_from_context, FunctionNode},
        save_build_artifact_to_file,
    },
    errors::Result,
};

/// Compile the program and its secret execution trace
#[derive(Debug, Clone, Args)]
pub struct CompileCommand {
    #[clap(flatten)]
    pub compile_options: CompileOptions,
}

pub fn run(args: CompileCommand, workspace: Workspace) -> Result<()> {
    compile_workspace_full(&workspace, &args.compile_options)?;
    Ok(())
}

pub struct CompilationResult {
    pub circuit_definitions: Vec<DPNFunctionCircuitDefinition>,
    pub function_metadata: HashMap<String, FunctionNode>,
}

/// Parse and compile the entire workspace, then report errors.
/// This is the main entry point used by all other commands that need
/// compilation.
pub fn compile_workspace_full(workspace: &Workspace, compile_options: &CompileOptions) -> Result<CompilationResult> {
    let crate_path_graph = super::resolve_crate_path_graph(workspace, compile_options.entry_path.clone());
    let method_names = resolve_workspace_method_names(workspace, compile_options)?;

    let mut interpret_result = psy_interpreter::interpret(compile_options.contract_name.clone(), method_names, crate_path_graph)?;

    let function_metadata = extract_function_metadata_from_context(
        &mut interpret_result.ctx,
        &mut interpret_result.typechecker,
        &interpret_result.compile_results,
    );

    if compile_options.debug {
        println!("workspace: {:?}", workspace);
        println!("compile_result: {:?}", interpret_result.compile_results);
    } else {
        save_build_artifact_to_file(
            &interpret_result.compile_results,
            &workspace.package.name.to_string(),
            &workspace.target_dir,
        )?;
    }

    Ok(CompilationResult {
        circuit_definitions: interpret_result.compile_results,
        function_metadata,
    })
}

/// Compile a resolver-based workspace assembled from in-memory sources.
pub fn compile_source_workspace_full(workspace: &ResolvedSourceWorkspace, compile_options: &CompileOptions) -> Result<CompilationResult> {
    let mut crate_path_graph = psy_common::Graph::new();
    let method_names = resolve_source_workspace_method_names(workspace, compile_options)?;

    let package_entry_path = workspace
        .packages
        .iter()
        .map(|(id, pkg)| {
            (
                id.clone(),
                source_vfs_to_pathbuf(workspace.source_map.path(pkg.entry_file_id).expect("entry file id exists")),
            )
        })
        .collect::<HashMap<_, _>>();

    for package in workspace.packages.values() {
        let package_entry = package_entry_path.get(&package.package_id).expect("entry path exists").clone();
        if !crate_path_graph.contains_node(&package_entry) {
            crate_path_graph.add_node(package_entry.clone());
        }
        for dep_pkg_id in package.dependency_packages.values() {
            if let Some(dep_entry) = package_entry_path.get(dep_pkg_id) {
                crate_path_graph.add_edge(package_entry.clone(), dep_entry.clone());
            }
        }
    }

    let virtual_files = workspace
        .source_map
        .snapshot()
        .into_iter()
        .map(|(_, path, text)| (source_vfs_to_pathbuf(&path), text))
        .collect::<Vec<_>>();

    let mut interpret_result =
        psy_interpreter::interpret_virtual_files(compile_options.contract_name.clone(), method_names, crate_path_graph, virtual_files)?;

    let function_metadata = extract_function_metadata_from_context(
        &mut interpret_result.ctx,
        &mut interpret_result.typechecker,
        &interpret_result.compile_results,
    );

    Ok(CompilationResult {
        circuit_definitions: interpret_result.compile_results,
        function_metadata,
    })
}

/// Options for the compile command
#[derive(Args, Clone, Debug, Default)]
pub struct CompileOptions {
    #[clap(short, long, default_value = None)]
    pub contract_name: Option<String>,
    #[clap(short, long, num_args = 1..)]
    pub method_names: Option<Vec<String>>,
    #[clap(long, hide = true, default_value = None)]
    pub entry_path: Option<PathBuf>,
    #[clap(long, hide = true, default_value = "false")]
    pub debug: bool,
}

fn source_vfs_to_pathbuf(path: &VfsPath) -> PathBuf {
    match path {
        VfsPath::Real(path) => path.clone(),
        VfsPath::Virtual(path) => PathBuf::from(path),
    }
}

pub(crate) fn resolve_workspace_method_names(workspace: &Workspace, compile_options: &CompileOptions) -> Result<Vec<String>> {
    if let Some(method_names) = &compile_options.method_names {
        if method_names.is_empty() {
            return Err(crate::errors::CliError::Generic(
                "method_names must not be empty when provided".to_string(),
            ));
        }
        return Ok(method_names.clone());
    }

    let mut root_package = workspace.package.clone();
    if let Some(entry_path) = &compile_options.entry_path {
        root_package.entry_path = entry_path.clone();
    }
    let root_source = std::fs::read_to_string(root_package.entry_canonical_path())?;
    if source_has_function_name(&root_source, "main") {
        return Ok(vec!["main".to_string()]);
    }

    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    queue.push_back(&workspace.package);

    let mut method_names = Vec::new();
    while let Some(package) = queue.pop_front() {
        let key = package.entry_canonical_path().display().to_string();
        if !visited.insert(key) {
            continue;
        }
        let source = std::fs::read_to_string(package.entry_canonical_path())?;
        extract_contract_method_names(&source, &mut method_names);
        for dep in package.dependencies.values() {
            match dep {
                psy_package::Dependency::Remote { package } | psy_package::Dependency::Local { package } => {
                    queue.push_back(package);
                }
            }
        }
    }

    method_names.sort();
    method_names.dedup();
    if method_names.is_empty() {
        return Err(crate::errors::CliError::Generic(
            "Unable to discover contract methods: add #[contract_method], #[contract::write_method], or #[contract::view_method] to at least one function or provide --method-names explicitly"
                .to_string(),
        ));
    }
    Ok(method_names)
}

pub(crate) fn resolve_source_workspace_method_names(workspace: &ResolvedSourceWorkspace, compile_options: &CompileOptions) -> Result<Vec<String>> {
    if let Some(method_names) = &compile_options.method_names {
        if method_names.is_empty() {
            return Err(crate::errors::CliError::Generic(
                "method_names must not be empty when provided".to_string(),
            ));
        }
        return Ok(method_names.clone());
    }

    let mut method_names = Vec::new();
    for (_, _, text) in workspace.source_map.snapshot() {
        extract_contract_method_names(&text, &mut method_names);
    }
    method_names.sort();
    method_names.dedup();
    if method_names.is_empty() {
        return Err(crate::errors::CliError::Generic(
            "Unable to discover contract methods: add #[contract_method], #[contract::write_method], or #[contract::view_method] to at least one function or provide --method-names explicitly"
                .to_string(),
        ));
    }
    Ok(method_names)
}

fn extract_contract_method_names(source: &str, method_names: &mut Vec<String>) {
    for marker in ["#[contract::write_method]", "#[contract::view_method]", "#[contract_method]"] {
        let mut search_start = 0usize;
        while let Some(relative) = source[search_start..].find(marker) {
            let attr_start = search_start + relative + marker.len();
            let rest = &source[attr_start..];
            if let Some(method_name) = extract_first_function_name(rest) {
                method_names.push(method_name);
            }
            search_start = attr_start;
        }
    }
}

fn extract_first_function_name(source: &str) -> Option<String> {
    extract_function_name(source, |_| true)
}

fn source_has_function_name(source: &str, expected_name: &str) -> bool {
    extract_function_name(source, |name| name == expected_name).is_some()
}

fn extract_function_name(source: &str, predicate: impl Fn(&str) -> bool) -> Option<String> {
    let marker = "fn ";
    let mut search_start = 0usize;
    while let Some(relative) = source[search_start..].find(marker) {
        let start = search_start + relative + marker.len();
        let rest = &source[start..];
        let end = rest
            .find(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
            .unwrap_or(rest.len());
        if end > 0 && predicate(&rest[..end]) {
            return Some(rest[..end].to_string());
        }
        search_start = start;
    }
    None
}
