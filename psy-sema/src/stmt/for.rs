use psy_ast::{Comment, Location};

use crate::{ExprId, Identifier, NodeInfo, NodeType, ScopeId};

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedForNode {
    pub variable: Identifier,
    pub start: ExprId,
    pub end: ExprId,
    pub body: ExprId,
    pub scope_id: ScopeId,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for CheckedForNode {
    fn node_type(&self) -> NodeType {
        NodeType::ForStmt
    }
}
