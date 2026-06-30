use std::{collections::HashMap, path::PathBuf};

use clap::Args;
use psy_abi::AbiExtractor;
use psy_interpreter::interpret;
use psy_package::Workspace;
use psy_vm::dpn::{
    ops::{exec_context::QExecContext, sym_felt::SymFeltRef},
    vm::def::DPNFunctionCircuitDefinition,
};

use crate::{
    cli::{resolve_crate_path_graph, save_build_artifact_to_file},
    errors::Result,
};

/// Generate ABI (Application Binary Interface) file for the contract
#[derive(Debug, Clone, Args)]
pub(crate) struct GenerateAbiCommand {
    /// Name of the contract to generate ABI for
    #[clap(short, long)]
    pub contract_name: String,

    /// Path to the entry file (optional, uses package entry by default)
    #[clap(long)]
    pub entry_path: Option<PathBuf>,

    /// Output directory for the generated ABI file
    #[clap(short, long)]
    pub output_dir: Option<PathBuf>,

    /// Pretty print the ABI JSON output
    #[clap(long, default_value = "true")]
    pub pretty: bool,
}

pub(crate) fn run(args: GenerateAbiCommand, workspace: Workspace) -> Result<()> {
    // Resolve the crate path graph
    let crate_path_graph = resolve_crate_path_graph(&workspace, args.entry_path.clone());

    // Interpret (parse + typecheck + compile) to get circuit definitions
    // which carry method_id and is_view metadata needed by the ABI extractor.
    let mut result = interpret(Some(args.contract_name.clone()), vec![], crate_path_graph)?;

    // Build method metadata from compiled circuit definitions
    let method_metadata: HashMap<String, (u32, bool)> = result
        .compile_results
        .iter()
        .map(|f| (f.name.clone(), (f.method_id, f.is_view_function())))
        .collect();

    let state_tree_height = derive_state_tree_height(&result.compile_results);

    // Create ABI extractor and extract the ABI
    let extractor = AbiExtractor::new(args.contract_name.clone());
    let abi = extractor
        .extract_abi(&mut result.ctx.program, state_tree_height, &method_metadata)
        .map_err(|e| crate::errors::CliError::Generic(e.to_string()))?;

    // Determine output directory
    let output_dir = args.output_dir.unwrap_or_else(|| workspace.target_dir.clone());

    // Generate the ABI file name (without extension, save_build_artifact_to_file
    // adds .json)
    let abi_filename = format!("{}.abi", args.contract_name);

    // Save ABI to file in target directory
    let abi_path = save_build_artifact_to_file(&abi, &abi_filename, &output_dir)?;

    println!("Generate ABI file successfully: {}", abi_path.display());
    Ok(())
}

fn derive_state_tree_height(compile_results: &[DPNFunctionCircuitDefinition]) -> u16 {
    let mut max_slot = None::<u64>;

    for circuit in compile_results {
        for cmd in &circuit.state_commands {
            let slot = match cmd {
                psy_vm::dpn::ops::state_cmd::data::DPNStateCmd::SetContractStateSlotSingle(c) => c.sub_slot_index,
                psy_vm::dpn::ops::state_cmd::data::DPNStateCmd::SetContractStateSlotRange(c) => c.sub_slot_index + c.value.len() as u64,
                psy_vm::dpn::ops::state_cmd::data::DPNStateCmd::GetSelfUserCurrentContractStateSlotSingle(c) => c.sub_slot_index,
                psy_vm::dpn::ops::state_cmd::data::DPNStateCmd::GetSelfUserCurrentContractStateSlotRange(c) => c.sub_slot_index + c.length as u64,
                _ => continue,
            };
            max_slot = Some(max_slot.map_or(slot, |m| m.max(slot)));
        }
    }

    let max_slot = max_slot.unwrap_or(0);
    // Each state tree leaf holds 4 felts; height must cover the max slot.
    let height = ((max_slot + 4) / 4).next_power_of_two().trailing_zeros() as u16;
    height.max(1)
}