use crate::{Comment, ExprId, Location, NodeInfo, NodeType, StmtId};

#[derive(Debug, Clone, PartialEq)]
pub struct BlockExprNode {
    pub stmts: Vec<StmtId>,
    pub expr: Option<ExprId>,
    pub expr_comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for BlockExprNode {
    fn node_type(&self) -> NodeType {
        NodeType::BlockExpr
    }
}
