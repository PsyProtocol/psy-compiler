use crate::{ExprId, Location, NodeInfo, NodeType, UncheckedType};

#[derive(Clone, Debug, PartialEq)]
pub enum IntrinsicExprNode {
    GetUserId {
        location: Location,
    },
    GetContractId {
        location: Location,
    },
    GetContractDeployer {
        contract_id: ExprId,
        location: Location,
    },
    GetContractStateTreeHeight {
        contract_id: ExprId,
        location: Location,
    },
    GetCallerContractId {
        location: Location,
    },
    GetCheckpointId {
        location: Location,
    },
    GetCheckpointStats {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetRegisterUsersRoot {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetGutasRoot {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetCheckpointUserTreeRoot {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetCheckpointContractTreeRoot {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetCheckpointDepositTreeRoot {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetCheckpointWithdrawalTreeRoot {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetCheckpointUserRegistrationTreeRoot {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetDeployContractsRoot {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetGutaFeesCollected {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetDaFeesCollected {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetUserOpsProcessed {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetTotalTransactions {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetSlotsModified {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetRegisterUsersCompleted {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetGutasCompleted {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetDeployContractsCompleted {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetLastNonce {
        location: Location,
    },
    GetUserPublicKeyHash {
        location: Location,
    },
    GetSessionProofTreeRoot {
        location: Location,
    },
    GetStateHashAt {
        slot_index: ExprId,
        location: Location,
    },
    ImtGet {
        key: ExprId,
        base_offset: ExprId,
        capacity: ExprId,
        location: Location,
    },
    ImtGetOtherUser {
        contract_state_tree_height: ExprId,
        user_id: ExprId,
        contract_id: ExprId,
        key: ExprId,
        base_offset: ExprId,
        capacity: ExprId,
        location: Location,
    },
    ImtContainsOtherUser {
        contract_state_tree_height: ExprId,
        user_id: ExprId,
        contract_id: ExprId,
        key: ExprId,
        base_offset: ExprId,
        capacity: ExprId,
        location: Location,
    },
    GetOtherContractStateHashAt {
        contract_state_tree_height: ExprId,
        contract_id: ExprId,
        slot_index: ExprId,
        location: Location,
    },
    GetOtherUserContractStateHashAt {
        contract_state_tree_height: ExprId,
        user_id: ExprId,
        contract_id: ExprId,
        slot_index: ExprId,
        location: Location,
    },
    CSetStateHashAt {
        slot_index: ExprId,
        new_value: ExprId,
        location: Location,
    },
    ImtSet {
        key: ExprId,
        new_value: ExprId,
        base_offset: ExprId,
        capacity: ExprId,
        location: Location,
    },
    ImtContains {
        key: ExprId,
        base_offset: ExprId,
        capacity: ExprId,
        location: Location,
    },
    MemTransmute {
        data: ExprId,
        target_type: UncheckedType,
        location: Location,
    },
    MemSizeOf {
        query_type: UncheckedType,
        location: Location,
    },
    StorageRead {
        contract_state_tree_height: ExprId,
        user_id: ExprId,
        contract_id: ExprId,
        offset: ExprId,
        location: Location,
    },
    StorageReadRange {
        contract_state_tree_height: ExprId,
        user_id: ExprId,
        contract_id: ExprId,
        offset: ExprId,
        length: ExprId,
        location: Location,
    },
    StorageWrite {
        offset: ExprId,
        value: ExprId,
        location: Location,
    },
    StorageWriteRange {
        offset: ExprId,
        values: ExprId,
        location: Location,
    },
    Hash {
        data: ExprId,
        location: Location,
    },
    Keccak256 {
        data: ExprId,
        location: Location,
    },
    HashTwoToOne {
        left: ExprId,
        right: ExprId,
        location: Location,
    },
    InvokeSync {
        contract_id: ExprId,
        method_id: ExprId,
        inputs: ExprId,
        return_type: UncheckedType,
        location: Location,
    },
    InvokeDeferred {
        contract_id: ExprId,
        method_id: ExprId,
        inputs: ExprId,
        location: Location,
    },
    Secp256k1Verify {
        pub_key: ExprId,
        msg: ExprId,
        sig: ExprId,
        location: Location,
    },
    SumBits {
        bits: ExprId,
        location: Location,
    },
    SplitBits {
        target: ExprId,
        num_bits: ExprId,
        location: Location,
    },
    Emit {
        event_data: ExprId,
        location: Location,
    },
}

impl NodeInfo for IntrinsicExprNode {
    fn node_type(&self) -> NodeType {
        NodeType::IntrinsicExpr
    }
}

impl IntrinsicExprNode {
    pub fn raw_name(&self) -> Option<&'static str> {
        use IntrinsicExprNode::*;
        Some(match self {
            GetUserId { .. } => "__ctx_get_user_id",
            GetContractId { .. } => "__ctx_get_contract_id",
            GetContractDeployer { .. } => "__ctx_get_contract_deployer",
            GetContractStateTreeHeight { .. } => "__ctx_get_contract_state_tree_height",
            GetCallerContractId { .. } => "__ctx_get_caller_contract_id",
            GetCheckpointId { .. } => "__ctx_get_checkpoint_id",
            GetCheckpointStats { .. } => "__ctx_get_checkpoint_stats",
            GetRegisterUsersRoot { .. } => "__ctx_get_register_users_root",
            GetGutasRoot { .. } => "__ctx_get_gutas_root",
            GetCheckpointUserTreeRoot { .. } => "__ctx_get_checkpoint_user_tree_root",
            GetCheckpointContractTreeRoot { .. } => "__ctx_get_checkpoint_contract_tree_root",
            GetCheckpointDepositTreeRoot { .. } => "__ctx_get_checkpoint_deposit_tree_root",
            GetCheckpointWithdrawalTreeRoot { .. } => "__ctx_get_checkpoint_withdrawal_tree_root",
            GetCheckpointUserRegistrationTreeRoot { .. } => "__ctx_get_checkpoint_user_registration_tree_root",
            GetDeployContractsRoot { .. } => "__ctx_get_deploy_contracts_root",
            GetGutaFeesCollected { .. } => "__ctx_get_guta_fees_collected",
            GetDaFeesCollected { .. } => "__ctx_get_da_fees_collected",
            GetUserOpsProcessed { .. } => "__ctx_get_user_ops_processed",
            GetTotalTransactions { .. } => "__ctx_get_total_transactions",
            GetSlotsModified { .. } => "__ctx_get_slots_modified",
            GetRegisterUsersCompleted { .. } => "__ctx_get_register_users_completed",
            GetGutasCompleted { .. } => "__ctx_get_gutas_completed",
            GetDeployContractsCompleted { .. } => "__ctx_get_deploy_contracts_completed",
            GetLastNonce { .. } => "__ctx_get_last_nonce",
            GetUserPublicKeyHash { .. } => "__ctx_get_user_public_key_hash",
            GetSessionProofTreeRoot { .. } => "__ctx_get_session_proof_tree_root",
            GetStateHashAt { .. } => "__ctx_get_state_hash_at",
            ImtGet { .. } => "__imt_get",
            ImtGetOtherUser { .. } => "__imt_get_other_user",
            ImtContainsOtherUser { .. } => "__imt_contains_other_user",
            GetOtherContractStateHashAt { .. } => "__ctx_get_other_contract_state_hash_at",
            GetOtherUserContractStateHashAt { .. } => "__ctx_get_other_user_contract_state_hash_at",
            CSetStateHashAt { .. } => "__ctx_set_state_hash_at",
            ImtSet { .. } => "__imt_set",
            ImtContains { .. } => "__imt_contains",
            MemTransmute { .. } => "__mem_transmute",
            MemSizeOf { .. } => "__mem_size_of",
            StorageRead { .. } => "__storage_read",
            StorageReadRange { .. } => "__storage_read_range",
            StorageWrite { .. } => "__storage_write",
            StorageWriteRange { .. } => "__storage_write_range",
            InvokeSync { .. } => "__invoke_sync",
            InvokeDeferred { .. } => "__invoke_deferred",
            Secp256k1Verify { .. } => "__secp256k1_verify",
            SumBits { .. } => "__sum_bits",
            SplitBits { .. } => "__split_bits",
            Emit { .. } => "__emit",
            Hash { .. } | Keccak256 { .. } | HashTwoToOne { .. } => return None,
        })
    }

    /// Source location of this intrinsic expression, regardless of variant.
    pub fn location(&self) -> Location {
        use IntrinsicExprNode::*;
        match self {
            GetUserId { location }
            | GetContractId { location }
            | GetContractDeployer { location, .. }
            | GetContractStateTreeHeight { location, .. }
            | GetCallerContractId { location }
            | GetCheckpointId { location }
            | GetCheckpointStats { location, .. }
            | GetRegisterUsersRoot { location, .. }
            | GetGutasRoot { location, .. }
            | GetCheckpointUserTreeRoot { location, .. }
            | GetCheckpointContractTreeRoot { location, .. }
            | GetCheckpointDepositTreeRoot { location, .. }
            | GetCheckpointWithdrawalTreeRoot { location, .. }
            | GetCheckpointUserRegistrationTreeRoot { location, .. }
            | GetDeployContractsRoot { location, .. }
            | GetGutaFeesCollected { location, .. }
            | GetDaFeesCollected { location, .. }
            | GetUserOpsProcessed { location, .. }
            | GetTotalTransactions { location, .. }
            | GetSlotsModified { location, .. }
            | GetRegisterUsersCompleted { location, .. }
            | GetGutasCompleted { location, .. }
            | GetDeployContractsCompleted { location, .. }
            | GetLastNonce { location }
            | GetUserPublicKeyHash { location }
            | GetSessionProofTreeRoot { location }
            | GetStateHashAt { location, .. }
            | ImtGet { location, .. }
            | ImtGetOtherUser { location, .. }
            | ImtContainsOtherUser { location, .. }
            | GetOtherContractStateHashAt { location, .. }
            | GetOtherUserContractStateHashAt { location, .. }
            | CSetStateHashAt { location, .. }
            | ImtSet { location, .. }
            | ImtContains { location, .. }
            | MemTransmute { location, .. }
            | MemSizeOf { location, .. }
            | StorageRead { location, .. }
            | StorageReadRange { location, .. }
            | StorageWrite { location, .. }
            | StorageWriteRange { location, .. }
            | Hash { location, .. }
            | Keccak256 { location, .. }
            | HashTwoToOne { location, .. }
            | InvokeSync { location, .. }
            | InvokeDeferred { location, .. }
            | Secp256k1Verify { location, .. }
            | SumBits { location, .. }
            | SplitBits { location, .. }
            | Emit { location, .. } => *location,
        }
    }
}
