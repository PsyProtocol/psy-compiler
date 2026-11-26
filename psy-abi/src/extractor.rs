use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
};

use psy_ast::{
    AttrNode, ConstValue, DefId, DefaultVisitorContext, DefinitionNode, ExprNode, FunctionNode, Identifier, Program, StructField,
    StructNode, UncheckedType, ValueNode, Visibility, VisitorContext,
};

use crate::{
    field_visibility, Abi, AbiContract, AbiMethod, AbiParam, AbiStateField, AbiStateLayout, AbiStructField, AbiStructType, AbiTypeKind, MapKind,
    PrimitiveTypeName, StateMutability, TypeRef,
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
    pub fn extract_abi<F: Clone + From<u32> + Debug>(
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
                .map(|(field_idx, (fname, field))| {
                    let felt_size = self.state_felt_size_for_type(&ctx, &field.ty, &struct_nodes, &mut struct_layouts.clone());
                    AbiStructField {
                        name: ctx.ident(*fname).0.to_string(),
                        ty: self.unchecked_type_to_typeref(&ctx, &field.ty),
                        offset_within_parent: layout.fields.get(field_idx).map(|f| f.offset).unwrap_or(0),
                        felt_size,
                        visibility: field_visibility(&field.visibility),
                    }
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
        // The public `state` list stays filtered, but the canonical manifest keeps
        // every physical field, including private prefix, interleaved, and trailing
        // storage, plus map alignment and the recursive storage footprint.
        let (state, manifest, _total_virtual_felts) = contract_struct
            .map(|sn| self.extract_canonical_state(&ctx, sn, &struct_nodes, &struct_layouts))
            .unwrap_or_else(|| (Vec::new(), Vec::new(), 0));
        let state_layout = AbiStateLayout::from_state_fields(&manifest, &types).map_err(|error| psy_common::Error::Message(error.to_string()))?;

        // --- Build methods ---
        let methods = self.extract_canonical_methods(&ctx, &struct_layouts, method_metadata)?;

        Ok(Abi {
            schema_version: "2.0.0".to_string(),
            contract: AbiContract {
                name: contract_name,
                state_tree_height,
                state,
                state_layout,
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
    pub fn compute_state_tree_height<F: Clone + From<u32> + Debug>(&self, program: &mut Program<F>) -> u16 {
        let ctx = DefaultVisitorContext::<F, ()>::new(program);
        let contract_struct = self.find_contract_struct(&ctx);
        let struct_nodes = self.collect_struct_nodes(&ctx);
        let struct_layouts = self.compute_struct_layouts(&ctx, &struct_nodes);
        let (_, _, total_virtual_felts) = contract_struct
            .map(|sn| self.extract_canonical_state(&ctx, sn, &struct_nodes, &struct_layouts))
            .unwrap_or_else(|| (Vec::new(), Vec::new(), 0));
        state_tree_height_for_total_felts(total_virtual_felts)
    }

    /// Convert an `UncheckedType` into a `TypeRef`.
    fn unchecked_type_to_typeref<F: Clone + From<u32> + Debug>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
    ) -> TypeRef {
        match ty {
            UncheckedType::Basic(identifier) => {
                let name = ctx.ident(*identifier).0.to_string();
                self.named_type_to_typeref(&name, ctx)
            }
            UncheckedType::Path(path) => match self.resolved_path_type(ctx, path) {
                Some(resolved) => self.unchecked_type_to_typeref(ctx, resolved),
                None => TypeRef::Struct { name: "unresolved".into() },
            },
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
    fn named_type_to_typeref<F: Clone + From<u32> + Debug>(&self, name: &str, ctx: &DefaultVisitorContext<F, ()>) -> TypeRef {
        if let Some(target) = self.type_alias_target(ctx, name) {
            return self.unchecked_type_to_typeref(ctx, target);
        }

        match name {
            "Felt" => TypeRef::Primitive { name: PrimitiveTypeName::Felt },
            "Bool" | "bool" => TypeRef::Primitive { name: PrimitiveTypeName::Bool },
            "u32" => TypeRef::Primitive { name: PrimitiveTypeName::U32 },
            "u64" => TypeRef::Primitive { name: PrimitiveTypeName::U64 },
            "u128" => TypeRef::Primitive { name: PrimitiveTypeName::U128 },
            "u256" => TypeRef::Primitive { name: PrimitiveTypeName::U256 },
            "Hash" => TypeRef::Primitive { name: PrimitiveTypeName::Hash },
            other => {
                // If it's a known struct, reference it
                TypeRef::Struct { name: other.to_string() }
            }
        }
    }

    /// Extract map info as `TypeRef`s.
    fn extract_canonical_map_info<F: Clone + From<u32> + Debug>(
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
        let capacity = self.const_len_from_type(ctx, &generics[2]).unwrap_or(0);
        Some((map_kind, key, value, capacity))
    }

    /// Build state fields from the contract struct.
    ///
    /// Returns `(public_fields, manifest_fields, total_virtual_felts)`.
    /// `manifest_fields` lists every physical field; `public_fields` is that
    /// list with private fields removed. `total_virtual_felts` is the running
    /// offset accumulated over all state fields (including private fields and
    /// the IMT-map aligned region) — the contract's total felt footprint,
    /// used to size the state tree height.
    fn extract_canonical_state<F: Clone + From<u32> + Debug>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        contract_struct: &StructNode,
        struct_nodes: &BTreeMap<String, &StructNode>,
        struct_layouts: &HashMap<String, StructLayout>,
    ) -> (Vec<AbiStateField>, Vec<AbiStateField>, usize) {
        let mut offset = 0usize;
        let mut public_fields = Vec::new();
        let mut manifest_fields = Vec::new();

        for (field_name, field) in &contract_struct.fields {
            let ty = self.unchecked_type_to_typeref(ctx, &field.ty);
            let (felt_size, is_map) = self.canonical_felt_size_and_is_map(ctx, &field.ty, struct_nodes, struct_layouts);
            let aligned = if is_map { (offset + 3) & !3 } else { offset };
            let state_field = AbiStateField {
                name: ctx.ident(*field_name).0.to_string(),
                ty,
                offset: aligned,
                felt_size,
                visibility: field_visibility(&field.visibility),
            };
            manifest_fields.push(state_field.clone());
            if Self::is_public(&field.visibility) {
                public_fields.push(state_field);
            }
            offset = aligned + felt_size;
        }

        (public_fields, manifest_fields, offset)
    }

    fn extract_canonical_methods<F: Clone + From<u32> + Debug>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        struct_layouts: &HashMap<String, StructLayout>,
        method_metadata: &HashMap<String, (u32, bool)>,
    ) -> Result<Vec<AbiMethod>, psy_common::Error> {
        let struct_nodes = self.collect_struct_nodes(ctx);
        let mut methods = Vec::with_capacity(method_metadata.len());
        for &definition in &ctx.program().compiled_method_definitions {
            let function = ctx.definition(definition).as_function()
                .ok_or_else(|| psy_common::Error::Message("compiled method declaration is not a function".into()))?;
            let name = ctx.ident(function.name);
            if let Some(&(method_id, is_view)) = method_metadata.get(name.0.as_str()) {
                methods.push(self.function_to_canonical_method(ctx, function, method_id, is_view, &struct_nodes, struct_layouts));
            }
        }
        if methods.len() != method_metadata.len() {
            return Err(psy_common::Error::Message("ABI method metadata does not match compiled declarations".into()));
        }
        methods.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(methods)
    }

    /// Convert a `FunctionNode` into a `AbiMethod`.
    fn function_to_canonical_method<F: Clone + From<u32> + Debug>(
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

    fn has_contract_attr<F: Clone + From<u32> + Debug>(&self, struct_node: &StructNode, ctx: &DefaultVisitorContext<F, ()>) -> bool {
        struct_node.attrs.iter().any(|attr| {
            let attr_name = ctx.ident(attr.name).0.as_str();
            attr_name == "contract" || attr_name == "storage"
        })
    }

    fn find_contract_struct<'a, F: Clone + From<u32> + Debug>(&self, ctx: &'a DefaultVisitorContext<F, ()>) -> Option<&'a StructNode> {
        if let Some(id) = ctx.program().compiled_contract_definition {
            return ctx.definition(id).as_struct();
        }
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            let Some(struct_node) = ctx.definition(def_id).as_struct() else {
                continue;
            };
            if ctx.ident(struct_node.name).0 == self.contract_name && self.has_contract_attr(struct_node, ctx) {
                return Some(struct_node);
            }
        }
        None
    }

    fn collect_struct_nodes<'a, F: Clone + From<u32> + Debug>(&self, ctx: &'a DefaultVisitorContext<F, ()>) -> BTreeMap<String, &'a StructNode> {
        let mut structs = BTreeMap::new();
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if let Some(struct_node) = ctx.definition(def_id).as_struct() {
                structs.insert(ctx.ident(struct_node.name).0.to_string(), struct_node);
            }
        }
        structs
    }

    fn type_alias_target<'a, F: Clone + From<u32> + Debug>(
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

    /// A rooted path denotes its associated type. A path with no root keeps
    /// its target. `None` is a missing or ambiguous member.
    pub(crate) fn resolved_path_type<'a, F: Clone + From<u32> + Debug>(
        &self,
        ctx: &'a DefaultVisitorContext<F, ()>,
        path: &'a psy_ast::PathNode,
    ) -> Option<&'a UncheckedType> {
        let Some(root) = path.root.as_ref() else {
            return Some(&path.target);
        };
        let mut seen = HashSet::new();
        seen.insert(format!("{root:?}"));
        let mut current = root;
        for member in path.segments.iter().chain(std::iter::once(&path.target)) {
            let member_name = member.as_basic().map(|ident| ctx.ident(ident.id).0.to_string())?;
            current = self.associated_type_of(ctx, current, &member_name)?;
            if !seen.insert(format!("{current:?}")) {
                return None;
            }
        }
        Some(current)
    }


    fn associated_type_of<'a, F: Clone + From<u32> + Debug>(
        &self,
        ctx: &'a DefaultVisitorContext<F, ()>,
        owner: &UncheckedType,
        member: &str,
    ) -> Option<&'a UncheckedType> {
        let owner_name = match owner {
            UncheckedType::Basic(ident) => ctx.ident(ident.id).0.to_string(),
            _ => return None,
        };
        let mut found = None;
        for index in 0..ctx.program().defs.len() {
            let (implementor, associated) = match ctx.definition(DefId::from(index)) {
                DefinitionNode::Impl(node) => (&node.ty, &node.associated_types),
                DefinitionNode::TraitImpl(node) => (&node.ty, &node.associated_types),
                _ => continue,
            };
            let UncheckedType::Basic(ident) = implementor else {
                continue;
            };
            let implementor_name = ctx.ident(ident.id).0.as_str();
            if implementor_name != owner_name {
                continue;
            }
            let Some((_, value)) = associated.iter().find(|(name, _)| ctx.ident(name.id).0 == member) else {
                continue;
            };
            if found.replace(&value.ty).is_some() {
                return None;
            }
        }
        found
    }


    fn compute_struct_layouts<F: Clone + From<u32> + Debug>(
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

    fn compute_struct_layout<F: Clone + From<u32> + Debug>(
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
        let mut visiting = HashSet::new();
        visiting.insert(name.to_string());
        for (_field_name, field) in &struct_node.fields {
            if self.type_is_map_bearing(ctx, &field.ty, struct_nodes, &mut visiting) {
                offset += (4 - (offset % 4)) % 4;
            }
            let footprint = self.state_felt_size_for_type(ctx, &field.ty, struct_nodes, layouts);
            fields.push(StructFieldLayout { offset });
            offset += footprint;
        }

        let layout = StructLayout { felt_size: offset, fields };
        layouts.insert(name.to_string(), layout.clone());
        layout
    }

    /// Compute felt_size for a type and whether it's map-bearing (for alignment).
    ///
    /// A type is map-bearing if it is or contains a map; its state-layout
    /// footprint counts the map's `(capacity + 1) * 4` felts (sentinel +
    /// capacity leaves) instead of the 0 felts a map occupies as a value.
    fn canonical_felt_size_and_is_map<F: Clone + From<u32> + Debug>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &BTreeMap<String, &StructNode>,
        struct_layouts: &HashMap<String, StructLayout>,
    ) -> (usize, bool) {
        if let Some((_, _, capacity)) = self.extract_imt_map_info(ctx, ty) {
            return (capacity.saturating_add(1).saturating_mul(4), true);
        }
        let mut layouts = struct_layouts.clone();
        let mut visiting = HashSet::new();
        if self.type_is_map_bearing(ctx, ty, struct_nodes, &mut visiting) {
            let footprint = self.state_felt_size_for_type(ctx, ty, struct_nodes, &mut layouts);
            return (footprint, true);
        }
        let size = self.felt_size_for_type(ctx, ty, struct_nodes, &mut layouts);
        (size, false)
    }

    /// Whether a type is or contains a map (`Map`/`NamespacedMap`/
    /// `ContractHashMap`, their `Ref` variants, `StorageRef`/arrays of
    /// map-bearing items, or a struct/alias resolving to any of these).
    fn type_is_map_bearing<F: Clone + From<u32> + Debug>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &BTreeMap<String, &StructNode>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        if self.extract_imt_map_info(ctx, ty).is_some() {
            return true;
        }
        match ty {
            UncheckedType::Generic(identifier, params, _) => {
                let type_name = ctx.ident(*identifier).0.as_str();
                match type_name {
                    "MapRef" | "NamespacedMapRef" => params.len() == 3,
                    "StorageRef" => params.len() == 1 && self.type_is_map_bearing(ctx, &params[0], struct_nodes, visiting),
                    "ArrayRef" => params.len() == 2 && self.type_is_map_bearing(ctx, &params[0], struct_nodes, visiting),
                    _ => params.iter().any(|param| self.type_is_map_bearing(ctx, param, struct_nodes, visiting)),
                }
            }
            UncheckedType::Array(inner, _, _) => self.type_is_map_bearing(ctx, inner, struct_nodes, visiting),
            UncheckedType::Path(path) => self
                .resolved_path_type(ctx, path)
                .is_some_and(|resolved| self.type_is_map_bearing(ctx, resolved, struct_nodes, visiting)),
            UncheckedType::Basic(identifier) => {
                let name = ctx.ident(*identifier).0.to_string();
                if let Some(target) = self.type_alias_target(ctx, &name) {
                    return self.type_is_map_bearing(ctx, target, struct_nodes, visiting);
                }
                let Some(struct_node) = struct_nodes.get(&name) else {
                    return false;
                };
                if !visiting.insert(name.clone()) {
                    return false;
                }
                let bearing = struct_node.fields.values().any(|field| self.type_is_map_bearing(ctx, &field.ty, struct_nodes, visiting));
                visiting.remove(&name);
                bearing
            }
            _ => false,
        }
    }

    /// State-layout footprint of a type: like `felt_size_for_type`, but maps
    /// (and map refs) occupy `(capacity + 1) * 4` felts instead of 0, so
    /// composites containing maps fold to their full storage footprint.
    fn state_felt_size_for_type<F: Clone + From<u32> + Debug>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &BTreeMap<String, &StructNode>,
        layouts: &mut HashMap<String, StructLayout>,
    ) -> usize {
        if let Some((_, _, capacity)) = self.extract_imt_map_info(ctx, ty) {
            return (capacity + 1) * 4;
        }
        match ty {
            UncheckedType::Generic(identifier, params, _) => {
                let type_name = ctx.ident(*identifier).0.as_str();
                match type_name {
                    "MapRef" | "NamespacedMapRef" if params.len() == 3 => {
                        let capacity = self.const_len_from_type(ctx, &params[2]).unwrap_or(0);
                        (capacity + 1) * 4
                    }
                    "StorageRef" if params.len() == 1 => self.state_felt_size_for_type(ctx, &params[0], struct_nodes, layouts),
                    "ArrayRef" if params.len() == 2 => {
                        let item = self.state_felt_size_for_type(ctx, &params[0], struct_nodes, layouts);
                        let length = self.const_len_from_type(ctx, &params[1]).unwrap_or(0);
                        item.saturating_mul(length)
                    }
                    _ => self.felt_size_for_type(ctx, ty, struct_nodes, layouts),
                }
            }
            UncheckedType::Array(inner, size, _) => {
                let item = self.state_felt_size_for_type(ctx, inner, struct_nodes, layouts);
                item.saturating_mul(size.as_u64().unwrap_or(0) as usize)
            }
            UncheckedType::Path(path) => match self.resolved_path_type(ctx, path) {
                Some(resolved) => self.state_felt_size_for_type(ctx, resolved, struct_nodes, layouts),
                None => 0,
            },
            UncheckedType::Basic(identifier) => {
                let name = ctx.ident(*identifier).0.to_string();
                if let Some(target) = self.type_alias_target(ctx, &name) {
                    return self.state_felt_size_for_type(ctx, target, struct_nodes, layouts);
                }
                self.felt_size_for_named_type(&name, struct_nodes, layouts, ctx)
            }
            _ => self.felt_size_for_type(ctx, ty, struct_nodes, layouts),
        }
    }

    fn felt_size_for_named_type<F: Clone + From<u32> + Debug>(
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
            "Felt" | "bool" | "Bool" | "i64" | "u32" => 1,
            "u64" => 2,
            "u128" => 4,
            "u256" => 8,
            "QHashOut" | "Hash" => 4,
            other if struct_nodes.contains_key(other) => self.compute_struct_layout(ctx, other, struct_nodes, layouts).felt_size,
            _ => 0,
        }
    }

    fn felt_size_for_type<F: Clone + From<u32> + Debug>(
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
            UncheckedType::Path(path) => match self.resolved_path_type(ctx, path) {
                Some(resolved) => self.felt_size_for_type(ctx, resolved, struct_nodes, layouts),
                None => 0,
            },
            UncheckedType::Array(inner, size, _) => self.felt_size_for_type(ctx, inner, struct_nodes, layouts).saturating_mul(size.as_u64().unwrap_or(0) as usize),
            UncheckedType::Tuple(items, _) => items.iter().map(|item| self.felt_size_for_type(ctx, item, struct_nodes, layouts)).sum(),
            UncheckedType::Generic(identifier, _, _) => self.felt_size_for_named_type(ctx.ident(*identifier).0.as_str(), struct_nodes, layouts, ctx),
            _ => 0,
        }
    }

    fn extract_imt_map_info<F: Clone + From<u32> + Debug>(&self, ctx: &DefaultVisitorContext<F, ()>, ty: &UncheckedType) -> Option<(String, String, usize)> {
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
        let capacity = self.const_len_from_type(ctx, &generics[2]).unwrap_or(0);
        Some((key_type, value_type, capacity))
    }

    /// A concrete length: a literal, or a `const` whose value is a literal
    /// integer. A name whose value is not a literal is not a length.
    fn const_len_from_type<F: Clone + From<u32> + Debug>(&self, ctx: &DefaultVisitorContext<F, ()>, ty: &UncheckedType) -> Option<usize> {
        self.resolved_const_len(ctx, ty, &mut HashSet::new())
    }

    fn resolved_const_len<F: Clone + From<u32> + Debug>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        seen: &mut HashSet<String>,
    ) -> Option<usize> {
        match ty {
            UncheckedType::Const(value, _) => value.as_u64().and_then(|value| usize::try_from(value).ok()),
            UncheckedType::Basic(identifier) => {
                let name = ctx.ident(*identifier).0.to_string();
                if !seen.insert(name.clone()) {
                    return None;
                }
                let value = (0..ctx.program().defs.len()).find_map(|index| {
                    let def_id = DefId::from(index);
                    match ctx.definition(def_id).as_const() {
                        Some(item) if ctx.ident(item.name).0 == name => Some(item.value),
                        _ => None,
                    }
                });
                let value = value?;
                let numeric = match ctx.expression(value) {
                    ExprNode::Value(ValueNode::Uint(limbs, _, _)) => Some(limbs[0] as u64 | (limbs[1] as u64) << 32),
                    ExprNode::Value(ValueNode::Felt(felt, _)) => literal_u64(felt),
                    _ => None,
                }?;
                usize::try_from(numeric).ok()
            }
            _ => None,
        }
    }


    fn stringify_unchecked_type<F: Clone + From<u32> + Debug>(&self, ctx: &DefaultVisitorContext<F, ()>, ty: &UncheckedType) -> String {
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
            UncheckedType::Path(path) => match self.resolved_path_type(ctx, path) {
                Some(resolved) => self.stringify_unchecked_type(ctx, resolved),
                None => "unresolved".to_string(),
            },
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

fn literal_u64<F: Clone + From<u32> + Debug>(felt: &F) -> Option<u64> {
    // `u32` prints as its value. A real felt prints a bare integer for `Felt`
    // and `<n>u32` for `u32`; anything else is not a literal capacity.
    let raw = format!("{felt:?}");
    let digits = raw.strip_suffix("u32").unwrap_or(raw.as_str());
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
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
    use psy_ast::{AttrNode, ConstValue, DefinitionNode, Identifier, Location, PathNode, StructField, StructNode, Visibility};

    fn type_fixture() -> (Program<u32>, HashMap<&'static str, Identifier>) {
        let mut program = Program::new();
        let identifiers = ["Felt", "Bool", "u32", "u64", "u128", "u256", "Hash", "Widget", "Map", "NamespacedMap", "Other"]
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
    fn map_layout_reserves_sentinel_and_capacity_before_next_field() {
        let (mut program, identifiers) = type_fixture();
        let mut ctx = DefaultVisitorContext::<u32, ()>::new(&mut program);
        let extractor = AbiExtractor::new("Contract".into());
        let map = UncheckedType::Generic(
            identifiers["Map"],
            vec![
                UncheckedType::Basic(identifiers["Felt"]),
                UncheckedType::Basic(identifiers["Felt"]),
                UncheckedType::Const(ConstValue::U32(4), Location::default()),
            ],
            Location::default(),
        );
        let next = UncheckedType::Basic(identifiers["Felt"]);
        let balances_name = Identifier::new(ctx.intern("balances"), Location::default());
        let tail_name = Identifier::new(ctx.intern("tail"), Location::default());
        let field = |name, ty| {
            (
                name,
                StructField {
                    ty,
                    attrs: Vec::new(),
                    visibility: Visibility::Public,
                    comments: Vec::new(),
                    location: Location::default(),
                },
            )
        };
        let contract = StructNode {
            name: identifiers["Widget"],
            generic_parameters: Vec::new(),
            fields: [field(balances_name, map), field(tail_name, next)].into_iter().collect(),
            attrs: Vec::new(),
            visibility: Visibility::Public,
            comments: Vec::new(),
            location: Location::default(),
            is_generated: false,
        };

        let (state, manifest, total) = extractor.extract_canonical_state(&ctx, &contract, &BTreeMap::new(), &HashMap::new());

        assert_eq!(state[0].offset, 0);
        assert!(matches!(state[0].ty, TypeRef::Map { capacity: 4, .. }));
        assert_eq!(state[0].felt_size, 20);
        assert_eq!(state[1].offset, 20);
        assert_eq!(state[1].felt_size, 1);
        assert_eq!(manifest.len(), state.len());
        assert_eq!(total, 21);
    }



    #[test]
    fn map_bearing_fields_are_aligned_in_canonical_state_and_struct_layout() {
        let (mut program, identifiers) = type_fixture();
        let mut ctx = DefaultVisitorContext::<u32, ()>::new(&mut program);
        let extractor = AbiExtractor::new("Contract".into());
        let map = UncheckedType::Generic(
            identifiers["Map"],
            vec![
                UncheckedType::Basic(identifiers["Felt"]),
                UncheckedType::Basic(identifiers["Felt"]),
                UncheckedType::Const(ConstValue::U32(4), Location::default()),
            ],
            Location::default(),
        );
        let felt = UncheckedType::Basic(identifiers["Felt"]);
        let prefix_name = Identifier::new(ctx.intern("prefix"), Location::default());
        let balances_name = Identifier::new(ctx.intern("balances"), Location::default());
        let tail_name = Identifier::new(ctx.intern("tail"), Location::default());
        let field = |name, ty| {
            (
                name,
                StructField {
                    ty,
                    attrs: Vec::new(),
                    visibility: Visibility::Public,
                    comments: Vec::new(),
                    location: Location::default(),
                },
            )
        };
        let contract = StructNode {
            name: identifiers["Widget"],
            generic_parameters: Vec::new(),
            fields: [field(prefix_name, felt.clone()), field(balances_name, map), field(tail_name, felt)]
                .into_iter()
                .collect(),
            attrs: Vec::new(),
            visibility: Visibility::Public,
            comments: Vec::new(),
            location: Location::default(),
            is_generated: false,
        };

        let (state, manifest, total) = extractor.extract_canonical_state(&ctx, &contract, &BTreeMap::new(), &HashMap::new());

        assert_eq!(state[0].offset, 0);
        assert_eq!(state[0].felt_size, 1);
        assert_eq!(state[1].offset, 4, "map-bearing field aligned to a multiple of 4");
        assert_eq!(state[1].felt_size, 20);
        assert_eq!(state[2].offset, 24);
        assert_eq!(state[2].felt_size, 1);
        assert_eq!(manifest, state);
        assert_eq!(total, 25);

        let struct_nodes = BTreeMap::from([("Widget".to_string(), &contract)]);
        let layout = extractor.compute_struct_layout(&ctx, "Widget", &struct_nodes, &mut HashMap::new());
        assert_eq!(layout.felt_size, 25);
        assert_eq!(layout.fields[0].offset, 0);
        assert_eq!(layout.fields[1].offset, 4);
        assert_eq!(layout.fields[2].offset, 24);
    }

    #[test]
    fn capacity_128_map_inside_struct_yields_tree_height_8() {
        let mut program = Program::<u32>::new();
        let felt_id = program.interner.intern_ident("Felt");
        let map_id = program.interner.intern_ident("Map");
        let inner_id = program.interner.intern_ident("Inner");
        let wrapped_id = program.interner.intern_ident("Wrapped");
        let contract_attr_id = program.interner.intern_ident("contract");
        let balances_id = program.interner.intern_ident("balances");
        let holder_id = program.interner.intern_ident("holder");
        let tail_id = program.interner.intern_ident("tail");
        let ident = |id| Identifier::new(id, Location::default());
        let field = |name_id, ty| {
            (
                Identifier::new(name_id, Location::default()),
                StructField {
                    ty,
                    attrs: Vec::new(),
                    visibility: Visibility::Public,
                    comments: Vec::new(),
                    location: Location::default(),
                },
            )
        };
        let map128 = UncheckedType::Generic(
            ident(map_id),
            vec![
                UncheckedType::Basic(ident(felt_id)),
                UncheckedType::Basic(ident(felt_id)),
                UncheckedType::Const(ConstValue::U32(128), Location::default()),
            ],
            Location::default(),
        );
        let inner = StructNode {
            name: ident(inner_id),
            generic_parameters: Vec::new(),
            fields: [field(balances_id, map128)].into_iter().collect(),
            attrs: Vec::new(),
            visibility: Visibility::Public,
            comments: Vec::new(),
            location: Location::default(),
            is_generated: false,
        };
        let contract = StructNode {
            name: ident(wrapped_id),
            generic_parameters: Vec::new(),
            fields: [
                field(holder_id, UncheckedType::Basic(ident(inner_id))),
                field(tail_id, UncheckedType::Basic(ident(felt_id))),
            ]
            .into_iter()
            .collect(),
            attrs: vec![AttrNode {
                path: Vec::new(),
                name: ident(contract_attr_id),
                properties: Vec::new(),
                location: Location::default(),
            }],
            visibility: Visibility::Public,
            comments: Vec::new(),
            location: Location::default(),
            is_generated: false,
        };
        program.defs.alloc_item(DefinitionNode::Struct(inner));
        program.defs.alloc_item(DefinitionNode::Struct(contract));

        let mut extractor = AbiExtractor::new("Wrapped".into());
        assert_eq!(extractor.compute_state_tree_height(&mut program), 8);
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
            extractor.unchecked_type_to_typeref(&ctx, &basic("u64")),
            TypeRef::Primitive { name: PrimitiveTypeName::U64 }
        );
        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &basic("u128")),
            TypeRef::Primitive { name: PrimitiveTypeName::U128 }
        );
        assert_eq!(
            extractor.unchecked_type_to_typeref(&ctx, &basic("u256")),
            TypeRef::Primitive { name: PrimitiveTypeName::U256 }
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

    /// The wire-size table is the ABI's storage-layout contract: one felt per
    /// 32 bits, with u32 staying exactly one felt.
    #[test]
    fn uint_felt_sizes_scale_with_width() {
        let (mut program, _identifiers) = type_fixture();
        let ctx = DefaultVisitorContext::<u32, ()>::new(&mut program);
        let extractor = AbiExtractor::new("Contract".into());
        let no_structs = std::collections::BTreeMap::new();
        let mut layouts = std::collections::HashMap::new();
        for (name, size) in [("u32", 1usize), ("u64", 2), ("u128", 4), ("u256", 8)] {
            assert_eq!(
                extractor.felt_size_for_named_type(name, &no_structs, &mut layouts, &ctx),
                size,
                "{name} must occupy {size} felts"
            );
        }
    }

    #[test]
    fn map_and_type_string_helpers_cover_invalid_and_nested_shapes() {
        let (mut program, identifiers) = type_fixture();
        let ctx = DefaultVisitorContext::<u32, ()>::new(&mut program);
        let extractor = AbiExtractor::new("Contract".into());
        let basic = |name| UncheckedType::Basic(identifiers[name]);
        let constant = |value| UncheckedType::Const(value, Location::default());

        assert_eq!(extractor.const_len_from_type(&ctx, &constant(ConstValue::Felt(9))), Some(9));
        assert_eq!(extractor.const_len_from_type(&ctx, &constant(ConstValue::U32(7))), Some(7));
        assert_eq!(extractor.const_len_from_type(&ctx, &constant(ConstValue::Bool(true))), None);
        assert_eq!(extractor.const_len_from_type(&ctx, &UncheckedType::Unknown), None);

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

    fn program_with_contract(program: &mut Program<u32>, fields: Vec<(Identifier, StructField)>, extras: Vec<DefinitionNode>) -> Abi {
        let contract_name = Identifier::new(program.interner.intern_ident("Holder"), Location::default());
        let contract_attr = Identifier::new(program.interner.intern_ident("contract"), Location::default());
        for extra in extras {
            program.defs.alloc_item(extra);
        }
        program.defs.alloc_item(DefinitionNode::Struct(StructNode {
            name: contract_name,
            generic_parameters: Vec::new(),
            fields: fields.into_iter().collect(),
            attrs: vec![AttrNode {
                path: Vec::new(),
                name: contract_attr,
                properties: Vec::new(),
                location: Location::default(),
            }],
            visibility: Visibility::Public,
            comments: Vec::new(),
            location: Location::default(),
            is_generated: false,
        }));
        let extractor = AbiExtractor::new("Holder".into());
        let height = extractor.compute_state_tree_height(program);
        extractor.extract_abi(program, height, &HashMap::new()).expect("extract abi")
    }

    fn named_field(program: &mut Program<u32>, name: &str, ty: UncheckedType, visibility: Visibility) -> (Identifier, StructField) {
        (
            Identifier::new(program.interner.intern_ident(name), Location::default()),
            StructField {
                ty,
                attrs: Vec::new(),
                visibility,
                comments: Vec::new(),
                location: Location::default(),
            },
        )
    }

    fn felt_ty(program: &mut Program<u32>) -> UncheckedType {
        UncheckedType::Basic(Identifier::new(program.interner.intern_ident("Felt"), Location::default()))
    }

    fn map_ty(program: &mut Program<u32>, capacity: u32) -> UncheckedType {
        let felt = Identifier::new(program.interner.intern_ident("Felt"), Location::default());
        let map = Identifier::new(program.interner.intern_ident("Map"), Location::default());
        UncheckedType::Generic(
            map,
            vec![
                UncheckedType::Basic(felt),
                UncheckedType::Basic(felt),
                UncheckedType::Const(ConstValue::U32(capacity), Location::default()),
            ],
            Location::default(),
        )
    }

    #[test]
    fn private_prefix_public_scalar_manifest_keeps_the_private_range() {
        let mut program = Program::<u32>::new();
        let felt = felt_ty(&mut program);
        let hidden = named_field(&mut program, "hidden", felt.clone(), Visibility::Private);
        let shown = named_field(&mut program, "shown", felt, Visibility::Public);
        let abi = program_with_contract(&mut program, vec![hidden, shown], Vec::new());

        assert_eq!(abi.contract.state.len(), 1);
        assert_eq!(abi.contract.state[0].name, "shown");
        assert_eq!((abi.contract.state[0].offset, abi.contract.state[0].felt_size), (1, 1));

        let manifest = &abi.contract.state_layout;
        assert_eq!(manifest.field_count, 2);
        assert_eq!(manifest.slot_count, 2);
        assert_eq!(manifest.fields[0].name, "hidden");
        assert_eq!(manifest.fields[0].start_slot, 0);
        assert_eq!(manifest.fields[0].slot_count, 1);
        assert_eq!(manifest.fields[1].name, "shown");
        assert_eq!(manifest.fields[1].start_slot, 1);
        assert_eq!(manifest.fields[1].slot_count, 1);
    }

    #[test]
    fn trailing_private_and_all_private_storage_stay_in_the_manifest() {
        let mut program = Program::<u32>::new();
        let felt = felt_ty(&mut program);
        let shown = named_field(&mut program, "shown", felt.clone(), Visibility::Public);
        let tail = named_field(&mut program, "tail", felt, Visibility::Private);
        let abi = program_with_contract(&mut program, vec![shown, tail], Vec::new());
        assert_eq!(abi.contract.state.len(), 1);
        assert_eq!(abi.contract.state_layout.field_count, 2);
        assert_eq!(abi.contract.state_layout.slot_count, 2);
        assert_eq!(abi.contract.state_layout.fields[1].name, "tail");
        assert_eq!(abi.contract.state_layout.fields[1].start_slot, 1);

        let mut program = Program::<u32>::new();
        let felt = felt_ty(&mut program);
        let only = named_field(&mut program, "only", felt, Visibility::Private);
        let abi = program_with_contract(&mut program, vec![only], Vec::new());
        assert!(abi.contract.state.is_empty());
        assert_eq!(abi.contract.state_layout.field_count, 1);
        assert_eq!(abi.contract.state_layout.fields[0].name, "only");
        assert_eq!(abi.contract.state_layout.slot_count, 1);
    }

    #[test]
    fn private_struct_member_used_as_state_keeps_its_manifest_range() {
        let mut program = Program::<u32>::new();
        let felt = Identifier::new(program.interner.intern_ident("Felt"), Location::default());
        let inner_name = Identifier::new(program.interner.intern_ident("Inner"), Location::default());
        let inner_ty = UncheckedType::Basic(inner_name);
        let member = Identifier::new(program.interner.intern_ident("secret"), Location::default());
        let inner = DefinitionNode::Struct(StructNode {
            name: inner_name,
            generic_parameters: Vec::new(),
            fields: [(member, StructField {
                ty: UncheckedType::Basic(felt),
                attrs: Vec::new(),
                visibility: Visibility::Private,
                comments: Vec::new(),
                location: Location::default(),
            })]
            .into_iter()
            .collect(),
            attrs: Vec::new(),
            visibility: Visibility::Public,
            comments: Vec::new(),
            location: Location::default(),
            is_generated: false,
        });
        let holder = named_field(&mut program, "holder", inner_ty, Visibility::Public);
        let abi = program_with_contract(&mut program, vec![holder], vec![inner]);

        assert_eq!(abi.contract.state.len(), 1);
        assert_eq!(abi.contract.state[0].felt_size, 1);
        assert_eq!(abi.contract.state_layout.slot_count, 1);
        let published = abi.types.iter().find(|ty| ty.name == "Inner").expect("inner type");
        assert_eq!(published.fields.len(), 1);
        assert_eq!(published.fields[0].name, "secret");
        assert_eq!(published.fields[0].felt_size, 1);
        assert_eq!(published.fields[0].visibility, crate::FieldVisibility::Private);
    }

    #[test]
    fn scalar_map_tail_manifest_uses_storage_offsets_and_width() {
        let mut program = Program::<u32>::new();
        let felt = felt_ty(&mut program);
        let balances_ty = map_ty(&mut program, 4);
        let prefix = named_field(&mut program, "prefix", felt.clone(), Visibility::Public);
        let balances = named_field(&mut program, "balances", balances_ty, Visibility::Public);
        let tail = named_field(&mut program, "tail", felt, Visibility::Public);
        let abi = program_with_contract(&mut program, vec![prefix, balances, tail], Vec::new());

        let fields = &abi.contract.state;
        assert_eq!(fields.iter().map(|field| field.felt_size).collect::<Vec<_>>(), vec![1, 20, 1]);
        assert_eq!(fields.iter().map(|field| field.offset).collect::<Vec<_>>(), vec![0, 4, 24]);
        assert_eq!(abi.contract.state_layout.slot_count, 25);
        assert_eq!(abi.contract.state_layout.fields[1].payload_offset, 3);
        assert_eq!(abi.contract.state_layout.fields[1].slot_count, 23);
        assert_eq!(abi.contract.state_layout.fields[2].start_slot, 24);
    }

    #[test]
    fn struct_type_map_field_publishes_storage_size_and_offset() {
        let mut program = Program::<u32>::new();
        let felt = felt_ty(&mut program);
        let map = map_ty(&mut program, 4);
        let prefix = named_field(&mut program, "prefix", felt.clone(), Visibility::Public);
        let balances = named_field(&mut program, "balances", map, Visibility::Public);
        let tail = named_field(&mut program, "tail", felt.clone(), Visibility::Public);
        let holder_name = Identifier::new(program.interner.intern_ident("Pocket"), Location::default());
        let holder = DefinitionNode::Struct(StructNode {
            name: holder_name,
            generic_parameters: Vec::new(),
            fields: [prefix, balances, tail].into_iter().collect(),
            attrs: Vec::new(),
            visibility: Visibility::Public,
            comments: Vec::new(),
            location: Location::default(),
            is_generated: false,
        });
        let marker = named_field(&mut program, "marker", felt, Visibility::Public);
        let abi = program_with_contract(&mut program, vec![marker], vec![holder]);
        let published = abi.types.iter().find(|ty| ty.name == "Pocket").expect("pocket type");
        assert_eq!(published.fields.iter().map(|field| field.felt_size).collect::<Vec<_>>(), vec![1, 20, 1]);
        assert_eq!(
            published.fields.iter().map(|field| field.offset_within_parent).collect::<Vec<_>>(),
            vec![0, 4, 24]
        );
        assert_eq!(published.felt_size, 25);
    }
    #[test]
    fn associated_type_map_field_uses_resolved_width() {
        let mut program = Program::<u32>::new();
        let felt = felt_ty(&mut program);
        let map = map_ty(&mut program, 4);
        let carrier_name = Identifier::new(program.interner.intern_ident("Carrier"), Location::default());
        let cells_name = Identifier::new(program.interner.intern_ident("Cells"), Location::default());
        let mut node = psy_ast::ImplNode {
            generic_parameters: Vec::new(),
            associated_types: Default::default(),
            ty: UncheckedType::Basic(carrier_name),
            body: Vec::new(),
            attrs: Vec::new(),
            comments: Vec::new(),
            location: Location::default(),
            is_generated: false,
        };
        node.associated_types.insert(
            cells_name,
            psy_ast::AssociatedTypeValue {
                ty: map,
                visibility: Visibility::Public,
                comments: Vec::new(),
                location: Location::default(),
            },
        );
        program.defs.alloc_item(DefinitionNode::Impl(node));
        let cells = UncheckedType::Path(Box::new(PathNode {
            root: Some(UncheckedType::Basic(carrier_name)),
            segments: Vec::new(),
            target: UncheckedType::Basic(cells_name),
            is_ty: true,
            location: Location::default(),
        }));
        let keyed = UncheckedType::Generic(
            Identifier::new(program.interner.intern_ident("Map"), Location::default()),
            vec![cells, felt.clone(), UncheckedType::Const(ConstValue::U32(4), Location::default())],
            Location::default(),
        );
        let field = named_field(&mut program, "cells", keyed, Visibility::Public);
        let tail = named_field(&mut program, "tail", felt, Visibility::Public);
        let abi = program_with_contract(&mut program, vec![field, tail], Vec::new());
        assert_eq!(abi.contract.state[0].felt_size, 20);
        assert_eq!(abi.contract.state[1].offset, 20);
        match &abi.contract.state[0].ty {
            TypeRef::Map { key, capacity, .. } => {
                assert_eq!(*capacity, 4);
                assert!(matches!(key.as_ref(), TypeRef::Map { capacity: 4, .. }));
            }
            other => panic!("associated map key should stay a map, got {other:?}"),
        }
    }


}

#[cfg(test)]
mod tuple_layout_tests {
    use psy_ast::{Identifier, Location};

    use super::*;

    #[test]
    fn felt_size_counts_tuple_elements() {
        let mut program = Program::<u32>::new();
        let felt = Identifier::new(program.interner.intern_ident("Felt"), Location::default());
        let u32_ident = Identifier::new(program.interner.intern_ident("u32"), Location::default());
        let ctx = DefaultVisitorContext::<u32, ()>::new(&mut program);

        let extractor = AbiExtractor::new("TestContract".to_string());
        let struct_nodes = BTreeMap::new();
        let mut layouts = HashMap::new();
        let tuple = UncheckedType::Tuple(vec![UncheckedType::Basic(felt), UncheckedType::Basic(u32_ident)], Location::default());

        assert_eq!(extractor.felt_size_for_type(&ctx, &tuple, &struct_nodes, &mut layouts), 2);
    }
}
