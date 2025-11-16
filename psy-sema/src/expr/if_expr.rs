use psy_ast::{ExprId, Location, NodeInfo, NodeType};

use crate::TypeId;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedCase {
    pub predicate: ExprId,
    pub type_id: TypeId,
    pub body: ExprId,
}

impl CheckedCase {
    pub fn new(predicate: ExprId, type_id: TypeId, body: ExprId) -> Self {
        Self { predicate, type_id, body }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedIfExprNode {
    pub if_branch: CheckedCase,
    pub elseif_branches: Vec<CheckedCase>,
    pub else_branch: Option<ExprId>,
    pub type_id: TypeId,
    pub location: Location,
}

impl NodeInfo for CheckedIfExprNode {
    fn node_type(&self) -> NodeType {
        NodeType::IfExpr
    }
}
