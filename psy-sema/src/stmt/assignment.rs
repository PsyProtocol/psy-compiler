use psy_ast::{AssignmentOperator, Comment, ExprId, Location, NodeInfo, NodeType};

use crate::TypeId;

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedAssignmentNode {
    pub target: ExprId,
    pub operator: AssignmentOperator,
    pub value: ExprId,
    pub type_id: TypeId,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for CheckedAssignmentNode {
    fn node_type(&self) -> NodeType {
        NodeType::AssignmentStmt
    }
}
