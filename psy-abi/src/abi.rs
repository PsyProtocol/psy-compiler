use std::collections::BTreeMap;

use anyhow::Context;
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
    /// Canonical, field-oriented storage manifest consumed by deployment and
    /// contract-update tooling. Hashes are computed downstream with the
    /// protocol hasher; the compiler owns declaration order, type shape, and
    /// physical slot ranges.
    pub state_layout: AbiStateLayout,
    pub methods: Vec<AbiMethod>,
}

/// Canonical storage manifest. Slot ranges include alignment padding and the
/// recursive storage footprint of every physical field, public or private.
/// The public `state` list is a filtered view of the same ranges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiStateLayout {
    pub layout_version: u16,
    pub encoding_version: u16,
    pub field_count: u64,
    pub slot_count: u64,
    pub fields: Vec<AbiLayoutField>,
    /// Deterministic, deduplicated jobs used by the prover to construct the
    /// recursive type-layout proof tree.
    pub type_proof_plan: AbiTypeProofPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiTypeProofPlan {
    pub protocol_version: u16,
    pub nodes: Vec<AbiTypeProofNode>,
    /// Node id for each top-level field, in field order.
    pub field_nodes: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiTypeProofNode {
    pub node_id: u32,
    /// Stable structural descriptor used for deterministic DAG deduplication.
    pub type_key: String,
    pub circuit_kind: AbiTypeProofCircuitKind,
    pub dependencies: Vec<u32>,
    /// The proving layer combines this material with the deployed circuit
    /// fingerprint; fingerprints intentionally do not come from source code.
    pub cache_key_material: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbiTypeProofCircuitKind {
    Primitive,
    FixedArray,
    FixedMap,
    Struct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiLayoutField {
    /// One-based position in the top-level layout tree.
    pub field_id: u64,
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    pub start_slot: u64,
    pub payload_offset: u64,
    pub slot_count: u64,
}

/// A state field with its absolute slot offset and felt footprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbiStateField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    pub offset: usize,
    pub felt_size: usize,
    /// Omitted for public fields so previously published ABI stays valid.
    #[serde(default, skip_serializing_if = "FieldVisibility::is_public")]
    pub visibility: FieldVisibility,
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
    /// Storage footprint of the member. A map occupies `(capacity + 1) * 4`
    /// felts, the same width `compute_struct_layout` uses for its offset.
    pub felt_size: usize,
    #[serde(default, skip_serializing_if = "FieldVisibility::is_public")]
    pub visibility: FieldVisibility,
}

/// Recursive type reference — the type model used by state, params,
/// and struct fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Whether a published field is part of the public ABI surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FieldVisibility {
    #[default]
    Public,
    Private,
}

impl FieldVisibility {
    pub fn is_public(&self) -> bool {
        matches!(self, FieldVisibility::Public)
    }
}

pub(crate) fn field_visibility(visibility: &psy_ast::Visibility) -> FieldVisibility {
    match visibility {
        psy_ast::Visibility::Public => FieldVisibility::Public,
        psy_ast::Visibility::Private => FieldVisibility::Private,
    }
}

impl Abi {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Existing top-level fields and every previously declared struct type are
    /// immutable. New struct definitions are allowed only under new names and
    /// can be referenced by newly appended top-level fields.
    pub fn validate_layout_update_from(&self, old: &Self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.contract.name == old.contract.name,
            "contract identity cannot change during layout update"
        );
        anyhow::ensure!(
            self.contract.state_tree_height == old.contract.state_tree_height,
            "contract state tree height cannot change"
        );
        self.contract
            .state_layout
            .validate_append_only_from(&old.contract.state_layout)?;
        for old_type in &old.types {
            let new_type = self
                .types
                .iter()
                .find(|candidate| candidate.name == old_type.name)
                .with_context(|| format!("existing ABI type '{}' was removed", old_type.name))?;
            anyhow::ensure!(new_type == old_type, "existing ABI type '{}' was modified", old_type.name);
        }
        Ok(())
    }
}

impl AbiStateLayout {
    pub fn empty() -> Self {
        Self {
            layout_version: 1,
            encoding_version: 1,
            field_count: 0,
            slot_count: 0,
            fields: Vec::new(),
            type_proof_plan: AbiTypeProofPlan {
                protocol_version: 1,
                nodes: Vec::new(),
                field_nodes: Vec::new(),
            },
        }
    }

    /// Build the canonical manifest from every physical state field.
    ///
    /// `state` is the complete physical field list, public and private, in
    /// declaration order. Each field's `offset` is its physical payload start
    /// after preceding private fields, map alignment, and the recursive
    /// storage footprint. Private fields are not filtered out: dropping them
    /// leaves a gap the alignment check cannot explain.
    pub fn from_state_fields(state: &[AbiStateField], types: &[AbiStructType]) -> anyhow::Result<Self> {
        let mut layout_frontier = 0u64;
        let fields = state
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let payload_start = u64::try_from(field.offset).context("state field offset does not fit in u64")?;
                let payload_offset = payload_start.checked_sub(layout_frontier).with_context(|| {
                    format!("state field '{}' offset {payload_start} moves before frontier {layout_frontier}", field.name)
                })?;
                let felt_size = u64::try_from(field.felt_size).context("state field size does not fit in u64")?;
                let slot_count = payload_offset
                    .checked_add(felt_size)
                    .context("state layout field slot count overflow")?;
                let alignment = match &field.ty {
                    TypeRef::Map { alignment_felts, .. } => u64::from(*alignment_felts),
                    _ => 1,
                };
                anyhow::ensure!(
                    alignment > 0 && alignment.is_power_of_two(),
                    "state layout field '{}' has invalid alignment",
                    field.name
                );
                let expected_payload_offset = (alignment - (layout_frontier % alignment)) % alignment;
                anyhow::ensure!(
                    payload_offset == expected_payload_offset,
                    "state layout field '{}' has incorrect alignment padding",
                    field.name
                );
                let result = AbiLayoutField {
                    field_id: index as u64 + 1,
                    name: field.name.clone(),
                    ty: field.ty.clone(),
                    start_slot: layout_frontier,
                    payload_offset,
                    slot_count,
                };
                layout_frontier = layout_frontier.checked_add(slot_count).context("state layout frontier overflow")?;
                Ok(result)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let layout = Self {
            layout_version: 1,
            encoding_version: 1,
            field_count: fields.len() as u64,
            slot_count: layout_frontier,
            fields,
            type_proof_plan: AbiTypeProofPlan::from_fields(state, types)?,
        };
        layout.validate()?;
        Ok(layout)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.layout_version == 1, "unsupported state layout version {}", self.layout_version);
        anyhow::ensure!(
            self.encoding_version == 1,
            "unsupported state layout encoding version {}",
            self.encoding_version
        );
        anyhow::ensure!(
            self.field_count == self.fields.len() as u64,
            "state layout field_count does not match fields length"
        );
        anyhow::ensure!(
            self.type_proof_plan.protocol_version == self.layout_version,
            "type proof plan protocol version does not match layout version"
        );
        anyhow::ensure!(
            self.type_proof_plan.field_nodes.len() == self.fields.len(),
            "type proof plan must contain one endpoint per field"
        );
        for (index, node) in self.type_proof_plan.nodes.iter().enumerate() {
            anyhow::ensure!(node.node_id == index as u32, "type proof plan node ids must be contiguous");
            anyhow::ensure!(
                node.dependencies.iter().all(|dependency| *dependency < node.node_id),
                "type proof plan dependencies must precede their parent"
            );
        }
        anyhow::ensure!(
            self.type_proof_plan
                .field_nodes
                .iter()
                .all(|node| (*node as usize) < self.type_proof_plan.nodes.len()),
            "type proof plan contains an invalid field endpoint"
        );
        let mut next_slot = 0u64;
        for (index, field) in self.fields.iter().enumerate() {
            anyhow::ensure!(
                field.field_id == index as u64 + 1,
                "state layout field ids must be contiguous and one-based"
            );
            anyhow::ensure!(field.slot_count > 0, "state layout field '{}' occupies zero slots", field.name);
            anyhow::ensure!(field.start_slot == next_slot, "state layout field '{}' is not contiguous", field.name);
            anyhow::ensure!(
                field.payload_offset < field.slot_count,
                "state layout field '{}' payload is outside its owned range",
                field.name
            );
            let alignment = match &field.ty {
                TypeRef::Map { alignment_felts, .. } => u64::from(*alignment_felts),
                _ => 1,
            };
            anyhow::ensure!(
                alignment > 0 && alignment.is_power_of_two(),
                "state layout field '{}' has invalid alignment",
                field.name
            );
            let expected_payload_offset = (alignment - (field.start_slot % alignment)) % alignment;
            anyhow::ensure!(
                field.payload_offset == expected_payload_offset,
                "state layout field '{}' has incorrect alignment padding",
                field.name
            );
            next_slot = next_slot.checked_add(field.slot_count).context("state layout slot count overflow")?;
        }
        anyhow::ensure!(self.slot_count == next_slot, "state layout slot_count does not match field ranges");
        Ok(())
    }

    pub fn validate_append_only_from(&self, old: &Self) -> anyhow::Result<()> {
        old.validate()?;
        self.validate()?;
        anyhow::ensure!(
            self.layout_version == old.layout_version && self.encoding_version == old.encoding_version,
            "state layout version or encoding cannot change"
        );
        anyhow::ensure!(self.fields.len() >= old.fields.len(), "state layout fields cannot be removed");
        anyhow::ensure!(
            self.fields[..old.fields.len()] == old.fields,
            "existing state layout fields were modified or reordered"
        );
        anyhow::ensure!(self.slot_count >= old.slot_count, "state layout slot count cannot decrease");
        Ok(())
    }
}

impl AbiTypeProofPlan {
    pub fn from_fields(fields: &[AbiStateField], types: &[AbiStructType]) -> anyhow::Result<Self> {
        let definitions = types
            .iter()
            .map(|definition| (definition.name.as_str(), definition))
            .collect::<BTreeMap<_, _>>();
        let mut by_key = BTreeMap::<String, u32>::new();
        let mut nodes = Vec::new();
        let mut visiting = Vec::new();
        let field_nodes = fields
            .iter()
            .map(|field| Self::intern_type(&field.ty, &definitions, &mut by_key, &mut nodes, &mut visiting))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            protocol_version: 1,
            nodes,
            field_nodes,
        })
    }

    fn intern_type(
        ty: &TypeRef,
        definitions: &BTreeMap<&str, &AbiStructType>,
        by_key: &mut BTreeMap<String, u32>,
        nodes: &mut Vec<AbiTypeProofNode>,
        visiting: &mut Vec<String>,
    ) -> anyhow::Result<u32> {
        let (type_key, circuit_kind, dependencies) = match ty {
            TypeRef::Primitive { name } => (format!("primitive:{name:?}"), AbiTypeProofCircuitKind::Primitive, Vec::new()),
            TypeRef::Array { item, length, .. } => {
                let child = Self::intern_type(item, definitions, by_key, nodes, visiting)?;
                let child_key = &nodes[child as usize].type_key;
                (format!("array:{length}:{child_key}"), AbiTypeProofCircuitKind::FixedArray, vec![child])
            }
            TypeRef::Map {
                map_kind,
                key,
                value,
                capacity,
                alignment_felts,
                ..
            } => {
                let key_node = Self::intern_type(key, definitions, by_key, nodes, visiting)?;
                let value_node = Self::intern_type(value, definitions, by_key, nodes, visiting)?;
                (
                    format!(
                        "map:{map_kind:?}:{capacity}:{alignment_felts}:{}:{}",
                        nodes[key_node as usize].type_key, nodes[value_node as usize].type_key
                    ),
                    AbiTypeProofCircuitKind::FixedMap,
                    vec![key_node, value_node],
                )
            }
            TypeRef::Struct { name } => {
                anyhow::ensure!(!visiting.contains(name), "recursive ABI struct '{name}' is not supported");
                let definition = definitions
                    .get(name.as_str())
                    .with_context(|| format!("ABI struct '{name}' not found"))?;
                visiting.push(name.clone());
                let dependencies = definition
                    .fields
                    .iter()
                    .map(|member| Self::intern_type(&member.ty, definitions, by_key, nodes, visiting))
                    .collect::<anyhow::Result<Vec<_>>>()?;
                visiting.pop();
                let member_keys = dependencies
                    .iter()
                    .map(|node| nodes[*node as usize].type_key.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                (format!("struct:{name}:[{member_keys}]"), AbiTypeProofCircuitKind::Struct, dependencies)
            }
        };
        if let Some(node_id) = by_key.get(&type_key) {
            return Ok(*node_id);
        }
        let node_id = u32::try_from(nodes.len()).context("type proof plan exceeds u32 node ids")?;
        nodes.push(AbiTypeProofNode {
            node_id,
            cache_key_material: format!("state-layout:v1:{circuit_kind:?}:{type_key}"),
            type_key: type_key.clone(),
            circuit_kind,
            dependencies,
        });
        by_key.insert(type_key, node_id);
        Ok(node_id)
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
    pub fn from_unchecked_type<F: Clone + From<u32> + std::fmt::Debug>(unchecked_type: &UncheckedType, ctx: &DefaultVisitorContext<F, ()>) -> Self {
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
            UncheckedType::Path(path) => match super::AbiExtractor::new(String::new()).resolved_path_type(ctx, path) {
                Some(resolved) => Self::from_unchecked_type(resolved, ctx),
                None => TypeAbiSpec::Basic("unresolved".to_string()),
            },
            _ => TypeAbiSpec::Basic("unknown".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_abi() -> Abi {
        let state = vec![AbiStateField {
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
            felt_size: 256,
            visibility: FieldVisibility::Public,
        }];
        let types = vec![AbiStructType {
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
                visibility: FieldVisibility::Public,
            }],
        }];
        let state_layout = AbiStateLayout::from_state_fields(&state, &types).expect("sample layout");
        Abi {
            schema_version: "2.0.0".to_string(),
            contract: AbiContract {
                name: "Wallet".to_string(),
                state_tree_height: 7,
                state,
                state_layout,
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
            types,
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

#[cfg(test)]
mod type_spec_tests {
    use psy_ast::{ConstValue, Identifier, Location, PathNode, Program};

    use super::*;

    fn ident<F: Clone + From<u32>>(program: &mut Program<F>, name: &str) -> Identifier {
        Identifier::new(program.interner.intern_ident(name), Location::default())
    }

    #[test]
    fn from_unchecked_type_covers_every_classified_shape() {
        let mut program = Program::<u32>::new();
        let felt = ident(&mut program, "Felt");
        let index_map = ident(&mut program, "IndexMap");
        let owner = ident(&mut program, "Owner");
        let ctx = &mut DefaultVisitorContext::<u32, ()>::new(&mut program);

        // Basic identifiers map to their plain name.
        let basic = UncheckedType::Basic(felt);
        assert!(matches!(TypeAbiSpec::from_unchecked_type(&basic, ctx), TypeAbiSpec::Basic(name) if name == "Felt"));

        // Arrays record the innermost element type and their length.
        let array = UncheckedType::Array(Box::new(basic.clone()), ConstValue::U32(3), Location::default());
        match TypeAbiSpec::from_unchecked_type(&array, ctx) {
            TypeAbiSpec::Array { type_name, inner_type, length } => {
                assert_eq!((type_name.as_str(), inner_type.as_str(), length), ("Array", "Felt", 3));
            }
            other => panic!("expected array spec, got {other:?}"),
        }

        // Nested arrays keep the innermost element type with the outer length.
        let nested = UncheckedType::Array(Box::new(array), ConstValue::U32(5), Location::default());
        match TypeAbiSpec::from_unchecked_type(&nested, ctx) {
            TypeAbiSpec::Array { inner_type, length, .. } => {
                assert_eq!((inner_type.as_str(), length), ("Felt", 5));
            }
            other => panic!("expected nested array spec, got {other:?}"),
        }

        // Generic types surface only the outer name.
        let generic = UncheckedType::Generic(index_map, vec![basic], Location::default());
        assert!(matches!(TypeAbiSpec::from_unchecked_type(&generic, ctx), TypeAbiSpec::Basic(name) if name == "IndexMap"));

        // Paths classify by their target.
        let path = UncheckedType::Path(Box::new(PathNode::from_target(UncheckedType::Basic(owner))));
        assert!(matches!(TypeAbiSpec::from_unchecked_type(&path, ctx), TypeAbiSpec::Basic(name) if name == "Owner"));

        // Everything else falls back to "unknown".
        let tuple = UncheckedType::Tuple(vec![], Location::default());
        assert!(matches!(TypeAbiSpec::from_unchecked_type(&tuple, ctx), TypeAbiSpec::Basic(name) if name == "unknown"));
        let unknown = UncheckedType::Const(ConstValue::Bool(true), Location::default());
        assert!(matches!(TypeAbiSpec::from_unchecked_type(&unknown, ctx), TypeAbiSpec::Basic(name) if name == "unknown"));
        assert!(matches!(
            TypeAbiSpec::from_unchecked_type(&UncheckedType::Unknown, ctx),
            TypeAbiSpec::Basic(name) if name == "unknown"
        ));
    }
}
