use crate::{ExprId, Identifier, Location, NodeInfo, NodeType, UncheckedType};

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
    /// Generic args on a member used as a bare function reference
    /// (`object.method::<T>` with no call parens). Empty for plain access.
    pub generic_parameters: Vec<UncheckedType>,
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
