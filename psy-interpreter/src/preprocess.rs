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
                        (UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location)), ConstValue::Felt(1))
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
            if !is_ref_struct
                && matches!(&field.ty, UncheckedType::Array(_, array_size, _) if array_size.as_u64().unwrap_or(0) > 0)
            {
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
                    let base_name = type_name
                        .strip_suffix("Ref")
                        .expect("#[ref] field type must use its generated Ref type");
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
                        (UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location)), ConstValue::Felt(1))
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
                        (UncheckedType::Basic(Identifier::new(ctx.intern(base_name), attr.location)), ConstValue::Felt(1))
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
