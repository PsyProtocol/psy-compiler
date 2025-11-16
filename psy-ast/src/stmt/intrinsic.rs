use crate::{Comment, ExprId, Location, NodeInfo, NodeType};

#[derive(Debug, Clone, PartialEq)]
pub enum IntrinsicStmtNode {
    Assert {
        left: ExprId,
        message: Option<String>,
        comments: Vec<Comment>,
        location: Location,
    },
    AssertEq {
        left: ExprId,
        right: ExprId,
        message: Option<String>,
        comments: Vec<Comment>,
        location: Location,
    },
    ClearEntireTree {
        comments: Vec<Comment>,
        location: Location,
    },
}

impl NodeInfo for IntrinsicStmtNode {
    fn node_type(&self) -> NodeType {
        NodeType::IntrinsicStmt
    }
}
