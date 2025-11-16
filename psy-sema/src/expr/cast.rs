use psy_ast::{Location, NodeInfo, NodeType};

use crate::{ExprId, TypeId};

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedCastNode {
    pub value: ExprId,
    pub target_type: TypeId,
    pub location: Location,
}

impl NodeInfo for CheckedCastNode {
    fn node_type(&self) -> NodeType {
        NodeType::CastExpr
    }
}
