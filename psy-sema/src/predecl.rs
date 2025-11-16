use indexmap::IndexMap;
use psy_ast::*;
use psy_vm::dpn::ops::context_trait::ContextFelt;

use crate::{
    CheckedDefinitionNode, CheckedFunctionNode, CheckedFunctionParameter, CheckedGenericParameter, CheckedStructNode, CheckedTraitNode,
    CheckedVariable, Error, Implementer, Result, ScopeKind, Type, TypeChecker, TypeCheckerVisitorContext, VOID_TYPE,
};

impl<F: Clone + From<u32> + ContextFelt, C> TypeChecker<F, C> {
    pub fn typecheck_function_predecl(
        &mut self,
        def_id: DefId,
        scope_kind: ScopeKind,
        _generic_scope_kind: ScopeKind,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<DefId> {
        if let Some(NodeId::Def(def_id)) = self.unchecked_checked.get(&def_id.into()) {
            return Ok(*def_id);
        }

        let function_node = ctx.definition(def_id).as_function().cloned().unwrap();

        ctx.symbols.start_scope(scope_kind);

        let type_id = ctx.symbols.next_type_id(0);
        let checked_function = CheckedFunctionNode {
            name: function_node.name,
            parameters: vec![],
            qualifier: function_node.qualifier,
            generic_parameters: vec![],
            body: None,
            return_type: VOID_TYPE,
            scope_id: ctx.symbols.current_scope_id().unwrap(),
            visibility: function_node.visibility,
            attrs: function_node.attrs,
            type_id,
            location: function_node.location,
            comments: function_node.comments,
        };

        ctx.add_type_reference(type_id, checked_function.name.location, false);

        let ty = Type::Function(checked_function);
        ctx.symbols.add_type(ctx.symbols.parent_scope_id(), function_node.name, ty)?;

        let checked_def_id = self
            .program
            .defs
            .alloc_item(CheckedDefinitionNode::Function(ctx.symbols[type_id].as_function().cloned().unwrap()));
        self.unchecked_checked.insert(def_id.into(), checked_def_id.into());

        ctx.symbols.end_scope();
        Ok(checked_def_id)
    }

    pub fn typecheck_struct_predecl(&mut self, def_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId> {
        if let Some(NodeId::Def(def_id)) = self.unchecked_checked.get(&def_id.into()) {
            return Ok(*def_id);
        }

        let struct_node = ctx.definition(def_id).as_struct().cloned().unwrap();

        ctx.symbols.start_scope(ScopeKind::Struct);

        let type_id = ctx.symbols.next_type_id(0);
        let checked_struct = CheckedStructNode {
            name: struct_node.name,
            generic_parameters: vec![],
            fields: IndexMap::new(),
            scope_id: ctx.symbols.current_scope_id().unwrap(),
            attrs: struct_node.attrs,
            visibility: struct_node.visibility,
            location: struct_node.location,
            comments: struct_node.comments,
            type_id,
        };

        ctx.add_type_reference(type_id, checked_struct.name.location, false);

        let ty = Type::Struct(checked_struct);
        ctx.symbols.add_type(ctx.symbols.parent_scope_id(), struct_node.name, ty)?;

        let checked_def_id = self
            .program
            .defs
            .alloc_item(CheckedDefinitionNode::Struct(ctx.symbols[type_id].as_struct().cloned().unwrap()));
        self.unchecked_checked.insert(def_id.into(), checked_def_id.into());

        ctx.symbols.end_scope();
        Ok(checked_def_id)
    }

    pub fn typecheck_trait_predecl(&mut self, def_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId> {
        if let Some(NodeId::Def(def_id)) = self.unchecked_checked.get(&def_id.into()) {
            return Ok(*def_id);
        }

        let trait_node = ctx.definition(def_id).as_trait().cloned().unwrap();

        ctx.symbols.start_scope(ScopeKind::Trait);

        let mut body = Vec::with_capacity(trait_node.body.len());
        for &function_id in &trait_node.body {
            ctx.push_node_id(NodeId::from(function_id));
            self.infcx.enter_scope();

            let method_id = self.typecheck_function_predecl(function_id, ScopeKind::TraitMethod, ScopeKind::Trait, ctx)?;

            self.infcx.exit_scope();
            ctx.pop_node_id();
            body.push(method_id);
        }

        let type_id = ctx.symbols.next_type_id(0);
        let checked_trait = CheckedTraitNode {
            associated_types: IndexMap::new(),
            generic_parameters: vec![],
            name: trait_node.name,
            body: body,
            unchecked_body: trait_node.body,
            scope_id: ctx.symbols.current_scope_id().unwrap(),
            visibility: trait_node.visibility,
            location: trait_node.location,
            comments: trait_node.comments,
            type_id,
        };

        ctx.add_type_reference(type_id, checked_trait.name.location, false);

        let ty = Type::Trait(checked_trait);
        ctx.symbols.add_type(ctx.symbols.parent_scope_id(), trait_node.name, ty)?;

        ctx.symbols.add_type_variable(
            ScopeKind::Trait,
            CheckedGenericParameter::new(
                IdentId::TYPE_SELF,
                vec![type_id],
                ctx.symbols.current_scope_id().unwrap(),
                Location::default(),
            ),
        )?;

        let checked_def_id = self
            .program
            .defs
            .alloc_item(CheckedDefinitionNode::Trait(ctx.symbols[type_id].as_trait().cloned().unwrap()));
        self.unchecked_checked.insert(def_id.into(), checked_def_id.into());

        ctx.symbols.end_scope();
        Ok(checked_def_id)
    }

    // so that the other modules can import
    pub fn typecheck_definition_predecl(&mut self, def_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        ctx.push_node_id(NodeId::from(def_id));
        let node_type = ctx.definition(def_id).node_type();
        match node_type {
            NodeType::FunctionDef => {
                self.typecheck_function_predecl(def_id, ScopeKind::Function, ScopeKind::Function, ctx)?;
            }
            NodeType::StructDef => {
                self.typecheck_struct_predecl(def_id, ctx)?;
            }
            NodeType::EnumDef => {}
            NodeType::ImplDef => {}
            NodeType::TraitImplDef => {}
            NodeType::TraitDef => {
                self.typecheck_trait_predecl(def_id, ctx)?;
            }
            NodeType::TypeAliasDef => {}
            NodeType::ConstDef => {
                self.visit_const(def_id, ctx)?;
            }
            NodeType::UseDef => {}
            _ => {}
        }
        ctx.pop_node_id();
        Ok(())
    }

    // so that the other module can access function signature and struct fields etc
    pub fn typecheck_definition_header(&mut self, def_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        ctx.push_node_id(NodeId::from(def_id));
        match ctx.definition(def_id).node_type() {
            NodeType::FunctionDef => {
                self.infcx.enter_context();
                self.typecheck_function_signature(def_id, ScopeKind::Function, ScopeKind::Function, ctx)?;
                self.infcx.exit_context();
            }
            NodeType::StructDef => {
                self.visit_struct(def_id, ctx)?;
            }
            NodeType::EnumDef => {}
            NodeType::ImplDef => {
                self.visit_impl(def_id, ctx)?;
            }
            NodeType::TraitImplDef => {
                self.visit_trait_impl(def_id, ctx)?;
            }
            NodeType::TraitDef => {
                self.visit_trait(def_id, ctx)?;
            }
            NodeType::TypeAliasDef => {
                self.visit_type_alias(def_id, ctx)?;
            }
            NodeType::ConstDef => {}
            NodeType::UseDef => {
                self.visit_use(def_id, ctx)?;
            }
            _ => std::unreachable!(),
        };
        ctx.pop_node_id();
        Ok(())
    }

    // so that interpreter can execute the function body
    pub fn typecheck_definition_body(&mut self, def_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        ctx.push_node_id(NodeId::from(def_id));
        match ctx.definition(def_id).node_type() {
            NodeType::FunctionDef => {
                self.infcx.enter_context();
                self.typecheck_function_body(def_id, ctx)?;
                self.infcx.exit_context();
            }
            NodeType::StructDef => {}
            NodeType::EnumDef => {
                self.visit_enum(def_id, ctx)?;
            }
            NodeType::ImplDef => {
                self.typecheck_impl_body(def_id, ctx)?;
            }
            NodeType::TraitImplDef => {
                self.typecheck_trait_impl_body(def_id, ctx)?;
            }
            NodeType::TraitDef => {
                self.typecheck_trait_body(def_id, ctx)?;
            }
            NodeType::TypeAliasDef => {}
            NodeType::ConstDef => {}
            NodeType::UseDef => {}
            _ => std::unreachable!(),
        };
        ctx.pop_node_id();
        Ok(())
    }

    pub fn typecheck_function_signature(
        &mut self,
        def_id: DefId,
        _scope_kind: ScopeKind,
        generic_scope_kind: ScopeKind,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<()> {
        let function = ctx.definition(def_id).as_function().cloned().unwrap();
        let checked_def_id = self.unchecked_checked.get(&def_id.into()).unwrap().into_def().unwrap();
        let type_id = self.program[checked_def_id].as_function().unwrap().type_id;
        let scope_id = ctx.symbols[type_id].scope_id();

        ctx.symbols.enter_scope(scope_id);

        let mut checked_generic_parameters = Vec::with_capacity(function.generic_parameters.len());
        for generic_parameter in &function.generic_parameters {
            let checked_generic_parameter = self.typecheck_generic_parameter(generic_parameter, ctx)?;
            let type_id = ctx.symbols.add_type_variable(generic_scope_kind, checked_generic_parameter)?;
            checked_generic_parameters.push(type_id);
        }

        let mut parameters = Vec::with_capacity(function.parameters.len());

        for parameter in &function.parameters {
            let parameter_type = self.typecheck(&parameter.ty, ctx)?;
            let variable = CheckedVariable::new(parameter.name, parameter_type, parameter.qualifier, scope_id, parameter.location);
            let var_id = ctx.symbols.declare_variable(variable).ok_or(Error::VariableAlreadyDefined {
                location: function.location,
                variable: parameter.name.id,
            })?;
            ctx.add_variable_reference(var_id, parameter.name.location, false);
            parameters.push(CheckedFunctionParameter::new(
                parameter.name,
                parameter.qualifier,
                parameter_type,
                parameter.location,
            ));
        }

        let return_type = if let Some(ref ret) = function.return_type {
            self.typecheck(ret, ctx)?
        } else {
            VOID_TYPE
        };

        self.program.modify_definition(checked_def_id, |def: &mut CheckedDefinitionNode| {
            let checked_function_mut = def.as_function_mut().unwrap();
            checked_function_mut.parameters = parameters.clone();
            checked_function_mut.return_type = return_type;
            checked_function_mut.generic_parameters = checked_generic_parameters.clone();
            Ok(())
        })?;
        ctx.symbols.modify_type(type_id, |ty: &mut Type| {
            let ty_mut = ty.as_function_mut().unwrap();
            ty_mut.generic_parameters = checked_generic_parameters;
            ty_mut.parameters = parameters;
            ty_mut.return_type = return_type;
            Ok(())
        })?;

        self.register_instance(type_id, type_id, ctx)?;

        ctx.symbols.exit_scope();
        Ok(())
    }

    pub fn typecheck_function_body(&mut self, def_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let function = ctx.definition(def_id).as_function().cloned().unwrap();
        let checked_def_id = self.unchecked_checked.get(&def_id.into()).unwrap().into_def().unwrap();
        let type_id = self.program[checked_def_id].as_function().unwrap().type_id;
        let expected_return_type = self.program[checked_def_id].as_function().unwrap().return_type;

        ctx.symbols.enter_scope(ctx.symbols[type_id].scope_id());

        let checked_body = if let Some(body) = function.body {
            let checked_body = self.visit_expr(body, ctx)?;
            let actual_return_type = checked_body.ty();
            if !self.unify(expected_return_type, actual_return_type, ctx) {
                return Err(Error::TypeMismatch {
                    location: function.location,
                    expected: vec![expected_return_type],
                    found: actual_return_type,
                });
            }
            Some(self.program.exprs.alloc_item(checked_body))
        } else {
            None
        };

        self.program.modify_definition(checked_def_id, |def: &mut CheckedDefinitionNode| {
            let checked_function_mut = def.as_function_mut().unwrap();
            checked_function_mut.body = checked_body;
            Ok(())
        })?;
        ctx.symbols.modify_type(type_id, |ty: &mut Type| {
            let ty_mut = ty.as_function_mut().unwrap();
            ty_mut.body = checked_body;
            Ok(())
        })?;

        ctx.symbols.exit_scope();
        Ok(())
    }

    pub fn typecheck_trait_body(&mut self, def_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let trait_node = ctx.definition(def_id).as_trait().cloned().unwrap();
        let checked_def_id = self.unchecked_checked.get(&def_id.into()).unwrap().into_def().unwrap();
        let checked_trait_node = self.program[checked_def_id].as_trait().cloned().unwrap();
        let type_id = checked_trait_node.type_id;

        ctx.symbols.enter_scope(ctx.symbols[type_id].scope_id());

        for &function_id in &trait_node.body {
            ctx.push_node_id(NodeId::from(function_id));
            self.infcx.enter_scope();
            self.typecheck_function_body(function_id, ctx)?;
            self.infcx.exit_scope();
            ctx.pop_node_id();
        }

        ctx.symbols.exit_scope();
        Ok(())
    }

    pub fn typecheck_impl_body(&mut self, def_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let impl_node = ctx.definition(def_id).as_impl().cloned().unwrap();
        let checked_def_id = self.unchecked_checked.get(&def_id.into()).unwrap().into_def().unwrap();
        let checked_impl_node = self.program[checked_def_id].as_impl().cloned().unwrap();

        ctx.symbols.enter_scope(checked_impl_node.scope_id);

        for &function_id in &impl_node.body {
            ctx.push_node_id(NodeId::from(function_id));
            self.infcx.enter_scope();
            self.typecheck_function_body(function_id, ctx)?;
            self.infcx.exit_scope();
            ctx.pop_node_id();
        }

        ctx.symbols.exit_scope();
        Ok(())
    }

    pub fn typecheck_trait_impl_body(&mut self, def_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let trait_impl_node = ctx.definition(def_id).as_trait_impl().cloned().unwrap();
        let checked_def_id = self.unchecked_checked.get(&def_id.into()).unwrap().into_def().unwrap();
        let checked_trait_impl_node = self.program[checked_def_id].as_trait_impl().cloned().unwrap();

        ctx.symbols.enter_scope(checked_trait_impl_node.scope_id);

        for &function_id in &trait_impl_node.body {
            ctx.push_node_id(NodeId::from(function_id));
            self.infcx.enter_scope();
            self.typecheck_function_body(function_id, ctx)?;
            self.infcx.exit_scope();
            ctx.pop_node_id();
        }

        ctx.symbols.exit_scope();
        Ok(())
    }
}
