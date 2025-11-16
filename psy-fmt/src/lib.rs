use std::fmt::Debug;

use psy_ast::{BlockExprNode, IfExprNode, *};
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};

#[derive(Debug)]
pub struct Formatter<'a, F: Clone + From<u32>, C> {
    output: String,
    indent: usize,
    _marker: std::marker::PhantomData<(&'a (), F, C)>,
}

impl<'a, F: Clone + From<u32> + Debug, C> Formatter<'a, F, C> {
    pub fn new() -> Self {
        Formatter {
            output: String::new(),
            indent: 0,
            _marker: std::marker::PhantomData,
        }
    }

    fn indent(&mut self) {
        self.indent += 1;
    }

    fn dedent(&mut self) {
        if self.indent > 0 {
            self.indent -= 1;
        }
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    fn read_indent(&self, extra: usize) -> String {
        let mut result = String::new();
        for _ in 0..self.indent + extra {
            result.push_str("    ");
        }
        result
    }

    #[allow(dead_code)]
    fn append_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    #[allow(dead_code)]
    fn write(&mut self, s: &str) {
        self.write_indent();
        self.output.push_str(s);
    }

    fn write_line(&mut self, s: &str) {
        self.write_indent();
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn visit_unchecked_type(&self, node: &UncheckedType, is_generic: bool, ctx: &impl VisitorContext<F, C>) -> String {
        match node {
            UncheckedType::Basic(name) => format!("{}", ctx.ident(name)),
            UncheckedType::Path(name) => self.visit_path_node(name, ctx),
            UncheckedType::Const(value, _) => value.to_string(),
            UncheckedType::Generic(name, generic_parameters, _) => {
                if name == &IdentId::TYPE_ARRAY {
                    assert!(generic_parameters.len() == 2);
                    if is_generic {
                        return format!(
                            "<[{}; {}]>",
                            self.visit_unchecked_type(&generic_parameters[0], is_generic, ctx),
                            self.visit_unchecked_type(&generic_parameters[1], is_generic, ctx)
                        );
                    }
                    return format!(
                        "[{}; {}]",
                        self.visit_unchecked_type(&generic_parameters[0], is_generic, ctx),
                        self.visit_unchecked_type(&generic_parameters[1], is_generic, ctx)
                    );
                }
                if is_generic {
                    return format!(
                        "<{}{}>",
                        &ctx.ident(name),
                        self.visit_unchecked_generic_parameters(generic_parameters, ctx)
                    );
                }
                format!("{}{}", &ctx.ident(name), self.visit_unchecked_generic_parameters(generic_parameters, ctx))
            }
            UncheckedType::Array(ty, size, _) => {
                if is_generic {
                    return format!("<[{}; {}]>", self.visit_unchecked_type(ty, is_generic, ctx), size);
                }
                format!("[{}; {}]", self.visit_unchecked_type(ty, is_generic, ctx), size)
            }
            UncheckedType::Tuple(tys, _) => format!(
                "({})",
                tys.iter()
                    .map(|ty| self.visit_unchecked_type(ty, is_generic, ctx))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            UncheckedType::Unknown => "unknown".to_string(),
            UncheckedType::FunctionSignature(sig, _) => {
                let parameters = sig
                    .parameters
                    .iter()
                    .map(|p| format!("{}", self.visit_unchecked_type(&p, is_generic, ctx)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({}){}", parameters, self.visit_return_type(&sig.return_type, ctx),)
            }
            UncheckedType::TraitCast(type_path, trait_path, _) => {
                format!(
                    "<{} as {}>",
                    self.visit_unchecked_type(type_path, false, ctx),
                    self.visit_unchecked_type(trait_path, false, ctx),
                )
            }
        }
    }

    fn visit_unchecked_generic_parameters(&self, generic_parameters: &[UncheckedType], ctx: &impl VisitorContext<F, C>) -> String {
        let generic_parameters = generic_parameters
            .iter()
            .map(|p| self.visit_unchecked_type(p, false, ctx))
            .collect::<Vec<_>>();
        if generic_parameters.is_empty() {
            "".to_string()
        } else {
            format!("<{}>", generic_parameters.join(", "))
        }
    }

    fn visit_generic_parameters(&self, generic_parameters: &[GenericParameter], ctx: &impl VisitorContext<F, C>) -> String {
        let generic_parameters = generic_parameters
            .iter()
            .map(|p| {
                if p.constraints.is_empty() {
                    ctx.ident(p.name.clone()).to_string()
                } else {
                    let constraints_content = p
                        .constraints
                        .iter()
                        .map(|constraint| self.visit_unchecked_type(&constraint, false, ctx))
                        .collect::<Vec<_>>()
                        .join(" + ");
                    format!("{}: {}", ctx.ident(p.name.clone()), constraints_content)
                }
            })
            .collect::<Vec<_>>();
        if generic_parameters.is_empty() {
            "".to_string()
        } else {
            format!("<{}>", generic_parameters.join(", "))
        }
    }

    fn visit_path_node(&self, node: &PathNode, ctx: &impl VisitorContext<F, C>) -> String {
        let mut path = node
            .root
            .as_ref()
            .map(|r| vec![self.visit_unchecked_type(r, true, ctx)])
            .unwrap_or_default();
        path.extend_from_slice(
            &node
                .segments
                .iter()
                .map(|s| self.visit_unchecked_type(&s, true, ctx))
                .collect::<Vec<String>>(),
        );
        path.extend_from_slice(&vec![self.visit_unchecked_type(&node.target, true, ctx)]);

        if path.len() > 1 {
            format!("<{}>", path.join("::"))
        } else {
            format!("{}", path.join("::"))
        }
    }

    fn visit_comments(&self, comments: &[Comment]) -> String {
        comments
            .iter()
            .map(|comment| format!("{}\n{}", comment.content(), self.read_indent(0)))
            .collect::<String>()
    }

    fn visit_visibility(&self, visibility: &Visibility) -> String {
        match visibility {
            Visibility::Public => "pub ".to_string(),
            Visibility::Private => "".to_string(),
        }
    }

    fn visit_qualifier(&self, qualifier: &Qualifier) -> String {
        format!(
            "{}{}",
            if qualifier.is_extern { "extern " } else { "" },
            if qualifier.is_const { "const " } else { "" }
        )
    }

    fn visit_type_qualifier(&self, qualifier: &TypeQualifier) -> String {
        format!("{}", if qualifier.is_mutable { "mut " } else { "" })
    }

    fn visit_return_type(&self, return_type: &Option<UncheckedType>, ctx: &impl VisitorContext<F, C>) -> String {
        if let Some(ret) = return_type {
            format!(" -> {}", self.visit_unchecked_type(&ret, false, ctx))
        } else {
            "".to_string()
        }
    }

    fn visit_attr(&self, attrs: &[AttrNode], ctx: &impl VisitorContext<F, C>) -> String {
        attrs
            .iter()
            .map(|attr| {
                if !attr.properties.is_empty() {
                    format!(
                        "#[{}({})]\n{}",
                        ctx.ident(attr.name),
                        attr.properties.iter().map(|p| ctx.ident(p).to_string()).collect::<Vec<_>>().join(", "),
                        self.read_indent(0)
                    )
                } else {
                    format!("#[{}]\n{}", ctx.ident(attr.name), self.read_indent(0))
                }
            })
            .collect::<String>()
    }

    pub fn get_output(&self) -> &str {
        &self.output
    }
}

impl<'a, F: ContextFelt + From<u32> + Debug + 'static, C: DPNContext<F>> AstVisitor<F, C> for Formatter<'a, F, C> {
    type ExprResult = String;
    type StmtResult = String;
    type Context = DefaultVisitorContext<'a, F, C>;
    type Error = psy_common::Error;
    type Expr = ExprNode<F>;
    type Stmt = StmtNode;
    type Definition = DefinitionNode;
    type DefinitionResult = String;

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
                let definition_result = self.visit_definition(def_id, ctx)?;
                Self::StmtResult::from(format!("{}\n", definition_result))
            }
            NodeType::ExpressionStmt => {
                let expr_id = ctx.statement(stmt_id).as_expression().unwrap().clone();
                format!("{};", Self::StmtResult::from(self.visit_expr(expr_id, ctx)?))
            }
            NodeType::IntrinsicStmt => self.visit_intrinsic_stmt(stmt_id, ctx)?,
            _ => unreachable!(),
        };
        ctx.pop_node_id();
        Ok(res)
    }

    fn visit_use(&mut self, def_id: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error> {
        let u = ctx.definition(def_id).as_use().cloned().unwrap();

        let mut path = vec![ctx.ident(u.kind).to_string()];
        let segments = u.segments.iter().map(|&s| ctx.ident(s).to_string()).collect::<Vec<_>>();
        path.extend(segments);
        let target = u.target.map(|t| ctx.ident(t).to_string()).unwrap_or("*".to_string());

        Ok(format!(
            "{}{}use {}::{};",
            self.visit_comments(&u.comments),
            self.visit_visibility(&u.visibility),
            path.join("::"),
            target
        ))
    }

    fn visit_path(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let node = ctx.expression(expr_id).as_path().unwrap();

        let mut path = node
            .root
            .as_ref()
            .map(|r| vec![self.visit_unchecked_type(r, true, ctx)])
            .unwrap_or_default();
        path.extend_from_slice(
            &node
                .segments
                .iter()
                .map(|s| self.visit_unchecked_type(&s, true, ctx))
                .collect::<Vec<String>>(),
        );
        path.extend_from_slice(&vec![self.visit_unchecked_type(&node.target, true, ctx)]);

        Ok(format!("{}", path.join("::")))
    }

    fn visit_index_access(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let &IndexAccessNode {
            target: value,
            index,
            location: ref _location,
        } = ctx.expression(expr_id).as_index_access().unwrap();
        Ok(format!("{}[{}]", self.visit_expr(value, ctx)?, self.visit_expr(index, ctx)?))
    }

    fn visit_member_access(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let &MemberAccessNode {
            target: value,
            field,
            location: ref _location,
        } = ctx.expression(expr_id).as_member_access().unwrap();
        Ok(format!("{}.{}", self.visit_expr(value, ctx)?, ctx.ident(field)))
    }

    fn visit_value(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let node = ctx.expression(expr_id).as_value().unwrap().clone();
        Ok(match node {
            ValueNode::Felt(f, _) => format!("{:?}", f),
            ValueNode::Bool(b, _) => format!("{:?}", b),
            ValueNode::U32(u, _) => format!("{:?}", u),
            ValueNode::Array(_, values, _) => format!(
                "[{}]",
                values
                    // TODO: remove clone
                    .clone()
                    .into_iter()
                    .map(|v| {
                        self.indent();
                        let res = self.visit_expr(v, ctx);
                        self.dedent();
                        res
                    })
                    .collect::<Result<Vec<_>, Self::Error>>()?
                    .join(", ")
            ),
            ValueNode::Struct(name, generic_parameters, field_values, _location) => {
                let name = self.visit_path(name, ctx)?;

                self.indent();
                let field_names = field_values
                    .iter()
                    .map(|(field_name, _)| ctx.ident(field_name).to_string())
                    .collect::<Vec<_>>();
                let field_values = field_values
                    .iter()
                    .map(|(_, field_expr)| -> Result<Self::ExprResult, Self::Error> { self.visit_expr(*field_expr, ctx) })
                    .collect::<Result<Vec<String>, Self::Error>>()?;
                let fiels_content = field_names
                    .iter()
                    .zip(field_values.iter())
                    .map(|(field_name, field_value)| format!("{}{}: {},\n", self.read_indent(0), field_name, field_value))
                    .collect::<String>();
                self.dedent();
                let generic_parameters = self.visit_unchecked_generic_parameters(&generic_parameters, ctx);

                format!(
                    "new {}{} {{\n{}{}}}",
                    name,
                    if generic_parameters.is_empty() {
                        "".to_string()
                    } else {
                        format!("#{}", generic_parameters)
                    },
                    fiels_content,
                    self.read_indent(0),
                )
            }
        })
    }

    fn visit_binary(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let &BinaryNode {
            lhs,
            operator,
            rhs,
            location: ref _location,
        } = ctx.expression(expr_id).as_binary().unwrap();
        Ok(format!("{} {} {}", self.visit_expr(lhs, ctx)?, operator, self.visit_expr(rhs, ctx)?))
    }

    fn visit_unary(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let &UnaryNode {
            operator,
            rhs,
            location: ref _location,
        } = ctx.expression(expr_id).as_unary().unwrap();
        Ok(format!("{}{}", operator, self.visit_expr(rhs, ctx)?))
    }

    fn visit_call(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let &CallNode {
            callee: variable,
            ref args,
            ref generic_parameters,
            location: ref _location,
        } = ctx.expression(expr_id).as_call().unwrap();

        let generic_parameters_content = self.visit_unchecked_generic_parameters(generic_parameters, ctx);
        let args = args
            // TOOD: remove clone
            .clone()
            .iter()
            .map(|&arg| self.visit_expr(arg, ctx))
            .collect::<Result<Vec<_>, Self::Error>>()?
            .join(", ");
        Ok(format!(
            "{}{}({})",
            self.visit_expr(variable, ctx)?,
            if generic_parameters_content.is_empty() {
                "".to_string()
            } else {
                format!("#{}", generic_parameters_content)
            },
            args
        ))
    }

    fn visit_member_call(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let &MemberCallNode {
            callee: variable,
            ref args,
            ref generic_parameters,
            ..
        } = ctx.expression(expr_id).as_member_call().unwrap();
        let generic_parameters_content = self.visit_unchecked_generic_parameters(generic_parameters, ctx);
        let args = args
            // TOOD: remove clone
            .clone()
            .iter()
            .map(|&arg| self.visit_expr(arg, ctx))
            .collect::<Result<Vec<_>, Self::Error>>()?
            .join(", ");
        Ok(format!("{}{}({})", self.visit_expr(variable, ctx)?, generic_parameters_content, args))
    }

    fn visit_cast(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let &CastNode {
            value,
            ref target_type,
            location: ref _location,
        } = ctx.expression(expr_id).as_cast().unwrap();
        // TODO: remove clone
        let target_type = target_type.clone();
        Ok(format!(
            "{} as {}",
            self.visit_expr(value, ctx)?,
            self.visit_unchecked_type(&target_type, false, ctx)
        ))
    }

    fn visit_while(&mut self, stmt_id: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        let &WhileNode {
            predicate,
            body,
            ref comments,
            location: ref _location,
        } = ctx.statement(stmt_id).as_while().unwrap();
        let comments_content = self.visit_comments(comments);
        let s = format!("while {} ", self.visit_expr(predicate, ctx)?);
        let block = self.visit_block_expr(body, ctx)?;
        Ok(format!("{}{}{}", comments_content, s, block))
    }

    fn visit_assignment(&mut self, stmt_id: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        let &AssignmentNode {
            target,
            operator,
            value,
            ref comments,
            location: ref _location,
        } = ctx.statement(stmt_id).as_assignment().unwrap();
        let s = format!(
            "{}{} {} {};",
            self.visit_comments(comments),
            self.visit_expr(target, ctx)?,
            operator,
            self.visit_expr(value, ctx)?
        );
        Ok(s)
    }

    fn visit_variable(&mut self, stmt_id: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        let comments_content = self.visit_comments(&ctx.statement(stmt_id).as_variable().unwrap().comments);
        let var_type = self.visit_unchecked_type(&ctx.statement(stmt_id).as_variable().unwrap().ty.clone(), false, ctx);

        let type_content = if var_type == "unknown" {
            "".to_string()
        } else {
            format!(": {}", var_type)
        };

        let s = format!(
            "{}let {}{}{} = {};",
            comments_content,
            self.visit_type_qualifier(&ctx.statement(stmt_id).as_variable().unwrap().qualifier),
            // TODO: remove to_owned
            ctx.ident(ctx.statement(stmt_id).as_variable().unwrap().name.id).to_owned(),
            type_content,
            self.visit_expr(ctx.statement(stmt_id).as_variable().unwrap().value, ctx)?
        );
        Ok(s)
    }

    fn visit_return(&mut self, stmt_id: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        let ReturnNode {
            expr_id,
            comments,
            location: ref _location,
        } = ctx.statement(stmt_id).as_return().unwrap();

        Ok(format!(
            "{}return{};",
            self.visit_comments(&comments),
            if let Some(ret) = expr_id {
                format!(" {}", self.visit_expr(ret.clone(), ctx)?)
            } else {
                "".to_string()
            }
        ))
    }

    fn visit_impl(&mut self, def_id: DefId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        let ImplNode {
            associated_types,
            generic_parameters,
            ty,
            body,
            comments,
            location: _location,
            is_generated,
        } = ctx.definition(def_id).as_impl().unwrap();
        // skip #[derive(Storage)]
        if *is_generated {
            return Ok(Default::default());
        }
        let associated_types = associated_types
            .iter()
            .map(|(name, ty)| {
                format!(
                    "{}{} type {} = {};",
                    self.visit_comments(&ty.comments),
                    self.visit_visibility(&ty.visibility),
                    ctx.ident(name.clone()),
                    self.visit_unchecked_type(&ty.ty, false, ctx)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let comments_content = self.visit_comments(&comments);

        let generic_parameters = self.visit_generic_parameters(&generic_parameters, ctx);
        let struct_name = self.visit_unchecked_type(&ty, false, ctx);
        self.indent();
        // TODO: remove clone
        let mut funcs_content = String::new();
        for func in body.clone() {
            let func_content = self.visit_function(func, ctx)?;
            funcs_content.push_str(&format!("{}{}\n", self.read_indent(0), func_content));
        }
        self.dedent();

        Ok(format!(
            "{}impl{} {} {{\n{}{}{}}}",
            comments_content,
            generic_parameters,
            struct_name,
            associated_types,
            funcs_content,
            self.read_indent(0),
        ))
    }

    fn visit_function(&mut self, def_id: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error> {
        let FunctionNode {
            name,
            parameters,
            generic_parameters,
            body,
            return_type,
            qualifier,
            visibility,
            comments,
            attrs,
            location: _location,
        } = ctx.definition(def_id).as_function().unwrap();
        let comments_content = self.visit_comments(&comments);
        let attrs_content = self.visit_attr(&attrs, ctx);
        let parameters = parameters
            .iter()
            .map(|p| {
                format!(
                    "{}{}: {}",
                    self.visit_type_qualifier(&p.qualifier),
                    &ctx.ident(p.name),
                    self.visit_unchecked_type(&p.ty, false, ctx)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let s = format!(
            "{}{}fn {}{}({}){} ",
            self.visit_visibility(&visibility),
            self.visit_qualifier(&qualifier),
            ctx.ident(name),
            self.visit_generic_parameters(&generic_parameters, ctx),
            parameters,
            self.visit_return_type(&return_type, ctx),
        );
        let block = match body {
            Some(body) => self.visit_block_expr(body.clone(), ctx)?,
            None => "{ }".to_string(),
        };
        Ok(format!("{}{}{}{}", comments_content, attrs_content, s, block))
    }

    fn visit_struct(&mut self, def_id: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error> {
        let StructNode {
            name,
            fields,
            generic_parameters,
            attrs,
            visibility,
            comments,
            location: _location,
            is_generated,
        } = ctx.definition(def_id).as_struct().unwrap();
        if *is_generated {
            return Ok(Default::default());
        }
        let comments_content = self.visit_comments(&comments);

        let attrs_content = self.visit_attr(&attrs, ctx);
        self.indent();
        let fiels_content = fields
            .iter()
            .map(|(field_name, field)| {
                format!(
                    "{}{}{}{}{}: {},\n",
                    self.read_indent(0),
                    self.visit_comments(&field.comments),
                    self.visit_attr(&field.attrs, ctx),
                    self.visit_visibility(&field.visibility),
                    ctx.ident(field_name),
                    self.visit_unchecked_type(&field.ty, false, ctx)
                )
            })
            .collect::<String>();
        self.dedent();

        let generic_parameter_content = self.visit_generic_parameters(&generic_parameters, ctx);
        Ok(format!(
            "{}{}{}struct {}{} {{\n{}{}}}",
            comments_content,
            attrs_content,
            self.visit_visibility(&visibility),
            &ctx.ident(name),
            generic_parameter_content,
            fiels_content,
            self.read_indent(0),
        ))
    }

    fn visit_enum(&mut self, def_id: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error> {
        let EnumNode {
            name,
            generic_parameters,
            variants,
            visibility,
            comments,
            location: _location,
        } = ctx.definition(def_id).as_enum().unwrap();
        let comments_content = self.visit_comments(&comments);

        self.indent();

        let variants_content = variants
            .iter()
            .map(|variant| match variant {
                EnumVariant::Basic(ident_id) => {
                    format!("{}{},\n", self.read_indent(0), ctx.ident(ident_id))
                }
                EnumVariant::Tuple(ident_id, types) => {
                    let types = types
                        .iter()
                        .map(|x| self.visit_unchecked_type(x, false, ctx))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}{}({}),\n", self.read_indent(0), ctx.ident(ident_id), types)
                }
                EnumVariant::Struct(ident_id, fields) => {
                    self.indent();
                    let fiels_content = fields
                        .iter()
                        .map(|(field_name, field)| {
                            format!(
                                "{}{}{}{}: {},\n",
                                self.read_indent(0),
                                self.visit_comments(&field.comments),
                                self.visit_visibility(&field.visibility),
                                ctx.ident(field_name),
                                self.visit_unchecked_type(&field.ty, false, ctx)
                            )
                        })
                        .collect::<String>();
                    self.dedent();

                    format!(
                        "{}{} {{\n{}{}}},\n",
                        self.read_indent(0),
                        ctx.ident(ident_id),
                        fiels_content,
                        self.read_indent(0),
                    )
                }
            })
            .collect::<String>();
        self.dedent();

        Ok(format!(
            "{}{}enum {}{} {{\n{}{}}}",
            comments_content,
            self.visit_visibility(&visibility),
            &ctx.ident(name),
            self.visit_generic_parameters(&generic_parameters, ctx),
            variants_content,
            self.read_indent(0),
        ))
    }

    fn visit_trait(&mut self, def_id: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error> {
        let TraitNode {
            associated_types,
            name,
            generic_parameters,
            body,
            visibility,
            comments,
            location: _location,
        } = ctx.definition(def_id).as_trait().cloned().unwrap();
        let associated_types = associated_types
            .iter()
            .map(|(name, ty)| {
                let comments_content = self.visit_comments(&ty.comments);
                if ty.constraints.is_empty() {
                    format!(
                        "{}{}{}type {};\n",
                        self.read_indent(1),
                        comments_content,
                        self.visit_visibility(&ty.visibility),
                        ctx.ident(name.clone()),
                    )
                } else {
                    let constraints_content = ty
                        .constraints
                        .iter()
                        .map(|constraint| self.visit_unchecked_type(&constraint, false, ctx))
                        .collect::<Vec<_>>()
                        .join(" + ");

                    format!(
                        "{}{}{}type {}: {};\n",
                        self.read_indent(1),
                        comments_content,
                        self.visit_visibility(&ty.visibility),
                        ctx.ident(name.clone()),
                        constraints_content
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("");
        let comments_content = self.visit_comments(&comments);

        let trait_name = ctx.ident(name).to_string();

        let generic_parameters = self.visit_generic_parameters(&generic_parameters, ctx);

        self.indent();

        let body_content = body
            .iter()
            .map(|func| -> Result<String, Self::Error> {
                let func = ctx.definition(func.clone()).as_function().cloned().unwrap();
                let func_comments_content = self.visit_comments(&func.comments);
                let func_name = ctx.ident(func.name).to_string();
                let parameters = func
                    .parameters
                    .iter()
                    .map(|p| {
                        format!(
                            "{}{}: {}",
                            self.visit_type_qualifier(&p.qualifier),
                            &ctx.ident(p.name),
                            self.visit_unchecked_type(&p.ty, false, ctx)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let func_body_content = match func.body {
                    Some(body) => format!(" {}", self.visit_block_expr(body.clone(), ctx)?),
                    None => ";".to_string(),
                };
                let func_content = format!(
                    "{}{}{}{}fn {}({}){}{}\n",
                    func_comments_content,
                    self.read_indent(0),
                    self.visit_visibility(&func.visibility),
                    self.visit_qualifier(&func.qualifier),
                    func_name,
                    parameters,
                    self.visit_return_type(&func.return_type, ctx),
                    func_body_content,
                );
                Ok(func_content)
            })
            .collect::<Result<String, Self::Error>>()?;
        self.dedent();

        Ok(format!(
            "{}{}trait {}{} {{\n{}{}{}}}",
            comments_content,
            self.visit_visibility(&visibility),
            trait_name,
            generic_parameters,
            if associated_types.is_empty() {
                "".to_string()
            } else {
                format!("{}\n", associated_types)
            },
            body_content,
            self.read_indent(0),
        ))
    }

    fn visit_intrinsic_expr(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let node = ctx.expression(node).as_intrinsic().cloned().unwrap();
        match node {
            IntrinsicExprNode::GetUserId { .. } => Ok("__ctx_get_user_id()".to_string()),
            IntrinsicExprNode::GetContractId { .. } => Ok("__ctx_get_contract_id()".to_string()),
            IntrinsicExprNode::GetContractDeployer { contract_id, .. } => {
                Ok(format!("__ctx_get_contract_deployer({})", self.visit_expr(contract_id.clone(), ctx)?))
            }
            IntrinsicExprNode::GetCallerContractId { .. } => Ok("__ctx_get_caller_contract_id()".to_string()),
            IntrinsicExprNode::GetLastNonce { .. } => Ok("__ctx_get_last_nonce()".to_string()),
            IntrinsicExprNode::GetCheckpointId { .. } => Ok("__ctx_get_checkpoint_id()".to_string()),
            IntrinsicExprNode::GetUserPublicKeyHash { .. } => Ok("__ctx_get_user_public_key_hash()".to_string()),
            IntrinsicExprNode::GetStateHashAt { slot_index, .. } => {
                Ok(format!("__ctx_get_state_hash_at({})", self.visit_expr(slot_index.clone(), ctx)?))
            }
            IntrinsicExprNode::GetOtherContractStateHashAt {
                contract_state_tree_height,
                contract_id,
                slot_index,
                ..
            } => Ok(format!(
                "__ctx_get_other_contract_state_hash_at({}, {}, {})",
                self.visit_expr(contract_state_tree_height.clone(), ctx)?,
                self.visit_expr(contract_id.clone(), ctx)?,
                self.visit_expr(slot_index.clone(), ctx)?
            )),
            IntrinsicExprNode::GetOtherUserContractStateHashAt {
                contract_state_tree_height,
                user_id,
                contract_id,
                slot_index,
                ..
            } => Ok(format!(
                "__ctx_get_other_user_contract_state_hash_at({}, {}, {}, {})",
                self.visit_expr(contract_state_tree_height.clone(), ctx)?,
                self.visit_expr(user_id.clone(), ctx)?,
                self.visit_expr(contract_id.clone(), ctx)?,
                self.visit_expr(slot_index.clone(), ctx)?
            )),
            IntrinsicExprNode::CSetStateHashAt { slot_index, new_value, .. } => Ok(format!(
                "__ctx_cset_state_hash_at({}, {})",
                self.visit_expr(slot_index.clone(), ctx)?,
                self.visit_expr(new_value.clone(), ctx)?
            )),
            IntrinsicExprNode::MemTransmute { data, target_type, .. } => Ok(format!(
                "__mem_transmute#<{}>({})",
                self.visit_unchecked_type(&target_type, false, ctx),
                self.visit_expr(data, ctx)?,
            )),
            IntrinsicExprNode::MemSizeOf { query_type: ty, .. } => Ok(format!("__mem_size_of#<{}>", self.visit_unchecked_type(&ty, false, ctx))),
            IntrinsicExprNode::StorageRead {
                contract_state_tree_height,
                user_id,
                contract_id,
                offset,
                ..
            } => Ok(format!(
                "__storage_read({}, {}, {}, {})",
                self.visit_expr(contract_state_tree_height, ctx)?,
                self.visit_expr(user_id, ctx)?,
                self.visit_expr(contract_id, ctx)?,
                self.visit_expr(offset, ctx)?
            )),
            IntrinsicExprNode::StorageReadRange {
                contract_state_tree_height,
                user_id,
                contract_id,
                offset,
                length,
                ..
            } => Ok(format!(
                "__storage_read_range({}, {}, {}, {},{})",
                self.visit_expr(contract_state_tree_height, ctx)?,
                self.visit_expr(user_id, ctx)?,
                self.visit_expr(contract_id, ctx)?,
                self.visit_expr(offset, ctx)?,
                self.visit_expr(length, ctx)?
            )),
            IntrinsicExprNode::StorageWrite { offset, value, .. } => Ok(format!(
                "__storage_write({}, {})",
                self.visit_expr(offset, ctx)?,
                self.visit_expr(value, ctx)?
            )),
            IntrinsicExprNode::StorageWriteRange { offset, values, .. } => Ok(format!(
                "__storage_write_range({}, {})",
                self.visit_expr(offset, ctx)?,
                self.visit_expr(values, ctx)?
            )),
            IntrinsicExprNode::Hash { data, .. } => Ok(format!("hash({})", self.visit_expr(data, ctx)?,)),
            IntrinsicExprNode::HashTwoToOne { left, right, .. } => Ok(format!(
                "hash_two_to_one({}, {})",
                self.visit_expr(left, ctx)?,
                self.visit_expr(right, ctx)?
            )),
            IntrinsicExprNode::InvokeSync {
                contract_id,
                method_id,
                inputs,
                return_type,
                ..
            } => Ok(format!(
                "__invoke_sync#<{}>({}, {}, {})",
                self.visit_unchecked_type(&return_type, false, ctx),
                self.visit_expr(contract_id, ctx)?,
                self.visit_expr(method_id, ctx)?,
                self.visit_expr(inputs, ctx)?,
            )),
            IntrinsicExprNode::InvokeDeferred {
                contract_id,
                method_id,
                inputs,
                ..
            } => Ok(format!(
                "__invoke_deferred({}, {}, {})",
                self.visit_expr(contract_id, ctx)?,
                self.visit_expr(method_id, ctx)?,
                self.visit_expr(inputs, ctx)?,
            )),
            IntrinsicExprNode::Secp256k1Verify { pub_key, msg, sig, .. } => Ok(format!(
                "__secp256k1_verify({}, {}, {})",
                self.visit_expr(pub_key, ctx)?,
                self.visit_expr(msg, ctx)?,
                self.visit_expr(sig, ctx)?,
            )),
            IntrinsicExprNode::GetCheckpointStats { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_checkpoint_stats({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::GetRegisterUsersRoot { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_register_users_root({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::GetGutasRoot { checkpoint_id, .. } => Ok(format!("__ctx_get_gutas_root({})", self.visit_expr(checkpoint_id, ctx)?)),
            IntrinsicExprNode::GetDeployContractsRoot { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_deploy_contracts_root({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::GetFeesCollected { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_fees_collected({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::GetUserOpsProcessed { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_user_ops_processed({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::GetTotalTransactions { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_total_transactions({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::GetSlotsModified { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_slots_modified({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::GetRegisterUsersCompleted { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_register_users_completed({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::GetGutasCompleted { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_gutas_completed({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::GetDeployContractsCompleted { checkpoint_id, .. } => {
                Ok(format!("__ctx_get_deploy_contracts_completed({})", self.visit_expr(checkpoint_id, ctx)?))
            }
            IntrinsicExprNode::SumBits { bits, .. } => Ok(format!("__sum_bits({})", self.visit_expr(bits, ctx)?,)),
            IntrinsicExprNode::SplitBits { target, num_bits, .. } => Ok(format!("__split_bits({}, {})", self.visit_expr(target, ctx)?, num_bits,)),
        }
    }

    fn visit_intrinsic_stmt(&mut self, node: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        let node = ctx.statement(node).as_intrinsic().cloned().unwrap();
        let s = match node {
            IntrinsicStmtNode::Assert {
                left,
                message,
                comments,
                location: _location,
            } => {
                let expr = self.visit_expr(left, ctx)?;
                let comments_content = self.visit_comments(&comments);
                format!("{}assert({}, \"{}\");", comments_content, expr, message.unwrap_or_default())
            }
            IntrinsicStmtNode::AssertEq {
                left,
                right,
                message,
                comments,
                location: _location,
            } => {
                let left = self.visit_expr(left, ctx)?;
                let right = self.visit_expr(right, ctx)?;
                let comments_content = self.visit_comments(&comments);
                format!("{}assert_eq({}, {}, \"{}\");", comments_content, left, right, message.unwrap_or_default())
            }
            psy_ast::IntrinsicStmtNode::ClearEntireTree { comments, .. } => {
                let comments_content = self.visit_comments(&comments);
                format!("{}__ctx_clear_entire_tree();", comments_content,)
            }
        };
        Ok(s)
    }
    fn visit_module(&mut self, module_id: ModuleId, ctx: &mut Self::Context) -> Result<(), Self::Error> {
        ctx.push_node_id(NodeId::from(module_id));

        let module = ctx.module(module_id).clone();

        if module.is_std() {
            return Ok(());
        }

        self.write_line(&format!(
            "{}mod {} {{",
            self.visit_visibility(&module.visibility),
            &ctx.ident(module.name)
        ));
        self.indent();

        for &child_module in ctx.program().modules.nodes().clone()[module_id].children() {
            // let child_module = ctx.module(child_module).clone();
            self.visit_module(child_module, ctx)?;
        }

        // TODO: remove clone
        for &definition in &module.definitions {
            let definition_content = self.visit_definition(definition, ctx)?;
            self.write_line(&definition_content);
        }

        self.dedent();
        self.write_line(&format!("}}"));

        ctx.pop_node_id();
        Ok(())
    }

    fn visit_program(&mut self, ctx: &mut Self::Context) -> Result<(), Self::Error> {
        self.visit_module(ModuleId::root(), ctx)?;
        Ok(())
    }

    fn visit_type_alias(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error> {
        let node = ctx.definition(node).as_type_alias().cloned().unwrap();
        let comments_content = self.visit_comments(&node.comments);
        Ok(format!(
            "{}type {} = {};",
            comments_content,
            ctx.ident(node.name),
            self.visit_unchecked_type(&node.ty, false, ctx)
        ))
    }

    fn visit_const(&mut self, node: DefId, ctx: &mut Self::Context) -> Result<Self::DefinitionResult, Self::Error> {
        let node = ctx.definition(node).as_const().cloned().unwrap();
        let value = self.visit_expr(node.value, ctx)?;

        Ok(format!(
            "{}{}const {}:{} = {};",
            self.visit_comments(&node.comments),
            if node.visibility.is_public() { "pub " } else { "" },
            ctx.ident(node.name),
            self.visit_unchecked_type(&node.ty, false, ctx),
            value
        ))
    }

    fn visit_for(&mut self, node: StmtId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        let &ForNode {
            variable,
            start,
            end,
            body,
            ref comments,
            location: ref _location,
        } = ctx.statement(node).as_for().unwrap();
        let comments_content = self.visit_comments(comments);

        let s = format!(
            "for {} in {}..{} ",
            ctx.ident(variable).to_string(),
            self.visit_expr(start, ctx)?,
            self.visit_expr(end, ctx)?
        );
        let block = self.visit_block_expr(body, ctx)?;
        Ok(format!("{}{}{}", comments_content, s, block))
    }

    fn visit_match(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        let MatchNode {
            scrutinee,
            arms,
            location: _location,
        } = ctx.expression(node).as_match().cloned().unwrap();

        let mut m = format!("match {} {{ ", self.visit_expr(scrutinee.clone(), ctx)?);
        self.indent();
        let current_indent = self.read_indent(0);
        for arm in arms {
            let s = format!(
                " \n{}{} => ",
                current_indent,
                if arm.pattern.is_place_holder() {
                    "_".to_string()
                } else {
                    let (pattern_expr, _) = arm.pattern.as_value().unwrap();
                    self.visit_expr(pattern_expr.clone(), ctx)?
                }
            );
            let expr = self.visit_expr(arm.body, ctx)?;
            m.push_str(&format!("{}{},", s, expr));
        }
        self.dedent();
        m.push_str(&format!("\n{}}}", self.read_indent(0)));
        Ok(m)
    }

    fn visit_parentheses(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let inner_expr_id = ctx.expression(node).as_parentheses().unwrap().clone();
        Ok(format!("({})", self.visit_expr(inner_expr_id, ctx)?))
    }

    fn visit_lambda_function(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let LambdaFunctionNode {
            parameters,
            return_type,
            body,
            ..
        } = ctx.expression(node).as_lambda_function().cloned().unwrap();
        let parameters = parameters
            .iter()
            .map(|p| {
                format!(
                    "{}{}: {}",
                    self.visit_type_qualifier(&p.qualifier),
                    &ctx.ident(p.name),
                    self.visit_unchecked_type(&p.ty, false, ctx)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");

        let lambda_function_body_content = self.visit_expr(body, ctx)?;

        let result = format!(
            "|{}|{} {}",
            parameters,
            self.visit_return_type(&return_type, ctx),
            lambda_function_body_content,
        );

        Ok(result)
    }

    fn visit_trait_impl(&mut self, def_id: DefId, ctx: &mut Self::Context) -> Result<Self::StmtResult, Self::Error> {
        let TraitImplNode {
            associated_types,
            generic_parameters,
            trait_ty,
            ty,
            body,
            comments,
            location: _location,
            is_generated,
        } = ctx.definition(def_id).as_trait_impl().unwrap();
        // skip #[derive(Storage)]
        if *is_generated {
            return Ok(Default::default());
        }
        let associated_types = associated_types
            .iter()
            .map(|(name, ty)| {
                let comments_content = self.visit_comments(&ty.comments);

                format!(
                    "{}{}{}type {} = {};\n",
                    self.read_indent(1),
                    comments_content,
                    if ty.visibility.is_public() { "pub " } else { "" },
                    ctx.ident(name.clone()),
                    self.visit_unchecked_type(&ty.ty, false, ctx)
                )
            })
            .collect::<Vec<_>>()
            .join("");

        let generic_parameters = self.visit_generic_parameters(&generic_parameters, ctx);

        let comments_content = self.visit_comments(&comments);
        let trait_name = self.visit_unchecked_type(&trait_ty, false, ctx);
        let struct_name = self.visit_unchecked_type(&ty, false, ctx);

        self.indent();
        let mut funcs_content = String::new();
        for func in body.clone() {
            let func_content = self.visit_function(func, ctx)?;
            funcs_content.push_str(&format!("{}{}\n", self.read_indent(0), func_content));
        }
        self.dedent();

        Ok(format!(
            "{}impl{} {} for {} {{\n{}{}{}}}",
            comments_content,
            generic_parameters,
            trait_name,
            struct_name,
            associated_types,
            funcs_content,
            self.read_indent(0),
        ))
    }

    fn visit_block_expr(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let BlockExprNode {
            stmts,
            expr: return_expr,
            expr_comments,
            location: _location,
        } = ctx.expression(node).as_block_expr().unwrap().clone();

        let mut block_expr_result = String::new();
        block_expr_result.push_str(format!("{{\n").as_str());

        self.indent();
        let current_indent = self.read_indent(0);

        for stmt in stmts.iter() {
            let stmt_result = self.visit_stmt(*stmt, ctx)?;
            if !stmt_result.is_empty() {
                block_expr_result.push_str(&format!("{}{}\n", current_indent, stmt_result));
            }
        }

        expr_comments
            .iter()
            .for_each(|comment| block_expr_result.push_str(&format!("{}{}\n", current_indent, comment.content())));
        //add return expr
        match return_expr {
            Some(expr) => {
                let return_expr_result = self.visit_expr(expr, ctx)?;
                block_expr_result.push_str(&format!("{}{}\n", current_indent, return_expr_result));
            }
            None => {}
        }

        self.dedent();
        block_expr_result.push_str(&self.read_indent(0));
        block_expr_result.push_str("}");

        Ok(block_expr_result)
    }

    fn visit_if_expr(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let IfExprNode {
            if_branch,
            elseif_branches,
            else_branch,
            ..
        } = ctx.expression(node).as_if_expr().unwrap().clone();

        let current_indent = self.read_indent(0);
        let mut result = format!("if {} ", self.visit_expr(if_branch.predicate, ctx)?);
        result.push_str(&format!("{}", self.visit_expr(if_branch.body, ctx)?));
        result.push_str(&format!("{}\n", current_indent));

        for branch in elseif_branches.into_iter() {
            result.push_str(&format!("{}else if {} ", current_indent, self.visit_expr(branch.predicate, ctx)?));
            result.push_str(&format!("{}", self.visit_expr(branch.body, ctx)?));
            result.push_str(&format!("{}\n", current_indent));
        }

        if let Some(else_branch) = else_branch {
            result.push_str(&format!("{}else ", current_indent));
            result.push_str(&format!("{}", self.visit_expr(else_branch, ctx)?));
        }
        Ok(result)
    }

    fn visit_tuple(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        let tuple_node = ctx.expression(node).as_tuple().unwrap();
        let elements = tuple_node.elements.clone();

        let formatted_elements = elements
            .iter()
            .map(|&expr_id| self.visit_expr(expr_id, ctx))
            .collect::<Result<Vec<_>, Self::Error>>()?
            .join(", ");

        Ok(format!("({})", formatted_elements))
    }

    fn visit_tuple_access(&mut self, node: ExprId, ctx: &mut Self::Context) -> Result<Self::ExprResult, Self::Error> {
        // get tuple access node, avoid borrowing ctx twice in closure
        let tuple_access_node = ctx.expression(node).as_tuple_access().unwrap();
        let target_expr_id = tuple_access_node.target;

        let index = tuple_access_node.index;

        let tuple_expr = self.visit_expr(target_expr_id, ctx)?;

        Ok(format!("{}.{}", tuple_expr, index))
    }
}

impl<'a, F: ContextFelt + From<u32> + Debug + 'static, C: DPNContext<F>> Formatter<'a, F, C> {
    fn is_inline_module(ctx: &DefaultVisitorContext<'a, F, C>, module_id: ModuleId) -> bool {
        let all_inline_modules = ctx
            .program()
            .modules
            .iter()
            .flat_map(|m| m.data().inline_modules.iter())
            .map(|m| m.name)
            .collect::<Vec<_>>();
        let target_module = ctx.module(module_id).name;

        all_inline_modules.contains(&target_module)
    }

    pub fn format_module_helper(
        &mut self,
        module_id: ModuleId,
        is_first: bool,
        ctx: &mut DefaultVisitorContext<'a, F, C>,
    ) -> Result<(), psy_common::Error> {
        let module = ctx.module(module_id).clone();

        if module.is_std() {
            return Ok(());
        }

        if !is_first && !Self::is_inline_module(ctx, module_id) {
            self.write_line(&format!("{}mod {};", self.visit_visibility(&module.visibility), &ctx.ident(module.name)));
            return Ok(());
        }

        if Self::is_inline_module(ctx, module_id) {
            self.write_line(&format!(
                "{}mod {} {{",
                self.visit_visibility(&module.visibility),
                &ctx.ident(module.name)
            ));
            self.indent();
        }

        let child_modules = ctx.program().modules.nodes().clone()[module_id]
            .children()
            .iter()
            .filter(|&child_module_id| !ctx.module(*child_module_id).is_std())
            .cloned()
            .collect::<Vec<_>>();

        let module_definitions = module
            .definitions
            .iter()
            .filter(|&def_id| !ctx.definition(*def_id).is_use() || ctx.definition(*def_id).as_use().unwrap().kind.id != IdentId::STD)
            .cloned()
            .collect::<Vec<_>>();

        if let Some((last_child_module, rest_child_modules)) = child_modules.split_last() {
            for child_module in rest_child_modules {
                self.format_module_helper(*child_module, false, ctx)?;
                self.write_line("");
            }

            self.format_module_helper(*last_child_module, false, ctx)?;
            if module_definitions.len() > 0 {
                self.write_line("");
            }
        }

        if let Some((last_definition, rest_definitions)) = module_definitions.split_last() {
            for &definition in rest_definitions {
                let definition_content = self.visit_definition(definition, ctx)?;
                if !definition_content.is_empty() {
                    self.write_line(&definition_content);
                    self.write_line("");
                }
            }
            let last_definition_content = self.visit_definition(*last_definition, ctx).unwrap();
            self.write_line(&last_definition_content);
        }

        if Self::is_inline_module(ctx, module_id) {
            self.dedent();
            self.write_line(&format!("}}"));
        }

        Ok(())
    }
}
