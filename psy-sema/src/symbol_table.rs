use std::{
    fmt::{Display, Formatter},
    hash::Hash,
    ops::{Index, IndexMut},
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::anyhow;
use enum_as_inner::EnumAsInner;
use indexmap::IndexMap;
use psy_ast::*;
use psy_common::{define_arena_id, FileId, TreeNode};
use psy_vm::dpn::ops::context_trait::ContextFelt;

use crate::{variable::CheckedVariable, CheckedGenericParameter, CheckedValueRef, Error, ModuleId, ModuleKind, Result, Type, TypeId, TypeKey};

define_arena_id!(ScopeId);
define_arena_id!(VarId);
define_arena_id!(ConstId);

pub struct PrimitiveScopeId(AtomicUsize);

impl PrimitiveScopeId {
    const UNSET: usize = usize::MAX;

    pub const fn new() -> Self {
        Self(AtomicUsize::new(Self::UNSET))
    }

    pub fn get(&self) -> Option<ScopeId> {
        let id = self.0.load(Ordering::Acquire);
        (id != Self::UNSET).then_some(ScopeId(id))
    }

    pub fn set(&self, scope_id: ScopeId) -> Result<(), ScopeId> {
        self.0
            .compare_exchange(Self::UNSET, scope_id.0, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| scope_id)
    }

    pub fn take(&self) -> Option<ScopeId> {
        let id = self.0.swap(Self::UNSET, Ordering::AcqRel);
        (id != Self::UNSET).then_some(ScopeId(id))
    }
}

impl Default for PrimitiveScopeId {
    fn default() -> Self {
        Self::new()
    }
}

// Compatibility export for downstream users. Compiler state is now stored in SymbolTable.
pub static STD_PRIMITIVE_SCOPE_ID: PrimitiveScopeId = PrimitiveScopeId::new();

impl ScopeId {
    pub const fn root() -> Self {
        Self(0)
    }

    pub fn primitive() -> Self {
        STD_PRIMITIVE_SCOPE_ID.get().expect("primitive scope has not been initialized")
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, EnumAsInner)]
pub enum ScopeKind {
    Module,
    Block,
    Function,
    LambdaFunction,
    Struct,
    Array,
    Enum,
    Impl,
    ImplMethod,
    Trait,
    TraitMethod,
}

#[derive(Clone, Debug)]
pub struct Scope<F: Clone> {
    pub kind: ScopeKind,
    pub parent: Option<ScopeId>,
    pub children: Vec<ScopeId>,
    pub variables: IndexMap<IdentId, VarId>,
    pub consts: IndexMap<IdentId, ConstId>,
    pub types: IndexMap<TypeKey, TypeId>,
    _marker: std::marker::PhantomData<F>,
}

#[derive(Clone, Debug)]
pub struct Frame<T: Clone> {
    pub variables: Vec<(ScopeId, IndexMap<IdentId, T>)>,
}

impl<T: Clone> Frame<T> {
    pub fn new(scope_id: ScopeId) -> Self {
        Self {
            variables: vec![(scope_id, IndexMap::new())],
        }
    }

    pub fn push_scope(&mut self, scope_id: ScopeId) {
        self.variables.push((scope_id, IndexMap::new()));
    }

    pub fn pop_scope(&mut self) {
        self.variables.pop();
    }

    pub fn set_value(&mut self, scope_id: ScopeId, key: IdentId, value: T) {
        for (sid, vars) in self.variables.iter_mut().rev() {
            if *sid == scope_id {
                vars.insert(key, value);
                return;
            }
        }
    }

    pub fn get_value(&self, scope_id: ScopeId, key: IdentId) -> Option<&T> {
        for (sid, vars) in self.variables.iter().rev() {
            if *sid == scope_id {
                return vars.get(&key);
            }
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct Module {
    pub name: Identifier,
    pub id: ModuleId,
    pub scope_id: ScopeId,
    pub kind: ModuleKind,
    pub parent: Option<ModuleId>,
    pub children: Vec<ModuleId>,
    pub visibility: Visibility,
    pub location: Location,
}

impl Module {
    pub fn new(
        name: Identifier,
        id: ModuleId,
        scope_id: ScopeId,
        file_id: FileId,
        parent: Option<ModuleId>,
        visibility: Visibility,
        location: Location,
    ) -> Self {
        Self {
            name,
            id,
            scope_id,
            kind: ModuleKind::File { file_id },
            parent,
            children: vec![],
            visibility,
            location,
        }
    }
}

impl<F: Clone> Scope<F> {
    pub fn new(kind: ScopeKind, parent: Option<ScopeId>) -> Self {
        Self {
            kind,
            parent,
            children: vec![],
            variables: IndexMap::with_capacity(10),
            consts: IndexMap::new(),
            types: IndexMap::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SymbolTable<F: Clone + From<u32> + ContextFelt> {
    scopes: Vec<Scope<F>>,
    scope_stack: Vec<ScopeId>,
    frames: Vec<Frame<CheckedValueRef<F>>>,
    primitive_scope_id: Option<ScopeId>,

    pub types: Vec<Type>,
    consts: Vec<CheckedValueRef<F>>,
    variables: Vec<CheckedVariable<F>>,
    modules: Vec<Module>,
    module_stack: Vec<ModuleId>,
}

macro_rules! impl_index {
    ($index_type:ty, $output_type:ty, $field:ident) => {
        impl<F: Clone + From<u32> + ContextFelt> Index<$index_type> for SymbolTable<F> {
            type Output = $output_type;
            fn index(&self, index: $index_type) -> &Self::Output {
                &self.$field[index.0]
            }
        }

        impl<F: Clone + From<u32> + ContextFelt> IndexMut<$index_type> for SymbolTable<F> {
            fn index_mut(&mut self, index: $index_type) -> &mut Self::Output {
                &mut self.$field[index.0]
            }
        }
    };
}

impl_index!(ModuleId, Module, modules);
impl_index!(TypeId, Type, types);
impl_index!(VarId, CheckedVariable<F>, variables);
impl_index!(ConstId, CheckedValueRef<F>, consts);
impl_index!(ScopeId, Scope<F>, scopes);

impl<T: Clone + From<u32> + ContextFelt> Display for SymbolTable<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for (i, scope) in self.scopes.iter().enumerate() {
            writeln!(f, "ScopeId({})", i)?;
            writeln!(f, "  kind: {:?}", scope.kind)?;
            writeln!(f, "  parent: {:?}", scope.parent)?;
            writeln!(f, "  children: {:?}", scope.children)?;
            writeln!(f, "  types:")?;
            //print scope type
            for (k, v) in &scope.types {
                writeln!(f, "    {:?} : {:?} ", k, v)?;
            }
            //variables
            writeln!(f, "  variables:")?;
            for (k, v) in &scope.variables {
                writeln!(f, "    {:?} : {:?}  ", k, v,)?;
            }
        }
        //print type
        for (i, ty) in self.types.iter().enumerate() {
            let ty = match ty {
                Type::Felt => format!("Felt"),
                Type::Bool => format!("Bool"),
                Type::Array(a) => format!("Array({:?})", a),
                Type::Struct(s) => format!("Struct({:?})", s),
                Type::Function(f) => format!("Function({:?})", f),
                Type::TypeVariable(t) => format!("TypeVariable({:?})", t),
                Type::Trait(t) => format!("Trait({:?})", t),
                Type::Const(c) => format!("Const({:?})", c),
                Type::Tuple(t) => format!("Tuple({:?})", t),
                _ => format!("unknown"),
            };
            writeln!(f, "TypeId({}) : {:?}", i, ty)?;
        }
        //print module
        for (i, module) in self.modules.iter().enumerate() {
            writeln!(f, "ModuleId({})", i)?;
            writeln!(f, "  name: I{:?}", module.name)?;
            writeln!(f, "  id: {:?}", module.id)?;
            writeln!(f, "  scope_id: {:?}", module.scope_id)?;
            writeln!(f, "  kind: {:?}", module.kind)?;
            writeln!(f, "  parent: {:?}", module.parent)?;
            writeln!(f, "  children: {:?}", module.children)?;
        }
        Ok(())
    }
}

impl<F: Clone + From<u32> + ContextFelt> SymbolTable<F> {
    pub fn new() -> Self {
        SymbolTable {
            scopes: vec![],
            scope_stack: vec![],
            frames: vec![],
            primitive_scope_id: None,

            types: vec![],
            consts: vec![],
            variables: vec![],
            modules: vec![],
            module_stack: vec![],
        }
    }

    pub fn load_modules<'a>(&mut self, modules: impl IntoIterator<Item = &'a TreeNode<ModuleId, ModuleNode>>) {
        for module in modules {
            let data = module.data();
            self.modules.push(Module {
                name: data.name,
                id: module.id(),
                scope_id: ScopeId(module.id().into()),
                kind: ModuleKind::File { file_id: data.file_id },
                parent: module.parent(),
                children: module.children().to_vec(),
                visibility: data.visibility,
                location: data.location,
            });
            self.scopes.push(Scope {
                kind: ScopeKind::Module,
                parent: module.parent().map(|x| ScopeId(x.into())),
                children: module.children().into_iter().map(|&x| ScopeId(x.into())).collect(),
                variables: IndexMap::with_capacity(10),
                consts: IndexMap::new(),
                types: IndexMap::new(),
                _marker: std::marker::PhantomData,
            })
        }
    }

    pub fn modules(&self) -> &Vec<Module> {
        &self.modules
    }

    pub fn types(&self) -> &Vec<Type> {
        &self.types
    }

    pub fn set_primitive_scope_id(&mut self, scope_id: ScopeId) {
        self.primitive_scope_id = Some(scope_id);
    }

    pub fn primitive_scope_id(&self) -> ScopeId {
        self.primitive_scope_id.expect("primitive scope has not been initialized")
    }

    pub fn type_scope_id(&self, type_id: TypeId) -> ScopeId {
        match &self[type_id] {
            Type::Felt | Type::Bool | Type::U32 | Type::Tuple(_) => self.primitive_scope_id(),
            ty => ty.scope_id(),
        }
    }

    pub fn current_scope_id(&self) -> Option<ScopeId> {
        self.scope_stack.last().cloned()
    }

    pub fn parent_scope_id(&self) -> Option<ScopeId> {
        self[self.current_scope_id()?].parent
    }

    pub fn current_module_id(&self) -> Option<ModuleId> {
        self.module_stack.last().cloned()
    }

    pub fn is_module_visible(&self, module_id: ModuleId) -> bool {
        let module = &self[module_id];
        if module.visibility == Visibility::Public {
            return true;
        }
        if let Some(current_module_id) = self.current_module_id() {
            let current_module = &self[current_module_id];
            if module.parent == Some(current_module_id) || module.parent == current_module.parent {
                return true;
            }
        }
        return false;
    }

    pub fn next_type_id(&self, offset: usize) -> TypeId {
        (self.types.len() + offset).into()
    }

    pub fn add_type_id<K: Into<TypeKey>>(&mut self, scope_id: Option<ScopeId>, name: K, type_id: TypeId) -> anyhow::Result<()> {
        let key = name.into();
        let scope_id = scope_id.or(self.current_scope_id()).unwrap();

        if self[scope_id].types.contains_key(&key) {
            return Err(anyhow!("Type already defined"));
        }

        self[scope_id].types.insert(key, type_id);
        Ok(())
    }

    pub fn create_type(&mut self, ty: Type) -> Result<TypeId> {
        let type_id = TypeId(self.types.len());
        self.types.push(ty);
        Ok(type_id)
    }

    pub fn add_type<K: Into<TypeKey>>(&mut self, scope_id: Option<ScopeId>, name: K, ty: Type) -> Result<TypeId> {
        let key = name.into();
        let type_id = TypeId(self.types.len());
        self.add_type_id(scope_id, key, type_id)?;
        self.types.push(ty);
        Ok(type_id)
    }

    pub fn modify_type(&mut self, type_id: TypeId, f: impl FnOnce(&mut Type) -> Result<()>) -> Result<()> {
        f(&mut self[type_id])
    }

    pub fn get_or_add_type<K: Into<TypeKey>>(&mut self, scope_id: Option<ScopeId>, name: K, ty: Type) -> Result<TypeId> {
        let key = name.into();
        let scope_id = scope_id.or(self.current_scope_id());
        if let Some(type_id) = self.get_type_id(scope_id, key.clone()) {
            Ok(type_id)
        } else {
            let type_id = TypeId(self.types.len());
            self[scope_id.unwrap()].types.insert(key, type_id);
            self.types.push(ty);
            Ok(type_id)
        }
    }

    pub fn add_type_variable(&mut self, kind: ScopeKind, ty: CheckedGenericParameter) -> Result<TypeId> {
        let key: TypeKey = ty.name.into();
        if let Some(_) = self.find(None, vec![kind], |scope| scope.types.get(&key).cloned()) {
            return Err(Error::TypeAlreadyDefined {
                location: ty.location,
                type_name: ty.name,
            });
        }

        let type_id = TypeId(self.types.len());
        let current_scope_id = self.current_scope_id().unwrap();
        self.types.push(Type::TypeVariable(ty));
        self[current_scope_id].types.insert(key, type_id);
        Ok(type_id)
    }

    pub fn get_constant_value(&self, const_id: ConstId) -> CheckedValueRef<F> {
        self[const_id].clone()
    }

    pub fn get_or_add_constant(&mut self, value: CheckedValueRef<F>) -> ConstId {
        if let Some(idx) = self.consts.iter().position(|c| c.eq(&value)) {
            ConstId(idx)
        } else {
            self.consts.push(value);
            ConstId(self.consts.len() - 1)
        }
    }

    pub fn enter_module(&mut self, module_id: ModuleId) {
        self.enter_scope(self[module_id].scope_id);
        self.module_stack.push(module_id);
    }

    pub fn exit_module(&mut self) {
        self.exit_scope();
        self.module_stack.pop();
    }

    pub fn enter_scope(&mut self, scope_id: ScopeId) {
        self.scope_stack.push(scope_id);
    }

    pub fn exit_scope(&mut self) {
        self.scope_stack.pop();
    }

    pub fn enter_function(&mut self, scope_id: ScopeId) {
        if self[scope_id].kind == ScopeKind::LambdaFunction {
            self.frames.last_mut().unwrap().push_scope(scope_id);
        } else {
            self.frames.push(Frame::new(scope_id));
        }
    }

    pub fn exit_function(&mut self, scope_id: ScopeId) {
        if self[scope_id].kind == ScopeKind::LambdaFunction {
            self.frames.last_mut().unwrap().pop_scope();
        } else {
            self.frames.pop();
        }
    }

    pub fn enter_block(&mut self, scope_id: ScopeId) {
        self.frames.last_mut().unwrap().push_scope(scope_id);
    }

    pub fn exit_block(&mut self) {
        self.frames.last_mut().unwrap().pop_scope();
    }

    pub fn start_scope(&mut self, kind: ScopeKind) {
        let current_scope_id = self.current_scope_id().unwrap();
        let child_scope_id = ScopeId(self.scopes.len());
        self.scopes.push(Scope::new(kind, Some(current_scope_id)));
        self[current_scope_id].children.push(child_scope_id);
        self.scope_stack.push(child_scope_id);
    }

    pub fn end_scope(&mut self) {
        self.scope_stack.pop();
    }

    pub fn start_function(&mut self) {
        self.start_scope(ScopeKind::Function);
    }

    pub fn end_function(&mut self) {
        self.end_scope();
    }

    pub fn get_type_id<K: Into<TypeKey>>(&self, start_scope: Option<ScopeId>, name: K) -> Option<TypeId> {
        let name: TypeKey = name.into();
        self.find(start_scope, vec![ScopeKind::Module], |scope| scope.types.get(&name).cloned())
    }

    pub fn find<R>(&self, start_scope: Option<ScopeId>, end_scope_kinds: Vec<ScopeKind>, f: impl Fn(&Scope<F>) -> Option<R>) -> Option<R> {
        let mut current_scope_id = start_scope.or(self.current_scope_id());

        while let Some(scope_id) = current_scope_id {
            if let Some(r) = f(&self[scope_id]) {
                return Some(r);
            }
            if end_scope_kinds.iter().any(|x| x == &self[scope_id].kind) {
                return None;
            }
            current_scope_id = self[scope_id].parent;
        }

        None
    }

    pub fn get_variable<I: Into<IdentId>>(&self, start_scope: Option<ScopeId>, key: I) -> Option<VarId> {
        let key = key.into();
        let var_id = self.find(
            start_scope,
            vec![ScopeKind::Function, ScopeKind::ImplMethod, ScopeKind::TraitMethod],
            |scope| scope.variables.get(&key).cloned(),
        )?;
        Some(var_id)
    }

    pub fn get_value(&self, var_id: VarId) -> Option<CheckedValueRef<F>> {
        let scope_id = self[var_id].scope_id;
        let key = self[var_id].name;
        self.frames.last().unwrap().get_value(scope_id, key.id).cloned()
    }

    pub fn set_value(&mut self, var_id: VarId, value: CheckedValueRef<F>) -> Result<()> {
        let scope_id = self[var_id].scope_id;
        let key = self[var_id].name;
        let location = self[var_id].location;

        if self.frames.last().unwrap().get_value(scope_id, key.id).is_some() && (!self[var_id.clone()].qualifier.is_mutable) {
            return Err(Error::ImmutableVariable {
                location: location,
                variable: key.id,
            });
        }
        self.frames.last_mut().unwrap().set_value(scope_id, key.id, value);
        Ok(())
    }

    pub fn set_variable(&mut self, scope_id: ScopeId, key: impl Into<IdentId>, value: CheckedValueRef<F>) -> Result<()> {
        let var_id = self[scope_id].variables.get(&key.into()).unwrap();
        return self.set_value(var_id.clone(), value);
    }

    pub fn declare_variable(&mut self, variable: CheckedVariable<F>) -> Option<VarId> {
        let scope_id = self.current_scope_id().unwrap();
        assert_eq!(variable.scope_id, scope_id);
        if self[scope_id].variables.contains_key(&variable.name.id) {
            return None;
        }
        let var_id = VarId(self.variables.len());
        self[scope_id].variables.insert(variable.name.id, var_id);
        self.variables.push(variable);
        Some(var_id)
    }

    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
            && self.scope_stack.is_empty()
            && self.frames.is_empty()
            && self.types.is_empty()
            && self.consts.is_empty()
            && self.variables.is_empty()
            && self.modules.is_empty()
            && self.module_stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CheckedArrayNode, CheckedConstNode, CheckedStructNode};
    use psy_vm::dpn::ops::sym_felt::SymFeltRef;

    #[test]
    fn primitive_scope_id_lifecycle() {
        // Function pointers keep these tiny accessors from being inlined into
        // the caller so their own bodies execute out-of-line.
        let new_fn: fn() -> PrimitiveScopeId = PrimitiveScopeId::new;
        let id = new_fn();
        assert_eq!(id.get(), None);

        id.set(ScopeId::root()).unwrap();
        assert_eq!(id.get(), Some(ScopeId::root()));
        assert_eq!(id.set(ScopeId(7)), Err(ScopeId(7)));

        let take_fn: fn(&PrimitiveScopeId) -> Option<ScopeId> = PrimitiveScopeId::take;
        assert_eq!(take_fn(&id), Some(ScopeId::root()));
        assert_eq!(take_fn(&id), None);
        assert_eq!(id.get(), None);

        let default_fn: fn() -> PrimitiveScopeId = PrimitiveScopeId::default;
        assert_eq!(default_fn().get(), None);
    }

    #[test]
    fn frame_scopes_shadow_and_restore_values() {
        let root = ScopeId(0);
        let child = ScopeId(1);
        let key = IdentId::TYPE_FELT;
        let mut frame = Frame::new(root);
        frame.set_value(root, key, 1u32);
        assert_eq!(frame.get_value(root, key), Some(&1));
        frame.push_scope(child);
        assert_eq!(frame.get_value(child, key), None);
        frame.set_value(child, key, 2u32);
        assert_eq!(frame.get_value(child, key), Some(&2));
        frame.pop_scope();
        assert_eq!(frame.get_value(root, key), Some(&1));
        frame.set_value(ScopeId(99), key, 3u32);
        assert_eq!(frame.get_value(root, key), Some(&1));
    }

    #[test]
    fn scope_type_operations_find_duplicates_and_reuse_existing_types() {
        let mut table = SymbolTable::<SymFeltRef>::new();
        assert!(table.is_empty());
        table.scopes.push(Scope::new(ScopeKind::Module, None));
        table.enter_scope(ScopeId::root());

        let name = IdentId::TYPE_FELT;
        let first = table.add_type(None, name, Type::Felt).unwrap();
        assert_eq!(table.get_type_id(None, name), Some(first));
        assert!(table.add_type(None, name, Type::Bool).is_err());
        assert_eq!(table.get_or_add_type(None, name, Type::Bool).unwrap(), first);
        table.modify_type(first, |ty| {
            *ty = Type::Bool;
            Ok(())
        })
        .unwrap();
        assert!(matches!(table[first], Type::Bool));
        assert!(!table.is_empty());
        table.exit_scope();
    }

    fn ident(id: usize) -> Identifier {
        Identifier::new(IdentId(id), Location::default())
    }

    #[test]
    fn frame_get_value_misses_unknown_scopes() {
        let frame = Frame::<u32>::new(ScopeId(0));
        assert_eq!(frame.get_value(ScopeId(7), IdentId::TYPE_FELT), None);
    }

    #[test]
    fn module_construction_and_accessors_round_trip() {
        let mut table = SymbolTable::<SymFeltRef>::new();
        assert_eq!(table.current_module_id(), None);

        let module = Module::new(
            ident(1),
            ModuleId(0),
            ScopeId(0),
            FileId(0),
            None,
            Visibility::Private,
            Location::default(),
        );
        assert_eq!(module.id, ModuleId(0));
        assert!(matches!(module.kind, ModuleKind::File { .. }));
        table.modules.push(module);

        table.scopes.push(Scope::new(ScopeKind::Module, None));
        table.enter_module(ModuleId(0));
        assert_eq!(table.current_module_id(), Some(ModuleId(0)));
        assert_eq!(table.current_scope_id(), Some(ScopeId(0)));

        table.start_function();
        assert_eq!(table.current_scope_id(), Some(ScopeId(1)));
        table.end_function();
        assert_eq!(table.current_scope_id(), Some(ScopeId(0)));

        assert_eq!(table.modules().len(), 1);
        assert!(table.types().is_empty());
        table.exit_module();
    }

    #[test]
    fn module_visibility_requires_public_or_family_relation() {
        let mut table = SymbolTable::<SymFeltRef>::new();
        let mk = |id: usize, parent: Option<ModuleId>, visibility: Visibility| {
            Module::new(ident(id), ModuleId(id), ScopeId(id), FileId(0), parent, visibility, Location::default())
        };
        // 0: private root, 1: private child of 0, 2: private child of 0,
        // 3: private root unrelated to 0, 4: public root.
        for (id, parent, vis) in [
            (0, None, Visibility::Private),
            (1, Some(ModuleId(0)), Visibility::Private),
            (2, Some(ModuleId(0)), Visibility::Private),
            (3, None, Visibility::Private),
            (4, None, Visibility::Public),
        ] {
            table.modules.push(mk(id, parent, vis));
            table.scopes.push(Scope::new(ScopeKind::Module, None));
        }

        assert!(table.is_module_visible(ModuleId(4)), "public modules are always visible");
        table.enter_module(ModuleId(1));
        assert!(table.is_module_visible(ModuleId(2)), "siblings sharing a parent are visible");
        assert!(table.is_module_visible(ModuleId(1)), "a module sees itself via the shared-parent arm");
        assert!(
            !table.is_module_visible(ModuleId(3)),
            "a private module unrelated to the current one is invisible"
        );
        table.exit_module();
    }

    #[test]
    fn primitive_scopes_are_local_to_each_symbol_table() {
        let mut first = SymbolTable::<SymFeltRef>::new();
        let mut second = SymbolTable::<SymFeltRef>::new();
        first.set_primitive_scope_id(ScopeId(3));
        second.set_primitive_scope_id(ScopeId(17));

        let first_felt = first.create_type(Type::Felt).unwrap();
        let second_felt = second.create_type(Type::Felt).unwrap();
        assert_eq!(first.primitive_scope_id(), ScopeId(3));
        assert_eq!(second.primitive_scope_id(), ScopeId(17));
        assert_eq!(first.type_scope_id(first_felt), ScopeId(3));
        assert_eq!(second.type_scope_id(second_felt), ScopeId(17));
    }

    #[test]
    #[should_panic(expected = "primitive scope has not been initialized")]
    fn primitive_scope_requires_initialization() {
        let table = SymbolTable::<SymFeltRef>::new();
        let _ = table.primitive_scope_id();
    }

    #[test]
    fn symbol_table_display_renders_scopes_types_and_modules() {
        let mut table = SymbolTable::<SymFeltRef>::new();
        table.scopes.push(Scope::new(ScopeKind::Module, None));
        table.enter_scope(ScopeId::root());
        table[ScopeId::root()].types.insert(TypeKey::from(IdentId(4)), TypeId(0));
        table[ScopeId::root()].variables.insert(IdentId(5), VarId(0));

        let felt = table.create_type(Type::Felt).unwrap();
        for ty in [
            Type::Bool,
            Type::U32,
            Type::Unknown,
            Type::Tuple(vec![felt]),
            Type::Array(CheckedArrayNode { inner_ty: felt, size_ty: felt, scope_id: ScopeId::root() }),
            Type::Const(CheckedConstNode {
                name: None,
                ty: felt,
                value: ConstId(0),
                visibility: Visibility::Private,
                scope_id: ScopeId::root(),
            }),
            Type::Struct(CheckedStructNode {
                name: ident(6),
                generic_parameters: vec![],
                fields: IndexMap::new(),
                scope_id: ScopeId::root(),
                attrs: vec![],
                visibility: Visibility::Private,
                comments: vec![],
                location: Location::default(),
                type_id: TypeId(0),
            }),
            Type::TypeVariable(CheckedGenericParameter::new(IdentId(9), vec![], ScopeId::root(), Location::default())),
        ] {
            table.create_type(ty).unwrap();
        }
        table.modules.push(Module::new(
            ident(1),
            ModuleId(0),
            ScopeId::root(),
            FileId(0),
            None,
            Visibility::Private,
            Location::default(),
        ));

        let rendered = table.to_string();
        assert!(rendered.contains("ScopeId(0)"), "{rendered}");
        assert!(rendered.contains("variables:"), "{rendered}");
        assert!(rendered.contains("Array("), "{rendered}");
        assert!(rendered.contains("Struct("), "{rendered}");
        assert!(rendered.contains("TypeVariable("), "{rendered}");
        assert!(rendered.contains("Tuple("), "{rendered}");
        assert!(rendered.contains("Const("), "{rendered}");
        assert!(rendered.contains("unknown"), "{rendered}");
        assert!(rendered.contains("ModuleId(0)"), "{rendered}");
    }
}
