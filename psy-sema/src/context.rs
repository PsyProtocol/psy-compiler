use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex as PetGraphIndex};
use psy_ast::{
    CrateId, DefId, DefinitionNode, ExprId, ExprNode, Ident, IdentId, InsertPosition, ModuleId, ModuleNode, NodeId, NodeInfo, NodeType, Program,
    StmtId, StmtNode, VisitorContext,
};
use psy_common::Graph;
use psy_vm::dpn::ops::context_trait::ContextFelt;

use crate::{LocationIndices, ReferenceId, SymbolTable, Type, TypeId};

pub struct TypeCheckerVisitorContext<F: Clone + From<u32> + ContextFelt, C> {
    path_stack: Vec<NodeId>,
    pub program: Program<F>,
    pub symbols: SymbolTable<F>,

    pub(crate) reference_graph: DiGraph<ReferenceId, ()>,
    pub(crate) reference_graph_indices: HashMap<ReferenceId, PetGraphIndex>,
    pub(crate) location_indices: LocationIndices,

    _marker: std::marker::PhantomData<(F, C)>,
}

impl<F: Clone + From<u32> + ContextFelt, C> TypeCheckerVisitorContext<F, C> {
    pub fn new(program: Program<F>) -> Self {
        TypeCheckerVisitorContext {
            path_stack: vec![],
            program,
            symbols: SymbolTable::new(),
            reference_graph: DiGraph::new(),
            reference_graph_indices: HashMap::new(),
            location_indices: LocationIndices::default(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<F: Clone + From<u32> + ContextFelt, C> VisitorContext<F, C> for TypeCheckerVisitorContext<F, C> {
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

    fn insert_definition(&mut self, _definition: Self::Definition, _pos: InsertPosition) {
        unimplemented!()
    }

    fn alloc_expression(&mut self, _expr: Self::Expr) -> ExprId {
        unimplemented!()
    }

    fn alloc_statement(&mut self, _stmt: Self::Stmt) -> StmtId {
        unimplemented!()
    }

    fn alloc_definition(&mut self, definition: Self::Definition) -> DefId {
        self.program.defs.alloc_item(definition)
    }

    fn replace_definition(&mut self, _def_id: DefId, _definition: Self::Definition) {
        unimplemented!()
    }

    fn replace_statement(&mut self, _stmt_id: StmtId, _statement: Self::Stmt) {
        unimplemented!()
    }

    fn intern_lambda(&mut self) -> IdentId {
        self.program.interner.intern_lambda()
    }
}

impl<F: Clone + From<u32> + ContextFelt, C> TypeCheckerVisitorContext<F, C> {
    pub fn size_of(&self, type_id: TypeId) -> usize {
        match &self.symbols[type_id] {
            Type::Felt => 1usize,
            Type::Bool => 1usize,
            Type::U32 => 1usize,
            Type::Struct(s) => s.fields.iter().map(|(_, field)| self.size_of(field.ty)).sum(),
            _ => todo!(),
        }
    }
}
