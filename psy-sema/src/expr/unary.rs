use psy_ast::{ExprId, Location, NodeInfo, NodeType, UnaryOperator};

use crate::TypeId;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedUnaryNode {
    pub operator: UnaryOperator,
    pub rhs: ExprId,
    pub type_id: TypeId,
    pub location: Location,
}

impl NodeInfo for CheckedUnaryNode {
    fn node_type(&self) -> NodeType {
        NodeType::UnaryExpr
    }
}
