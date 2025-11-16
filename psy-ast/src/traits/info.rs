use crate::{DefId, ExprId, NodeType};

pub trait NodeInfo {
    fn node_type(&self) -> NodeType;
    fn as_expression(&self) -> Option<ExprId> {
        None
    }
    fn as_definition(&self) -> Option<DefId> {
        None
    }
}
