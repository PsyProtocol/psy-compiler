use psy_ast::{Comment, ExprId, Location, NodeInfo, NodeType};

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedReturnNode {
    pub ret: Option<ExprId>,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for CheckedReturnNode {
    fn node_type(&self) -> NodeType {
        NodeType::ReturnStmt
    }
}
