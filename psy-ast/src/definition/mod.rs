mod attr;
mod r#const;
mod r#enum;
mod function;
mod generic;
mod r#impl;
mod r#struct;
mod r#trait;
mod type_alias;

pub use attr::*;
use enum_as_inner::EnumAsInner;
pub use function::*;
pub use generic::*;
pub use r#const::*;
pub use r#enum::*;
pub use r#impl::*;
pub use r#struct::*;
pub use r#trait::*;
pub use type_alias::*;

use crate::{Comment, Location, NodeInfo, NodeType, UseNode};

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum DefinitionNode {
    Function(FunctionNode),
    Struct(StructNode),
    Enum(EnumNode),
    Impl(ImplNode),
    TraitImpl(TraitImplNode),
    Trait(TraitNode),
    TypeAlias(TypeAliasNode),
    Const(ConstNode),
    Use(UseNode),
}

impl NodeInfo for DefinitionNode {
    fn node_type(&self) -> NodeType {
        match self {
            Self::Function(node) => node.node_type(),
            Self::Struct(node) => node.node_type(),
            Self::Enum(node) => node.node_type(),
            Self::Impl(node) => node.node_type(),
            Self::TraitImpl(node) => node.node_type(),
            Self::Trait(node) => node.node_type(),
            Self::TypeAlias(node) => node.node_type(),
            Self::Const(node) => node.node_type(),
            Self::Use(node) => node.node_type(),
        }
    }
}

impl PartialOrd for DefinitionNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.is_use().partial_cmp(&other.is_use())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommentNode {
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for CommentNode {
    fn node_type(&self) -> NodeType {
        NodeType::Comment
    }
}
