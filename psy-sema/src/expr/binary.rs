use psy_ast::{BinaryOperator, ExprId, Location, NodeInfo, NodeType};

use crate::TypeId;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedBinaryNode {
    pub lhs: ExprId,
    pub operator: BinaryOperator,
    pub rhs: ExprId,
    pub type_id: TypeId,
    pub location: Location,
}

impl NodeInfo for CheckedBinaryNode {
    fn node_type(&self) -> NodeType {
        NodeType::BinaryExpr
    }
}
