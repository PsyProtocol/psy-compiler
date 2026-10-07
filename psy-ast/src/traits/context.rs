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

#[cfg(test)]
mod tests {
    use psy_common::FileId;

    use crate::{Comment, Identifier, Location, ModuleNode, UseNode, ValueNode, Visibility};

    use super::*;

    type Ctx<'a> = DefaultVisitorContext<'a, u32, ()>;

    fn module_node(name: usize) -> ModuleNode {
        ModuleNode {
            name: Identifier::new(IdentId(name), Location::default()),
            file_id: FileId(0),
            modules: vec![],
            inline_modules: vec![],
            definitions: vec![],
            visibility: Visibility::Public,
            comments: vec![],
            location: Location::default(),
        }
    }

    fn use_definition(id: usize) -> DefinitionNode {
        DefinitionNode::Use(UseNode {
            visibility: Visibility::Private,
            kind: Identifier::new(IdentId(id), Location::default()),
            segments: vec![],
            target: None,
            comments: vec![Comment::new_line("doc".to_string(), Location::default())],
            location: Location::default(),
        })
    }

    fn felt_value() -> ExprNode<u32> {
        ExprNode::Value(ValueNode::Felt(7u32, Location::default()))
    }

    #[test]
    fn function_node_types_are_functions() {
        assert!(NodeType::FunctionDef.is_function());
        assert!(NodeType::LambdaFunctionExpr.is_function());
        assert!(!NodeType::Module.is_function());
        assert!(!NodeType::UseDef.is_function());
        assert!(!NodeType::ValueExpr.is_function());
    }

    #[test]
    fn node_id_stack_tracks_ancestry() {
        let mut program = Program::<u32>::new();
        let mut context = Ctx::new(&mut program);
        context.push_node_id(NodeId::Expr(ExprId(1)));
        context.push_node_id(NodeId::Stmt(StmtId(2)));
        context.push_node_id(NodeId::Def(DefId(3)));

        assert_eq!(context.node_id(), NodeId::Def(DefId(3)));
        assert_eq!(context.ancestor_node_id(0), NodeId::Def(DefId(3)));
        assert_eq!(context.ancestor_node_id(2), NodeId::Expr(ExprId(1)));
        assert_eq!(context.node_path().len(), 3);

        context.pop_node_id();
        assert_eq!(context.node_id(), NodeId::Stmt(StmtId(2)));
    }

    #[test]
    fn node_type_dispatches_by_node_kind() {
        let mut program = Program::<u32>::new();
        let expr_id = program.exprs.alloc_item(felt_value());
        let stmt_id = program.stmts.alloc_item(StmtNode::Expression(expr_id));
        let def_id = program.defs.alloc_item(use_definition(1));

        let mut context = Ctx::new(&mut program);
        context.push_node_id(NodeId::Expr(expr_id));
        assert_eq!(context.node_type(), NodeType::ValueExpr);
        context.push_node_id(NodeId::Stmt(stmt_id));
        assert_eq!(context.ancestor_node_type(1), NodeType::ValueExpr);
        assert_eq!(context.node_type(), NodeType::ExpressionStmt);
        context.push_node_id(NodeId::Def(def_id));
        assert_eq!(context.node_type(), NodeType::UseDef);
        context.push_node_id(NodeId::Module(ModuleId(0)));
        assert_eq!(context.node_type(), NodeType::Module);
        assert_eq!(context.ancestor_node_type(3), NodeType::ValueExpr);
    }

    #[test]
    fn ident_interning_round_trips() {
        let mut program = Program::<u32>::new();
        let mut context = Ctx::new(&mut program);
        let id = context.intern("inserted");
        assert_eq!(context.ident(id).0, "inserted");
        assert_ne!(context.intern_lambda(), context.intern_lambda());
    }

    #[test]
    fn modules_are_indexed_and_traversed() {
        let mut program = Program::<u32>::new();
        let root_id = program.modules.add_node(module_node(10));
        let child_id = program.modules.add_node(module_node(11));
        program.add_module_child(Some(root_id), child_id);

        let context = Ctx::new(&mut program);
        assert_eq!(context.module(root_id).name.id, IdentId(10));
        assert_eq!(context.module_children(root_id), &[child_id]);
        assert_eq!(context.module_children(child_id), &[]);
        assert_eq!(context.program().modules[root_id].data().name.id, IdentId(10));
        assert_eq!(context.dependency_graph().nodes().len(), 0);
    }

    #[test]
    fn insert_definition_places_items_relative_to_existing_ones() {
        let mut program = Program::<u32>::new();
        let module_id = program.modules.add_node(module_node(20));
        let first = program.defs.alloc_item(use_definition(21));
        let second = program.defs.alloc_item(use_definition(22));
        program.modules[module_id].data_mut().definitions = vec![first, second];

        let mut context = Ctx::new(&mut program);
        // insert_definition resolves the target module one level up the stack.
        context.push_node_id(NodeId::Module(module_id));
        context.push_node_id(NodeId::Def(first));

        context.insert_definition(use_definition(23), InsertPosition::Front);
        context.insert_definition(use_definition(24), InsertPosition::End);
        context.insert_definition(use_definition(25), InsertPosition::Before(NodeId::Def(second)));
        context.insert_definition(use_definition(26), InsertPosition::After(NodeId::Def(first)));

        let inserted: Vec<usize> = program.modules[module_id]
            .data()
            .definitions
            .iter()
            .map(|def_id| program.defs[*def_id].as_use().unwrap().kind.id.0 as usize)
            .collect();
        assert_eq!(inserted, vec![23, 21, 26, 25, 22, 24]);
    }

    #[test]
    fn allocation_and_replacement_round_trip() {
        let mut program = Program::<u32>::new();
        let mut context = Ctx::new(&mut program);

        let expr_id = context.alloc_expression(felt_value());
        let stmt_id = context.alloc_statement(StmtNode::Expression(expr_id));
        let def_id = context.alloc_definition(use_definition(30));

        assert!(matches!(context.expression(expr_id), ExprNode::Value(ValueNode::Felt(_, _))));
        assert!(matches!(context.statement(stmt_id), StmtNode::Expression(_)));
        assert!(matches!(context.definition(def_id), DefinitionNode::Use(_)));

        context.replace_definition(def_id, use_definition(31));
        assert_eq!(context.definition(def_id).as_use().unwrap().kind.id, IdentId(31));
        context.replace_statement(stmt_id, StmtNode::Definition(def_id));
        assert!(matches!(context.statement(stmt_id), StmtNode::Definition(id) if *id == def_id));
    }
}
