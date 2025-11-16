use psy_ast::{ExprId, Location, NodeInfo, NodeType};

use crate::TypeId;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedCallNode {
    pub callee: ExprId,
    pub generic_parameters: Vec<TypeId>,
    pub args: Vec<ExprId>,
    pub type_id: TypeId,
    pub location: Location,
}

impl NodeInfo for CheckedCallNode {
    fn node_type(&self) -> NodeType {
        NodeType::CallExpr
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedMemberCallNode {
    pub callee: ExprId,
    pub receiver: ExprId,
    pub generic_parameters: Vec<TypeId>,
    pub args: Vec<ExprId>,
    pub type_id: TypeId,
    pub location: Location,
}

impl NodeInfo for CheckedMemberCallNode {
    fn node_type(&self) -> NodeType {
        NodeType::MemberCallExpr
    }
}
