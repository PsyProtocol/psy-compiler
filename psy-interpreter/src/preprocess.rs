use std::{collections::HashSet, marker::PhantomData};

use indexmap::IndexMap;
use psy_ast::*;
use psy_vm::dpn::ops::context_trait::ContextFelt;

#[derive(Debug)]
pub struct StorageProcessor<'a> {
    _marker: PhantomData<&'a ()>,
}

impl<'a> StorageProcessor<'a> {
    pub fn new() -> Self {
        Self { _marker: PhantomData }
    }

    fn has_ref_type_attr<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        attrs: &[AttrNode],
        ctx: &mut V,
    ) -> bool {
        attrs.iter().any(|a| a.name.id == ctx.intern("ref"))
    }

    fn is_map_ref_type<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        ty: &UncheckedType,
        ctx: &mut V,
    ) -> bool {
        matches!(ty, UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("MapRef") && params.len() == 3)
    }

    fn struct_has_map_ref_fields<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        struct_node: &StructNode,
        ctx: &mut V,
    ) -> bool {
        struct_node.fields.values().any(|field| self.is_map_ref_type(&field.ty, ctx))
    }

    fn find_struct_definition<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode, Program = Program<F>>,
    >(
        &self,
        struct_id: IdentId,
        ctx: &mut V,
    ) -> Option<StructNode> {
        ctx.program().defs.iter().find_map(|def| match def {
            DefinitionNode::Struct(s) if s.name.id == struct_id => Some(s.clone()),
            _ => None,
        })
    }

    fn count_maps_in_type<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode, Program = Program<F>>,
    >(
        &self,
        ty: &UncheckedType,
        ctx: &mut V,
        visiting: &mut HashSet<IdentId>,
    ) -> usize {
        match ty {
            UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("Map") => 1,
            UncheckedType::Array(elem_ty, _, _) => self.count_maps_in_type(elem_ty, ctx, visiting),
            UncheckedType::Basic(ident) => {
                if !visiting.insert(ident.id) {
                    return 0;
                }
                let total = self
                    .find_struct_definition(ident.id, ctx)
                    .map(|s| s.fields.values().map(|field| self.count_maps_in_type(&field.ty, ctx, visiting)).sum())
                    .unwrap_or(0);
                visiting.remove(&ident.id);
                total
            }
            UncheckedType::Generic(_, params, _) => params.iter().map(|param| self.count_maps_in_type(param, ctx, visiting)).sum(),
            _ => 0,
        }
    }

    fn count_maps_in_struct<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode, Program = Program<F>>,
    >(
        &self,
        struct_node: &StructNode,
        ctx: &mut V,
    ) -> usize {
        let mut visiting = HashSet::new();
        visiting.insert(struct_node.name.id);
        struct_node
            .fields
            .values()
            .map(|field| self.count_maps_in_type(&field.ty, ctx, &mut visiting))
            .sum()
    }

    fn generate_event_impl<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> TraitImplNode {
        TraitImplNode {
            associated_types: IndexMap::new(),
            generic_parameters: vec![],
            ty: UncheckedType::Basic(struct_node.name),
            trait_ty: UncheckedType::Basic(Identifier::new(ctx.intern("Event"), attr.location)),
            body: vec![],
            attrs: vec![],
            comments: vec![],
            location: attr.location,
            is_generated: true,
        }
    }

    fn generate_storage_impl<F: Clone + From<u32>, C, V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>>(
        &self,
        struct_node: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> TraitImplNode {
        let mut methods = Vec::new();
        let mut associated_types = IndexMap::new();

        methods.push(self.generate_storage_size_method(struct_node, attr, ctx));
        methods.push(self.generate_storage_read_method(struct_node, attr, ctx));
        methods.push(self.generate_storage_write_method(struct_node, attr, ctx));

        let has_storage_ref_behavior = struct_node.attrs.iter().any(|a| {
            a.is_derive()
                && a.properties
                    .iter()
                    .any(|p| p.id == ctx.intern("StorageRef") || p.id == ctx.intern("Storage"))
        });
        let ref_type = if has_storage_ref_behavior {
            let ref_struct_name = format!("{}Ref", ctx.ident(struct_node.name.id));
            UncheckedType::Basic(Identifier::new(ctx.intern(ref_struct_name), attr.location))
        } else {
            UncheckedType::Generic(
                Identifier::new(ctx.intern("StorageRef"), attr.location),
                vec![UncheckedType::Basic(struct_node.name)],
                attr.location,
            )
        };
        associated_types.insert(
            Identifier::new(ctx.intern("RefType"), attr.location),
            AssociatedTypeValue {
                ty: ref_type,
                visibility: Visibility::Public,
                comments: vec![],
                location: attr.location,
            },
        );

        TraitImplNode {
            associated_types,
            generic_parameters: vec![],
            trait_ty: UncheckedType::Basic(Identifier::new(ctx.intern("Storage"), attr.location)),
            ty: UncheckedType::Basic(struct_node.name),
            body: methods,
            attrs: vec![],
            comments: vec![],
            location: attr.location,
            is_generated: true,
        }
    }

    fn generate_storage_at_impl<
        F: Clone + From<u32> + ContextFelt,
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
                    Identifier::new(ctx.intern("ArrayRef"), attr.location),
                    vec![elem_ty.as_ref().clone(), UncheckedType::Const(*size, attr.location)],
                    attr.location,
                ),
                body: methods,
                attrs: vec![],
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
            let transformed_type = if self.has_ref_type_attr(&field.attrs, ctx) {
                let base_type = match &field.ty {
                    UncheckedType::Basic(ident) => ident,
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                };
                let ref_type_name = format!("{}Ref", ctx.ident(base_type.id));
                UncheckedType::Basic(Identifier::new(ctx.intern(ref_type_name.as_str()), attr.location))
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("Map") => {
                        if params.len() != 3 {
                            panic!("Map must have exactly three generic parameters");
                        }
                        UncheckedType::Generic(Identifier::new(ctx.intern("MapRef"), attr.location), params.clone(), attr.location)
                    }
                    UncheckedType::Array(elem_ty, size, _) => UncheckedType::Generic(
                        Identifier::new(ctx.intern("ArrayRef"), attr.location),
                        vec![elem_ty.as_ref().clone(), UncheckedType::Const(*size, attr.location)],
                        attr.location,
                    ),
                    _ => UncheckedType::Generic(
                        Identifier::new(ctx.intern("StorageRef"), attr.location),
                        vec![field.ty.clone()],
                        attr.location,
                    ),
                }
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
                    .filter(|a| {
                        !a.is_derive()
                            || !a
                                .properties
                                .iter()
                                .any(|p| p.id == ctx.intern("StorageRef") || p.id == ctx.intern("Storage"))
                    })
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
                is_ty: false,
                location: attr.location,
            }))
        } else {
            ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::from(0), attr.location)))
        };
        let mut offset = initial_offset;
        let mut field_inits = IndexMap::new();

        for (field_name, field) in &struct_node.fields {
            let (inner_ty, size, target_type) = if self.has_ref_type_attr(&field.attrs, ctx) {
                let base_type = match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.strip_suffix("Ref").expect("generated ref type must end with Ref");
                        UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location))
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                };
                (base_type.clone(), ConstValue::Felt(1), field.ty.clone())
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 1 {
                            panic!("StorageRef must have exactly one generic parameter");
                        }
                        (params[0].clone(), ConstValue::Felt(1), field.ty.clone())
                    }
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("ArrayRef") => {
                        if params.len() != 2 {
                            panic!("ArrayRef must have exactly two generic parameters");
                        }
                        let size = match params[1] {
                            UncheckedType::Const(size, _) => size,
                            _ => {
                                panic!("Second generic parameter of ArrayRef must be a numeric const")
                            }
                        };
                        (params[0].clone(), size, field.ty.clone())
                    }
                    _ => (field.ty.clone(), ConstValue::Felt(1), field.ty.clone()),
                }
            };
            let target_path = PathNode {
                root: None,
                segments: vec![],
                target: target_type,
                is_ty: false,
                location: attr.location,
            };
            let new_path = PathNode {
                root: Some(UncheckedType::Path(Box::new(target_path.clone()))),
                segments: vec![],
                target: UncheckedType::Basic(Identifier::new(ctx.intern("new"), attr.location)),
                is_ty: false,
                location: attr.location,
            };
            let metadata_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
                root: None,
                segments: vec![],
                target: UncheckedType::Basic(metadata_ident),
                is_ty: false,
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

            let field_size = self.generate_struct_field_size(attr, field, ctx);
            offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: offset,
                operator: BinaryOperator::Add,
                rhs: field_size,
                location: attr.location,
            }));
        }

        let struct_path = ctx.alloc_expression(ExprNode::Path(PathNode::from_target_ty(UncheckedType::Basic(struct_node.name))));
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
        if is_ref_struct && !self.struct_has_map_ref_fields(struct_node, ctx) {
            methods.push(self.generate_struct_getter(struct_node, attr, ctx));
            methods.push(self.generate_struct_setter(struct_node, attr, ctx));
        }

        // Add per-field accessors for #[storage] structs or array fields
        let mut offset = ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::from(0), attr.location)));
        for (field_name, field) in &struct_node.fields {
            let (inner_ty, size) = if is_ref_struct && self.has_ref_type_attr(&field.attrs, ctx) {
                match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.strip_suffix("Ref").expect("generated ref type must end with Ref");
                        (
                            UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location)),
                            ConstValue::Felt(1),
                        )
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                }
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 1 {
                            panic!("StorageRef must have exactly one generic parameter");
                        }
                        (params[0].clone(), ConstValue::Felt(1))
                    }
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("ArrayRef") => {
                        if params.len() != 2 {
                            panic!("ArrayRef must have exactly two generic parameters");
                        }
                        let size = match params[1] {
                            UncheckedType::Const(size, _) => size,
                            _ => {
                                panic!("Second generic parameter of ArrayRef must be a numeric const")
                            }
                        };
                        (params[0].clone(), size)
                    }
                    _ => (field.ty.clone(), ConstValue::Felt(1)),
                }
            };

            // Generate per-field get/set for #[storage] structs
            if !is_ref_struct {
                methods.push(self.generate_getter(attr, &field_name.id, &inner_ty, offset, ctx));
                methods.push(self.generate_setter(attr, &field_name.id, &inner_ty, offset, ctx));
            }

            // A one-element array still has a valid index (zero).
            if !is_ref_struct && matches!(&field.ty, UncheckedType::Array(_, array_size, _) if array_size.as_u64().unwrap_or(0) > 0) {
                methods.push(self.generate_getter_at(attr, &field_name.id, &field.ty, offset, ctx));
                methods.push(self.generate_setter_at(attr, &field_name.id, &field.ty, offset, ctx));
            }

            // Advance by the complete field footprint. For ArrayRef<T, N>,
            // passing only `inner_ty` advances by one element and makes the
            // following field overlap the array.
            let field_size = if is_ref_struct {
                self.generate_struct_field_size(attr, field, ctx)
            } else {
                self.generate_field_size(attr, &field.ty, ctx)
            };
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
            attrs: vec![],
            comments: vec![],
            location: attr.location,
            is_generated: true,
        }
    }

    fn generate_ref_struct_eq_assign_impl<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        ref_struct: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> TraitImplNode {
        let ref_name = ctx.ident(ref_struct.name.id).to_string();
        let base_name = ref_name.strip_suffix("Ref").expect("generated ref type must end with Ref");
        let base_ident = Identifier::new(ctx.intern(base_name), attr.location);
        let base_ty = UncheckedType::Basic(base_ident);
        let rhs_ident = Identifier::new(ctx.intern("rhs"), attr.location);

        let self_target = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(Identifier::new(IdentId::SELF, attr.location)),
            is_ty: false,
            location: attr.location,
        }));
        let self_receiver = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(Identifier::new(IdentId::SELF, attr.location)),
            is_ty: false,
            location: attr.location,
        }));
        let rhs_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(rhs_ident),
            is_ty: false,
            location: attr.location,
        }));

        let set_ident = Identifier::new(ctx.intern("set"), attr.location);
        let callee = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
            target: self_target,
            field: set_ident,
            generic_parameters: Vec::new(),
            location: attr.location,
        }));
        let set_call = ctx.alloc_expression(ExprNode::MemberCall(MemberCallNode {
            callee,
            receiver: self_receiver,
            generic_parameters: vec![],
            args: vec![rhs_expr],
            location: attr.location,
        }));
        let set_stmt = ctx.alloc_statement(StmtNode::Expression(set_call));
        let body = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: vec![set_stmt],
            expr: None,
            expr_comments: vec![],
            location: attr.location,
        }));

        let eq_assign_fn = FunctionNode {
            name: Identifier::new(ctx.intern("eq_assign"), attr.location),
            parameters: vec![
                FunctionParameter::new(
                    Identifier::new(IdentId::SELF, attr.location),
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_SELF, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(rhs_ident, TypeQualifier::new(false, attr.location), base_ty.clone(), attr.location),
            ],
            generic_parameters: vec![],
            body: Some(body),
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

        TraitImplNode {
            associated_types: IndexMap::new(),
            generic_parameters: ref_struct.generic_parameters.clone(),
            trait_ty: UncheckedType::Generic(Identifier::new(ctx.intern("EqAssign"), attr.location), vec![base_ty], attr.location),
            ty: UncheckedType::Basic(ref_struct.name),
            body: vec![ctx.alloc_definition(DefinitionNode::Function(eq_assign_fn))],
            attrs: vec![],
            comments: vec![],
            location: attr.location,
            is_generated: true,
        }
    }

    fn generate_ref_struct_eq_impl<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        ref_struct: &StructNode,
        attr: &AttrNode,
        ctx: &mut V,
    ) -> TraitImplNode {
        let ref_name = ctx.ident(ref_struct.name.id).0.to_string();
        let base_name = ref_name.strip_suffix("Ref").expect("generated ref type must end with Ref").to_string();
        let base_ty = UncheckedType::Basic(Identifier::new(ctx.intern(base_name.as_str()), attr.location));

        let rhs_ident = Identifier::new(ctx.intern("rhs"), attr.location);
        let self_target = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(Identifier::new(IdentId::SELF, attr.location)),
            is_ty: false,
            location: attr.location,
        }));
        let self_receiver = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(Identifier::new(IdentId::SELF, attr.location)),
            is_ty: false,
            location: attr.location,
        }));
        let rhs_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(rhs_ident),
            is_ty: false,
            location: attr.location,
        }));

        let mut eq_expr: Option<ExprId> = None;
        for (field_name, _) in &ref_struct.fields {
            let self_field_target = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                target: self_target,
                field: *field_name,
                generic_parameters: Vec::new(),
                location: attr.location,
            }));
            let self_field_receiver = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                target: self_receiver,
                field: *field_name,
                generic_parameters: Vec::new(),
                location: attr.location,
            }));
            let rhs_field = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                target: rhs_expr,
                field: *field_name,
                generic_parameters: Vec::new(),
                location: attr.location,
            }));
            let eq_ident = Identifier::new(ctx.intern("eq"), attr.location);
            let callee = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                target: self_field_target,
                field: eq_ident,
                generic_parameters: Vec::new(),
                location: attr.location,
            }));
            let field_eq = ctx.alloc_expression(ExprNode::MemberCall(MemberCallNode {
                callee,
                receiver: self_field_receiver,
                generic_parameters: vec![],
                args: vec![rhs_field],
                location: attr.location,
            }));

            eq_expr = Some(match eq_expr {
                None => field_eq,
                Some(prev) => ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                    lhs: prev,
                    operator: BinaryOperator::And,
                    rhs: field_eq,
                    location: attr.location,
                })),
            });
        }

        let default_true_expr = ctx.alloc_expression(ExprNode::Value(ValueNode::Bool(F::from(1u32), attr.location)));
        let body_expr = eq_expr.unwrap_or(default_true_expr);
        let body = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: Vec::new(),
            expr: Some(body_expr),
            expr_comments: vec![],
            location: attr.location,
        }));

        let eq_fn = FunctionNode {
            name: Identifier::new(ctx.intern("eq"), attr.location),
            parameters: vec![
                FunctionParameter::new(
                    Identifier::new(IdentId::SELF, attr.location),
                    TypeQualifier::new(false, attr.location),
                    UncheckedType::Basic(Identifier::new(IdentId::TYPE_SELF, attr.location)),
                    attr.location,
                ),
                FunctionParameter::new(rhs_ident, TypeQualifier::new(false, attr.location), base_ty.clone(), attr.location),
            ],
            generic_parameters: vec![],
            body: Some(body),
            return_type: Some(UncheckedType::Basic(Identifier::new(IdentId::TYPE_BOOL, attr.location))),
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

        TraitImplNode {
            associated_types: IndexMap::new(),
            generic_parameters: ref_struct.generic_parameters.clone(),
            trait_ty: UncheckedType::Generic(Identifier::new(ctx.intern("Eq"), attr.location), vec![base_ty], attr.location),
            ty: UncheckedType::Basic(ref_struct.name),
            body: vec![ctx.alloc_definition(DefinitionNode::Function(eq_fn))],
            attrs: vec![],
            comments: vec![],
            location: attr.location,
            is_generated: true,
        }
    }

    fn supports_generated_eq_field<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        ty: &UncheckedType,
        ctx: &mut V,
    ) -> bool {
        match ty {
            UncheckedType::Basic(ident) => {
                let name = ctx.ident(ident.id).0.to_string();
                // Generated Ref types compare against their base type via generated Eq impl.
                name.ends_with("Ref")
            }
            UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") && params.len() == 1 => {
                matches!(
                    &params[0],
                    UncheckedType::Basic(inner)
                        if inner.id == IdentId::TYPE_FELT || inner.id == IdentId::TYPE_U32 || inner.id == IdentId::TYPE_BOOL
                )
            }
            UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("ArrayRef") && params.len() == 2 => {
                matches!(
                    &params[0],
                    UncheckedType::Basic(inner)
                        if inner.id == IdentId::TYPE_FELT || inner.id == IdentId::TYPE_U32
                )
            }
            _ => false,
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
                if params.len() != 1 {
                    panic!("StorageRef must have exactly one generic parameter");
                }
                params[0].clone()
            }
            UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("ArrayRef") => {
                if params.len() != 2 {
                    panic!("ArrayRef must have exactly two generic parameters");
                }
                let array_size = match &params[1] {
                    UncheckedType::Const(size, _) => *size,
                    _ => panic!("Second generic parameter of ArrayRef must be a numeric const"),
                };
                // Keep the array wrapper even for lengths zero and one. In
                // particular, ArrayRef<T, 0> occupies zero storage slots.
                UncheckedType::Array(Box::new(params[0].clone()), array_size, attr.location)
            }
            _ => field_type.clone(),
        };
        let variable = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: Some(base_type),
            segments: vec![],
            target: UncheckedType::Basic(size_ident),
            is_ty: false,
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

    fn generate_struct_field_size<
        F: Clone + From<u32>,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        attr: &AttrNode,
        field: &StructField,
        ctx: &mut V,
    ) -> ExprId {
        let field_type = if self.has_ref_type_attr(&field.attrs, ctx) {
            match &field.ty {
                UncheckedType::Basic(ident) => {
                    let type_name = ctx.ident(ident.id).0.to_string();
                    let base_name = type_name.strip_suffix("Ref").expect("#[ref] field type must use its generated Ref type");
                    UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location))
                }
                _ => panic!("#[ref] attribute only supported on basic struct types"),
            }
        } else {
            field.ty.clone()
        };
        self.generate_field_size(attr, &field_type, ctx)
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
        let mut fields = struct_node.fields.iter();
        let mut sum = match fields.next() {
            Some((_, first)) => self.generate_field_size(attr, &first.ty, ctx),
            // Empty storage structs occupy zero slots.
            None => ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::from(0), attr.location))),
        };
        for (_, field) in fields {
            let node = BinaryNode {
                lhs: sum,
                operator: BinaryOperator::Add,
                rhs: self.generate_field_size(attr, &field.ty, ctx),
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
            is_ty: false,
            location: attr.location,
        }));
        let csth_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(csth_ident),
            is_ty: false,
            location: attr.location,
        }));
        let user_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(user_id_ident),
            is_ty: false,
            location: attr.location,
        }));
        let contract_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(contract_id_ident),
            is_ty: false,
            location: attr.location,
        }));

        let mut field_reads = IndexMap::new();
        let mut offset = offset_expr;
        for (field_name, field) in &struct_node.fields {
            let inner_ty = match &field.ty {
                UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                    if params.len() != 1 {
                        panic!("StorageRef must have exactly one generic parameter");
                    }
                    params[0].clone()
                }
                UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("ArrayRef") => {
                    if params.len() != 2 {
                        panic!("ArrayRef must have exactly two generic parameters");
                    }
                    params[0].clone()
                }
                _ => field.ty.clone(),
            };
            let (_key, value) = self.generate_field_read(attr, &field_name.id, &inner_ty, offset, csth_expr, user_id_expr, contract_id_expr, ctx);
            field_reads.insert(field_name.clone(), value);
            // Keep this in sync with the getter: ArrayRef fields occupy
            // N * T::size(), not just T::size().
            let field_size = self.generate_field_size(attr, &field.ty, ctx);
            offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: offset,
                operator: BinaryOperator::Add,
                rhs: field_size,
                location: attr.location,
            }));
        }
        let name_path = ctx.alloc_expression(ExprNode::Path(PathNode::from_target_ty(UncheckedType::Basic(struct_node.name))));
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
            is_ty: false,
            location: attr.location,
        }));
        let value_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(value_ident),
            is_ty: false,
            location: attr.location,
        }));

        let mut field_writes = Vec::new();
        let mut offset = offset_expr;
        for (field_name, field) in &struct_node.fields {
            let inner_ty = match &field.ty {
                UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                    if params.len() != 1 {
                        panic!("StorageRef must have exactly one generic parameter");
                    }
                    params[0].clone()
                }
                UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("ArrayRef") => {
                    if params.len() != 2 {
                        panic!("ArrayRef must have exactly two generic parameters");
                    }
                    params[0].clone()
                }
                _ => field.ty.clone(),
            };
            let stmt_id = self.generate_field_write(attr, &field_name.id, &inner_ty, offset, ctx);
            field_writes.push(stmt_id);
            let field_size = self.generate_field_size(attr, &field.ty, ctx);
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
            is_ty: false,
            location: attr.location,
        }));

        let mut field_reads = IndexMap::new();
        let mut offset = ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::from(0), attr.location)));
        for (field_name, field) in &struct_node.fields {
            let (inner_ty, size) = if self.has_ref_type_attr(&field.attrs, ctx) {
                match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.strip_suffix("Ref").expect("generated ref type must end with Ref");
                        (
                            UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location)),
                            ConstValue::Felt(1),
                        )
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                }
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 1 {
                            panic!("StorageRef must have exactly one generic parameter");
                        }
                        (params[0].clone(), ConstValue::Felt(1))
                    }
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("ArrayRef") => {
                        if params.len() != 2 {
                            panic!("ArrayRef must have exactly two generic parameters");
                        }
                        let size = match params[1] {
                            UncheckedType::Const(size, _) => size,
                            _ => {
                                panic!("Second generic parameter of ArrayRef must be a numeric const")
                            }
                        };
                        (params[0].clone(), size)
                    }
                    _ => (field.ty.clone(), ConstValue::Felt(1)),
                }
            };

            let read_expr = if size.as_u64().unwrap_or(0) > 1 {
                let field_type = UncheckedType::Array(Box::new(inner_ty.clone()), size, attr.location);
                let read_ident = Identifier::new(ctx.intern("read"), attr.location);
                let read_path = PathNode {
                    root: Some(field_type),
                    segments: vec![],
                    target: UncheckedType::Basic(read_ident),
                    is_ty: false,
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
                    generic_parameters: Vec::new(),
                    location: attr.location,
                }));
                let metadata_expr = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: metadata_target_expr.clone(),
                    field: metadata_ident,
                    generic_parameters: Vec::new(),
                    location: attr.location,
                }));
                let csth_expr = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: metadata_expr.clone(),
                    field: contract_state_tree_height_ident,
                    generic_parameters: Vec::new(),
                    location: attr.location,
                }));
                let user_id_expr = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: metadata_expr.clone(),
                    field: user_id_ident,
                    generic_parameters: Vec::new(),
                    location: attr.location,
                }));
                let contract_id_expr = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: metadata_expr,
                    field: contract_id_ident,
                    generic_parameters: Vec::new(),
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
                    generic_parameters: Vec::new(),
                    location: attr.location,
                }));
                let get_ident = Identifier::new(ctx.intern("get"), attr.location);
                let get_callee = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: field_access.clone(),
                    field: get_ident,
                    generic_parameters: Vec::new(),
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

            let field_size = self.generate_struct_field_size(attr, field, ctx);
            offset = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
                lhs: offset,
                operator: BinaryOperator::Add,
                rhs: field_size,
                location: attr.location,
            }));
        }

        let base_struct_name = if ctx.ident(struct_node.name.id).to_string().ends_with("Ref") {
            let ref_name = ctx.ident(struct_node.name.id).to_string();
            let base_name = ref_name.strip_suffix("Ref").expect("generated ref type must end with Ref").to_string();
            Identifier::new(ctx.intern(base_name), attr.location)
        } else {
            struct_node.name
        };

        let struct_path = ctx.alloc_expression(ExprNode::Path(PathNode::from_target_ty(UncheckedType::Basic(base_struct_name))));
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
            is_ty: false,
            location: attr.location,
        }));
        let value_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(value_ident),
            is_ty: false,
            location: attr.location,
        }));

        let mut stmts = Vec::new();
        let mut offset = ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::from(0), attr.location)));
        for (field_name, field) in &struct_node.fields {
            let (inner_ty, size) = if self.has_ref_type_attr(&field.attrs, ctx) {
                match &field.ty {
                    UncheckedType::Basic(ident) => {
                        let type_name = ctx.ident(ident.id).0.to_string();
                        let base_name = type_name.strip_suffix("Ref").expect("generated ref type must end with Ref");
                        (
                            UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location)),
                            ConstValue::Felt(1),
                        )
                    }
                    _ => panic!("#[ref] attribute only supported on basic struct types"),
                }
            } else {
                match &field.ty {
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("StorageRef") => {
                        if params.len() != 1 {
                            panic!("StorageRef must have exactly one generic parameter");
                        }
                        (params[0].clone(), ConstValue::Felt(1))
                    }
                    UncheckedType::Generic(ident, params, _) if ident.id == ctx.intern("ArrayRef") => {
                        if params.len() != 2 {
                            panic!("ArrayRef must have exactly two generic parameters");
                        }
                        let size = match params[1] {
                            UncheckedType::Const(size, _) => size,
                            _ => {
                                panic!("Second generic parameter of ArrayRef must be a numeric const")
                            }
                        };
                        (params[0].clone(), size)
                    }
                    _ => (field.ty.clone(), ConstValue::Felt(1)),
                }
            };

            let value_field = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                target: value_expr.clone(),
                field: Identifier::new(field_name.id, attr.location),
                generic_parameters: Vec::new(),
                location: attr.location,
            }));

            let write_stmt = if size.as_u64().unwrap_or(0) > 1 {
                let field_type = UncheckedType::Array(Box::new(inner_ty.clone()), size, attr.location);
                let write_ident = Identifier::new(ctx.intern("write"), attr.location);
                let write_path = PathNode {
                    root: Some(field_type),
                    segments: vec![],
                    target: UncheckedType::Basic(write_ident),
                    is_ty: false,
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
                    generic_parameters: Vec::new(),
                    location: attr.location,
                }));
                let set_ident = Identifier::new(ctx.intern("set"), attr.location);
                let set_callee = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
                    target: field_access.clone(),
                    field: set_ident,
                    generic_parameters: Vec::new(),
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

            let field_size = self.generate_struct_field_size(attr, field, ctx);
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
            let ref_name = ctx.ident(struct_node.name.id).to_string();
            let base_name = ref_name.strip_suffix("Ref").expect("generated ref type must end with Ref").to_string();
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
            is_ty: false,
            location: attr.location,
        }));
        let user_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(user_id_ident),
            is_ty: false,
            location: attr.location,
        }));
        let contract_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(contract_id_ident),
            is_ty: false,
            location: attr.location,
        }));

        let read_ident = Identifier::new(ctx.intern("read"), attr.location);
        let read_path = PathNode {
            root: Some(field_type.clone()),
            segments: vec![],
            target: UncheckedType::Basic(read_ident),
            is_ty: false,
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
            is_ty: false,
            location: attr.location,
        }));

        let write_ident = Identifier::new(ctx.intern("write"), attr.location);
        let write_path = PathNode {
            root: Some(field_type.clone()),
            segments: vec![],
            target: UncheckedType::Basic(write_ident),
            is_ty: false,
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
            is_ty: false,
            location: attr.location,
        }));
        let csth_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(csth_ident),
            is_ty: false,
            location: attr.location,
        }));
        let user_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(user_id_ident),
            is_ty: false,
            location: attr.location,
        }));
        let contract_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(contract_id_ident),
            is_ty: false,
            location: attr.location,
        }));

        let elem_ty = if let UncheckedType::Generic(ident, params, _) = field_type {
            if ident.id == ctx.intern("StorageRef") && params.len() == 1 {
                params[0].clone()
            } else if ident.id == ctx.intern("ArrayRef") && params.len() == 2 {
                params[0].clone()
            } else {
                panic!("Expected StorageRef<T> or ArrayRef<T, N>");
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
            is_ty: false,
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
            is_ty: false,
            location: attr.location,
        }));

        let elem_ty = if let UncheckedType::Generic(ident, params, _) = field_type {
            if ident.id == ctx.intern("StorageRef") && params.len() == 1 {
                params[0].clone()
            } else if ident.id == ctx.intern("ArrayRef") && params.len() == 2 {
                params[0].clone()
            } else {
                panic!("Expected StorageRef<T> or ArrayRef<T, N>");
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
            is_ty: false,
            location: attr.location,
        }));

        let write_ident = Identifier::new(ctx.intern("write"), attr.location);
        let write_path = PathNode {
            root: Some(elem_ty.clone()),
            segments: vec![],
            target: UncheckedType::Basic(write_ident),
            is_ty: false,
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
        F: Clone + From<u32> + ContextFelt,
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
            is_ty: false,
            location: attr.location,
        }));
        let index_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(index_ident),
            is_ty: false,
            location: attr.location,
        }));
        let csth_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(csth_ident),
            is_ty: false,
            location: attr.location,
        }));
        let user_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(user_id_ident),
            is_ty: false,
            location: attr.location,
        }));
        let contract_id_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(contract_id_ident),
            is_ty: false,
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
            is_ty: false,
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
        F: Clone + From<u32> + ContextFelt,
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
            is_ty: false,
            location: attr.location,
        }));
        let index_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(index_ident),
            is_ty: false,
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
            is_ty: false,
            location: attr.location,
        }));

        let write_ident = Identifier::new(ctx.intern("write"), attr.location);
        let variable = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: Some(elem_ty.clone()),
            segments: vec![],
            target: UncheckedType::Basic(write_ident),
            is_ty: false,
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
        F: Clone + From<u32> + ContextFelt,
        C,
        V: VisitorContext<F, C, Expr = ExprNode<F>, Stmt = StmtNode, Definition = DefinitionNode>,
    >(
        &self,
        attr: &AttrNode,
        index_ident: Identifier,
        bound: ConstValue,
        ctx: &mut V,
    ) -> StmtId {
        let index_expr = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(index_ident),
            is_ty: false,
            location: attr.location,
        }));
        let bound_expr = ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(F::cns(bound.as_u64().unwrap_or(0)), attr.location)));
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
            is_ty: false,
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
            is_ty: false,
            location: attr.location,
        }));
        let value = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target: UncheckedType::Basic(value_ident),
            is_ty: false,
            location: attr.location,
        }));
        let field = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
            target: value,
            field: Identifier::new(*field_name, attr.location),
            generic_parameters: Vec::new(),
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

impl<'a, F: Clone + From<u32> + ContextFelt + 'static, C> AstVisitor<F, C> for StorageProcessor<'a> {
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

        let event_trait_id = ctx.intern("Event");
        let has_contract_attr = s.attrs.iter().any(|a| a.name == contract_attribute_id);
        let has_storage_derive = s
            .attrs
            .iter()
            .any(|a| a.is_derive() && a.properties.iter().any(|p| p.id == storage_trait_id));

        if has_contract_attr && has_storage_derive {
            let total_maps = self.count_maps_in_struct(&s, ctx);
            if total_maps > 1 {
                return Err(psy_common::Error::Message(format!(
                    "Only one Map is currently supported per contract. Found {} map fields in contract '{}'",
                    total_maps,
                    ctx.ident(s.name.id)
                )));
            }
        }

        let mut generated_storage_ref = false;
        for attr in &s.attrs {
            if attr.is_derive() && attr.properties.iter().any(|p| p.id == event_trait_id) {
                let impl_node = self.generate_event_impl(&s, attr, ctx);
                ctx.insert_definition(DefinitionNode::TraitImpl(impl_node), InsertPosition::End);
            }

            if attr.name == storage_attribute_id {
                let impl_node = self.generate_accessor_impl(&s, attr, ctx, false);
                ctx.insert_definition(DefinitionNode::Impl(impl_node), InsertPosition::End);
            }

            if !generated_storage_ref
                && attr.is_derive()
                && attr
                    .properties
                    .iter()
                    .any(|p| p.id == storage_ref_attribute_id || p.id == storage_trait_id)
            {
                let ref_struct = self.transform_struct_to_storage_ref(&s, attr, ctx, Some("Ref"));
                ctx.insert_definition(DefinitionNode::Struct(ref_struct.clone()), InsertPosition::End);

                let include_offset = !s.attrs.iter().any(|a| a.name == contract_attribute_id);
                let mut methods = Vec::new();
                let new_method = self.generate_new_method(&ref_struct, attr, ctx, include_offset);
                methods.push(new_method);
                let impl_node = ImplNode {
                    associated_types: IndexMap::new(),
                    generic_parameters: ref_struct.generic_parameters.clone(),
                    ty: UncheckedType::Basic(ref_struct.name),
                    body: methods,
                    attrs: vec![],
                    comments: vec![],
                    location: attr.location,
                    is_generated: true,
                };
                ctx.insert_definition(DefinitionNode::Impl(impl_node), InsertPosition::End);

                let storage_new_impl = TraitImplNode {
                    associated_types: IndexMap::new(),
                    generic_parameters: ref_struct.generic_parameters.clone(),
                    trait_ty: UncheckedType::Basic(Identifier::new(ctx.intern("StorageNew"), attr.location)),
                    ty: UncheckedType::Basic(ref_struct.name),
                    body: vec![self.generate_new_method(&ref_struct, attr, ctx, include_offset)],
                    attrs: vec![],
                    comments: vec![],
                    location: attr.location,
                    is_generated: true,
                };
                ctx.insert_definition(DefinitionNode::TraitImpl(storage_new_impl), InsertPosition::End);

                let accessor_impl = self.generate_accessor_impl(&ref_struct, attr, ctx, true);
                ctx.insert_definition(DefinitionNode::Impl(accessor_impl), InsertPosition::End);
                if !self.struct_has_map_ref_fields(&ref_struct, ctx) {
                    let eq_assign_impl = self.generate_ref_struct_eq_assign_impl(&ref_struct, attr, ctx);
                    ctx.insert_definition(DefinitionNode::TraitImpl(eq_assign_impl), InsertPosition::End);
                    if ref_struct
                        .fields
                        .iter()
                        .all(|(_, field)| self.supports_generated_eq_field(&field.ty, ctx))
                    {
                        let eq_impl = self.generate_ref_struct_eq_impl(&ref_struct, attr, ctx);
                        ctx.insert_definition(DefinitionNode::TraitImpl(eq_impl), InsertPosition::End);
                    }
                }

                for (_, field) in &ref_struct.fields {
                    if let Some(impl_node) = self.generate_storage_at_impl(&field.ty, attr, ctx) {
                        ctx.insert_definition(DefinitionNode::TraitImpl(impl_node), InsertPosition::End);
                    }
                }

                generated_storage_ref = true;
            }

            if attr.is_derive() && attr.properties.iter().any(|p| p.id == storage_trait_id) {
                let impl_node = self.generate_storage_impl(&s, attr, ctx);
                ctx.insert_definition(DefinitionNode::TraitImpl(impl_node), InsertPosition::End);
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

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use psy_ast::*;
    use psy_vm::dpn::ops::sym_felt::SymFeltRef;

    use super::*;

    type TestCtx<'a> = DefaultVisitorContext<'a, SymFeltRef, ()>;

    fn loc() -> Location {
        Location::default()
    }

    fn idn(ctx: &mut TestCtx, name: &str) -> Identifier {
        Identifier::new(ctx.intern(name), loc())
    }

    fn attr(ctx: &mut TestCtx, name: &str, properties: &[&str]) -> AttrNode {
        let attr_name = idn(ctx, name);
        let props = properties.iter().map(|p| idn(ctx, p)).collect();
        AttrNode {
            path: vec![],
            name: attr_name,
            properties: props,
            location: loc(),
        }
    }

    fn derive_attr(ctx: &mut TestCtx, property: &str) -> AttrNode {
        attr(ctx, "derive", &[property])
    }

    fn ref_attr(ctx: &mut TestCtx) -> AttrNode {
        attr(ctx, "ref", &[])
    }

    // Type constructors. Each takes only names/sizes so call sites never nest
    // `&mut ctx` borrows inside one expression.
    fn ty_basic(ctx: &mut TestCtx, name: &str) -> UncheckedType {
        UncheckedType::Basic(idn(ctx, name))
    }

    fn ty_const(ctx: &mut TestCtx, value: u32) -> UncheckedType {
        UncheckedType::Const(ConstValue::U32(value), loc())
    }

    fn ty_generic(ctx: &mut TestCtx, name: &str, params: Vec<UncheckedType>) -> UncheckedType {
        let ident = idn(ctx, name);
        UncheckedType::Generic(ident, params, loc())
    }

    fn ty_map(ctx: &mut TestCtx) -> UncheckedType {
        let felt = ty_basic(ctx, "Felt");
        let size = ty_const(ctx, 4);
        ty_generic(ctx, "Map", vec![felt.clone(), felt, size])
    }

    fn ty_map_ref(ctx: &mut TestCtx) -> UncheckedType {
        let felt = ty_basic(ctx, "Felt");
        let size = ty_const(ctx, 4);
        ty_generic(ctx, "MapRef", vec![felt.clone(), felt, size])
    }

    fn ty_storage_ref(ctx: &mut TestCtx, inner: &str) -> UncheckedType {
        let param = ty_basic(ctx, inner);
        ty_generic(ctx, "StorageRef", vec![param])
    }

    fn ty_array(ctx: &mut TestCtx, elem: &str, size: u32) -> UncheckedType {
        let elem_ty = ty_basic(ctx, elem);
        UncheckedType::Array(Box::new(elem_ty), ConstValue::U32(size), loc())
    }

    fn ty_array_ref(ctx: &mut TestCtx, elem: &str, size: u32) -> UncheckedType {
        let elem_ty = ty_basic(ctx, elem);
        let size_ty = ty_const(ctx, size);
        ty_generic(ctx, "ArrayRef", vec![elem_ty, size_ty])
    }

    fn field_of(ctx: &mut TestCtx, ty: UncheckedType, attrs: Vec<AttrNode>) -> StructField {
        StructField {
            ty,
            attrs,
            visibility: Visibility::Public,
            comments: vec![],
            location: loc(),
        }
    }

    /// Build a struct whose fields all take plain (attr-free) types.
    fn strukt(ctx: &mut TestCtx, name: &str, fields: Vec<(&str, UncheckedType)>, attrs: Vec<AttrNode>) -> StructNode {
        let struct_name = idn(ctx, name);
        let mut field_map = IndexMap::new();
        for (field_name, ty) in fields {
            let ident = idn(ctx, field_name);
            let field = field_of(ctx, ty, vec![]);
            field_map.insert(ident, field);
        }
        StructNode {
            name: struct_name,
            generic_parameters: vec![],
            fields: field_map,
            attrs,
            visibility: Visibility::Public,
            comments: vec![],
            location: loc(),
            is_generated: false,
        }
    }

    /// Build a struct whose first listed field carries a `#[ref]` annotation.
    fn strukt_with_ref_field(
        ctx: &mut TestCtx,
        name: &str,
        ref_field: (&str, UncheckedType),
        fields: Vec<(&str, UncheckedType)>,
        attrs: Vec<AttrNode>,
    ) -> StructNode {
        let struct_name = idn(ctx, name);
        let ref_ident = idn(ctx, ref_field.0);
        let annotation = ref_attr(ctx);
        let mut field_map = IndexMap::new();
        field_map.insert(ref_ident, field_of(ctx, ref_field.1, vec![annotation]));
        for (field_name, ty) in fields {
            let ident = idn(ctx, field_name);
            let field = field_of(ctx, ty, vec![]);
            field_map.insert(ident, field);
        }
        StructNode {
            name: struct_name,
            generic_parameters: vec![],
            fields: field_map,
            attrs,
            visibility: Visibility::Public,
            comments: vec![],
            location: loc(),
            is_generated: false,
        }
    }

    fn felt_zero(ctx: &mut TestCtx) -> ExprId {
        ctx.alloc_expression(ExprNode::Value(ValueNode::Felt(SymFeltRef(0), loc())))
    }

    fn panics_with(message: &str, f: impl FnOnce(&mut TestCtx)) {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut ctx)));
        let error = result.unwrap_err();
        let text = error
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("<non-string panic>");
        assert!(text.contains(message), "panic {text:?} did not contain {message:?}");
    }

    #[test]
    fn map_counting_walks_generics_arrays_structs_and_cycles() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();

        let inner_felt = ty_basic(&mut ctx, "Felt");
        let inner = strukt(&mut ctx, "Inner", vec![("x", inner_felt)], vec![]);
        let map = ty_map(&mut ctx);
        let nested_map = ty_generic(&mut ctx, "StorageRef", vec![map.clone()]);
        let inner2 = ty_basic(&mut ctx, "Inner");
        let plain_felt = ty_basic(&mut ctx, "Felt");
        let holder = strukt(
            &mut ctx,
            "Holder",
            vec![
                ("m", map),
                ("g", nested_map),
                ("arr", UncheckedType::Array(Box::new(inner2), ConstValue::U32(2), loc())),
                ("plain", plain_felt),
            ],
            vec![],
        );
        ctx.alloc_definition(DefinitionNode::Struct(inner));
        ctx.alloc_definition(DefinitionNode::Struct(holder.clone()));

        // m -> 1, Map nested inside a non-Map generic -> 1, array of structs -> 0.
        assert_eq!(processor.count_maps_in_struct(&holder, &mut ctx), 2);

        // A struct cycle terminates through the visiting guard.
        let cyc_b = ty_basic(&mut ctx, "CycB");
        let cyc_a = strukt(&mut ctx, "CycA", vec![("b", cyc_b)], vec![]);
        let cyc_a_ty = ty_basic(&mut ctx, "CycA");
        let cyc_b_node = strukt(&mut ctx, "CycB", vec![("a", cyc_a_ty)], vec![]);
        ctx.alloc_definition(DefinitionNode::Struct(cyc_a.clone()));
        ctx.alloc_definition(DefinitionNode::Struct(cyc_b_node));
        assert_eq!(processor.count_maps_in_struct(&cyc_a, &mut ctx), 0);
    }

    #[test]
    fn ref_helpers_classify_field_types() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();

        let map_ref = ty_map_ref(&mut ctx);
        assert!(processor.is_map_ref_type(&map_ref, &mut ctx));
        let one_param = ty_basic(&mut ctx, "Felt");
        assert!(!processor.is_map_ref_type(&ty_generic(&mut ctx, "MapRef", vec![one_param]), &mut ctx));
        assert!(!processor.is_map_ref_type(&ty_basic(&mut ctx, "MapRef"), &mut ctx));

        let felt = ty_basic(&mut ctx, "Felt");
        let with_map = strukt(&mut ctx, "WithMap", vec![("m", map_ref)], vec![]);
        let plain = strukt(&mut ctx, "Plain", vec![("p", felt)], vec![]);
        assert!(processor.struct_has_map_ref_fields(&with_map, &mut ctx));
        assert!(!processor.struct_has_map_ref_fields(&plain, &mut ctx));

        let inner_ty = ty_basic(&mut ctx, "Inner");
        let annotation = ref_attr(&mut ctx);
        let ref_field = field_of(&mut ctx, inner_ty, vec![annotation]);
        assert!(processor.has_ref_type_attr(&ref_field.attrs, &mut ctx));
        assert!(!processor.has_ref_type_attr(&plain.fields[0].attrs, &mut ctx));

        ctx.alloc_definition(DefinitionNode::Struct(plain.clone()));
        assert_eq!(
            processor.find_struct_definition(plain.name.id, &mut ctx).map(|s| s.name.id),
            Some(plain.name.id)
        );
        let missing = idn(&mut ctx, "Missing").id;
        assert!(processor.find_struct_definition(missing, &mut ctx).is_none());
    }

    #[test]
    fn transform_struct_to_storage_ref_maps_every_field_shape() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();

        let derive = derive_attr(&mut ctx, "Storage");
        let contract = attr(&mut ctx, "contract", &[]);
        let inner_ty = ty_basic(&mut ctx, "Inner");
        let map = ty_map(&mut ctx);
        let grid = ty_array(&mut ctx, "Felt", 2);
        let note = ty_basic(&mut ctx, "Felt");
        let source = strukt_with_ref_field(
            &mut ctx,
            "Wallet",
            ("inner", inner_ty),
            vec![("balances", map), ("grid", grid), ("note", note)],
            vec![derive.clone(), contract],
        );
        let ref_struct = processor.transform_struct_to_storage_ref(&source, &derive, &mut ctx, Some("Ref"));
        assert_eq!(ctx.ident(ref_struct.name.id).0, "WalletRef");
        // The Storage derive is dropped on the generated Ref struct.
        assert!(!ref_struct.attrs.iter().any(|a| a.is_derive()));

        let name_of = |ctx: &mut TestCtx, ty: &UncheckedType| -> String {
            match ty {
                UncheckedType::Basic(ident) => ctx.ident(ident.id).0.to_string(),
                UncheckedType::Generic(ident, _, _) => ctx.ident(ident.id).0.to_string(),
                other => format!("{other:?}"),
            }
        };
        let inner_field = ref_struct.fields[&idn(&mut ctx, "inner")].ty.clone();
        assert_eq!(name_of(&mut ctx, &inner_field), "InnerRef");
        let balances = ref_struct.fields[&idn(&mut ctx, "balances")].ty.clone();
        assert_eq!(name_of(&mut ctx, &balances), "MapRef");
        let grid_field = ref_struct.fields[&idn(&mut ctx, "grid")].ty.clone();
        assert_eq!(name_of(&mut ctx, &grid_field), "ArrayRef");
        let note_field = ref_struct.fields[&idn(&mut ctx, "note")].ty.clone();
        assert_eq!(name_of(&mut ctx, &note_field), "StorageRef");

        // Without a suffix the name and attrs are preserved.
        let untouched = processor.transform_struct_to_storage_ref(&source, &derive, &mut ctx, None);
        assert_eq!(untouched.name.id, source.name.id);
        assert_eq!(untouched.attrs.len(), source.attrs.len());
    }

    #[test]
    fn transform_rejects_malformed_ref_annotations() {
        let processor = StorageProcessor::new();
        panics_with("only supported on basic struct types", |ctx| {
            let grid = ty_array(ctx, "Felt", 1);
            let node = strukt_with_ref_field(ctx, "Bad", ("x", grid), vec![], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.transform_struct_to_storage_ref(&node, &derive, ctx, Some("Ref"));
        });
        panics_with("exactly three generic parameters", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let bad_map = ty_generic(ctx, "Map", vec![felt]);
            let node = strukt(ctx, "Bad", vec![("x", bad_map)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.transform_struct_to_storage_ref(&node, &derive, ctx, Some("Ref"));
        });
    }

    #[test]
    fn storage_at_impl_synthesizes_read_and_write_only_for_arrays() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();
        let derive = derive_attr(&mut ctx, "Storage");

        let array = ty_array(&mut ctx, "Felt", 4);
        let impl_node = processor
            .generate_storage_at_impl(&array, &derive, &mut ctx)
            .expect("array fields must generate a StorageAt impl");
        assert_eq!(impl_node.body.len(), 2);
        match &impl_node.ty {
            UncheckedType::Generic(ident, params, _) => {
                assert_eq!(ctx.ident(ident.id).0, "ArrayRef");
                assert_eq!(params.len(), 2);
            }
            other => panic!("impl type should be ArrayRef<...>, got {other:?}"),
        }

        let felt = ty_basic(&mut ctx, "Felt");
        assert!(processor.generate_storage_at_impl(&felt, &derive, &mut ctx).is_none());
        let map = ty_map(&mut ctx);
        assert!(processor.generate_storage_at_impl(&map, &derive, &mut ctx).is_none());
    }

    #[test]
    fn new_method_handles_every_ref_field_kind() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();
        let derive = derive_attr(&mut ctx, "Storage");

        let inner_ty = ty_basic(&mut ctx, "Inner");
        let map = ty_map(&mut ctx);
        let grid = ty_array(&mut ctx, "Felt", 2);
        let note = ty_basic(&mut ctx, "Felt");
        let source = strukt_with_ref_field(
            &mut ctx,
            "Wallet",
            ("inner", inner_ty),
            vec![("balances", map), ("grid", grid), ("note", note)],
            vec![derive.clone()],
        );
        let ref_struct = processor.transform_struct_to_storage_ref(&source, &derive, &mut ctx, Some("Ref"));

        for include_offset in [true, false] {
            let def_id = processor.generate_new_method(&ref_struct, &derive, &mut ctx, include_offset);
            match ctx.definition(def_id) {
                DefinitionNode::Function(node) => {
                    assert_eq!(ctx.ident(node.name.id).0, "new");
                    assert_eq!(node.parameters.len() + usize::from(!include_offset), 2);
                }
                other => panic!("new method should be a function definition, got {other:?}"),
            }
        }
    }

    #[test]
    fn new_method_rejects_malformed_ref_types() {
        let processor = StorageProcessor::new();
        panics_with("exactly one generic parameter", |ctx| {
            let felt_a = ty_basic(ctx, "Felt");
            let felt_b = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "StorageRef", vec![felt_a, felt_b]);
            let node = strukt(ctx, "Bad", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_new_method(&node, &derive, ctx, true);
        });
        panics_with("exactly two generic parameters", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "ArrayRef", vec![felt]);
            let node = strukt(ctx, "Bad", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_new_method(&node, &derive, ctx, true);
        });
        panics_with("numeric const", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let n = ty_basic(ctx, "N");
            let bad = ty_generic(ctx, "ArrayRef", vec![felt, n]);
            let node = strukt(ctx, "Bad", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_new_method(&node, &derive, ctx, true);
        });
        panics_with("only supported on basic struct types", |ctx| {
            let grid = ty_array(ctx, "Felt", 1);
            let node = strukt_with_ref_field(ctx, "Bad", ("x", grid), vec![], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_new_method(&node, &derive, ctx, true);
        });
        panics_with("must end with Ref", |ctx| {
            let inner = ty_basic(ctx, "Inner");
            let node = strukt_with_ref_field(ctx, "Bad", ("x", inner), vec![], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_new_method(&node, &derive, ctx, true);
        });
    }

    #[test]
    fn accessor_impl_generates_for_ref_and_plain_structs() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();
        let derive = derive_attr(&mut ctx, "Storage");

        // A Ref struct without Map fields gets whole-struct get/set plus the
        // ref-aware per-field bookkeeping.
        let inner_ref = ty_basic(&mut ctx, "InnerRef");
        let grid_ref = ty_array_ref(&mut ctx, "Felt", 2);
        let storage_ref = ty_storage_ref(&mut ctx, "Felt");
        let ref_source = strukt_with_ref_field(
            &mut ctx,
            "WalletRef",
            ("inner", inner_ref),
            vec![("grid", grid_ref), ("note", storage_ref)],
            vec![derive.clone()],
        );
        let ref_impl = processor.generate_accessor_impl(&ref_source, &derive, &mut ctx, true);
        assert!(ref_impl.body.len() >= 2);

        // A plain #[storage] struct with an array field gets per-field get/set
        // and indexed get_at/set_at accessors.
        let plain_grid = ty_array(&mut ctx, "Felt", 2);
        let plain_note = ty_basic(&mut ctx, "Felt");
        let plain = strukt(&mut ctx, "Ledger", vec![("grid", plain_grid), ("note", plain_note)], vec![derive.clone()]);
        let plain_impl = processor.generate_accessor_impl(&plain, &derive, &mut ctx, false);
        assert!(plain_impl.body.len() >= 4);

        // A Ref struct that contains a MapRef field skips whole-struct get/set.
        let map_ref = ty_map_ref(&mut ctx);
        let with_map = strukt(&mut ctx, "BankRef", vec![("m", map_ref)], vec![derive.clone()]);
        let map_impl = processor.generate_accessor_impl(&with_map, &derive, &mut ctx, true);
        assert!(map_impl.body.is_empty());
    }

    #[test]
    fn accessor_impl_rejects_malformed_ref_types() {
        let processor = StorageProcessor::new();
        panics_with("exactly one generic parameter", |ctx| {
            let felt_a = ty_basic(ctx, "Felt");
            let felt_b = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "StorageRef", vec![felt_a, felt_b]);
            let node = strukt(ctx, "Bad", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_accessor_impl(&node, &derive, ctx, true);
        });
        panics_with("exactly two generic parameters", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "ArrayRef", vec![felt]);
            let node = strukt(ctx, "Bad", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_accessor_impl(&node, &derive, ctx, true);
        });
        panics_with("numeric const", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let n = ty_basic(ctx, "N");
            let bad = ty_generic(ctx, "ArrayRef", vec![felt, n]);
            let node = strukt(ctx, "Bad", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_accessor_impl(&node, &derive, ctx, true);
        });
        panics_with("only supported on basic struct types", |ctx| {
            let grid = ty_array(ctx, "Felt", 1);
            let node = strukt_with_ref_field(ctx, "Bad", ("x", grid), vec![], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_accessor_impl(&node, &derive, ctx, true);
        });
    }

    #[test]
    fn eq_support_covers_primitives_ref_names_and_arrays() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();

        let named_ref = ty_basic(&mut ctx, "InnerRef");
        assert!(processor.supports_generated_eq_field(&named_ref, &mut ctx));
        let felt = ty_basic(&mut ctx, "Felt");
        assert!(!processor.supports_generated_eq_field(&felt, &mut ctx));
        let storage_ref_felt = ty_storage_ref(&mut ctx, "Felt");
        assert!(processor.supports_generated_eq_field(&storage_ref_felt, &mut ctx));
        let storage_ref_struct = ty_storage_ref(&mut ctx, "Inner");
        assert!(!processor.supports_generated_eq_field(&storage_ref_struct, &mut ctx));
        let array_ref_felt = ty_array_ref(&mut ctx, "Felt", 2);
        assert!(processor.supports_generated_eq_field(&array_ref_felt, &mut ctx));
        let array_ref_bool = ty_array_ref(&mut ctx, "bool", 2);
        assert!(!processor.supports_generated_eq_field(&array_ref_bool, &mut ctx));
        let map = ty_map(&mut ctx);
        assert!(!processor.supports_generated_eq_field(&map, &mut ctx));
        let tuple = UncheckedType::Tuple(vec![], loc());
        assert!(!processor.supports_generated_eq_field(&tuple, &mut ctx));
    }

    #[test]
    fn field_size_generators_cover_all_type_shapes() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();
        let derive = derive_attr(&mut ctx, "Storage");

        let felt = ty_basic(&mut ctx, "Felt");
        let storage_ref = ty_storage_ref(&mut ctx, "Felt");
        let array_ref = ty_array_ref(&mut ctx, "Felt", 3);
        for ty in [felt, storage_ref, array_ref] {
            let size = processor.generate_field_size(&derive, &ty, &mut ctx);
            assert!(matches!(ctx.expression(size), ExprNode::Call(_)));
        }

        let inner_ref = ty_basic(&mut ctx, "InnerRef");
        let annotation = ref_attr(&mut ctx);
        let ref_field = field_of(&mut ctx, inner_ref, vec![annotation]);
        let sized = processor.generate_struct_field_size(&derive, &ref_field, &mut ctx);
        assert!(matches!(ctx.expression(sized), ExprNode::Call(_)));
        let plain_felt = ty_basic(&mut ctx, "Felt");
        let plain_field = field_of(&mut ctx, plain_felt, vec![]);
        let sized_plain = processor.generate_struct_field_size(&derive, &plain_field, &mut ctx);
        assert!(matches!(ctx.expression(sized_plain), ExprNode::Call(_)));
    }

    #[test]
    fn field_size_generators_reject_malformed_ref_types() {
        let processor = StorageProcessor::new();
        panics_with("exactly one generic parameter", |ctx| {
            let felt_a = ty_basic(ctx, "Felt");
            let felt_b = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "StorageRef", vec![felt_a, felt_b]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_field_size(&derive, &bad, ctx);
        });
        panics_with("exactly two generic parameters", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "ArrayRef", vec![felt]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_field_size(&derive, &bad, ctx);
        });
        panics_with("numeric const", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let n = ty_basic(ctx, "N");
            let bad = ty_generic(ctx, "ArrayRef", vec![felt, n]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_field_size(&derive, &bad, ctx);
        });
        panics_with("only supported on basic struct types", |ctx| {
            let grid = ty_array(ctx, "Felt", 1);
            let annotation = ref_attr(ctx);
            let ref_field = field_of(ctx, grid, vec![annotation]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_struct_field_size(&derive, &ref_field, ctx);
        });
        panics_with("must use its generated Ref type", |ctx| {
            let inner = ty_basic(ctx, "Inner");
            let annotation = ref_attr(ctx);
            let ref_field = field_of(ctx, inner, vec![annotation]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_struct_field_size(&derive, &ref_field, ctx);
        });
    }

    #[test]
    fn struct_getter_and_setter_strip_ref_names_and_size_fields() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();
        let derive = derive_attr(&mut ctx, "Storage");

        let inner_ref = ty_basic(&mut ctx, "InnerRef");
        let bare_ref = ty_basic(&mut ctx, "FooRef");
        let grid_ref = ty_array_ref(&mut ctx, "Felt", 2);
        let storage_ref = ty_storage_ref(&mut ctx, "Felt");
        let ref_struct = strukt_with_ref_field(
            &mut ctx,
            "WalletRef",
            ("inner", inner_ref),
            vec![("bare", bare_ref), ("grid", grid_ref), ("note", storage_ref)],
            vec![derive.clone()],
        );
        let getter = processor.generate_struct_getter(&ref_struct, &derive, &mut ctx);
        let setter = processor.generate_struct_setter(&ref_struct, &derive, &mut ctx);
        assert!(matches!(ctx.definition(getter), DefinitionNode::Function(_)));
        assert!(matches!(ctx.definition(setter), DefinitionNode::Function(_)));

        // Structs whose name does not end in Ref keep it as the base name.
        let plain_ref = ty_storage_ref(&mut ctx, "Felt");
        let named = strukt(&mut ctx, "Vault", vec![("note", plain_ref)], vec![derive.clone()]);
        let named_getter = processor.generate_struct_getter(&named, &derive, &mut ctx);
        let named_setter = processor.generate_struct_setter(&named, &derive, &mut ctx);
        assert!(matches!(ctx.definition(named_getter), DefinitionNode::Function(_)));
        assert!(matches!(ctx.definition(named_setter), DefinitionNode::Function(_)));
    }

    #[test]
    fn struct_getter_and_setter_reject_malformed_ref_types() {
        let processor = StorageProcessor::new();
        panics_with("exactly one generic parameter", |ctx| {
            let felt_a = ty_basic(ctx, "Felt");
            let felt_b = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "StorageRef", vec![felt_a, felt_b]);
            let node = strukt(ctx, "BadRef", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_struct_getter(&node, &derive, ctx);
        });
        panics_with("exactly two generic parameters", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "ArrayRef", vec![felt]);
            let node = strukt(ctx, "BadRef", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_struct_setter(&node, &derive, ctx);
        });
        panics_with("numeric const", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let n = ty_basic(ctx, "N");
            let bad = ty_generic(ctx, "ArrayRef", vec![felt, n]);
            let node = strukt(ctx, "BadRef", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_struct_getter(&node, &derive, ctx);
        });
        panics_with("only supported on basic struct types", |ctx| {
            let grid = ty_array(ctx, "Felt", 1);
            let node = strukt_with_ref_field(ctx, "BadRef", ("x", grid), vec![], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_struct_setter(&node, &derive, ctx);
        });
        panics_with("must end with Ref", |ctx| {
            let inner = ty_basic(ctx, "Inner");
            let node = strukt_with_ref_field(ctx, "BadRef", ("x", inner), vec![], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_struct_getter(&node, &derive, ctx);
        });
    }

    #[test]
    fn getter_at_and_setter_at_accept_refs_and_plain_arrays() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();
        let derive = derive_attr(&mut ctx, "Storage");
        let field_ident = idn(&mut ctx, "grid");
        let offset = felt_zero(&mut ctx);

        let shapes = vec![
            ty_storage_ref(&mut ctx, "Felt"),
            ty_array_ref(&mut ctx, "Felt", 2),
            ty_array(&mut ctx, "Felt", 2),
        ];
        for shape in shapes {
            let getter = processor.generate_getter_at(&derive, &field_ident.id, &shape, offset, &mut ctx);
            let setter = processor.generate_setter_at(&derive, &field_ident.id, &shape, offset, &mut ctx);
            assert!(matches!(ctx.definition(getter), DefinitionNode::Function(_)));
            assert!(matches!(ctx.definition(setter), DefinitionNode::Function(_)));
        }
    }

    #[test]
    fn getter_at_and_setter_at_reject_non_indexable_types() {
        let processor = StorageProcessor::new();
        panics_with("Expected StorageRef<T> or ArrayRef<T, N>", |ctx| {
            let map = ty_map(ctx);
            let field_ident = idn(ctx, "m");
            let derive = derive_attr(ctx, "Storage");
            let offset = felt_zero(ctx);
            processor.generate_getter_at(&derive, &field_ident.id, &map, offset, ctx);
        });
        panics_with("generate_setter_at called on non-StorageRef", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let field_ident = idn(ctx, "x");
            let derive = derive_attr(ctx, "Storage");
            let offset = felt_zero(ctx);
            processor.generate_setter_at(&derive, &field_ident.id, &felt, offset, ctx);
        });
    }

    #[test]
    fn storage_impl_chooses_ref_type_from_derives() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();
        let derive = derive_attr(&mut ctx, "Storage");

        // With a Storage derive the RefType points at the generated XRef type.
        let felt = ty_basic(&mut ctx, "Felt");
        let derived = strukt(&mut ctx, "Wallet", vec![("note", felt)], vec![derive.clone()]);
        let impl_node = processor.generate_storage_impl(&derived, &derive, &mut ctx);
        let ref_key = idn(&mut ctx, "RefType");
        match &impl_node.associated_types[&ref_key].ty {
            UncheckedType::Basic(ident) => assert_eq!(ctx.ident(ident.id).0, "WalletRef"),
            other => panic!("derived RefType should be WalletRef, got {other:?}"),
        }

        // Without Storage/StorageRef derives it falls back to StorageRef<X>.
        let plain_felt = ty_basic(&mut ctx, "Felt");
        let contract_attr = attr(&mut ctx, "contract", &[]);
        let undecorated = strukt(&mut ctx, "Vault", vec![("note", plain_felt)], vec![contract_attr]);
        let plain_impl = processor.generate_storage_impl(&undecorated, &derive, &mut ctx);
        match &plain_impl.associated_types[&ref_key].ty {
            UncheckedType::Generic(ident, params, _) => {
                assert_eq!(ctx.ident(ident.id).0, "StorageRef");
                assert_eq!(params.len(), 1);
            }
            other => panic!("undecorated RefType should be StorageRef<Vault>, got {other:?}"),
        }
    }

    #[test]
    fn storage_read_and_write_methods_walk_ref_fields() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();
        let derive = derive_attr(&mut ctx, "Storage");

        let storage_ref = ty_storage_ref(&mut ctx, "Felt");
        let array_ref = ty_array_ref(&mut ctx, "Felt", 2);
        let plain = ty_basic(&mut ctx, "Felt");
        let ref_struct = strukt(
            &mut ctx,
            "WalletRef",
            vec![("note", storage_ref), ("grid", array_ref), ("other", plain)],
            vec![derive.clone()],
        );
        let read = processor.generate_storage_read_method(&ref_struct, &derive, &mut ctx);
        let write = processor.generate_storage_write_method(&ref_struct, &derive, &mut ctx);
        assert!(matches!(ctx.definition(read), DefinitionNode::Function(_)));
        assert!(matches!(ctx.definition(write), DefinitionNode::Function(_)));
    }

    #[test]
    fn storage_read_and_write_methods_reject_malformed_ref_types() {
        let processor = StorageProcessor::new();
        panics_with("exactly one generic parameter", |ctx| {
            let felt_a = ty_basic(ctx, "Felt");
            let felt_b = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "StorageRef", vec![felt_a, felt_b]);
            let node = strukt(ctx, "BadRef", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_storage_read_method(&node, &derive, ctx);
        });
        panics_with("exactly two generic parameters", |ctx| {
            let felt = ty_basic(ctx, "Felt");
            let bad = ty_generic(ctx, "ArrayRef", vec![felt]);
            let node = strukt(ctx, "BadRef", vec![("x", bad)], vec![]);
            let derive = derive_attr(ctx, "Storage");
            processor.generate_storage_write_method(&node, &derive, ctx);
        });
    }

    #[test]
    fn event_impl_is_generated_with_the_event_trait() {
        let mut program = Program::<SymFeltRef>::new();
        let mut ctx = TestCtx::new(&mut program);
        let processor = StorageProcessor::new();
        let derive = derive_attr(&mut ctx, "Event");

        let felt = ty_basic(&mut ctx, "Felt");
        let node = strukt(&mut ctx, "Transferred", vec![("amount", felt)], vec![derive.clone()]);
        let impl_node = processor.generate_event_impl(&node, &derive, &mut ctx);
        match &impl_node.trait_ty {
            UncheckedType::Basic(ident) => assert_eq!(ctx.ident(ident.id).0, "Event"),
            other => panic!("event impl should implement Event, got {other:?}"),
        }
        assert!(impl_node.body.is_empty());
        assert!(impl_node.is_generated);
    }

    // The StorageProcessor only descends into definitions, so its expression
    // and statement visitors are exercised here by dispatching one node of
    // every kind through the default `visit_expr`/`visit_stmt`/`visit_definition`.
    #[test]
    fn visitor_noops_accept_every_node_kind() {
        let mut program = Program::<SymFeltRef>::new();
        let module_name = Identifier::new(program.interner.intern_ident("crate"), loc());
        let module_id = program.modules.add_node(ModuleNode {
            name: module_name,
            file_id: psy_common::FileId(0),
            modules: vec![],
            inline_modules: vec![],
            definitions: vec![],
            visibility: Visibility::Public,
            comments: vec![],
            location: loc(),
        });
        let mut ctx = TestCtx::new(&mut program);
        let mut processor: StorageProcessor = StorageProcessor::new();

        let felt = felt_zero(&mut ctx);
        let target = ty_basic(&mut ctx, "x");
        let path = ctx.alloc_expression(ExprNode::Path(PathNode {
            root: None,
            segments: vec![],
            target,
            is_ty: false,
            location: loc(),
        }));
        let block = ctx.alloc_expression(ExprNode::BlockExpr(BlockExprNode {
            stmts: vec![],
            expr: None,
            expr_comments: vec![],
            location: loc(),
        }));
        let binary = ctx.alloc_expression(ExprNode::Binary(BinaryNode {
            lhs: felt,
            operator: BinaryOperator::Add,
            rhs: felt,
            location: loc(),
        }));
        let unary = ctx.alloc_expression(ExprNode::Unary(UnaryNode {
            operator: UnaryOperator::Not,
            rhs: felt,
            location: loc(),
        }));
        let call = ctx.alloc_expression(ExprNode::Call(CallNode {
            callee: path,
            generic_parameters: vec![],
            args: vec![],
            location: loc(),
        }));
        let member_call = ctx.alloc_expression(ExprNode::MemberCall(MemberCallNode {
            callee: path,
            receiver: felt,
            generic_parameters: vec![],
            args: vec![],
            location: loc(),
        }));
        let cast_target = ty_basic(&mut ctx, "Felt");
        let cast = ctx.alloc_expression(ExprNode::Cast(CastNode {
            value: felt,
            target_type: cast_target,
            location: loc(),
        }));
        let index_access = ctx.alloc_expression(ExprNode::IndexAccess(IndexAccessNode {
            target: felt,
            index: felt,
            location: loc(),
        }));
        let field_ident = idn(&mut ctx, "f");
        let member_access = ctx.alloc_expression(ExprNode::MemberAccess(MemberAccessNode {
            target: felt,
            field: field_ident,
            generic_parameters: vec![],
            location: loc(),
        }));
        let intrinsic = ctx.alloc_expression(ExprNode::Intrinsic(IntrinsicExprNode::GetUserId { location: loc() }));
        let lambda = ctx.alloc_expression(ExprNode::LambdaFunction(LambdaFunctionNode {
            parameters: vec![],
            body: felt,
            return_type: None,
            location: loc(),
        }));
        let if_expr = ctx.alloc_expression(ExprNode::IfExpr(IfExprNode {
            if_branch: Case::new(felt, block, loc()),
            elseif_branches: vec![],
            else_branch: None,
            location: loc(),
        }));
        let tuple = ctx.alloc_expression(ExprNode::Tuple(TupleExprNode {
            elements: vec![],
            location: loc(),
        }));
        let tuple_access = ctx.alloc_expression(ExprNode::TupleAccess(TupleAccessNode {
            target: felt,
            index: 0,
            location: loc(),
        }));
        let match_expr = ctx.alloc_expression(ExprNode::Match(MatchNode {
            scrutinee: felt,
            arms: vec![MatchArm {
                pattern: MatchPattern::PlaceHolder(loc()),
                body: felt,
                location: loc(),
            }],
            location: loc(),
        }));
        let parentheses = ctx.alloc_expression(ExprNode::Parentheses(felt));
        for expr_id in [
            path,
            felt,
            binary,
            unary,
            call,
            member_call,
            cast,
            index_access,
            member_access,
            intrinsic,
            lambda,
            block,
            if_expr,
            tuple,
            tuple_access,
            match_expr,
            parentheses,
        ] {
            processor.visit_expr(expr_id, &mut ctx).unwrap();
        }

        let use_kind = idn(&mut ctx, "std");
        let use_def = ctx.alloc_definition(DefinitionNode::Use(UseNode {
            visibility: Visibility::Private,
            kind: use_kind,
            segments: vec![],
            target: None,
            comments: vec![],
            location: loc(),
        }));
        let while_stmt = ctx.alloc_statement(StmtNode::While(WhileNode {
            predicate: felt,
            body: block,
            comments: vec![],
            location: loc(),
        }));
        let loop_var = idn(&mut ctx, "i");
        let for_stmt = ctx.alloc_statement(StmtNode::For(ForNode {
            variable: loop_var,
            start: felt,
            end: felt,
            body: block,
            comments: vec![],
            location: loc(),
        }));
        let assignment = ctx.alloc_statement(StmtNode::Assignment(AssignmentNode {
            target: path,
            operator: AssignmentOperator::Eq,
            value: felt,
            comments: vec![],
            location: loc(),
        }));
        let var_name = idn(&mut ctx, "x");
        let var_ty = ty_basic(&mut ctx, "Felt");
        let variable = ctx.alloc_statement(StmtNode::Variable(VariableNode {
            name: var_name,
            ty: var_ty,
            qualifier: TypeQualifier::new(false, loc()),
            value: felt,
            comments: vec![],
            location: loc(),
        }));
        let def_stmt = ctx.alloc_statement(StmtNode::Definition(use_def));
        let expr_stmt = ctx.alloc_statement(StmtNode::Expression(felt));
        let ret = ctx.alloc_statement(StmtNode::Return(ReturnNode {
            expr_id: Some(felt),
            comments: vec![],
            location: loc(),
        }));
        let assert_stmt = ctx.alloc_statement(StmtNode::Intrinsic(IntrinsicStmtNode::Assert {
            left: felt,
            message: None,
            comments: vec![],
            location: loc(),
        }));
        for stmt_id in [while_stmt, for_stmt, assignment, variable, def_stmt, expr_stmt, ret, assert_stmt] {
            processor.visit_stmt(stmt_id, &mut ctx).unwrap();
        }

        // Definitions of every kind dispatch through visit_definition; the
        // storage struct needs a module ancestor for its insertions.
        ctx.push_node_id(NodeId::Module(module_id));

        let storage_derive = derive_attr(&mut ctx, "Storage");
        let contract = attr(&mut ctx, "contract", &[]);
        let grid = ty_array(&mut ctx, "Felt", 2);
        let note = ty_basic(&mut ctx, "Felt");
        let storage = strukt(&mut ctx, "Wallet", vec![("grid", grid), ("note", note)], vec![storage_derive, contract]);
        let enum_name = idn(&mut ctx, "Kind");
        let variant = idn(&mut ctx, "A");
        let enum_def = ctx.alloc_definition(DefinitionNode::Enum(EnumNode {
            name: enum_name,
            generic_parameters: vec![],
            variants: vec![EnumVariant::Basic(variant)],
            visibility: Visibility::Public,
            comments: vec![],
            location: loc(),
        }));
        let wallet_ty = ty_basic(&mut ctx, "Wallet");
        let impl_def = ctx.alloc_definition(DefinitionNode::Impl(ImplNode {
            generic_parameters: vec![],
            associated_types: IndexMap::new(),
            ty: wallet_ty.clone(),
            body: vec![],
            attrs: vec![],
            comments: vec![],
            location: loc(),
            is_generated: false,
        }));
        let storage_ty = ty_basic(&mut ctx, "Storage");
        let trait_impl_def = ctx.alloc_definition(DefinitionNode::TraitImpl(TraitImplNode {
            generic_parameters: vec![],
            associated_types: IndexMap::new(),
            trait_ty: storage_ty,
            ty: wallet_ty,
            body: vec![],
            attrs: vec![],
            comments: vec![],
            location: loc(),
            is_generated: false,
        }));
        let trait_name = idn(&mut ctx, "Store");
        let trait_def = ctx.alloc_definition(DefinitionNode::Trait(TraitNode {
            name: trait_name,
            associated_types: IndexMap::new(),
            generic_parameters: vec![],
            body: vec![],
            visibility: Visibility::Public,
            comments: vec![],
            location: loc(),
        }));
        let alias_name = idn(&mut ctx, "Amount");
        let alias_ty = ty_basic(&mut ctx, "Felt");
        let alias_def = ctx.alloc_definition(DefinitionNode::TypeAlias(TypeAliasNode {
            name: alias_name,
            ty: alias_ty,
            visibility: Visibility::Public,
            comments: vec![],
            location: loc(),
        }));
        let const_name = idn(&mut ctx, "MAX");
        let const_ty = ty_basic(&mut ctx, "Felt");
        let const_def = ctx.alloc_definition(DefinitionNode::Const(ConstNode {
            name: const_name,
            ty: const_ty,
            value: felt,
            visibility: Visibility::Public,
            comments: vec![],
            location: loc(),
        }));
        let fn_name = idn(&mut ctx, "read");
        let fn_ret = ty_basic(&mut ctx, "Felt");
        let function_def = ctx.alloc_definition(DefinitionNode::Function(FunctionNode {
            name: fn_name,
            parameters: vec![],
            generic_parameters: vec![],
            body: Some(block),
            return_type: Some(fn_ret),
            qualifier: Qualifier {
                is_extern: false,
                is_const: false,
                location: loc(),
            },
            visibility: Visibility::Public,
            attrs: vec![],
            comments: vec![],
            location: loc(),
        }));
        let storage_def = ctx.alloc_definition(DefinitionNode::Struct(storage));

        for def_id in [
            use_def,
            enum_def,
            impl_def,
            trait_impl_def,
            trait_def,
            alias_def,
            const_def,
            function_def,
            storage_def,
        ] {
            processor.visit_definition(def_id, &mut ctx).unwrap();
        }

        // visit_impl descends into the impl body definitions.
        ctx.push_node_id(NodeId::Def(impl_def));
        processor.visit_impl(impl_def, &mut ctx).unwrap();
        ctx.pop_node_id();

        // The storage struct generated Ref definitions into its module.
        let generated = program.modules[module_id].data().definitions.len();
        assert!(generated > 0, "visit_struct must insert generated definitions");
    }
}
