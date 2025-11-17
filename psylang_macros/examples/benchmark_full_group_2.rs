use std::{sync::Arc, time::Duration};

use clap::Parser;
use fred::prelude::*;
use kvq::memory::simple::KVQSimpleMemoryBackingStore;
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
    coordinator::state::{
        edge::CoordinatorEdgeContext,
        processor::{CoordinatorConfig, CoordinatorProcessorContext},
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
    store::journal::JournalStore,
};
use psy_ups_circuit::{
    circuit_manager::core::{PsyUPSStepCircuitManager, QCircuitManager},
    session::UserProvingSessionManager,
};
use psy_vm::ups::circuit_manager::UPSCircuitManager;

mod test_helpers;
use test_helpers::{contract::gen_test_contract, ups::ExampleDemoUserInfoStore};

#[derive(Parser, Debug)]
struct BenchmarkFullGroup2Args {
    #[arg(long, default_value = "redis://127.0.0.1:6379")]
    redis_url: String,

    #[arg(long, default_value = "10")]
    num_workers: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    psy_common::setup_logging()?;
    run(BenchmarkFullGroup2Args::parse()).await
}

async fn run_test3() -> anyhow::Result<()> {
    type C = PoseidonGoldilocksConfig;
    const D: usize = 2;
    let mut timer = DebugTimer::new("dq_rust_2v2");
    timer.lap("start");

    let q = ProofStoreRedis::new("redis://127.0.0.1:6379", "wq1".to_string()).await?;
    let realm_q = ProofStoreRedis::new("redis://127.0.0.1:6379", "rwq1".to_string()).await?;
    timer.lap("connected to redis");

    let store_reader: Arc<KVQSimpleMemoryBackingStore> = Arc::new(KVQSimpleMemoryBackingStore::new());

    store_reader.initialize_store(None).await?;
    //let worker_count = 16usize;
    //let items_per_worker = 2000usize;

    let coord_config = CoordinatorConfig::get_standard();

    let qps = Arc::new(q.clone());

    let realm_qps = Arc::new(realm_q.clone());

    let st = store_reader.clone();

    timer.lap("initialized store");

    let task_store = Arc::new(
        QProvingTaskStoreImpl::new("redis://127.0.0.1/", 10, "biz_key")
            .await
            .expect("Failed to create JobTaskStore"),
    );

    let proof_verifier = Arc::new(get_cached_generic_verifier::<C, D>());
    timer.lap("created proof verifier");

    use psy_config::get_default_worker_public_key;
    let coordinator_worker_circuits =
        PsyCoordinatorCircuitManager::<C, D>::new_with_library(&proof_verifier.library, get_default_worker_public_key::<GoldilocksField>());

    timer.lap("built coordinator worker circuits");

    let coordinator_edge_node = CoordinatorEdgeContext::new(coord_config, st.clone(), qps.clone(), qps.clone(), Arc::clone(&proof_verifier)).await?;

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

    let mut helper = ExampleDemoUserInfoStore::new();

    timer.lap("finished building wallet/zksig/helper circuits");
    let (contract_helper, contract_deploy_cmd) = gen_test_contract::<C, D>(QHashOut::rand())?;
    coordinator_edge_node.handle_deploy_contract(contract_deploy_cmd).await?;

    let mut current_g_reg_id = 0;
    let priv_keys_batch_1 = (0..100).map(|x| QHashOut::rand()).collect::<Vec<_>>();
    let mut g_users = helper
        .register_users(&coordinator_edge_node, current_g_reg_id, &priv_keys_batch_1)
        .await?;

    timer.lap("deployed contract and registered users");

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

    let realm_result: GUTARealmCheckpointResult<PsyFelt> = {
        let bytes = realm_qps.get_bytes_by_id(realm_worker_output_job_id).await?;
        bincode::deserialize(&bytes)
    }
    .map_err(|e| anyhow::anyhow!("{:?}", e))?;
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

    let lps: PsyLocalProvingSessionStore<GoldilocksField, Arc<KVQSimpleMemoryBackingStore>> = PsyLocalProvingSessionStore::new_at(
        store_reader.clone(),
        GoldilocksField::from_noncanonical_u64(latest_block_state.checkpoint_id),
        GoldilocksField::from_noncanonical_u64(0),
        GoldilocksField::ONE,
        UPS_SESSION_PROOF_TREE_HEIGHT as usize,
    );

    let mut circuit_info = SessionCircuitInfoStore::new();

    circuit_info.register_circuit(
        LocalCircuitType::SimpleZKSignature.into(),
        helper.wallet.circuit.get_fingerprint(),
        helper.wallet.circuit.get_verifier_config_ref().into(),
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
    type F = GoldilocksField;
    let mut mgr = helper
        .run_txs_for_users_prep(
            mgr,
            &contract_helper,
            &main_circuits,
            0,
            vec![(
                g_users[0],
                vec![
                    ("simple_mint_debug", vec![F::from_canonical_u64(1000)]),
                    ("simple_transfer", vec![F::from_canonical_u64(g_users[1]), F::from_canonical_u64(100)]),
                ],
            )],
        )
        .await?;

    timer.lap("proved group");

    helper.send_txs_to_edge(&realm_edge_node).await?;
    timer.lap("sent all to edge");

    realm_processor_node.build_block(realm_processor_node.latest_checkpoint().await? + 1, 0).await?;
    timer.lap("built block");
    let realm_worker_output_job_id =
        SimpleAsyncRealmWorker::run_worker_until_done::<_, _, SimpleCircuitLibrary<GoldilocksField>, PsyCoordinatorCircuitManager<C, D>, C, D>(
            &realm_q.clone(),
            &realm_q.clone(),
            &coordinator_worker_circuits,
            &proof_verifier.library,
        )
        .await?;

    let realm_result: GUTARealmCheckpointResult<PsyFelt> = {
        let bytes = realm_qps.get_bytes_by_id(realm_worker_output_job_id).await?;
        bincode::deserialize(&bytes)
    }
    .map_err(|e| anyhow::anyhow!("{:?}", e))?;
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

    realm_processor_node.build_block(realm_processor_node.latest_checkpoint().await? + 1, 0).await?;
    timer.lap("built block");
    let realm_worker_output_job_id =
        SimpleAsyncRealmWorker::run_worker_until_done::<_, _, SimpleCircuitLibrary<GoldilocksField>, PsyCoordinatorCircuitManager<C, D>, C, D>(
            &realm_q.clone(),
            &realm_q.clone(),
            &coordinator_worker_circuits,
            &proof_verifier.library,
        )
        .await?;

    let realm_result: GUTARealmCheckpointResult<PsyFelt> = {
        let bytes = realm_qps.get_bytes_by_id(realm_worker_output_job_id).await?;
        bincode::deserialize(&bytes)
    }
    .map_err(|e| anyhow::anyhow!("{:?}", e))?;
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
pub async fn run(args: BenchmarkFullGroup2Args) -> anyhow::Result<()> {
    run_test3().await
}
