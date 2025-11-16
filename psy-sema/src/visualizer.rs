use itertools::Itertools;
use psy_ast::{Comment, DefId, DefinitionNode, EnumVariant, ExprId, ExprNode, IdentId, StmtId, StmtNode, Visibility, VisitorContext};
use psy_vm::dpn::ops::context_trait::ContextFelt;

use crate::{CheckedEnumNode, CheckedFunctionNode, CheckedStructNode, CheckedTraitNode, ScopeId, Type, TypeCheckerVisitorContext, TypeId, VarId};

macro_rules! writeln {
    ($fmt:expr, $($arg:tt)*) => {
        $fmt.writeln(format!($($arg)*))
    };
}
macro_rules! write {
    ($fmt:expr, $($arg:tt)*) => {
        $fmt.write(format!($($arg)*))
    };
}
pub trait AstVisualizer<F: Clone + From<u32>, C>: VisitorContext<F, C> {
    type DebugResult;

    fn debug_scope(&self, scope_id: ScopeId) -> Self::DebugResult;
    fn debug_type(&self, type_id: TypeId) -> Self::DebugResult;

    fn debug_variable(&self, ident_id: IdentId, var_id: VarId) -> Self::DebugResult;

    fn debug_expr(&self, expr_id: ExprId) -> Self::DebugResult;

    fn debug_stmt(&self, statement: StmtId) -> Self::DebugResult;

    fn debug_definition(&self, def_id: DefId) -> Self::DebugResult;
}

struct IndentFormatter {
    output: String,
    indent: usize,
}

impl IndentFormatter {
    const INDENT: &'static str = "    ";

    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    pub fn indent(&mut self) {
        self.indent += 1;
    }

    pub fn dedent(&mut self) {
        self.indent -= 1;
    }

    pub fn write<T: AsRef<str>>(&mut self, s: T) {
        self.output.push_str(s.as_ref());
    }

    pub fn writeln<T: AsRef<str>>(&mut self, s: T) {
        self.write_indent();
        self.output.push_str(s.as_ref());
        self.output.push_str("\n");
    }

    pub fn write_indent(&mut self) {
        self.output.push_str(&Self::INDENT.repeat(self.indent));
    }

    pub fn finish(self) -> String {
        self.output
    }
    pub fn finish_without_new_line(self) -> String {
        self.finish().trim_end_matches('\n').to_string()
    }
}

struct TypeCheckerVisitorVisualizerInner<'a, F: Clone + From<u32> + ContextFelt, C> {
    pub context: &'a TypeCheckerVisitorContext<F, C>,
}

impl<'a, F: Clone + From<u32> + ContextFelt, C> TypeCheckerVisitorVisualizerInner<'a, F, C> {
    pub fn debug_scope(&self, scope_id: ScopeId, fmt: &mut IndentFormatter) {
        let scope = &self.context.symbols[scope_id];
        writeln!(fmt, "{:?} ({:?})", scope_id, scope.kind);
        fmt.indent();

        if !scope.consts.is_empty() {
            writeln!(fmt, "Constants:");
            fmt.indent();
            for (indent_id, const_id) in &scope.consts {
                writeln!(fmt, "{:?}: {:?}", const_id, self.context.ident(*indent_id));
            }
            fmt.dedent();
        }

        if !scope.variables.is_empty() {
            writeln!(fmt, "Variables:");
            fmt.indent();
            for (indent_id, var_id) in &scope.variables {
                self.debug_variable_inline(*indent_id, *var_id, fmt);
                // writeln!(fmt, "{:?}: {:?}", var_id,
                // self.context.ident(*indent_id));
            }
            fmt.dedent();
        }

        if !scope.types.is_empty() {
            writeln!(fmt, "Types:");
            fmt.indent();
            for (_type_key, type_id) in &scope.types {
                let ty = &self.context.symbols[*type_id];
                match &ty {
                    Type::Unknown | Type::VOID | Type::Bool | Type::U32 => {}
                    _ => {
                        self.debug_type(*type_id, fmt);
                    }
                }
            }
            fmt.dedent();
        }

        for child in &scope.children {
            self.debug_scope(*child, fmt);
        }
        fmt.dedent();
    }

    pub fn debug_type_declaration(&self, type_id: TypeId, fmt: &mut IndentFormatter) {
        let ty = &self.context.symbols[type_id];
        let visibility = ty.visibility();
        fmt.write_indent();
        if visibility == Visibility::Public {
            write!(fmt, "pub ");
        };
        match &ty {
            Type::Struct(CheckedStructNode { name, .. }) => {
                let type_name = &self.context.ident(name);
                write!(fmt, "struct {} ", type_name);
            }
            Type::Enum(CheckedEnumNode { name, .. }) => {
                let type_name = &self.context.ident(name);
                write!(fmt, "enum {} ", type_name);
            }
            Type::Trait(CheckedTraitNode { name, .. }) => {
                let type_name = &self.context.ident(name);
                write!(fmt, "trait {} ", type_name);
            }
            Type::Function(CheckedFunctionNode { name, qualifier, .. }) => {
                if qualifier.is_extern {
                    write!(fmt, "extern ");
                }
                if qualifier.is_const {
                    write!(fmt, "const ");
                }
                let type_name = &self.context.ident(name);
                write!(fmt, "fn {} ", type_name);
            }
            Type::Const(node) => {
                write!(fmt, "{:?} ", self.context.symbols.get_constant_value(node.value));
            }
            Type::Array(_) => {
                write!(fmt, "Array ");
            }
            Type::TypeVariable(_) => {
                write!(fmt, "{} ", self.get_type_name(type_id));
            }
            _ => {
                write!(fmt, "{:?} ", ty.kind());
            }
        };
        write!(fmt, "({:?})", type_id);
        write!(fmt, "\n");
    }

    pub fn debug_type(&self, type_id: TypeId, fmt: &mut IndentFormatter) {
        self.debug_type_declaration(type_id, fmt);
        match &self.context.symbols[type_id] {
            Type::Array(_) | Type::Const(_) => return,
            _ => {}
        }
        fmt.indent();
        match &self.context.symbols[type_id] {
            Type::Unknown | Type::VOID => {}
            Type::U32 => {
                self.fmt_type_names("Implementations", &[], fmt);
            }
            Type::Felt => {
                self.fmt_type_names("Implementations", &[], fmt);
            }
            Type::Bool => {
                self.fmt_type_names("Implementations", &[], fmt);
            }
            Type::Struct(node) => {
                if !node.fields.is_empty() {
                    writeln!(fmt, "Fields:");
                    fmt.indent();
                    for (ident_id, field) in node.fields.iter() {
                        let ident_name = &self.context.ident(ident_id);
                        fmt.write_indent();
                        if field.visibility == Visibility::Public {
                            write!(fmt, "pub ");
                        };
                        let type_name = self.get_type_name(field.ty);
                        write!(fmt, "{} {} ({:?}, {:?})", type_name, ident_name, ident_id, type_id);
                        write!(fmt, "\n");
                    }
                    fmt.dedent();
                };
                if !node.generic_parameters.is_empty() {
                    writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                }
                self.fmt_type_names("Implementations", &[], fmt);
            }
            Type::Function(node) => {
                if !node.parameters.is_empty() {
                    writeln!(fmt, "Parameters:");
                    fmt.indent();
                    for parameter in node.parameters.iter() {
                        fmt.write_indent();
                        if parameter.qualifier.is_mutable {
                            write!(fmt, "mut ")
                        }
                        let type_name = self.get_type_name(parameter.ty);
                        let ident_name = &self.context.ident(parameter.name);
                        write!(fmt, "{} {} ({:?}, {:?})", type_name, ident_name, parameter.name, type_id);
                        write!(fmt, "\n");
                    }
                    fmt.dedent();
                };
                writeln!(fmt, "Return Type: {} ({:?})", self.get_type_name(node.return_type), node.return_type);
                if let Some(body) = node.body {
                    writeln!(fmt, "Body: {:?}", body);
                }
                if !node.generic_parameters.is_empty() {
                    writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                }
                if !node.attrs.is_empty() {
                    writeln!(fmt, "Attrs: {:?}", node.attrs);
                }
            }
            Type::Trait(node) => {
                if !node.body.is_empty() {
                    writeln!(fmt, "Body: {:?}", node.body);
                }
                if !node.unchecked_body.is_empty() {
                    writeln!(fmt, "Unchecked Body: {:?}", node.unchecked_body);
                }
                if !node.generic_parameters.is_empty() {
                    writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                }
                self.fmt_type_names("Implementor", &[], fmt);
            }
            Type::Array(node) => {
                writeln!(fmt, "[{}; {}]", self.get_type_name(node.inner_ty), self.get_type_name(node.size_ty),);
                self.fmt_type_names("Implementations", &[], fmt);
            }
            Type::TypeVariable(_) => {}
            x => writeln!(fmt, "Remaining {:?}", x),
        }
        fmt.dedent();
    }

    pub fn debug_expr(&self, expr_id: ExprId, fmt: &mut IndentFormatter) {
        let node = self.context.expression(expr_id);
        match node {
            ExprNode::Path(node) => {
                writeln!(fmt, "Path");
                fmt.indent();
                writeln!(fmt, "Root: {:?}", node.root);
                let segments = node.segments.iter().map(|segment| format!("{:?}", segment)).join(", ");
                writeln!(fmt, "Segments: [{}]", segments);
                writeln!(fmt, "Target: {:?}", format!("{:?}", node.target));
                fmt.indent();
            }
            ExprNode::Value(node) => writeln!(fmt, "{:?}", node),
            ExprNode::Binary(node) => {
                writeln!(fmt, "Binary: {:?} {} {:?}", node.lhs, node.operator, node.rhs)
            }
            ExprNode::Unary(node) => {
                writeln!(fmt, "Unary: {}{:?}", node.operator, node.rhs);
            }
            ExprNode::Call(node) => {
                writeln!(fmt, "Call");
                fmt.indent();
                writeln!(fmt, "Callee: {:?}", node.callee);
                writeln!(fmt, "Args: {:?}", node.args);
                writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                fmt.dedent();
            }
            ExprNode::MemberCall(node) => {
                writeln!(fmt, "Member Call");
                fmt.indent();
                writeln!(fmt, "Callee: {:?}", node.callee);
                writeln!(fmt, "Receiver: {:?}", node.receiver);
                writeln!(fmt, "Args: {:?}", node.args);
                writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                fmt.dedent();
            }
            ExprNode::Cast(node) => {
                writeln!(fmt, "Cast");
                fmt.indent();
                writeln!(fmt, "Value: {:?}", node.value);
                writeln!(fmt, "Target Type: {:?}", node.target_type);
                fmt.dedent();
            }
            ExprNode::IndexAccess(node) => {
                writeln!(fmt, "Index Access");
                fmt.indent();
                writeln!(fmt, "Target: {:?}", node.target);
                writeln!(fmt, "Index: {:?}", node.index);
                fmt.dedent()
            }
            ExprNode::MemberAccess(node) => {
                writeln!(fmt, "Member Access");
                fmt.indent();
                writeln!(fmt, "Target: {:?}", node.target);
                fmt.dedent()
            }
            ExprNode::BlockExpr(node) => {
                writeln!(fmt, "Block Expr");
                fmt.indent();
                writeln!(fmt, "Statements: {:?}", node.stmts);
                if !node.stmts.is_empty() {
                    writeln!(fmt, "Statements:");
                    fmt.indent();
                    for stmt_id in node.stmts.iter() {
                        let stmt = self.context.statement(*stmt_id);
                        writeln!(fmt, "{:?}", stmt);
                    }
                    fmt.dedent();
                }
                writeln!(fmt, "Expr: {:?}", node.expr);
                if !node.expr_comments.is_empty() {
                    writeln!(fmt, "Comments:");
                    fmt.indent();
                    for comment in node.expr_comments.iter() {
                        writeln!(fmt, "{:?}", comment);
                    }
                    fmt.dedent();
                }
                fmt.dedent();
            }
            ExprNode::IfExpr(node) => {
                writeln!(fmt, "If Expr");
                fmt.indent();
                writeln!(fmt, "If Branch: {:?}", node.if_branch);
                writeln!(fmt, "Else Branch: {:?}", node.else_branch);
                if !node.elseif_branches.is_empty() {
                    writeln!(fmt, "Else If Branches:");
                    fmt.indent();
                    for case in node.elseif_branches.iter() {
                        writeln!(fmt, "{:?}", case);
                    }
                    fmt.dedent();
                }
                fmt.dedent();
            }
            ExprNode::Intrinsic(node) => writeln!(fmt, "Intrinsic Expr: {:?}", node),
            ExprNode::LambdaFunction(node) => {
                writeln!(fmt, "Lambda Function");
                fmt.indent();
                if !node.parameters.is_empty() {
                    writeln!(fmt, "Parameters:");
                    fmt.indent();
                    for parameter in node.parameters.iter() {
                        writeln!(fmt, "{:?}", parameter);
                    }
                    fmt.dedent();
                };
                writeln!(fmt, "Body: {:?}", node.body);
                writeln!(fmt, "Return Type: {:?}", node.return_type);
                fmt.dedent();
            }
            ExprNode::Tuple(node) => {
                let elements = node
                    .elements
                    .iter()
                    .map(|expr_id| match self.context.expression(*expr_id) {
                        ExprNode::Value(node) => format!("{:?}", node),
                        _ => format!("{:?}", expr_id),
                    })
                    .join(", ");
                writeln!(fmt, "Tuple: ({})", elements);
            }
            ExprNode::TupleAccess(node) => {
                writeln!(fmt, "Tuple Access");
                fmt.indent();
                writeln!(fmt, "Target: {:?}", node.target);
                writeln!(fmt, "Index: {:?}", node.index);
                fmt.dedent();
            }
            ExprNode::Match(node) => {
                writeln!(fmt, "Match");
                fmt.indent();
                writeln!(fmt, "Scrutinee: {:?}", node.scrutinee);
                if !node.arms.is_empty() {
                    writeln!(fmt, "Arms:");
                    fmt.indent();
                    for arm in node.arms.iter() {
                        writeln!(fmt, "{:?}", arm);
                    }
                    fmt.dedent();
                }
                fmt.dedent();
            }
            ExprNode::Parentheses(node) => {
                writeln!(fmt, "Parentheses: {:?}", node);
            }
        }
    }

    pub fn debug_stmt(&self, statement: StmtId, fmt: &mut IndentFormatter) {
        let node = self.context.statement(statement);
        match node {
            StmtNode::While(node) => {
                writeln!(fmt, "While");
                fmt.indent();
                writeln!(fmt, "Body: {:?}", node.body);
                writeln!(fmt, "Predicate: {:?}", node.predicate);
                self.write_comments(&node.comments, fmt);
                fmt.dedent();
            }
            StmtNode::For(node) => {
                writeln!(fmt, "For");
                fmt.indent();
                writeln!(fmt, "Body: {:?}", node.body);
                writeln!(fmt, "Variable: {:?}", node.variable);
                writeln!(fmt, "Range: {:?} .. {:?}", node.start, node.end);
                self.write_comments(&node.comments, fmt);
                fmt.dedent();
            }
            StmtNode::Assignment(node) => {
                writeln!(fmt, "Assignment");
                fmt.indent();
                writeln!(fmt, "Target: {:?}", node.target);
                writeln!(fmt, "Operator: {:?}", node.operator);
                writeln!(fmt, "Value: {:?}", node.value);
                self.write_comments(&node.comments, fmt);
                fmt.dedent();
            }
            StmtNode::Variable(node) => {
                writeln!(fmt, "Variable");
                fmt.indent();
                writeln!(fmt, "Name: {:?}", node.name);
                writeln!(fmt, "Type: {:?}", node.ty);
                writeln!(fmt, "Qualifier: {:?}", node.qualifier);
                writeln!(fmt, "Value: {:?}", node.value);
                self.write_comments(&node.comments, fmt);
                fmt.dedent();
            }
            StmtNode::Definition(def_id) => {}
            StmtNode::Expression(expr_id) => self.debug_expr(*expr_id, fmt),
            StmtNode::Return(node) => {
                writeln!(fmt, "Return");
                fmt.indent();
                if let Some(expr_id) = node.expr_id {
                    self.debug_expr(expr_id, fmt);
                }
                self.write_comments(&node.comments, fmt);
                fmt.dedent();
            }
            StmtNode::Intrinsic(node) => {
                writeln!(fmt, "Intrinsic: {:?}", node)
            }
        }
    }

    pub fn debug_definition(&self, def_id: DefId, fmt: &mut IndentFormatter) {
        let node = self.context.definition(def_id);
        match node {
            DefinitionNode::Function(node) => {
                writeln!(fmt, "Function");
                fmt.indent();
                writeln!(fmt, "Name: {:?}", node.name);
                writeln!(fmt, "Visibility: {:?}", node.visibility);
                writeln!(fmt, "Parameters: {:?}", node.parameters);
                writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                writeln!(fmt, "Body: {:?}", node.body);
                writeln!(fmt, "Return Type: {:?}", node.return_type);
                writeln!(fmt, "Qualifier: {:?}", node.qualifier);
                self.write_comments(&node.comments, fmt);
                fmt.dedent();
            }
            DefinitionNode::Struct(node) => {
                writeln!(fmt, "Struct");
                fmt.indent();
                writeln!(fmt, "Name: {:?}", node.name);
                writeln!(fmt, "Visibility: {:?}", node.visibility);
                writeln!(fmt, "Fields");
                fmt.indent();
                for (ident_id, field) in node.fields.iter() {
                    fmt.write_indent();
                    if field.visibility == Visibility::Public {
                        write!(fmt, "pub ");
                    };
                    let ident_name = &self.context.ident(ident_id);
                    write!(fmt, "{}", ident_name);
                    write!(fmt, "\n");
                }
                fmt.dedent();
                writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                writeln!(fmt, "Attrs: {:?}", node.attrs);
                self.write_comments(&node.comments, fmt);
                fmt.dedent();
            }
            DefinitionNode::Enum(node) => {
                writeln!(fmt, "Enum");
                fmt.indent();
                writeln!(fmt, "Name: {:?}", node.name);
                writeln!(fmt, "Visibility: {:?}", node.visibility);
                writeln!(fmt, "Variants");
                fmt.indent();
                for variant in node.variants.iter() {
                    match variant {
                        EnumVariant::Basic(ident) => {
                            writeln!(fmt, "Basic: {:?}", ident);
                        }
                        EnumVariant::Tuple(ident, types) => {
                            writeln!(fmt, "Tuple: {:?} {:?}", ident, types);
                        }
                        EnumVariant::Struct(ident, fields) => {
                            writeln!(fmt, "Struct: {:?}", ident);
                            fmt.indent();
                            for (field_name, field_value) in fields.iter() {
                                writeln!(fmt, "{:?}: {:?}", field_name, field_value);
                            }
                            fmt.dedent();
                        }
                    }
                }
                fmt.dedent();
                self.write_comments(&node.comments, fmt);
                fmt.dedent();
            }

            DefinitionNode::Impl(node) => {
                writeln!(fmt, "Impl");
                fmt.indent();
                writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                writeln!(fmt, "Associated Types");
                fmt.indent();
                for (ident_id, associated_type) in node.associated_types.iter() {
                    fmt.write_indent();
                    if associated_type.visibility == Visibility::Public {
                        write!(fmt, "pub ");
                    };
                    let ident_name = &self.context.ident(ident_id);
                    write!(fmt, "{}", ident_name);
                    writeln!(fmt, ": {:?}", associated_type.ty);
                }
                fmt.dedent();
                writeln!(fmt, "Type: {:?}", node.ty);
                writeln!(fmt, "Body: {:?}", node.body);
                writeln!(fmt, "Comments: {:?}", node.comments);
                fmt.dedent();
            }
            DefinitionNode::TraitImpl(node) => {
                // pub struct TraitImplNode {
                //     pub generic_parameters: Vec<GenericParameter>,
                //     pub associated_types: IndexMap<Identifier, AssociatedTypeValue>,
                //     pub trait_ty: UncheckedType,
                //     pub ty: UncheckedType,
                //     pub body: Vec<DefId>,
                //     pub comments: Vec<Comment>,
                //     pub location: Location,
                //     pub is_generated: bool,
                // }
                writeln!(fmt, "Trait Impl");
                fmt.indent();
                writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                writeln!(fmt, "Associated Types");
                fmt.indent();
                for (ident_id, associated_type) in node.associated_types.iter() {
                    fmt.write_indent();
                    if associated_type.visibility == Visibility::Public {
                        write!(fmt, "pub ");
                    };
                    let ident_name = &self.context.ident(ident_id);
                    write!(fmt, "{}", ident_name);
                    writeln!(fmt, ": {:?}", associated_type.ty);
                }
                fmt.dedent();
                writeln!(fmt, "Type: {:?}", node.ty);
                writeln!(fmt, "Body: {:?}", node.body);
                writeln!(fmt, "Comments: {:?}", node.comments);
                fmt.dedent();
            }
            DefinitionNode::Trait(node) => {
                writeln!(fmt, "Trait");
                writeln!(fmt, "Name: {:?}", node.name);
                writeln!(fmt, "Visibility: {:?}", node.visibility);
                writeln!(fmt, "Associated Types");
                fmt.indent();
                for (ident_id, associated_type) in node.associated_types.iter() {
                    fmt.write_indent();
                    if associated_type.visibility == Visibility::Public {
                        write!(fmt, "pub ");
                    };
                    let ident_name = &self.context.ident(ident_id);
                    write!(fmt, "{}", ident_name);
                    write!(fmt, "\n");
                }
                fmt.dedent();
                writeln!(fmt, "Generic Parameters: {:?}", node.generic_parameters);
                writeln!(fmt, "Body: {:?}", node.body);
                fmt.dedent();
            }
            DefinitionNode::TypeAlias(node) => {
                writeln!(fmt, "Type Alias");
                fmt.indent();
                writeln!(fmt, "Name: {:?}", node.name);
                writeln!(fmt, "Visibility: {:?}", node.visibility);
                writeln!(fmt, "Type: {:?}", node.ty);
                fmt.dedent();
            }
            DefinitionNode::Const(node) => {
                writeln!(fmt, "Const");
                fmt.indent();
                writeln!(fmt, "Name: {:?}", node.name);
                writeln!(fmt, "Visibility: {:?}", node.visibility);
                writeln!(fmt, "Type: {:?}", node.ty);
                writeln!(fmt, "Value: {:?}", node.value);
                fmt.indent()
            }
            DefinitionNode::Use(node) => {
                writeln!(fmt, "Use");
                fmt.indent();
                writeln!(fmt, "Visibility: {:?}", node.visibility);
                writeln!(fmt, "Kind: {:?}", node.kind);
                let segments = node.segments.iter().map(|identifier| self.indent_name(identifier.id)).join(", ");
                writeln!(fmt, "Segments: [{}]", segments);
                if let Some(target) = node.target {
                    writeln!(fmt, "Target: {:?}", self.indent_name(target.id));
                }
                fmt.indent();
            }
        }
    }

    fn write_comments(&self, comments: &[Comment], fmt: &mut IndentFormatter) {
        if !comments.is_empty() {
            writeln!(fmt, "Comments:");
            fmt.indent();
            for comment in comments.iter() {
                writeln!(fmt, "{}", comment);
            }
            fmt.dedent();
        }
    }

    pub fn get_type_name(&self, type_id: TypeId) -> String {
        let ty = &self.context.symbols[type_id];
        let ty_indent_id = match &ty {
            Type::Unknown => IdentId::TYPE_UNKNOWN,
            Type::Struct(CheckedStructNode { name, .. }) => name.id,
            Type::Enum(CheckedEnumNode { name, .. }) => name.id,
            Type::Function(CheckedFunctionNode { name, .. }) => name.id,
            Type::Trait(CheckedTraitNode { name, .. }) => name.id,
            Type::Const(node) => match node.name {
                Some(name) => name.id,
                None => return self.get_type_name(node.ty),
            },
            Type::Array(_) => IdentId::TYPE_ARRAY,
            Type::VOID => IdentId::TYPE_VOID,
            Type::Felt => IdentId::TYPE_FELT,
            Type::Bool => IdentId::TYPE_BOOL,
            Type::U32 => IdentId::TYPE_U32,
            Type::Tuple(_) => IdentId::TYPE_TUPLE,
            Type::TypeVariable(tvar) => tvar.name,
            Type::LambdaFunction(_) => {
                return "LambdaFunction".to_string();
            }
            Type::FunctionSignature(_) => {
                return "FunctionSignature".to_string();
            }
        };
        let type_name = &self.context.ident(ty_indent_id);
        type_name.to_string()
    }

    pub fn fmt_type_names(&self, attr: &str, values: &[TypeId], fmt: &mut IndentFormatter) {
        if !values.is_empty() {
            let implementations = values.iter().map(|ty| self.get_type_name(*ty)).collect::<Vec<_>>().join(", ");
            writeln!(fmt, "{}: {}", attr, implementations);
        }
    }

    pub fn debug_variable_inline(&self, ident_id: IdentId, var_id: VarId, fmt: &mut IndentFormatter) {
        let var_name = &self.context.ident(ident_id).0;
        let checked_var = &self.context.symbols[var_id];
        fmt.write_indent();
        if checked_var.qualifier.is_mutable {
            write!(fmt, "mut ");
        };
        let type_name = self.get_type_name(checked_var.ty);
        write!(fmt, "{}: {}", var_name, type_name);
        write!(fmt, "\n")
    }

    pub fn indent_name(&self, ident_id: IdentId) -> String {
        self.context.ident(ident_id).to_string()
    }
}

impl<F: Clone + From<u32> + ContextFelt, C> AstVisualizer<F, C> for TypeCheckerVisitorContext<F, C> {
    type DebugResult = String;
    fn debug_scope(&self, scope_id: ScopeId) -> Self::DebugResult {
        let visualizer = TypeCheckerVisitorVisualizerInner { context: &self };
        let mut fmt = IndentFormatter::new();
        visualizer.debug_scope(scope_id, &mut fmt);
        fmt.finish_without_new_line()
    }

    fn debug_type(&self, type_id: TypeId) -> Self::DebugResult {
        let visualizer = TypeCheckerVisitorVisualizerInner { context: &self };
        let mut fmt = IndentFormatter::new();
        visualizer.debug_type(type_id, &mut fmt);
        fmt.finish_without_new_line()
    }

    fn debug_variable(&self, ident_id: IdentId, var_id: VarId) -> Self::DebugResult {
        let visualizer = TypeCheckerVisitorVisualizerInner { context: &self };
        let mut fmt = IndentFormatter::new();
        visualizer.debug_variable_inline(ident_id, var_id, &mut fmt);
        fmt.finish_without_new_line()
    }

    fn debug_expr(&self, expr_id: ExprId) -> Self::DebugResult {
        let visualizer = TypeCheckerVisitorVisualizerInner { context: &self };
        let mut fmt = IndentFormatter::new();
        visualizer.debug_expr(expr_id, &mut fmt);
        fmt.finish_without_new_line()
    }

    fn debug_stmt(&self, statement: StmtId) -> Self::DebugResult {
        let visualizer = TypeCheckerVisitorVisualizerInner { context: &self };
        let mut fmt = IndentFormatter::new();
        visualizer.debug_stmt(statement, &mut fmt);
        fmt.finish_without_new_line()
    }

    fn debug_definition(&self, def_id: DefId) -> Self::DebugResult {
        let visualizer = TypeCheckerVisitorVisualizerInner { context: &self };
        let mut fmt = IndentFormatter::new();
        visualizer.debug_definition(def_id, &mut fmt);
        fmt.finish_without_new_line()
    }
}
