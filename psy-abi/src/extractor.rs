use std::collections::{BTreeMap, HashMap, HashSet};

use psy_ast::{DefId, DefaultVisitorContext, FunctionNode, Program, StructNode, UncheckedType, Visibility, VisitorContext};

use crate::{
    Abi, AbiContract, AbiMethod, AbiParam, AbiStateField, AbiStructField, AbiStructType,
    AbiTypeKind, MapKind, PrimitiveTypeName, StateMutability, TypeRef,
};

#[derive(Clone)]
struct StructFieldLayout {
    pub offset: usize,
}

#[derive(Clone)]
struct StructLayout {
    felt_size: usize,
    fields: Vec<StructFieldLayout>,
}

pub struct AbiExtractor {
    pub contract_name: String,
}

impl AbiExtractor {
    pub fn new(contract_name: String) -> Self {
        Self { contract_name }
    }
    /// Extract the ABI from the same checked program.
    ///
    /// `state_tree_height` is computed once at the build step via
    /// [`compute_state_tree_height`](Self::compute_state_tree_height) and
    /// passed in here; `method_metadata` comes from the compiler pipeline.
    pub fn extract_abi<F: Clone + From<u32>>(
        &self,
        program: &mut Program<F>,
        state_tree_height: u16,
        method_metadata: &HashMap<String, (u32, bool)>,
    ) -> Result<Abi, psy_common::Error> {
        let ctx = DefaultVisitorContext::<F, ()>::new(program);
        let contract_struct = self.find_contract_struct(&ctx);
        let contract_name = contract_struct
            .map(|struct_node| ctx.ident(struct_node.name).0.to_string())
            .unwrap_or_else(|| self.contract_name.clone());

        let struct_nodes = self.collect_struct_nodes(&ctx);
        let struct_layouts = self.compute_struct_layouts(&ctx, &struct_nodes);

        // --- Build the type table (non-contract structs only) ---
        let mut types: Vec<AbiStructType> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (name, struct_node) in &struct_nodes {
            if self.is_internal_type(name) || *name == contract_name {
                continue;
            }
            if !seen.insert(name.clone()) {
                continue;
            }
            let layout = struct_layouts.get(name).cloned().unwrap_or(StructLayout {
                felt_size: 0,
                fields: vec![],
            });
            let fields = struct_node
                .fields
                .iter()
                .enumerate()
                .filter_map(|(field_idx, (fname, field))| {
                    if !Self::is_public(&field.visibility) {
                        return None;
                    }
                    let felt_size = self.felt_size_for_type(&ctx, &field.ty, &struct_nodes, &mut struct_layouts.clone());
                    Some(AbiStructField {
                        name: ctx.ident(*fname).0.to_string(),
                        ty: self.unchecked_type_to_typeref(&ctx, &field.ty),
                        offset_within_parent: layout.fields.get(field_idx).map(|f| f.offset).unwrap_or(0),
                        felt_size,
                    })
                })
                .collect();
            types.push(AbiStructType {
                kind: AbiTypeKind::Struct,
                name: name.clone(),
                felt_size: layout.felt_size,
                fields,
            });
        }

        // --- Build state layout ---
        // `total_virtual_felts` is a byproduct of the field walk; the authoritative
        // height is computed at the build step (see `compute_state_tree_height`).
        let (state, _total_virtual_felts) = contract_struct
            .map(|sn| self.extract_canonical_state(&ctx, sn, &struct_nodes, &struct_layouts))
            .unwrap_or_default();

        // --- Build methods ---
        let methods = self.extract_canonical_methods(&ctx, &contract_name, &struct_layouts, method_metadata);

        Ok(Abi {
            schema_version: "2.0.0".to_string(),
            contract: AbiContract {
                name: contract_name,
                state_tree_height,
                state,
                methods,
            },
            types,
        })
    }

    /// Compute the contract state-tree height from the storage layout.
    ///
    /// This mirrors the computation done inside
    /// [`extract_abi`](Self::extract_abi) so callers that need the height
    /// independently (e.g. to build a contract-code artifact) stay
    /// consistent with the emitted ABI.
    ///
    /// Each state-tree leaf packs four felts, so the height is
    /// `ceil(log2(ceil(total_felts / 4)))` (min 4), where `total_felts` is the
    /// contract's total state felt footprint. It is intentionally *not* derived
    /// from the compiled circuits' `state_commands`: those carry circuit
    /// wire ids, not slot magnitudes, and would silently underestimate
    /// contracts with large dynamically-indexed arrays (e.g. `[T;
    /// 16_777_216]`).
    pub fn compute_state_tree_height<F: Clone + From<u32>>(&self, program: &mut Program<F>) -> u16 {
        let ctx = DefaultVisitorContext::<F, ()>::new(program);
        let contract_struct = self.find_contract_struct(&ctx);
        let struct_nodes = self.collect_struct_nodes(&ctx);
        let struct_layouts = self.compute_struct_layouts(&ctx, &struct_nodes);
        let (_, total_virtual_felts) = contract_struct
            .map(|sn| self.extract_canonical_state(&ctx, sn, &struct_nodes, &struct_layouts))
            .unwrap_or_default();
        state_tree_height_for_total_felts(total_virtual_felts)
    }

    /// Convert an `UncheckedType` into a `TypeRef`.
    fn unchecked_type_to_typeref<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
    ) -> TypeRef {
        match ty {
            UncheckedType::Basic(identifier) => {
                let name = ctx.ident(*identifier).0.to_string();
                self.named_type_to_typeref(&name, ctx)
            }
            UncheckedType::Path(path) => self.unchecked_type_to_typeref(ctx, &path.target),
            UncheckedType::Array(inner, size, _) => {
                let struct_nodes = self.collect_struct_nodes(ctx);
                let mut layouts = self.compute_struct_layouts(ctx, &struct_nodes);
                let item = self.unchecked_type_to_typeref(ctx, inner);
                let item_felt_size = self.felt_size_for_type(ctx, inner, &struct_nodes, &mut layouts);
                TypeRef::Array {
                    item: Box::new(item),
                    length: size.as_u64().unwrap_or(0),
                    item_felt_size,
                }
            }
            UncheckedType::Generic(identifier, generics, _) => {
                let type_name = ctx.ident(*identifier).0.as_str();
                if let Some((map_kind, key, value, capacity)) =
                    self.extract_canonical_map_info(ctx, type_name, generics)
                {
                    let value_felt_size = self.felt_size_for_type(ctx, &generics[1], &self.collect_struct_nodes(ctx), &mut self.compute_struct_layouts(ctx, &self.collect_struct_nodes(ctx)));
                    return TypeRef::Map {
                        map_kind,
                        key: Box::new(key),
                        value: Box::new(value),
                        capacity,
                        value_felt_size,
                        alignment_felts: 4,
                    };
                }
                self.named_type_to_typeref(type_name, ctx)
            }
            _ => TypeRef::Primitive {
                name: PrimitiveTypeName::Felt,
            },
        }
    }

    /// Map a named type to a `TypeRef` — primitives, structs, or unknown (Felt fallback).
    fn named_type_to_typeref<F: Clone + From<u32>>(&self, name: &str, ctx: &DefaultVisitorContext<F, ()>) -> TypeRef {
        if let Some(target) = self.type_alias_target(ctx, name) {
            return self.unchecked_type_to_typeref(ctx, target);
        }

        match name {
            "Felt" => TypeRef::Primitive { name: PrimitiveTypeName::Felt },
            "Bool" | "bool" => TypeRef::Primitive { name: PrimitiveTypeName::Bool },
            "u32" => TypeRef::Primitive { name: PrimitiveTypeName::U32 },
            "Hash" => TypeRef::Primitive { name: PrimitiveTypeName::Hash },
            other => {
                // If it's a known struct, reference it
                TypeRef::Struct { name: other.to_string() }
            }
        }
    }

    /// Extract map info as `TypeRef`s.
    fn extract_canonical_map_info<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        type_name: &str,
        generics: &[UncheckedType],
    ) -> Option<(MapKind, TypeRef, TypeRef, usize)> {
        let map_kind = match type_name {
            "ContractHashMap" => MapKind::ContractHashMap,
            "Map" => MapKind::Map,
            "NamespacedMap" => MapKind::NamespacedMap,
            _ => return None,
        };
        if generics.len() < 3 {
            return None;
        }
        let key = self.unchecked_type_to_typeref(ctx, &generics[0]);
        let value = self.unchecked_type_to_typeref(ctx, &generics[1]);
        let capacity = self.const_len_from_type(&generics[2]).unwrap_or(0);
        Some((map_kind, key, value, capacity))
    }

    /// Build state fields from the contract struct.
    ///
    /// Returns `(fields, total_virtual_felts)`, where `total_virtual_felts` is
    /// the running offset accumulated over all state fields (including
    /// private fields and the IMT-map aligned region) — the contract's
    /// total felt footprint, used to size the state tree height.
    fn extract_canonical_state<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        contract_struct: &StructNode,
        struct_nodes: &BTreeMap<String, &StructNode>,
        struct_layouts: &HashMap<String, StructLayout>,
    ) -> (Vec<AbiStateField>, usize) {
        let mut offset = 0usize;
        let mut fields = Vec::new();

        for (field_name, field) in &contract_struct.fields {
            let ty = self.unchecked_type_to_typeref(ctx, &field.ty);
            let (felt_size, is_map) = self.canonical_felt_size_and_is_map(ctx, &field.ty, struct_nodes, struct_layouts);

            if !Self::is_public(&field.visibility) {
                if is_map {
                    let aligned_offset = (offset + 3) & !3;
                    offset = aligned_offset + felt_size;
                } else {
                    offset += felt_size;
                }
                continue;
            }

            let field_offset = if is_map {
                let aligned = (offset + 3) & !3;
                fields.push(AbiStateField {
                    name: ctx.ident(*field_name).0.to_string(),
                    ty,
                    offset: aligned,
                    felt_size,
                });
                aligned + felt_size
            } else {
                fields.push(AbiStateField {
                    name: ctx.ident(*field_name).0.to_string(),
                    ty,
                    offset,
                    felt_size,
                });
                offset + felt_size
            };
            offset = field_offset;
        }

        (fields, offset)
    }

    /// Compute felt_size for a type and whether it's a map (for alignment).
    fn canonical_felt_size_and_is_map<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &BTreeMap<String, &StructNode>,
        struct_layouts: &HashMap<String, StructLayout>,
    ) -> (usize, bool) {
        if self.extract_imt_map_info(ctx, ty).is_some() {
            let (_, _, capacity) = self.extract_imt_map_info(ctx, ty).unwrap();
            return (capacity.saturating_mul(4), true);
        }
        let mut layouts = struct_layouts.clone();
        let size = self.felt_size_for_type(ctx, ty, struct_nodes, &mut layouts);
        (size, false)
    }

    /// Extract methods from impl blocks, using method_metadata.
    fn extract_canonical_methods<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        contract_name: &str,
        struct_layouts: &HashMap<String, StructLayout>,
        method_metadata: &HashMap<String, (u32, bool)>,
    ) -> Vec<AbiMethod> {
        let mut methods = Vec::new();
        let struct_nodes = self.collect_struct_nodes(ctx);

        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if let Some(function) = ctx.definition(def_id).as_function() {
                let method_name = ctx.ident(function.name).0.to_string();
                let Some(&(method_id, is_view)) = method_metadata.get(&method_name) else {
                    continue;
                };
                methods.push(self.function_to_canonical_method(ctx, function, method_id, is_view, &struct_nodes, struct_layouts));
            } else if let Some(impl_node) = ctx.definition(def_id).as_impl() {
                let impl_type_name = self.extract_type_name(&impl_node.ty, ctx);
                if impl_type_name != contract_name && impl_type_name != format!("{}Ref", contract_name) {
                    continue;
                }
                for &function_def_id in &impl_node.body {
                    let Some(function) = ctx.definition(function_def_id).as_function() else {
                        continue;
                    };
                    let method_name = ctx.ident(function.name).0.to_string();
                    let Some(&(method_id, is_view)) = method_metadata.get(&method_name) else {
                        continue;
                    };
                    methods.push(self.function_to_canonical_method(ctx, function, method_id, is_view, &struct_nodes, struct_layouts));
                }
            }
        }

        methods.sort_by(|a, b| a.name.cmp(&b.name));
        methods.dedup_by(|a, b| a.name == b.name);
        methods
    }

    /// Convert a `FunctionNode` into a `AbiMethod`.
    fn function_to_canonical_method<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        function: &FunctionNode,
        method_id: u32,
        is_view: bool,
        struct_nodes: &BTreeMap<String, &StructNode>,
        struct_layouts: &HashMap<String, StructLayout>,
    ) -> AbiMethod {
        let inputs: Vec<AbiParam> = function
            .parameters
            .iter()
            .filter(|param| ctx.ident(param.name).0.as_str() != "self")
            .map(|param| {
                let felt_size = self.felt_size_for_type(ctx, &param.ty, struct_nodes, &mut struct_layouts.clone());
                AbiParam {
                    name: ctx.ident(param.name).0.to_string(),
                    ty: self.unchecked_type_to_typeref(ctx, &param.ty),
                    felt_size,
                }
            })
            .collect();

        let input_felt_count: usize = inputs.iter().map(|p| p.felt_size).sum();

        // Outputs — extract from the function's return_type.
        let outputs: Vec<AbiParam> = function.return_type.as_ref().map(|rt| {
            let felt_size = self.felt_size_for_type(ctx, rt, struct_nodes, &mut struct_layouts.clone());
            vec![AbiParam {
                name: "return".to_string(),
                ty: self.unchecked_type_to_typeref(ctx, rt),
                felt_size,
            }]
        }).unwrap_or_default();
        let output_felt_count: usize = outputs.iter().map(|p| p.felt_size).sum();

        AbiMethod {
            name: ctx.ident(function.name).0.to_string(),
            method_id,
            state_mutability: if is_view { StateMutability::View } else { StateMutability::External },
            inputs,
            outputs,
            input_felt_count,
            output_felt_count,
            vm_type: None,
        }
    }

    fn is_public(visibility: &Visibility) -> bool {
        matches!(visibility, Visibility::Public)
    }

    fn is_internal_type(&self, type_name: &str) -> bool {
        // Filter out internal types that shouldn't appear in the ABI
        // 1. Known internal types
        if matches!(type_name, "ContractMetadata" | "StorageRef") {
            return true;
        }

        // 2. Generated Ref types (e.g., ContractRef, OtherUserInfoRef)
        if type_name.ends_with("Ref") {
            return true;
        }

        false
    }

    fn is_internal_function(&self, function_name: &str) -> bool {
        // Filter out internal functions that shouldn't appear in the ABI
        matches!(function_name, "new" | "get" | "set")
    }

    fn has_contract_attr<F: Clone + From<u32>>(&self, struct_node: &StructNode, ctx: &DefaultVisitorContext<F, ()>) -> bool {
        struct_node.attrs.iter().any(|attr| {
            let attr_name = ctx.ident(attr.name).0.as_str();
            attr_name == "contract" || attr_name == "storage"
        })
    }

    fn extract_type_name<F: Clone + From<u32>>(&self, unchecked_type: &psy_ast::UncheckedType, ctx: &DefaultVisitorContext<F, ()>) -> String {
        match unchecked_type {
            psy_ast::UncheckedType::Basic(identifier) => ctx.ident(*identifier).0.to_string(),
            psy_ast::UncheckedType::Path(path) => self.extract_type_name(&path.target, ctx),
            _ => "unknown".to_string(),
        }
    }

    fn find_contract_struct<'a, F: Clone + From<u32>>(&self, ctx: &'a DefaultVisitorContext<F, ()>) -> Option<&'a StructNode> {
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            let Some(struct_node) = ctx.definition(def_id).as_struct() else {
                continue;
            };
            if self.has_contract_attr(struct_node, ctx) {
                return Some(struct_node);
            }
        }
        None
    }

    fn collect_struct_nodes<'a, F: Clone + From<u32>>(&self, ctx: &'a DefaultVisitorContext<F, ()>) -> BTreeMap<String, &'a StructNode> {
        let mut structs = BTreeMap::new();
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if let Some(struct_node) = ctx.definition(def_id).as_struct() {
                structs.insert(ctx.ident(struct_node.name).0.to_string(), struct_node);
            }
        }
        structs
    }

    fn type_alias_target<'a, F: Clone + From<u32>>(
        &self,
        ctx: &'a DefaultVisitorContext<F, ()>,
        name: &str,
    ) -> Option<&'a UncheckedType> {
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            let Some(alias) = ctx.definition(def_id).as_type_alias() else {
                continue;
            };
            if ctx.ident(alias.name.id).0 == name {
                return Some(&alias.ty);
            }
        }
        None
    }

    fn compute_struct_layouts<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        struct_nodes: &BTreeMap<String, &StructNode>,
    ) -> HashMap<String, StructLayout> {
        let mut layouts = HashMap::new();
        let names = struct_nodes.keys().cloned().collect::<Vec<_>>();
        for name in names {
            let _ = self.compute_struct_layout(ctx, &name, struct_nodes, &mut layouts);
        }
        layouts
    }

    fn compute_struct_layout<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        name: &str,
        struct_nodes: &BTreeMap<String, &StructNode>,
        layouts: &mut HashMap<String, StructLayout>,
    ) -> StructLayout {
        if let Some(layout) = layouts.get(name) {
            return layout.clone();
        }

        let Some(struct_node) = struct_nodes.get(name).copied() else {
            return StructLayout {
                felt_size: 0,
                fields: vec![],
            };
        };

        let mut offset = 0usize;
        let mut fields = Vec::new();
        for (_field_name, field) in &struct_node.fields {
            let felt_size = self.felt_size_for_type(ctx, &field.ty, struct_nodes, layouts);
            fields.push(StructFieldLayout {
                offset,
            });
            offset += felt_size;
        }

        let layout = StructLayout { felt_size: offset, fields };
        layouts.insert(name.to_string(), layout.clone());
        layout
    }

    fn felt_size_for_type<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &BTreeMap<String, &StructNode>,
        layouts: &mut HashMap<String, StructLayout>,
    ) -> usize {
        if self.extract_imt_map_info(ctx, ty).is_some() {
            return 0;
        }

        match ty {
            UncheckedType::Basic(identifier) => self.felt_size_for_named_type(ctx.ident(*identifier).0.as_str(), struct_nodes, layouts, ctx),
            UncheckedType::Path(path) => self.felt_size_for_type(ctx, &path.target, struct_nodes, layouts),
            UncheckedType::Array(inner, size, _) => self.felt_size_for_type(ctx, inner, struct_nodes, layouts).saturating_mul(size.as_u64().unwrap_or(0) as usize),
            UncheckedType::Tuple(items, _) => items.iter().map(|item| self.felt_size_for_type(ctx, item, struct_nodes, layouts)).sum(),
            UncheckedType::Generic(identifier, _, _) => self.felt_size_for_named_type(ctx.ident(*identifier).0.as_str(), struct_nodes, layouts, ctx),
            _ => 0,
        }
    }

    fn felt_size_for_named_type<F: Clone + From<u32>>(
        &self,
        type_name: &str,
        struct_nodes: &BTreeMap<String, &StructNode>,
        layouts: &mut HashMap<String, StructLayout>,
        ctx: &DefaultVisitorContext<F, ()>,
    ) -> usize {
        if let Some(target) = self.type_alias_target(ctx, type_name) {
            return self.felt_size_for_type(ctx, target, struct_nodes, layouts);
        }

        match type_name {
            "Felt" | "bool" | "Bool" | "u64" | "i64" | "u32" => 1,
            "u256" | "QHashOut" | "Hash" => 4,
            other if struct_nodes.contains_key(other) => self.compute_struct_layout(ctx, other, struct_nodes, layouts).felt_size,
            _ => 0,
        }
    }

    fn extract_imt_map_info<F: Clone + From<u32>>(&self, ctx: &DefaultVisitorContext<F, ()>, ty: &UncheckedType) -> Option<(String, String, usize)> {
        let UncheckedType::Generic(identifier, generics, _) = ty else {
            return None;
        };
        let type_name = ctx.ident(*identifier).0.as_str();
        if type_name != "Map" && type_name != "NamespacedMap" && type_name != "ContractHashMap" {
            return None;
        }
        if generics.len() < 3 {
            return None;
        }
        let key_type = self.stringify_unchecked_type(ctx, &generics[0]);
        let value_type = self.stringify_unchecked_type(ctx, &generics[1]);
        let capacity = self.const_len_from_type(&generics[2]).unwrap_or(0);
        Some((key_type, value_type, capacity))
    }

    fn const_len_from_type(&self, ty: &UncheckedType) -> Option<usize> {
        match ty {
            UncheckedType::Const(value, _) => match value {
                psy_ast::ConstValue::Felt(value) => usize::try_from(*value).ok(),
                psy_ast::ConstValue::U32(value) => Some(*value as usize),
                psy_ast::ConstValue::Bool(_) => None,
            },
            _ => None,
        }
    }

    fn stringify_unchecked_type<F: Clone + From<u32>>(&self, ctx: &DefaultVisitorContext<F, ()>, ty: &UncheckedType) -> String {
        match ty {
            UncheckedType::Basic(identifier) => ctx.ident(*identifier).0.to_string(),
            UncheckedType::Const(value, _) => match value {
                psy_ast::ConstValue::Felt(value) => value.to_string(),
                psy_ast::ConstValue::U32(value) => format!("{value}u32"),
                psy_ast::ConstValue::Bool(value) => value.to_string(),
            },
            UncheckedType::Generic(identifier, generics, _) => {
                let inner = generics
                    .iter()
                    .map(|generic| self.stringify_unchecked_type(ctx, generic))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{}>", ctx.ident(*identifier).0, inner)
            }
            UncheckedType::Array(inner, size, _) => format!("[{}; {}]", self.stringify_unchecked_type(ctx, inner), size),
            UncheckedType::Tuple(items, _) => {
                let inner = items
                    .iter()
                    .map(|item| self.stringify_unchecked_type(ctx, item))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({inner})")
            }
            UncheckedType::Path(path) => self.stringify_unchecked_type(ctx, &path.target),
            UncheckedType::TraitCast(inner, trait_ty, _) => format!(
                "{} as {}",
                self.stringify_unchecked_type(ctx, inner),
                self.stringify_unchecked_type(ctx, trait_ty)
            ),
            UncheckedType::FunctionSignature(_, _) => "fn".to_string(),
            UncheckedType::Unknown => "unknown".to_string(),
        }
    }
}

/// `ceil(log2(value))` for `value >= 1`; returns 0 for `value <= 1`.
fn ceil_log2(value: u64) -> u16 {
    if value <= 1 {
        return 0;
    }
    (u64::BITS - (value - 1).leading_zeros()) as u16
}

fn state_tree_height_for_total_felts(total_felts: usize) -> u16 {
    let total_leaves = total_felts.max(1).saturating_add(3) / 4;
    ceil_log2(total_leaves as u64).max(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use psy_ast::{ConstValue, Identifier, Location, PathNode};

    fn type_fixture() -> (Program<u32>, HashMap<&'static str, Identifier>) {
        let mut program = Program::new();
        let identifiers = ["Felt", "Bool", "u32", "Hash", "Widget", "Map", "NamespacedMap", "Other"]
            .into_iter()
            .map(|name| {
                let id = program.interner.intern_ident(name);
                (name, Identifier::new(id, Location::default()))
            })
            .collect();
        (program, identifiers)
    }

    #[test]
    fn state_tree_height_accounts_for_four_felts_per_leaf() {
        assert_eq!(state_tree_height_for_total_felts(0), 4);
        assert_eq!(state_tree_height_for_total_felts(64), 4);
        assert_eq!(state_tree_height_for_total_felts(65), 5);
        assert_eq!(state_tree_height_for_total_felts(128), 5);
        assert_eq!(state_tree_height_for_total_felts(129), 6);
    }

    #[test]
    fn ceil_log2_handles_zero_one_powers_and_u64_boundaries() {
        assert_eq!(ceil_log2(0), 0);
        assert_eq!(ceil_log2(1), 0);
        assert_eq!(ceil_log2(2), 1);
        assert_eq!(ceil_log2(3), 2);
        assert_eq!(ceil_log2(4), 2);
        assert_eq!(ceil_log2(5), 3);
        assert_eq!(ceil_log2(1u64 << 63), 63);
        assert_eq!(ceil_log2(u64::MAX), 64);
    }

    #[test]
    fn state_tree_height_saturates_large_felt_counts_without_overflow() {
        let height = state_tree_height_for_total_felts(usize::MAX);

        assert!(height >= 4);
        assert!(height <= u16::MAX);
    }

    #[test]
    fn state_tree_height_clamps_small_layouts_to_the_minimum_tree() {
        // 1..=16 felts all pack into at most 4 leaves (2^2), clamped to min 4.
        for felts in 1..=16 {
            assert_eq!(state_tree_height_for_total_felts(felts), 4, "felts = {felts}");
        }
        // 17 felts round up to 5 leaves, which needs height 3 — still clamped.
        assert_eq!(state_tree_height_for_total_felts(17), 4);
        // The clamp stops binding once 256 leaves (2^8) are exceeded: 1024 felts
        // are exactly 256 leaves (height 8), 1025 spill into 257 (height 9).
        assert_eq!(state_tree_height_for_total_felts(1024), 8);
        assert_eq!(state_tree_height_for_total_felts(1025), 9);
    }

    #[test]
    fn state_tree_height_saturates_instead_of_panicking_at_usize_max_neighbors() {
        // saturating_add(3) on usize::MAX must not wrap around to a small value.
        assert_eq!(state_tree_height_for_total_felts(usize::MAX), state_tree_height_for_total_felts(usize::MAX - 3));
        assert_eq!(state_tree_height_for_total_felts(usize::MAX - 2), state_tree_height_for_total_felts(usize::MAX));
    }

    #[test]
    fn test_abi_extractor_creation() {
        let extractor = AbiExtractor::new("TestContract".to_string());
        assert_eq!(extractor.contract_name, "TestContract");
    }

    #[test]
    fn test_internal_type_filtering() {
        let extractor = AbiExtractor::new("TestContract".to_string());

        // Test internal types
        assert!(extractor.is_internal_type("ContractMetadata"));
        assert!(extractor.is_internal_type("StorageRef"));
        assert!(extractor.is_internal_type("ContractRef"));
        assert!(extractor.is_internal_type("SomeStructRef"));

        // Test valid types
        assert!(!extractor.is_internal_type("Contract"));
        assert!(!extractor.is_internal_type("UserInfo"));
        assert!(!extractor.is_internal_type("Balance"));
    }

    #[test]
    fn test_internal_function_filtering() {
        let extractor = AbiExtractor::new("TestContract".to_string());

        // Test internal functions
        assert!(extractor.is_internal_function("new"));
        assert!(extractor.is_internal_function("get"));
        assert!(extractor.is_internal_function("set"));

        // Test valid functions
        assert!(!extractor.is_internal_function("transfer"));
        assert!(!extractor.is_internal_function("mint"));
        assert!(!extractor.is_internal_function("claim"));
    }

    #[test]
    fn type_refs_cover_primitives_structs_arrays_paths_and_maps() {
        let (mut program, identifiers) = type_fixture();
        let ctx = DefaultVisitorContext::<u32, ()>::new(&mut program);
        let extractor = AbiExtractor::new("Contract".into());
        let basic = |name| UncheckedType::Basic(identifiers[name]);

        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &basic("Felt")),
            TypeRef::Primitive { name: PrimitiveTypeName::Felt }
        );
        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &basic("Bool")),
            TypeRef::Primitive { name: PrimitiveTypeName::Bool }
        );
        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &basic("u32")),
            TypeRef::Primitive { name: PrimitiveTypeName::U32 }
        );
        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &basic("Hash")),
            TypeRef::Primitive { name: PrimitiveTypeName::Hash }
        );
        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &basic("Widget")),
            TypeRef::Struct { name: "Widget".into() }
        );

        let array = UncheckedType::Array(
            Box::new(basic("Hash")),
            ConstValue::U32(3),
            Location::default(),
        );
        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &array),
            TypeRef::Array {
                item: Box::new(TypeRef::Primitive { name: PrimitiveTypeName::Hash }),
                length: 3,
                item_felt_size: 4,
            }
        );

        let path = UncheckedType::Path(Box::new(PathNode::from_target_ty(basic("u32"))));
        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &path),
            TypeRef::Primitive { name: PrimitiveTypeName::U32 }
        );

        let map = UncheckedType::Generic(
            identifiers["Map"],
            vec![basic("Hash"), basic("Widget"), UncheckedType::Const(ConstValue::Felt(16), Location::default())],
            Location::default(),
        );
        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &map),
            TypeRef::Map {
                map_kind: MapKind::Map,
                key: Box::new(TypeRef::Primitive { name: PrimitiveTypeName::Hash }),
                value: Box::new(TypeRef::Struct { name: "Widget".into() }),
                capacity: 16,
                value_felt_size: 0,
                alignment_felts: 4,
            }
        );
    }

    #[test]
    fn map_and_type_string_helpers_cover_invalid_and_nested_shapes() {
        let (mut program, identifiers) = type_fixture();
        let ctx = DefaultVisitorContext::<u32, ()>::new(&mut program);
        let extractor = AbiExtractor::new("Contract".into());
        let basic = |name| UncheckedType::Basic(identifiers[name]);
        let constant = |value| UncheckedType::Const(value, Location::default());

        assert_eq!(extractor.const_len_from_type(&constant(ConstValue::Felt(9))), Some(9));
        assert_eq!(extractor.const_len_from_type(&constant(ConstValue::U32(7))), Some(7));
        assert_eq!(extractor.const_len_from_type(&constant(ConstValue::Bool(true))), None);
        assert_eq!(extractor.const_len_from_type(&UncheckedType::Unknown), None);

        assert!(extractor
            .extract_canonical_map_info(&ctx, "Other", &[])
            .is_none());
        assert!(extractor
            .extract_canonical_map_info(&ctx, "Map", &[basic("Felt")])
            .is_none());
        let namespaced = extractor
            .extract_canonical_map_info(
                &ctx,
                "NamespacedMap",
                &[basic("Felt"), basic("u32"), constant(ConstValue::Bool(false))],
            )
            .unwrap();
        assert_eq!(namespaced.0, MapKind::NamespacedMap);
        assert_eq!(namespaced.3, 0);

        let tuple = UncheckedType::Tuple(vec![basic("Felt"), basic("u32")], Location::default());
        let nested = UncheckedType::Generic(
            identifiers["Other"],
            vec![tuple.clone(), constant(ConstValue::U32(2))],
            Location::default(),
        );
        assert_eq!(extractor.stringify_unchecked_type(&ctx, &tuple), "(Felt, u32)");
        assert_eq!(extractor.stringify_unchecked_type(&ctx, &nested), "Other<(Felt, u32), 2u32>");
        assert_eq!(
            extractor.stringify_unchecked_type(
                &ctx,
                &UncheckedType::TraitCast(
                    Box::new(basic("Widget")),
                    Box::new(basic("Other")),
                    Location::default(),
                ),
            ),
            "Widget as Other"
        );
        assert_eq!(extractor.stringify_unchecked_type(&ctx, &UncheckedType::Unknown), "unknown");
    }

    #[test]
    fn empty_program_has_minimum_tree_height_and_fallback_contract_name() {
        let mut program = Program::<u32>::new();
        let extractor = AbiExtractor::new("Fallback".into());
        assert_eq!(extractor.compute_state_tree_height(&mut program), 4);
        let abi = extractor.extract_abi(&mut program, 4, &HashMap::new()).unwrap();
        assert_eq!(abi.contract.name, "Fallback");
        assert!(abi.contract.state.is_empty());
        assert!(abi.contract.methods.is_empty());
        assert!(abi.types.is_empty());
    }

}
