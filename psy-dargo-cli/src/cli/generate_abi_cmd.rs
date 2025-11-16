use std::path::PathBuf;

use clap::Args;
use psy_abi::AbiExtractor;
use psy_interpreter::Interpreter;
use psy_package::Workspace;
use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};

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

    // Parse and type-check the code to build the AST
    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let (_typechecker, mut ctx) = interpreter.typecheck(crate_path_graph)?;

    // Create ABI extractor
    let extractor = AbiExtractor::new(args.contract_name.clone());

    // Extract spec-compliant ABI from the program
    // SAFETY: We're extending the lifetime to 'static for the duration of this
    // function call only. This is safe because we own the program and it lives
    // for the entire duration of this function.
    let program_ptr: *mut _ = &mut ctx.program;
    let spec_abi = unsafe {
        let static_program = &mut *(program_ptr as *mut psy_ast::Program<psy_vm::dpn::ops::sym_felt::SymFeltRef>);
        extractor.extract_spec_compliant_abi(static_program)?
    };

    // Determine output directory
    let output_dir = args.output_dir.unwrap_or_else(|| workspace.target_dir.clone());

    // Generate the ABI file name (without extension, save_build_artifact_to_file
    // adds .json)
    let abi_filename = format!("{}.abi", args.contract_name);

    // Save ABI to file in target directory
    let abi_path = save_build_artifact_to_file(&spec_abi, &abi_filename, &output_dir)?;

    println!("Generate ABI file successfully");
    Ok(())
}
