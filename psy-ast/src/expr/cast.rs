use crate::{ExprId, Location, NodeInfo, NodeType, UncheckedType};

#[derive(Debug, Clone, PartialEq)]
pub struct CastNode {
    pub value: ExprId,
    pub target_type: UncheckedType,
    pub location: Location,
}

impl CastNode {
    pub fn new(value: ExprId, target_type: UncheckedType, location: Location) -> Self {
        Self {
            value,
            target_type,
            location,
        }
    }
}

impl NodeInfo for CastNode {
    fn node_type(&self) -> NodeType {
        NodeType::CastExpr
    }
}
