use psy_sema::AstVisualizer;
use kvq::memory::simple::KVQSimpleMemoryBackingStore;
use plonky2::field::{goldilocks_field::GoldilocksField, types::Field};
use psy_common::data::qhashout::QHashOut;
use psy_common_circuit::circuits::zk_signature3::manager::SimplePsyZKSignatureManager;
use psy_config::network_constants::GLOBAL_USER_TREE_HEIGHT;
use psy_crypto::signature::zk::wallet::SimplePsyPrivateKey;
use psy_data::{
    config::store_config::{PsyFelt, PsyHasher, C, D},
    qblock::{cmds::register_user::QBCRegisterUser, process::simple::SimpleBlockProcessor},
    qdata::checkpoint::{PsyBlockState, PsyCheckpointLeaf, PsyCheckpointLeafStats},
    qstore::controllers::proving_session::PsyLocalProvingSessionStore,
    traits::qdatastore::qmetadata::{QMetaDataStoreReaderSync, QMetaDataStoreWriterSync},
};
use psy_interpreter::Interpreter;
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_vm::{
    dpn::{
        ops::{exec_context::QExecContext, sym_felt::SymFeltRef},
        vm::compile::PsyCompileResult,
    },
    vm::exec::PsyEvalSessionResult,
};
use serial_test::serial;

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_interpreter() {
    psy_common::setup_logging().ok();

    insta::glob!("../../tests", "{struct*.psy,fn_test.psy,fn_chain_call_test.psy}", |path| {
        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
        let (mut typechecker, mut ctx) = interpreter.typecheck_single(path.into()).unwrap();

        let compile_results = interpreter
            .interpret(
                &mut typechecker,
                &mut ctx,
                None::<String>,
                vec!["main".to_string()],
                |context, (method_name, method_id, outputs)| {
                    PsyCompileResult::compile_exec(method_name, method_id, &context.store, context, &outputs)
                },
            )
            .unwrap();

        let priv_key = QHashOut::rand();
        let wallet = SimplePsyZKSignatureManager::<C, D>::new();
        let priv_key_w = SimplePsyPrivateKey::new(priv_key);
        let pub_key_param = priv_key_w.get_public_key_param::<PsyHasher>();
        let contract_state_tree_height = GLOBAL_USER_TREE_HEIGHT as usize;

        let deployer: u64 = 0;
        let (_circuits, deploy_cmd) =
            gen_contract_deploy_and_circuits_for_functions::<C, D>(deployer, contract_state_tree_height as u8, &compile_results).unwrap();

        let mut lps = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let store = KVQSimpleMemoryBackingStore::new();
                store.set_block_state(&PsyBlockState::get_genesis_value())?;
                store.set_checkpoint_leaf_data(
                    0,
                    &PsyCheckpointLeaf {
                        global_chain_root: QHashOut::ZERO,
                        stats: PsyCheckpointLeafStats::new_empty(),
                    },
                )?;
                let final_store = SimpleBlockProcessor::prepare_environment_with_real_contract(
                    vec![QBCRegisterUser::new(wallet.get_zksig_circuit_fingerprint(), pub_key_param)],
                    vec![deploy_cmd],
                    store,
                )
                .await?;
                let latest_block_state = final_store.get_latest_block_state().await?;
                let checkpoint_id = PsyFelt::from_canonical_u64(latest_block_state.checkpoint_id);
                let user_id = PsyFelt::from_canonical_u64(5);
                Ok::<_, anyhow::Error>(PsyLocalProvingSessionStore::<_, _, PsyHasher>::new_at(
                    final_store,
                    checkpoint_id,
                    user_id,
                    PsyFelt::ZERO,
                    PsyFelt::ZERO,
                    GLOBAL_USER_TREE_HEIGHT as usize,
                ))
            })
        })
        .unwrap();
        let contract_id = GoldilocksField::from_canonical_u64(2);

        let cfc_input = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                PsyEvalSessionResult::new()
                    .exec_contract_call(&mut lps, contract_id, &compile_results[0], vec![])
                    .await
            })
        })
        .unwrap();

        println!("result_vm: {:?}", cfc_input.outputs);
        println!("result_events: {:?}", cfc_input.events);

        insta::assert_snapshot!("prover_tests__test_interpreter", ctx.debug_scope(psy_sema::ScopeId::root()));
    });
}
