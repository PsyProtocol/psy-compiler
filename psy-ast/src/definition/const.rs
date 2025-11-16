use crate::{Comment, ExprId, Identifier, Location, NodeInfo, NodeType, UncheckedType, Visibility};

#[derive(Clone, Debug, PartialEq)]
pub struct ConstNode {
    pub name: Identifier,
    pub ty: UncheckedType,
    pub value: ExprId,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for ConstNode {
    fn node_type(&self) -> NodeType {
        NodeType::ConstDef
    }
}
