mod binary;
mod block_expr;
mod call;
mod cast;
mod r#if;
mod index;
mod intrinsic;
mod lambda;
mod r#match;
mod path;
mod tuple;
mod unary;

pub use binary::*;
pub use block_expr::*;
pub use call::*;
pub use cast::*;
use enum_as_inner::EnumAsInner;
pub use index::*;
pub use intrinsic::*;
pub use lambda::*;
pub use path::*;
pub use r#if::*;
pub use r#match::*;
pub use tuple::*;
pub use unary::*;

use crate::{ExprId, NodeInfo, NodeType, ValueNode};

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum ExprNode<F: Clone + From<u32>> {
    Path(PathNode),
    Value(ValueNode<F>),
    Binary(BinaryNode),
    Unary(UnaryNode),
    Call(CallNode),
    MemberCall(MemberCallNode),
    Cast(CastNode),
    IndexAccess(IndexAccessNode),
    MemberAccess(MemberAccessNode),
    BlockExpr(BlockExprNode),
    IfExpr(IfExprNode),
    Intrinsic(IntrinsicExprNode),
    LambdaFunction(LambdaFunctionNode),
    Tuple(TupleExprNode),
    TupleAccess(TupleAccessNode),
    Match(MatchNode),
    Parentheses(ExprId),
}

impl<F: Clone + From<u32>> NodeInfo for ExprNode<F> {
    fn node_type(&self) -> NodeType {
        match self {
            Self::Path(node) => node.node_type(),
            Self::Value(node) => node.node_type(),
            Self::Binary(node) => node.node_type(),
            Self::Unary(node) => node.node_type(),
            Self::Call(node) => node.node_type(),
            Self::MemberCall(node) => node.node_type(),
            Self::Cast(node) => node.node_type(),
            Self::IndexAccess(node) => node.node_type(),
            Self::MemberAccess(node) => node.node_type(),
            Self::Intrinsic(node) => node.node_type(),
            Self::LambdaFunction(node) => node.node_type(),
            Self::BlockExpr(node) => node.node_type(),
            Self::IfExpr(node) => node.node_type(),
            Self::Tuple(node) => node.node_type(),
            Self::TupleAccess(node) => node.node_type(),
            Self::Match(node) => node.node_type(),
            Self::Parentheses(_) => NodeType::ParenthesesExpr,
        }
    }
}
