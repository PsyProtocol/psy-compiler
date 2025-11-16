use crate::{ExprId, Identifier, Location, NodeInfo, NodeType};

#[derive(Clone, Debug, PartialEq)]
pub struct IndexAccessNode {
    pub target: ExprId,
    pub index: ExprId,
    pub location: Location,
}

impl NodeInfo for IndexAccessNode {
    fn node_type(&self) -> NodeType {
        NodeType::IndexAccessExpr
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemberAccessNode {
    pub target: ExprId,
    pub field: Identifier,
    pub location: Location,
}

impl NodeInfo for MemberAccessNode {
    fn node_type(&self) -> NodeType {
        NodeType::MemberAccessExpr
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TupleAccessNode {
    pub target: ExprId,
    pub index: usize,
    pub location: Location,
}

impl NodeInfo for TupleAccessNode {
    fn node_type(&self) -> NodeType {
        NodeType::TupleAccessExpr
    }
}
