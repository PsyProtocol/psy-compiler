mod binary;
mod block_expr;
mod call;
mod cast;
mod if_expr;
mod index;
mod intrinsic;
mod lambda;
mod r#match;
mod path;
mod unary;

pub use binary::*;
pub use block_expr::*;
pub use call::*;
pub use cast::*;
use enum_as_inner::EnumAsInner;
pub use if_expr::*;
pub use index::*;
pub use intrinsic::*;
pub use lambda::*;
pub use path::*;
use psy_ast::{Location, NodeInfo, NodeType};
pub use r#match::*;
pub use unary::*;

use crate::{CheckedValueNode, TypeId, BOOL_TYPE, FELT_TYPE, U32_TYPE};

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum CheckedExprNode<F> {
    Path(CheckedPathNode),
    Value(CheckedValueNode<F>),
    Binary(CheckedBinaryNode),
    Unary(CheckedUnaryNode),
    Cast(CheckedCastNode),
    Call(CheckedCallNode),
    MemberCall(CheckedMemberCallNode),
    IndexAccess(CheckedIndexAccessNode),
    TupleAccess(CheckedTupleAccessNode),
    MemberAccess(CheckedMemberAccessNode),
    Intrinsic(CheckedIntrinsicExprNode),
    LambdaFunction(CheckedLambdaFunctionNode),
    BlockExpr(CheckedBlockExprNode),
    IfExpr(CheckedIfExprNode),
    Match(CheckedMatchNode),
}

impl<F> NodeInfo for CheckedExprNode<F> {
    fn node_type(&self) -> NodeType {
        match self {
            CheckedExprNode::Path(node) => node.node_type(),
            CheckedExprNode::Value(node) => node.node_type(),
            CheckedExprNode::Binary(node) => node.node_type(),
            CheckedExprNode::Unary(node) => node.node_type(),
            CheckedExprNode::Cast(node) => node.node_type(),
            CheckedExprNode::Call(node) => node.node_type(),
            CheckedExprNode::MemberCall(node) => node.node_type(),
            CheckedExprNode::TupleAccess(node) => node.node_type(),
            CheckedExprNode::IndexAccess(node) => node.node_type(),
            CheckedExprNode::MemberAccess(node) => node.node_type(),
            CheckedExprNode::Intrinsic(node) => node.node_type(),
            CheckedExprNode::LambdaFunction(node) => node.node_type(),
            CheckedExprNode::BlockExpr(node) => node.node_type(),
            CheckedExprNode::IfExpr(node) => node.node_type(),
            CheckedExprNode::Match(node) => node.node_type(),
        }
    }
}

impl<F> CheckedExprNode<F> {
    pub fn ty(&self) -> TypeId {
        match self {
            CheckedExprNode::Path(p) => p.type_id,
            CheckedExprNode::Value(v) => match v {
                CheckedValueNode::Felt(_, _) => FELT_TYPE,
                CheckedValueNode::Bool(_, _) => BOOL_TYPE,
                CheckedValueNode::U32(_, _) => U32_TYPE,
                CheckedValueNode::Array(type_id, _, _) => type_id.clone(),
                CheckedValueNode::ArrayRepeat(type_id, _, _, _) => type_id.clone(),
                CheckedValueNode::Struct(type_id, _, _) => type_id.clone(),
                CheckedValueNode::Type(type_id) => type_id.clone(),
                CheckedValueNode::Tuple(type_id, _, _) => type_id.clone(),
            },
            CheckedExprNode::Binary(b) => b.type_id,
            CheckedExprNode::Unary(u) => u.type_id,
            CheckedExprNode::Cast(c) => c.target_type,
            CheckedExprNode::Call(c) => c.type_id,
            CheckedExprNode::MemberCall(c) => c.type_id,
            CheckedExprNode::IndexAccess(i) => i.type_id,
            CheckedExprNode::MemberAccess(m) => m.type_id,
            CheckedExprNode::TupleAccess(t) => t.type_id,
            CheckedExprNode::Intrinsic(i) => match i {
                CheckedIntrinsicExprNode::GetUserId { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetContractId { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetContractDeployer { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetContractStateTreeHeight { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetCallerContractId { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetCheckpointId { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetLastNonce { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetUserPublicKeyHash { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetSessionProofTreeRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetStateHashAt { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::ImtGet { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::ImtGetOtherUser { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::ImtContainsOtherUser { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetOtherContractStateHashAt { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetOtherUserContractStateHashAt { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::CSetStateHashAt { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::ImtSet { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::ImtContains { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::StorageRead { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::StorageWrite { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::Hash { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::Keccak256 { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::HashTwoToOne { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::MemTransmute { target_type, .. } => target_type.clone(),
                CheckedIntrinsicExprNode::MemSizeOf { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::StorageReadRange { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::StorageWriteRange { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::InvokeSync { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::InvokeDeferred { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::Secp256k1Verify { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::SumBits { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::SplitBits { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetCheckpointStats { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetRegisterUsersRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetGutasRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetCheckpointUserTreeRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetCheckpointContractTreeRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetCheckpointDepositTreeRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetCheckpointWithdrawalTreeRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetCheckpointUserRegistrationTreeRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetDeployContractsRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetGutaFeesCollected { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetDaFeesCollected { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetUserOpsProcessed { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetTotalTransactions { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetSlotsModified { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetRegisterUsersCompleted { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetGutasCompleted { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetDeployContractsCompleted { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::Emit { type_id, .. } => type_id.clone(),
            },
            CheckedExprNode::LambdaFunction(c) => c.type_id.clone(),
            CheckedExprNode::IfExpr(i) => i.type_id,
            CheckedExprNode::BlockExpr(b) => b.type_id,
            CheckedExprNode::Match(m) => m.type_id,
        }
    }

    pub fn location(&self) -> Location {
        match self {
            CheckedExprNode::Path(p) => p.location,
            CheckedExprNode::Value(v) => match v {
                CheckedValueNode::Felt(_, location) => location.clone(),
                CheckedValueNode::Bool(_, location) => location.clone(),
                CheckedValueNode::U32(_, location) => location.clone(),
                CheckedValueNode::Array(_, _, location) => location.clone(),
                CheckedValueNode::ArrayRepeat(_, _, _, location) => location.clone(),
                CheckedValueNode::Struct(_, _, location) => location.clone(),
                CheckedValueNode::Type(_) => unreachable!(),
                CheckedValueNode::Tuple(_, _, location) => location.clone(),
            },
            CheckedExprNode::Binary(b) => b.location,
            CheckedExprNode::Unary(u) => u.location,
            CheckedExprNode::Cast(c) => c.location,
            CheckedExprNode::Call(c) => c.location,
            CheckedExprNode::MemberCall(c) => c.location,
            CheckedExprNode::IndexAccess(i) => i.location,
            CheckedExprNode::MemberAccess(m) => m.location,
            CheckedExprNode::TupleAccess(t) => t.location,
            CheckedExprNode::Intrinsic(i) => match i {
                CheckedIntrinsicExprNode::GetUserId { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetContractId { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetContractDeployer { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetContractStateTreeHeight { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetCallerContractId { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetCheckpointId { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetLastNonce { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetUserPublicKeyHash { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetSessionProofTreeRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetStateHashAt { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::ImtGet { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::ImtGetOtherUser { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::ImtContainsOtherUser { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetOtherContractStateHashAt { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetOtherUserContractStateHashAt { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::CSetStateHashAt { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::ImtSet { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::ImtContains { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::StorageRead { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::StorageWrite { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::Hash { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::Keccak256 { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::HashTwoToOne { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::MemTransmute { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::MemSizeOf {
                    query_type: _ty, location, ..
                } => location.clone(),
                CheckedIntrinsicExprNode::StorageReadRange { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::StorageWriteRange { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::InvokeSync {
                    contract_id,
                    method_id,
                    inputs,
                    type_id,
                    location,
                } => location.clone(),
                CheckedIntrinsicExprNode::InvokeDeferred {
                    contract_id,
                    method_id,
                    inputs,
                    type_id,
                    location,
                } => location.clone(),
                CheckedIntrinsicExprNode::Secp256k1Verify {
                    pub_key,
                    msg,
                    sig,
                    type_id,
                    location,
                } => location.clone(),
                CheckedIntrinsicExprNode::SumBits { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::SplitBits { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetCheckpointStats { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetRegisterUsersRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetGutasRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetCheckpointUserTreeRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetCheckpointContractTreeRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetCheckpointDepositTreeRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetCheckpointWithdrawalTreeRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetCheckpointUserRegistrationTreeRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetDeployContractsRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetGutaFeesCollected { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetDaFeesCollected { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetUserOpsProcessed { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetTotalTransactions { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetSlotsModified { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetRegisterUsersCompleted { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetGutasCompleted { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetDeployContractsCompleted { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::Emit { location, .. } => location.clone(),
            },
            CheckedExprNode::LambdaFunction(c) => c.location,
            CheckedExprNode::IfExpr(i) => i.location,
            CheckedExprNode::BlockExpr(b) => b.location,
            CheckedExprNode::Match(m) => m.location,
        }
    }
}

#[cfg(test)]
mod accessor_tests {
    use indexmap::IndexMap;
    use psy_ast::{ConstValue, ExprId, IdentId, Identifier, PathNode, UncheckedType};
    use psy_common::FileId;

    use super::*;
    use crate::ScopeId;

    fn loc(start: usize) -> Location {
        Location::new(FileId(0), start, start + 1)
    }

    fn empty_path_node() -> PathNode {
        PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(Identifier { id: IdentId(0), location: loc(1) }),
            is_ty: false,
            location: loc(1),
        }
    }

    #[test]
    fn intrinsic_accessors_round_trip() {
        let e = ExprId(0);
        // One maker per `CheckedIntrinsicExprNode` variant; each receives the
        // type id and location it must carry so the asserts below are real
        // round-trips rather than tautologies.
        let makers: Vec<Box<dyn Fn(usize, Location) -> CheckedIntrinsicExprNode>> = vec![
            Box::new(|t, l| CheckedIntrinsicExprNode::GetUserId { type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetContractId { type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetContractDeployer { contract_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetContractStateTreeHeight { contract_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetCallerContractId { type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetCheckpointId { type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetCheckpointStats { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetRegisterUsersRoot { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetGutasRoot { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetCheckpointUserTreeRoot { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetCheckpointContractTreeRoot { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetCheckpointDepositTreeRoot { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetCheckpointWithdrawalTreeRoot { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetCheckpointUserRegistrationTreeRoot { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetDeployContractsRoot { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetGutaFeesCollected { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetDaFeesCollected { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetUserOpsProcessed { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetTotalTransactions { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetSlotsModified { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetRegisterUsersCompleted { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetGutasCompleted { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetDeployContractsCompleted { checkpoint_id: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetLastNonce { type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetUserPublicKeyHash { type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetSessionProofTreeRoot { type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::GetStateHashAt { slot_index: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::ImtGet {
                    key: e,
                    base_offset: e,
                    capacity: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::ImtGetOtherUser {
                    contract_state_tree_height: e,
                    user_id: e,
                    contract_id: e,
                    key: e,
                    base_offset: e,
                    capacity: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::ImtContainsOtherUser {
                    contract_state_tree_height: e,
                    user_id: e,
                    contract_id: e,
                    key: e,
                    base_offset: e,
                    capacity: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::GetOtherContractStateHashAt {
                    contract_state_tree_height: e,
                    contract_id: e,
                    slot_index: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::GetOtherUserContractStateHashAt {
                    contract_state_tree_height: e,
                    user_id: e,
                    contract_id: e,
                    slot_index: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::CSetStateHashAt {
                    slot_index: e,
                    new_value: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::ImtSet {
                    key: e,
                    new_value: e,
                    base_offset: e,
                    capacity: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::ImtContains {
                    key: e,
                    base_offset: e,
                    capacity: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| CheckedIntrinsicExprNode::MemTransmute { data: e, target_type: TypeId(t), location: l }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::MemSizeOf {
                    query_type: TypeId(t),
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::StorageRead {
                    contract_state_tree_height: e,
                    user_id: e,
                    contract_id: e,
                    offset: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::StorageReadRange {
                    contract_state_tree_height: e,
                    user_id: e,
                    contract_id: e,
                    offset: e,
                    length: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| CheckedIntrinsicExprNode::StorageWrite { offset: e, value: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::StorageWriteRange { offset: e, values: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::Hash { data: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::Keccak256 { data: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| CheckedIntrinsicExprNode::HashTwoToOne { left: e, right: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::InvokeSync {
                    contract_id: e,
                    method_id: e,
                    inputs: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::InvokeDeferred {
                    contract_id: e,
                    method_id: e,
                    inputs: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::Secp256k1Verify {
                    pub_key: e,
                    msg: e,
                    sig: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| CheckedIntrinsicExprNode::SumBits { bits: e, type_id: TypeId(t), location: l }),
            Box::new(|t, l| {
                CheckedIntrinsicExprNode::SplitBits {
                    target: e,
                    num_bits: e,
                    type_id: TypeId(t),
                    location: l,
                }
            }),
            Box::new(|t, l| CheckedIntrinsicExprNode::Emit { event_data: e, type_id: TypeId(t), location: l }),
        ];

        for (i, make) in makers.into_iter().enumerate() {
            let type_id = TypeId(1000 + i);
            let location = loc(2000 + i);
            let node = CheckedExprNode::<u64>::Intrinsic(make(1000 + i, location));
            assert_eq!(node.ty(), type_id, "ty() for intrinsic case {i}");
            assert_eq!(node.location(), location, "location() for intrinsic case {i}");
            assert_eq!(node.node_type(), NodeType::IntrinsicExpr, "node_type() for intrinsic case {i}");
        }
    }

    #[test]
    fn value_accessors_round_trip() {
        let cases: Vec<(CheckedExprNode<u64>, TypeId, Location)> = vec![
            (CheckedExprNode::Value(CheckedValueNode::Felt(7, loc(10))), FELT_TYPE, loc(10)),
            (CheckedExprNode::Value(CheckedValueNode::Bool(1, loc(11))), BOOL_TYPE, loc(11)),
            (CheckedExprNode::Value(CheckedValueNode::U32(9, loc(12))), U32_TYPE, loc(12)),
            (
                CheckedExprNode::Value(CheckedValueNode::Array(TypeId(20), vec![ExprId(0)], loc(13))),
                TypeId(20),
                loc(13),
            ),
            (
                CheckedExprNode::Value(CheckedValueNode::ArrayRepeat(TypeId(21), ExprId(0), ConstValue::Felt(1), loc(14))),
                TypeId(21),
                loc(14),
            ),
            (
                CheckedExprNode::Value(CheckedValueNode::Struct(TypeId(22), IndexMap::new(), loc(15))),
                TypeId(22),
                loc(15),
            ),
            (
                CheckedExprNode::Value(CheckedValueNode::Tuple(TypeId(23), vec![(TypeId(1), ExprId(0))], loc(16))),
                TypeId(23),
                loc(16),
            ),
        ];
        for (node, type_id, location) in cases {
            assert_eq!(node.ty(), type_id);
            assert_eq!(node.location(), location, "Value location for {type_id:?}");
            assert_eq!(node.node_type(), NodeType::ValueExpr);
        }

        // `Type` values are unevaluated type references and deliberately have
        // no location — only the type accessor is meaningful for them.
        let type_value = CheckedExprNode::<u64>::Value(CheckedValueNode::Type(TypeId(24)));
        assert_eq!(type_value.ty(), TypeId(24));
        assert_eq!(type_value.node_type(), NodeType::ValueExpr);
    }

    #[test]
    fn other_expr_accessors_round_trip() {
        let e = ExprId(0);
        let cases: Vec<(CheckedExprNode<u64>, TypeId, Location, NodeType)> = vec![
            (
                CheckedExprNode::Path(CheckedPathNode {
                    variable: None,
                    root: None,
                    target: None,
                    origin_path: empty_path_node(),
                    type_id: TypeId(30),
                    trait_ty: None,
                    location: loc(30),
                }),
                TypeId(30),
                loc(30),
                NodeType::PathExpr,
            ),
            (
                CheckedExprNode::Binary(CheckedBinaryNode {
                    lhs: e,
                    operator: psy_ast::BinaryOperator::Add,
                    rhs: e,
                    type_id: TypeId(31),
                    location: loc(31),
                }),
                TypeId(31),
                loc(31),
                NodeType::BinaryExpr,
            ),
            (
                CheckedExprNode::Unary(CheckedUnaryNode {
                    operator: psy_ast::UnaryOperator::Not,
                    rhs: e,
                    type_id: TypeId(32),
                    location: loc(32),
                }),
                TypeId(32),
                loc(32),
                NodeType::UnaryExpr,
            ),
            (
                CheckedExprNode::Cast(CheckedCastNode {
                    value: e,
                    target_type: TypeId(33),
                    location: loc(33),
                }),
                TypeId(33),
                loc(33),
                NodeType::CastExpr,
            ),
            (
                CheckedExprNode::Call(CheckedCallNode {
                    callee: e,
                    generic_parameters: vec![],
                    args: vec![],
                    type_id: TypeId(34),
                    location: loc(34),
                }),
                TypeId(34),
                loc(34),
                NodeType::CallExpr,
            ),
            (
                CheckedExprNode::MemberCall(CheckedMemberCallNode {
                    callee: e,
                    receiver: e,
                    generic_parameters: vec![],
                    args: vec![],
                    type_id: TypeId(35),
                    location: loc(35),
                }),
                TypeId(35),
                loc(35),
                NodeType::MemberCallExpr,
            ),
            (
                CheckedExprNode::IndexAccess(CheckedIndexAccessNode {
                    target: e,
                    index: e,
                    type_id: TypeId(36),
                    location: loc(36),
                }),
                TypeId(36),
                loc(36),
                NodeType::IndexAccessExpr,
            ),
            (
                CheckedExprNode::MemberAccess(CheckedMemberAccessNode {
                    target: e,
                    field: Identifier { id: IdentId(0), location: loc(1) },
                    type_id: TypeId(37),
                    location: loc(37),
                }),
                TypeId(37),
                loc(37),
                NodeType::MemberAccessExpr,
            ),
            (
                CheckedExprNode::TupleAccess(CheckedTupleAccessNode {
                    target: e,
                    index: 0,
                    type_id: TypeId(38),
                    location: loc(38),
                }),
                TypeId(38),
                loc(38),
                NodeType::TupleAccessExpr,
            ),
            (
                CheckedExprNode::LambdaFunction(CheckedLambdaFunctionNode {
                    name: Identifier { id: IdentId(0), location: loc(1) },
                    parameters: vec![],
                    body: e,
                    return_type: TypeId(39),
                    return_type_path: None,
                    scope_id: ScopeId(0),
                    type_id: TypeId(39),
                    location: loc(39),
                }),
                TypeId(39),
                loc(39),
                NodeType::LambdaFunctionExpr,
            ),
            (
                CheckedExprNode::BlockExpr(CheckedBlockExprNode {
                    stmts: vec![],
                    expr: None,
                    type_id: TypeId(40),
                    scope_id: ScopeId(0),
                    location: loc(40),
                }),
                TypeId(40),
                loc(40),
                NodeType::BlockExpr,
            ),
            (
                CheckedExprNode::IfExpr(CheckedIfExprNode {
                    if_branch: CheckedCase {
                        predicate: e,
                        type_id: TypeId(1),
                        body: e,
                    },
                    elseif_branches: vec![],
                    else_branch: None,
                    type_id: TypeId(41),
                    location: loc(41),
                }),
                TypeId(41),
                loc(41),
                NodeType::IfExpr,
            ),
            (
                CheckedExprNode::Match(CheckedMatchNode {
                    value: e,
                    cases: vec![CheckedMatchArm {
                        pattern: None,
                        body: e,
                        location: loc(1),
                    }],
                    type_id: TypeId(42),
                    scope_id: ScopeId(0),
                    location: loc(42),
                }),
                TypeId(42),
                loc(42),
                NodeType::MatchExpr,
            ),
        ];
        for (node, type_id, location, kind) in cases {
            assert_eq!(node.ty(), type_id, "ty() for {kind:?}");
            assert_eq!(node.location(), location, "location() for {kind:?}");
            assert_eq!(node.node_type(), kind);
        }
    }
}
