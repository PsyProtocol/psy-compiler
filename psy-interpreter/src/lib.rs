#![feature(try_trait_v2)]

mod control;
pub mod error;
mod preprocess;

use std::{collections::HashMap, iter::once, path::PathBuf};

use error::{Error, Result};
use indexmap::IndexMap;
pub use preprocess::StorageProcessor;
use psy_ast::*;
use psy_common::Graph;
use psy_crypto::hash::utils::gen_dapen_contract_function_method_id;
use psy_fmt::Formatter;
use psy_parser::Parser;
use psy_sema::{Error as SemaError, *};
use psy_vm::dpn::{
    ops::{
        context_trait::{ContextFelt, DPNContext, ToFelts},
        exec_context::QExecContext,
        op_types::DPNOpType,
        sym_felt::SymFeltRef,
    },
    vm::{compile::PsyCompileResult, def::DPNFunctionCircuitDefinition},
};
use tracing::instrument;

use crate::{
    control::ControlState,
    error::{lowering_parse_error, lowering_sema_error, parse_error_to_diagnostic, typecheck_error_to_diagnostic},
};

pub struct InterpretResult {
    pub compile_results: Vec<DPNFunctionCircuitDefinition>,
    pub typechecker: TypeChecker<SymFeltRef, QExecContext>,
    pub ctx: TypeCheckerVisitorContext<SymFeltRef, QExecContext>,
}

pub fn interpret(contract_name: Option<String>, method_names: Vec<String>, crate_path_graph: Graph<PathBuf>) -> anyhow::Result<InterpretResult> {
    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let (mut typechecker, mut ctx) = interpreter.typecheck(crate_path_graph)?;

    let compile_results = interpreter.interpret(
        &mut typechecker,
        &mut ctx,
        contract_name,
        method_names,
        |context, (method_name, method_id, outputs)| PsyCompileResult::compile_exec(method_name, method_id, &context.store, context, &outputs),
    )?;

    Ok(InterpretResult {
        compile_results,
        typechecker,
        ctx,
    })
}

#[derive(Clone, Debug)]
pub struct Interpreter<F: Clone + From<u32>, C> {
    pub context: C,
    _marker: std::marker::PhantomData<F>,
}

impl<F: ContextFelt + From<u32>, C: DPNContext<F> + 'static> Evaluator<F, C> for Interpreter<F, C> {
    fn evaluate_expr(
        &mut self,
        program: &CheckedProgram<F>,
        expr: &CheckedExprNode<F>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> CheckedValueRef<F> {
        self.__interpret_expr__(program, expr, ctx).unwrap()
    }

    fn to_constant(&mut self, value: F) -> u64 {
        self.context.get_constant_value(value)
    }

    fn from_constant(&mut self, value: ConstValue) -> F {
        match value {
            ConstValue::Felt(value) => self.context.op_const(value),
            ConstValue::U32(value) => self.context.op_const_u32(value),
            ConstValue::Bool(value) => {
                if value {
                    self.context.op_true()
                } else {
                    self.context.op_false()
                }
            }
        }
    }
}

impl<F: ContextFelt + From<u32>, C: DPNContext<F> + 'static> Interpreter<F, C> {
    pub fn new(context: C) -> Self {
        Self {
            context,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn calculate_type_size(&mut self, type_id: TypeId, ctx: &TypeCheckerVisitorContext<F, C>) -> usize {
        match &ctx.symbols[type_id] {
            Type::Felt | Type::Bool | Type::U32 => 1,
            Type::Struct(s) => s.fields.iter().map(|(_, field)| self.calculate_type_size(field.ty, ctx)).sum(),
            Type::Array(arr) => {
                let element_size = self.calculate_type_size(arr.inner_ty, ctx);

                let array_size = match &ctx.symbols[arr.size_ty] {
                    Type::Const(const_node) => {
                        let const_value_ref = &ctx.symbols[const_node.value];
                        match &*const_value_ref.borrow() {
                            CheckedValue::Felt(f) => self.context.get_constant_value(f.clone()) as usize,
                            CheckedValue::U32(f) => self.context.get_constant_value(f.clone()) as usize,
                            _ => panic!("Array size must be a numeric constant"),
                        }
                    }
                    _ => panic!("Array size must be a const type"),
                };

                element_size * array_size
            }
            Type::Tuple(elements) => elements.iter().map(|&ty| self.calculate_type_size(ty, ctx)).sum(),
            _ => panic!("Unsupported type for size calculation: {:?}", ctx.symbols[type_id]),
        }
    }

    pub fn to_input(&mut self, ty: TypeId, symbols: &SymbolTable<F>) -> CheckedValue<F> {
        match symbols[ty].clone() {
            Type::Felt => CheckedValue::Felt(self.context.add_input()),
            Type::Bool => CheckedValue::Bool(self.context.add_bool_input()),
            Type::U32 => CheckedValue::U32(self.context.add_u32_input()),
            Type::Tuple(elements) => {
                let mut result = Vec::new();
                for element_type in elements {
                    let value = CheckedValueRef::new_rc(self.to_input(element_type, symbols));

                    result.push((element_type, value));
                }
                let type_id = symbols.get_type_id(Some(ScopeId::primitive()), symbols[ty].key()).unwrap();

                CheckedValue::Tuple { type_id, elements: result }
            }
            Type::Struct(s) => {
                let mut result = IndexMap::new();
                for (field_name, field) in &s.fields {
                    result.insert(field_name.clone(), CheckedValueRef::new_rc(self.to_input(field.ty.clone(), symbols)));
                }
                let type_id = symbols.get_type_id(Some(s.scope_id), symbols[ty].key()).unwrap();
                CheckedValue::Struct(type_id, result)
            }
            Type::Array(arr) => {
                let size = match &symbols[arr.size_ty] {
                    Type::Const(const_node) => {
                        let const_value_ref = &symbols[const_node.value];
                        match &*const_value_ref.borrow() {
                            CheckedValue::Felt(f) => self.context.get_constant_value(f.clone()) as usize,
                            CheckedValue::U32(f) => self.context.get_constant_value(f.clone()) as usize,
                            _ => panic!("Array size must be a numeric constant"),
                        }
                    }
                    _ => panic!("Array size must be a const type"),
                };

                let mut elements = Vec::new();
                for _ in 0..size {
                    elements.push(CheckedValueRef::new_rc(self.to_input(arr.inner_ty.clone(), symbols)));
                }

                let type_id = symbols.get_type_id(Some(arr.scope_id), symbols[ty].key()).unwrap();

                CheckedValue::Array(type_id, elements)
            }
            other => panic!("Unsupported type in to_input: {:?}", other),
        }
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret<I: Into<Ident>>(
        &mut self,
        typechecker: &mut TypeChecker<F, C>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
        contract_name: Option<I>,
        method_names: Vec<I>,
        compile_fn: impl Fn(&C, (String, u32, Vec<F>)) -> DPNFunctionCircuitDefinition,
    ) -> anyhow::Result<Vec<DPNFunctionCircuitDefinition>>
    where
        F: 'static,
    {
        let scope_id = ctx.symbols[ModuleId::root()].scope_id;
        let type_ids = if let Some(contract_name) = contract_name {
            let contract_name = ctx.intern(contract_name.into());
            let type_id = ctx.symbols[scope_id]
                .types
                .get::<TypeKey>(&contract_name.into())
                .ok_or(Error::UndefinedFunction)?
                .clone();

            method_names
                .into_iter()
                .map(|method_name| {
                    let method_name = ctx.intern(method_name.into());
                    Ok(typechecker.find_member(type_id, None, method_name, ctx)?)
                })
                .collect::<Result<Vec<TypeId>>>()?
        } else {
            method_names
                .into_iter()
                .map(|method_name| {
                    let method_name = ctx.intern(method_name.into());
                    ctx.symbols[scope_id]
                        .types
                        .get::<TypeKey>(&method_name.into())
                        .ok_or(Error::UndefinedFunction)
                        .cloned()
                })
                .collect::<Result<Vec<TypeId>>>()?
        };

        let mut outputs = Vec::new();
        // backup context
        let context = self.context.clone();

        for type_id in type_ids {
            let node: &CheckedFunctionNode = ctx.symbols[type_id.clone()].as_function().unwrap();

            let mut parameters = vec![];
            for parameter in node.parameters.iter() {
                parameters.push(CheckedValueRef::new_rc(self.to_input(parameter.ty, &ctx.symbols)));
            }
            let res = self.__interpret__(&typechecker.program, type_id, parameters, ctx)?;
            outputs.push(compile_fn(&self.context, res));

            // restore context
            self.context = context.clone();
        }

        Ok(outputs)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn test(
        &mut self,
        typechecker: &mut TypeChecker<F, C>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
        compile_fn: impl Fn(&C, (String, u32, Vec<F>)) -> DPNFunctionCircuitDefinition,
    ) -> anyhow::Result<Vec<DPNFunctionCircuitDefinition>>
    where
        F: 'static,
    {
        let mut type_ids = Vec::new();
        ctx.program.dependency_graph.clone().ts::<SemaError>(&mut |&crate_id| {
            // Visit the entry module and all its children for this crate
            let entry_module_id = ModuleId::from(crate_id);
            self.collect_test_functions_from_module_tree(entry_module_id, ctx, &mut type_ids)
                .map_err(|e| match e {
                    Error::SemaError(se) => se,
                    _ => panic!("Unexpected error type in collect_test_functions_from_module_tree"),
                })?;
            Ok(())
        })?;

        let mut outputs = Vec::new();
        // backup context
        let context = self.context.clone();
        for (_, type_ids) in type_ids {
            for type_id in type_ids {
                assert!(ctx.symbols[type_id].as_function().unwrap().parameters.is_empty());

                // Check if the function has a should_panic attribute
                let func = ctx.symbols[type_id].as_function().unwrap();
                let should_panic = func.attrs.iter().find(|attr| attr.is_should_panic()).is_some();
                let func_name = ctx.ident(func.name).to_string();

                // Run the test function
                let result = self.__interpret__(&typechecker.program, type_id, vec![], ctx);

                match (result, should_panic) {
                    // Successful test when not expecting panic
                    (Ok(res), false) => {
                        outputs.push(compile_fn(&self.context, res));
                    }
                    // Failed test when not expecting panic - this is an error
                    (Err(err), false) => {
                        panic!("Test {func_name} failed with error: {err:?}",);
                    }
                    // Successful test when expecting panic - this is an error
                    (Ok(_), true) => {
                        panic!("Test {func_name} passed: Expected panic but test completed successfully");
                    }
                    // Failed test when expecting panic - this is success!
                    (Err(_), true) => {
                        println!("Test {func_name} passed: Panicked as expected")
                    }
                }

                // restore context
                self.context = context.clone();
            }
        }

        Ok(outputs)
    }

    fn collect_test_functions_from_module_tree(
        &self,
        module_id: ModuleId,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
        type_ids: &mut Vec<(ModuleId, Vec<TypeId>)>,
    ) -> Result<()> {
        // Collect test functions from current module
        let scope_id = ctx.symbols[module_id].scope_id;
        let functions = ctx.symbols[scope_id]
            .types
            .iter()
            .filter(|(_, &v)| {
                ctx.symbols[v.clone()].is_function()
                    && ctx.symbols[v.clone()]
                        .as_function()
                        .map(|x| x.attrs.iter().any(|y| y.is_test()))
                        .unwrap_or(false)
            })
            .map(|(_, v)| v.clone())
            .collect::<Vec<_>>();
        type_ids.push((module_id, functions));

        // Visit all child modules recursively
        let children = ctx.module_children(module_id).to_vec();
        for child_id in children {
            self.collect_test_functions_from_module_tree(child_id, ctx, type_ids)?;
        }

        Ok(())
    }

    fn __interpret__(
        &mut self,
        program: &CheckedProgram<F>,
        type_id: TypeId,
        parameters: Vec<CheckedValueRef<F>>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<(String, u32, Vec<F>)>
    where
        F: 'static,
    {
        let node: &CheckedFunctionNode = ctx.symbols[type_id.clone()].as_function().unwrap();
        let method_name = ctx.program[node.name.id].to_string();
        let mut method_args = Vec::with_capacity(node.parameters.len());

        for parameter in node.parameters.iter() {
            method_args.push((
                ctx.program[parameter.name.id].to_string(),
                self.calculate_type_size(parameter.ty.clone(), ctx),
            ));
        }

        let outputs = self.interpret_function(&program, type_id, parameters, ctx)?.unwrap();

        let method_id = gen_dapen_contract_function_method_id(method_name.clone(), &method_args);

        Ok((method_name, method_id, outputs.to_felts()))
    }

    pub fn typecheck_single(&mut self, file: PathBuf) -> anyhow::Result<(TypeChecker<F, C>, TypeCheckerVisitorContext<F, C>)>
    where
        F: 'static,
    {
        let mut crate_path_graph = Graph::new();
        crate_path_graph.add_node(file);
        self.typecheck(crate_path_graph)
    }

    pub fn typecheck(&mut self, crate_path_graph: Graph<PathBuf>) -> anyhow::Result<(TypeChecker<F, C>, TypeCheckerVisitorContext<F, C>)>
    where
        F: 'static,
    {
        let mut program = Program::new();
        let mut parser = Parser::new(&mut program, &mut self.context, crate_path_graph);
        parser.parse().map_err(|err| {
            let context = lowering_parse_error(&err, &program);
            anyhow::Error::from(err).context(context)
        })?;

        let mut typechecker = TypeChecker::new(CheckedProgram::new(), Box::new(self.clone()));
        let mut storage_preprocessor: StorageProcessor = StorageProcessor::new();
        let mut default_visitor_context: DefaultVisitorContext<'_, F, C> = DefaultVisitorContext::new(&mut program);
        storage_preprocessor.visit_program(&mut default_visitor_context)?;

        let mut formatter = Formatter::new();
        formatter.visit_program(&mut default_visitor_context)?;
        println!("formatted:\n{}", formatter.get_output());

        let mut typechecker_context = TypeCheckerVisitorContext::new(program);
        typechecker.visit_program(&mut typechecker_context).map_err(|err| {
            let context = lowering_sema_error(&err, &mut typechecker_context);
            anyhow::Error::from(err).context(context)
        })?;
        Ok((typechecker, typechecker_context))
    }

    pub fn typecheck_lsp(
        &mut self,
        crate_path_graph: Graph<PathBuf>,
    ) -> std::result::Result<(TypeChecker<F, C>, TypeCheckerVisitorContext<F, C>), TypeCheckError>
    where
        F: 'static,
    {
        let mut program = Program::new();

        let mut parser = Parser::new(&mut program, &mut self.context, crate_path_graph);
        parser.parse().map_err(|err| {
            let err_desc = parse_error_to_diagnostic(&err, &program);
            TypeCheckError::Parse(err_desc)
        })?;

        let mut typechecker = TypeChecker::new(CheckedProgram::new(), Box::new(self.clone()));
        let mut storage_preprocessor: StorageProcessor = StorageProcessor::new();
        let mut default_visitor_context: DefaultVisitorContext<'_, F, C> = DefaultVisitorContext::new(&mut program);
        match storage_preprocessor.visit_program(&mut default_visitor_context) {
            Ok(_) => {}
            Err(e) => {
                let message = format!("Storage analysis error: {}", e);
                return Err(TypeCheckError::StoragePreprocess(message));
            }
        }

        let mut typechecker_context = TypeCheckerVisitorContext::new(program);
        typechecker.visit_program(&mut typechecker_context).map_err(|err| {
            let err_desc = typecheck_error_to_diagnostic(&err, &mut typechecker_context);
            TypeCheckError::TypeCheck(err_desc)
        })?;
        Ok((typechecker, typechecker_context))
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_function(
        &mut self,
        program: &CheckedProgram<F>,
        type_id: TypeId,
        parameters: Vec<CheckedValueRef<F>>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<ControlState<CheckedValueRef<F>>> {
        let scope_id = ctx.symbols[type_id].scope_id();
        ctx.symbols.enter_function(scope_id);
        let res = self.__interpret_function__(program, type_id, parameters, ctx);
        ctx.symbols.exit_function(scope_id);
        res
    }

    fn __interpret_function__(
        &mut self,
        program: &CheckedProgram<F>,
        type_id: TypeId,
        args: Vec<CheckedValueRef<F>>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<ControlState<CheckedValueRef<F>>> {
        let parameters = ctx.symbols[type_id].parameters();

        assert_eq!(
            args.len(),
            parameters.len(),
            "expected {} parameters for main function, got {}",
            parameters.len(),
            args.len()
        );

        for (parameter, arg) in parameters.iter().zip(args.into_iter()) {
            ctx.symbols.set_variable(ctx.symbols[type_id].scope_id(), parameter.name.id, arg)?
        }
        Ok(ControlState::Return(self.interpret_expr(
            program,
            ctx.symbols[type_id].body().unwrap(),
            ctx,
        )?))
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_while(&mut self, program: &CheckedProgram<F>, stmt_id: StmtId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let node = program[stmt_id].as_while().unwrap();
        loop {
            let predicate = self.interpret_expr(program, node.predicate, ctx)?.to_bool();

            if !self.is_constant(predicate) {
                return Err(Error::UncertainLoopCondition {
                    loop_location: node.location,
                });
            }

            if self.context.get_constant_value(predicate) == 1 {
                self.interpret_expr(program, node.body, ctx)?;
            } else {
                break Ok(());
            }
        }
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_for(&mut self, program: &CheckedProgram<F>, stmt_id: StmtId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let node = program[stmt_id].as_for().unwrap();
        let start = self.interpret_expr(program, node.start, ctx)?;
        let end_f = self.interpret_expr(program, node.end, ctx)?.to_value();

        if !self.is_constant(start.to_value()) || !self.is_constant(end_f) {
            return Err(Error::UncertainLoopCondition {
                loop_location: node.location,
            });
        }

        ctx.symbols.enter_block(node.scope_id);
        ctx.symbols.set_variable(node.scope_id, node.variable, start)?;

        loop {
            let var_id = ctx.symbols.get_variable(Some(node.scope_id), &node.variable).unwrap();
            let value_f = ctx.symbols.get_value(var_id).unwrap().to_u32();
            if value_f != end_f {
                self.interpret_expr(program, node.body, ctx)?;
                let one = self.context.op_const_u32(1);
                let value = CheckedValueRef::from_u32(self.context.op_u32_add(value_f, one));
                ctx.symbols.set_variable(node.scope_id, node.variable, value)?;
            } else {
                ctx.symbols.exit_block();
                break Ok(());
            }
        }
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_statement(
        &mut self,
        program: &CheckedProgram<F>,
        stmt_id: StmtId,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<ControlState<CheckedValueRef<F>>> {
        let node = &program[stmt_id];
        match node {
            CheckedStmtNode::For(r#_for) => self.interpret_for(program, stmt_id, ctx)?,
            CheckedStmtNode::Assignment(r#_assignment) => self.interpret_assignment(program, stmt_id, ctx)?,
            CheckedStmtNode::Variable(_variable) => self.interpret_variable(program, stmt_id, ctx)?,
            CheckedStmtNode::While(r#_while) => self.interpret_while(program, stmt_id, ctx)?,
            CheckedStmtNode::Definition(_definition) => {}
            CheckedStmtNode::Expression(expr_id) => {
                self.interpret_expr(program, *expr_id, ctx)?;
            }
            CheckedStmtNode::Return(_return_node) => {
                return self.interpret_ret(program, stmt_id, ctx);
            }
            CheckedStmtNode::Intrinsic(intrinsic_node) => match intrinsic_node {
                CheckedIntrinsicStmtNode::Assert {
                    left,
                    message,
                    comments: _,
                    location,
                } => {
                    let lhs_value = self.interpret_expr(program, left.clone(), ctx)?;
                    let condition = lhs_value.to_bool();
                    self.context
                        .assert_true(condition, Box::leak(message.clone().unwrap_or_default().into_boxed_str()));
                }
                CheckedIntrinsicStmtNode::AssertEq {
                    left,
                    right,
                    message,
                    comments: _,
                    location,
                } => {
                    let lhs_value = self.interpret_expr(program, left.clone(), ctx)?;
                    let rhs_value = self.interpret_expr(program, right.clone(), ctx)?;
                    let lhs = lhs_value.to_value();
                    let rhs = rhs_value.to_value();

                    self.context
                        .assert_eq(lhs, rhs, Box::leak(message.clone().unwrap_or_default().into_boxed_str()));
                }
                CheckedIntrinsicStmtNode::ClearEntireTree { .. } => {
                    let _result = self.context.clear_entire_tree();
                }
            },
        }
        Ok(ControlState::Normal(CheckedValueRef::new_rc(CheckedValue::Type(VOID_TYPE))))
    }

    #[instrument(level = "debug", skip_all)]
    fn interpret_ret(
        &mut self,
        program: &CheckedProgram<F>,
        stmt_id: StmtId,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<ControlState<CheckedValueRef<F>>> {
        let return_node = program[stmt_id].as_return().unwrap();
        if let Some(expr) = &return_node.ret {
            let value = self.interpret_expr(program, *expr, ctx)?;
            return Ok(ControlState::Return(value));
        }
        Ok(ControlState::Normal(CheckedValueRef::new_rc(CheckedValue::Type(VOID_TYPE))))
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_value(
        &mut self,
        program: &CheckedProgram<F>,
        node: &CheckedValueNode<F>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValue<F>> {
        Ok(match node {
            CheckedValueNode::Felt(value, _location) => CheckedValue::Felt(*value),
            CheckedValueNode::Bool(value, _location) => CheckedValue::Bool(*value),
            CheckedValueNode::U32(value, _location) => CheckedValue::U32(*value),
            CheckedValueNode::Array(type_id, elements, _location) => {
                let mut values = Vec::new();
                for element in elements {
                    values.push(self.interpret_expr(program, *element, ctx)?);
                }
                CheckedValue::Array(*type_id, values)
            }
            CheckedValueNode::Struct(type_id, field_values, _location) => {
                let mut values = IndexMap::new();
                for (field_name, field_value) in field_values {
                    values.insert(field_name.clone(), self.interpret_expr(program, *field_value, ctx)?);
                }
                CheckedValue::Struct(*type_id, values)
            }
            CheckedValueNode::Type(type_id) => CheckedValue::Type(*type_id),
            CheckedValueNode::Tuple(type_id, elements, _location) => {
                let mut values = Vec::new();
                for (elem_type, expr_id) in elements {
                    let value = self.interpret_expr(program, *expr_id, ctx)?;

                    values.push((*elem_type, value));
                }
                CheckedValue::Tuple {
                    type_id: *type_id,
                    elements: values,
                }
            }
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_unary(
        &mut self,
        program: &CheckedProgram<F>,
        unary_node: &CheckedUnaryNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValue<F>> {
        let rhs_value = self.interpret_expr(program, unary_node.rhs, ctx)?;
        let rhs_borrow = rhs_value.borrow();

        Ok(match (&*rhs_borrow, unary_node.operator) {
            (CheckedValue::Felt(b), UnaryOperator::Neg) => CheckedValue::Felt(self.context.op_neg(*b)),
            (CheckedValue::Bool(b), UnaryOperator::Neg) => CheckedValue::Bool(self.context.op_bool_not(*b)),
            // TODO: should be bitwise not
            (CheckedValue::Felt(v), UnaryOperator::Not) => CheckedValue::Felt(self.context.op_bool_not(*v)),
            (CheckedValue::Bool(v), UnaryOperator::Not) => CheckedValue::Bool(self.context.op_bool_not(*v)),
            _ => todo!(),
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_binary(
        &mut self,
        program: &CheckedProgram<F>,
        binary_node: &CheckedBinaryNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValue<F>> {
        use BinaryOperator::*;
        let lhs_value = self.interpret_expr(program, binary_node.lhs, ctx)?;
        let rhs_value = self.interpret_expr(program, binary_node.rhs, ctx)?;

        let value = match (&*lhs_value.borrow(), &*rhs_value.borrow(), binary_node.operator) {
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Add) => self.context.op_add(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Sub) => self.context.op_sub(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Mul) => self.context.op_mul(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Div) => self.context.op_div(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Pow) => self.context.op_exp(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Mod) => self.context.op_mod(*l, *r),
            // TODO: don't use u32 opcodes
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitShr) => self.context.op_u32_shr(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitShl) => self.context.op_u32_shl(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitAnd) => self.context.op_u32_and(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitOr) => self.context.op_u32_or(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitXor) => self.context.op_u32_xor(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Eq) => self.context.op_eq(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Neq) => self.context.op_neq(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Lt) => self.context.op_lt(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Lte) => self.context.op_lte(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Gt) => self.context.op_gt(*l, *r),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), Gte) => self.context.op_gte(*l, *r),

            (CheckedValue::U32(l), CheckedValue::U32(r), Add) => self.context.op_u32_add(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Sub) => self.context.op_u32_sub(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Mul) => self.context.op_u32_mul(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Div) => self.context.op_u32_div(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitShr) => self.context.op_u32_shr(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitShl) => self.context.op_u32_shl(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitAnd) => self.context.op_u32_and(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitOr) => self.context.op_u32_or(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitXor) => self.context.op_u32_xor(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Eq) => self.context.op_eq(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Neq) => self.context.op_neq(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Lt) => self.context.op_lt(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Lte) => self.context.op_lte(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Gt) => self.context.op_gt(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Gte) => self.context.op_gte(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Mod) => self.context.op_u32_mod(*l, *r),
            (CheckedValue::U32(l), CheckedValue::U32(r), Pow) => self.context.op_u32_exp(*l, *r),

            (CheckedValue::Bool(l), CheckedValue::Bool(r), And) => self.context.op_bool_and(*l, *r),
            (CheckedValue::Bool(l), CheckedValue::Bool(r), Or) => self.context.op_bool_or(*l, *r),
            (CheckedValue::Bool(l), CheckedValue::Bool(r), Eq) => self.context.op_eq(*l, *r),
            (CheckedValue::Bool(l), CheckedValue::Bool(r), Neq) => self.context.op_neq(*l, *r),
            (CheckedValue::Bool(l), CheckedValue::Bool(r), Lt) => self.context.op_lt(*l, *r),
            (CheckedValue::Bool(l), CheckedValue::Bool(r), Lte) => self.context.op_lte(*l, *r),
            (CheckedValue::Bool(l), CheckedValue::Bool(r), Gt) => self.context.op_gt(*l, *r),
            (CheckedValue::Bool(l), CheckedValue::Bool(r), Gte) => self.context.op_gte(*l, *r),
            (CheckedValue::Bool(l), CheckedValue::Bool(r), BitXor) => self.context.op_bool_xor(*l, *r),

            _ => unreachable!(),
        };

        match binary_node.type_id {
            id if id == BOOL_TYPE => Ok(CheckedValue::Bool(value)),
            id if id == U32_TYPE => Ok(CheckedValue::U32(value)),
            _ => Ok(CheckedValue::Felt(value)),
        }
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_assignment_value(
        &mut self,
        _program: &CheckedProgram<F>,
        old_value: &CheckedValueRef<F>,
        operator: AssignmentOperator,
        value: CheckedValueRef<F>,
        _ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        use AssignmentOperator::*;

        let new_value = match (&*old_value.borrow(), &*value.borrow(), operator) {
            (_, _, Eq) => value.clone(),

            (CheckedValue::Felt(l), CheckedValue::Felt(r), AddAssign) => CheckedValueRef::from_felt(self.context.op_add(*l, *r)),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), SubAssign) => CheckedValueRef::from_felt(self.context.op_sub(*l, *r)),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), MulAssign) => CheckedValueRef::from_felt(self.context.op_mul(*l, *r)),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), DivAssign) => CheckedValueRef::from_felt(self.context.op_div(*l, *r)),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), ModAssign) => CheckedValueRef::from_felt(self.context.op_mod(*l, *r)),
            // TODO: don't use u32 opcodes
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitAndAssign) => CheckedValueRef::from_felt(self.context.op_u32_and(*l, *r)),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitOrAssign) => CheckedValueRef::from_felt(self.context.op_u32_or(*l, *r)),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitXorAssign) => CheckedValueRef::from_felt(self.context.op_u32_xor(*l, *r)),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitShlAssign) => CheckedValueRef::from_felt(self.context.op_u32_shl(*l, *r)),
            (CheckedValue::Felt(l), CheckedValue::Felt(r), BitShrAssign) => CheckedValueRef::from_felt(self.context.op_u32_shr(*l, *r)),

            (CheckedValue::U32(l), CheckedValue::U32(r), AddAssign) => CheckedValueRef::from_u32(self.context.op_u32_add(*l, *r)),
            (CheckedValue::U32(l), CheckedValue::U32(r), SubAssign) => CheckedValueRef::from_u32(self.context.op_u32_sub(*l, *r)),
            (CheckedValue::U32(l), CheckedValue::U32(r), MulAssign) => CheckedValueRef::from_u32(self.context.op_u32_mul(*l, *r)),
            (CheckedValue::U32(l), CheckedValue::U32(r), DivAssign) => CheckedValueRef::from_u32(self.context.op_u32_div(*l, *r)),
            (CheckedValue::U32(l), CheckedValue::U32(r), ModAssign) => CheckedValueRef::from_u32(self.context.op_mod(*l, *r)),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitAndAssign) => CheckedValueRef::from_u32(self.context.op_u32_and(*l, *r)),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitOrAssign) => CheckedValueRef::from_u32(self.context.op_u32_or(*l, *r)),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitXorAssign) => CheckedValueRef::from_u32(self.context.op_u32_xor(*l, *r)),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitShlAssign) => CheckedValueRef::from_u32(self.context.op_u32_shl(*l, *r)),
            (CheckedValue::U32(l), CheckedValue::U32(r), BitShrAssign) => CheckedValueRef::from_u32(self.context.op_u32_shr(*l, *r)),

            _ => unreachable!(),
        };

        Ok(CheckedValueRef::<F>::select(
            &mut self.context,
            &new_value,
            &old_value,
            &|ctx: &mut C, n: &F, o: &F| ctx.cset(o.clone(), n.clone()),
        ))
    }

    #[instrument(level = "debug", skip_all)]
    fn __interpret_expr__(
        &mut self,
        program: &CheckedProgram<F>,
        node: &CheckedExprNode<F>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        match node {
            CheckedExprNode::Path(path) => Ok(self.interpret_path(program, path, ctx)?),
            CheckedExprNode::Intrinsic(ctx_node) => Ok({
                match ctx_node {
                    CheckedIntrinsicExprNode::GetUserId { .. } => CheckedValueRef::from_felt(self.context.get_user_id()),
                    CheckedIntrinsicExprNode::GetContractId { .. } => CheckedValueRef::from_felt(self.context.get_contract_id()),
                    CheckedIntrinsicExprNode::GetContractDeployer { contract_id, type_id, .. } => {
                        let contract_id = self.interpret_expr(program, contract_id.clone(), ctx)?.to_felt();
                        CheckedValueRef::from_vec(type_id.clone(), self.context.get_contract_deployer(contract_id))
                    }
                    CheckedIntrinsicExprNode::GetCallerContractId { .. } => CheckedValueRef::from_felt(self.context.get_caller_contract_id()),
                    CheckedIntrinsicExprNode::GetCheckpointId { .. } => CheckedValueRef::from_felt(self.context.get_checkpoint_id()),
                    CheckedIntrinsicExprNode::GetLastNonce { .. } => CheckedValueRef::from_felt(self.context.get_last_nonce()),
                    CheckedIntrinsicExprNode::GetUserPublicKeyHash { type_id, .. } => {
                        CheckedValueRef::from_vec(type_id.clone(), self.context.get_user_public_key_hash())
                    }
                    CheckedIntrinsicExprNode::GetStateHashAt { slot_index, type_id, .. } => {
                        let slot_index = self.interpret_expr(program, slot_index.clone(), ctx)?.to_felt();
                        CheckedValueRef::from_vec(type_id.clone(), self.context.get_state_hash_at(slot_index))
                    }
                    CheckedIntrinsicExprNode::GetOtherContractStateHashAt {
                        contract_state_tree_height,
                        contract_id,
                        slot_index,
                        type_id,
                        ..
                    } => {
                        let contract_state_tree_height = self.interpret_expr(program, contract_state_tree_height.clone(), ctx)?.to_felt();
                        let contract_id = self.interpret_expr(program, contract_id.clone(), ctx)?.to_felt();
                        let slot_index = self.interpret_expr(program, slot_index.clone(), ctx)?.to_felt();
                        CheckedValueRef::from_vec(
                            type_id.clone(),
                            self.context
                                .get_other_contract_state_hash_at(contract_state_tree_height, contract_id, slot_index),
                        )
                    }
                    CheckedIntrinsicExprNode::GetOtherUserContractStateHashAt {
                        contract_state_tree_height,
                        user_id,
                        contract_id,
                        slot_index,
                        type_id,
                        ..
                    } => {
                        let contract_state_tree_height = self.interpret_expr(program, contract_state_tree_height.clone(), ctx)?.to_felt();
                        let user_id = self.interpret_expr(program, user_id.clone(), ctx)?.to_felt();
                        let contract_id = self.interpret_expr(program, contract_id.clone(), ctx)?.to_felt();
                        let slot_index = self.interpret_expr(program, slot_index.clone(), ctx)?.to_felt();
                        CheckedValueRef::from_vec(
                            type_id.clone(),
                            self.context
                                .get_other_user_contract_state_hash_at(contract_state_tree_height, user_id, contract_id, slot_index),
                        )
                    }
                    CheckedIntrinsicExprNode::CSetStateHashAt {
                        slot_index,
                        new_value,
                        type_id,
                        ..
                    } => {
                        let new_value = self.interpret_expr(program, new_value.clone(), ctx)?.to_array();
                        let slot_index = self.interpret_expr(program, slot_index.clone(), ctx)?.to_felt();
                        CheckedValueRef::from_vec(type_id.clone(), self.context.cset_state_hash_at(slot_index, new_value))
                    }
                    CheckedIntrinsicExprNode::StorageRead {
                        contract_state_tree_height,
                        user_id,
                        contract_id,
                        offset,
                        ..
                    } => {
                        let contract_state_tree_height = self.interpret_expr(program, contract_state_tree_height.clone(), ctx)?.to_felt();
                        let user_id = self.interpret_expr(program, user_id.clone(), ctx)?.to_felt();
                        let contract_id = self.interpret_expr(program, contract_id.clone(), ctx)?.to_felt();
                        let offset = self.interpret_expr(program, offset.clone(), ctx)?.to_felt();
                        let value = self.context.op_get_state_felt(
                            u16::try_from(self.context.get_constant_value(contract_state_tree_height)).unwrap(),
                            contract_id,
                            user_id,
                            offset,
                        );
                        return Ok(CheckedValueRef::from_felt(value));
                    }
                    CheckedIntrinsicExprNode::StorageWrite { offset, value, .. } => {
                        let offset = self.interpret_expr(program, offset.clone(), ctx)?;
                        let value = self.interpret_expr(program, value.clone(), ctx)?;
                        return Ok(CheckedValueRef::from_felt(
                            self.context.op_set_state_felt(offset.to_felt(), value.to_felt()),
                        ));
                    }
                    CheckedIntrinsicExprNode::Hash { data, type_id, .. } => {
                        let data = self.interpret_expr(program, data.clone(), ctx)?;
                        return Ok(CheckedValueRef::from_vec(type_id.clone(), self.context.hash(&data.to_felts())));
                    }
                    CheckedIntrinsicExprNode::HashTwoToOne { left, right, type_id, .. } => {
                        let left = self.interpret_expr(program, left.clone(), ctx)?;
                        let right = self.interpret_expr(program, right.clone(), ctx)?;
                        let left_felts = left.to_felts();
                        let right_felts = right.to_felts();

                        assert!(left_felts.len() == 4, "hash_two_to_one left input must be Hash (4 felts)");
                        assert!(right_felts.len() == 4, "hash_two_to_one right input must be Hash (4 felts)");

                        let left_array: [F; 4] = [left_felts[0].clone(), left_felts[1].clone(), left_felts[2].clone(), left_felts[3].clone()];
                        let right_array: [F; 4] = [
                            right_felts[0].clone(),
                            right_felts[1].clone(),
                            right_felts[2].clone(),
                            right_felts[3].clone(),
                        ];

                        return Ok(CheckedValueRef::from_vec(
                            type_id.clone(),
                            self.context.hash_two_to_one(&left_array, &right_array).to_vec(),
                        ));
                    }
                    CheckedIntrinsicExprNode::MemTransmute { data, target_type, .. } => {
                        let data = self.interpret_expr(program, data.clone(), ctx)?;
                        return Ok(CheckedValueRef::decode_felts(&data.to_felts(), ctx, target_type.clone()));
                    }
                    CheckedIntrinsicExprNode::MemSizeOf { query_type: ty, .. } => {
                        let size = self.calculate_type_size(ty.clone(), ctx);
                        return Ok(CheckedValueRef::from_felt(self.context.op_const(size as u64)));
                    }
                    CheckedIntrinsicExprNode::StorageReadRange {
                        contract_state_tree_height,
                        user_id,
                        contract_id,
                        offset,
                        length,
                        ..
                    } => {
                        let contract_state_tree_height = self.interpret_expr(program, contract_state_tree_height.clone(), ctx)?.to_felt();
                        let user_id = self.interpret_expr(program, user_id.clone(), ctx)?.to_felt();
                        let contract_id = self.interpret_expr(program, contract_id.clone(), ctx)?.to_felt();
                        let offset = self.interpret_expr(program, offset.clone(), ctx)?.to_felt();
                        let length = self.interpret_expr(program, length.clone(), ctx)?.to_felt();
                        let values =
                            self.context
                                .get_other_user_contract_state_range_at(contract_state_tree_height, user_id, contract_id, offset, length);
                        return Ok(CheckedValueRef::from_vec(UNKOWN_TYPE, values));
                    }
                    CheckedIntrinsicExprNode::StorageWriteRange { offset, values, .. } => {
                        let offset = self.interpret_expr(program, offset.clone(), ctx)?;
                        let values = self.interpret_expr(program, values.clone(), ctx)?;
                        self.context.cset_state_range_at(offset.to_felt(), &values.to_felts());
                        return Ok(CheckedValueRef::new_rc(CheckedValue::Type(VOID_TYPE)));
                    }
                    CheckedIntrinsicExprNode::InvokeSync {
                        contract_id,
                        method_id,
                        inputs,
                        type_id,
                        location,
                    } => {
                        let contract_id = self.interpret_expr(program, contract_id.clone(), ctx)?.to_felt();
                        let method_id = self.interpret_expr(program, method_id.clone(), ctx)?.to_felt();
                        let inputs = self.interpret_expr(program, *inputs, ctx)?.to_vec();
                        let output_size = self.calculate_type_size(type_id.clone(), ctx);
                        let outputs =
                            self.context
                                .cinvoke_external_contract_function_sync(contract_id, method_id, inputs, output_size.try_into().unwrap());
                        return Ok(CheckedValueRef::decode_felts(&outputs, ctx, type_id.clone()));
                    }
                    CheckedIntrinsicExprNode::InvokeDeferred {
                        contract_id,
                        method_id,
                        inputs,
                        type_id,
                        location,
                    } => {
                        let contract_id = self.interpret_expr(program, contract_id.clone(), ctx)?.to_felt();
                        let method_id = self.interpret_expr(program, method_id.clone(), ctx)?.to_felt();
                        let inputs = self.interpret_expr(program, *inputs, ctx)?.to_vec();
                        self.context.cinvoke_external_contract_function_deferred(contract_id, method_id, inputs);
                        return Ok(CheckedValueRef::new_rc(CheckedValue::Type(type_id.clone())));
                    }
                    CheckedIntrinsicExprNode::Secp256k1Verify {
                        pub_key,
                        msg,
                        sig,
                        type_id,
                        location,
                    } => {
                        let public_key = self.interpret_expr(program, pub_key.clone(), ctx)?.to_u32_array();
                        let msg = self.interpret_expr(program, msg.clone(), ctx)?.to_array();
                        let signature = self.interpret_expr(program, sig.clone(), ctx)?.to_u32_array();

                        return Ok(CheckedValueRef::from_bool(self.context.op_secp256k1_verify(public_key, msg, signature)));
                    }
                    CheckedIntrinsicExprNode::SumBits { bits, type_id, location } => {
                        let bits = self.interpret_expr(program, bits.clone(), ctx)?.to_vec();

                        return Ok(CheckedValueRef::from_felt(self.context.sum_bits(&bits)));
                    }
                    CheckedIntrinsicExprNode::SplitBits {
                        target,
                        num_bits,
                        type_id,
                        location,
                    } => {
                        let target = self.interpret_expr(program, target.clone(), ctx)?.to_felt();

                        return Ok(CheckedValueRef::from_vec(type_id.clone(), self.context.split_bits(target, *num_bits)));
                    }
                    CheckedIntrinsicExprNode::GetCheckpointStats {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felts().first().unwrap().clone();
                        let stats = self.context.get_checkpoint_stats(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_vec(type_id.clone(), stats));
                    }
                    CheckedIntrinsicExprNode::GetRegisterUsersRoot {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let root = self.context.get_register_users_root(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_vec(type_id.clone(), root.to_vec()));
                    }
                    CheckedIntrinsicExprNode::GetGutasRoot {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let root = self.context.get_gutas_root(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_vec(type_id.clone(), root.to_vec()));
                    }
                    CheckedIntrinsicExprNode::GetDeployContractsRoot {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let root = self.context.get_deploy_contracts_root(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_vec(type_id.clone(), root.to_vec()));
                    }
                    CheckedIntrinsicExprNode::GetFeesCollected {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let fees = self.context.get_fees_collected(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_felt(fees));
                    }
                    CheckedIntrinsicExprNode::GetUserOpsProcessed {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let ops = self.context.get_user_ops_processed(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_felt(ops));
                    }
                    CheckedIntrinsicExprNode::GetTotalTransactions {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let txns = self.context.get_total_transactions(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_felt(txns));
                    }
                    CheckedIntrinsicExprNode::GetSlotsModified {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let slots = self.context.get_slots_modified(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_felt(slots));
                    }
                    CheckedIntrinsicExprNode::GetRegisterUsersCompleted {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let completed = self.context.get_register_users_completed(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_felt(completed));
                    }
                    CheckedIntrinsicExprNode::GetGutasCompleted {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let completed = self.context.get_gutas_completed(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_felt(completed));
                    }
                    CheckedIntrinsicExprNode::GetDeployContractsCompleted {
                        checkpoint_id,
                        type_id,
                        location,
                    } => {
                        let checkpoint_id = self.interpret_expr(program, checkpoint_id.clone(), ctx)?;
                        let checkpoint_id_value = checkpoint_id.to_felt();
                        let completed = self.context.get_deploy_contracts_completed(checkpoint_id_value);
                        return Ok(CheckedValueRef::from_felt(completed));
                    }
                }
            }),
            CheckedExprNode::Value(value_node) => Ok(CheckedValueRef::new_rc(self.interpret_value(program, &value_node, ctx)?)),
            CheckedExprNode::Binary(binary_node) => Ok(CheckedValueRef::new_rc(self.interpret_binary(program, binary_node, ctx)?)),
            CheckedExprNode::Unary(unary_node) => Ok(CheckedValueRef::new_rc(self.interpret_unary(program, unary_node, ctx)?)),
            CheckedExprNode::Call(call_node) => Ok(self.interpret_call(program, call_node, ctx)?.unwrap()),
            CheckedExprNode::MemberCall(checked_member_call_node) => Ok(self.interpret_member_call(program, checked_member_call_node, ctx)?.unwrap()),
            CheckedExprNode::Cast(cast_node) => Ok(CheckedValueRef::new_rc(self.interpret_cast(program, cast_node, ctx)?)),
            CheckedExprNode::IndexAccess(index_access_node) => Ok(self.interpret_index_access(program, index_access_node, ctx)?),
            CheckedExprNode::TupleAccess(tuple_access_node) => Ok(self.interpret_tuple_access(program, tuple_access_node, ctx)?),
            CheckedExprNode::MemberAccess(member_access_node) => Ok(self.interpret_member_access(program, member_access_node, ctx)?),
            CheckedExprNode::LambdaFunction(checked_lambda_function_node) => {
                Ok(CheckedValueRef::new_rc(CheckedValue::Type(checked_lambda_function_node.type_id)))
            }
            CheckedExprNode::BlockExpr(block_expr) => {
                ctx.symbols.enter_block(block_expr.scope_id);
                let res = self.interpret_block_expr(program, block_expr, ctx)?;
                ctx.symbols.exit_block();
                Ok(res)
            }
            CheckedExprNode::IfExpr(if_expr) => Ok(self.interpret_if_expr(program, if_expr, ctx)?),
            CheckedExprNode::Match(match_expr) => Ok(self.interpret_match(program, match_expr, ctx)?),
        }
    }
    #[instrument(level = "debug", skip_all)]
    pub fn interpret_expr(
        &mut self,
        program: &CheckedProgram<F>,
        expr_id: ExprId,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        let node = &program[expr_id];
        self.__interpret_expr__(program, node, ctx)
    }
    #[instrument(level = "debug", skip_all)]
    fn interpret_member_access(
        &mut self,
        program: &CheckedProgram<F>,
        member_access_node: &CheckedMemberAccessNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        let s = self.interpret_expr(program, member_access_node.target, ctx)?;
        if ctx.symbols[member_access_node.type_id].is_function() {
            return Ok(CheckedValueRef::new_rc(CheckedValue::Type(member_access_node.type_id)));
        } else {
            Ok(s.get_path(&mut self.context, &[IndexPath::Normal(member_access_node.field.id.into())])
                .unwrap())
        }
    }

    #[instrument(level = "debug", skip_all)]
    fn interpret_index_access(
        &mut self,
        program: &CheckedProgram<F>,
        index_access_node: &CheckedIndexAccessNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        let a = self.interpret_expr(program, index_access_node.target, ctx)?;
        let index = self.interpret_expr(program, index_access_node.index, ctx)?;

        return Ok(a.get_path(&mut self.context, &[IndexPath::Felt(index.to_felt())]).unwrap());
    }

    #[instrument(level = "debug", skip_all)]
    fn interpret_tuple_access(
        &mut self,
        program: &CheckedProgram<F>,
        tuple_access_node: &CheckedTupleAccessNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        let t = self.interpret_expr(program, tuple_access_node.target, ctx)?;

        Ok(t.get_path(&mut self.context, &[IndexPath::Normal(tuple_access_node.index.into())])
            .unwrap())
    }

    #[instrument(level = "debug", skip_all)]
    fn interpret_cast(
        &mut self,
        program: &CheckedProgram<F>,
        cast_node: &CheckedCastNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValue<F>> {
        let value = self.interpret_expr(program, cast_node.value, ctx)?.to_value();

        if self.is_constant(value) {
            let const_value = self.context.get_constant_value(value);
            match cast_node.target_type {
                BOOL_TYPE => {
                    if const_value > 1 {
                        return Err(Error::SemaError(psy_sema::Error::InvalidCast {
                            location: cast_node.location,
                            expected: "cast to bool".to_string(),
                            found: format!("value {} > 1", const_value),
                        }));
                    }
                }
                FELT_TYPE => {}
                U32_TYPE => {
                    if const_value > 0xffffffffu64 {
                        return Err(Error::SemaError(psy_sema::Error::InvalidCast {
                            location: cast_node.location,
                            expected: "cast to u32".to_string(),
                            found: format!("value {} > 0xffffffffu64", const_value),
                        }));
                    }
                }
                _ => unimplemented!(),
            }
        }

        match cast_node.target_type {
            BOOL_TYPE => Ok(CheckedValue::Bool(self.context.op_cast_bool(value))),
            FELT_TYPE => Ok(CheckedValue::Felt(self.context.op_cast_felt(value))),
            U32_TYPE => Ok(CheckedValue::U32(self.context.op_cast_u32(value))),
            _ => unimplemented!(),
        }
    }

    #[instrument(level = "debug", skip_all)]
    fn interpret_call(
        &mut self,
        program: &CheckedProgram<F>,
        call_node: &CheckedCallNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<ControlState<CheckedValueRef<F>>> {
        let f = self.interpret_expr(program, call_node.callee, ctx)?;
        let mut parameters = Vec::new();
        for arg in call_node.args.iter() {
            parameters.push(self.interpret_expr(program, arg.clone(), ctx)?);
        }
        return self.interpret_function(program, f.type_id(), parameters, ctx);
    }

    #[instrument(level = "debug", skip_all)]
    fn interpret_member_call(
        &mut self,
        program: &CheckedProgram<F>,
        call_node: &CheckedMemberCallNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<ControlState<CheckedValueRef<F>>> {
        let f = self.interpret_expr(program, call_node.callee, ctx)?;
        let mut parameters = Vec::new();
        for arg in once(&call_node.receiver).chain(call_node.args.iter()) {
            parameters.push(self.interpret_expr(program, arg.clone(), ctx)?);
        }
        return self.interpret_function(program, f.type_id(), parameters, ctx);
    }

    #[instrument(level = "debug", skip_all)]
    fn interpret_path(
        &mut self,
        _program: &CheckedProgram<F>,
        path: &CheckedPathNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        if let Some(var_id) = path.variable {
            return Ok(ctx.symbols.get_value(var_id).unwrap());
        } else if let Some(CheckedConstNode { value, .. }) = ctx.symbols[path.type_id].as_const() {
            return Ok(ctx.symbols.get_constant_value(value.clone()));
        } else {
            return Ok(CheckedValueRef::new_rc(CheckedValue::Type(path.type_id.clone())));
        }
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_assignment(&mut self, program: &CheckedProgram<F>, stmt_id: StmtId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let node = program[stmt_id].as_assignment().unwrap();
        let value = self.interpret_expr(program, node.value, ctx)?;
        let mut path: Vec<IndexPath<F>> = vec![];
        let mut index_condition = vec![];

        let (old_value, var_id) = self.interpret_assignment_target(program, node, &program[node.target], &mut path, ctx)?;

        let new_value = self.interpret_assignment_value(program, &old_value, node.operator, value, ctx)?;

        let mut variable_value = ctx.symbols.get_value(var_id).unwrap();
        variable_value.set_path(&mut self.context, &path, &mut index_condition, new_value)?;

        ctx.symbols.set_value(var_id, variable_value)?;

        Ok(())
    }

    fn interpret_assignment_target(
        &mut self,
        program: &CheckedProgram<F>,
        node: &CheckedAssignmentNode,
        expr_node: &CheckedExprNode<F>,
        path: &mut Vec<IndexPath<F>>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<(CheckedValueRef<F>, VarId)> {
        match expr_node {
            CheckedExprNode::Path(path_node) => self.interpret_path_assignment(program, node, path_node, path, ctx),
            CheckedExprNode::MemberAccess(member_access_node) => self.interpret_member_assignment(program, node, member_access_node, path, ctx),
            CheckedExprNode::IndexAccess(index_access_node) => self.interpret_index_assignment(program, node, index_access_node, path, ctx),
            CheckedExprNode::TupleAccess(tuple_access_node) => self.interpret_tuple_assignment(program, node, tuple_access_node, path, ctx),
            _ => unreachable!(),
        }
    }

    fn interpret_index_assignment(
        &mut self,
        program: &CheckedProgram<F>,
        node: &CheckedAssignmentNode,
        index_access_node: &CheckedIndexAccessNode,
        path: &mut Vec<IndexPath<F>>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<(CheckedValueRef<F>, VarId)> {
        let (inner_value, inner_var_id) = self.interpret_assignment_target(program, node, &program[index_access_node.target], path, ctx)?;

        let index = IndexPath::Felt(self.interpret_expr(program, index_access_node.index, ctx)?.to_felt());

        path.push(index.clone());

        Ok((inner_value.get_path(&mut self.context, &[index]).unwrap(), inner_var_id))
    }

    fn interpret_tuple_assignment(
        &mut self,
        program: &CheckedProgram<F>,
        node: &CheckedAssignmentNode,
        tuple_access_node: &CheckedTupleAccessNode,
        path: &mut Vec<IndexPath<F>>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<(CheckedValueRef<F>, VarId)> {
        let (tuple_value, tuple_var_id) = self.interpret_assignment_target(program, node, &program[tuple_access_node.target], path, ctx)?;

        let index = IndexPath::Normal(tuple_access_node.index);
        path.push(index.clone());

        let element_value = tuple_value.get_path(&mut self.context, &[index]).unwrap();

        Ok((element_value, tuple_var_id))
    }

    fn interpret_member_assignment(
        &mut self,
        program: &CheckedProgram<F>,
        node: &CheckedAssignmentNode,
        member_access_node: &CheckedMemberAccessNode,
        path: &mut Vec<IndexPath<F>>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<(CheckedValueRef<F>, VarId)> {
        let (inner_value, inner_var_id) = self.interpret_assignment_target(program, node, &program[member_access_node.target], path, ctx)?;

        let index = IndexPath::Normal(member_access_node.field.id.into());
        path.push(index.clone());

        Ok((inner_value.get_path(&mut self.context, &[index]).unwrap(), inner_var_id))
    }

    fn interpret_path_assignment(
        &mut self,
        _program: &CheckedProgram<F>,
        _node: &CheckedAssignmentNode,
        path_node: &CheckedPathNode,
        _path: &mut Vec<IndexPath<F>>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<(CheckedValueRef<F>, VarId)> {
        let var_id = path_node.variable.unwrap();
        Ok((ctx.symbols.get_value(var_id).unwrap(), var_id))
    }

    #[instrument(level = "debug", skip_all)]
    fn interpret_variable(&mut self, program: &CheckedProgram<F>, stmt_id: StmtId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let node = program[stmt_id].as_variable().unwrap();
        let value = self.interpret_expr(program, node.value, ctx)?;

        ctx.symbols.set_variable(node.scope_id, node.name.id, value.clone())?;
        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_block_expr(
        &mut self,
        program: &CheckedProgram<F>,
        block_expr: &CheckedBlockExprNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        for stmt_id in &block_expr.stmts {
            if let ControlState::Return(value) = self.interpret_statement(program, *stmt_id, ctx)? {
                return Ok(value);
            }
        }

        if let Some(return_expr) = block_expr.expr {
            return self.interpret_expr(program, return_expr.clone(), ctx);
        };

        Ok(CheckedValueRef::new_rc(CheckedValue::Type(VOID_TYPE)))
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_if_expr(
        &mut self,
        program: &CheckedProgram<F>,
        node: &CheckedIfExprNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        let predicate = self.interpret_expr(program, node.if_branch.predicate, ctx)?.to_bool();

        self.context.start_if_block(predicate);

        let mut result = self.interpret_expr(program, node.if_branch.body, ctx)?;

        for condition in &node.elseif_branches {
            let predicate = self.interpret_expr(program, condition.predicate, ctx)?.to_bool();

            self.context.start_else_if_block(predicate);

            let elseif_result = self.interpret_expr(program, condition.body, ctx)?;

            result = CheckedValueRef::<F>::select(&mut self.context, &elseif_result, &result, &|ctx: &mut C, n: &F, o: &F| {
                ctx.cset(o.clone(), n.clone())
            });
        }

        if let Some(else_branch) = &node.else_branch {
            self.context.start_else_block();

            let else_result = self.interpret_expr(program, else_branch.clone(), ctx)?;
            result = CheckedValueRef::<F>::select(&mut self.context, &else_result, &result, &|ctx: &mut C, n: &F, o: &F| {
                ctx.cset(o.clone(), n.clone())
            });
        }

        self.context.end_if_block();

        Ok(result)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn interpret_match(
        &mut self,
        program: &CheckedProgram<F>,
        match_node: &CheckedMatchNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<CheckedValueRef<F>> {
        let scrutinee_value = self.interpret_expr(program, match_node.value, ctx)?;
        let scrutinee_type = scrutinee_value.type_id();
        let mut return_value = CheckedValueRef::new_rc(CheckedValue::Type(VOID_TYPE));
        let mut wildcard_case: Option<&CheckedMatchArm> = None;
        let mut match_cases = match_node.cases.iter();

        if let Some(first_arm) = match_cases.next() {
            if let Some(pattern_expr) = &first_arm.pattern {
                let matched = self.match_pattern(program, ctx, pattern_expr, &scrutinee_value)?;
                self.context.start_if_block(matched);
                //note: use the first arm return value to initialize the return value
                return_value = self.interpret_expr(program, first_arm.body, ctx)?;
            } else {
                wildcard_case = Some(first_arm);
            }
        }

        for arm in match_cases {
            if let Some(pattern_expr) = &arm.pattern {
                let matched = self.match_pattern(program, ctx, pattern_expr, &scrutinee_value)?;
                self.context.start_else_if_block(matched);
                let body_value = self.interpret_expr(program, arm.body, ctx)?;

                return_value = CheckedValueRef::<F>::select(&mut self.context, &body_value, &return_value, &|ctx: &mut C, n: &F, o: &F| {
                    ctx.cset(o.clone(), n.clone())
                });
            } else {
                if wildcard_case.is_some() {
                    return Err(Error::SemaError(SemaError::DuplicateWildcard { location: arm.location }));
                }
                wildcard_case = Some(arm);
            }
        }
        //Caution: Must have a "_" in the match
        if let Some(wildcard_arm) = wildcard_case {
            self.context.start_else_block();
            let body_value = self.interpret_expr(program, wildcard_arm.body, ctx)?;
            return_value = CheckedValueRef::<F>::select(&mut self.context, &body_value, &return_value, &|ctx: &mut C, n: &F, o: &F| {
                ctx.cset(o.clone(), n.clone())
            });
        } else {
            //Caution: in white list, no need to have a "_" in the match
            let white_list = vec![BOOL_TYPE];
            if !white_list.contains(&scrutinee_type) {
                return Err(Error::SemaError(SemaError::IncompleteMatch {
                    location: match_node.location,
                    message: "need a \"_\" for match".to_string(),
                }));
            }
        }

        self.context.end_if_block();

        Ok(return_value)
    }

    fn match_pattern(
        &mut self,
        program: &CheckedProgram<F>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
        pattern_expr: &ExprId,
        scrutinee_value: &CheckedValueRef<F>,
    ) -> Result<F> {
        let pattern_value = self.interpret_expr(program, *pattern_expr, ctx)?;
        let (scrutinee_borrow, pattern_borrow) = (scrutinee_value.borrow(), pattern_value.borrow());

        match (&*scrutinee_borrow, &*pattern_borrow) {
            (CheckedValue::Felt(l), CheckedValue::Felt(r))
            | (CheckedValue::U32(l), CheckedValue::U32(r))
            | (CheckedValue::Bool(l), CheckedValue::Bool(r)) => Ok(self.context.op_eq(*l, *r)),
            _ => unreachable!("Unsupported match pattern between {:?} and {:?}", scrutinee_borrow, pattern_borrow),
        }
    }

    pub fn is_constant(&self, value: F) -> bool {
        let constant_types = [
            DPNOpType::Constant,
            DPNOpType::ConstantTrue,
            DPNOpType::ConstantFalse,
            DPNOpType::ConstantU32,
        ];

        constant_types.contains(&self.context.get_op_type(value))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{BufWriter, Write},
    };

    use insta::assert_snapshot;
    use plonky2::field::{goldilocks_field::GoldilocksField, types::Field};
    use psy_common::data::qhashout::QHashOut;
    use psy_common_circuit::circuits::zk_signature3::manager::SimplePsyZKSignatureManager;
    use psy_config::network_constants::GLOBAL_USER_TREE_HEIGHT;
    use psy_crypto::signature::zk::wallet::SimplePsyPrivateKey;
    use psy_data::{
        config::store_config::{PsyHasher, C, D},
        qblock::cmds::register_user::QBCRegisterUser,
    };
    use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
    use psy_store::prepare_environment_with_real_contract;
    use psy_vm::{
        dpn::{
            ops::{exec_context::QExecContext, sym_felt::SymFeltRef},
            vm::compile::PsyCompileResult,
        },
        vm::exec::PsyEvalSessionResult,
    };
    use serial_test::serial;

    use super::*;

    #[test]
    #[serial]
    fn test_crates_resolve() {
        let entry: PathBuf = "../tests/module_test/foo/src/main.psy".into();
        let dependency_entry: PathBuf = "../tests/module_test/bar/src/lib.psy".into();

        let mut crate_path_graph = Graph::new();
        crate_path_graph.add_node(entry.clone());
        crate_path_graph.add_edge(entry.clone(), dependency_entry);

        let result = super::interpret(Option::<String>::None, vec!["main".into()], crate_path_graph).unwrap();
        println!("compile_result: {:?}", result.compile_results);
        #[allow(static_mut_refs)]
        unsafe {
            STD_PRIMITIVE_SCOPE_ID.take().unwrap()
        };
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_interpreter() {
        psy_common::setup_logging().ok();

        insta::glob!("../../tests", "{struct*.psy,fn_test.psy,fn_chain_call_test.psy}", |path| {
            let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
            let (mut typechecker, mut ctx) = interpreter.typecheck_single(path.into()).unwrap();

            let compile_results = interpreter
                .interpret(
                    &mut typechecker,
                    &mut ctx,
                    None,
                    vec!["main"],
                    |context, (method_name, method_id, outputs)| {
                        PsyCompileResult::compile_exec(method_name, method_id, &context.store, &context, &outputs)
                    },
                )
                .unwrap();

            let priv_key = QHashOut::rand();
            let wallet = SimplePsyZKSignatureManager::<C, D>::new();
            let priv_key_w = SimplePsyPrivateKey::new(priv_key);
            let pub_key_param = priv_key_w.get_public_key_param::<PsyHasher>();
            let contract_state_tree_height = GLOBAL_USER_TREE_HEIGHT as usize;

            let deployer = QHashOut::rand();
            let (_circuits, deploy_cmd) =
                gen_contract_deploy_and_circuits_for_functions::<C, D>(deployer, contract_state_tree_height as u8, &compile_results).unwrap();

            let mut lps = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    prepare_environment_with_real_contract(
                        vec![QBCRegisterUser::new(wallet.get_zksig_circuit_fingerprint(), pub_key_param)],
                        vec![deploy_cmd],
                        None,
                        None,
                        None,
                    )
                    .await
                })
            })
            .unwrap();
            let contract_id = GoldilocksField::from_canonical_u64(2);

            let cfc_input = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    PsyEvalSessionResult::new()
                        .exec_contract_call(&mut lps, contract_id, &compile_results[0], vec![])
                        .await
                })
            })
            .unwrap();

            println!("result_vm: {:?}", cfc_input.outputs);
            #[allow(static_mut_refs)]
            unsafe {
                STD_PRIMITIVE_SCOPE_ID.take().unwrap()
            };

            assert_snapshot!(ctx.debug_scope(ScopeId::root()))
        });
    }

    #[test]
    #[serial]
    fn test_format_file() {
        psy_common::setup_logging().ok();

        insta::glob!("../../tests", "*_test.psy", |path| {
            let entry: PathBuf = path.into();
            let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
            let (_typechecker, mut ctx) = interpreter.typecheck_single(entry.clone()).unwrap();

            #[allow(static_mut_refs)]
            unsafe {
                STD_PRIMITIVE_SCOPE_ID.take().unwrap()
            };

            let formatted_content = ctx.format_file(&entry).unwrap();

            let file = File::create(&entry).unwrap();
            let mut writer = BufWriter::new(file);
            writer.write_all(formatted_content.as_bytes()).unwrap();
            writer.flush().unwrap();

            assert!(interpreter.typecheck_single(entry.clone()).is_ok(), "{}", entry.display());

            #[allow(static_mut_refs)]
            unsafe {
                STD_PRIMITIVE_SCOPE_ID.take().unwrap()
            };
        });
    }
}
