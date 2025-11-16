use crate::{Comment, Identifier, Location, NodeInfo, NodeType, UncheckedType, Visibility};

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAliasNode {
    pub name: Identifier,
    pub ty: UncheckedType,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for TypeAliasNode {
    fn node_type(&self) -> NodeType {
        NodeType::TypeAliasDef
    }
}
