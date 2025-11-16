use psy_ast::{ExprId, Location, NodeInfo, NodeType};

use crate::TypeId;

#[derive(Clone, Debug, PartialEq)]
pub enum CheckedIntrinsicExprNode {
    GetUserId {
        type_id: TypeId,
        location: Location,
    },
    GetContractId {
        type_id: TypeId,
        location: Location,
    },
    GetContractDeployer {
        contract_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetCallerContractId {
        type_id: TypeId,
        location: Location,
    },
    GetCheckpointId {
        type_id: TypeId,
        location: Location,
    },
    GetCheckpointStats {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetRegisterUsersRoot {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetGutasRoot {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetDeployContractsRoot {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetFeesCollected {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetUserOpsProcessed {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetTotalTransactions {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetSlotsModified {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetRegisterUsersCompleted {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetGutasCompleted {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetDeployContractsCompleted {
        checkpoint_id: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetLastNonce {
        type_id: TypeId,
        location: Location,
    },
    GetUserPublicKeyHash {
        type_id: TypeId,
        location: Location,
    },
    GetStateHashAt {
        slot_index: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetOtherContractStateHashAt {
        contract_state_tree_height: ExprId,
        contract_id: ExprId,
        slot_index: ExprId,
        type_id: TypeId,
        location: Location,
    },
    GetOtherUserContractStateHashAt {
        contract_state_tree_height: ExprId,
        user_id: ExprId,
        contract_id: ExprId,
        slot_index: ExprId,
        type_id: TypeId,
        location: Location,
    },
    CSetStateHashAt {
        slot_index: ExprId,
        new_value: ExprId,
        type_id: TypeId,
        location: Location,
    },
    MemTransmute {
        data: ExprId,
        target_type: TypeId,
        location: Location,
    },
    MemSizeOf {
        query_type: TypeId,
        type_id: TypeId,
        location: Location,
    },
    StorageRead {
        contract_state_tree_height: ExprId,
        user_id: ExprId,
        contract_id: ExprId,
        offset: ExprId,
        type_id: TypeId,
        location: Location,
    },
    StorageReadRange {
        contract_state_tree_height: ExprId,
        user_id: ExprId,
        contract_id: ExprId,
        offset: ExprId,
        length: ExprId,
        type_id: TypeId,
        location: Location,
    },
    StorageWrite {
        offset: ExprId,
        value: ExprId,
        type_id: TypeId,
        location: Location,
    },
    StorageWriteRange {
        offset: ExprId,
        values: ExprId,
        type_id: TypeId,
        location: Location,
    },
    Hash {
        data: ExprId,
        type_id: TypeId,
        location: Location,
    },
    HashTwoToOne {
        left: ExprId,
        right: ExprId,
        type_id: TypeId,
        location: Location,
    },
    InvokeSync {
        contract_id: ExprId,
        method_id: ExprId,
        inputs: ExprId,
        type_id: TypeId,
        location: Location,
    },
    InvokeDeferred {
        contract_id: ExprId,
        method_id: ExprId,
        inputs: ExprId,
        type_id: TypeId,
        location: Location,
    },
    Secp256k1Verify {
        pub_key: ExprId,
        msg: ExprId,
        sig: ExprId,
        type_id: TypeId,
        location: Location,
    },
    SumBits {
        bits: ExprId,
        type_id: TypeId,
        location: Location,
    },
    SplitBits {
        target: ExprId,
        num_bits: u64,
        type_id: TypeId,
        location: Location,
    },
}

impl NodeInfo for CheckedIntrinsicExprNode {
    fn node_type(&self) -> NodeType {
        NodeType::IntrinsicExpr
    }
}
