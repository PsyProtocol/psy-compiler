use std::{collections::HashMap, path::PathBuf};

use clap::Args;
use psy_abi::AbiExtractor;
use psy_interpreter::interpret;
use psy_package::Workspace;

use crate::{
    cli::{
        compile_cmd::{CompileOptions, resolve_workspace_method_names},
        resolve_crate_path_graph,
        save_build_artifact_to_file,
        validate_artifact_name,
    },
    errors::Result,
};

/// Generate ABI (Application Binary Interface) file for the contract
#[derive(Debug, Clone, Args)]
pub(crate) struct GenerateAbiCommand {
    /// Name of the contract type to generate ABI for (e.g. PsyTokenContractRef)
    #[clap(short, long)]
    pub contract_name: String,

    /// Path to the entry file (optional, uses package entry by default)
    #[clap(long)]
    pub entry_path: Option<PathBuf>,

    /// Output directory for the generated ABI file
    #[clap(short, long)]
    pub output_dir: Option<PathBuf>,

    /// Output filename stem (without extension); defaults to contract_name if not set
    #[clap(long)]
    pub abi_name: Option<String>,

    /// Pretty print the ABI JSON output
    #[clap(long, default_value = "true")]
    pub pretty: bool,

    /// Restrict ABI output to the same method subset used by compile targets.
    #[clap(short, long, num_args = 1..)]
    pub method_names: Option<Vec<String>>,
}

pub(crate) fn run(args: GenerateAbiCommand, workspace: Workspace) -> Result<()> {
    // `abi_name` is a file stem, not a path. Reject invalid input before the
    // comparatively expensive parse/typecheck/compile pipeline.
    let abi_stem = args.abi_name.clone().unwrap_or_else(|| args.contract_name.clone());
    validate_artifact_name(&abi_stem)?;

    // Resolve the crate path graph
    let crate_path_graph = resolve_crate_path_graph(&workspace, args.entry_path.clone());

    let method_names = resolve_workspace_method_names(
        &workspace,
        &CompileOptions {
            contract_name: Some(args.contract_name.clone()),
            method_names: args.method_names.clone(),
            entry_path: args.entry_path.clone(),
            debug: false,
        },
    )?;

    // Interpret (parse + typecheck + compile) to get circuit definitions
    // which carry method_id and is_view metadata needed by the ABI extractor.
    let mut result = interpret(Some(args.contract_name.clone()), method_names, crate_path_graph)?;

    // Build method metadata from compiled circuit definitions
    let method_metadata: HashMap<String, (u32, bool)> = result
        .compile_results
        .iter()
        .map(|f| (f.name.clone(), (f.method_id, f.is_view_function())))
        .collect();

    // Create ABI extractor and extract the ABI
    let extractor = AbiExtractor::new(args.contract_name.clone());
    let state_tree_height = extractor.compute_state_tree_height(&mut result.ctx.program);
    let abi = extractor
        .extract_abi(&mut result.ctx.program, state_tree_height, &method_metadata)
        .map_err(|e| crate::errors::CliError::Generic(e.to_string()))?;

    // Determine output directory
    let output_dir = args.output_dir.unwrap_or_else(|| workspace.target_dir.clone());

    // Use abi_name if provided, otherwise derive from contract_name
    // Save ABI next to the compiled artifact without clobbering <contract>.json.
    let abi_filename = format!("{}.abi", abi_stem);

    // Save ABI to file in target directory
    let abi_path = save_build_artifact_to_file(&abi, &abi_filename, &output_dir)?;

    println!("Generate ABI file successfully: {}", abi_path.display());
    Ok(())
}
