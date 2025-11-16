mod assignment;
mod block;
mod r#for;
mod intrinsic;
mod r#return;
mod variable;
mod r#while;

pub use assignment::*;
use enum_as_inner::EnumAsInner;
pub use intrinsic::*;
pub use r#for::*;
pub use r#return::*;
pub use r#while::*;
pub use variable::*;

use crate::{DefId, ExprId, NodeInfo, NodeType};

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum StmtNode {
    While(WhileNode),
    For(ForNode),
    Assignment(AssignmentNode),
    Variable(VariableNode),
    Definition(DefId),
    Expression(ExprId),
    Return(ReturnNode),
    Intrinsic(IntrinsicStmtNode),
}

impl NodeInfo for StmtNode {
    fn node_type(&self) -> NodeType {
        match self {
            StmtNode::While(node) => node.node_type(),
            StmtNode::For(node) => node.node_type(),
            StmtNode::Assignment(node) => node.node_type(),
            StmtNode::Variable(node) => node.node_type(),
            StmtNode::Definition(_) => NodeType::DefinitionStmt,
            StmtNode::Expression(_) => NodeType::ExpressionStmt,
            StmtNode::Return(node) => node.node_type(),
            StmtNode::Intrinsic(node) => node.node_type(),
        }
    }

    fn as_expression(&self) -> Option<ExprId> {
        match self {
            StmtNode::Expression(expr) => Some(*expr),
            _ => None,
        }
    }

    fn as_definition(&self) -> Option<DefId> {
        match self {
            StmtNode::Definition(def) => Some(*def),
            _ => None,
        }
    }
}

impl From<ExprId> for StmtNode {
    fn from(value: ExprId) -> Self {
        Self::Expression(value)
    }
}

impl From<DefId> for StmtNode {
    fn from(value: DefId) -> Self {
        Self::Definition(value)
    }
}
