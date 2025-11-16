use crate::{Comment, ExprId, Location, NodeInfo, NodeType};

#[derive(Debug, Clone, PartialEq)]
pub struct WhileNode {
    pub predicate: ExprId,
    pub body: ExprId,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for WhileNode {
    fn node_type(&self) -> NodeType {
        NodeType::WhileStmt
    }
}
