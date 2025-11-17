use std::{marker::PhantomData, sync::Arc};

use kvq::memory::simple::KVQSimpleMemoryBackingStore;
use plonky2::{
    field::{
        goldilocks_field::GoldilocksField,
        types::{Field, PrimeField64},
    },
    hash::poseidon::PoseidonHash,
    plonk::config::PoseidonGoldilocksConfig,
};
use psy_common::{
    data::qhashout::QHashOut,
    ups::circuits::{LocalCircuitId, LocalCircuitType},
    utils::debug_timer::DebugTimer,
};
use psy_common_circuit::circuits::{traits::qstandard::QStandardCircuit, zk_signature3::manager::SimplePsyZKSignatureManager};
use psy_config::{
    network_constants::{GLOBAL_USER_TREE_HEIGHT, UPS_SESSION_PROOF_TREE_HEIGHT},
    PSY_NETWORK_MAGIC,
};
use psy_crypto::{hash::utils::gen_dapen_contract_function_method_id, signature::zk::wallet::SimplePsyPrivateKey};
use psy_data::{
    guta::api::SubmitUserEndCapProofAPIInput,
    proof_store::simple::SimpleProofStoreMemory,
    protocol::circuit_fingerprints::PsyWorkerToolboxCoreCircuitFingerprints,
    qblock::{
        cmds::{core::PsyBlockCommands, deploy_contract::QBCDeployContract, register_user::QBCRegisterUser},
        process::simple::SimpleBlockProcessor,
    },
    qdata::contract::{ContractCodeDefinition, ContractFunctionCodeDefinition},
    qstore::controllers::{proving_session::PsyLocalProvingSessionStore, session_info::SessionCircuitInfoStore},
    traits::qdatastore::{qmetadata::QMetaDataStoreReaderSync, qtreedata::PsyComboDataStoreReaderWriterSync},
};
use psy_dpn_circuit::circuits::cfc::DapenContractFunctionCircuit;
use psy_network_circuit::guta::guta_helper::PsyGUTACircuitManager;
use psy_prover::local::simple::SimpleAPI;
use psy_store::{node::coordinator::PsyCoordinatorStoreWriterAsyncImm, prepare_environment_with_real_contract};
use psy_ups_circuit::{
    circuit_manager::core::{PsyUPSStepCircuitManager, QCircuitManager},
    session::UserProvingSessionManager,
};
use psy_vm::{
    dpn::{
        ops::{context_trait::DPNContext, exec_context::QExecContext, sym_felt::SymFeltRef},
        vm::{compile::PsyCompileResult, def::DPNFunctionCircuitDefinition},
    },
    ups::circuit_manager::UPSCircuitManager,
};
use psylang_macros::qcontract;

type Felt = SymFeltRef;

pub struct SimpleContractStateful<C: DPNContext<Felt>> {
    _phantom: PhantomData<C>,
}
impl<C: DPNContext<Felt>> SimpleContractStateful<C> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

#[qcontract]
impl<C: DPNContext<Felt>> SimpleContractStateful<C> {
    pub fn simple_mint_debug(&mut self, ctx: &mut C, amount: Felt) -> Felt {
        let self_user_leaf = ctx.get_state_hash_at(ctx.get_user_id());
        //[balance,alt1,alt2,alt3]
        let current_balance = self_user_leaf[0];

        let new_balance = current_balance + amount;
        ctx.cset_state_hash_at(ctx.get_user_id(), [new_balance, self_user_leaf[1], self_user_leaf[2], self_user_leaf[3]]);

        new_balance
    }
    pub fn simple_transfer(&mut self, ctx: &mut C, recipient: Felt, amount: Felt) -> Felt {
        let self_user_id = ctx.get_user_id();
        let self_user_leaf = ctx.get_state_hash_at(self_user_id);

        let current_balance = self_user_leaf[0];

        ctx.assert_true(amount <= current_balance, "insufficient balance");

        let new_balance = current_balance - amount;
        ctx.assert_true(new_balance < current_balance, "user balance overflow");

        ctx.cset_state_hash_at(self_user_id, [new_balance, self_user_leaf[1], self_user_leaf[2], self_user_leaf[3]]);

        let p2p_leaf = ctx.get_state_hash_at(recipient);
        let previous_total_sent_to_recipient = p2p_leaf[2];

        let new_total_sent_to_recipient = previous_total_sent_to_recipient + amount;
        ctx.assert_true(new_total_sent_to_recipient > previous_total_sent_to_recipient, "sent amount overflow");

        ctx.cset_state_hash_at(recipient, [p2p_leaf[0], p2p_leaf[1], new_total_sent_to_recipient, p2p_leaf[3]]);
        current_balance
    }
    pub fn simple_claim(&mut self, ctx: &mut C, sender: Felt) -> Felt {
        let self_user_id = ctx.get_user_id();
        ctx.assert_true(sender != self_user_id, "you cannot claim from your self");

        let self_leaf = ctx.get_state_hash_at(self_user_id);
        let current_balance = self_leaf[0];

        let loc_transfer_info_for_sender = ctx.get_state_hash_at(sender);
        let loc_previous_total_recieved_from_sender = loc_transfer_info_for_sender[0];

        let sender_transfer_info_leaf_for_me = ctx.get_other_user_contract_state_hash_at(0, sender, ctx.get_contract_id(), self_user_id);

        let sender_total_sent_to_me = sender_transfer_info_leaf_for_me[2];

        ctx.assert_true(
            sender_total_sent_to_me > loc_previous_total_recieved_from_sender,
            "no tokens to claim from this sender",
        );

        let tokens_to_claim = sender_total_sent_to_me - loc_previous_total_recieved_from_sender;

        let loc_new_total_recieved_from_sender = sender_total_sent_to_me;

        ctx.cset_state_hash_at(
            sender,
            [
                loc_new_total_recieved_from_sender,
                loc_transfer_info_for_sender[1],
                loc_transfer_info_for_sender[2],
                loc_transfer_info_for_sender[3],
            ],
        );

        let new_balance = tokens_to_claim + current_balance;
        ctx.assert_true(current_balance < new_balance, "balance overflow");

        ctx.cset_state_hash_at(self_user_id, [new_balance, self_leaf[1], self_leaf[2], self_leaf[3]]);

        new_balance
    }
}

const D: usize = 2;
type C = PoseidonGoldilocksConfig;

fn compile_simple_mint_debug() -> anyhow::Result<DPNFunctionCircuitDefinition> {
    let mut ctx = QExecContext::new();
    let mut contract = SimpleContractStateful::new();
    let amount = ctx.add_input();
    let z = contract.simple_mint_debug(&mut ctx, amount);
    let outputs = vec![z];
    let method_args = [("amount".to_string(), 1usize)];
    let method_name = "simple_mint_debug".to_string();
    let method_id = gen_dapen_contract_function_method_id(method_name.clone(), &method_args);
    let fn_circuit_def = PsyCompileResult::compile_exec("simple_mint_debug".to_string(), method_id, &ctx.store, &ctx, &outputs);

    Ok(fn_circuit_def)
}
fn compile_simple_transfer() -> anyhow::Result<DPNFunctionCircuitDefinition> {
    let mut ctx = QExecContext::new();
    let mut contract = SimpleContractStateful::new();
    let recipient = ctx.add_input();
    let amount = ctx.add_input();
    let z = contract.simple_transfer(&mut ctx, recipient, amount);
    let outputs = vec![z];
    let method_args = [("recipient".to_string(), 1usize), ("amount".to_string(), 1usize)];
    let method_name = "simple_transfer".to_string();
    let method_id = gen_dapen_contract_function_method_id(method_name.clone(), &method_args);
    let fn_circuit_def = PsyCompileResult::compile_exec("simple_transfer".to_string(), method_id, &ctx.store, &ctx, &outputs);

    Ok(fn_circuit_def)
}

fn compile_simple_claim() -> anyhow::Result<DPNFunctionCircuitDefinition> {
    let mut ctx = QExecContext::new();
    let mut contract = SimpleContractStateful::new();
    let sender = ctx.add_input();
    let z = contract.simple_claim(&mut ctx, sender);
    let outputs = vec![z];
    let method_args = [("sender".to_string(), 1usize)];
    let method_name = "simple_claim".to_string();
    let method_id = gen_dapen_contract_function_method_id(method_name.clone(), &method_args);
    let fn_circuit_def = PsyCompileResult::compile_exec("simple_claim".to_string(), method_id, &ctx.store, &ctx, &outputs);

    Ok(fn_circuit_def)
}

async fn demo_user_proving_session() -> anyhow::Result<()> {
    let mut timer = DebugTimer::new("demo_user_proving_session");

    timer.lap("start");

    let simple_mint_debug_def = compile_simple_mint_debug()?;
    timer.lap("compiled simple_mint_debug");
    let simple_transfer_def = compile_simple_transfer()?;
    timer.lap("compiled simple_transfer");
    let simple_claim_def = compile_simple_claim()?;
    timer.lap("compiled simple_claim");

    let contract_state_tree_height = GLOBAL_USER_TREE_HEIGHT as usize;
    const D: usize = 2;
    type C = PoseidonGoldilocksConfig;

    let deployer = QHashOut::rand();
    let defs_array = [simple_mint_debug_def, simple_transfer_def, simple_claim_def];
    timer.lap("start building circuits");

    use psy_prover::session::session::gen_contract_deploy_and_circuits_for_functions;

    let (result_circuits, deploy_cmd) =
        gen_contract_deploy_and_circuits_for_functions::<C, D>(deployer, contract_state_tree_height as u8, &defs_array)?;
    let mut result_circuits = result_circuits;
    timer.lap("finished building fn circuits");
    let priv_key_0 = QHashOut::rand();
    let priv_key_1 = QHashOut::rand();
    let mut wallet = SimplePsyZKSignatureManager::<C, D>::new();
    let pub_key_0 = wallet.add_private_key_get_info(SimplePsyPrivateKey::new(priv_key_0));
    let pub_key_1 = wallet.add_private_key_get_info(SimplePsyPrivateKey::new(priv_key_1));
    timer.lap("finished building wallet/zksig circuits");

    timer.lap("prepared environement");

    let contract_id = GoldilocksField::from_canonical_u64(3);

    let [simple_mint_debug_def, simple_transfer_def, simple_claim_def] = defs_array;

    timer.lap("start: setup circuits");

    let simple_claim_circuit = result_circuits.pop().unwrap();
    let simple_transfer_circuit = result_circuits.pop().unwrap();
    let simple_mint_debug_circuit = result_circuits.pop().unwrap();
    timer.lap("end: setup circuits");

    /*
    println!("\n\n[simple_claim_circuit.common]:\n{:?}",simple_claim_circuit.get_common_circuit_data_ref());
    println!("\n\n[simple_transfer_circuit.common]:\n{:?}",simple_transfer_circuit.get_common_circuit_data_ref());
    println!("\n\n[simple_mint_debug_circuit.common]:\n{:?}\n\n",simple_mint_debug_circuit.get_common_circuit_data_ref());
    */

    timer.lap("start: init PsyUPSStepCircuitManager");

    let main_circuits = PsyUPSStepCircuitManager::<C, D>::new_with_config(PSY_NETWORK_MAGIC);
    //main_circuits.print_common_config();

    timer.lap("end: init PsyUPSStepCircuitManager");

    let lps = prepare_environment_with_real_contract(
        vec![
            QBCRegisterUser::new_from_u64s([1; 4], [1; 4]),
            QBCRegisterUser::new_from_u64s([1; 4], [13371, 13372, 13373, 13374]),
            QBCRegisterUser::new_from_u64s([1; 4], [13375, 13376, 13377, 13378]),
            QBCRegisterUser::new(QHashOut::rand(), QHashOut::rand()),
            QBCRegisterUser::new(QHashOut::rand(), QHashOut::rand()),
            pub_key_0.into(),
            pub_key_1.into(),
        ],
        vec![
            QBCDeployContract {
                deployer: QHashOut::from_values(13371, 13372, 13373, 13374),
                code_definition: ContractCodeDefinition {
                    state_tree_height: 12 as u16,
                    functions: vec![ContractFunctionCodeDefinition::default()],
                },
                function_whitelist: vec![
                    QHashOut::rand(),
                    QHashOut::rand(),
                    QHashOut::from_values(1, 0, 0, 0),
                    QHashOut::from_values(0, 0, 0, 0),
                ],
            },
            QBCDeployContract {
                deployer: QHashOut::from_values(13375, 13376, 13377, 13378),
                code_definition: ContractCodeDefinition {
                    state_tree_height: 13 as u16,
                    functions: vec![ContractFunctionCodeDefinition::default()],
                },
                function_whitelist: vec![
                    QHashOut::rand(),
                    QHashOut::rand(),
                    QHashOut::from_values(2, 0, 0, 0),
                    QHashOut::from_values(0, 0, 0, 0),
                ],
            },
            deploy_cmd,
        ],
        None,
        None,
        Some(UPS_SESSION_PROOF_TREE_HEIGHT as usize),
    )
    .await?;

    timer.lap("start build guta circuits");

    // Now QCircuitManager is just PsyUPSStepCircuitManager, so access directly
    let end_cap_proof_common_data = main_circuits.ups_end_cap.get_common_circuit_data_ref();
    use psy_config::get_default_worker_public_key;

    let guta_circuits = PsyGUTACircuitManager::<C, D>::new_with_config(
        end_cap_proof_common_data,
        UPSCircuitManager::ups_end_cap_circuit_verifier_config(&main_circuits)
            .await?
            .constants_sigmas_cap
            .height(),
        UPSCircuitManager::ups_end_cap_circuit_fingerprint(&main_circuits).await?,
        get_default_worker_public_key::<GoldilocksField>(),
    );
    timer.lap("built guta circuits");
    let proof_store = SimpleProofStoreMemory::new();

    let mut api = SimpleAPI::<_, _, GoldilocksField, C, D>::new(proof_store, lps.clone_read_store(), guta_circuits).await?;
    //main_circuits.print_common_config();
    api.guta_circuits.print_common_config();

    let mut circuit_info = SessionCircuitInfoStore::new();

    circuit_info.register_circuit(
        LocalCircuitType::SimpleZKSignature.into(),
        wallet.circuit.get_fingerprint(),
        wallet.circuit.get_verifier_config_ref().into(),
    );

    UPSCircuitManager::register_info(&main_circuits, &mut circuit_info).await;
    circuit_info.register_circuit(
        LocalCircuitId::new_cfc(contract_id.to_canonical_u64() as u32, simple_mint_debug_def.method_id),
        simple_mint_debug_circuit.get_fingerprint(),
        simple_mint_debug_circuit.get_verifier_config_ref().into(),
    );
    circuit_info.register_circuit(
        LocalCircuitId::new_cfc(contract_id.to_canonical_u64() as u32, simple_transfer_def.method_id),
        simple_transfer_circuit.get_fingerprint(),
        simple_transfer_circuit.get_verifier_config_ref().into(),
    );
    circuit_info.register_circuit(
        LocalCircuitId::new_cfc(contract_id.to_canonical_u64() as u32, simple_claim_def.method_id),
        simple_claim_circuit.get_fingerprint(),
        simple_claim_circuit.get_verifier_config_ref().into(),
    );

    let mut mgr = UserProvingSessionManager::<GoldilocksField, _, _, C, D>::new(
        lps,
        circuit_info,
        UPSCircuitManager::ups_circuit_whitelist_root(&main_circuits).await?,
    )
    .await?;

    timer.lap("START USER PROVING SESSION");

    mgr.prove_ups_start(&main_circuits).await?;
    timer.lap("proved ups_start");

    mgr.prove_contract_call(
        &main_circuits,
        contract_id,
        0,
        &simple_mint_debug_def,
        vec![GoldilocksField::from_noncanonical_u64(1000)],
    )
    .await?;
    timer.lap("proved token.simple_mint_debug(amount: 1000)");

    mgr.prove_contract_call(
        &main_circuits,
        contract_id,
        1,
        &simple_transfer_def,
        vec![GoldilocksField::from_noncanonical_u64(2), GoldilocksField::from_noncanonical_u64(100)],
    )
    .await?;
    timer.lap("proved token.simple_transfer(recipient: 2, amount: 100)");

    let new_nonce = GoldilocksField::from_noncanonical_u64(1);
    let sighash = mgr.get_sighash(PSY_NETWORK_MAGIC, new_nonce);

    let signature_proof = wallet.zk_sign_for_private_key_value(priv_key_0, sighash)?;
    timer.lap("generated zk signature for UPS transaction batch");
    mgr.proof_tree_state.finalize_tree(&main_circuits).await?;
    timer.lap("aggregated all UPS proofs into a single proof");
    let public_key_param = SimplePsyPrivateKey::new(priv_key_0).get_public_key_param::<PoseidonHash>();
    let end_cap_proof = mgr
        .prove_end_cap(
            &main_circuits,
            PSY_NETWORK_MAGIC,
            new_nonce,
            wallet.circuit.get_fingerprint(),
            public_key_param,
            signature_proof,
            wallet.circuit.get_verifier_config_ref().to_owned(),
        )
        .await?;
    timer.lap("Proved End Cap for UPS Session 🎉");

    // the end cap proof the proof that we send off to the network 🎉

    //main_circuits.ups_end_cap.circuit_data.verify(end_cap_proof)?;
    timer.lap("✅ Verified End Cap Proof");

    let user_a_api_input = SubmitUserEndCapProofAPIInput {
        input: mgr.get_api_input().await?,
        proof: end_cap_proof,
    };

    let mut mgr = mgr.into_clean_for_user(GoldilocksField::from_canonical_u32(6)).await?;

    timer.lap("START USER PROVING SESSION");

    mgr.prove_ups_start(&main_circuits).await?;
    timer.lap("proved ups_start");

    mgr.prove_contract_call(
        &main_circuits,
        contract_id,
        0,
        &simple_mint_debug_def,
        vec![GoldilocksField::from_noncanonical_u64(10000)],
    )
    .await?;
    timer.lap("proved token.simple_mint_debug(amount: 10000)");

    mgr.prove_contract_call(
        &main_circuits,
        contract_id,
        1,
        &simple_transfer_def,
        vec![GoldilocksField::from_noncanonical_u64(3), GoldilocksField::from_noncanonical_u64(1337)],
    )
    .await?;
    timer.lap("proved token.simple_transfer(recipient: 3, amount: 1337)");

    let new_nonce = GoldilocksField::from_noncanonical_u64(1);
    let sighash = mgr.get_sighash(PSY_NETWORK_MAGIC, new_nonce);

    let signature_proof = wallet.zk_sign_for_private_key_value(priv_key_1, sighash)?;
    timer.lap("generated zk signature for UPS transaction batch");
    mgr.proof_tree_state.finalize_tree(&main_circuits).await?;
    timer.lap("aggregated all UPS proofs into a single proof");
    let public_key_param = SimplePsyPrivateKey::new(priv_key_1).get_public_key_param::<PoseidonHash>();
    let end_cap_proof = mgr
        .prove_end_cap(
            &main_circuits,
            PSY_NETWORK_MAGIC,
            new_nonce,
            wallet.circuit.get_fingerprint(),
            public_key_param,
            signature_proof,
            wallet.circuit.get_verifier_config_ref().to_owned(),
        )
        .await?;
    timer.lap("Proved End Cap for UPS Session 🎉");

    // the end cap proof the proof that we send off to the network 🎉

    //main_circuits.ups_end_cap.circuit_data.verify(end_cap_proof)?;
    timer.lap("✅ Verified End Cap Proof");

    let user_b_api_input = SubmitUserEndCapProofAPIInput {
        input: mgr.get_api_input().await?,
        proof: end_cap_proof,
    };

    api.submit_proof(user_a_api_input)?;
    api.submit_proof(user_b_api_input)?;
    timer.lap("start generating start witnesses");

    let (pairs, left_over) = api.get_start_witnesses().await?;
    timer.lap("finished generating start witnesses");
    for p in pairs.into_iter() {
        let verifier = UPSCircuitManager::ups_end_cap_circuit_verifier_config(&main_circuits).await?;
        let _proof = api.proof_start_dbg(p, &verifier)?;
        timer.lap("Proved Recursive Global User Tree Aggregation");
    }

    //guta_circuits.verify_two_end_cap.prove_base(input, child_a_proof,
    // child_b_proof, end_cap_verifier_data)

    Ok(())
}

#[tokio::main]
async fn main() {
    demo_user_proving_session().await.unwrap();
}
