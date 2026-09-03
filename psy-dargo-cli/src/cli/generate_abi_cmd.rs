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

#[cfg(test)]
mod tests {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    use psy_package::{resolve_workspace_from_toml, Workspace};

    use super::{run, GenerateAbiCommand};

    #[test]
    fn generate_abi_rejects_path_like_output_name_before_compilation() {
        let error = run(
            GenerateAbiCommand {
                contract_name: "Contract".to_string(),
                entry_path: None,
                output_dir: None,
                abi_name: Some("../escape".to_string()),
                pretty: true,
                method_names: None,
            },
            Workspace::default(),
        )
        .expect_err("path-like ABI names must be rejected before compilation");
        assert!(error.to_string().contains("Invalid artifact name"));
    }

    #[test]
    #[serial_test::serial]
    fn generate_abi_compiles_a_contract_and_writes_the_abi_file() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let dir = std::env::temp_dir().join(format!("psy_dargo_genabi_{nanos}"));
        fs::create_dir_all(dir.join("src")).expect("create src");
        fs::write(
            dir.join("src").join("app.psy"),
            "#[contract]\npub struct Demo {}\n#[contract::write_method]\nfn main() { assert_eq(1, 1, \"ok\"); }\n",
        )
        .expect("write app.psy");
        fs::write(
            dir.join("Dargo.toml"),
            "[package]\nname = \"genabidemo\"\ntype = \"bin\"\nentry = \"src/app.psy\"\nauthors = [\"\"]\n\n[dependencies]\n",
        )
        .expect("write Dargo.toml");
        let workspace = resolve_workspace_from_toml(&dir.join("Dargo.toml")).expect("resolve workspace");

        let output_dir = dir.join("target");
        run(
            GenerateAbiCommand {
                contract_name: "Demo".to_string(),
                entry_path: None,
                output_dir: Some(output_dir.clone()),
                abi_name: Some("demo_abi".to_string()),
                pretty: true,
                method_names: Some(vec!["main".to_string()]),
            },
            workspace,
        )
        .expect("generate-abi must compile the contract and write the ABI file");

        let abi = fs::read_to_string(output_dir.join("demo_abi.abi.json")).expect("ABI file must exist");
        assert!(abi.contains("\"main\""), "ABI must list the compiled method: {abi}");

        #[allow(static_mut_refs)]
        unsafe {
            psy_sema::STD_PRIMITIVE_SCOPE_ID.take();
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    #[serial_test::serial]
    fn generate_abi_defaults_the_stem_to_the_contract_and_the_output_to_the_target_dir() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let dir = std::env::temp_dir().join(format!("psy_dargo_genabi_default_{nanos}"));
        fs::create_dir_all(dir.join("src")).expect("create src");
        fs::write(
            dir.join("src").join("app.psy"),
            "#[contract]\npub struct Demo {}\n#[contract::write_method]\nfn main() { assert_eq(1, 1, \"ok\"); }\n",
        )
        .expect("write app.psy");
        fs::write(
            dir.join("Dargo.toml"),
            "[package]\nname = \"genabidefault\"\ntype = \"bin\"\nentry = \"src/app.psy\"\nauthors = [\"\"]\n\n[dependencies]\n",
        )
        .expect("write Dargo.toml");
        let workspace = resolve_workspace_from_toml(&dir.join("Dargo.toml")).expect("resolve workspace");
        let target_dir = workspace.target_dir.clone();

        run(
            GenerateAbiCommand {
                contract_name: "Demo".to_string(),
                entry_path: None,
                output_dir: None,
                abi_name: None,
                pretty: true,
                method_names: Some(vec!["main".to_string()]),
            },
            workspace,
        )
        .expect("generate-abi must fall back to the contract-named output in the target dir");

        let abi = fs::read_to_string(target_dir.join("Demo.abi.json"))
            .expect("the default ABI file must land in the workspace target dir");
        assert!(abi.contains("\"main\""), "ABI must list the compiled method: {abi}");

        #[allow(static_mut_refs)]
        unsafe {
            psy_sema::STD_PRIMITIVE_SCOPE_ID.take();
        }
        fs::remove_dir_all(dir).ok();
    }
}
