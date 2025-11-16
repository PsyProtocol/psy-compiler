use std::collections::HashMap;

use crate::*;

pub trait AstVisitor<F: Clone + From<u32>, C> {
    type Expr: NodeInfo;
    type Stmt: NodeInfo;
    type Definition: NodeInfo;
    type ExprResult;
    type StmtResult: From<Self::ExprResult> + From<Self::DefinitionResult>;
    type DefinitionResult;
    type Context: VisitorContext<F, C, Expr = Self::Expr, Stmt = Self::Stmt, Definition = Self::Definition>;
    type Error: std::fmt::Debug + From<psy_common::Error>;

    fn visit_expr(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        ctx.push_node_id(NodeId::from(expr_id));
        let res = match ctx.expression(expr_id).node_type() {
            NodeType::PathExpr => self.visit_path(expr_id, ctx)?,
            NodeType::ValueExpr => self.visit_value(expr_id, ctx)?,
            NodeType::BinaryExpr => self.visit_binary(expr_id, ctx)?,
            NodeType::UnaryExpr => self.visit_unary(expr_id, ctx)?,
            NodeType::CallExpr => self.visit_call(expr_id, ctx)?,
            NodeType::MemberCallExpr => self.visit_member_call(expr_id, ctx)?,
            NodeType::CastExpr => self.visit_cast(expr_id, ctx)?,
            NodeType::IndexAccessExpr => self.visit_index_access(expr_id, ctx)?,
            NodeType::MemberAccessExpr => self.visit_member_access(expr_id, ctx)?,
            NodeType::IntrinsicExpr => self.visit_intrinsic_expr(expr_id, ctx)?,
            NodeType::LambdaFunctionExpr => self.visit_lambda_function(expr_id, ctx)?,
            NodeType::BlockExpr => self.visit_block_expr(expr_id, ctx)?,
            NodeType::IfExpr => self.visit_if_expr(expr_id, ctx)?,
            NodeType::TupleExpr => self.visit_tuple(expr_id, ctx)?,
            NodeType::TupleAccessExpr => self.visit_tuple_access(expr_id, ctx)?,
            NodeType::MatchExpr => self.visit_match(expr_id, ctx)?,
            NodeType::ParenthesesExpr => self.visit_parentheses(expr_id, ctx)?,
            _ => unreachable!(),
        };
        ctx.pop_node_id();
        Ok(res)
    }

    fn visit_definition(&mut self, def_id: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error> {
        ctx.push_node_id(NodeId::from(def_id));
        let res = match ctx.definition(def_id).node_type() {
            NodeType::FunctionDef => self.visit_function(def_id, ctx)?,
            NodeType::StructDef => self.visit_struct(def_id, ctx)?,
            NodeType::EnumDef => self.visit_enum(def_id, ctx)?,
            NodeType::ImplDef => self.visit_impl(def_id, ctx)?,
            NodeType::TraitImplDef => self.visit_trait_impl(def_id, ctx)?,
            NodeType::TraitDef => self.visit_trait(def_id, ctx)?,
            NodeType::TypeAliasDef => self.visit_type_alias(def_id, ctx)?,
            NodeType::ConstDef => self.visit_const(def_id, ctx)?,
            NodeType::UseDef => self.visit_use(def_id, ctx)?,
            _ => unreachable!(),
        };
        ctx.pop_node_id();
        Ok(res)
    }

    fn visit_stmt(&mut self, stmt_id: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        ctx.push_node_id(NodeId::from(stmt_id));
        let res = match ctx.statement(stmt_id).node_type() {
            NodeType::WhileStmt => self.visit_while(stmt_id, ctx)?,
            NodeType::ForStmt => self.visit_for(stmt_id, ctx)?,
            NodeType::AssignmentStmt => self.visit_assignment(stmt_id, ctx)?,
            NodeType::VariableStmt => self.visit_variable(stmt_id, ctx)?,
            NodeType::ReturnStmt => self.visit_return(stmt_id, ctx)?,
            NodeType::DefinitionStmt => {
                let def_id = ctx.statement(stmt_id).as_definition().unwrap().clone();
                Self::StmtResult::from(self.visit_definition(def_id, ctx)?)
            }
            NodeType::ExpressionStmt => {
                let expr_id = ctx.statement(stmt_id).as_expression().unwrap().clone();
                Self::StmtResult::from(self.visit_expr(expr_id, ctx)?)
            }
            NodeType::IntrinsicStmt => self.visit_intrinsic_stmt(stmt_id, ctx)?,
            _ => unreachable!(),
        };
        ctx.pop_node_id();
        Ok(res)
    }

    fn visit_use(&mut self, def_id: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error>;

    fn visit_module(&mut self, module_id: ModuleId, ctx: &mut Self::Context) -> Result<(), Self::Error> {
        ctx.push_node_id(NodeId::from(module_id));
        // Avoid clone
        let len = ctx.module(module_id).definitions.len();
        for i in 0..len {
            let definition = ctx.module(module_id).definitions[i];
            let _ = self.visit_definition(definition, ctx)?;
        }
        ctx.pop_node_id();

        Ok(())
    }

    fn visit_program(&mut self, ctx: &mut Self::Context) -> Result<(), Self::Error> {
        ctx.dependency_graph()
            .ts::<Self::Error>(&mut |&crate_id| self.visit_crate(crate_id, ctx))?;

        Ok(())
    }

    fn visit_crate(&mut self, crate_id: CrateId, ctx: &mut Self::Context) -> Result<(), Self::Error> {
        let entry_module_id = ModuleId::from(crate_id);
        self.visit_module_tree(entry_module_id, ctx)
    }

    fn visit_module_tree(&mut self, module_id: ModuleId, ctx: &mut Self::Context) -> Result<(), Self::Error> {
        // Visit all child modules recursively
        let children = ctx.module_children(module_id).to_vec();
        for child_id in children {
            self.visit_module_tree(child_id, ctx)?;
        }

        self.visit_module(module_id, ctx)?;

        Ok(())
    }

    fn visit_path(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_index_access(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_member_access(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_intrinsic_expr(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_value(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_binary(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_unary(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_call(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_member_call(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_cast(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_lambda_function(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;

    fn visit_block_expr(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;

    fn visit_while(&mut self, node: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error>;
    fn visit_for(&mut self, node: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error>;
    fn visit_assignment(&mut self, node: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error>;
    fn visit_variable(&mut self, node: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error>;
    fn visit_return(&mut self, expr: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error>;
    fn visit_intrinsic_stmt(&mut self, node: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error>;
    fn visit_match(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_parentheses(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;

    fn visit_impl(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error>;
    fn visit_trait_impl(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error>;
    fn visit_trait(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error>;
    fn visit_function(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error>;
    fn visit_struct(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error>;
    fn visit_enum(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error>;
    fn visit_if_expr(&mut self, node: ExprId, ctx: &mut Self::Context) -> std::result::Result<Self::ExprResult, Self::Error>;
    fn visit_type_alias(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error>;
    fn visit_const(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error>;
    fn visit_tuple(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
    fn visit_tuple_access(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error>;
}
