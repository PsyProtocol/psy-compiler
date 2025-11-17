use std::sync::Arc;
// use psy_user_cli::subcommand::lps::run_local;
// use reth_libmdbx::{Environment, EnvironmentFlags, Geometry, Mode, PageSize, SyncMode, RW};
use std::{path::PathBuf, time::Duration};

use clap::Parser;
use fred::prelude::*;
use plonky2::{
    field::{goldilocks_field::GoldilocksField, types::Field},
    hash::poseidon::PoseidonHash,
    plonk::config::PoseidonGoldilocksConfig,
};
use psy_common::{
    data::qhashout::QHashOut,
    job::traits::{QProofStoreAsyncImm, QProofStoreReaderAsync},
    ups::circuits::LocalCircuitType,
    utils::debug_timer::DebugTimer,
};
use psy_common_circuit::circuits::{traits::qstandard::QStandardCircuit, zk_signature3::manager::SimplePsyZKSignatureManager};
use psy_config::{network_constants::UPS_SESSION_PROOF_TREE_HEIGHT, PSY_NETWORK_MAGIC};
use psy_crypto::{
    common::simple_circuit_library::SimpleCircuitLibrary,
    hash::traits::qhashable::QFieldHashable,
    signature::zk::{data::ZKPublicKeyInfo, wallet::SimplePsyPrivateKey},
};
use psy_data::{
    config::store_config::{PsyFelt, PsyHasher},
    guta::api::{GUTARealmCheckpointResult, SubmitGUTARealmResultAPINoProofInput, SubmitUserEndCapProofAPIInput},
    qstore::controllers::{proving_session::PsyLocalProvingSessionStore, session_info::SessionCircuitInfoStore},
    traits::qdatastore::qtreedata::PsyComboDataStoreReaderWriterSync,
};
use psy_network_circuit::coordinator::coordinator_helper::PsyCoordinatorCircuitManager;
use psy_node::{
    common::verifier::get_cached_generic_verifier,
    coordinator::{
        edge::rpc::CoordinatorEdgeRpcClient,
        state::{
            edge::CoordinatorEdgeContext,
            processor::{CoordinatorConfig, CoordinatorProcessorContext},
        },
    },
    realm::state::{
        edge::RealmEdgeContext,
        processor::{RealmConfig, RealmProcessorContext},
    },
    worker::{simple_async_coord::SimpleAsyncCoordinatorWorker, simple_async_realm::SimpleAsyncRealmWorker},
};
use psy_store::{
    node::coordinator::{PsyCoordinatorStoreReaderAsync, PsyCoordinatorStoreWriterAsyncImm},
    queue::{
        task_queue::{QProvingTaskStore, QProvingTaskStoreImpl},
        ProofStoreRedis,
    },
    store::{journal::JournalStore, KVQlibmdbxStore, PsyStore},
};
use psy_ups_circuit::{
    circuit_manager::core::{PsyUPSStepCircuitManager, QCircuitManager},
    session::UserProvingSessionManager,
};
use psy_vm::ups::circuit_manager::UPSCircuitManager;

mod test_helpers;
use test_helpers::contract::{gen_test_contract, gen_test_contract_2};

#[derive(Parser, Debug)]
struct TestFullGroup1Args {
    #[arg(long, default_value = "redis://127.0.0.1:6379")]
    redis_url: String,

    #[arg(long, default_value = "db")]
    db_path: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    psy_common::setup_logging()?;
    run(TestFullGroup1Args::parse()).await
}
async fn run_test3() -> anyhow::Result<()> {
    type C = PoseidonGoldilocksConfig;
    const D: usize = 2;
    let mut timer = DebugTimer::new("dq_rust_2v2");
    timer.lap("start");
    let q = ProofStoreRedis::new("redis://127.0.0.1:6379", "wq1".to_string()).await?;
    let realm_q = ProofStoreRedis::new("redis://127.0.0.1:6379", "rwq1".to_string()).await?;
    timer.lap("connected to redis");
    let store_reader = Arc::new(KVQlibmdbxStore::new_write("db")?);
    store_reader.initialize_store(None).await?;
    //let worker_count = 16usize;
    //let items_per_worker = 2000usize;
    let coord_config = CoordinatorConfig::get_standard();
    let qps = Arc::new(q.clone());
    let realm_qps = Arc::new(realm_q.clone());
    let st = store_reader.clone();
    timer.lap("initialized store");
    let task_store = Arc::new(
        QProvingTaskStoreImpl::new("redis://127.0.0.1/", 10, "biz_key1")
            .await
            .expect("Failed to create JobTaskStore"),
    );
    let proof_verifier = Arc::new(get_cached_generic_verifier::<C, D>());
    timer.lap("created proof verifier");
    use psy_config::get_default_worker_public_key;
    let coordinator_worker_circuits =
        PsyCoordinatorCircuitManager::<C, D>::new_with_library(&proof_verifier.library, get_default_worker_public_key::<GoldilocksField>());
    timer.lap("built coordinator worker circuits");
    let coordinator_edge_node =
        CoordinatorEdgeContext::new(coord_config, Arc::clone(&st), qps.clone(), qps.clone(), Arc::clone(&proof_verifier)).await?;
    let mut coordinator_processor_node = CoordinatorProcessorContext::new(
        coord_config,
        Arc::new(JournalStore::new(store_reader.clone())),
        qps.clone(),
        qps.clone(),
        qps.clone(),
        qps.clone(),
        task_store.clone(),
        Arc::clone(&proof_verifier),
        None,
        None,
    )
    .await?;
    timer.lap("created coordinator nodes");
    let priv_key_0 = QHashOut::rand();
    let priv_key_1 = QHashOut::rand();
    let mut wallet = SimplePsyZKSignatureManager::<C, D>::new();
    let pub_key_0 = wallet.add_private_key_get_info(SimplePsyPrivateKey::new(priv_key_0));
    let pub_alt_0 = SimplePsyPrivateKey::new(priv_key_0).get_public_key_for_fingerprint::<PsyHasher>(wallet.circuit.get_fingerprint());
    let pub_key_1 = wallet.add_private_key_get_info(SimplePsyPrivateKey::new(priv_key_1));
    timer.lap("finished building wallet/zksig circuits");
    let (contract_helper, contract_deploy_cmd) = gen_test_contract_2::<C, D>(pub_key_1.qfhash::<PsyHasher>())?;
    coordinator_edge_node.handle_deploy_contract(contract_deploy_cmd).await?;
    coordinator_edge_node.handle_process_regsiter_user(pub_key_0).await?;
    coordinator_edge_node.handle_process_regsiter_user(pub_key_1).await?;
    timer.lap("sent requests");
    coordinator_processor_node.build_block(0).await?;
    SimpleAsyncCoordinatorWorker::run_worker_until_done::<_, _, SimpleCircuitLibrary<GoldilocksField>, PsyCoordinatorCircuitManager<C, D>, C, D>(
        &q.clone(),
        &q.clone(),
        &coordinator_worker_circuits,
        &proof_verifier.library,
    )
    .await?;
    let realm_config = RealmConfig::get_standard(0);
    let realm_edge_node = RealmEdgeContext::new(
        realm_config,
        st.clone(),
        realm_qps.clone(),
        realm_qps.clone(),
        Arc::clone(&proof_verifier),
    )
    .await?;
    let mut realm_processor_node = RealmProcessorContext::new(
        realm_config,
        None,
        JournalStore::new(st.clone()),
        realm_qps.clone(),
        realm_qps.clone(),
        realm_qps.clone(),
        realm_qps.clone(),
        task_store.clone(),
        Arc::clone(&proof_verifier),
    )
    .await?;
    //realm_edge_node.handle_recv_checkpoint_sync(coordinator_processor_node.store.
    // get_checkpoint_sync_info_compact(1).await?).await?;
    let sync1 = coordinator_processor_node.store.get_checkpoint_sync_info_compact(1).await?;
    realm_processor_node.handle_checkpoint_sync(sync1).await?;
    realm_processor_node.build_block(realm_processor_node.latest_checkpoint().await? + 1, 0).await?;
    let realm_worker_output_job_id =
        SimpleAsyncRealmWorker::run_worker_until_done::<_, _, SimpleCircuitLibrary<GoldilocksField>, PsyCoordinatorCircuitManager<C, D>, C, D>(
            &realm_q.clone(),
            &realm_q.clone(),
            &coordinator_worker_circuits,
            &proof_verifier.library,
        )
        .await?;
    let realm_result: GUTARealmCheckpointResult<PsyFelt> =
        bincode::deserialize(&realm_qps.get_bytes_by_id(realm_worker_output_job_id).await?).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    let realm_proof = realm_qps.get_proof_by_id(realm_result.proof_id).await?;
    coordinator_edge_node
        .handle_recv_guta_from_realm(
            SubmitGUTARealmResultAPINoProofInput {
                realm_id: 0,
                checkpoint_id: realm_result.checkpoint_id,
                guta_stats: realm_result.guta_stats,
                top_line_proof: realm_result.top_line_proof,
                checkpoint_tree_root: realm_result.checkpoint_tree_root,
                proof_id: realm_result.proof_id,
            },
            &realm_proof,
        )
        .await?;
    coordinator_processor_node.build_block(0).await?;
    SimpleAsyncCoordinatorWorker::run_worker_until_done::<_, _, SimpleCircuitLibrary<GoldilocksField>, PsyCoordinatorCircuitManager<C, D>, C, D>(
        &q.clone(),
        &q.clone(),
        &coordinator_worker_circuits,
        &proof_verifier.library,
    )
    .await?;
    let latest = store_reader.get_latest_block_state().await?;
    let new_sync = store_reader.get_checkpoint_sync_info_compact(latest.checkpoint_id).await?;
    realm_processor_node.handle_checkpoint_sync(new_sync).await?;
    let latest_block_state = st.get_latest_block_state().await?;
    //let stroots =
    // st.get_checkpoint_global_state_roots(latest_block_state.checkpoint_id).
    // await?; println!("[mainfnc] current_state_roots:
    // {}",serde_json::to_string_pretty(&stroots).unwrap());
    timer.lap("start: init PsyUPSStepCircuitManager");
    let main_circuits = PsyUPSStepCircuitManager::<C, D>::new_with_config(PSY_NETWORK_MAGIC);
    let boxed_main_circuits: QCircuitManager<C, D> = Box::new(PsyUPSStepCircuitManager::<C, D>::new_with_config(PSY_NETWORK_MAGIC));
    //main_circuits.print_common_config();
    timer.lap("end: init PsyUPSStepCircuitManager");
    let user_0_pub_key = st.get_user_registration_tree_leaf_hash(latest_block_state.checkpoint_id, 0).await?;
    let priv_key_user_0 = if pub_key_0.qfhash::<PsyHasher>() == user_0_pub_key {
        priv_key_0
    } else if pub_key_1.qfhash::<PsyHasher>() == user_0_pub_key {
        priv_key_1
    } else {
        anyhow::bail!("missing private key!");
    };
    let lps: PsyLocalProvingSessionStore<GoldilocksField, Arc<KVQlibmdbxStore>> = PsyLocalProvingSessionStore::new_at(
        store_reader.clone(),
        GoldilocksField::from_noncanonical_u64(latest_block_state.checkpoint_id),
        GoldilocksField::from_noncanonical_u64(0),
        GoldilocksField::ONE,
        UPS_SESSION_PROOF_TREE_HEIGHT as usize,
    );
    let mut circuit_info = SessionCircuitInfoStore::new();
    circuit_info.register_circuit(
        LocalCircuitType::SimpleZKSignature.into(),
        wallet.circuit.get_fingerprint(),
        wallet.circuit.get_verifier_config_ref().into(),
    );
    main_circuits.register_info(&mut circuit_info);
    contract_helper.register_funcs(0, &mut circuit_info);
    let mut mgr = UserProvingSessionManager::<GoldilocksField, PoseidonHash, _, C, D>::new(
        lps,
        circuit_info,
        main_circuits.ups_circuit_whitelist_root().await?,
    )
    .await?;
    timer.lap("setup mgr");
    timer.lap("started up");
    timer.lap("START USER PROVING SESSION");
    mgr.prove_ups_start(&main_circuits).await?;
    timer.lap("proved ups_start");
    contract_helper
        .prove_func(
            &main_circuits,
            &mut mgr,
            0,
            "simple_mint_debug",
            vec![GoldilocksField::from_noncanonical_u64(1000)],
        )
        .await?;
    timer.lap("proved token.simple_mint_debug(amount: 1000)");
    contract_helper
        .prove_func(
            &main_circuits,
            &mut mgr,
            0,
            "simple_transfer",
            vec![GoldilocksField::from_noncanonical_u64(1), GoldilocksField::from_noncanonical_u64(100)],
        )
        .await?;
    timer.lap("proved token.simple_transfer(recipient: 2, amount: 100)");
    let new_nonce = GoldilocksField::from_noncanonical_u64(1);
    let sighash = mgr.get_sighash(PSY_NETWORK_MAGIC, new_nonce);
    let signature_proof = wallet.zk_sign_for_private_key_value(priv_key_user_0, sighash)?;
    timer.lap("generated zk signature for UPS transaction batch");
    mgr.proof_tree_state.finalize_tree(&main_circuits).await?;
    timer.lap("aggregated all UPS proofs into a single proof");
    let public_key_param = SimplePsyPrivateKey::new(priv_key_user_0).get_public_key_param::<PsyHasher>();
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
    /*
    let user_a_api_input = SubmitUserEndCapProofAPIInput{
        input: mgr.get_api_input()?,
        proof: end_cap_proof,
    };*/
    realm_edge_node
        .handle_recv_end_cap_from_user(mgr.get_api_input().await?, &end_cap_proof)
        .await?;
    // let user_0_pub_key =
    // st.get_user_registration_tree_leaf_hash(latest_block_state.checkpoint_id,
    // 0).await?; let priv_key_user_0 = if pub_key_0.qfhash::<PsyHasher>() ==
    // user_0_pub_key {     priv_key_0
    // }else if pub_key_1.qfhash::<PsyHasher>() == user_0_pub_key {
    //     priv_key_1
    // }else{
    //     anyhow::bail!("missing private key!");
    // };
    // let (exec_input , end_cap_proof) = run_local(st.dup(),
    // "/home/longer/workspace/private/psylang-rust-dev/contract_call.json",
    // &priv_key_user_0.to_string())?;
    // realm_edge_node.handle_recv_end_cap_from_user(
    //     exec_input,
    //     &end_cap_proof
    // ).await?;
    realm_processor_node.build_block(realm_processor_node.latest_checkpoint().await? + 1, 0).await?;
    let realm_worker_output_job_id =
        SimpleAsyncRealmWorker::run_worker_until_done::<_, _, SimpleCircuitLibrary<GoldilocksField>, PsyCoordinatorCircuitManager<C, D>, C, D>(
            &realm_q.clone(),
            &realm_q.clone(),
            &coordinator_worker_circuits,
            &proof_verifier.library,
        )
        .await?;
    let realm_result: GUTARealmCheckpointResult<PsyFelt> =
        bincode::deserialize(&realm_qps.get_bytes_by_id(realm_worker_output_job_id).await?).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    println!("rr: {:?}", realm_result);
    let realm_proof = realm_qps.get_proof_by_id(realm_result.proof_id.get_output_id()).await?;
    coordinator_edge_node
        .handle_recv_guta_from_realm(
            SubmitGUTARealmResultAPINoProofInput {
                realm_id: 0,
                checkpoint_id: realm_result.checkpoint_id,
                guta_stats: realm_result.guta_stats,
                top_line_proof: realm_result.top_line_proof,
                checkpoint_tree_root: realm_result.checkpoint_tree_root,
                proof_id: realm_result.proof_id,
            },
            &realm_proof,
        )
        .await?;
    coordinator_processor_node.build_block(0).await?;
    SimpleAsyncCoordinatorWorker::run_worker_until_done::<_, _, SimpleCircuitLibrary<GoldilocksField>, PsyCoordinatorCircuitManager<C, D>, C, D>(
        &q.clone(),
        &q.clone(),
        &coordinator_worker_circuits,
        &proof_verifier.library,
    )
    .await?;
    let latest = store_reader.get_latest_block_state().await?;
    let new_sync = store_reader.get_checkpoint_sync_info_compact(latest.checkpoint_id).await?;
    realm_processor_node.handle_checkpoint_sync(new_sync).await?;
    timer.lap("finished jobs");
    Ok(())
}
pub async fn run(args: TestFullGroup1Args) -> anyhow::Result<()> {
    run_test3().await
}
