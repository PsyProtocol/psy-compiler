use crate::{ExprId, Location, NodeInfo, NodeType, UncheckedType};

#[derive(Debug, Clone, PartialEq)]
pub struct CallNode {
    pub callee: ExprId,
    pub generic_parameters: Vec<UncheckedType>,
    pub args: Vec<ExprId>,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemberCallNode {
    pub callee: ExprId,
    pub receiver: ExprId,
    pub generic_parameters: Vec<UncheckedType>,
    pub args: Vec<ExprId>,
    pub location: Location,
}

impl NodeInfo for CallNode {
    fn node_type(&self) -> NodeType {
        NodeType::CallExpr
    }
}

impl NodeInfo for MemberCallNode {
    fn node_type(&self) -> NodeType {
        NodeType::MemberCallExpr
    }
}
