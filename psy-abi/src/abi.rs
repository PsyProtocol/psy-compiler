use psy_ast::{DefaultVisitorContext, UncheckedType, VisitorContext};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ABI types — the single source-of-truth ABI shape.
// ---------------------------------------------------------------------------

/// Top-level ABI shape emitted by the compiler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Abi {
    pub schema_version: String,
    pub contract: AbiContract,
    pub types: Vec<AbiStructType>,
}

/// Contract-level metadata + state layout + methods.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbiContract {
    pub name: String,
    pub state_tree_height: u16,
    pub state: Vec<AbiStateField>,
    pub methods: Vec<AbiMethod>,
}

/// A state field with its absolute slot offset and felt footprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbiStateField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    pub offset: usize,
    pub felt_size: usize,
}

/// A method with explicit compiler-owned metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbiMethod {
    pub name: String,
    pub method_id: u32,
    pub state_mutability: StateMutability,
    pub inputs: Vec<AbiParam>,
    pub outputs: Vec<AbiParam>,
    pub input_felt_count: usize,
    pub output_felt_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm_type: Option<String>,
}

/// View vs. external mutability — replaces `is_view` heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateMutability {
    View,
    External,
}

/// A typed parameter (input or output).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbiParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    pub felt_size: usize,
}

/// A named struct type entry in the `types[]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbiStructType {
    pub kind: AbiTypeKind,
    pub name: String,
    pub felt_size: usize,
    pub fields: Vec<AbiStructField>,
}

/// Marker for the type-table entry kind.  Currently only `struct`, but
/// reserved for future `enum` / `alias` entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbiTypeKind {
    Struct,
}

/// A field within a `AbiStructType`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbiStructField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    pub offset_within_parent: usize,
    pub felt_size: usize,
}

/// Recursive type reference — the type model used by state, params,
/// and struct fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeRef {
    Primitive {
        name: PrimitiveTypeName,
    },
    Struct {
        name: String,
    },
    Array {
        item: Box<TypeRef>,
        length: u64,
        item_felt_size: usize,
    },
    Map {
        map_kind: MapKind,
        key: Box<TypeRef>,
        value: Box<TypeRef>,
        capacity: usize,
        value_felt_size: usize,
        alignment_felts: u32,
    },
}

/// The set of primitive type names the compiler actually emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveTypeName {
    Felt,
    Bool,
    U32,
    Hash,
}

/// Source-level map type family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapKind {
    ContractHashMap,
    Map,
    NamespacedMap,
}

impl Abi {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

impl StateMutability {
    pub fn is_view(&self) -> bool {
        matches!(self, StateMutability::View)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TypeAbiSpec {
    Basic(String),
    Array {
        #[serde(rename = "type")]
        type_name: String,
        inner_type: String,
        length: u64,
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
                    length: size.as_u64().unwrap_or(0),
                }
            }
            UncheckedType::Generic(identifier, _generics, _) => TypeAbiSpec::Basic(ctx.ident(*identifier).0.to_string()),
            UncheckedType::Path(path) => Self::from_unchecked_type(&path.target, ctx),
            _ => TypeAbiSpec::Basic("unknown".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_abi() -> Abi {
        Abi {
            schema_version: "2.0.0".to_string(),
            contract: AbiContract {
                name: "Wallet".to_string(),
                state_tree_height: 7,
                state: vec![AbiStateField {
                    name: "balances".to_string(),
                    ty: TypeRef::Map {
                        map_kind: MapKind::Map,
                        key: Box::new(TypeRef::Primitive {
                            name: PrimitiveTypeName::Hash,
                        }),
                        value: Box::new(TypeRef::Array {
                            item: Box::new(TypeRef::Primitive {
                                name: PrimitiveTypeName::Felt,
                            }),
                            length: 2,
                            item_felt_size: 1,
                        }),
                        capacity: 128,
                        value_felt_size: 2,
                        alignment_felts: 4,
                    },
                    offset: 0,
                    felt_size: 2,
                }],
                methods: vec![AbiMethod {
                    name: "balance".to_string(),
                    method_id: 3,
                    state_mutability: StateMutability::View,
                    inputs: vec![AbiParam {
                        name: "owner".to_string(),
                        ty: TypeRef::Struct { name: "Owner".to_string() },
                        felt_size: 4,
                    }],
                    outputs: vec![AbiParam {
                        name: "result".to_string(),
                        ty: TypeRef::Primitive {
                            name: PrimitiveTypeName::U32,
                        },
                        felt_size: 1,
                    }],
                    input_felt_count: 4,
                    output_felt_count: 1,
                    vm_type: None,
                }],
            },
            types: vec![AbiStructType {
                kind: AbiTypeKind::Struct,
                name: "Owner".to_string(),
                felt_size: 4,
                fields: vec![AbiStructField {
                    name: "id".to_string(),
                    ty: TypeRef::Primitive {
                        name: PrimitiveTypeName::Hash,
                    },
                    offset_within_parent: 0,
                    felt_size: 4,
                }],
            }],
        }
    }

    #[test]
    fn abi_json_round_trip_preserves_recursive_layout() {
        let abi = sample_abi();
        let json = abi.to_json().expect("sample ABI should serialize");
        let decoded: Abi = serde_json::from_str(&json).expect("serialized ABI should deserialize");

        assert_eq!(decoded, abi);
        assert!(json.contains("\"state_mutability\": \"view\""));
        assert!(!json.contains("vm_type"), "None vm_type must be omitted");
    }

    #[test]
    fn abi_enum_wire_names_are_stable() {
        assert_eq!(serde_json::to_string(&StateMutability::External).unwrap(), "\"external\"");
        assert_eq!(serde_json::to_string(&AbiTypeKind::Struct).unwrap(), "\"struct\"");
        assert_eq!(serde_json::to_string(&MapKind::ContractHashMap).unwrap(), "\"contract_hash_map\"");
        assert_eq!(serde_json::to_string(&MapKind::NamespacedMap).unwrap(), "\"namespaced_map\"");
        assert!(StateMutability::View.is_view());
        assert!(!StateMutability::External.is_view());
    }

    #[test]
    fn type_abi_spec_supports_both_wire_shapes() {
        let basic: TypeAbiSpec = serde_json::from_str("\"Felt\"").unwrap();
        assert_eq!(basic, TypeAbiSpec::Basic("Felt".to_string()));

        let array: TypeAbiSpec = serde_json::from_str(r#"{"type":"Array","inner_type":"Hash","length":8}"#).unwrap();
        assert_eq!(
            array,
            TypeAbiSpec::Array {
                type_name: "Array".to_string(),
                inner_type: "Hash".to_string(),
                length: 8,
            }
        );
    }
}
