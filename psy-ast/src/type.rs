use std::fmt::Display;

use enum_as_inner::EnumAsInner;

use crate::{ConstValue, FunctionSignature, Identifier, Location, PathNode};

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum UncheckedType {
    Basic(Identifier), // u8, T
    Const(ConstValue, Location),
    Generic(Identifier, Vec<UncheckedType>, Location), // IndexMap<K, V>
    Array(Box<UncheckedType>, u32, Location),          // [u8; 10]
    Tuple(Vec<UncheckedType>, Location),
    FunctionSignature(Box<FunctionSignature>, Location),
    Path(Box<PathNode>),                                         // PathNode
    TraitCast(Box<UncheckedType>, Box<UncheckedType>, Location), // Type as Trait
    Unknown,
}

impl UncheckedType {
    pub fn location(&self) -> Location {
        match self {
            UncheckedType::Const(_, location) => *location,
            UncheckedType::Basic(ty) => ty.location,
            UncheckedType::Generic(_, _, location) => *location,
            UncheckedType::Array(_, _, location) => *location,
            UncheckedType::Tuple(_unchecked_types, location) => *location,
            UncheckedType::FunctionSignature(_function_signature, location) => *location,
            UncheckedType::TraitCast(_, _, location) => *location,
            UncheckedType::Path(path_node) => path_node.location,
            UncheckedType::Unknown => Location::default(),
        }
    }

    pub fn basic_target(&self) -> Option<Identifier> {
        match self {
            UncheckedType::Basic(ty) => Some(*ty),
            _ => None,
        }
    }
}
