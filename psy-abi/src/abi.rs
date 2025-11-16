use psy_ast::{DefaultVisitorContext, UncheckedType, VisitorContext};
use serde::Serialize;

// New spec-compliant ABI structures
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpecCompliantAbi {
    pub version: String,
    pub structs: Vec<StructAbiSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StructAbiSpec {
    pub name: String,
    pub is_contract: bool,
    pub fields: Vec<FieldAbiSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub functions: Option<Vec<FunctionAbiSpec>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldAbiSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: TypeAbiSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionAbiSpec {
    pub name: String,
    pub params: Vec<ParamAbiSpec>,
    #[serde(rename = "return")]
    pub return_type: Vec<TypeAbiSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParamAbiSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: TypeAbiSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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

impl SpecCompliantAbi {
    pub fn new(version: String) -> Self {
        Self {
            version,
            structs: Vec::new(),
        }
    }

    pub fn add_struct(&mut self, struct_spec: StructAbiSpec) {
        self.structs.push(struct_spec);
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
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
