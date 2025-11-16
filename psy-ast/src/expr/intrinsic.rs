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
    GetDeployContractsRoot {
        checkpoint_id: ExprId,
        location: Location,
    },
    GetFeesCollected {
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
    GetStateHashAt {
        slot_index: ExprId,
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
        num_bits: u64,
        location: Location,
    },
}

impl NodeInfo for IntrinsicExprNode {
    fn node_type(&self) -> NodeType {
        NodeType::IntrinsicExpr
    }
}
