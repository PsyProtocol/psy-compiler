use clap::Args;
use plonky2::field::{goldilocks_field::GoldilocksField, types::Field};
use psy_common::data::qhashout::QHashOut;
use psy_common_circuit::circuits::zk_signature3::manager::SimplePsyZKSignatureManager;
use psy_crypto::signature::zk::wallet::SimplePsyPrivateKey;
use psy_data::{
    config::store_config::{PsyHasher, C, D},
    qblock::cmds::register_user::QBCRegisterUser,
};
use psy_package::Workspace;
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_vm::vm::exec::PsyEvalSessionResult;

use crate::cli::{
    compile_cmd::{compile_workspace_full, CompileOptions},
    doc_cmd::run_doc,
    test_helpers::prepare_environment_with_real_contract,
};

/// Executes a circuit to calculate its return value
#[derive(Debug, Clone, Args)]
pub(crate) struct ExecuteCommand {
    #[clap(flatten)]
    pub compile_options: CompileOptions,

    #[clap(short, long, value_parser = parse_vec_u64, num_args = 0..)]
    pub parameters: Vec<Vec<u64>>,

    #[clap(long, hide = true, default_value = "false")]
    pub doc: bool,
}

fn parse_vec_u64(s: &str) -> Result<Vec<u64>, String> {
    s.split(',').map(|num| num.parse::<u64>().map_err(|e| e.to_string())).collect()
}

pub(crate) async fn run(mut args: ExecuteCommand, workspace: Workspace) -> crate::errors::Result<()> {
    if args.doc {
        return run_doc(args, workspace).await;
    }

    // Compile the full workspace in order to generate any build artifacts.
    let compile_results = compile_workspace_full(&workspace, &args.compile_options)?;
    args.parameters.resize(compile_results.circuit_definitions.len(), Vec::new());

    let priv_key = QHashOut::rand();
    let wallet = SimplePsyZKSignatureManager::<C, D>::new();
    let priv_key_w = SimplePsyPrivateKey::new(priv_key);
    let pub_key_param = priv_key_w.get_public_key_param::<PsyHasher>();
    let contract_state_tree_height = compile_results.state_tree_height as usize;

    let deployer = QHashOut::rand();
    let (circuits, deploy_cmd) =
        gen_contract_deploy_and_circuits_for_functions::<C, D>(deployer, contract_state_tree_height as u8, &compile_results.circuit_definitions)?;

    let mut lps = prepare_environment_with_real_contract(
        vec![QBCRegisterUser::new(wallet.get_zksig_circuit_fingerprint(), pub_key_param)],
        vec![deploy_cmd],
        None,
        None,
        None,
    )
    .await?;
    let contract_id = GoldilocksField::from_canonical_u64(2);

    for ((def, parameters), circuit) in compile_results
        .circuit_definitions
        .into_iter()
        .zip(args.parameters.into_iter())
        .zip(circuits.into_iter())
    {
        println!(
            "circuit_stats method={} degree_bits={} gate_kinds={}",
            def.name,
            circuit.circuit_data.common.degree_bits(),
            circuit.circuit_data.common.gates.len()
        );
        let cfc_input = PsyEvalSessionResult::new()
            .exec_contract_call(
                &mut lps,
                contract_id,
                &def,
                parameters.into_iter().map(GoldilocksField::from_noncanonical_u64).collect(),
            )
            .await?;
        println!("result_vm: {:?}", cfc_input.outputs);
        println!("result_events: {:?}", cfc_input.events);

        let proof = circuit.prove_base(&cfc_input).unwrap();
        println!("public_inputs: {:?}", &proof.public_inputs);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use psy_package::{resolve_workspace_from_toml, Workspace};
    use serial_test::serial;

    use super::{parse_vec_u64, run, ExecuteCommand};
    use crate::cli::compile_cmd::CompileOptions;

    fn compile_options() -> CompileOptions {
        CompileOptions { contract_name: None, method_names: None, entry_path: None, debug: false }
    }

    /// Materializes `source` as a throwaway binary workspace and resolves it.
    /// The entry file keeps an identifier stem so the derived module name stays valid.
    fn temp_workspace(source: &str, label: &str) -> (PathBuf, Workspace) {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let dir = std::env::temp_dir().join(format!("psy_dargo_exec_{label}_{nanos}"));
        fs::create_dir_all(dir.join("src")).expect("create src");
        fs::write(dir.join("src").join("app.psy"), source).expect("write app.psy");
        fs::write(
            dir.join("Dargo.toml"),
            "[package]\nname = \"execdemo\"\ntype = \"bin\"\nentry = \"src/app.psy\"\nauthors = [\"\"]\n\n[dependencies]\n",
        )
        .expect("write Dargo.toml");
        let workspace = resolve_workspace_from_toml(&dir.join("Dargo.toml")).expect("resolve workspace");
        (dir, workspace)
    }

    fn reset_std_scope() {
        #[allow(static_mut_refs)]
        unsafe {
            psy_sema::STD_PRIMITIVE_SCOPE_ID.take();
        }
    }

    #[test]
    fn parse_vec_u64_splits_comma_separated_values() {
        assert_eq!(parse_vec_u64("1,2,3").unwrap(), vec![1, 2, 3]);
        assert_eq!(parse_vec_u64("7").unwrap(), vec![7]);
        assert!(parse_vec_u64("").is_err());
        assert!(parse_vec_u64("1,x,3").is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn run_reports_compile_errors_before_proving() {
        let (dir, workspace) = temp_workspace("fn main() -> Felt { return true; }", "bad_type");
        let error = run(
            ExecuteCommand { compile_options: compile_options(), parameters: vec![], doc: false },
            workspace,
        )
        .await
        .expect_err("type error must abort before any proving");
        assert!(
            error.to_string().to_lowercase().contains("typemismatch"),
            "expected a type mismatch error, got: {error}"
        );
        reset_std_scope();
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn run_executes_program_and_proves_result() {
        let (dir, workspace) =
            temp_workspace("fn main(a: Felt) -> Felt { return a + 1; }", "add_one");
        run(
            ExecuteCommand { compile_options: compile_options(), parameters: vec![vec![41]], doc: false },
            workspace,
        )
        .await
        .expect("simple add program must compile, execute, and prove");
        reset_std_scope();
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn doc_flag_delegates_to_run_doc_and_checks_commented_output() {
        let source = "// input: 41\n// output: 42\nfn main(a: Felt) -> Felt { return a + 1; }\n";
        let (dir, workspace) = temp_workspace(source, "doc_mode");
        run(
            ExecuteCommand { compile_options: compile_options(), parameters: vec![], doc: true },
            workspace,
        )
        .await
        .expect("doc mode must replay the commented input and match the commented output");
        reset_std_scope();
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn doc_mode_with_debug_flag_prints_function_metadata_and_results() {
        let source = "// input: 41\n// output: 42\nfn main(a: Felt) -> Felt { return a + 1; }\n";
        let (dir, workspace) = temp_workspace(source, "doc_debug");
        run(
            ExecuteCommand {
                compile_options: CompileOptions {
                    contract_name: None,
                    method_names: Some(vec!["main".to_string()]),
                    entry_path: None,
                    debug: true,
                },
                parameters: vec![],
                doc: true,
            },
            workspace,
        )
        .await
        .expect("debug doc mode must replay the commented input and dump diagnostics");
        reset_std_scope();
        fs::remove_dir_all(dir).ok();
    }
}
