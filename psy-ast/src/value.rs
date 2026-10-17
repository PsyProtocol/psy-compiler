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

/// Builtin unsigned integer widths. One metadata table drives every layer
/// (lexer suffixes, sema typing, limb layouts, ABI names); adding a width is a
/// new variant plus the table rows it feeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, EnumAsInner)]
pub enum UintWidth {
    U32,
    U64,
    U128,
    U256,
}

impl UintWidth {
    pub const ALL: [UintWidth; 4] = [UintWidth::U32, UintWidth::U64, UintWidth::U128, UintWidth::U256];

    /// Base-2^32 limb count: u32 = 1, u64 = 2, u128 = 4, u256 = 8.
    pub const fn limb_count(self) -> usize {
        match self {
            UintWidth::U32 => 1,
            UintWidth::U64 => 2,
            UintWidth::U128 => 4,
            UintWidth::U256 => 8,
        }
    }

    pub const fn bits(self) -> u32 {
        32 * self.limb_count() as u32
    }

    pub const fn name(self) -> &'static str {
        match self {
            UintWidth::U32 => "u32",
            UintWidth::U64 => "u64",
            UintWidth::U128 => "u128",
            UintWidth::U256 => "u256",
        }
    }
}

#[derive(Clone, Debug, PartialEq, EnumAsInner)]
pub enum ValueNode<F: Clone + From<u32>> {
    Felt(F, Location),
    Bool(F, Location),
    /// Typed unsigned integer literal: fixed 8-limb payload, narrow widths
    /// zero-pad the high limbs.
    Uint([u32; 8], UintWidth, Location),
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
            ValueNode::Uint(_, width, _) => write!(f, "{}", width.name()),
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
        let limbs = |low: u32| [low, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(ValueNode::<u64>::Uint(limbs(1), UintWidth::U32, location).to_string(), "u32");
        assert_eq!(ValueNode::<u64>::Uint(limbs(1), UintWidth::U64, location).to_string(), "u64");
        assert_eq!(ValueNode::<u64>::Uint(limbs(1), UintWidth::U128, location).to_string(), "u128");
        assert_eq!(ValueNode::<u64>::Uint(limbs(1), UintWidth::U256, location).to_string(), "u256");

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
