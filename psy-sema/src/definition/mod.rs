mod array;
mod r#const;
mod r#enum;
mod function;
mod r#impl;
mod r#struct;
mod r#trait;
mod type_alias;

pub use array::*;
use enum_as_inner::EnumAsInner;
pub use function::*;
use psy_ast::{IdentId, NodeInfo, NodeType, UseNode};
pub use r#const::*;
pub use r#enum::*;
pub use r#impl::*;
pub use r#struct::*;
pub use r#trait::*;
pub use type_alias::*;

use crate::TypeId;

pub type CheckedUseNode = UseNode;

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum CheckedDefinitionNode {
    Function(CheckedFunctionNode),
    Struct(CheckedStructNode),
    Enum(CheckedEnumNode),
    Impl(CheckedImplNode),
    TraitImpl(CheckedTraitImplNode),
    Trait(CheckedTraitNode),
    TypeAlias(CheckedTypeAliasNode),
    Const(CheckedConstNode),
    Use(CheckedUseNode),
}

impl CheckedDefinitionNode {
    pub fn name(&self) -> IdentId {
        match self {
            Self::Function(node) => node.name.id,
            Self::Struct(node) => node.name.id,
            Self::Enum(node) => node.name.id,
            Self::Trait(node) => node.name.id,
            Self::TypeAlias(node) => node.name.id,
            Self::Const(node) => node.name.unwrap().id,
            _ => unreachable!(),
        }
    }

    pub fn type_id(&self) -> TypeId {
        match self {
            Self::Function(node) => node.type_id,
            Self::Struct(node) => node.type_id,
            Self::Trait(node) => node.type_id,
            _ => unreachable!(),
        }
    }
}

impl NodeInfo for CheckedDefinitionNode {
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
