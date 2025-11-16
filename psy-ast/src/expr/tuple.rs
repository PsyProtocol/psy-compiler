use crate::{ExprId, Location, NodeInfo, NodeType};

#[derive(Debug, Clone, PartialEq)]
pub struct TupleExprNode {
    pub elements: Vec<ExprId>,
    pub location: Location,
}

impl NodeInfo for TupleExprNode {
    fn node_type(&self) -> NodeType {
        NodeType::TupleExpr
    }
}
