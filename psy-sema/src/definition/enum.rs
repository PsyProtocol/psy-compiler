use indexmap::IndexMap;
use psy_ast::{Comment, Identifier, Location, NodeInfo, NodeType, Visibility};

use crate::{CheckedStructField, ScopeId, TypeId};

#[derive(Clone, Debug, PartialEq)]
pub enum CheckedEnumVariant {
    Basic(Identifier, TypeId),
    Tuple(Identifier, Vec<TypeId>),
    Struct(Identifier, IndexMap<Identifier, CheckedStructField>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedEnumNode {
    pub name: Identifier,
    pub generic_parameters: Vec<TypeId>,
    pub variants: Vec<CheckedEnumVariant>,
    pub scope_id: ScopeId,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for CheckedEnumNode {
    fn node_type(&self) -> NodeType {
        NodeType::EnumDef
    }
}
