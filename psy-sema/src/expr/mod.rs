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
                CheckedIntrinsicExprNode::GetCallerContractId { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetCheckpointId { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetLastNonce { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetUserPublicKeyHash { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetStateHashAt { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetOtherContractStateHashAt { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetOtherUserContractStateHashAt { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::CSetStateHashAt { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::StorageRead { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::StorageWrite { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::Hash { type_id, .. } => type_id.clone(),
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
                CheckedIntrinsicExprNode::GetDeployContractsRoot { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetFeesCollected { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetUserOpsProcessed { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetTotalTransactions { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetSlotsModified { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetRegisterUsersCompleted { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetGutasCompleted { type_id, .. } => type_id.clone(),
                CheckedIntrinsicExprNode::GetDeployContractsCompleted { type_id, .. } => type_id.clone(),
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
                CheckedIntrinsicExprNode::GetCallerContractId { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetCheckpointId { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetLastNonce { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetUserPublicKeyHash { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetStateHashAt { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetOtherContractStateHashAt { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetOtherUserContractStateHashAt { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::CSetStateHashAt { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::StorageRead { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::StorageWrite { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::Hash { location, .. } => location.clone(),
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
                CheckedIntrinsicExprNode::GetDeployContractsRoot { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetFeesCollected { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetUserOpsProcessed { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetTotalTransactions { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetSlotsModified { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetRegisterUsersCompleted { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetGutasCompleted { location, .. } => location.clone(),
                CheckedIntrinsicExprNode::GetDeployContractsCompleted { location, .. } => location.clone(),
            },
            CheckedExprNode::LambdaFunction(c) => c.location,
            CheckedExprNode::IfExpr(i) => i.location,
            CheckedExprNode::BlockExpr(b) => b.location,
            CheckedExprNode::Match(m) => m.location,
        }
    }
}
