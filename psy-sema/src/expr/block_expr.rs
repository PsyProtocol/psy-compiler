use psy_ast::{ExprId, Location, NodeInfo, NodeType, StmtId};

use crate::{ScopeId, TypeId};

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedBlockExprNode {
    pub stmts: Vec<StmtId>,
    pub expr: Option<ExprId>,
    pub type_id: TypeId,
    pub scope_id: ScopeId,
    pub location: Location,
}

impl NodeInfo for CheckedBlockExprNode {
    fn node_type(&self) -> NodeType {
        NodeType::BlockExpr
    }
}
