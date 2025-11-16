use psy_ast::{Comment, ExprId, Identifier, Location, NodeInfo, NodeType, TypeQualifier};

use crate::{ScopeId, TypeId};

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedVariableNode {
    pub name: Identifier,
    pub ty: TypeId,
    pub qualifier: TypeQualifier,
    pub value: ExprId,
    pub scope_id: ScopeId,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for CheckedVariableNode {
    fn node_type(&self) -> NodeType {
        NodeType::VariableStmt
    }
}
