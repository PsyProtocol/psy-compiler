use psy_ast::{ExprId, Identifier, Location, NodeInfo, NodeType};

use crate::TypeId;

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedIndexAccessNode {
    pub target: ExprId,
    pub index: ExprId,
    pub type_id: TypeId,
    pub location: Location,
}

impl NodeInfo for CheckedIndexAccessNode {
    fn node_type(&self) -> NodeType {
        NodeType::IndexAccessExpr
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedMemberAccessNode {
    pub target: ExprId,
    pub field: Identifier,
    pub type_id: TypeId,
    pub location: Location,
}

impl NodeInfo for CheckedMemberAccessNode {
    fn node_type(&self) -> NodeType {
        NodeType::MemberAccessExpr
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedTupleAccessNode {
    pub target: ExprId,  // `ExprId` for the tuple being accessed
    pub index: usize,    // The index of the tuple element being accessed
    pub type_id: TypeId, // The type of the element at `index`
    pub location: Location,
}

impl NodeInfo for CheckedTupleAccessNode {
    fn node_type(&self) -> NodeType {
        NodeType::TupleAccessExpr
    }
}
