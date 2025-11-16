use crate::{Comment, ExprId, Location, NodeInfo, NodeType};

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnNode {
    pub expr_id: Option<ExprId>,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for ReturnNode {
    fn node_type(&self) -> NodeType {
        NodeType::ReturnStmt
    }
}
