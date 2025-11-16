use indexmap::IndexMap;
use itertools::Itertools;
use psy_ast::IdentId;
use psy_vm::dpn::ops::context_trait::ContextFelt;

use crate::{
    rewriter::Rewriter, CheckedArrayNode, CheckedFunctionSignature, CheckedStructField, CheckedStructNode, Constraint, Implementer, Result, ScopeId,
    Type, TypeChecker, TypeCheckerVisitorContext, TypeId,
};

#[derive(Debug)]
pub struct InferCtxt<F: Clone + From<u32> + ContextFelt, C> {
    contexts: Vec<Vec<IndexMap<TypeId, TypeId>>>,
    _marker: std::marker::PhantomData<(F, C)>,
}

impl<F: Clone + From<u32> + ContextFelt, C> InferCtxt<F, C> {
    pub fn new() -> Self {
        InferCtxt {
            contexts: vec![vec![IndexMap::new()]],
            _marker: std::marker::PhantomData,
        }
    }

    pub fn enter_context(&mut self) {
        self.contexts.push(vec![IndexMap::new()]);
    }

    pub fn exit_context(&mut self) {
        self.contexts.pop();
    }

    pub fn enter_scope(&mut self) {
        self.contexts.last_mut().unwrap().push(IndexMap::new());
    }

    pub fn exit_scope(&mut self) {
        self.contexts.last_mut().unwrap().pop();
    }

    pub fn has_equations(&self) -> bool {
        self.contexts.last().unwrap().iter().any(|x| !x.is_empty())
    }

    pub fn get_equations(&self) -> IndexMap<TypeId, TypeId> {
        self.contexts.last().unwrap().iter().cloned().flatten().collect()
    }

    pub fn probe(&self, type_id: TypeId) -> Option<TypeId> {
        self.contexts.last().unwrap().iter().rev().find_map(|x| x.get(&type_id)).cloned()
    }

    pub fn equate(&mut self, lhs_ty: TypeId, rhs_ty: TypeId) {
        self.contexts.last_mut().unwrap().last_mut().unwrap().insert(lhs_ty, rhs_ty);
    }
}

impl<F: Clone + From<u32> + ContextFelt, C> TypeChecker<F, C> {
    pub fn unify(&mut self, lhs_ty: TypeId, rhs_ty: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> bool {
        let lhs_ty = self.substitute_all(lhs_ty, ctx).unwrap();
        let rhs_ty = self.substitute_all(rhs_ty, ctx).unwrap();

        if lhs_ty == rhs_ty {
            return true;
        }

        match (&ctx.symbols[lhs_ty], &ctx.symbols[rhs_ty]) {
            (Type::Unknown, Type::Unknown) => false,
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            (Type::TypeVariable(_), _) => {
                if self.satisfies_constraint(rhs_ty, lhs_ty, ctx) {
                    self.infcx.equate(lhs_ty, rhs_ty);
                    return true;
                }
                false
            }
            (_, Type::TypeVariable(_)) => {
                if self.satisfies_constraint(lhs_ty, rhs_ty, ctx) {
                    self.infcx.equate(rhs_ty, lhs_ty);
                    return true;
                }
                false
            }
            (Type::Struct(s1), Type::Struct(s2)) => {
                self.poly_of(lhs_ty, ctx) == self.poly_of(rhs_ty, ctx)
                    && s1.generic_parameters.len() == s2.generic_parameters.len()
                    && s1
                        .generic_parameters
                        .clone()
                        .into_iter()
                        .zip_eq(s2.generic_parameters.clone().into_iter())
                        .all(|(p1, p2)| self.unify(p1, p2, ctx))
            }
            (
                &Type::Array(CheckedArrayNode {
                    inner_ty: lhs_inner_ty,
                    size_ty: lhs_size_ty,
                    ..
                }),
                &Type::Array(CheckedArrayNode {
                    inner_ty: rhs_inner_ty,
                    size_ty: rhs_size_ty,
                    ..
                }),
            ) => self.unify(lhs_inner_ty, rhs_inner_ty, ctx) && self.unify(lhs_size_ty, rhs_size_ty, ctx),
            (Type::Tuple(t1), Type::Tuple(t2)) => {
                for (lhs_ty, rhs_ty) in t1.clone().into_iter().zip_eq(t2.clone().into_iter()) {
                    if !self.unify(lhs_ty, rhs_ty, ctx) {
                        return false;
                    }
                }
                true
            }
            (Type::Trait(t1), Type::Trait(t2)) => {
                self.poly_of(lhs_ty, ctx) == self.poly_of(rhs_ty, ctx)
                    && t1.generic_parameters.len() == t2.generic_parameters.len()
                    && t1
                        .generic_parameters
                        .clone()
                        .into_iter()
                        .zip_eq(t2.generic_parameters.clone().into_iter())
                        .all(|(p1, p2)| self.unify(p1, p2, ctx))
            }

            (Type::Function(f), Type::FunctionSignature(sig)) | (Type::FunctionSignature(sig), Type::Function(f)) => &f.signature() == sig,
            (Type::LambdaFunction(f), Type::FunctionSignature(sig)) | (Type::FunctionSignature(sig), Type::LambdaFunction(f)) => {
                &f.signature() == sig
            }
            (Type::Const(c), Type::Const(d)) => c.ty == d.ty,
            (Type::Const(c), _) => c.ty == rhs_ty,
            (_, Type::Const(c)) => c.ty == lhs_ty,
            _ => lhs_ty == rhs_ty,
        }
    }

    pub fn substitute_all(&mut self, type_id: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<TypeId> {
        if !self.infcx.has_equations() {
            return Ok(type_id);
        }

        let mut result = self.substitute_type(type_id, ctx)?;

        loop {
            let fixed_point = self.substitute_type(type_id, ctx)?;

            if fixed_point == result {
                break;
            } else {
                result = fixed_point;
            }
        }

        Ok(result)
    }

    pub fn substitute_type(&mut self, type_id: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<TypeId> {
        if let Some(subst_type) = self.infcx.probe(type_id) {
            return Ok(subst_type);
        }

        match ctx.symbols[type_id].clone() {
            Type::TypeVariable(_) => Ok(type_id),

            Type::Array(array) => {
                let ty = Type::Array(CheckedArrayNode {
                    inner_ty: self.substitute_all(array.inner_ty, ctx)?,
                    size_ty: self.substitute_all(array.size_ty, ctx)?,
                    scope_id: array.scope_id,
                });
                let type_id = ctx.symbols.get_or_add_type(Some(ScopeId::primitive()), ty.key(), ty)?;
                let poly_ty = ctx.symbols.get_type_id(Some(ScopeId::primitive()), IdentId::TYPE_ARRAY).unwrap();
                self.register_instance(type_id, poly_ty, ctx)?;
                Ok(type_id)
            }

            Type::Tuple(tuple) => {
                let mut new_types = Vec::new();
                for ty in tuple {
                    new_types.push(self.substitute_all(ty, ctx)?);
                }
                let ty = Type::Tuple(new_types);
                ctx.symbols.get_or_add_type(Some(ScopeId::primitive()), ty.key(), ty)
            }

            Type::FunctionSignature(sig) => {
                let mut new_parameters = Vec::new();
                for parameter in sig.parameters {
                    new_parameters.push(self.substitute_all(parameter, ctx)?);
                }
                let new_return_type = self.substitute_all(sig.return_type, ctx)?;
                let ty = Type::FunctionSignature(CheckedFunctionSignature {
                    parameters: new_parameters,
                    return_type: new_return_type,
                });
                ctx.symbols.get_or_add_type(None, ty.key(), ty)
            }

            Type::Function(func) => {
                let generic_parameters = func
                    .generic_parameters
                    .iter()
                    .map(|x| self.substitute_all(*x, ctx))
                    .collect::<Result<Vec<_>>>()?;

                let poly_ty = self.poly_of(type_id, ctx).unwrap();

                if let Some(instance) = self.find_instance(poly_ty, generic_parameters.clone(), ctx) {
                    return Ok(instance);
                }

                let instance = self.instantiate_function(poly_ty, generic_parameters, ctx)?;
                Ok(self.program[instance].as_function().unwrap().type_id)
            }

            Type::Struct(struct_node) => {
                let mut new_fields = IndexMap::new();
                for (
                    field_name,
                    CheckedStructField {
                        ty: field_type,
                        attrs,
                        visibility,
                        comments,
                        location,
                    },
                ) in struct_node.fields
                {
                    let substituted_type = self.substitute_all(field_type, ctx)?;
                    new_fields.insert(
                        field_name,
                        CheckedStructField {
                            ty: substituted_type,
                            attrs,
                            visibility,
                            comments,
                            location,
                        },
                    );
                }

                let mut new_generic_parameters = Vec::new();
                for generic_param in struct_node.generic_parameters {
                    new_generic_parameters.push(self.substitute_all(generic_param, ctx)?);
                }

                let ty = Type::Struct(CheckedStructNode {
                    name: struct_node.name,
                    generic_parameters: new_generic_parameters,
                    fields: new_fields,
                    scope_id: struct_node.scope_id,
                    attrs: struct_node.attrs,
                    visibility: struct_node.visibility,
                    comments: struct_node.comments,
                    location: struct_node.location,
                    type_id: struct_node.type_id,
                });
                let scope_id = ctx.symbols[struct_node.scope_id].parent;
                let type_id = ctx.symbols.get_or_add_type(scope_id, ty.key(), ty)?;
                let poly_ty = ctx.symbols.get_type_id(scope_id, struct_node.name).unwrap();
                self.register_instance(type_id, poly_ty, ctx)?;
                Ok(type_id)
            }

            Type::Trait(trait_node) => {
                let generic_parameters = trait_node
                    .generic_parameters
                    .iter()
                    .map(|x| self.substitute_all(*x, ctx))
                    .collect::<Result<Vec<_>>>()?;

                let poly_ty = self.poly_of(type_id, ctx).unwrap();

                if let Some(instance) = self.find_instance(poly_ty, generic_parameters.clone(), ctx) {
                    return Ok(instance);
                }

                let instance = self.instantiate_trait(poly_ty, generic_parameters, ctx)?;
                Ok(self.program[instance].as_trait().unwrap().type_id)
            }

            _ => Ok(type_id),
        }
    }
}
