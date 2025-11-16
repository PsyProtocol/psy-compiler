use std::fmt::Display;

use crate::{ExprId, Location, NodeInfo, NodeType};

#[derive(Copy, Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Neg,
    Not,
}

impl Display for UnaryOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnaryOperator::Neg => write!(f, "-"),
            UnaryOperator::Not => write!(f, "!"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnaryNode {
    pub operator: UnaryOperator,
    pub rhs: ExprId,
    pub location: Location,
}

impl NodeInfo for UnaryNode {
    fn node_type(&self) -> NodeType {
        NodeType::UnaryExpr
    }
}
