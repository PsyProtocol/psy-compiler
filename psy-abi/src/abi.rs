use psy_ast::{DefaultVisitorContext, UncheckedType, VisitorContext};
pub use psy_data::abi::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TypeAbiSpec {
    Basic(String),
    Array {
        #[serde(rename = "type")]
        type_name: String,
        inner_type: String,
        length: u32,
    },
}

impl TypeAbiSpec {
    pub fn from_unchecked_type<F: Clone + From<u32>>(unchecked_type: &UncheckedType, ctx: &DefaultVisitorContext<F, ()>) -> Self {
        match unchecked_type {
            UncheckedType::Basic(identifier) => TypeAbiSpec::Basic(ctx.ident(*identifier).0.to_string()),
            UncheckedType::Array(element_type, size, _) => {
                let inner_type = match Self::from_unchecked_type(element_type, ctx) {
                    TypeAbiSpec::Basic(name) => name,
                    TypeAbiSpec::Array { inner_type, .. } => inner_type,
                };
                TypeAbiSpec::Array {
                    type_name: "Array".to_string(),
                    inner_type,
                    length: *size,
                }
            }
            UncheckedType::Generic(identifier, _generics, _) => TypeAbiSpec::Basic(ctx.ident(*identifier).0.to_string()),
            UncheckedType::Path(path) => Self::from_unchecked_type(&path.target, ctx),
            _ => TypeAbiSpec::Basic("unknown".to_string()),
        }
    }
}
