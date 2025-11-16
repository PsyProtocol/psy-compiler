use psy_ast::Visibility;

use crate::{ConstId, Identifier, NodeInfo, NodeType, ScopeId, TypeId};

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedConstNode {
    pub name: Option<Identifier>,
    pub ty: TypeId,
    pub value: ConstId,
    pub visibility: Visibility,
    pub scope_id: ScopeId,
}

impl NodeInfo for CheckedConstNode {
    fn node_type(&self) -> NodeType {
        NodeType::ConstDef
    }
}
