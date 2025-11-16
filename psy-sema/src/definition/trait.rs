use indexmap::IndexMap;
use psy_ast::{Comment, DefId, Identifier, Location, NodeInfo, NodeType, Visibility};

use crate::{ScopeId, TypeId};

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedTraitNode {
    pub name: Identifier,
    pub associated_types: IndexMap<Identifier, CheckedAssociatedType>,
    pub generic_parameters: Vec<TypeId>,
    pub body: Vec<DefId>,
    pub unchecked_body: Vec<DefId>,
    pub scope_id: ScopeId,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
    pub type_id: TypeId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedAssociatedType {
    pub type_id: TypeId,
    pub constraints: Vec<TypeId>,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for CheckedTraitNode {
    fn node_type(&self) -> NodeType {
        NodeType::TraitDef
    }
}
