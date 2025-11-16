use psy_ast::{DefId, DefaultVisitorContext, FunctionNode, Program, StructNode, Visibility, VisitorContext};

use crate::{FieldAbiSpec, FunctionAbiSpec, ParamAbiSpec, SpecCompliantAbi, StructAbiSpec, TypeAbiSpec};

pub struct AbiExtractor {
    pub contract_name: String,
}

impl AbiExtractor {
    pub fn new(contract_name: String) -> Self {
        Self { contract_name }
    }

    pub fn extract_spec_compliant_abi<F: Clone + From<u32> + 'static>(
        self,
        program: &'static mut Program<F>,
    ) -> Result<SpecCompliantAbi, psy_common::Error> {
        let ctx = DefaultVisitorContext::<F, ()>::new(program);
        let mut spec_abi = SpecCompliantAbi::new("1.0.0".to_string());

        // First pass: collect all struct information
        let mut struct_map = std::collections::HashMap::new();

        // Collect all structs by iterating through all definitions in the arena
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
            if let Some(struct_node) = ctx.definition(def_id).as_struct() {
                let struct_name = ctx.ident(struct_node.name).0.to_string();

                // Skip internal types
                if self.is_internal_type(&struct_name) {
                    continue;
                }

                let is_contract = self.has_contract_attr(struct_node, &ctx);
                let _is_public = Self::is_public(&struct_node.visibility);

                // Include all structs (both public and private)
                // We'll let the user decide what should be in ABI
                let fields = struct_node
                    .fields
                    .iter()
                    .filter(|(_, field)| Self::is_public(&field.visibility))
                    .map(|(name, field)| FieldAbiSpec {
                        name: ctx.ident(*name).0.to_string(),
                        field_type: TypeAbiSpec::from_unchecked_type(&field.ty, &ctx),
                    })
                    .collect();

                let mut struct_spec = StructAbiSpec {
                    name: struct_name.clone(),
                    is_contract,
                    fields,
                    functions: None,
                };

                // Find associated functions (only for contracts or when functions are
                // explicitly associated)
                let functions = self.find_impl_functions(&struct_name, &ctx);
                if !functions.is_empty() {
                    struct_spec.functions = Some(functions);
                }

                struct_map.insert(struct_name, struct_spec);
            }
        }

        // Add all structs to the ABI
        for struct_spec in struct_map.into_values() {
            spec_abi.add_struct(struct_spec);
        }

        Ok(spec_abi)
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

    fn find_impl_functions<F: Clone + From<u32>>(&self, struct_name: &str, ctx: &DefaultVisitorContext<F, ()>) -> Vec<FunctionAbiSpec> {
        let mut functions = Vec::new();

        // Look for impl blocks that implement this struct
        for i in 0..ctx.program().defs.len() {
            let def_id = DefId::from(i);
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
