use kvq::memory::simple::KVQSimpleMemoryBackingStore;
use plonky2::field::types::Field;
use psy_common::data::qhashout::QHashOut;
use psy_config::network_constants::GLOBAL_USER_TREE_HEIGHT;
use psy_data::{
    config::store_config::PsyFelt,
    qblock::{
        cmds::{deploy_contract::QBCDeployContract, register_user::QBCRegisterUser},
        process::simple::SimpleBlockProcessor,
    },
    qdata::checkpoint::{PsyBlockState, PsyCheckpointLeaf, PsyCheckpointLeafStats},
    qstore::controllers::proving_session::PsyLocalProvingSessionStore,
    traits::qdatastore::qmetadata::{QMetaDataStoreReaderSync, QMetaDataStoreWriterSync},
};

pub async fn prepare_environment_with_real_contract(
    register_users: Vec<QBCRegisterUser<PsyFelt>>,
    deploy_contracts: Vec<QBCDeployContract<PsyFelt>>,
    user_id: Option<u64>,
    nonce: Option<PsyFelt>,
    session_proof_tree_height: Option<usize>,
) -> anyhow::Result<PsyLocalProvingSessionStore<PsyFelt, KVQSimpleMemoryBackingStore>> {
    let store = KVQSimpleMemoryBackingStore::new();

    // Initialize empty genesis state
    store.set_block_state(&PsyBlockState::get_genesis_value())?;
    store.set_checkpoint_leaf_data(
        0,
        &PsyCheckpointLeaf {
            global_chain_root: QHashOut::ZERO,
            stats: PsyCheckpointLeafStats::new_empty(),
        },
    )?;

    let final_store = SimpleBlockProcessor::prepare_environment_with_real_contract(register_users, deploy_contracts, store).await?;

    let latest_block_state = final_store.get_latest_block_state().await?;
    let final_user_id = PsyFelt::from_canonical_u64(user_id.unwrap_or(5));
    let final_nonce = nonce.unwrap_or(PsyFelt::ZERO);
    let final_height = session_proof_tree_height.unwrap_or(GLOBAL_USER_TREE_HEIGHT as usize);
    let final_checkpoint_id = PsyFelt::from_canonical_u64(latest_block_state.checkpoint_id);

    Ok(PsyLocalProvingSessionStore::new_at(
        final_store,
        final_checkpoint_id,
        final_user_id,
        final_nonce,
        PsyFelt::ZERO,
        final_height,
    ))
}
