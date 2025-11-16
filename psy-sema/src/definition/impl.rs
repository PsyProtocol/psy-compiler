use indexmap::IndexMap;
use psy_ast::{Comment, DefId, IdentId, Identifier, Location, NodeInfo, NodeType, Visibility};

use crate::{ScopeId, TypeId};

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedImplNode {
    pub generic_parameters: Vec<TypeId>,
    pub associated_types: IndexMap<Identifier, CheckedAssociatedTypeValue>,
    pub ty: TypeId,
    pub body: Vec<DefId>,
    pub scope_id: ScopeId,
    pub comments: Vec<Comment>,
    pub location: Location,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedTraitImplNode {
    pub generic_parameters: Vec<TypeId>,
    pub associated_types: IndexMap<Identifier, CheckedAssociatedTypeValue>,
    pub trait_ty: TypeId,
    pub ty: TypeId,
    pub body: Vec<DefId>,
    pub scope_id: ScopeId,
    pub comments: Vec<Comment>,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedAssociatedTypeValue {
    pub root: Option<TypeId>,
    pub target: Option<IdentId>,
    pub type_id: TypeId,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for CheckedImplNode {
    fn node_type(&self) -> NodeType {
        NodeType::ImplDef
    }
}

impl NodeInfo for CheckedTraitImplNode {
    fn node_type(&self) -> NodeType {
        NodeType::TraitImplDef
    }
}
