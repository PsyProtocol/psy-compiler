use indexmap::IndexMap;

use crate::{AttrNode, Comment, GenericParameter, Identifier, Location, NodeInfo, NodeType, UncheckedType, Visibility};

#[derive(Clone, Debug, PartialEq)]
pub struct StructNode {
    pub name: Identifier,
    pub generic_parameters: Vec<GenericParameter>,
    pub fields: IndexMap<Identifier, StructField>,
    pub attrs: Vec<AttrNode>,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
    pub is_generated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructField {
    pub ty: UncheckedType,
    pub attrs: Vec<AttrNode>,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for StructNode {
    fn node_type(&self) -> NodeType {
        NodeType::StructDef
    }
}
