use indexmap::IndexMap;

use crate::{Comment, DefId, GenericParameter, Identifier, Location, NodeInfo, NodeType, UncheckedType, Visibility};

#[derive(Debug, Clone, PartialEq)]
pub struct ImplNode {
    pub generic_parameters: Vec<GenericParameter>,
    pub associated_types: IndexMap<Identifier, AssociatedTypeValue>,
    pub ty: UncheckedType,
    pub body: Vec<DefId>,
    pub comments: Vec<Comment>,
    pub location: Location,
    pub is_generated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitImplNode {
    pub generic_parameters: Vec<GenericParameter>,
    pub associated_types: IndexMap<Identifier, AssociatedTypeValue>,
    pub trait_ty: UncheckedType,
    pub ty: UncheckedType,
    pub body: Vec<DefId>,
    pub comments: Vec<Comment>,
    pub location: Location,
    pub is_generated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssociatedTypeValue {
    pub ty: UncheckedType,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for ImplNode {
    fn node_type(&self) -> NodeType {
        NodeType::ImplDef
    }
}

impl NodeInfo for TraitImplNode {
    fn node_type(&self) -> NodeType {
        NodeType::TraitImplDef
    }
}
