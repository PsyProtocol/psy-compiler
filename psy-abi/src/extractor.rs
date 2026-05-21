use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use psy_ast::{DefId, DefaultVisitorContext, FunctionNode, Program, StructNode, UncheckedType, Visibility, VisitorContext};

use crate::{
    CompatMethod, CompatMethodParam, CompatStateField, CompatSubField, ContractCompatAbi, FieldAbiSpec, FunctionAbiSpec, ParamAbiSpec,
    SpecCompliantAbi, StructAbiSpec, TypeAbiSpec,
};

#[derive(Clone)]
struct MethodCompatInfo {
    method_id: u32,
    is_view: bool,
}

#[derive(Clone)]
struct CompatStructLayout {
    felt_size: usize,
    fields: Vec<CompatSubField>,
}

pub struct AbiExtractor {
    pub contract_name: String,
}

impl AbiExtractor {
    pub fn new(contract_name: String) -> Self {
        Self { contract_name }
    }

    pub fn extract_spec_compliant_abi<F: Clone + From<u32>>(self, program: &mut Program<F>) -> Result<SpecCompliantAbi, psy_common::Error> {
        self.extract_spec_compliant_abi_with_package_root(program, None)
    }

    pub fn extract_spec_compliant_abi_for_package_root<F: Clone + From<u32>>(
        self,
        program: &mut Program<F>,
        package_root: &Path,
    ) -> Result<SpecCompliantAbi, psy_common::Error> {
        self.extract_spec_compliant_abi_with_package_root(program, Some(package_root))
    }

    fn extract_spec_compliant_abi_with_package_root<F: Clone + From<u32>>(
        self,
        program: &mut Program<F>,
        package_root: Option<&Path>,
    ) -> Result<SpecCompliantAbi, psy_common::Error> {
        let ctx = DefaultVisitorContext::<F, ()>::new(program);
        let mut spec_abi = SpecCompliantAbi::new("1.0.0".to_string());
        let included_defs = package_root.map(|package_root| self.collect_package_def_ids(&ctx, package_root));

        let struct_defs = self.collect_struct_def_ids(&ctx);
        let mut struct_map = HashMap::new();
        let mut pending_dependency_types = Vec::new();

        // Collect package structs first. These keep their associated functions.
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if included_defs.as_ref().is_some_and(|defs| !defs.contains(&def_id)) {
                continue;
            }
            if let Some(struct_node) = ctx.definition(def_id).as_struct() {
                let struct_name = ctx.ident(struct_node.name).0.to_string();

                // Skip internal types
                if self.is_internal_type(&struct_name) {
                    continue;
                }

                pending_dependency_types.extend(self.collect_referenced_type_names_from_struct(struct_node, &ctx));
                pending_dependency_types.extend(self.collect_referenced_type_names_from_impl_functions(&struct_name, &ctx, included_defs.as_ref()));
                let struct_spec = self.extract_struct_abi_spec(struct_node, &ctx, true, true, included_defs.as_ref());
                struct_map.insert(struct_name, struct_spec);
            }
        }

        // Add external structs that are needed to resolve package struct fields
        // and method params. These are data-only definitions: no external
        // package functions are exported into this ABI.
        while let Some(type_name) = pending_dependency_types.pop() {
            if struct_map.contains_key(&type_name) || self.is_internal_type(&type_name) {
                continue;
            }
            let Some(def_id) = struct_defs.get(&type_name).copied() else {
                continue;
            };
            if included_defs.as_ref().is_some_and(|defs| defs.contains(&def_id)) {
                continue;
            }
            let Some(struct_node) = ctx.definition(def_id).as_struct() else {
                continue;
            };

            pending_dependency_types.extend(self.collect_referenced_type_names_from_struct(struct_node, &ctx));
            struct_map.insert(type_name, self.extract_struct_abi_spec(struct_node, &ctx, false, false, included_defs.as_ref()));
        }

        // Add all structs to the ABI
        for struct_spec in struct_map.into_values() {
            spec_abi.add_struct(struct_spec);
        }

        // Inject built-in composite types that don't have AST definitions
        self.inject_builtin_types(&mut spec_abi);

        Ok(spec_abi)
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

    fn extract_struct_abi_spec<F: Clone + From<u32>>(
        &self,
        struct_node: &StructNode,
        ctx: &DefaultVisitorContext<F, ()>,
        is_contract: bool,
        include_functions: bool,
        included_defs: Option<&HashSet<DefId>>,
    ) -> StructAbiSpec {
        let struct_name = ctx.ident(struct_node.name).0.to_string();
        let fields = struct_node
            .fields
            .iter()
            .filter(|(_, field)| Self::is_public(&field.visibility))
            .map(|(name, field)| FieldAbiSpec {
                name: ctx.ident(*name).0.to_string(),
                field_type: TypeAbiSpec::from_unchecked_type(&field.ty, ctx),
            })
            .collect();

        let functions = if include_functions {
            let functions = self.find_impl_functions(&struct_name, ctx, included_defs);
            (!functions.is_empty()).then_some(functions)
        } else {
            None
        };

        StructAbiSpec {
            name: struct_name,
            is_contract: is_contract && self.has_contract_attr(struct_node, ctx),
            fields,
            functions,
        }
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

    pub fn extract_contract_abi<F: Clone + From<u32>>(
        &self,
        program: &mut Program<F>,
        state_tree_height: u16,
        method_metadata: &HashMap<String, (u32, bool)>,
    ) -> Result<ContractCompatAbi, psy_common::Error> {
        let ctx = DefaultVisitorContext::<F, ()>::new(program);
        let contract_struct = self.find_contract_struct(&ctx);
        let contract_name = contract_struct
            .map(|struct_node| ctx.ident(struct_node.name).0.to_string())
            .unwrap_or_else(|| self.contract_name.clone());

        let struct_nodes = self.collect_struct_nodes(&ctx);
        let struct_layouts = self.compute_struct_layouts(&ctx, &struct_nodes);

        let state_layout = contract_struct
            .map(|struct_node| self.extract_state_layout(&ctx, struct_node, &struct_layouts))
            .unwrap_or_default();

        let methods = self.extract_contract_methods(&ctx, &contract_name, &struct_layouts, method_metadata);

        Ok(ContractCompatAbi {
            contract_name,
            state_tree_height,
            state_layout,
            methods,
        })
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

    fn find_impl_functions<F: Clone + From<u32>>(
        &self,
        struct_name: &str,
        ctx: &DefaultVisitorContext<F, ()>,
        included_defs: Option<&HashSet<DefId>>,
    ) -> Vec<FunctionAbiSpec> {
        let mut functions = Vec::new();

        // Look for impl blocks that implement this struct
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if included_defs.is_some_and(|defs| !defs.contains(&def_id)) {
                continue;
            }
            if let Some(impl_node) = ctx.definition(def_id).as_impl() {
                // Check if this impl is for our target struct or its Ref version
                let impl_type_name = self.extract_type_name(&impl_node.ty, ctx);
                if impl_type_name == struct_name || impl_type_name == format!("{}Ref", struct_name) {
                    // Extract functions from this impl block
                    for &function_def_id in &impl_node.body {
                        if let Some(function) = ctx.definition(function_def_id).as_function() {
                            let function_name = ctx.ident(function.name).0.to_string();

                            // Skip internal functions and only include public functions
                            if Self::is_public(&function.visibility) && !self.is_internal_function(&function_name) {
                                let function_spec = self.extract_function_abi_spec(function, ctx);
                                functions.push(function_spec);
                            }
                        }
                    }
                }
            }
        }

        functions
    }

    fn extract_type_name<F: Clone + From<u32>>(&self, unchecked_type: &psy_ast::UncheckedType, ctx: &DefaultVisitorContext<F, ()>) -> String {
        match unchecked_type {
            psy_ast::UncheckedType::Basic(identifier) => ctx.ident(*identifier).0.to_string(),
            psy_ast::UncheckedType::Path(path) => self.extract_type_name(&path.target, ctx),
            _ => "unknown".to_string(),
        }
    }

    fn extract_function_abi_spec<F: Clone + From<u32>>(&self, function: &FunctionNode, ctx: &DefaultVisitorContext<F, ()>) -> FunctionAbiSpec {
        let params = function
            .parameters
            .iter()
            .map(|param| ParamAbiSpec {
                name: ctx.ident(param.name).0.to_string(),
                param_type: TypeAbiSpec::from_unchecked_type(&param.ty, ctx),
            })
            .collect();

        FunctionAbiSpec {
            name: ctx.ident(function.name).0.to_string(),
            params,
            return_type: vec![], // As per spec, always empty for now
        }
    }

    fn inject_builtin_types(&self, spec_abi: &mut SpecCompliantAbi) {
        // Inject Hash = [Felt; 4]
        if self.type_is_used_in_abi("Hash", spec_abi) {
            spec_abi.add_struct(StructAbiSpec {
                name: "Hash".to_string(),
                is_contract: false,
                fields: vec![FieldAbiSpec {
                    name: "value".to_string(),
                    field_type: TypeAbiSpec::Array {
                        type_name: "Array".to_string(),
                        inner_type: "Felt".to_string(),
                        length: 4,
                    },
                }],
                functions: None,
            });
        }
    }

    fn type_is_used_in_abi(&self, type_name: &str, spec_abi: &SpecCompliantAbi) -> bool {
        let check_type = |t: &TypeAbiSpec| -> bool { matches!(t, TypeAbiSpec::Basic(n) if n == type_name) };

        for s in &spec_abi.structs {
            for f in &s.fields {
                if check_type(&f.field_type) {
                    return true;
                }
            }
            if let Some(functions) = &s.functions {
                for func in functions {
                    for p in &func.params {
                        if check_type(&p.param_type) {
                            return true;
                        }
                    }
                }
            }
        }
        false
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
    ) -> HashMap<String, CompatStructLayout> {
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
        layouts: &mut HashMap<String, CompatStructLayout>,
    ) -> CompatStructLayout {
        if let Some(layout) = layouts.get(name) {
            return layout.clone();
        }

        let Some(struct_node) = struct_nodes.get(name).copied() else {
            return CompatStructLayout {
                felt_size: 0,
                fields: vec![],
            };
        };

        let mut offset = 0usize;
        let mut fields = Vec::new();
        for (field_name, field) in &struct_node.fields {
            let felt_size = self.felt_size_for_type(ctx, &field.ty, struct_nodes, layouts);
            fields.push(CompatSubField {
                name: ctx.ident(*field_name).0.to_string(),
                offset,
                felt_size,
            });
            offset += felt_size;
        }

        let layout = CompatStructLayout { felt_size: offset, fields };
        layouts.insert(name.to_string(), layout.clone());
        layout
    }

    fn extract_state_layout<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        contract_struct: &StructNode,
        struct_layouts: &HashMap<String, CompatStructLayout>,
    ) -> Vec<CompatStateField> {
        let struct_nodes = self.collect_struct_nodes(ctx);
        let mut offset = 0usize;
        let mut fields = Vec::new();

        for (field_name, field) in &contract_struct.fields {
            let field_type = self.stringify_unchecked_type(ctx, &field.ty);
            let (felt_size, is_array, array_count, element_type, element_felt_size, is_imt_map, imt_key_type, imt_value_type, imt_capacity) =
                self.compat_type_metadata(ctx, &field.ty, &struct_nodes, struct_layouts);

            let sub_fields = self.resolve_sub_fields_for_type(ctx, &field.ty, &struct_nodes, struct_layouts);

            if !Self::is_public(&field.visibility) {
                if is_imt_map {
                    let aligned_offset = (offset + 3) & !3;
                    offset = aligned_offset + felt_size;
                } else {
                    offset += felt_size;
                }
                continue;
            }

            fields.push(CompatStateField {
                name: ctx.ident(*field_name).0.to_string(),
                field_type,
                offset,
                felt_size,
                is_array,
                array_count,
                element_type,
                element_felt_size,
                sub_fields,
                is_imt_map,
                imt_key_type,
                imt_value_type,
                imt_capacity,
            });

            if is_imt_map {
                let aligned_offset = (offset + 3) & !3;
                offset = aligned_offset + felt_size;
                if let Some(last) = fields.last_mut() {
                    last.offset = aligned_offset;
                }
            } else {
                offset += felt_size;
            }
        }

        fields
    }

    fn extract_contract_methods<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        contract_name: &str,
        struct_layouts: &HashMap<String, CompatStructLayout>,
        method_metadata: &HashMap<String, (u32, bool)>,
    ) -> Vec<CompatMethod> {
        let mut methods = Vec::new();

        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if let Some(function) = ctx.definition(def_id).as_function() {
                let method_name = ctx.ident(function.name).0.to_string();
                let Some(&(method_id, is_view)) = method_metadata.get(&method_name) else {
                    continue;
                };
                methods.push(self.function_to_compat_method(ctx, function, method_id, is_view, struct_layouts));
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
                    methods.push(self.function_to_compat_method(ctx, function, method_id, is_view, struct_layouts));
                }
            }
        }

        methods.sort_by(|a, b| a.name.cmp(&b.name));
        methods.dedup_by(|a, b| a.name == b.name);
        methods
    }

    fn function_to_compat_method<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        function: &FunctionNode,
        method_id: u32,
        is_view: bool,
        struct_layouts: &HashMap<String, CompatStructLayout>,
    ) -> CompatMethod {
        let struct_nodes = self.collect_struct_nodes(ctx);
        let params = function
            .parameters
            .iter()
            .filter(|param| ctx.ident(param.name).0.as_str() != "self")
            .map(|param| CompatMethodParam {
                name: ctx.ident(param.name).0.to_string(),
                param_type: self.stringify_unchecked_type(ctx, &param.ty),
                felt_size: self.felt_size_for_type(ctx, &param.ty, &struct_nodes, &mut struct_layouts.clone()),
            })
            .collect();

        CompatMethod {
            name: ctx.ident(function.name).0.to_string(),
            method_id,
            params,
            is_view,
        }
    }

    fn compat_type_metadata<F: Clone + From<u32>>(
        &self,
        ctx: &DefaultVisitorContext<F, ()>,
        ty: &UncheckedType,
        struct_nodes: &HashMap<String, &StructNode>,
        struct_layouts: &HashMap<String, CompatStructLayout>,
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
        struct_layouts: &HashMap<String, CompatStructLayout>,
    ) -> Option<Vec<CompatSubField>> {
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
        layouts: &mut HashMap<String, CompatStructLayout>,
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
        layouts: &mut HashMap<String, CompatStructLayout>,
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
        if type_name != "Map" && type_name != "NamespacedMap" {
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
