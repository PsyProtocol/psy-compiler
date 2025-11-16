use std::fmt::{Display, Formatter};

use enum_as_inner::EnumAsInner;
use indexmap::IndexMap;

#[derive(Debug, Copy, Clone, PartialEq, EnumAsInner)]
pub enum ConstValue {
    Felt(u64),
    U32(u32),
    Bool(bool),
}

impl From<bool> for ConstValue {
    fn from(value: bool) -> Self {
        ConstValue::Bool(value)
    }
}

impl From<u32> for ConstValue {
    fn from(value: u32) -> Self {
        ConstValue::U32(value)
    }
}

impl From<u64> for ConstValue {
    fn from(value: u64) -> Self {
        ConstValue::Felt(value)
    }
}

impl Display for ConstValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstValue::Felt(value) => write!(f, "{}", value),
            ConstValue::U32(value) => write!(f, "{}", value),
            ConstValue::Bool(value) => write!(f, "{}", value),
        }
    }
}
use crate::{ExprId, Identifier, Location, NodeInfo, NodeType, UncheckedType};

#[derive(Clone, Debug, PartialEq, EnumAsInner)]
pub enum ValueNode<F: Clone + From<u32>> {
    Felt(F, Location),
    Bool(F, Location),
    U32(F, Location),
    Array(u32, Vec<ExprId>, Location),
    Struct(ExprId, Vec<UncheckedType>, IndexMap<Identifier, ExprId>, Location),
}

impl<F: Clone + From<u32>> NodeInfo for ValueNode<F> {
    fn node_type(&self) -> NodeType {
        NodeType::ValueExpr
    }
}

impl<F: Clone + From<u32>> Display for ValueNode<F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ValueNode::Felt(_, _) => write!(f, "Felt"),
            ValueNode::Bool(_, _) => write!(f, "Bool"),
            ValueNode::U32(_, _) => write!(f, "U32"),
            ValueNode::Array(_, expr_ids, _) => {
                write!(f, "Array(")?;
                for (i, value) in expr_ids.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{:?}", value)?;
                }
                write!(f, ")")
            }
            ValueNode::Struct(name, _, fields, _) => {
                write!(f, "Struct {:?} {{ ", name)?;
                for (i, (field_name, field_value)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{:?}: {:?}", field_name, field_value)?;
                }
                write!(f, " }}")
            }
        }
    }
}
