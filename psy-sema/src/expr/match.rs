use psy_ast::{ExprId, Location, NodeInfo, NodeType};

use crate::{ScopeId, TypeId};

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedMatchNode {
    pub value: ExprId,
    pub cases: Vec<CheckedMatchArm>,
    pub type_id: TypeId,
    pub scope_id: ScopeId,
    pub location: Location,
}

impl NodeInfo for CheckedMatchNode {
    fn node_type(&self) -> NodeType {
        NodeType::MatchExpr
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedMatchArm {
    pub pattern: Option<ExprId>,
    pub body: ExprId, //indeed it's a block
    pub location: Location,
}
