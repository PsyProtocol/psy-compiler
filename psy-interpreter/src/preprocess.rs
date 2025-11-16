use std::marker::PhantomData;

use indexmap::IndexMap;
use psy_ast::*;

#[derive(Debug)]
pub struct StorageProcessor<'a> {
    _marker: PhantomData<&'a ()>,
}

impl<'a> StorageProcessor<'a> {
    pub fn new() -> Self {
        Self { _marker: PhantomData }
    }

    fn generate_storage_impl<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> TraitImplNode {
        let mut methods = Vec::new();

        methods.push(self.generate_storage_size_method(struct_node, attr, ctx));
        methods.push(self.generate_storage_read_method(struct_node, attr, ctx));
        methods.push(self.generate_storage_write_method(struct_node, attr, ctx));

        TraitImplNode {
            associated_types: IndexMap::new(),
            generic_parameters: vec![],
            trait_ty: UncheckedType::Basic(Identifier::new(ctx.intern("Storage"), attr.location)),
            ty: UncheckedType::Basic(struct_node.name),
            body: methods,
            comments: vec![],
            location: attr.location,
            is_generated: true,
        }
    }

    fn generate_storage_at_impl<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        field_type: &UncheckedType,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> Option<TraitImplNode> {
        if let UncheckedType::Array(elem_ty, size, _loc) = field_type {
            let mut methods = Vec::new();
            methods.push(self.generate_storage_read_at_method(field_type, attr, ctx));
            methods.push(self.generate_storage_write_at_method(field_type, attr, ctx));

            Some(TraitImplNode {
                associated_types: IndexMap::new(),
                generic_parameters: vec![],
                trait_ty: UncheckedType::Path(Box::new(PathNode::from_target(UncheckedType::Basic(Identifier::new(
                    ctx.intern("StorageAt"),
                    attr.location,
                ))))),
                ty: UncheckedType::Generic(
                    Identifier::new(ctx.intern("StorageRef"), attr.location),
                    vec![elem_ty.as_ref().clone(), UncheckedType::Const(ConstValue::U32(*size), attr.location)],
                    attr.location,
                ),
                body: methods,
                comments: vec![],
                location: attr.location,
                is_generated: true,
            })
        } else {
            None
        }
    }

    fn transform_struct_to_storage_ref<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
        new_name_suffix: Option<&str>,
    ) -> StructNode {
        let mut new_fields = IndexMap::new();
        for (field_name, field) in &struct_node.fields {
            let transformed_type = if field.attrs.iter().any(|a| a.name.id == ctx.intern("ref")) {
                let base_type = match &field.ty {
                    UncheckedType::Basic(ident) => ident,
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                };
                let ref_type_name = format!("{}Ref", ctx.ident(base_type.id));
                UncheckedType::Basic(Identifier::new(ctx.intern(ref_type_name.as_str()), attr.location))
            } else {
                let (inner_ty, size) = match &field.ty {
                    UncheckedType::Array(elem_ty, size, _) => (elem_ty.as_ref().clone(), *size),
                    _ => (field.ty.clone(), 1),
                };
                UncheckedType::Generic(
                    Identifier::new(ctx.intern("StorageRef"), attr.location),
                    vec![inner_ty, UncheckedType::Const(ConstValue::U32(size), attr.location)],
                    attr.location,
                )
            };
            new_fields.insert(
                field_name.clone(),
                StructField {
                    ty: transformed_type,
                    visibility: field.visibility,
                    comments: field.comments.clone(),
                    location: field.location,
                    attrs: field.attrs.clone(),
                },
            );
        }

        let new_name = if let Some(suffix) = new_name_suffix {
            let new_name_str = format!("{}{}", ctx.ident(struct_node.name.id), suffix);
            Identifier::new(ctx.intern(new_name_str.as_str()), attr.location)
        } else {
            struct_node.name
        };

        StructNode {
            name: new_name,
            generic_parameters: struct_node.generic_parameters.clone(),
            fields: new_fields,
            attrs: if new_name_suffix.is_some() {
                struct_node
                    .attrs
                    .iter()
                    .filter(|a| !a.is_derive() || !a.properties.iter().any(|p| p.id == ctx.intern("StorageRef")))
                    .cloned()
                    .collect()
            } else {
                struct_node.attrs.clone()
            },
            visibility: struct_node.visibility,
            comments: struct_node.comments.clone(),
            location: attr.location,
            is_generated: true,
        }
    }

    fn generate_new_method<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
        include_offset: bool,
    ) -> DefId {
        let metadata_ident = Identifier::new(ctx.intern("metadata"), attr.location);
        let offset_ident = Identifier::new(ctx.intern("offset"), attr.location);
        let initial_offset = if include_offset {
            ctx.alloc_expression(ExprNode::Path(PathNode {
                root: None,
                segments: vec![],
                target: UncheckedType::Basic(offset_ident),
                location: attr.location,
            }))
        } else {
            ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::from(0), attr.location)))
        };
        let mut offset = initial_offset;
        let mut field_inits = IndexMap::new();

        for (field_name, field) in &struct_node.fields {
            let (inner_ty, size, target_type) = if field.attrs.iter().any(|a| a.name.id == ctx.intern("ref")) {
                let base_type = match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.trim_end_matches("Ref");
                        UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location))
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                };
                (base_type.clone(), 1, field.ty.clone())
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 2 {
                            panic!("StorageRef must have exactly two generic parameters");
                        }
                        let size = match params[1] {
                            UncheckedType::Const(ConstValue::U32(size), _) => size,
                            _ => {
                                panic!("Second generic parameter of StorageRef must be a u32 const")
                            }
                        };
                        (params[0].clone(), size, field.ty.clone())
                    }
                    _ => (field.ty.clone(), 1, field.ty.clone()),
                }
            };
            let target_path = PathNode {
                root: None,
                segments: vec![],
                target: target_type,
                location: attr.location,
            };
            let new_path = PathNode {
                root: Some(UncheckedType::Path(Box::new(target_path.clone()))),
                segments: vec![],
                target: UncheckedType::Basic(Identifier::new(ctx.intern("new"), attr.location)),
                location: attr.location,
            };
            let metadata_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
                root: None,
                segments: vec![],
                target: UncheckedType::Basic(metadata_ident),
                location: attr.location,
            }));
            let args = vec![offset, metadata_expr];
            let callee = ctx.alloc_expression(ExprNode::Path(new_path));
            let new_call = ctx.alloc_expression(ExprNode::Call(CallNode {
                callee,
                generic_parameters: vec![],
                args,
                location: attr.location,
            }));
            field_inits.insert(field_name.clone(), new_call);

            let field_size = self.generate_field_size(attr, &field.ty, ctx);
            offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: offset,
                operator: BinaryOperator::Add,
                rhs: field_size,
                location: attr.location,
            }));
        }

        let struct_path = ctx.alloc_expression(ExprNode::Path(PathNode::from_target(UncheckedType::Basic(struct_node.name))));
        let struct_init = ctx.alloc_expression(ExprNode::Value(ValueNode::Struct(struct_path, vec![], field_inits, attr.location)));

        let return_stmt = ctx.alloc_statement(StmtNode::Return(ReturnNode {
            expr_id: Some(struct_init),
            comments: vec![],
            location: attr.location,
        }));

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: vec![return_stmt],
            expr: None,
            expr_comments: vec![],
            location: attr.location,
        }));

        let parameters = if include_offset {
            vec![
                FunctionParameter::new(
                    offset_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    metadata_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(ctx.intern("ContractMetadata"), attr.location)),
                    attr.location,
                ),
            ]
        } else {
            vec![FunctionParameter::new(
                metadata_ident,
                TypeQualifier::new(false, attr.location),
                UncheckedType::Basic(Identifier::new(ctx.intern("ContractMetadata"), attr.location)),
                attr.location,
            )]
        };

        let function = FunctionNode {
            name: Identifier::new(ctx.intern("new"), attr.location),
            parameters,
            generic_parameters: struct_node.generic_parameters.clone(),
            body: Some(block),
            return_type: Some(UncheckedType::Basic(Identifier::new(IdentId::TYPE_SELF, attr.location))),
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(function))
    }

    fn generate_accessor_impl<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
        is_ref_struct: bool,
    ) -> ImplNode {
        let mut methods = Vec::new();

        // Add get and set methods for the entire struct if it's a Ref struct
        if is_ref_struct {
            methods.push(self.generate_struct_getter(struct_node, attr, ctx));
            methods.push(self.generate_struct_setter(struct_node, attr, ctx));
        }

        // Add per-field accessors for #[storage] structs or array fields
        let mut offset = ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::from(0), attr.location)));
        for (field_name, field) in &struct_node.fields {
            let (inner_ty, size) = if field.attrs.iter().any(|a| a.name.id == ctx.intern("ref")) {
                match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.trim_end_matches("Ref");
                        (UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location)), 1)
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                }
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 2 {
                            panic!("StorageRef must have exactly two generic parameters");
                        }
                        let size = match params[1] {
                            UncheckedType::Const(ConstValue::U32(size), _) => size,
                            _ => {
                                panic!("Second generic parameter of StorageRef must be a u32 const")
                            }
                        };
                        (params[0].clone(), size)
                    }
                    _ => (field.ty.clone(), 1),
                }
            };

            // Generate per-field get/set for #[storage] structs
            if !is_ref_struct {
                methods.push(self.generate_getter(attr, &field_name.id, &inner_ty, offset, ctx));
                methods.push(self.generate_setter(attr, &field_name.id, &inner_ty, offset, ctx));
            }

            // Generate get_at/set_at for array fields (size > 1) in non-Ref structs
            if size > 1 && !is_ref_struct {
                methods.push(self.generate_getter_at(attr, &field_name.id, &field.ty, offset, ctx));
                methods.push(self.generate_setter_at(attr, &field_name.id, &field.ty, offset, ctx));
            }

            let field_size = self.generate_field_size(attr, &inner_ty, ctx);
            offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: offset,
                operator: BinaryOperator::Add,
                rhs: field_size,
                location: attr.location,
            }));
        }

        ImplNode {
            associated_types: IndexMap::new(),
            generic_parameters: struct_node.generic_parameters.clone(),
            ty: UncheckedType::Basic(struct_node.name),
            body: methods,
            comments: vec![],
            location: attr.location,
            is_generated: true,
        }
    }

    fn generate_field_size<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        attr: &AttrNode,
        field_type: &UncheckedType,
        ctx: &mut V,
    ) -> ExprId {
        let size_ident = Identifier::new(ctx.intern("size"), attr.location);
        let base_type = match field_type {
            UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                if params.len() != 2 {
                    panic!("StorageRef must have exactly two generic parameters");
                }
                let array_size = match &params[1] {
                    UncheckedType::Const(ConstValue::U32(size), _) => *size,
                    _ => panic!("Second generic parameter of StorageRef must be a u32 const"),
                };
                if array_size > 1 {
                    UncheckedType::Array(Box::new(params[0].clone()), array_size, attr.location)
                } else {
                    params[0].clone()
                }
            }
            UncheckedType::Basic(ident) => {
                let type_name = ctx.ident(ident.id).0.to_string();
                if type_name.ends_with("Ref") {
                    let base_name = type_name.trim_end_matches("Ref");
                    UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location))
                } else {
                    field_type.clone()
                }
            }
            _ => field_type.clone(),
        };
        let variable = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: Some(base_type),
            segments: vec![],
            target: UncheckedType::Basic(size_ident),
            location: attr.location,
        }));
        let node = CallNode {
            callee: variable,
            generic_parameters: Vec::new(),
            args: Vec::new(),
            location: attr.location,
        };
        ctx.alloc_expression(ExprNode::Call(node))
    }

    fn generate_storage_size_method<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> DefId {
        let mut sum = self.generate_field_size(attr, &struct_node.fields.iter().next().unwrap().1.ty, ctx);
        for (_, field) in struct_node.fields.iter().skip(1) {
            let inner_ty = if field.attrs.iter().any(|a| a.name.id == ctx.intern("ref")) {
                match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.trim_end_matches("Ref");
                        UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location))
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                }
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 2 {
                            panic!("StorageRef must have exactly two generic parameters");
                        }
                        params[0].clone()
                    }
                    _ => field.ty.clone(),
                }
            };
            let node = BinaryNode {
                lhs: sum,
                operator: BinaryOperator::Add,
                rhs: self.generate_field_size(attr, &inner_ty, ctx),
                location: attr.location,
            };
            sum = ctx.alloc_expression(ExprNode::Binary(node));
        }
        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: Vec::new(),
            expr: Some(sum),
            expr_comments: vec![],
            location: attr.location,
        }));

        let f = FunctionNode {
            name: Identifier::new(ctx.intern("size"), attr.location),
            parameters: vec![],
            generic_parameters: vec![],
            body: Some(block),
            return_type: Some(UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location))),
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(f))
    }

    fn generate_storage_read_method<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> DefId {
        let offset_ident = Identifier::new(ctx.intern("offset"), attr.location);
        let csth_ident = Identifier::new(ctx.intern("contract_state_tree_height"), attr.location);
        let user_id_ident = Identifier::new(ctx.intern("user_id"), attr.location);
        let contract_id_ident = Identifier::new(ctx.intern("contract_id"), attr.location);
        let offset_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(offset_ident),
            location: attr.location,
        }));
        let csth_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(csth_ident),
            location: attr.location,
        }));
        let user_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(user_id_ident),
            location: attr.location,
        }));
        let contract_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(contract_id_ident),
            location: attr.location,
        }));

        let mut field_reads = IndexMap::new();
        let mut offset = offset_expr;
        for (field_name, field) in &struct_node.fields {
            let inner_ty = if field.attrs.iter().any(|a| a.name.id == ctx.intern("ref")) {
                match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.trim_end_matches("Ref");
                        UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location))
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                }
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 2 {
                            panic!("StorageRef must have exactly two generic parameters");
                        }
                        params[0].clone()
                    }
                    _ => field.ty.clone(),
                }
            };
            let (_key, value) = self.generate_field_read(attr, &field_name.id, &inner_ty, offset, csth_expr, user_id_expr, contract_id_expr, ctx);
            field_reads.insert(field_name.clone(), value);
            let field_size = self.generate_field_size(attr, &inner_ty, ctx);
            offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: offset,
                operator: BinaryOperator::Add,
                rhs: field_size,
                location: attr.location,
            }));
        }
        let name_path = ctx.alloc_expression(ExprNode::Path(PathNode::from_target(UncheckedType::Basic(struct_node.name))));
        let read_expr = ctx.alloc_expression(ExprNode::Value(ValueNode::Struct(name_path, vec![], field_reads, attr.location)));

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: Vec::new(),
            expr: Some(read_expr),
            expr_comments: vec![],
            location: attr.location,
        }));

        let f = FunctionNode {
            name: Identifier::new(ctx.intern("read"), attr.location),
            parameters: vec![
                FunctionParameter::new(
                    csth_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    user_id_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    contract_id_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    offset_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
            ],
            generic_parameters: vec![],
            body: Some(block),
            return_type: Some(UncheckedType::Basic(Identifier::new(IdentId::TYPE_SELF, attr.location))),
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(f))
    }

    fn generate_storage_write_method<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> DefId {
        let offset_ident = Identifier::new(ctx.intern("offset"), attr.location);
        let value_ident = Identifier::new(ctx.intern("value"), attr.location);
        let offset_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(offset_ident),
            location: attr.location,
        }));
        let value_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(value_ident),
            location: attr.location,
        }));

        let mut field_writes = Vec::new();
        let mut offset = offset_expr;
        for (field_name, field) in &struct_node.fields {
            let inner_ty = if field.attrs.iter().any(|a| a.name.id == ctx.intern("ref")) {
                match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.trim_end_matches("Ref");
                        UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location))
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                }
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 2 {
                            panic!("StorageRef must have exactly two generic parameters");
                        }
                        params[0].clone()
                    }
                    _ => field.ty.clone(),
                }
            };
            let stmt_id = self.generate_field_write(attr, &field_name.id, &inner_ty, offset, ctx);
            field_writes.push(stmt_id);
            let field_size = self.generate_field_size(attr, &inner_ty, ctx);
            offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: offset,
                operator: BinaryOperator::Add,
                rhs: field_size,
                location: attr.location,
            }));
        }

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: field_writes,
            expr: None,
            expr_comments: vec![],
            location: attr.location,
        }));

        let f = FunctionNode {
            name: Identifier::new(ctx.intern("write"), attr.location),
            parameters: vec![
                FunctionParameter::new(
                    offset_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    value_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_SELF, attr.location)),
                    attr.location,
                ),
            ],
            generic_parameters: vec![],
            body: Some(block),
            return_type: None,
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(f))
    }

    fn generate_struct_getter<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> DefId {
        let self_ident = Identifier::new(ctx.intern("self"), attr.location);
        let self_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(self_ident),
            location: attr.location,
        }));

        let mut field_reads = IndexMap::new();
        let mut offset = ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::from(0), attr.location)));
        for (field_name, field) in &struct_node.fields {
            let (inner_ty, size) = if field.attrs.iter().any(|a| a.name.id == ctx.intern("ref")) {
                match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.trim_end_matches("Ref");
                        (UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location)), 1)
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                }
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 2 {
                            panic!("StorageRef must have exactly two generic parameters");
                        }
                        let size = match params[1] {
                            UncheckedType::Const(ConstValue::U32(size), _) => size,
                            _ => {
                                panic!("Second generic parameter of StorageRef must be a u32 const")
                            }
                        };
                        (params[0].clone(), size)
                    }
                    _ => (field.ty.clone(), 1),
                }
            };

            let read_expr = if size > 1 {
                let field_type = UncheckedType::Array(Box::new(inner_ty.clone()), size, attr.location);
                let read_ident = Identifier::new(ctx.intern("read"), attr.location);
                let read_path = PathNode {
                    root: Some(field_type),
                    segments: vec![],
                    target: UncheckedType::Basic(read_ident),
                    location: attr.location,
                };
                let read_expr = ctx.alloc_expression(ExprNode::Path(read_path));
                // For StorageRef, use metadata
                let metadata_ident = Identifier::new(ctx.intern("metadata"), attr.location);
                let contract_state_tree_height_ident = Identifier::new(ctx.intern("contract_state_tree_height"), attr.location);
                let user_id_ident = Identifier::new(ctx.intern("user_id"), attr.location);
                let contract_id_ident = Identifier::new(ctx.intern("contract_id"), attr.location);
                let metadata_target_expr = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: self_expr.clone(),
                    field: field_name.clone(),
                    location: attr.location,
                }));
                let metadata_expr = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: metadata_target_expr.clone(),
                    field: metadata_ident,
                    location: attr.location,
                }));
                let csth_expr = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: metadata_expr.clone(),
                    field: contract_state_tree_height_ident,
                    location: attr.location,
                }));
                let user_id_expr = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: metadata_expr.clone(),
                    field: user_id_ident,
                    location: attr.location,
                }));
                let contract_id_expr = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: metadata_expr,
                    field: contract_id_ident,
                    location: attr.location,
                }));
                ctx.alloc_expression(ExprNode::Call(CallNode {
                    callee: read_expr,
                    generic_parameters: vec![],
                    args: vec![csth_expr, user_id_expr, contract_id_expr, offset],
                    location: attr.location,
                }))
            } else {
                let field_access = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: self_expr.clone(),
                    field: Identifier::new(field_name.id, attr.location),
                    location: attr.location,
                }));
                let get_ident = Identifier::new(ctx.intern("get"), attr.location);
                let get_callee = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: field_access.clone(),
                    field: get_ident,
                    location: attr.location,
                }));
                ctx.alloc_expression(ExprNode::MemberCall(MemberCallNode {
                    callee: get_callee,
                    receiver: field_access,
                    generic_parameters: vec![],
                    args: vec![],
                    location: attr.location,
                }))
            };
            field_reads.insert(field_name.clone(), read_expr);

            let field_size = self.generate_field_size(attr, &inner_ty, ctx);
            offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: offset,
                operator: BinaryOperator::Add,
                rhs: field_size,
                location: attr.location,
            }));
        }

        let base_struct_name = if ctx.ident(struct_node.name.id).to_string().ends_with("Ref") {
            let base_name = ctx.ident(struct_node.name.id).to_string().trim_end_matches("Ref").to_string();
            Identifier::new(ctx.intern(base_name), attr.location)
        } else {
            struct_node.name
        };

        let struct_path = ctx.alloc_expression(ExprNode::Path(PathNode::from_target(UncheckedType::Basic(base_struct_name))));
        let struct_init = ctx.alloc_expression(ExprNode::Value(ValueNode::Struct(struct_path, vec![], field_reads, attr.location)));

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: Vec::new(),
            expr: Some(struct_init),
            expr_comments: vec![],
            location: attr.location,
        }));

        let base_type = UncheckedType::Basic(base_struct_name);

        let function = FunctionNode {
            name: Identifier::new(ctx.intern("get"), attr.location),
            parameters: vec![FunctionParameter::new(
                self_ident,
                TypeQualifier::new(false, attr.location),
                UncheckedType::Basic(Identifier::new(IdentId::TYPE_SELF, attr.location)),
                attr.location,
            )],
            generic_parameters: vec![],
            body: Some(block),
            return_type: Some(base_type),
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(function))
    }

    fn generate_struct_setter<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> DefId {
        let self_ident = Identifier::new(ctx.intern("self"), attr.location);
        let value_ident = Identifier::new(ctx.intern("value"), attr.location);
        let self_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(self_ident),
            location: attr.location,
        }));
        let value_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(value_ident),
            location: attr.location,
        }));

        let mut stmts = Vec::new();
        let mut offset = ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::from(0), attr.location)));
        for (field_name, field) in &struct_node.fields {
            let (inner_ty, size) = if field.attrs.iter().any(|a| a.name.id == ctx.intern("ref")) {
                match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.trim_end_matches("Ref");
                        (UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location)), 1)
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                }
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 2 {
                            panic!("StorageRef must have exactly two generic parameters");
                        }
                        let size = match params[1] {
                            UncheckedType::Const(ConstValue::U32(size), _) => size,
                            _ => {
                                panic!("Second generic parameter of StorageRef must be a u32 const")
                            }
                        };
                        (params[0].clone(), size)
                    }
                    _ => (field.ty.clone(), 1),
                }
            };

            let value_field = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                target: value_expr.clone(),
                field: Identifier::new(field_name.id, attr.location),
                location: attr.location,
            }));

            let write_stmt = if size > 1 {
                let field_type = UncheckedType::Array(Box::new(inner_ty.clone()), size, attr.location);
                let write_ident = Identifier::new(ctx.intern("write"), attr.location);
                let write_path = PathNode {
                    root: Some(field_type),
                    segments: vec![],
                    target: UncheckedType::Basic(write_ident),
                    location: attr.location,
                };
                let write_expr = ctx.alloc_expression(ExprNode::Path(write_path));
                let write_call = ctx.alloc_expression(ExprNode::Call(CallNode {
                    callee: write_expr,
                    generic_parameters: vec![],
                    args: vec![offset, value_field],
                    location: attr.location,
                }));
                ctx.alloc_statement(StmtNode::Expression(write_call))
            } else {
                let field_access = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: self_expr.clone(),
                    field: Identifier::new(field_name.id, attr.location),
                    location: attr.location,
                }));
                let set_ident = Identifier::new(ctx.intern("set"), attr.location);
                let set_callee = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: field_access.clone(),
                    field: set_ident,
                    location: attr.location,
                }));
                let set_call = ctx.alloc_expression(ExprNode::MemberCall(MemberCallNode {
                    callee: set_callee,
                    receiver: field_access,
                    generic_parameters: vec![],
                    args: vec![value_field],
                    location: attr.location,
                }));
                ctx.alloc_statement(StmtNode::Expression(set_call))
            };
            stmts.push(write_stmt);

            let field_size = self.generate_field_size(attr, &inner_ty, ctx);
            offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: offset,
                operator: BinaryOperator::Add,
                rhs: field_size,
                location: attr.location,
            }));
        }

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts,
            expr: None,
            expr_comments: vec![],
            location: attr.location,
        }));

        let base_struct_name = if ctx.ident(struct_node.name.id).to_string().ends_with("Ref") {
            let base_name = ctx.ident(struct_node.name.id).to_string().trim_end_matches("Ref").to_string();
            Identifier::new(ctx.intern(base_name), attr.location)
        } else {
            struct_node.name
        };

        let base_type = UncheckedType::Basic(base_struct_name);

        let function = FunctionNode {
            name: Identifier::new(ctx.intern("set"), attr.location),
            parameters: vec![
                FunctionParameter::new(
                    self_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_SELF, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(value_ident, TypeQualifier::new(false, attr.location), base_type, attr.location),
            ],
            generic_parameters: vec![],
            body: Some(block),
            return_type: None,
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(function))
    }

    fn generate_getter<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        attr: &AttrNode,
        field_name: &IdentId,
        field_type: &UncheckedType,
        offset: ExprId,
        ctx: &mut V,
    ) -> DefId {
        let getter_name = format!("get_{}", ctx.ident(*field_name));
        let getter_ident = Identifier::new(ctx.intern(getter_name), attr.location);
        let csth_ident = Identifier::new(ctx.intern("contract_state_tree_height"), attr.location);
        let user_id_ident = Identifier::new(ctx.intern("user_id"), attr.location);
        let contract_id_ident = Identifier::new(ctx.intern("contract_id"), attr.location);
        let csth_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(csth_ident),
            location: attr.location,
        }));
        let user_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(user_id_ident),
            location: attr.location,
        }));
        let contract_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(contract_id_ident),
            location: attr.location,
        }));

        let read_ident = Identifier::new(ctx.intern("read"), attr.location);
        let read_path = PathNode {
            root: Some(field_type.clone()),
            segments: vec![],
            target: UncheckedType::Basic(read_ident),
            location: attr.location,
        };
        let read_expr = ctx.alloc_expression(ExprNode::Path(read_path));
        let read_call = ctx.alloc_expression(ExprNode::Call(CallNode {
            callee: read_expr,
            generic_parameters: vec![],
            args: vec![csth_expr, user_id_expr, contract_id_expr, offset],
            location: attr.location,
        }));

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: Vec::new(),
            expr: Some(read_call),
            expr_comments: vec![],
            location: attr.location,
        }));

        let function = FunctionNode {
            name: getter_ident,
            parameters: vec![
                FunctionParameter::new(
                    csth_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    user_id_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    contract_id_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
            ],
            generic_parameters: vec![],
            body: Some(block),
            return_type: Some(field_type.clone()),
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(function))
    }

    fn generate_setter<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        attr: &AttrNode,
        field_name: &IdentId,
        field_type: &UncheckedType,
        offset: ExprId,
        ctx: &mut V,
    ) -> DefId {
        let setter_name = format!("set_{}", ctx.ident(*field_name));
        let setter_ident = Identifier::new(ctx.intern(setter_name), attr.location);
        let value_ident = Identifier::new(ctx.intern("value"), attr.location);

        let value_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(value_ident),
            location: attr.location,
        }));

        let write_ident = Identifier::new(ctx.intern("write"), attr.location);
        let write_path = PathNode {
            root: Some(field_type.clone()),
            segments: vec![],
            target: UncheckedType::Basic(write_ident),
            location: attr.location,
        };
        let write_expr = ctx.alloc_expression(ExprNode::Path(write_path));
        let write_call = ctx.alloc_expression(ExprNode::Call(CallNode {
            callee: write_expr,
            generic_parameters: vec![],
            args: vec![offset, value_expr],
            location: attr.location,
        }));
        let write_stmt = ctx.alloc_statement(StmtNode::Expression(write_call));

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: vec![write_stmt],
            expr: None,
            expr_comments: vec![],
            location: attr.location,
        }));

        let function = FunctionNode {
            name: setter_ident,
            parameters: vec![FunctionParameter::new(
                value_ident,
                TypeQualifier::new(false, attr.location),
                field_type.clone(),
                attr.location,
            )],
            generic_parameters: vec![],
            body: Some(block),
            return_type: None,
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(function))
    }

    fn generate_getter_at<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        attr: &AttrNode,
        field_name: &IdentId,
        field_type: &UncheckedType,
        offset: ExprId,
        ctx: &mut V,
    ) -> DefId {
        let getter_name = format!("get_{}_at", ctx.ident(*field_name));
        let getter_ident = Identifier::new(ctx.intern(getter_name), attr.location);
        let index_ident = Identifier::new(ctx.intern("index"), attr.location);
        let csth_ident = Identifier::new(ctx.intern("contract_state_tree_height"), attr.location);
        let user_id_ident = Identifier::new(ctx.intern("user_id"), attr.location);
        let contract_id_ident = Identifier::new(ctx.intern("contract_id"), attr.location);

        let offset_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(index_ident),
            location: attr.location,
        }));
        let csth_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(csth_ident),
            location: attr.location,
        }));
        let user_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(user_id_ident),
            location: attr.location,
        }));
        let contract_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(contract_id_ident),
            location: attr.location,
        }));

        let elem_ty = if let UncheckedType::Generic(ident, params, _) = field_type {
            if ident.id == ctx.intern("StorageRef") && params.len() == 2 {
                params[0].clone()
            } else {
                panic!("Expected StorageRef with two generic parameters");
            }
        } else if let UncheckedType::Array(elem_ty, _, _) = field_type {
            elem_ty.as_ref().clone()
        } else {
            panic!("generate_getter_at called on non-StorageRef or non-array type");
        };

        let elem_size = self.generate_field_size(attr, &elem_ty, ctx);
        let scaled_index = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
            lhs: offset_expr,
            operator: BinaryOperator::Mul,
            rhs: elem_size,
            location: attr.location,
        }));
        let final_offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
            lhs: offset,
            operator: BinaryOperator::Add,
            rhs: scaled_index,
            location: attr.location,
        }));

        let read_ident = Identifier::new(ctx.intern("read"), attr.location);
        let read_path = PathNode {
            root: Some(elem_ty.clone()),
            segments: vec![],
            target: UncheckedType::Basic(read_ident),
            location: attr.location,
        };
        let read_expr = ctx.alloc_expression(ExprNode::Path(read_path));
        let read_call = ctx.alloc_expression(ExprNode::Call(CallNode {
            callee: read_expr,
            generic_parameters: vec![],
            args: vec![csth_expr, user_id_expr, contract_id_expr, final_offset],
            location: attr.location,
        }));

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: Vec::new(),
            expr: Some(read_call),
            expr_comments: vec![],
            location: attr.location,
        }));

        let function = FunctionNode {
            name: getter_ident,
            parameters: vec![
                FunctionParameter::new(
                    csth_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    user_id_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    contract_id_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    index_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
            ],
            generic_parameters: vec![],
            body: Some(block),
            return_type: Some(elem_ty),
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(function))
    }

    fn generate_setter_at<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        attr: &AttrNode,
        field_name: &IdentId,
        field_type: &UncheckedType,
        offset: ExprId,
        ctx: &mut V,
    ) -> DefId {
        let setter_name = format!("set_{}_at", ctx.ident(*field_name));
        let setter_ident = Identifier::new(ctx.intern(setter_name), attr.location);
        let index_ident = Identifier::new(ctx.intern("index"), attr.location);
        let value_ident = Identifier::new(ctx.intern("value"), attr.location);

        let offset_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(index_ident),
            location: attr.location,
        }));

        let elem_ty = if let UncheckedType::Generic(ident, params, _) = field_type {
            if ident.id == ctx.intern("StorageRef") && params.len() == 2 {
                params[0].clone()
            } else {
                panic!("Expected StorageRef with two generic parameters");
            }
        } else if let UncheckedType::Array(elem_ty, _, _) = field_type {
            elem_ty.as_ref().clone()
        } else {
            panic!("generate_setter_at called on non-StorageRef or non-array type");
        };

        let elem_size = self.generate_field_size(attr, &elem_ty, ctx);
        let scaled_index = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
            lhs: offset_expr,
            operator: BinaryOperator::Mul,
            rhs: elem_size,
            location: attr.location,
        }));
        let final_offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
            lhs: offset,
            operator: BinaryOperator::Add,
            rhs: scaled_index,
            location: attr.location,
        }));

        let value_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(value_ident),
            location: attr.location,
        }));

        let write_ident = Identifier::new(ctx.intern("write"), attr.location);
        let write_path = PathNode {
            root: Some(elem_ty.clone()),
            segments: vec![],
            target: UncheckedType::Basic(write_ident),
            location: attr.location,
        };
        let write_expr = ctx.alloc_expression(ExprNode::Path(write_path));
        let write_call = ctx.alloc_expression(ExprNode::Call(CallNode {
            callee: write_expr,
            generic_parameters: vec![],
            args: vec![final_offset, value_expr],
            location: attr.location,
        }));
        let write_stmt = ctx.alloc_statement(StmtNode::Expression(write_call));

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: vec![write_stmt],
            expr: None,
            expr_comments: vec![],
            location: attr.location,
        }));

        let function = FunctionNode {
            name: setter_ident,
            parameters: vec![
                FunctionParameter::new(
                    index_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(value_ident, TypeQualifier::new(false, attr.location), elem_ty.clone(), attr.location),
            ],
            generic_parameters: vec![],
            body: Some(block),
            return_type: None,
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(function))
    }

    fn generate_storage_read_at_method<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        field_type: &UncheckedType,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> DefId {
        let offset_ident = Identifier::new(ctx.intern("offset"), attr.location);
        let index_ident = Identifier::new(ctx.intern("index"), attr.location);
        let csth_ident = Identifier::new(ctx.intern("contract_state_tree_height"), attr.location);
        let user_id_ident = Identifier::new(ctx.intern("user_id"), attr.location);
        let contract_id_ident = Identifier::new(ctx.intern("contract_id"), attr.location);

        let mut stmts = Vec::new();
        let array_size = if let UncheckedType::Array(_, size, _) = field_type {
            *size
        } else {
            panic!("generate_storage_read_at_method called on non-array type");
        };

        let assert_stmt = self.generate_index_bounds_check(attr, index_ident, array_size, ctx);
        stmts.push(assert_stmt);

        let offset_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(offset_ident),
            location: attr.location,
        }));
        let index_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(index_ident),
            location: attr.location,
        }));
        let csth_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(csth_ident),
            location: attr.location,
        }));
        let user_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(user_id_ident),
            location: attr.location,
        }));
        let contract_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(contract_id_ident),
            location: attr.location,
        }));

        let elem_ty = if let UncheckedType::Array(elem_ty, _, _) = field_type {
            elem_ty.as_ref().clone()
        } else {
            unreachable!()
        };
        let elem_size = self.generate_field_size(attr, &elem_ty, ctx);
        let scaled_index = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
            lhs: index_expr,
            operator: BinaryOperator::Mul,
            rhs: elem_size,
            location: attr.location,
        }));
        let final_offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
            lhs: offset_expr,
            operator: BinaryOperator::Add,
            rhs: scaled_index,
            location: attr.location,
        }));

        let read_ident = Identifier::new(ctx.intern("read"), attr.location);
        let variable = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: Some(elem_ty.clone()),
            segments: vec![],
            target: UncheckedType::Basic(read_ident),
            location: attr.location,
        }));
        let read_call = CallNode {
            callee: variable,
            generic_parameters: Vec::new(),
            args: vec![csth_expr, user_id_expr, contract_id_expr, final_offset],
            location: attr.location,
        };
        let read_expr = ctx.alloc_expression(ExprNode::Call(read_call));

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts,
            expr: Some(read_expr),
            expr_comments: vec![],
            location: attr.location,
        }));

        let f = FunctionNode {
            name: Identifier::new(ctx.intern("read_at"), attr.location),
            parameters: vec![
                FunctionParameter::new(
                    csth_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    user_id_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    contract_id_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    index_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    offset_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
            ],
            generic_parameters: vec![],
            body: Some(block),
            return_type: Some(elem_ty),
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(f))
    }

    fn generate_storage_write_at_method<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        field_type: &UncheckedType,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> DefId {
        let offset_ident = Identifier::new(ctx.intern("offset"), attr.location);
        let index_ident = Identifier::new(ctx.intern("index"), attr.location);
        let value_ident = Identifier::new(ctx.intern("value"), attr.location);

        let mut stmts = Vec::new();
        let array_size = if let UncheckedType::Array(_, size, _) = field_type {
            *size
        } else {
            panic!("generate_storage_write_at_method called on non-array type");
        };

        let assert_stmt = self.generate_index_bounds_check(attr, index_ident, array_size, ctx);
        stmts.push(assert_stmt);

        let offset_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(offset_ident),
            location: attr.location,
        }));
        let index_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(index_ident),
            location: attr.location,
        }));

        let elem_ty = if let UncheckedType::Array(elem_ty, _, _) = field_type {
            elem_ty.as_ref().clone()
        } else {
            unreachable!()
        };
        let elem_size = self.generate_field_size(attr, &elem_ty, ctx);
        let scaled_index = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
            lhs: index_expr,
            operator: BinaryOperator::Mul,
            rhs: elem_size,
            location: attr.location,
        }));
        let final_offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
            lhs: offset_expr,
            operator: BinaryOperator::Add,
            rhs: scaled_index,
            location: attr.location,
        }));

        let value_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(value_ident),
            location: attr.location,
        }));

        let write_ident = Identifier::new(ctx.intern("write"), attr.location);
        let variable = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: Some(elem_ty.clone()),
            segments: vec![],
            target: UncheckedType::Basic(write_ident),
            location: attr.location,
        }));
        let write_call = CallNode {
            callee: variable,
            generic_parameters: Vec::new(),
            args: vec![final_offset, value_expr],
            location: attr.location,
        };
        let write_expr = ctx.alloc_expression(ExprNode::Call(write_call));
        let write_stmt = ctx.alloc_statement(StmtNode::Expression(write_expr));
        stmts.push(write_stmt);

        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts,
            expr: None,
            expr_comments: vec![],
            location: attr.location,
        }));

        let f = FunctionNode {
            name: Identifier::new(ctx.intern("write_at"), attr.location),
            parameters: vec![
                FunctionParameter::new(
                    offset_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(
                    index_ident,
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(value_ident, TypeQualifier::new(false, attr.location), elem_ty.clone(), attr.location),
            ],
            generic_parameters: vec![],
            body: Some(block),
            return_type: None,
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: attr.location,
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
        };

        ctx.alloc_definition(DefinitionNode::Function(f))
    }

    fn generate_index_bounds_check<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        attr: &AttrNode,
        index_ident: Identifier,
        bound: u32,
        ctx: &mut V,
    ) -> StmtId {
        let index_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(index_ident),
            location: attr.location,
        }));
        let bound_expr = ctx.alloc_expression(ExprNode::Value(ValueNode::U32(F::from(bound), attr.location)));
        let assert_node = IntrinsicStmtNode::Assert {
            left: ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: index_expr,
                operator: BinaryOperator::Lt,
                rhs: bound_expr,
                location: attr.location,
            })),
            message: Some("Error: index out of bounds".to_string()),
            comments: vec![],
            location: attr.location,
        };
        ctx.alloc_statement(StmtNode::Intrinsic(assert_node))
    }

    fn generate_field_read<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        attr: &AttrNode,
        field_name: &IdentId,
        field_type: &UncheckedType,
        offset: ExprId,
        csth_expr: ExprId,
        user_id_expr: ExprId,
        contract_id_expr: ExprId,
        ctx: &mut V,
    ) -> (IdentId, ExprId) {
        let read_ident = Identifier::new(ctx.intern("read"), attr.location);
        let variable = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: Some(field_type.clone()),
            segments: vec![],
            target: UncheckedType::Basic(read_ident),
            location: attr.location,
        }));
        let node = CallNode {
            callee: variable,
            generic_parameters: Vec::new(),
            args: vec![csth_expr, user_id_expr, contract_id_expr, offset],
            location: attr.location,
        };
        (field_name.clone(), ctx.alloc_expression(ExprNode::Call(node)))
    }

    fn generate_field_write<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        attr: &AttrNode,
        field_name: &IdentId,
        field_type: &UncheckedType,
        offset: ExprId,
        ctx: &mut V,
    ) -> StmtId {
        let value_ident = Identifier::new(ctx.intern("value"), attr.location);
        let write_ident = Identifier::new(ctx.intern("write"), attr.location);
        let variable = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: Some(field_type.clone()),
            segments: vec![],
            target: UncheckedType::Basic(write_ident),
            location: attr.location,
        }));
        let value = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(value_ident),
            location: attr.location,
        }));
        let field = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
            target: value,
            field: Identifier::new(*field_name, attr.location),
            location: attr.location,
        }));
        let node = CallNode {
            callee: variable,
            generic_parameters: Vec::new(),
            args: vec![offset, field],
            location: attr.location,
        };
        let node_id = ctx.alloc_expression(ExprNode::Call(node));
        ctx.alloc_statement(StmtNode::Expression(node_id))
    }
}

impl<'a, F: Clone + From<u32> + 'static, C> AstVisitor<F, C> for StorageProcessor<'a> {
    type Context = DefaultVisitorContext<'a, F, C>;
    type ExprResult = ();
    type StmtResult = ();
    type Error = psy_common::Error;
    type Expr = ExprNode<F>;
    type Stmt = StmtNode;
    type Definition = DefinitionNode;
    type DefinitionResult = ();

    fn visit_struct(
        &mut self,
        node: DefId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::DefinitionResult, Self::Error> {
        let s: StructNode = ctx.definition(node).as_struct().unwrap().clone();
        let storage_trait_id = ctx.intern("Storage");
        let storage_attribute_id = ctx.intern("storage");
        let storage_ref_attribute_id = ctx.intern("StorageRef");
        let contract_attribute_id = ctx.intern("contract");

        for attr in &s.attrs {
            if attr.is_derive() && attr.properties.iter().any(|p| p == &storage_trait_id) {
                let impl_node = self.generate_storage_impl(&s, attr, ctx);
                ctx.insert_definition(DefinitionNode::TraitImpl(impl_node), InsertPosition::End);
            }

            if attr.name == storage_attribute_id {
                let impl_node = self.generate_accessor_impl(&s, attr, ctx, false);
                ctx.insert_definition(DefinitionNode::Impl(impl_node), InsertPosition::End);
            }

            if attr.is_derive() && attr.properties.iter().any(|p| p == &storage_ref_attribute_id) {
                let ref_struct = self.transform_struct_to_storage_ref(&s, attr, ctx, Some("Ref"));
                ctx.insert_definition(DefinitionNode::Struct(ref_struct.clone()), InsertPosition::End);

                let include_offset = !s.attrs.iter().any(|a| a.name == contract_attribute_id);
                let mut methods = Vec::new();
                methods.push(self.generate_new_method(&ref_struct, attr, ctx, include_offset));
                let impl_node = ImplNode {
                    associated_types: IndexMap::new(),
                    generic_parameters: ref_struct.generic_parameters.clone(),
                    ty: UncheckedType::Basic(ref_struct.name),
                    body: methods,
                    comments: vec![],
                    location: attr.location,
                    is_generated: true,
                };
                ctx.insert_definition(DefinitionNode::Impl(impl_node), InsertPosition::End);

                let accessor_impl = self.generate_accessor_impl(&ref_struct, attr, ctx, true);
                ctx.insert_definition(DefinitionNode::Impl(accessor_impl), InsertPosition::End);

                for (_, field) in &ref_struct.fields {
                    if let Some(impl_node) = self.generate_storage_at_impl(&field.ty, attr, ctx) {
                        ctx.insert_definition(DefinitionNode::TraitImpl(impl_node), InsertPosition::End);
                    }
                }
            }
        }

        Ok(())
    }

    fn visit_use(&mut self, _def_id: DefId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::StmtResult, Self::Error> {
        Ok(())
    }

    fn visit_path(&mut self, _node: ExprId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_index_access(
        &mut self,
        _node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_member_access(
        &mut self,
        _node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_value(&mut self, _node: ExprId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_binary(
        &mut self,
        _node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_unary(&mut self, _node: ExprId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_call(&mut self, _node: ExprId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_cast(&mut self, _node: ExprId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_while(&mut self, _node: StmtId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::StmtResult, Self::Error> {
        Ok(())
    }

    fn visit_assignment(
        &mut self,
        _node: StmtId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::StmtResult, Self::Error> {
        Ok(())
    }

    fn visit_variable(
        &mut self,
        _node: StmtId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::StmtResult, Self::Error> {
        Ok(())
    }

    fn visit_return(
        &mut self,
        _expr: StmtId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::StmtResult, Self::Error> {
        Ok(())
    }

    fn visit_impl(
        &mut self,
        node: DefId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::DefinitionResult, Self::Error> {
        let defs: Vec<DefId> = ctx.definition(node).as_impl().unwrap().body.clone();
        for def_id in defs {
            self.visit_definition(def_id, ctx)?;
        }
        Ok(())
    }

    fn visit_trait(
        &mut self,
        _node: DefId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::DefinitionResult, Self::Error> {
        Ok(())
    }

    fn visit_function(
        &mut self,
        _node: DefId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::DefinitionResult, Self::Error> {
        Ok(())
    }

    fn visit_enum(&mut self, _node: DefId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::StmtResult, Self::Error> {
        Ok(())
    }

    fn visit_intrinsic_expr(
        &mut self,
        _node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_intrinsic_stmt(
        &mut self,
        _node: StmtId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::StmtResult, Self::Error> {
        Ok(())
    }

    fn visit_member_call(
        &mut self,
        _node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_type_alias(
        &mut self,
        _node: DefId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::DefinitionResult, Self::Error> {
        Ok(())
    }

    fn visit_const(
        &mut self,
        _node: DefId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::DefinitionResult, Self::Error> {
        Ok(())
    }

    fn visit_for(&mut self, _node: StmtId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::StmtResult, Self::Error> {
        Ok(())
    }

    fn visit_match(&mut self, _node: ExprId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::StmtResult, Self::Error> {
        Ok(())
    }

    fn visit_parentheses(
        &mut self,
        node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::StmtResult, Self::Error> {
        let inner_expr_id = ctx.expression(node).as_parentheses().unwrap().clone();
        self.visit_expr(inner_expr_id, ctx)
    }

    fn visit_lambda_function(
        &mut self,
        _node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_trait_impl(
        &mut self,
        _node: DefId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::DefinitionResult, Self::Error> {
        Ok(())
    }

    fn visit_if_expr(
        &mut self,
        _node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_block_expr(
        &mut self,
        _node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_tuple(&mut self, _node: ExprId, ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }

    fn visit_tuple_access(
        &mut self,
        _node: ExprId,
        ctx: &mut <StorageProcessor<'a> as AstVisitor<F, C>>::Context,
    ) -> Result<Self::ExprResult, Self::Error> {
        Ok(())
    }
}
