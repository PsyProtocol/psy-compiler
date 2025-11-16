use psy_ast::{Comment, Visibility};

use crate::{Identifier, NodeInfo, NodeType, TypeId};

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedTypeAliasNode {
    pub name: Identifier,
    pub ty: TypeId,
    pub comments: Vec<Comment>,
    pub visibility: Visibility,
}

impl NodeInfo for CheckedTypeAliasNode {
    fn node_type(&self) -> NodeType {
        NodeType::TypeAliasDef
    }
}
