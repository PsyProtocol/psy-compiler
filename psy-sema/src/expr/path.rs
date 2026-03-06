use psy_ast::{IdentId, Location, NodeInfo, NodeType, PathNode};

use crate::{TypeId, VarId};

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedPathNode {
    pub variable: Option<VarId>,
    pub root: Option<TypeId>,
    pub target: Option<IdentId>,
    pub origin_path: PathNode,
    pub type_id: TypeId,
    pub trait_ty: Option<TypeId>,
    pub location: Location,
}

impl CheckedPathNode {
    pub fn new(
        variable: Option<VarId>,
        root: Option<TypeId>,
        target: Option<IdentId>,
        origin_path: PathNode,
        type_id: TypeId,
        trait_ty: Option<TypeId>,
        location: Location,
    ) -> Self {
        Self {
            variable,
            root,
            target,
            origin_path,
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
