use anyhow::anyhow;
use indexmap::{IndexMap, IndexSet};
use psy_ast::{DefId, IdentId, VisitorContext};
use psy_vm::dpn::ops::context_trait::ContextFelt;
use tracing::instrument;

use crate::{
    rewriter::Rewriter, AstVisualizer, CheckedImplNode, CheckedTraitImplNode, Constraint, Result, ScopeKind, TypeChecker, TypeCheckerVisitorContext,
    TypeId, TypeKey,
};

#[derive(Debug)]
pub struct ImplementerCtxt {
    // poly
    impl_ids: IndexMap<TypeId, IndexMap<Constraint, IndexSet<DefId>>>,
    // trait poly -> poly
    trait_impls: IndexMap<TypeId, IndexMap<Constraint, IndexSet<TypeId>>>,
    // poly -> instance
    instances: IndexMap<TypeId, IndexMap<Constraint, TypeId>>,
    // instance -> poly
    polys: IndexMap<TypeId, TypeId>,
}

impl ImplementerCtxt {
    pub fn new() -> Self {
        Self {
            impl_ids: IndexMap::new(),
            trait_impls: IndexMap::new(),
            instances: IndexMap::new(),
            polys: IndexMap::new(),
        }
    }
}

pub trait Implementer<F: Clone + From<u32> + ContextFelt, C> {
    fn register_impl(&mut self, impl_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()>;
    fn register_trait_impl(&mut self, impl_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()>;
    fn register_instance(&mut self, ty: TypeId, poly_ty: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()>;
    fn register_poly(&mut self, ty: TypeId, poly_ty: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()>;
    fn find_instance(&mut self, ty: TypeId, generic_parameters: Vec<TypeId>, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Option<TypeId>;
    fn find_member(
        &mut self,
        ty: TypeId,
        trait_ty: Option<TypeId>,
        method: impl Into<IdentId>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<TypeId>;
    fn find_associated_type(
        &mut self,
        ty: TypeId,
        trait_ty: Option<TypeId>,
        method: impl Into<IdentId>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<TypeId>;
    fn get_impl_id<R: Copy>(
        &self,
        ty: TypeId,
        f: impl Fn(&CheckedImplNode) -> Option<R>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Option<(Constraint, DefId, R)>;
    fn get_trait_impl_ids<R: Copy>(
        &self,
        ty: TypeId,
        f: impl Fn(&CheckedTraitImplNode) -> Option<R>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Option<Vec<(Constraint, Vec<(Constraint, DefId, R)>)>>;
    fn implements_trait(
        &mut self,
        ty: TypeId,       // mono
        trait_ty: TypeId, // mono OR poly
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> bool;
    fn implemented_traits(
        &mut self,
        ty: TypeId, // mono
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Vec<(TypeId, Constraint)>;
    fn satisfies_constraint(&mut self, gen_ty: TypeId, constr_ty: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> bool;
    fn satisfies_constraints(&mut self, generics: Vec<TypeId>, constraint: &Constraint, ctx: &mut TypeCheckerVisitorContext<F, C>) -> bool;
    fn poly_of(&self, type_id: TypeId, ctx: &TypeCheckerVisitorContext<F, C>) -> Option<TypeId>;
}

impl<F: Clone + From<u32> + ContextFelt, C> Implementer<F, C> for TypeChecker<F, C> {
    #[instrument(level = "debug", skip_all)]
    fn register_impl(&mut self, impl_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let impl_node = self.program[impl_id].as_impl().unwrap();
        let constraint = Constraint::new(ctx.symbols[impl_node.ty].generic_parameters());

        let _ = self
            .implementer
            .impl_ids
            .entry(self.poly_of(impl_node.ty, ctx).unwrap())
            .or_insert_with(IndexMap::new)
            .entry(constraint)
            .or_insert_with(IndexSet::new)
            .insert(impl_id);

        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    fn register_trait_impl(&mut self, impl_id: DefId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let trait_impl_node = self.program[impl_id].as_trait_impl().unwrap();
        let trait_constraint = Constraint::new(ctx.symbols[trait_impl_node.trait_ty].generic_parameters());
        let constraint = Constraint::new(ctx.symbols[trait_impl_node.ty].generic_parameters());

        let trait_poly_ty = self.poly_of(trait_impl_node.trait_ty, ctx).unwrap();
        let poly_ty = self.poly_of(trait_impl_node.ty, ctx).unwrap();

        self.implementer
            .trait_impls
            .entry(trait_poly_ty)
            .or_insert_with(IndexMap::new)
            .entry(trait_constraint)
            .or_insert_with(IndexSet::new)
            .insert(poly_ty);

        self.implementer
            .impl_ids
            .entry(poly_ty)
            .or_insert_with(IndexMap::new)
            .entry(constraint)
            .or_insert_with(IndexSet::new)
            .insert(impl_id);

        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    fn register_instance(&mut self, ty: TypeId, poly_ty: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        let constraint = Constraint::new(ctx.symbols[ty].generic_parameters());

        self.implementer
            .instances
            .entry(poly_ty)
            .or_insert_with(IndexMap::new)
            .entry(constraint)
            .or_insert(ty);

        self.register_poly(ty, poly_ty, ctx)?;

        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    fn register_poly(&mut self, ty: TypeId, poly_ty: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        self.implementer.polys.entry(ty).or_insert(poly_ty);
        Ok(())
    }

    fn find_instance(&mut self, ty: TypeId, generic_parameters: Vec<TypeId>, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Option<TypeId> {
        self.implementer
            .instances
            .get(&ty)
            .and_then(|instance_map| instance_map.get(&Constraint::new(generic_parameters.clone())))
            .cloned()
    }

    fn find_member(
        &mut self,
        ty: TypeId, // mono OR poly
        trait_ty: Option<TypeId>,
        member: impl Into<IdentId>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<TypeId> {
        let poly_ty = self.poly_of(ty, ctx).unwrap();
        let generic_parameters = ctx.symbols[ty].generic_parameters();
        let member = member.into();

        let get_trait_member = |trait_type_id: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>| -> Option<_> {
            let trait_scope_id = ctx.symbols[trait_type_id].scope_id();
            if let Some(&method_type_id) = ctx.symbols[trait_scope_id].types.get::<TypeKey>(&member.into()) {
                return Some(method_type_id);
            }
            None
        };

        if let Some((constraint, impl_id, function_idx)) = self.get_impl_id(
            ty,
            |impl_node: &CheckedImplNode| {
                impl_node
                    .body
                    .iter()
                    .position(|&function_id| self.program[function_id].as_function().unwrap().name == member)
            },
            ctx,
        ) {
            if generic_parameters == constraint.constraints {
                let function_id = self.program[impl_id].as_impl().unwrap().body[function_idx];
                return Ok(self.program[function_id].as_function().unwrap().type_id);
            }

            if self.satisfies_constraints(generic_parameters.clone(), &constraint, ctx) {
                let instance = self.instantiate_impl(impl_id, generic_parameters.clone(), ctx)?;
                let function_id = self.program[instance].as_impl().unwrap().body[function_idx];
                return Ok(self.program[function_id].as_function().unwrap().type_id);
            }
        } else if let Some(results) = self.get_trait_impl_ids(
            ty,
            |trait_impl_node: &CheckedTraitImplNode| {
                trait_impl_node
                    .body
                    .iter()
                    .position(|&function_id| self.program[function_id].as_function().unwrap().name == member)
            },
            ctx,
        ) {
            for (constraint, result) in results {
                for (trait_constraint, impl_id, function_idx) in result {
                    if let Some(trait_ty) = trait_ty {
                        if self.poly_of(trait_ty, ctx).unwrap() != self.poly_of(self.program[impl_id].as_trait_impl().unwrap().trait_ty, ctx).unwrap()
                        {
                            continue;
                        }
                    }

                    if generic_parameters == constraint.constraints {
                        let function_id = self.program[impl_id].as_trait_impl().unwrap().body[function_idx];
                        return Ok(self.program[function_id].as_function().unwrap().type_id);
                    }

                    if self.satisfies_constraints(generic_parameters.clone(), &constraint, ctx) {
                        let instance = self.instantiate_trait_impl(impl_id, trait_constraint.constraints, generic_parameters.clone(), ctx)?;
                        let function_id = self.program[instance].as_trait_impl().unwrap().body[function_idx];
                        return Ok(self.program[function_id].as_function().unwrap().type_id);
                    }
                }
            }
        } else if let Some(constraints) = ctx.symbols[ty].as_type_variable().map(|x| x.constraints.clone()) {
            for trait_type_id in constraints.into_iter() {
                if let Some(method_type_id) = get_trait_member(trait_type_id, ctx) {
                    return Ok(method_type_id);
                }
            }
        } else if let Ok(associated_type) = self.find_associated_type(ty, trait_ty, member, ctx) {
            return Ok(associated_type);
        }

        return Err(anyhow!("{} not found", ctx.ident(member)).into());
    }

    fn find_associated_type(
        &mut self,
        ty: TypeId, // mono OR poly
        trait_ty: Option<TypeId>,
        member: impl Into<IdentId>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Result<TypeId> {
        let poly_ty = self.poly_of(ty, ctx).unwrap();
        let generic_parameters = ctx.symbols[ty].generic_parameters();
        let member = member.into();

        if let Some((constraint, impl_id, name)) = self.get_impl_id(
            ty,
            |impl_node: &CheckedImplNode| {
                impl_node
                    .associated_types
                    .iter()
                    .find(|(name, ty)| name.id == member)
                    .map(|(name, _)| name.clone())
            },
            ctx,
        ) {
            if generic_parameters == constraint.constraints {
                return Ok(self.program[impl_id]
                    .as_impl()
                    .and_then(|x| x.associated_types.get(&name))
                    .unwrap()
                    .type_id);
            }

            if self.satisfies_constraints(generic_parameters.clone(), &constraint, ctx) {
                let instance = self.instantiate_impl(impl_id, generic_parameters.clone(), ctx)?;
                return Ok(self.program[instance]
                    .as_impl()
                    .and_then(|x| x.associated_types.get(&name))
                    .unwrap()
                    .type_id);
            }
        } else if let Some(results) = self.get_trait_impl_ids(
            ty,
            |impl_node: &CheckedTraitImplNode| {
                impl_node
                    .associated_types
                    .iter()
                    .find(|(name, ty)| name.id == member)
                    .map(|(name, _)| name.clone())
            },
            ctx,
        ) {
            for (constraint, result) in results {
                for (trait_constraint, impl_id, name) in result {
                    if let Some(trait_ty) = trait_ty {
                        if self.poly_of(trait_ty, ctx).unwrap() != self.poly_of(self.program[impl_id].as_trait_impl().unwrap().trait_ty, ctx).unwrap()
                        {
                            continue;
                        }
                    }

                    if generic_parameters == constraint.constraints {
                        return Ok(self.program[impl_id]
                            .as_trait_impl()
                            .and_then(|x| x.associated_types.get(&name))
                            .unwrap()
                            .type_id);
                    }

                    if self.satisfies_constraints(generic_parameters.clone(), &constraint, ctx) {
                        let instance = self.instantiate_trait_impl(impl_id, trait_constraint.constraints, generic_parameters.clone(), ctx)?;
                        return Ok(self.program[instance]
                            .as_trait_impl()
                            .and_then(|x| x.associated_types.get(&name))
                            .unwrap()
                            .type_id);
                    }
                }
            }
        }

        return Err(anyhow!("associated type not found").into());
    }

    fn implements_trait(
        &mut self,
        ty: TypeId,       // mono
        trait_ty: TypeId, // mono or poly
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> bool {
        let _trait_poly_ty = match self.poly_of(trait_ty, ctx) {
            Some(ty) => ty,
            None => return false,
        };

        for (trait_poly_ty, trait_constraint) in self.implemented_traits(ty, ctx) {
            if _trait_poly_ty == trait_poly_ty && self.satisfies_constraints(ctx.symbols[trait_ty].generic_parameters(), &trait_constraint, ctx) {
                return true;
            }
        }

        false
    }

    fn implemented_traits(&mut self, ty: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Vec<(TypeId, Constraint)> {
        let poly_ty = match self.poly_of(ty, ctx) {
            Some(ty) => ty,
            None => return vec![],
        };

        let find_constraints = |poly_ty: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>| -> Vec<(Constraint, TypeId, Constraint)> {
            let mut result = Vec::new();
            if let Some(impl_map) = self.implementer.impl_ids.get(&poly_ty) {
                for (constraint, impl_set) in impl_map.iter() {
                    for &impl_id in impl_set {
                        if let Some(impl_node) = self.program[impl_id].as_trait_impl() {
                            let trait_poly_ty = self.poly_of(impl_node.trait_ty, ctx).unwrap();
                            if let Some(trait_map) = self.implementer.trait_impls.get(&trait_poly_ty) {
                                for (trait_constraint, trait_impl_set) in trait_map.iter() {
                                    if trait_impl_set.contains(&poly_ty) {
                                        result.push((constraint.clone(), trait_poly_ty, trait_constraint.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            result
        };

        let mut results = vec![];
        for (constraint, trait_poly_ty, trait_constraint) in find_constraints(poly_ty, ctx) {
            if self.satisfies_constraints(ctx.symbols[ty].generic_parameters(), &constraint, ctx) {
                results.push((trait_poly_ty, trait_constraint))
            }
        }

        results
    }

    // rhs_ty will be substituted by lhs_ty if they are both type variables
    // so lhs_ty should be the stricter one
    fn satisfies_constraint(&mut self, lhs_ty: TypeId, rhs_ty: TypeId, ctx: &mut TypeCheckerVisitorContext<F, C>) -> bool {
        let lhs_ty = self.substitute_all(lhs_ty, ctx).unwrap();
        let rhs_ty = self.substitute_all(rhs_ty, ctx).unwrap();

        let is_lhs_var = ctx.symbols[lhs_ty].is_type_variable();
        let is_rhs_var = ctx.symbols[rhs_ty].is_type_variable();

        self.infcx.enter_scope();
        let satisfied = match (is_rhs_var, is_lhs_var) {
            (false, false) => self.unify(rhs_ty, lhs_ty, ctx),

            (true, false) => {
                let rhs_traits = ctx.symbols[rhs_ty].as_type_variable().unwrap().constraints.clone();
                rhs_traits.is_empty()
                    || rhs_traits.iter().all(|&type_id| {
                        if ctx.symbols[type_id].is_trait() {
                            return self.implements_trait(lhs_ty, type_id, ctx);
                        }
                        self.unify(lhs_ty, type_id, ctx)
                    })
            }

            (false, true) => {
                let lhs_traits = ctx.symbols[lhs_ty].as_type_variable().unwrap().constraints.clone();
                lhs_traits.is_empty()
                    || lhs_traits.iter().all(|&type_id| {
                        if ctx.symbols[type_id].is_trait() {
                            return self.implements_trait(rhs_ty, type_id, ctx);
                        }
                        self.unify(rhs_ty, type_id, ctx)
                    })
            }

            (true, true) => {
                let rhs_traits = ctx.symbols[rhs_ty].as_type_variable().unwrap().constraints.clone();
                let lhs_traits = ctx.symbols[lhs_ty].as_type_variable().unwrap().constraints.clone();
                rhs_traits
                    .iter()
                    .all(|r_trait| lhs_traits.iter().any(|l_trait| self.unify(*l_trait, *r_trait, ctx)))
            }
        };
        self.infcx.exit_scope();

        satisfied
    }

    fn satisfies_constraints(&mut self, generic_args: Vec<TypeId>, constraint: &Constraint, ctx: &mut TypeCheckerVisitorContext<F, C>) -> bool {
        if generic_args.len() != constraint.constraints.len() {
            return false;
        }

        let satisfied = constraint
            .constraints
            .iter()
            .zip(generic_args.iter())
            .all(|(constr_ty, gen_ty)| self.satisfies_constraint(*gen_ty, *constr_ty, ctx));

        satisfied
    }

    fn poly_of(&self, type_id: TypeId, ctx: &TypeCheckerVisitorContext<F, C>) -> Option<TypeId> {
        self.implementer.polys.get(&type_id).cloned().or(Some(type_id))
    }

    fn get_impl_id<R: Copy>(
        &self,
        ty: TypeId,
        f: impl Fn(&CheckedImplNode) -> Option<R>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Option<(Constraint, DefId, R)> {
        let poly_ty = self.poly_of(ty, ctx).unwrap();
        let generic_parameters = ctx.symbols[ty].generic_parameters();

        let get_result = |impl_set: &IndexSet<DefId>| {
            for &impl_id in impl_set {
                if let Some(impl_node) = self.program[impl_id].as_impl() {
                    if let Some(function_idx) = f(impl_node) {
                        return Some((impl_id, function_idx));
                    }
                }
            }
            None
        };

        let impl_map = self.implementer.impl_ids.get(&poly_ty)?;
        let constraint = Constraint::new(generic_parameters.clone());

        for (constraint, impl_set) in impl_map
            .get(&constraint)
            .map(|impl_set| (constraint.clone(), impl_set))
            .into_iter()
            .chain(
                impl_map
                    .iter()
                    .filter(|(c, _)| c != &&constraint)
                    .map(|(c, impl_set)| (c.clone(), impl_set)),
            )
        {
            if let Some((impl_id, function_idx)) = get_result(impl_set) {
                return Some((constraint.clone(), impl_id, function_idx));
            }
        }

        None
    }

    fn get_trait_impl_ids<R: Copy>(
        &self,
        ty: TypeId,
        f: impl Fn(&CheckedTraitImplNode) -> Option<R>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> Option<Vec<(Constraint, Vec<(Constraint, DefId, R)>)>> {
        let poly_ty = self.poly_of(ty, ctx).unwrap();
        let generic_parameters = ctx.symbols[ty].generic_parameters();

        let mut get_result = |impl_set: &IndexSet<DefId>| {
            let mut result = Vec::new();
            for &impl_id in impl_set {
                if let Some(impl_node) = self.program[impl_id].as_trait_impl() {
                    if let Some(function_idx) = f(impl_node) {
                        let trait_poly_ty = self.poly_of(impl_node.trait_ty, ctx).unwrap();
                        if ctx
                            .symbols
                            .find(None, vec![ScopeKind::Module], |scope| {
                                scope.types.values().find(|type_id| type_id == &&trait_poly_ty).cloned()
                            })
                            .is_none()
                        {
                            continue;
                        }
                        if let Some(trait_map) = self.implementer.trait_impls.get(&trait_poly_ty) {
                            for (trait_constraint, trait_impl_set) in trait_map.iter() {
                                if trait_impl_set.contains(&poly_ty) {
                                    result.push((trait_constraint.clone(), impl_id, function_idx));
                                }
                            }
                        }
                    }
                }
            }
            result
        };

        let mut results = Vec::new();
        let impl_map = self.implementer.impl_ids.get(&poly_ty)?;
        let constraint = Constraint::new(generic_parameters.clone());

        for (constraint, impl_set) in impl_map
            .get(&constraint)
            .map(|impl_set| (constraint.clone(), impl_set))
            .into_iter()
            .chain(
                impl_map
                    .iter()
                    .filter(|(c, _)| c != &&constraint)
                    .map(|(c, impl_set)| (c.clone(), impl_set)),
            )
        {
            let result = get_result(impl_set);
            if !result.is_empty() {
                results.push((constraint.clone(), result));
            }
        }

        if !results.is_empty() {
            return Some(results);
        }

        None
    }
}
