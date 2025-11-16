use psy_ast::{IdentId, Identifier, ModuleId, PathNode, UncheckedType, UseNode, VisitorContext};
use psy_vm::dpn::ops::context_trait::ContextFelt;

use crate::{CheckedPathNode, Error, Implementer, Result, TypeChecker, TypeCheckerVisitorContext, TypeId, TypeKey};

#[derive(Debug)]
pub struct ResolverCtxt {}

impl ResolverCtxt {
    pub fn new() -> Self {
        Self {}
    }
}

impl<F: Clone + From<u32> + ContextFelt, C> TypeChecker<F, C> {
    pub fn resolve_path(&mut self, path: &PathNode, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<CheckedPathNode> {
        let _current_module_id = ctx.symbols.current_module_id().unwrap();

        let mut src_module = match path.root.as_ref() {
            Some(ty) => match ty {
                UncheckedType::Basic(path) if let Ok(module) = self.resolve_module(&path, ctx) => module,
                _ => {
                    if let Some((impl_ty, trait_ty, _)) = ty.as_trait_cast() {
                        let trait_type_id = self.typecheck(trait_ty, ctx)?;
                        let impl_ty_id = self.typecheck(impl_ty, ctx)?;

                        if !ctx.symbols[trait_type_id].is_trait() || !self.implements_trait(impl_ty_id, trait_type_id, ctx) {
                            return Err(Error::TypeMismatch {
                                location: path.location,
                                expected: self
                                    .implemented_traits(impl_ty_id, ctx)
                                    .into_iter()
                                    .map(|(trait_poly_ty, _)| trait_poly_ty)
                                    .collect(),
                                found: trait_type_id,
                            });
                        }

                        if !path.segments.is_empty() {
                            let segment = path.segments[0].basic_target().ok_or(Error::InvalidPathSegment {
                                location: path.segments[0].location(),
                                segment: format!("{:?}", path.segments[0]),
                            })?;
                            let mut root_ty_id = self.find_member(impl_ty_id, Some(trait_type_id), segment, ctx)?;
                            for segment in path.segments.iter().skip(1) {
                                let segment = segment.basic_target().ok_or(Error::InvalidPathSegment {
                                    location: segment.location(),
                                    segment: format!("{:?}", segment),
                                })?;
                                root_ty_id = self.find_member(root_ty_id, None, segment, ctx)?;
                            }

                            let path_target = path.target.basic_target().ok_or(Error::InvalidPathSegment {
                                location: path.segments[0].location(),
                                segment: format!("{:?}", path.segments[0]),
                            })?;
                            let member_ty_id = self.find_member(root_ty_id, None, path_target, ctx)?;

                            return Ok(CheckedPathNode::new(
                                None,
                                Some(root_ty_id),
                                Some(path_target.id),
                                self.substitute_all(member_ty_id, ctx)?,
                                None,
                                path.location,
                            ));
                        } else {
                            let path_target = path.target.as_basic().unwrap();
                            let member_ty_id = self.find_member(impl_ty_id, Some(trait_type_id), path_target, ctx)?;
                            return Ok(CheckedPathNode::new(
                                None,
                                Some(impl_ty_id),
                                Some(path_target.id),
                                self.substitute_all(member_ty_id, ctx)?,
                                Some(self.substitute_all(trait_type_id, ctx)?),
                                path.location,
                            ));
                        };
                    }

                    if !path.segments.is_empty() {
                        return Err(Error::InvalidPathSegment {
                            location: path.segments[0].location(),
                            segment: format!("{:?}", path.segments[0]),
                        });
                    }

                    let path_target = path.target.as_basic().unwrap();

                    let root_type_id = self.typecheck(ty, ctx)?;
                    let type_id = self.resolve_member_type(path, root_type_id, path_target.id, ctx)?;
                    return Ok(CheckedPathNode::new(
                        None,
                        Some(self.substitute_all(root_type_id, ctx)?),
                        Some(path_target.id),
                        self.substitute_all(type_id, ctx)?,
                        None,
                        path.location,
                    ));
                }
            },
            None => {
                if !path.segments.is_empty() {
                    return Err(Error::InvalidPathSegment {
                        location: path.segments[0].location(),
                        segment: format!("{:?}", path.segments[0]),
                    });
                }

                if !path.target.is_basic() {
                    let type_id = self.typecheck(&path.target, ctx)?;
                    return Ok(CheckedPathNode::new(
                        None,
                        None,
                        None,
                        self.substitute_all(type_id, ctx)?,
                        None,
                        path.location,
                    ));
                }

                let path_target = path.target.as_basic().unwrap();
                if let Some(var_id) = ctx.symbols.get_variable(None, path_target) {
                    return Ok(CheckedPathNode::new(
                        Some(var_id),
                        None,
                        Some(path_target.id),
                        self.substitute_all(ctx.symbols[var_id].ty, ctx)?,
                        None,
                        path.location,
                    ));
                }
                let type_id = ctx.symbols.get_type_id(None, path_target).ok_or_else(|| Error::UnresolvedType {
                    location: path.location,
                    resolved_type: path_target.id,
                })?;
                return Ok(CheckedPathNode::new(
                    None,
                    None,
                    Some(path_target.id),
                    self.substitute_all(type_id, ctx)?,
                    None,
                    path.location,
                ));
            }
        };

        for (i, segment) in path.segments.iter().enumerate() {
            // only last segment and target can be generic
            match segment {
                UncheckedType::Basic(segment_name) => {
                    if let Some(&target_module_id) = ctx.symbols[src_module]
                        .children
                        .iter()
                        .find(|&&id| ctx.symbols[id].name == segment_name.id)
                    {
                        if !ctx.symbols[target_module_id].visibility.is_public() {
                            return Err(Error::ModuleNotPublic {
                                location: path.location,
                                module: ctx.symbols[target_module_id].name.id,
                            });
                        }
                        src_module = target_module_id;
                    } else {
                        return Err(Error::InvalidPathSegment {
                            location: segment_name.location,
                            segment: format!("{:?}", ctx.ident(segment_name)),
                        });
                    }
                }
                UncheckedType::Path(_) => {
                    return Err(Error::InvalidPathSegment {
                        location: segment.location(),
                        segment: format!("{:?}", segment),
                    });
                }
                _ => {
                    if i != path.segments.len() - 1 {
                        panic!("only last segment can be generic")
                    }

                    let root_type_id = self.resolve_module_type(path, src_module, segment, ctx)?;

                    if !path.target.is_basic() {
                        // generic function call's target is function name identifier, basic unchecked
                        // type
                        panic!("last segment is generic, so target must be basic?")
                    }
                    let target_id = path.target.as_basic().unwrap().id;
                    let type_id = self.resolve_member_type(path, root_type_id, target_id, ctx)?;
                    return Ok(CheckedPathNode::new(
                        None,
                        Some(root_type_id),
                        Some(target_id),
                        self.substitute_all(type_id, ctx)?,
                        None,
                        path.location,
                    ));
                }
            }
        }

        let type_id = self.resolve_module_type(path, src_module, &path.target, ctx)?;
        return Ok(CheckedPathNode::new(
            None,
            None,
            path.target.as_basic().map(|t| t.id),
            self.substitute_all(type_id, ctx)?,
            None,
            path.location,
        ));
    }

    pub fn resolve_use(&self, use_path: &UseNode, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<Vec<(TypeKey, TypeId)>> {
        let top_module_id = self.resolve_module(&use_path.kind, ctx)?;
        ctx.add_module_reference(top_module_id, use_path.kind.location, false);

        let target_parent_module_id = use_path.segments.iter().try_fold(top_module_id, |module_id, segment| {
            Self::traverse_path_segment(module_id, segment, use_path, ctx)
        })?;

        let scope_id = ctx.symbols[target_parent_module_id].scope_id;

        match use_path.target {
            Some(target) => {
                // If target is a type, then we need to check if it is public
                if let Some((key, &type_id)) = ctx.symbols[scope_id].types.get_key_value::<TypeKey>(&target.id.into()) {
                    if !key.visibility.is_public() || !ctx.symbols[type_id].visibility().is_public() {
                        return Err(Error::TypeNotPublic {
                            location: use_path.location,
                            ty: type_id,
                        });
                    }
                    return Ok(vec![(key.clone(), type_id)]);
                }
                // Else target is a module.
                match Self::traverse_path_segment(target_parent_module_id, &target, use_path, ctx) {
                    // Return if it is visible to current module
                    Ok(_) => Ok(vec![]),
                    Err(_) => Err(Error::ModuleNotPublic {
                        location: use_path.location,
                        module: target.id,
                    }),
                }
            }
            None => Ok(ctx.symbols[scope_id]
                .types
                .iter()
                .filter_map(|(k, &id)| (k.visibility.is_public() && ctx.symbols[id].visibility().is_public()).then_some((k.clone(), id)))
                .collect()),
        }
    }

    fn traverse_path_segment(
        parent_module_id: ModuleId,
        segment: &Identifier,
        use_path: &UseNode,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<ModuleId> {
        if let Some(target_module_id) = ctx.symbols[parent_module_id]
            .children
            .iter()
            .find(|&&id| ctx.symbols[id].name == segment.id)
            .copied()
        {
            if ctx.symbols.is_module_visible(target_module_id) {
                ctx.add_module_reference(target_module_id, segment.location, false);
                Ok(target_module_id)
            } else {
                Err(Error::ModuleNotPublic {
                    location: use_path.location,
                    module: segment.id,
                })
            }
        } else {
            Err(Error::ModuleNotFound {
                location: use_path.location,
                module: segment.id,
            })
        }
    }

    pub fn resolve_module(&self, module: &Identifier, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<ModuleId> {
        let _current_module_id = ctx.symbols.current_module_id().unwrap();
        Ok(match module.id {
            IdentId::SELF => _current_module_id,
            IdentId::CRATE => {
                let mut root_id = _current_module_id;
                while let Some(parent) = ctx.symbols[root_id].parent {
                    root_id = parent;
                }
                root_id
            }
            IdentId::SUPER => ctx.symbols[_current_module_id]
                .parent
                .ok_or(Error::NoParentModule { location: module.location })?,
            name => ctx
                .symbols
                .modules()
                .iter()
                .position(|x| x.name == name && ctx.symbols.is_module_visible(x.id))
                .map(ModuleId)
                // Workaround: passing check for USE node
                // .filter(|&id| ctx.symbols[current_module_id].children.contains(&id))
                .ok_or(Error::ModuleNotFound {
                    location: module.location,
                    module: name,
                })?,
        })
    }

    pub fn resolve_member_type(
        &mut self,
        path: &PathNode,
        root: TypeId,
        target: IdentId,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<TypeId> {
        // let scope_id = ctx.symbols[root].scope_id();
        // if let Some(&type_id) =
        // ctx.symbols[scope_id].types.get::<TypeKey>(&target.into()) {
        //     if !ctx.symbols[type_id].visibility().is_public() {
        //         return Err(Error::TypeNotPublic {
        //             location: path.location,
        //             ty: type_id,
        //         });
        //     }
        //     return Ok(type_id);
        // }

        let method_type_id = self.find_member(root, None, target, ctx)?;
        if !ctx.symbols[method_type_id].visibility().is_public() {
            return Err(Error::MemberNotPublic {
                location: path.location,
                ty: root,
                field: target,
            });
        }
        return Ok(method_type_id);
    }

    pub fn resolve_module_type(
        &mut self,
        path: &PathNode,
        module: ModuleId,
        ty: &UncheckedType,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<TypeId> {
        let scope_id = ctx.symbols[module].scope_id;
        ctx.symbols.enter_scope(scope_id);
        let type_id = self.typecheck(ty, ctx)?;
        if !ctx.symbols[type_id].visibility().is_public() {
            return Err(Error::TypeNotPublic {
                location: path.location,
                ty: type_id,
            });
        }
        ctx.symbols.exit_scope();
        return Ok(type_id);
    }
}
