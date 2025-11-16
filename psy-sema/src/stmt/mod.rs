mod assignment;
mod r#for;
mod intrinsic;
mod r#return;
mod variable;
mod r#while;

pub use assignment::*;
use enum_as_inner::EnumAsInner;
pub use intrinsic::*;
use psy_ast::{DefId, ExprId, NodeInfo, NodeType};
pub use r#for::*;
pub use r#return::*;
pub use r#while::*;
pub use variable::*;

use crate::{CheckedDefinitionNode, CheckedExprNode};

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum CheckedStmtNode {
    While(CheckedWhileNode),
    For(CheckedForNode),
    Assignment(CheckedAssignmentNode),
    Variable(CheckedVariableNode),
    Definition(DefId),
    Expression(ExprId),
    Return(CheckedReturnNode),
    Intrinsic(CheckedIntrinsicStmtNode),
}

impl NodeInfo for CheckedStmtNode {
    fn node_type(&self) -> NodeType {
        match self {
            Self::While(node) => node.node_type(),
            Self::For(node) => node.node_type(),
            Self::Assignment(node) => node.node_type(),
            Self::Variable(node) => node.node_type(),
            Self::Definition(_node) => NodeType::DefinitionStmt,
            Self::Expression(_node) => NodeType::ExpressionStmt,
            Self::Return(node) => node.node_type(),
            Self::Intrinsic(node) => node.node_type(),
        }
    }

    fn as_expression(&self) -> Option<ExprId> {
        match self {
            Self::Expression(expr) => Some(*expr),
            _ => None,
        }
    }

    fn as_definition(&self) -> Option<DefId> {
        match self {
            Self::Definition(def) => Some(*def),
            _ => None,
        }
    }
}

impl From<ExprId> for CheckedStmtNode {
    fn from(value: ExprId) -> Self {
        Self::Expression(value)
    }
}

impl From<DefId> for CheckedStmtNode {
    fn from(value: DefId) -> Self {
        Self::Definition(value)
    }
}

impl<F> From<CheckedExprNode<F>> for CheckedStmtNode {
    fn from(_value: CheckedExprNode<F>) -> Self {
        todo!()
    }
}

impl From<CheckedDefinitionNode> for CheckedStmtNode {
    fn from(_value: CheckedDefinitionNode) -> Self {
        todo!()
    }
}
