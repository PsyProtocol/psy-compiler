use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use psy_ast::{DefId, DefaultVisitorContext, FunctionNode, Program, StructNode, UncheckedType, Visibility, VisitorContext};

use crate::{
    Abi, AbiContract, AbiMethod, AbiParam, AbiStateField, AbiStructField, AbiStructType,
    AbiTypeKind, MapKind, PrimitiveTypeName, StateMutability, TypeRef,
};

#[derive(Clone)]
struct MethodCompatInfo {
    method_id: u32,
    is_view: bool,
}

#[derive(Clone)]
struct StructFieldLayout {
    pub name: String,
    pub offset: usize,
    pub felt_size: usize,
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


    fn collect_struct_def_ids<F: Clone + From<u32>>(&self, ctx: &DefaultVisitorContext<F, ()>) -> HashMap<String, DefId> {
        let mut structs = HashMap::new();
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if let Some(struct_node) = ctx.definition(def_id).as_struct() {
                let struct_name = ctx.ident(struct_node.name).0.to_string();
                if !self.is_internal_type(&struct_name) {
                    structs.insert(struct_name, def_id);
                }
            }
        }
        structs
    }

    fn collect_referenced_type_names_from_struct<F: Clone + From<u32>>(
        &self,
        struct_node: &StructNode,
        ctx: &DefaultVisitorContext<F, ()>,
    ) -> Vec<String> {
        let mut names = Vec::new();
        for (_, field) in struct_node.fields.iter().filter(|(_, field)| Self::is_public(&field.visibility)) {
            self.collect_referenced_type_names(&field.ty, ctx, &mut names);
        }
        names
    }

    fn collect_referenced_type_names_from_impl_functions<F: Clone + From<u32>>(
        &self,
        struct_name: &str,
        ctx: &DefaultVisitorContext<F, ()>,
        included_defs: Option<&HashSet<DefId>>,
    ) -> Vec<String> {
        let mut names = Vec::new();
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if included_defs.is_some_and(|defs| !defs.contains(&def_id)) {
                continue;
            }
            let Some(impl_node) = ctx.definition(def_id).as_impl() else {
                continue;
            };
            let impl_type_name = self.extract_type_name(&impl_node.ty, ctx);
            if impl_type_name != struct_name && impl_type_name != format!("{struct_name}Ref") {
                continue;
            }
            for &function_def_id in &impl_node.body {
                let Some(function) = ctx.definition(function_def_id).as_function() else {
                    continue;
                };
                let function_name = ctx.ident(function.name).0.to_string();
                if !Self::is_public(&function.visibility) || self.is_internal_function(&function_name) {
                    continue;
                }
                for param in &function.parameters {
                    self.collect_referenced_type_names(&param.ty, ctx, &mut names);
                }
                if let Some(return_type) = &function.return_type {
                    self.collect_referenced_type_names(return_type, ctx, &mut names);
                }
            }
        }
        names
    }

    fn collect_referenced_type_names<F: Clone + From<u32>>(&self, ty: &UncheckedType, ctx: &DefaultVisitorContext<F, ()>, names: &mut Vec<String>) {
        match ty {
            UncheckedType::Basic(identifier) => names.push(ctx.ident(*identifier).0.to_string()),
            UncheckedType::Generic(identifier, generics, _) => {
                names.push(ctx.ident(*identifier).0.to_string());
                for generic in generics {
                    self.collect_referenced_type_names(generic, ctx, names);
                }
            }
            UncheckedType::Array(inner, _, _) => self.collect_referenced_type_names(inner, ctx, names),
            UncheckedType::Tuple(items, _) => {
                for item in items {
                    self.collect_referenced_type_names(item, ctx, names);
                }
            }
            UncheckedType::FunctionSignature(signature, _) => {
                for parameter in &signature.parameters {
                    self.collect_referenced_type_names(parameter, ctx, names);
                }
                if let Some(return_type) = &signature.return_type {
                    self.collect_referenced_type_names(return_type, ctx, names);
                }
            }
            UncheckedType::Path(path) => self.collect_referenced_type_names(&path.target, ctx, names),
            UncheckedType::TraitCast(inner, trait_ty, _) => {
                self.collect_referenced_type_names(inner, ctx, names);
                self.collect_referenced_type_names(trait_ty, ctx, names);
            }
            UncheckedType::Const(_, _) | UncheckedType::Unknown => {}
        }
    }

    fn collect_package_def_ids<F: Clone + From<u32>>(&self, ctx: &DefaultVisitorContext<F, ()>, package_root: &Path) -> HashSet<DefId> {
        let package_root = normalize_path_for_prefix(package_root);
        let mut defs = HashSet::new();

        for module in ctx.program().modules.iter() {
            let Some(module_path) = ctx.program().file_resolver.resolve_path(&module.data().file_id) else {
                continue;
            };
            let module_path = normalize_path_for_prefix(module_path);
            if module_path.starts_with(&package_root) {
                defs.extend(module.data().definitions.iter().copied());
            }
        }

        defs
    }



    /// Extract the ABI from the same checked program.
    ///
    /// `state_tree_height` and `method_metadata` come from the compiler
    /// pipeline (the same data `extract_contract_abi` consumes).
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
        let state = contract_struct
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
                    length: *size,
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
    fn extract_canonical_state<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        contract_struct: &StructNode,
        struct_nodes: &HashMap<String, &StructNode>,
        struct_layouts: &HashMap<String, StructLayout>,
    ) -> Vec<AbiStateField> {
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

        fields
    }

    /// Compute felt_size for a type and whether it's a map (for alignment).
    fn canonical_felt_size_and_is_map<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &HashMap<String, &StructNode>,
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
        struct_nodes: &HashMap<String, &StructNode>,
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

    fn collect_struct_nodes<'a, F: Clone + From<u32>>(&self, ctx: &'a DefaultVisitorContext<F, ()>) -> HashMap<String, &'a StructNode> {
        let mut structs = HashMap::new();
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if let Some(struct_node) = ctx.definition(def_id).as_struct() {
                structs.insert(ctx.ident(struct_node.name).0.to_string(), struct_node);
            }
        }
        structs
    }

    fn compute_struct_layouts<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        struct_nodes: &HashMap<String, &StructNode>,
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
        struct_nodes: &HashMap<String, &StructNode>,
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
        for (field_name, field) in &struct_node.fields {
            let felt_size = self.felt_size_for_type(ctx, &field.ty, struct_nodes, layouts);
            fields.push(StructFieldLayout {
                name: ctx.ident(*field_name).0.to_string(),
                offset,
                felt_size,
            });
            offset += felt_size;
        }

        let layout = StructLayout { felt_size: offset, fields };
        layouts.insert(name.to_string(), layout.clone());
        layout
    }

    fn compat_type_metadata<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &HashMap<String, &StructNode>,
        struct_layouts: &HashMap<String, StructLayout>,
    ) -> (
        usize,
        bool,
        Option<usize>,
        Option<String>,
        Option<usize>,
        bool,
        Option<String>,
        Option<String>,
        Option<usize>,
    ) {
        let mut layouts = struct_layouts.clone();
        if let Some((key_type, value_type, capacity)) = self.extract_imt_map_info(ctx, ty) {
            return (
                capacity.saturating_mul(4),
                false,
                None,
                None,
                Some(4),
                true,
                Some(key_type),
                Some(value_type),
                Some(capacity),
            );
        }

        match ty {
            UncheckedType::Array(inner, size, _) => {
                let inner_size = self.felt_size_for_type(ctx, inner, struct_nodes, &mut layouts);
                (
                    inner_size.saturating_mul(*size as usize),
                    true,
                    Some(*size as usize),
                    Some(self.stringify_unchecked_type(ctx, inner)),
                    Some(inner_size),
                    false,
                    None,
                    None,
                    None,
                )
            }
            _ => (
                self.felt_size_for_type(ctx, ty, struct_nodes, &mut layouts),
                false,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
            ),
        }
    }

    fn resolve_sub_fields_for_type<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &HashMap<String, &StructNode>,
        struct_layouts: &HashMap<String, StructLayout>,
    ) -> Option<Vec<StructFieldLayout>> {
        let struct_name = match ty {
            UncheckedType::Basic(identifier) => Some(ctx.ident(*identifier).0.to_string()),
            UncheckedType::Path(path) => Some(self.extract_type_name(&path.target, ctx)),
            UncheckedType::Array(inner, _, _) => match inner.as_ref() {
                UncheckedType::Basic(identifier) => Some(ctx.ident(*identifier).0.to_string()),
                UncheckedType::Path(path) => Some(self.extract_type_name(&path.target, ctx)),
                _ => None,
            },
            _ => None,
        }?;

        if !struct_nodes.contains_key(&struct_name) {
            return None;
        }

        struct_layouts
            .get(&struct_name)
            .and_then(|layout| if layout.fields.is_empty() { None } else { Some(layout.fields.clone()) })
    }

    fn felt_size_for_type<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &HashMap<String, &StructNode>,
        layouts: &mut HashMap<String, StructLayout>,
    ) -> usize {
        if self.extract_imt_map_info(ctx, ty).is_some() {
            return 0;
        }

        match ty {
            UncheckedType::Basic(identifier) => self.felt_size_for_named_type(ctx.ident(*identifier).0.as_str(), struct_nodes, layouts, ctx),
            UncheckedType::Path(path) => self.felt_size_for_type(ctx, &path.target, struct_nodes, layouts),
            UncheckedType::Array(inner, size, _) => self.felt_size_for_type(ctx, inner, struct_nodes, layouts).saturating_mul(*size as usize),
            UncheckedType::Tuple(items, _) => items.iter().map(|item| self.felt_size_for_type(ctx, item, struct_nodes, layouts)).sum(),
            UncheckedType::Generic(identifier, _, _) => self.felt_size_for_named_type(ctx.ident(*identifier).0.as_str(), struct_nodes, layouts, ctx),
            _ => 0,
        }
    }

    fn felt_size_for_named_type<F: Clone + From<u32>>(
        &self,
        type_name: &str,
        struct_nodes: &HashMap<String, &StructNode>,
        layouts: &mut HashMap<String, StructLayout>,
        ctx: &DefaultVisitorContext<F, ()>,
    ) -> usize {
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

fn normalize_path_for_prefix(path: &Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
