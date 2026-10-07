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

impl ConstValue {
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            ConstValue::Felt(value) => Some(*value),
            ConstValue::U32(value) => Some(u64::from(*value)),
            ConstValue::Bool(_) => None,
        }
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
    Array(ConstValue, Vec<ExprId>, Location),
    ArrayRepeat(ExprId, ConstValue, Location),
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
            ValueNode::ArrayRepeat(expr_id, size, _) => write!(f, "ArrayRepeat({:?}; {})", expr_id, size),
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

#[cfg(test)]
mod tests {
    use crate::{ExprId, IdentId, Identifier, Location, NodeType};

    use super::*;

    #[test]
    fn const_value_conversions_and_display_round_trip() {
        assert_eq!(ConstValue::from(true), ConstValue::Bool(true));
        assert_eq!(ConstValue::from(7u32), ConstValue::U32(7));
        assert_eq!(ConstValue::from(9u64), ConstValue::Felt(9));

        assert_eq!(ConstValue::Felt(3).as_u64(), Some(3));
        assert_eq!(ConstValue::U32(5).as_u64(), Some(5));
        assert_eq!(ConstValue::Bool(false).as_u64(), None);

        assert_eq!(ConstValue::Felt(12).to_string(), "12");
        assert_eq!(ConstValue::U32(34).to_string(), "34");
        assert_eq!(ConstValue::Bool(true).to_string(), "true");
    }

    #[test]
    fn value_node_displays_every_variant() {
        let location = Location::default();
        assert_eq!(ValueNode::<u64>::Felt(1, location).to_string(), "Felt");
        assert_eq!(ValueNode::<u64>::Bool(1, location).to_string(), "Bool");
        assert_eq!(ValueNode::<u64>::U32(1, location).to_string(), "U32");

        let array = ValueNode::<u64>::Array(ConstValue::from(2u32), vec![ExprId(0), ExprId(1)], location);
        let rendered = array.to_string();
        assert!(rendered.starts_with("Array("), "array display: {rendered}");
        assert!(rendered.contains(", "), "array display joins elements: {rendered}");
        assert!(rendered.ends_with(')'), "array display: {rendered}");
        let empty = ValueNode::<u64>::Array(ConstValue::from(0u32), vec![], location);
        assert_eq!(empty.to_string(), "Array()");

        let repeat = ValueNode::<u64>::ArrayRepeat(ExprId(0), ConstValue::from(3u32), location);
        assert!(repeat.to_string().starts_with("ArrayRepeat("));

        let mut fields = IndexMap::new();
        fields.insert(Identifier::new(IdentId(0), location), ExprId(0));
        let structure = ValueNode::<u64>::Struct(ExprId(2), vec![], fields, location);
        let rendered = structure.to_string();
        assert!(rendered.starts_with("Struct "), "struct display: {rendered}");
        assert!(rendered.ends_with(" }"), "struct display: {rendered}");

        assert_eq!(ValueNode::<u64>::Felt(1, location).node_type(), NodeType::ValueExpr);
    }
}
