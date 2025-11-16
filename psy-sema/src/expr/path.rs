use psy_ast::{IdentId, Location, NodeInfo, NodeType};

use crate::{TypeId, VarId};

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedPathNode {
    pub variable: Option<VarId>,
    pub root: Option<TypeId>,
    pub target: Option<IdentId>,
    pub type_id: TypeId,
    pub trait_ty: Option<TypeId>,
    pub location: Location,
}

impl CheckedPathNode {
    pub fn new(
        variable: Option<VarId>,
        root: Option<TypeId>,
        target: Option<IdentId>,
        type_id: TypeId,
        trait_ty: Option<TypeId>,
        location: Location,
    ) -> Self {
        Self {
            variable,
            root,
            target,
            type_id,
            trait_ty,
            location,
        }
    }
}

impl NodeInfo for CheckedPathNode {
    fn node_type(&self) -> NodeType {
        NodeType::PathExpr
    }
}
