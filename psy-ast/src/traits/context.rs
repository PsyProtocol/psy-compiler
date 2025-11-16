use enum_as_inner::EnumAsInner;
use psy_common::Graph;

use crate::{CrateId, DefId, DefinitionNode, ExprId, ExprNode, Ident, IdentId, ModuleId, ModuleNode, NodeInfo, Program, StmtId, StmtNode};

#[derive(Debug, Clone, PartialEq)]
pub enum InsertPosition {
    Before(NodeId),
    After(NodeId),
    Front,
    End,
}

#[derive(Copy, Debug, Clone, PartialEq, EnumAsInner, Hash, Eq)]
pub enum NodeId {
    Expr(ExprId),
    Stmt(StmtId),
    Def(DefId),
    Module(ModuleId),
}

impl From<ExprId> for NodeId {
    fn from(value: ExprId) -> Self {
        Self::Expr(value)
    }
}

impl From<StmtId> for NodeId {
    fn from(value: StmtId) -> Self {
        Self::Stmt(value)
    }
}

impl From<DefId> for NodeId {
    fn from(value: DefId) -> Self {
        Self::Def(value)
    }
}

impl From<ModuleId> for NodeId {
    fn from(value: ModuleId) -> Self {
        Self::Module(value)
    }
}

#[derive(Copy, Debug, Clone, PartialEq, EnumAsInner)]
pub enum NodeType {
    PathExpr,
    ValueExpr,
    BinaryExpr,
    UnaryExpr,
    CallExpr,
    MemberCallExpr,
    CastExpr,
    MemberAccessExpr,
    IndexAccessExpr,
    IntrinsicExpr,
    LambdaFunctionExpr,
    BlockExpr,
    IfExpr,
    TupleExpr,
    TupleAccessExpr,
    MatchExpr,
    ParenthesesExpr,

    WhileStmt,
    AssignmentStmt,
    VariableStmt,
    DefinitionStmt,
    ExpressionStmt,
    ReturnStmt,
    IntrinsicStmt,
    ForStmt,
    FunctionDef,
    StructDef,
    EnumDef,
    ImplDef,
    TraitImplDef,
    TraitDef,
    TypeAliasDef,
    ConstDef,
    UseDef,
    Comment,

    Module,
}

impl NodeType {
    pub fn is_function(&self) -> bool {
        match self {
            NodeType::FunctionDef | NodeType::LambdaFunctionExpr => true,
            _ => false,
        }
    }
}

pub trait VisitorContext<F: Clone + From<u32>, C> {
    type Expr: NodeInfo;
    type Stmt: NodeInfo;
    type Definition: NodeInfo;
    type Program;

    fn node_id(&self) -> NodeId;
    fn ancestor_node_id(&self, offset_from_top: usize) -> NodeId;
    fn node_path(&self) -> &[NodeId];
    fn push_node_id(&mut self, node_id: NodeId);
    fn pop_node_id(&mut self);
    fn node_type(&self) -> NodeType;
    fn ancestor_node_type(&self, offset_from_top: usize) -> NodeType;
    fn ident(&self, id: impl Into<IdentId>) -> &Ident;
    fn intern<S: Into<Ident>>(&mut self, s: S) -> IdentId;
    fn module(&self, module_id: ModuleId) -> &ModuleNode;
    fn module_children(&self, module_id: ModuleId) -> &[ModuleId];
    fn program(&self) -> &Self::Program;
    fn dependency_graph(&self) -> Graph<CrateId>;
    fn alloc_expression(&mut self, expr: Self::Expr) -> ExprId;
    fn alloc_statement(&mut self, stmt: Self::Stmt) -> StmtId;
    fn alloc_definition(&mut self, definition: Self::Definition) -> DefId;
    fn expression(&self, expr_id: ExprId) -> &Self::Expr;
    fn statement(&self, stmt_id: StmtId) -> &Self::Stmt;
    fn definition(&self, def_id: DefId) -> &Self::Definition;
    fn insert_definition(&mut self, definition: Self::Definition, pos: InsertPosition);
    fn replace_definition(&mut self, def_id: DefId, definition: Self::Definition);
    fn replace_statement(&mut self, stmt_id: StmtId, statement: Self::Stmt);
    fn intern_lambda(&mut self) -> IdentId;
}

pub struct DefaultVisitorContext<'a, F: Clone + From<u32>, C> {
    path_stack: Vec<NodeId>,
    program: &'a mut Program<F>,
    _marker: std::marker::PhantomData<(F, C)>,
}

impl<'a, F: Clone + From<u32>, C> DefaultVisitorContext<'a, F, C> {
    pub fn new(program: &'a mut Program<F>) -> Self {
        DefaultVisitorContext {
            path_stack: vec![],
            program,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a, F: Clone + From<u32>, C> VisitorContext<F, C> for DefaultVisitorContext<'a, F, C> {
    type Expr = ExprNode<F>;

    type Stmt = StmtNode;

    type Definition = DefinitionNode;

    type Program = Program<F>;

    fn node_id(&self) -> NodeId {
        self.path_stack.last().unwrap().clone()
    }

    fn ancestor_node_id(&self, offset_from_top: usize) -> NodeId {
        self.path_stack[self.path_stack.len() - 1 - offset_from_top].clone()
    }

    fn node_path(&self) -> &[NodeId] {
        &self.path_stack
    }

    fn push_node_id(&mut self, node_id: NodeId) {
        self.path_stack.push(node_id);
    }

    fn pop_node_id(&mut self) {
        self.path_stack.pop();
    }

    fn node_type(&self) -> NodeType {
        match self.node_id() {
            NodeId::Expr(expr_id) => self.expression(expr_id).node_type(),
            NodeId::Stmt(stmt_id) => self.statement(stmt_id).node_type(),
            NodeId::Def(def_id) => self.definition(def_id).node_type(),
            NodeId::Module(_) => NodeType::Module,
        }
    }

    fn ancestor_node_type(&self, offset_from_top: usize) -> NodeType {
        match self.ancestor_node_id(offset_from_top) {
            NodeId::Expr(expr_id) => self.expression(expr_id).node_type(),
            NodeId::Stmt(stmt_id) => self.statement(stmt_id).node_type(),
            NodeId::Def(def_id) => self.definition(def_id).node_type(),
            NodeId::Module(_) => NodeType::Module,
        }
    }

    fn ident(&self, id: impl Into<IdentId>) -> &Ident {
        &self.program.interner[id.into()]
    }

    fn intern<S: Into<Ident>>(&mut self, s: S) -> IdentId {
        self.program.interner.intern_ident(s)
    }

    fn module(&self, module_id: ModuleId) -> &ModuleNode {
        self.program.modules[module_id].data()
    }

    fn module_children(&self, module_id: ModuleId) -> &[ModuleId] {
        self.program.modules[module_id].children()
    }

    fn program(&self) -> &Program<F> {
        &self.program
    }

    fn dependency_graph(&self) -> Graph<CrateId> {
        self.program.dependency_graph.clone()
    }

    fn expression(&self, expr_id: ExprId) -> &Self::Expr {
        &self.program.exprs[expr_id]
    }

    fn statement(&self, stmt_id: StmtId) -> &Self::Stmt {
        &self.program.stmts[stmt_id]
    }

    fn definition(&self, def_id: DefId) -> &Self::Definition {
        &self.program.defs[def_id]
    }

    fn insert_definition(&mut self, definition: Self::Definition, pos: InsertPosition) {
        let def_id = self.program.defs.alloc_item(definition);
        assert!(self.ancestor_node_type(1) == NodeType::Module);
        let module_id = self.ancestor_node_id(1).as_module().unwrap().clone();

        match pos {
            InsertPosition::Front => {
                self.program.modules[module_id].data_mut().definitions.insert(0, def_id);
            }
            InsertPosition::End => {
                self.program.modules[module_id].data_mut().definitions.push(def_id);
            }
            InsertPosition::Before(before) => {
                let idx = self.program.modules[module_id]
                    .data()
                    .definitions
                    .iter()
                    .position(|d| d == before.as_def().unwrap())
                    .unwrap();
                self.program.modules[module_id].data_mut().definitions.insert(idx, def_id);
            }
            InsertPosition::After(after) => {
                let idx = self.program.modules[module_id]
                    .data()
                    .definitions
                    .iter()
                    .position(|d| d == after.as_def().unwrap())
                    .unwrap();
                self.program.modules[module_id].data_mut().definitions.insert(idx + 1, def_id);
            }
        };
    }

    fn alloc_expression(&mut self, expr: Self::Expr) -> ExprId {
        self.program.exprs.alloc_item(expr)
    }

    fn alloc_statement(&mut self, stmt: Self::Stmt) -> StmtId {
        self.program.stmts.alloc_item(stmt)
    }

    fn alloc_definition(&mut self, definition: Self::Definition) -> DefId {
        self.program.defs.alloc_item(definition)
    }

    fn replace_definition(&mut self, def_id: DefId, definition: Self::Definition) {
        self.program.defs.replace_item(def_id, definition);
    }

    fn replace_statement(&mut self, stmt_id: StmtId, statement: Self::Stmt) {
        self.program.stmts.replace_item(stmt_id, statement);
    }

    fn intern_lambda(&mut self) -> IdentId {
        self.program.interner.intern_lambda()
    }
}
