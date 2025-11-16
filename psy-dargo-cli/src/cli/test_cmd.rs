use std::path::PathBuf;

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
use psy_interpreter::Interpreter;
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_store::prepare_environment_with_real_contract;
use psy_vm::{
    dpn::{
        ops::{exec_context::QExecContext, sym_felt::SymFeltRef},
        vm::compile::PsyCompileResult,
    },
    vm::exec::PsyEvalSessionResult,
};

/// Test the program file
#[derive(Debug, Clone, Args)]
pub(crate) struct TestCommand {
    #[clap(short, env, long)]
    pub file: PathBuf,
}
pub(crate) async fn run(args: TestCommand) -> crate::errors::Result<()> {
    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let (mut typechecker, mut ctx) = interpreter.typecheck_single(args.file.clone())?;
    let compile_results = interpreter.test(&mut typechecker, &mut ctx, |context, (method_name, method_id, outputs)| {
        PsyCompileResult::compile_exec(method_name, method_id, &context.store, &context, &outputs)
    })?;
    println!("compile_result: {:?}", compile_results);

    let priv_key = QHashOut::rand();
    let wallet = SimplePsyZKSignatureManager::<C, D>::new();
    let priv_key_w = SimplePsyPrivateKey::new(priv_key);
    let pub_key_param = priv_key_w.get_public_key_param::<PsyHasher>();
    let contract_state_tree_height = GLOBAL_USER_TREE_HEIGHT as usize;

    let deployer = QHashOut::rand();
    let (circuits, deploy_cmd) =
        gen_contract_deploy_and_circuits_for_functions::<C, D>(deployer, contract_state_tree_height as u8, &compile_results)?;

    let mut lps = prepare_environment_with_real_contract(
        vec![QBCRegisterUser::new(wallet.get_zksig_circuit_fingerprint(), pub_key_param)],
        vec![deploy_cmd],
        None,
        None,
        None,
    )
    .await?;
    let contract_id = GoldilocksField::from_canonical_u64(2);

    for (def, circuit) in compile_results.into_iter().zip(circuits.into_iter()) {
        let cfc_input = PsyEvalSessionResult::new()
            .exec_contract_call(&mut lps, contract_id, &def, vec![])
            .await?;
        println!("result_vm: {:?}", cfc_input.outputs);

        let proof = circuit.prove_base(&cfc_input).unwrap();
        println!("public_inputs: {:?}", &proof.public_inputs);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn psy_unit_test() {
        insta::glob!("../../../tests", "*_test.psy", |path| {
            let args = TestCommand { file: path.into() };
            tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(run(args))).unwrap();
            #[allow(static_mut_refs)]
            unsafe {
                psy_sema::STD_PRIMITIVE_SCOPE_ID.take()
            };
        });
    }
}
