//! Preserve semantic type identity in the ABI's string-keyed type table.
use super::*;

type Context = TypeCheckerVisitorContext<SymFeltRef, QExecContext>;

struct AbiTypeNames {
    qualified: HashMap<(IdentId, Location), IdentId>,
    collisions: HashSet<IdentId>,
    aliases: HashSet<IdentId>,
    templates: HashMap<(IdentId, Location), DefId>,
    active_instances: HashSet<IdentId>,
}

impl AbiTypeNames {
    fn collect(ctx: &mut Context) -> Self {
        let mut declarations = HashMap::<IdentId, Vec<(ModuleId, Identifier)>>::new();
        let mut aliases = HashSet::new();
        let mut templates = HashMap::new();
        for module in ctx.program.modules.iter() {
            for &id in &module.data().definitions {
                let name = match ctx.definition(id) {
                    DefinitionNode::Struct(node) => {
                        if !node.generic_parameters.is_empty() && !node.is_generated && !ctx.program.is_module_std(module.id()) {
                            templates.insert((node.name.id, node.name.location), id);
                        }
                        node.name
                    }
                    DefinitionNode::TypeAlias(node) => {
                        aliases.insert(node.name.id);
                        node.name
                    }
                    _ => continue,
                };
                declarations.entry(name.id).or_default().push((module.id(), name));
            }
        }
        let mut qualified = HashMap::new();
        let mut collisions = HashSet::new();
        for (name, entries) in declarations {
            if entries.len() < 2 {
                continue;
            }
            collisions.insert(name);
            // Standard storage types have protocol-defined ABI spellings, e.g. Map.
            // Qualify the user declaration when it shares one of those names.
            for (module, ident) in entries {
                if ctx.program.is_module_std(module) { continue; }
                let mut segments = vec![ctx.ident(name).0.to_string()];
                let mut owner = Some(module);
                while let Some(id) = owner {
                    if id == ModuleId::root() {
                        if module == id { segments.push("crate".into()); }
                        break;
                    }
                    segments.push(ctx.ident(ctx.module(id).name).0.to_string());
                    owner = ctx.program[id].parent();
                }
                segments.reverse();
                qualified.insert((name, ident.location), ctx.intern(segments.join("::")));
            }
        }
        Self { qualified, collisions, aliases, templates, active_instances: HashSet::new() }
    }

    fn name(&self, ident: Identifier) -> IdentId {
        self.qualified.get(&(ident.id, ident.location)).copied().unwrap_or(ident.id)
    }

    fn rewrite(&mut self, source: &mut UncheckedType, ty: TypeId, ctx: &mut Context) -> anyhow::Result<()> {
        let checked = ctx.symbols[ty].clone();
        if let Type::Struct(node) = &checked {
            if self.templates.contains_key(&(node.name.id, node.name.location)) &&
                node.generic_parameters.iter().all(|&id| !ctx.symbols[id].is_type_variable()) {
                *source = self.semantic_type(ty, source.location(), ctx)?;
                return Ok(());
            }
        }
        match (source, &checked) {
            (UncheckedType::Array(inner, _, _), Type::Array(array)) => self.rewrite(inner, array.inner_ty, ctx),
            (UncheckedType::Tuple(items, _), Type::Tuple(types)) => {
                for (item, &id) in items.iter_mut().zip(types) { self.rewrite(item, id, ctx)?; }
                Ok(())
            }
            (UncheckedType::Generic(name, args, _), Type::Struct(node)) => {
                name.id = self.name(node.name);
                for (arg, &id) in args.iter_mut().zip(&node.generic_parameters) { self.rewrite(arg, id, ctx)?; }
                Ok(())
            }
            (source @ (UncheckedType::Basic(_) | UncheckedType::Path(_)), checked) => {
                let source_id = match &*source {
                    UncheckedType::Basic(id) => Some(id.id),
                    UncheckedType::Path(path) => path.target.basic_target().map(|id| id.id),
                    _ => None,
                };
                let qualified_struct = checked.as_struct().is_some_and(|node| self.name(node.name) != node.name.id);
                let module_type = matches!(source, UncheckedType::Path(_)) && source_id.is_some_and(|id| {
                    checked.as_struct().is_some_and(|node| node.name.id == id) || self.aliases.contains(&id)
                });
                if module_type || qualified_struct || source_id.is_some_and(|id| self.collisions.contains(&id)) {
                    *source = self.semantic_type(ty, source.location(), ctx)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn semantic_type(&mut self, ty: TypeId, loc: Location, ctx: &mut Context) -> anyhow::Result<UncheckedType> {
        let basic = |id| UncheckedType::Basic(Identifier::new(id, loc));
        Ok(match &ctx.symbols[ty].clone() {
            Type::Felt => basic(IdentId::TYPE_FELT),
            Type::Bool => basic(IdentId::TYPE_BOOL),
            Type::Uint(width) => basic(match width {
                UintWidth::U32 => IdentId::TYPE_U32, UintWidth::U64 => IdentId::TYPE_U64,
                UintWidth::U128 => IdentId::TYPE_U128, UintWidth::U256 => IdentId::TYPE_U256,
            }),
            Type::Struct(node) => {
                let id = self.name(node.name);
                if node.generic_parameters.is_empty() {
                    basic(id)
                } else {
                    let args = node.generic_parameters.iter().map(|&id| self.semantic_type(id, loc, ctx)).collect::<anyhow::Result<_>>()?;
                    let concrete = UncheckedType::Generic(Identifier::new(id, loc), args, loc);
                    if let Some(&definition) = self.templates.get(&(node.name.id, node.name.location)) {
                        let spelling = Formatter::new().format_type(&concrete, ctx);
                        let instance = ctx.intern(spelling);
                        if !ctx.program.abi_struct_instances.contains_key(&instance) {
                            anyhow::ensure!(self.active_instances.insert(instance), "recursive ABI layout for {}", ctx.ident(instance));
                            let mut layout = ctx.definition(definition).as_struct().unwrap().clone();
                            layout.name.id = instance;
                            layout.generic_parameters.clear();
                            for (name, field) in &mut layout.fields {
                                field.ty = self.semantic_type(node.fields[name].ty, field.ty.location(), ctx)?;
                            }
                            self.active_instances.remove(&instance);
                            ctx.program.abi_struct_instances.insert(instance, layout);
                        }
                        basic(instance)
                    } else {
                        concrete
                    }
                }
            }
            Type::Array(_) if ty == HASH_TYPE => basic(IdentId::TYPE_HASH),
            Type::Array(array) => {
                let UncheckedType::Const(length, _) = self.semantic_type(array.size_ty, loc, ctx)? else {
                    anyhow::bail!("ABI array length is not constant");
                };
                UncheckedType::Array(Box::new(self.semantic_type(array.inner_ty, loc, ctx)?), length, loc)
            }
            Type::Const(value) => {
                let length = match &*ctx.symbols[value.value].borrow() {
                    CheckedValue::Felt(f) => ConstValue::Felt(f.get_constant_value()),
                    CheckedValue::Bool(f) => ConstValue::Bool(f.get_constant_value() != 0),
                    CheckedValue::Uint(words, UintWidth::U32) => ConstValue::U32(words[0].get_constant_value() as u32),
                    _ => anyhow::bail!("unsupported ABI type constant"),
                };
                UncheckedType::Const(length, loc)
            }
            Type::Tuple(items) => UncheckedType::Tuple(items.iter().map(|&id| self.semantic_type(id, loc, ctx)).collect::<anyhow::Result<_>>()?, loc),
            Type::TypeVariable(var) => basic(var.name),
            _ => anyhow::bail!("unsupported ABI type {:?}", ctx.symbols[ty].kind()),
        })
    }
}

pub(super) fn prepare(checker: &TypeChecker<SymFeltRef, QExecContext>, ctx: &mut Context) -> anyhow::Result<()> {
    let mut names = AbiTypeNames::collect(ctx);
    let aliases: HashMap<_, _> = checker.program.defs.iter().filter_map(|definition| {
        let alias = definition.as_type_alias()?;
        Some(((alias.name.id, alias.name.location), alias.ty))
    }).collect();
    for index in 0..ctx.program.defs.len() {
        let id = DefId::from(index);
        let checked = checker.unchecked_checked.get(&id.into()).and_then(|n| n.as_def().copied()).map(|id| &checker.program[id]);
        let mut declaration = match ctx.definition(id) {
            DefinitionNode::Struct(_) | DefinitionNode::TypeAlias(_) => ctx.definition(id).clone(),
            DefinitionNode::Function(_) if ctx.program.compiled_method_definitions.contains(&id) => ctx.definition(id).clone(),
            _ => continue,
        };
        match &mut declaration {
            DefinitionNode::Struct(node) => {
                node.name.id = names.name(node.name);
                if let Some(checked) = checked.and_then(|d| d.as_struct()) {
                    for (field, value) in &mut node.fields {
                        names.rewrite(&mut value.ty, checked.fields[field].ty, ctx)?;
                    }
                }
            }
            DefinitionNode::TypeAlias(node) => {
                if let Some(&ty) = aliases.get(&(node.name.id, node.name.location)) {
                    names.rewrite(&mut node.ty, ty, ctx)?;
                }
                node.name.id = names.name(node.name);
            }
            DefinitionNode::Function(node) => {
                let signature = checked.and_then(|d| d.as_function()).ok_or_else(|| anyhow::anyhow!("missing checked method declaration"))?;
                for (param, checked) in node.parameters.iter_mut().zip(&signature.parameters) {
                    names.rewrite(&mut param.ty, checked.ty, ctx)?;
                }
                if let Some(output) = &mut node.return_type { names.rewrite(output, signature.return_type, ctx)?; }
            }
            _ => unreachable!(),
        }
        if &declaration != ctx.definition(id) {
            ctx.program.abi_definitions.insert(id, declaration);
        }
    }
    Ok(())
}
