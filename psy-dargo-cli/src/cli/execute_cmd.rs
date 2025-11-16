use clap::Args;
use plonky2::field::{goldilocks_field::GoldilocksField, types::Field};
use psy_common::data::qhashout::QHashOut;
use psy_common_circuit::circuits::zk_signature3::manager::SimplePsyZKSignatureManager;
use psy_config::network_constants::GLOBAL_USER_TREE_HEIGHT;
use psy_crypto::signature::zk::wallet::SimplePsyPrivateKey;
use psy_data::{
    config::store_config::{PsyHasher, C, D},
    qblock::cmds::register_user::QBCRegisterUser,
};
use psy_package::Workspace;
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_store::prepare_environment_with_real_contract;
use psy_vm::vm::exec::PsyEvalSessionResult;

use crate::cli::{
    compile_cmd::{compile_workspace_full, CompileOptions},
    doc_cmd::run_doc,
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

    args.parameters.resize(args.compile_options.method_names.len(), Vec::new());

    // Compile the full workspace in order to generate any build artifacts.
    let compile_results = compile_workspace_full(&workspace, &args.compile_options)?;

    let priv_key = QHashOut::rand();
    let wallet = SimplePsyZKSignatureManager::<C, D>::new();
    let priv_key_w = SimplePsyPrivateKey::new(priv_key);
    let pub_key_param = priv_key_w.get_public_key_param::<PsyHasher>();
    let contract_state_tree_height = GLOBAL_USER_TREE_HEIGHT as usize;

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
        let cfc_input = PsyEvalSessionResult::new()
            .exec_contract_call(
                &mut lps,
                contract_id,
                &def,
                parameters.into_iter().map(GoldilocksField::from_noncanonical_u64).collect(),
            )
            .await?;
        println!("result_vm: {:?}", cfc_input.outputs);

        let proof = circuit.prove_base(&cfc_input).unwrap();
        println!("public_inputs: {:?}", &proof.public_inputs);
    }

    Ok(())
}
