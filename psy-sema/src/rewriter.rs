use itertools::Itertools;
use psy_ast::{DefId, ExprId, StmtId};
use psy_vm::dpn::ops::context_trait::ContextFelt;
use tracing::instrument;

use crate::{
    CheckedDefinitionNode, CheckedExprNode, CheckedIntrinsicExprNode, CheckedIntrinsicStmtNode, CheckedStmtNode, CheckedValueNode, Error,
    Implementer, Result, Type, TypeChecker, TypeCheckerVisitorContext, TypeId,
};

pub trait Rewriter<F: Clone + From<u32> + ContextFelt, C> {
    fn instantiate_impl(&mut self, impl_id: DefId, generic_parameters: Vec<TypeId>, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId>;
    fn instantiate_trait_impl(
        &mut self,
        impl_id: DefId,
        trait_generic_parameters: Vec<TypeId>,
        generic_parameters: Vec<TypeId>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<DefId>;
    fn instantiate_trait(&mut self, type_id: TypeId, generic_parameters: Vec<TypeId>, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId>;
    fn instantiate_function(&mut self, type_id: TypeId, generic_parameters: Vec<TypeId>, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId>;
    fn instantiate_function_signature(
        &mut self,
        function_id: DefId,
        generic_parameters: Vec<TypeId>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<DefId>;
    fn instantiate_function_body(&mut self, function_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId>;
    fn rewrite_stmt(&mut self, stmt_id: StmtId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<StmtId>;
    fn rewrite_expr(&mut self, expr_id: ExprId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<ExprId>;
}

impl<F: Clone + From<u32> + ContextFelt, C> Rewriter<F, C> for TypeChecker<F, C> {
    #[instrument(level = "debug", skip_all)]
    fn instantiate_impl(&mut self, impl_id: DefId, generic_parameters: Vec<TypeId>, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId> {
        let mut checked_impl = self.program[impl_id].as_impl().cloned().unwrap();
        self.infcx.enter_context();

        for (generic_parameter, generic_arg) in ctx.symbols[checked_impl.ty]
            .generic_parameters()
            .iter()
            .zip_eq(generic_parameters.into_iter())
        {
            if !self.unify(generic_parameter.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: checked_impl.location,
                    expected: vec![generic_parameter.clone()],
                    found: generic_arg,
                });
            }
        }

        checked_impl.ty = self.substitute_all(checked_impl.ty, ctx)?;

        for (_name, associated_type) in &mut checked_impl.associated_types {
            associated_type.type_id = if let Some((ref mut root, target)) = associated_type.root.zip(associated_type.target) {
                *root = self.substitute_all(*root, ctx)?;
                self.find_associated_type(*root, None, target, ctx)?
            } else {
                self.substitute_all(associated_type.type_id, ctx)?
            };
        }

        for method in &mut checked_impl.body {
            *method = self.instantiate_function_signature(*method, vec![], ctx)?;
        }

        let impl_id = self.program.defs.alloc_item(CheckedDefinitionNode::Impl(checked_impl.clone()));
        self.register_impl(impl_id, ctx)?;

        for method in &checked_impl.body {
            self.instantiate_function_body(*method, ctx)?;
        }

        self.infcx.exit_context();
        Ok(impl_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn instantiate_trait_impl(
        &mut self,
        impl_id: DefId,
        trait_generic_parameters: Vec<TypeId>,
        generic_parameters: Vec<TypeId>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<DefId> {
        let mut checked_impl = self.program[impl_id].as_trait_impl().cloned().unwrap();
        self.infcx.enter_context();

        for (generic_parameter, generic_arg) in ctx.symbols[checked_impl.ty]
            .generic_parameters()
            .iter()
            .zip_eq(generic_parameters.into_iter())
        {
            if !self.unify(generic_parameter.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: checked_impl.location,
                    expected: vec![generic_parameter.clone()],
                    found: generic_arg,
                });
            }
        }

        for (generic_parameter, generic_arg) in ctx.symbols[checked_impl.trait_ty]
            .generic_parameters()
            .iter()
            .zip_eq(trait_generic_parameters.into_iter())
        {
            if !self.unify(generic_parameter.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: checked_impl.location,
                    expected: vec![generic_parameter.clone()],
                    found: generic_arg,
                });
            }
        }

        checked_impl.ty = self.substitute_all(checked_impl.ty, ctx)?;

        for (name, associated_type) in &mut checked_impl.associated_types {
            let type_id = if let Some((ref mut root, target)) = associated_type.root.zip(associated_type.target) {
                *root = self.substitute_all(*root, ctx)?;
                self.find_associated_type(*root, None, target, ctx)?
            } else {
                self.substitute_all(associated_type.type_id, ctx)?
            };

            let generic_associated_type = ctx.symbols[checked_impl.trait_ty]
                .as_trait()
                .unwrap()
                .associated_types
                .get(name)
                .unwrap()
                .type_id;

            if !self.unify(generic_associated_type, type_id, ctx) {
                return Err(Error::TypeMismatch {
                    location: associated_type.location,
                    expected: vec![associated_type.type_id.clone()],
                    found: type_id,
                });
            }

            associated_type.type_id = type_id;
        }

        checked_impl.trait_ty = self.substitute_all(checked_impl.trait_ty, ctx)?;

        for method in &mut checked_impl.body {
            *method = self.instantiate_function_signature(*method, vec![], ctx)?;
        }

        let impl_id = self.program.defs.alloc_item(CheckedDefinitionNode::TraitImpl(checked_impl.clone()));
        self.register_trait_impl(impl_id, ctx)?;

        for method in &checked_impl.body {
            self.instantiate_function_body(*method, ctx)?;
        }

        self.infcx.exit_context();
        Ok(impl_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn instantiate_trait(&mut self, poly_ty: TypeId, generic_parameters: Vec<TypeId>, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId> {
        // assuming type_id is the poly type
        let mut checked_trait = ctx.symbols[poly_ty].as_trait().cloned().unwrap();
        self.infcx.enter_context();

        for (generic_parameter, generic_arg) in ctx.symbols[checked_trait.type_id]
            .generic_parameters()
            .iter()
            .zip_eq(generic_parameters.into_iter())
        {
            if !self.unify(generic_parameter.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: checked_trait.location,
                    expected: vec![generic_parameter.clone()],
                    found: generic_arg,
                });
            }
        }

        for generic_parameter in &mut checked_trait.generic_parameters {
            *generic_parameter = self.substitute_all(*generic_parameter, ctx)?;
        }

        for (_name, associated_type) in &mut checked_trait.associated_types {
            associated_type.type_id = self.substitute_all(associated_type.type_id, ctx)?;
        }

        for method in &mut checked_trait.body {
            *method = self.instantiate_function_signature(*method, vec![], ctx)?;
        }

        checked_trait.type_id = ctx.symbols.next_type_id(0);

        let ty = Type::Trait(checked_trait.clone());
        let type_id = ctx.symbols.add_type(ctx.symbols[checked_trait.scope_id].parent, ty.key(), ty)?;

        let trait_id = self.program.defs.alloc_item(CheckedDefinitionNode::Trait(checked_trait.clone()));
        self.register_instance(type_id, poly_ty, ctx)?;

        for method in &checked_trait.body {
            self.instantiate_function_body(*method, ctx)?;
        }

        self.infcx.exit_context();
        Ok(trait_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn instantiate_function(&mut self, poly_ty: TypeId, generic_parameters: Vec<TypeId>, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId> {
        // assuming type_id is the poly type
        let mut checked_function = ctx.symbols[poly_ty].as_function().cloned().unwrap();
        self.infcx.enter_scope();

        for (generic_parameter, generic_arg) in ctx.symbols[checked_function.type_id]
            .generic_parameters()
            .iter()
            .zip(generic_parameters.into_iter())
        {
            if !self.unify(generic_parameter.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: checked_function.location,
                    expected: vec![generic_parameter.clone()],
                    found: generic_arg,
                });
            }
        }

        if let Some(ref mut body) = checked_function.body {
            *body = self.rewrite_expr(*body, ctx)?;
        }
        for generic_parameter in &mut checked_function.generic_parameters {
            *generic_parameter = self.substitute_all(*generic_parameter, ctx)?;
        }
        for parameter in &mut checked_function.parameters {
            parameter.ty = self.substitute_all(parameter.ty, ctx)?;
        }
        checked_function.return_type = self.substitute_all(checked_function.return_type, ctx)?;
        checked_function.type_id = ctx.symbols.next_type_id(0);

        let ty = Type::Function(checked_function.clone());
        let type_id = ctx.symbols.create_type(ty)?;

        let function_id = self.program.defs.alloc_item(CheckedDefinitionNode::Function(checked_function));
        self.register_instance(type_id, poly_ty, ctx)?;

        self.infcx.exit_scope();
        Ok(function_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn instantiate_function_signature(
        &mut self,
        function_id: DefId,
        generic_parameters: Vec<TypeId>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<DefId> {
        let mut checked_function = self.program[function_id].as_function().cloned().unwrap();
        let poly_ty = self.poly_of(checked_function.type_id, ctx).unwrap();
        self.infcx.enter_scope();

        for (generic_parameter, generic_arg) in ctx.symbols[checked_function.type_id]
            .generic_parameters()
            .iter()
            .zip(generic_parameters.into_iter())
        {
            if !self.unify(generic_parameter.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: checked_function.location,
                    expected: vec![generic_parameter.clone()],
                    found: generic_arg,
                });
            }
        }

        for generic_parameter in &mut checked_function.generic_parameters {
            *generic_parameter = self.substitute_all(*generic_parameter, ctx)?;
        }
        for parameter in &mut checked_function.parameters {
            parameter.ty = self.substitute_all(parameter.ty, ctx)?;
        }
        checked_function.return_type = self.substitute_all(checked_function.return_type, ctx)?;
        checked_function.type_id = ctx.symbols.next_type_id(0);

        let ty = Type::Function(checked_function.clone());
        let type_id = ctx.symbols.create_type(ty)?;

        let function_id = self.program.defs.alloc_item(CheckedDefinitionNode::Function(checked_function));
        self.register_instance(type_id, poly_ty, ctx)?;

        self.infcx.exit_scope();
        Ok(function_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn instantiate_function_body(&mut self, function_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<DefId> {
        let mut checked_function = self.program[function_id].as_function().cloned().unwrap();
        self.infcx.enter_scope();

        if let Some(ref mut body) = checked_function.body {
            *body = self.rewrite_expr(*body, ctx)?;
        }

        self.program.modify_definition(function_id, |def: &mut CheckedDefinitionNode| {
            def.as_function_mut().unwrap().body = checked_function.body;
            Ok(())
        })?;
        ctx.symbols.modify_type(checked_function.type_id, |ty: &mut Type| {
            ty.as_function_mut().unwrap().body = checked_function.body;
            Ok(())
        })?;

        self.infcx.exit_scope();
        Ok(function_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn rewrite_stmt(&mut self, stmt_id: StmtId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<StmtId> {
        if !self.infcx.has_equations() {
            return Ok(stmt_id);
        }

        let mut checked_stmt = self.program[stmt_id].clone();
        match &mut checked_stmt {
            CheckedStmtNode::While(checked_while_node) => {
                checked_while_node.type_id = self.substitute_all(checked_while_node.type_id, ctx)?;
                checked_while_node.predicate = self.rewrite_expr(checked_while_node.predicate, ctx)?;
                checked_while_node.body = self.rewrite_expr(checked_while_node.body, ctx)?;
            }
            CheckedStmtNode::For(checked_for_node) => {
                checked_for_node.start = self.rewrite_expr(checked_for_node.start, ctx)?;
                checked_for_node.end = self.rewrite_expr(checked_for_node.end, ctx)?;
                checked_for_node.body = self.rewrite_expr(checked_for_node.body, ctx)?;
            }
            CheckedStmtNode::Assignment(checked_assignment_node) => {
                checked_assignment_node.type_id = self.substitute_all(checked_assignment_node.type_id, ctx)?;
                checked_assignment_node.target = self.rewrite_expr(checked_assignment_node.target, ctx)?;
                checked_assignment_node.value = self.rewrite_expr(checked_assignment_node.value, ctx)?;
            }
            CheckedStmtNode::Variable(checked_variable_node) => {
                checked_variable_node.ty = self.substitute_all(checked_variable_node.ty, ctx)?;
                checked_variable_node.value = self.rewrite_expr(checked_variable_node.value, ctx)?;
            }
            CheckedStmtNode::Definition(_def_id) => {}
            CheckedStmtNode::Expression(expr_id) => {
                *expr_id = self.rewrite_expr(*expr_id, ctx)?;
            }
            CheckedStmtNode::Return(checked_return_node) => {
                if let Some(ret) = checked_return_node.ret {
                    checked_return_node.ret = Some(self.rewrite_expr(ret, ctx)?);
                }
            }
            CheckedStmtNode::Intrinsic(checked_intrinsic_stmt_node) => match checked_intrinsic_stmt_node {
                CheckedIntrinsicStmtNode::Assert { left, .. } => {
                    *left = self.rewrite_expr(*left, ctx)?;
                }
                CheckedIntrinsicStmtNode::AssertEq { left, right, .. } => {
                    *left = self.rewrite_expr(*left, ctx)?;
                    *right = self.rewrite_expr(*right, ctx)?;
                }
                CheckedIntrinsicStmtNode::ClearEntireTree { .. } => {}
            },
        }

        Ok(self.program.stmts.alloc_item(checked_stmt))
    }

    #[instrument(level = "debug", skip_all)]
    fn rewrite_expr(&mut self, expr_id: ExprId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<ExprId> {
        if !self.infcx.has_equations() {
            return Ok(expr_id);
        }

        let mut checked_expr = self.program[expr_id].clone();
        match &mut checked_expr {
            CheckedExprNode::Path(checked_path_node) => {
                if let Some(ref mut trait_ty) = checked_path_node.trait_ty {
                    *trait_ty = self.substitute_all(*trait_ty, ctx)?;
                }

                if let Some((ref mut root, target)) = checked_path_node.root.zip(checked_path_node.target) {
                    *root = self.substitute_all(*root, ctx)?;
                    checked_path_node.type_id = self.find_member(*root, None, target, ctx)?;
                } else {
                    checked_path_node.type_id = self.substitute_all(checked_path_node.type_id, ctx)?;
                }
            }
            CheckedExprNode::Value(checked_value_node) => match checked_value_node {
                CheckedValueNode::Felt(_, _location) => {}
                CheckedValueNode::Bool(_, _location) => {}
                CheckedValueNode::U32(_, _location) => {}
                CheckedValueNode::Array(type_id, vec, _location) => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                    for value in vec {
                        *value = self.rewrite_expr(*value, ctx)?;
                    }
                }
                CheckedValueNode::Tuple(type_id, vec, _location) => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                    for value in vec {
                        value.0 = self.substitute_all(value.0, ctx)?;
                        value.1 = self.rewrite_expr(value.1, ctx)?;
                    }
                }
                CheckedValueNode::Struct(type_id, index_map, _location) => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                    for (_index, value) in index_map {
                        *value = self.rewrite_expr(*value, ctx)?;
                    }
                }
                CheckedValueNode::Type(type_id) => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
            },
            CheckedExprNode::Binary(checked_binary_node) => {
                checked_binary_node.type_id = self.substitute_all(checked_binary_node.type_id, ctx)?;
                checked_binary_node.lhs = self.rewrite_expr(checked_binary_node.lhs, ctx)?;
                checked_binary_node.rhs = self.rewrite_expr(checked_binary_node.rhs, ctx)?;
            }
            CheckedExprNode::Unary(checked_unary_node) => {
                checked_unary_node.type_id = self.substitute_all(checked_unary_node.type_id, ctx)?;
                checked_unary_node.rhs = self.rewrite_expr(checked_unary_node.rhs, ctx)?;
            }
            CheckedExprNode::Cast(checked_cast_node) => {
                checked_cast_node.value = self.rewrite_expr(checked_cast_node.value, ctx)?;
                checked_cast_node.target_type = self.substitute_all(checked_cast_node.target_type, ctx)?;
            }
            CheckedExprNode::Call(checked_call_node) => {
                checked_call_node.type_id = self.substitute_all(checked_call_node.type_id, ctx)?;
                checked_call_node.callee = self.rewrite_expr(checked_call_node.callee, ctx)?;
                for arg in &mut checked_call_node.args {
                    *arg = self.rewrite_expr(*arg, ctx)?;
                }
                for generic_parameter in &mut checked_call_node.generic_parameters {
                    *generic_parameter = self.substitute_all(*generic_parameter, ctx)?;
                }
            }
            CheckedExprNode::MemberCall(checked_member_call_node) => {
                checked_member_call_node.type_id = self.substitute_all(checked_member_call_node.type_id, ctx)?;
                checked_member_call_node.callee = self.rewrite_expr(checked_member_call_node.callee, ctx)?;
                checked_member_call_node.receiver = self.rewrite_expr(checked_member_call_node.receiver, ctx)?;
                for arg in &mut checked_member_call_node.args {
                    *arg = self.rewrite_expr(*arg, ctx)?;
                }
                for generic_parameter in &mut checked_member_call_node.generic_parameters {
                    *generic_parameter = self.substitute_all(*generic_parameter, ctx)?;
                }
            }
            CheckedExprNode::IndexAccess(checked_index_access_node) => {
                checked_index_access_node.type_id = self.substitute_all(checked_index_access_node.type_id, ctx)?;
                checked_index_access_node.index = self.rewrite_expr(checked_index_access_node.index, ctx)?;
                checked_index_access_node.target = self.rewrite_expr(checked_index_access_node.target, ctx)?;
            }
            CheckedExprNode::TupleAccess(checked_tuple_access_node) => {
                checked_tuple_access_node.type_id = self.substitute_all(checked_tuple_access_node.type_id, ctx)?;
                checked_tuple_access_node.target = self.rewrite_expr(checked_tuple_access_node.target, ctx)?;
            }
            CheckedExprNode::MemberAccess(checked_member_access_node) => {
                checked_member_access_node.type_id = self.substitute_all(checked_member_access_node.type_id, ctx)?;
                checked_member_access_node.target = self.rewrite_expr(checked_member_access_node.target, ctx)?;
            }
            CheckedExprNode::Intrinsic(checked_intrinsic_expr_node) => match checked_intrinsic_expr_node {
                CheckedIntrinsicExprNode::GetUserId { type_id, .. } => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetContractId { type_id, .. } => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetContractDeployer { contract_id, type_id, .. } => {
                    *contract_id = self.rewrite_expr(*contract_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetCallerContractId { type_id, .. } => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetCheckpointId { type_id, .. } => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetLastNonce { type_id, .. } => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetUserPublicKeyHash { type_id, .. } => {
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetStateHashAt { slot_index, type_id, .. } => {
                    *slot_index = self.rewrite_expr(*slot_index, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetOtherContractStateHashAt {
                    contract_state_tree_height,
                    contract_id,
                    slot_index,
                    type_id,
                    ..
                } => {
                    *contract_state_tree_height = self.rewrite_expr(*contract_state_tree_height, ctx)?;
                    *contract_id = self.rewrite_expr(*contract_id, ctx)?;
                    *slot_index = self.rewrite_expr(*slot_index, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetOtherUserContractStateHashAt {
                    contract_state_tree_height,
                    user_id,
                    contract_id,
                    slot_index,
                    type_id,
                    ..
                } => {
                    *contract_state_tree_height = self.rewrite_expr(*contract_state_tree_height, ctx)?;
                    *user_id = self.rewrite_expr(*user_id, ctx)?;
                    *contract_id = self.rewrite_expr(*contract_id, ctx)?;
                    *slot_index = self.rewrite_expr(*slot_index, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::CSetStateHashAt {
                    slot_index,
                    new_value,
                    type_id,
                    ..
                } => {
                    *new_value = self.rewrite_expr(*new_value, ctx)?;
                    *slot_index = self.rewrite_expr(*slot_index, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::StorageRead { offset, type_id, .. } => {
                    *offset = self.rewrite_expr(*offset, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::StorageWrite { offset, value, type_id, .. } => {
                    *offset = self.rewrite_expr(*offset, ctx)?;
                    *value = self.rewrite_expr(*value, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::Hash { data, type_id, .. } => {
                    *data = self.rewrite_expr(*data, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::HashTwoToOne { left, right, type_id, .. } => {
                    *left = self.rewrite_expr(*left, ctx)?;
                    *right = self.rewrite_expr(*right, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::MemTransmute { data, target_type, .. } => {
                    *data = self.rewrite_expr(*data, ctx)?;
                    *target_type = self.substitute_all(*target_type, ctx)?;
                }
                CheckedIntrinsicExprNode::MemSizeOf { query_type: ty, type_id, .. } => {
                    *ty = self.substitute_all(*ty, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::StorageReadRange { offset, length, type_id, .. } => {
                    *offset = self.rewrite_expr(*offset, ctx)?;
                    *length = self.rewrite_expr(*length, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::StorageWriteRange { offset, values, type_id, .. } => {
                    *offset = self.rewrite_expr(*offset, ctx)?;
                    *values = self.rewrite_expr(*values, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::InvokeSync {
                    contract_id,
                    method_id,
                    inputs,
                    type_id,
                    location,
                } => {
                    *contract_id = self.rewrite_expr(*contract_id, ctx)?;
                    *method_id = self.rewrite_expr(*method_id, ctx)?;
                    *inputs = self.rewrite_expr(*inputs, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::InvokeDeferred {
                    contract_id,
                    method_id,
                    inputs,
                    type_id,
                    location,
                } => {
                    *contract_id = self.rewrite_expr(*contract_id, ctx)?;
                    *method_id = self.rewrite_expr(*method_id, ctx)?;
                    *inputs = self.rewrite_expr(*inputs, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::Secp256k1Verify {
                    pub_key,
                    msg,
                    sig,
                    type_id,
                    location,
                } => {
                    *pub_key = self.rewrite_expr(*pub_key, ctx)?;
                    *msg = self.rewrite_expr(*msg, ctx)?;
                    *sig = self.rewrite_expr(*sig, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::SumBits { bits, type_id, location } => {
                    *bits = self.rewrite_expr(*bits, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::SplitBits {
                    target,
                    num_bits,
                    type_id,
                    location,
                } => {
                    *target = self.rewrite_expr(*target, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetCheckpointStats {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetRegisterUsersRoot {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetGutasRoot {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetDeployContractsRoot {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetFeesCollected {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetUserOpsProcessed {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetTotalTransactions {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetSlotsModified {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetRegisterUsersCompleted {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetGutasCompleted {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
                CheckedIntrinsicExprNode::GetDeployContractsCompleted {
                    checkpoint_id,
                    type_id,
                    location,
                } => {
                    *checkpoint_id = self.rewrite_expr(*checkpoint_id, ctx)?;
                    *type_id = self.substitute_all(*type_id, ctx)?;
                }
            },
            CheckedExprNode::LambdaFunction(_) => {}
            CheckedExprNode::BlockExpr(checked_block_expr_node) => {
                checked_block_expr_node.type_id = self.substitute_all(checked_block_expr_node.type_id, ctx)?;
                for stmt in &mut checked_block_expr_node.stmts {
                    *stmt = self.rewrite_stmt(*stmt, ctx)?;
                }
                if let Some(expr) = checked_block_expr_node.expr {
                    checked_block_expr_node.expr = Some(self.rewrite_expr(expr, ctx)?);
                }
            }
            CheckedExprNode::IfExpr(checked_if_expr_node) => {
                checked_if_expr_node.type_id = self.substitute_all(checked_if_expr_node.type_id, ctx)?;

                checked_if_expr_node.if_branch.type_id = self.substitute_all(checked_if_expr_node.if_branch.type_id, ctx)?;
                checked_if_expr_node.if_branch.predicate = self.rewrite_expr(checked_if_expr_node.if_branch.predicate, ctx)?;
                checked_if_expr_node.if_branch.body = self.rewrite_expr(checked_if_expr_node.if_branch.body, ctx)?;

                for elseif_branch in &mut checked_if_expr_node.elseif_branches {
                    elseif_branch.type_id = self.substitute_all(elseif_branch.type_id, ctx)?;
                    elseif_branch.predicate = self.rewrite_expr(elseif_branch.predicate, ctx)?;
                    elseif_branch.body = self.rewrite_expr(elseif_branch.body, ctx)?;
                }

                if let Some(else_branch) = checked_if_expr_node.else_branch {
                    checked_if_expr_node.else_branch = Some(self.rewrite_expr(else_branch, ctx)?);
                }
            }
            CheckedExprNode::Match(checked_match_node) => {
                checked_match_node.type_id = self.substitute_all(checked_match_node.type_id, ctx)?;
                checked_match_node.value = self.rewrite_expr(checked_match_node.value, ctx)?;
                for case in &mut checked_match_node.cases {
                    if let Some(pattern) = &mut case.pattern {
                        *pattern = self.rewrite_expr(*pattern, ctx)?;
                    }
                    case.body = self.rewrite_expr(case.body, ctx)?;
                }
            }
        }

        Ok(self.program.exprs.alloc_item(checked_expr))
    }
}
