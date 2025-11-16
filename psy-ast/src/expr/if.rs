use crate::{ExprId, Location, NodeInfo, NodeType};

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub predicate: ExprId,
    pub body: ExprId,
    pub location: Location,
}

impl Case {
    pub fn new(predicate: ExprId, body: ExprId, location: Location) -> Self {
        Self { predicate, body, location }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfExprNode {
    pub if_branch: Case,
    pub elseif_branches: Vec<Case>,
    pub else_branch: Option<ExprId>,
    pub location: Location,
}

impl NodeInfo for IfExprNode {
    fn node_type(&self) -> NodeType {
        NodeType::IfExpr
    }
}
