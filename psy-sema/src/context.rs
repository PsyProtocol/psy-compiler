use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex as PetGraphIndex};
use psy_ast::{
    CrateId, DefId, DefinitionNode, ExprId, ExprNode, Ident, IdentId, InsertPosition, ModuleId, ModuleNode, NodeId, NodeInfo, NodeType, Program,
    StmtId, StmtNode, VisitorContext,
};
use psy_common::Graph;
use psy_vm::dpn::ops::context_trait::ContextFelt;

use crate::{ExpectedFunctionSignature, LocationIndices, ReferenceId, SymbolTable, Type, TypeId};

pub struct TypeCheckerVisitorContext<F: Clone + From<u32> + ContextFelt, C> {
    path_stack: Vec<NodeId>,
    expected_signature_stack: Vec<ExpectedFunctionSignature>,
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
            expected_signature_stack: vec![],
            program,
            symbols: SymbolTable::new(),
            reference_graph: DiGraph::new(),
            reference_graph_indices: HashMap::new(),
            location_indices: LocationIndices::default(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn expected_signature(&self) -> Option<ExpectedFunctionSignature> {
        self.expected_signature_stack.last().cloned()
    }

    pub fn ancestor_expected_signature(&self, offset_from_top: usize) -> Option<&ExpectedFunctionSignature> {
        let len = self.expected_signature_stack.len();
        if offset_from_top >= len {
            None
        } else {
            self.expected_signature_stack.get(len - 1 - offset_from_top)
        }
    }

    pub fn expected_signature_path(&self) -> &[ExpectedFunctionSignature] {
        &self.expected_signature_stack
    }

    pub fn push_expected_signature(&mut self, sig: ExpectedFunctionSignature) {
        self.expected_signature_stack.push(sig);
    }

    pub fn pop_expected_signature(&mut self) {
        self.expected_signature_stack.pop();
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

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use psy_ast::{
        Comment, DefId, DefinitionNode, ExprId, ExprNode, IdentId, Identifier, Location, ModuleId, NodeId, NodeType, StmtId, StmtNode, UseNode,
        ValueNode, Visibility,
    };
    use psy_vm::dpn::ops::sym_felt::SymFeltRef;

    use super::*;
    use crate::{CheckedStructField, CheckedStructNode, ExpectedFunctionSignature, ExpectedReturnType, FELT_TYPE, ScopeId, VOID_TYPE};

    type Ctx = TypeCheckerVisitorContext<SymFeltRef, ()>;

    fn ctx() -> Ctx {
        TypeCheckerVisitorContext::new(Program::new())
    }

    fn signature(parameter: TypeId) -> ExpectedFunctionSignature {
        ExpectedFunctionSignature {
            parameters: vec![parameter],
            return_type: ExpectedReturnType::Known(VOID_TYPE),
            receiver: None,
        }
    }

    fn felt_value() -> ExprNode<SymFeltRef> {
        ExprNode::Value(ValueNode::Felt(SymFeltRef(7), Location::default()))
    }

    fn use_definition() -> DefinitionNode {
        DefinitionNode::Use(UseNode {
            visibility: Visibility::Private,
            kind: Identifier::new(IdentId(1), Location::default()),
            segments: vec![],
            target: None,
            comments: vec![Comment::new_line("doc".to_string(), Location::default())],
            location: Location::default(),
        })
    }

    #[test]
    fn expected_signature_stack_lifecycle() {
        let mut context = ctx();
        assert_eq!(context.expected_signature(), None);
        assert_eq!(context.ancestor_expected_signature(0), None);

        let outer = signature(FELT_TYPE);
        context.push_expected_signature(outer.clone());
        let inner = signature(VOID_TYPE);
        context.push_expected_signature(inner.clone());

        assert_eq!(context.expected_signature(), Some(inner.clone()));
        assert_eq!(context.ancestor_expected_signature(0), Some(&inner));
        assert_eq!(context.ancestor_expected_signature(1), Some(&outer));
        assert_eq!(context.ancestor_expected_signature(2), None);
        assert_eq!(context.expected_signature_path().len(), 2);

        context.pop_expected_signature();
        assert_eq!(context.expected_signature(), Some(outer));
    }

    #[test]
    fn node_id_stack_tracks_ancestry() {
        let mut context = ctx();
        context.push_node_id(NodeId::Expr(ExprId(1)));
        context.push_node_id(NodeId::Stmt(StmtId(2)));
        context.push_node_id(NodeId::Def(DefId(3)));

        assert_eq!(context.node_id(), NodeId::Def(DefId(3)));
        assert_eq!(context.ancestor_node_id(0), NodeId::Def(DefId(3)));
        assert_eq!(context.ancestor_node_id(1), NodeId::Stmt(StmtId(2)));
        assert_eq!(context.ancestor_node_id(2), NodeId::Expr(ExprId(1)));
        assert_eq!(context.node_path().len(), 3);

        context.pop_node_id();
        assert_eq!(context.node_id(), NodeId::Stmt(StmtId(2)));
    }

    #[test]
    fn node_type_dispatches_by_node_kind() {
        let mut context = ctx();

        let expr_id = context.program.exprs.alloc_item(felt_value());
        context.push_node_id(NodeId::Expr(expr_id));
        assert_eq!(context.node_type(), NodeType::ValueExpr);
        assert_eq!(context.ancestor_node_type(0), NodeType::ValueExpr);

        let stmt_id = context.program.stmts.alloc_item(StmtNode::Expression(expr_id));
        context.push_node_id(NodeId::Stmt(stmt_id));
        assert_eq!(context.node_type(), NodeType::ExpressionStmt);

        let def_id = context.program.defs.alloc_item(use_definition());
        context.push_node_id(NodeId::Def(def_id));
        assert_eq!(context.node_type(), NodeType::UseDef);

        context.push_node_id(NodeId::Module(ModuleId(0)));
        assert_eq!(context.node_type(), NodeType::Module);
        assert_eq!(context.ancestor_node_type(3), NodeType::ValueExpr);
    }

    #[test]
    fn ident_interning_round_trips() {
        let mut context = ctx();
        let id = context.intern("size_of_it");
        assert_eq!(context.ident(id).0, "size_of_it");

        let lambda_a = context.intern_lambda();
        let lambda_b = context.intern_lambda();
        assert_ne!(lambda_a, lambda_b);
    }

    #[test]
    fn program_accessors_expose_arena_items() {
        let mut context = ctx();
        let expr_id = context.program.exprs.alloc_item(felt_value());
        let stmt_id = context.program.stmts.alloc_item(StmtNode::Expression(expr_id));
        let def_id = context.alloc_definition(use_definition());

        assert!(matches!(context.expression(expr_id), ExprNode::Value(ValueNode::Felt(_, _))));
        assert!(matches!(context.statement(stmt_id), StmtNode::Expression(id) if *id == expr_id));
        assert!(matches!(context.definition(def_id), DefinitionNode::Use(_)));
        assert_eq!(context.program().exprs[expr_id], ExprNode::Value(ValueNode::Felt(SymFeltRef(7), Location::default())));
        assert_eq!(context.dependency_graph().nodes().len(), 0);
    }

    #[test]
    fn size_of_counts_primitives_and_nested_struct_fields() {
        let mut context = ctx();
        let felt = context.symbols.create_type(Type::Felt).unwrap();
        let bool_ty = context.symbols.create_type(Type::Bool).unwrap();
        let u32_ty = context.symbols.create_type(Type::U32).unwrap();
        assert_eq!(context.size_of(felt), 1);
        assert_eq!(context.size_of(bool_ty), 1);
        assert_eq!(context.size_of(u32_ty), 1);

        let point = {
            let mut fields = IndexMap::new();
            fields.insert(
                Identifier::new(IdentId(10), Location::default()),
                CheckedStructField::new(felt, vec![], Visibility::Public, vec![], Location::default()),
            );
            fields.insert(
                Identifier::new(IdentId(11), Location::default()),
                CheckedStructField::new(felt, vec![], Visibility::Public, vec![], Location::default()),
            );
            context
                .symbols
                .create_type(Type::Struct(CheckedStructNode {
                    name: Identifier::new(IdentId(12), Location::default()),
                    generic_parameters: vec![],
                    fields,
                    scope_id: ScopeId(0),
                    attrs: vec![],
                    visibility: Visibility::Public,
                    comments: vec![],
                    location: Location::default(),
                    type_id: TypeId(999),
                }))
                .unwrap()
        };
        assert_eq!(context.size_of(point), 2);

        let outer = {
            let mut fields = IndexMap::new();
            fields.insert(
                Identifier::new(IdentId(13), Location::default()),
                CheckedStructField::new(point, vec![], Visibility::Public, vec![], Location::default()),
            );
            fields.insert(
                Identifier::new(IdentId(14), Location::default()),
                CheckedStructField::new(felt, vec![], Visibility::Public, vec![], Location::default()),
            );
            context
                .symbols
                .create_type(Type::Struct(CheckedStructNode {
                    name: Identifier::new(IdentId(15), Location::default()),
                    generic_parameters: vec![],
                    fields,
                    scope_id: ScopeId(0),
                    attrs: vec![],
                    visibility: Visibility::Public,
                    comments: vec![],
                    location: Location::default(),
                    type_id: TypeId(998),
                }))
                .unwrap()
        };
        assert_eq!(context.size_of(outer), 3);
    }

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn size_of_rejects_unsupported_shapes() {
        let mut context = ctx();
        let felt = context.symbols.create_type(Type::Felt).unwrap();
        let tuple = context.symbols.create_type(Type::Tuple(vec![felt, felt])).unwrap();
        let _ = context.size_of(tuple);
    }
}
