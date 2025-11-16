use indexmap::IndexMap;

use crate::{Comment, DefId, GenericParameter, Identifier, Location, NodeInfo, NodeType, UncheckedType, Visibility};

#[derive(Clone, Debug, PartialEq)]
pub struct TraitNode {
    pub name: Identifier,
    pub associated_types: IndexMap<Identifier, AssociatedType>,
    pub generic_parameters: Vec<GenericParameter>,
    pub body: Vec<DefId>,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssociatedType {
    pub constraints: Vec<UncheckedType>,
    pub visibility: Visibility,
    pub location: Location,
    pub comments: Vec<Comment>,
}

impl NodeInfo for TraitNode {
    fn node_type(&self) -> NodeType {
        NodeType::TraitDef
    }
}
