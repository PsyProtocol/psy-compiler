use std::collections::HashMap;

use plonky2::{
    field::{goldilocks_field::GoldilocksField, types::Field},
    hash::poseidon::PoseidonHash,
    plonk::{config::PoseidonGoldilocksConfig, proof::ProofWithPublicInputs},
};
use psy_common::{
    data::qhashout::QHashOut,
    job::{drain_queue::CheckpointDrainQueueEmitterAsyncImm, traits::QProofStoreAsyncImm},
    utils::debug_timer::DebugTimer,
};
use psy_common_circuit::{
    circuits::{traits::qstandard::QStandardCircuit, zk_signature3::manager::SimplePsyZKSignatureManager},
    wallet::zk::SimpleZKSignatureWallet,
};
use psy_config::PSY_NETWORK_MAGIC;
use psy_crypto::{common::user_id::get_user_id_from_registration_id, signature::zk::wallet::SimplePsyPrivateKey};
use psy_data::{
    config::store_config::PsyHasher, guta::end_cap_input::SubmitUserEndCapNonProofInput, qstore::imm::cmd_processor::PsyReadCommandProcessorSync,
};
use psy_node::{coordinator::state::edge::CoordinatorEdgeContext, realm::state::edge::RealmEdgeContext};
use psy_store::node::{coordinator::PsyCoordinatorStoreReaderAsync, realm::PsyRealmStoreReaderAsync};
use psy_ups_circuit::{
    circuit_manager::core::{PsyUPSStepCircuitManager, QCircuitManager},
    session::UserProvingSessionManager,
};

use super::contract::SimpleTestContract;

type C = PoseidonGoldilocksConfig;
type F = GoldilocksField;
const D: usize = 2;

pub struct ExampleDemoUserInfoStore {
    pub user_private_keys: HashMap<u64, QHashOut<F>>,
    pub nonce_map: HashMap<u64, u64>,
    pub wallet: SimplePsyZKSignatureManager<C, D>,
    //pub mgr: UserProvingSessionManager<F, PoseidonHash, R, C, D>,
    pub current_user: u64,
    pub awaiting_send_end_caps: Vec<(SubmitUserEndCapNonProofInput<F>, ProofWithPublicInputs<F, C, D>)>,
}

impl ExampleDemoUserInfoStore {
    pub fn new() -> Self {
        Self {
            user_private_keys: HashMap::new(),
            wallet: SimplePsyZKSignatureManager::new(),
            current_user: 0,
            //mgr,
            nonce_map: HashMap::new(),
            awaiting_send_end_caps: Vec::new(),
        }
    }
    pub async fn register_users<SR: PsyCoordinatorStoreReaderAsync<F>, DQ: CheckpointDrainQueueEmitterAsyncImm, PS: QProofStoreAsyncImm>(
        &mut self,
        edge: &CoordinatorEdgeContext<SR, DQ, PS>,
        start_registration_id: u64,
        user_private_keys: &[QHashOut<F>],
    ) -> anyhow::Result<Vec<u64>> {
        let mut user_ids: Vec<u64> = Vec::with_capacity(user_private_keys.len());
        for (i, priv_key) in user_private_keys.iter().enumerate() {
            let user_id = get_user_id_from_registration_id(i as u64 + start_registration_id);
            let info = self.wallet.add_private_key_get_info(SimplePsyPrivateKey::new(*priv_key));
            self.user_private_keys.insert(user_id, *priv_key);
            edge.handle_process_regsiter_user(info).await?;

            user_ids.push(user_id);
        }

        Ok(user_ids)
    }

    pub async fn run_tx_for_current_user<
        R: PsyReadCommandProcessorSync<F>
            + psy_data::traits::qdatastore::qtreedata::PsyComboDataStoreReaderSync<F>
            + kvq::traits::KVQBinaryStore
            + Send
            + Sync,
    >(
        &self,
        mgr: &mut UserProvingSessionManager<F, PoseidonHash, R, C, D>,
        contract: &SimpleTestContract<C, D>,
        circuit_mgr: &PsyUPSStepCircuitManager<C, D>,
        contract_id: u32,
        fn_name: &str,
        inputs: Vec<F>,
    ) -> anyhow::Result<()> {
        contract.prove_func(circuit_mgr, mgr, contract_id, fn_name, inputs).await
    }
    pub async fn run_txs_for_current_user<
        R: PsyReadCommandProcessorSync<F>
            + psy_data::traits::qdatastore::qtreedata::PsyComboDataStoreReaderSync<F>
            + kvq::traits::KVQBinaryStore
            + Send
            + Sync,
    >(
        &mut self,
        mgr: &mut UserProvingSessionManager<F, PoseidonHash, R, C, D>,
        contract: &SimpleTestContract<C, D>,
        circuit_mgr: &PsyUPSStepCircuitManager<C, D>,
        contract_id: u32,
        calls: Vec<(&str, Vec<F>)>,
    ) -> anyhow::Result<()> {
        for (fn_name, inputs) in calls {
            contract.prove_func(circuit_mgr, mgr, contract_id, fn_name, inputs).await?;
        }
        Ok(())
    }

    pub async fn new_run_txs_for_user<
        R: PsyReadCommandProcessorSync<F>
            + psy_data::traits::qdatastore::qtreedata::PsyComboDataStoreReaderSync<F>
            + kvq::traits::KVQBinaryStore
            + Send
            + Sync,
    >(
        &mut self,
        mut mgr: UserProvingSessionManager<F, PoseidonHash, R, C, D>,

        contract: &SimpleTestContract<C, D>,
        circuit_mgr: &PsyUPSStepCircuitManager<C, D>,
        contract_id: u32,
        user_id_u64: u64,
        calls: Vec<(&str, Vec<F>)>,
    ) -> anyhow::Result<(
        UserProvingSessionManager<F, PoseidonHash, R, C, D>,
        SubmitUserEndCapNonProofInput<F>,
        ProofWithPublicInputs<F, C, D>,
    )> {
        let mut timer = DebugTimer::new("run_txs_for_users");
        let mut mgr = mgr.into_clean_for_user(F::from_noncanonical_u64(user_id_u64)).await?;
        //.into_clean_for_user(F::from_noncanonical_u64(user_id_u64))?;
        mgr.prove_ups_start(circuit_mgr).await?;

        for (fn_name, inputs) in calls {
            contract.prove_func(circuit_mgr, &mut mgr, contract_id, fn_name, inputs).await?;
        }

        let old_nonce = match self.nonce_map.get(&user_id_u64) {
            Some(x) => *x,
            None => 0,
        };
        let new_nonce = old_nonce + 1;
        self.nonce_map.insert(user_id_u64, new_nonce);

        let sighash = mgr.get_sighash(PSY_NETWORK_MAGIC, F::from_canonical_u64(new_nonce));

        let signature_proof = self
            .wallet
            .zk_sign_for_private_key_value(*self.user_private_keys.get(&user_id_u64).unwrap(), sighash)?;
        timer.lap("generated zk signature for UPS transaction batch");
        mgr.proof_tree_state.finalize_tree(circuit_mgr).await?;
        timer.lap("aggregated all UPS proofs into a single proof");
        let public_key_param = SimplePsyPrivateKey::new(*self.user_private_keys.get(&user_id_u64).unwrap()).get_public_key_param::<PsyHasher>();
        let end_cap_proof = mgr
            .prove_end_cap(
                &circuit_mgr,
                PSY_NETWORK_MAGIC,
                F::from_canonical_u64(new_nonce),
                self.wallet.circuit.get_fingerprint(),
                public_key_param,
                signature_proof,
                self.wallet.circuit.get_verifier_config_ref().to_owned(),
            )
            .await?;
        timer.lap("Proved End Cap for UPS Session 🎉");

        // the end cap proof the proof that we send off to the network 🎉

        //main_circuits.ups_end_cap.circuit_data.verify(end_cap_proof)?;
        timer.lap("✅ Verified End Cap Proof");

        /*
        let user_a_api_input = SubmitUserEndCapProofAPIInput{
            input: mgr.get_api_input()?,
            proof: end_cap_proof,
        };*/
        let api_input = mgr.get_api_input().await?;
        Ok((mgr, api_input, end_cap_proof))
    }
    pub async fn run_txs_for_users<
        R: PsyReadCommandProcessorSync<F>
            + psy_data::traits::qdatastore::qtreedata::PsyComboDataStoreReaderSync<F>
            + kvq::traits::KVQBinaryStore
            + Send
            + Sync,
    >(
        &mut self,
        mut mgr: UserProvingSessionManager<F, PoseidonHash, R, C, D>,

        contract: &SimpleTestContract<C, D>,
        circuit_mgr: &PsyUPSStepCircuitManager<C, D>,
        contract_id: u32,
        user_calls: Vec<(u64, Vec<(&str, Vec<F>)>)>,
    ) -> anyhow::Result<(
        UserProvingSessionManager<F, PoseidonHash, R, C, D>,
        Vec<(SubmitUserEndCapNonProofInput<F>, ProofWithPublicInputs<F, C, D>)>,
    )> {
        let mut timer = DebugTimer::new("run_txs_for_users");
        let mut results = Vec::new();
        for (user_id_u64, calls) in user_calls {
            if !self.user_private_keys.contains_key(&user_id_u64) {
                anyhow::bail!("missing private key for user id {}", user_id_u64);
            }
            let nmgr = mgr.into_clean_for_user(F::from_noncanonical_u64(user_id_u64)).await?;
            mgr = nmgr;
            mgr.prove_ups_start(circuit_mgr).await?;

            for (fn_name, inputs) in calls {
                contract.prove_func(circuit_mgr, &mut mgr, contract_id, fn_name, inputs).await?;
            }

            let old_nonce = match self.nonce_map.get(&user_id_u64) {
                Some(x) => *x,
                None => 0,
            };
            let new_nonce = old_nonce + 1;
            self.nonce_map.insert(user_id_u64, new_nonce);

            let sighash = mgr.get_sighash(PSY_NETWORK_MAGIC, F::from_canonical_u64(new_nonce));

            let signature_proof = self
                .wallet
                .zk_sign_for_private_key_value(*self.user_private_keys.get(&user_id_u64).unwrap(), sighash)?;
            timer.lap("generated zk signature for UPS transaction batch");
            mgr.proof_tree_state.finalize_tree(circuit_mgr).await?;
            timer.lap("aggregated all UPS proofs into a single proof");
            let public_key_param = SimplePsyPrivateKey::new(*self.user_private_keys.get(&user_id_u64).unwrap()).get_public_key_param::<PsyHasher>();
            let end_cap_proof = mgr
                .prove_end_cap(
                    &circuit_mgr,
                    PSY_NETWORK_MAGIC,
                    F::from_canonical_u64(new_nonce),
                    self.wallet.circuit.get_fingerprint(),
                    public_key_param,
                    signature_proof,
                    self.wallet.circuit.get_verifier_config_ref().to_owned(),
                )
                .await?;
            timer.lap("Proved End Cap for UPS Session 🎉");

            // the end cap proof the proof that we send off to the network 🎉

            //main_circuits.ups_end_cap.circuit_data.verify(end_cap_proof)?;
            timer.lap("✅ Verified End Cap Proof");

            /*
            let user_a_api_input = SubmitUserEndCapProofAPIInput{
                input: mgr.get_api_input()?,
                proof: end_cap_proof,
            };*/

            results.push((mgr.get_api_input().await?, end_cap_proof));
        }
        Ok((mgr, results))
    }
    pub async fn run_txs_for_users_prep<
        R: PsyReadCommandProcessorSync<F>
            + psy_data::traits::qdatastore::qtreedata::PsyComboDataStoreReaderSync<F>
            + kvq::traits::KVQBinaryStore
            + Send
            + Sync,
    >(
        &mut self,
        mgr: UserProvingSessionManager<F, PoseidonHash, R, C, D>,

        contract: &SimpleTestContract<C, D>,
        circuit_mgr: &PsyUPSStepCircuitManager<C, D>,
        contract_id: u32,
        user_calls: Vec<(u64, Vec<(&str, Vec<F>)>)>,
    ) -> anyhow::Result<(UserProvingSessionManager<F, PoseidonHash, R, C, D>)> {
        let (new_mgr, mut results) = self.run_txs_for_users(mgr, contract, circuit_mgr, contract_id, user_calls).await?;
        self.awaiting_send_end_caps.append(&mut results);
        Ok(new_mgr)
    }
    pub async fn send_txs_to_edge<SR: PsyRealmStoreReaderAsync<F> + Sync, DQ: CheckpointDrainQueueEmitterAsyncImm, PS: QProofStoreAsyncImm>(
        &mut self,
        edge: &RealmEdgeContext<SR, DQ, PS>,
    ) -> anyhow::Result<()> {
        self.awaiting_send_end_caps = {
            let r = self.awaiting_send_end_caps.split_off(0);
            for (input, proof) in r {
                edge.handle_recv_end_cap_from_user(input, &proof).await?;
            }

            Vec::new()
        };

        Ok(())
    }
}
