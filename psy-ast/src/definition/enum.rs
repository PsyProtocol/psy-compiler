use indexmap::IndexMap;

use crate::{Comment, GenericParameter, Identifier, Location, NodeInfo, NodeType, StructField, UncheckedType, Visibility};

#[derive(Clone, Debug, PartialEq)]
pub enum EnumVariant {
    Basic(Identifier),
    Tuple(Identifier, Vec<UncheckedType>),
    Struct(Identifier, IndexMap<Identifier, StructField>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumNode {
    pub name: Identifier,
    pub generic_parameters: Vec<GenericParameter>,
    pub variants: Vec<EnumVariant>,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for EnumNode {
    fn node_type(&self) -> NodeType {
        NodeType::EnumDef
    }
}
