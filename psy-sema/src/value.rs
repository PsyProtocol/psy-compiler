use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use enum_as_inner::EnumAsInner;
use indexmap::IndexMap;
use psy_ast::{ConstValue, ExprId, IdentId, Identifier, Location, NodeInfo, NodeType};
use psy_vm::dpn::ops::{
    context_trait::{ContextFelt, DPNContext, ToFelts},
    op_types::DPNOpType,
};

use crate::{Result, Type, TypeCheckerVisitorContext, TypeId, BOOL_TYPE, FELT_TYPE, U32_TYPE, VOID_TYPE};

pub fn is_constant_op(op: DPNOpType) -> bool {
    matches!(
        op,
        DPNOpType::Constant | DPNOpType::ConstantTrue | DPNOpType::ConstantFalse | DPNOpType::ConstantU32
    )
}

#[derive(Clone, Debug, PartialEq, EnumAsInner)]
pub enum CheckedValueNode<F> {
    Felt(F, Location),
    Bool(F, Location),
    U32(F, Location),
    Array(TypeId, Vec<ExprId>, Location),
    ArrayRepeat(TypeId, ExprId, ConstValue, Location),
    Tuple(TypeId, Vec<(TypeId, ExprId)>, Location),
    Struct(TypeId, IndexMap<Identifier, ExprId>, Location),
    Type(TypeId),
}

impl<F> NodeInfo for CheckedValueNode<F> {
    fn node_type(&self) -> NodeType {
        NodeType::ValueExpr
    }
}

#[derive(Debug, EnumAsInner)]
pub enum CheckedValue<F: Clone + From<u32> + ContextFelt> {
    Felt(F),
    Bool(F),
    U32(F),
    Array(TypeId, Vec<CheckedValueRef<F>>),
    Struct(TypeId, IndexMap<Identifier, CheckedValueRef<F>>),
    Tuple {
        type_id: TypeId,
        elements: Vec<(TypeId, CheckedValueRef<F>)>,
    },
    Type(TypeId),
}

/// **NOTE**: please dont implement Deref for CheckedValueRef in case of wrong
/// usage
#[derive(Debug)]
pub struct CheckedValueRef<F: Clone + From<u32> + ContextFelt>(Arc<RwLock<CheckedValue<F>>>);

impl<F: Clone + From<u32> + ContextFelt> Clone for CheckedValueRef<F> {
    fn clone(&self) -> Self {
        match &*self.borrow() {
            CheckedValue::Felt(f) => CheckedValueRef(Arc::new(RwLock::new(CheckedValue::Felt(f.clone())))),
            CheckedValue::Bool(b) => CheckedValueRef(Arc::new(RwLock::new(CheckedValue::Bool(b.clone())))),
            CheckedValue::U32(u) => CheckedValueRef(Arc::new(RwLock::new(CheckedValue::U32(u.clone())))),
            CheckedValue::Array(_type_id, _values) => CheckedValueRef(self.0.clone()),
            CheckedValue::Tuple { type_id, elements } => CheckedValueRef(Arc::new(RwLock::new(CheckedValue::Tuple {
                type_id: type_id.clone(),
                elements: elements.iter().map(|(t, v)| (t.clone(), v.clone())).collect(),
            }))),
            CheckedValue::Struct(_type_id, _fields) => CheckedValueRef(self.0.clone()),
            CheckedValue::Type(type_id) => CheckedValueRef(Arc::new(RwLock::new(CheckedValue::Type(type_id.clone())))),
        }
    }
}
impl<F: Clone + From<u32> + ContextFelt> PartialEq for CheckedValueRef<F> {
    fn eq(&self, other: &Self) -> bool {
        match (&*self.borrow(), &*other.borrow()) {
            (CheckedValue::Felt(f1), CheckedValue::Felt(f2)) => f1 == f2,
            (CheckedValue::Bool(b1), CheckedValue::Bool(b2)) => b1 == b2,
            (CheckedValue::U32(u1), CheckedValue::U32(u2)) => u1 == u2,
            (CheckedValue::Array(_, _a1), CheckedValue::Array(_, _a2)) => std::ptr::eq(Arc::as_ptr(&self.as_rc()), Arc::as_ptr(&other.as_rc())),
            (CheckedValue::Struct(_, _s1), CheckedValue::Struct(_, _s2)) => std::ptr::eq(Arc::as_ptr(&self.as_rc()), Arc::as_ptr(&other.as_rc())),
            (CheckedValue::Type(t1), CheckedValue::Type(t2)) => t1 == t2,
            _ => false,
        }
    }
}

impl<F: Clone + From<u32> + ContextFelt> ToFelts<F> for CheckedValueRef<F> {
    fn to_felts(&self) -> Vec<F> {
        match &*self.borrow() {
            CheckedValue::Felt(f) => vec![f.clone()],
            CheckedValue::Bool(b) => vec![b.clone()],
            CheckedValue::U32(u) => vec![u.clone()],
            CheckedValue::Array(_type_id, values) => {
                let mut result = Vec::new();
                for value in values {
                    result.extend(value.to_felts());
                }
                result
            }
            CheckedValue::Tuple { elements, .. } => elements.iter().flat_map(|(_, v)| v.to_felts()).collect(),

            CheckedValue::Struct(_type_id, fields) => {
                let mut result = Vec::new();
                for (_, value) in fields {
                    result.extend(value.to_felts());
                }
                result
            }
            CheckedValue::Type(type_id) => match type_id {
                &VOID_TYPE => vec![],
                _ => unreachable!(),
            },
        }
    }

    fn from_felts(_felts: &[F]) -> Self {
        todo!()
    }
}

pub trait FeltRepr<F: Clone + From<u32> + ContextFelt, C: DPNContext<F>>: Clone {
    fn encode_felts(&self) -> Vec<F>;
    fn decode_felts(felts: &[F], ctx: &TypeCheckerVisitorContext<F, C>, target_type: TypeId) -> Self;
}

impl<F: Clone + From<u32> + ContextFelt, C: DPNContext<F>> FeltRepr<F, C> for CheckedValueRef<F> {
    fn encode_felts(&self) -> Vec<F> {
        return ToFelts::to_felts(self);
    }

    fn decode_felts(felts: &[F], ctx: &TypeCheckerVisitorContext<F, C>, target_type: TypeId) -> Self {
        match &ctx.symbols[target_type] {
            Type::Felt => {
                assert_eq!(felts.len(), 1);
                CheckedValueRef::from_felt(felts[0].clone())
            }
            Type::Bool => {
                assert_eq!(felts.len(), 1);
                CheckedValueRef::from_bool(felts[0].clone())
            }
            Type::U32 => {
                assert_eq!(felts.len(), 1);
                CheckedValueRef::from_u32(felts[0].clone())
            }
            Type::Array(checked_array_node) => {
                let size = ctx.size_of(checked_array_node.inner_ty);
                let values = felts
                    .chunks(size)
                    .map(|chunk| FeltRepr::decode_felts(chunk, ctx, checked_array_node.inner_ty))
                    .collect();

                Self::new_rc(CheckedValue::Array(target_type, values))
            }
            Type::Struct(checked_struct_node) => {
                let mut fields = IndexMap::new();
                let mut field_offset = 0;
                for (_i, (field_name, field)) in checked_struct_node.fields.iter().enumerate() {
                    let field_size = ctx.size_of(field.ty);
                    let field_value = FeltRepr::decode_felts(&felts[field_offset..(field_offset + field_size)], ctx, field.ty);
                    fields.insert(field_name.clone(), field_value);
                    field_offset += field_size;
                }

                Self::new_rc(CheckedValue::Struct(target_type, fields))
            }
            Type::Tuple(vec) => {
                let mut elements = vec![];
                let mut element_offset = 0;

                for ty in vec {
                    let element_size = ctx.size_of(*ty);
                    let element = FeltRepr::decode_felts(&felts[element_offset..(element_offset + element_size)], ctx, *ty);
                    elements.push((*ty, element));
                    element_offset += element_size;
                }

                Self::new_rc(CheckedValue::Tuple {
                    type_id: target_type,
                    elements,
                })
            }
            _ => unreachable!(),
        }
    }
}

impl<F: Clone + From<u32> + ContextFelt> CheckedValueRef<F> {
    pub fn new_rc(value: CheckedValue<F>) -> Self {
        Self(Arc::new(RwLock::new(value)))
    }

    pub fn as_rc(&self) -> Arc<RwLock<CheckedValue<F>>> {
        Arc::clone(&self.0)
    }

    pub fn as_type(&self) -> Option<TypeId> {
        match &*self.borrow() {
            CheckedValue::Type(type_id) => Some(type_id.clone()),
            _ => None,
        }
    }

    pub fn borrow(&self) -> RwLockReadGuard<'_, CheckedValue<F>> {
        self.0.read().unwrap()
    }
    pub fn borrow_mut(&self) -> RwLockWriteGuard<'_, CheckedValue<F>> {
        self.0.write().unwrap()
    }

    pub fn from_felt(value: F) -> Self {
        Self::new_rc(CheckedValue::Felt(value))
    }

    pub fn from_bool(value: F) -> Self {
        Self::new_rc(CheckedValue::Bool(value))
    }

    pub fn from_u32(value: F) -> Self {
        Self::new_rc(CheckedValue::U32(value))
    }

    pub fn from_value(value: F, ty: TypeId) -> Self {
        match ty {
            FELT_TYPE => Self::from_felt(value),
            BOOL_TYPE => Self::from_bool(value),
            U32_TYPE => Self::from_u32(value),
            _ => panic!("Expected felt/u32/bool"),
        }
    }

    pub fn from_vec(type_id: TypeId, data: impl IntoIterator<Item = F>) -> Self {
        Self::new_rc(CheckedValue::Array(type_id, data.into_iter().map(CheckedValueRef::from_felt).collect()))
    }

    pub fn to_felt(&self) -> F {
        match &*self.borrow() {
            CheckedValue::Felt(f) => f.clone(),
            _ => panic!("Expected felt value"),
        }
    }

    pub fn to_bool(&self) -> F {
        match &*self.borrow() {
            CheckedValue::Bool(f) => f.clone(),
            _ => panic!("Expected bool value"),
        }
    }

    pub fn to_u32(&self) -> F {
        match &*self.borrow() {
            CheckedValue::U32(f) => f.clone(),
            _ => panic!("Expected u32 value"),
        }
    }

    pub fn to_value(&self) -> F {
        match &*self.borrow() {
            CheckedValue::Felt(f) => f.clone(),
            CheckedValue::U32(u) => u.clone(),
            CheckedValue::Bool(b) => b.clone(),
            _ => panic!("Expected felt/u32/bool value"),
        }
    }

    pub fn to_array<const N: usize>(&self) -> [F; N]
    where
        F: std::fmt::Debug,
    {
        self.to_vec().try_into().unwrap()
    }

    pub fn to_u32_array<const N: usize>(&self) -> [F; N]
    where
        F: std::fmt::Debug,
    {
        self.to_u32_vec().try_into().unwrap()
    }

    pub fn to_vec(&self) -> Vec<F> {
        match &*self.borrow() {
            CheckedValue::Array(_, arr) => arr.into_iter().map(|x| x.to_felt()).collect::<Vec<F>>(),
            _ => panic!("Expected array value"),
        }
    }

    pub fn to_u32_vec(&self) -> Vec<F> {
        match &*self.borrow() {
            CheckedValue::Array(_, arr) => arr.into_iter().map(|x| x.to_u32()).collect::<Vec<F>>(),
            _ => panic!("Expected array value"),
        }
    }

    pub fn type_id(&self) -> TypeId {
        match &*self.borrow() {
            CheckedValue::Felt(_) => FELT_TYPE,
            CheckedValue::Bool(_) => BOOL_TYPE,
            CheckedValue::U32(_) => U32_TYPE,
            CheckedValue::Array(type_id, _) => type_id.clone(),
            CheckedValue::Struct(type_id, _) => type_id.clone(),
            CheckedValue::Type(type_id) => type_id.clone(),
            CheckedValue::Tuple { type_id, .. } => type_id.clone(),
        }
    }

    pub fn set_path<C>(&mut self, ctx: &mut C, path: &[IndexPath<F>], index_condition: &mut Vec<F>, value: Self) -> Result<()>
    where
        F: Clone + From<u32> + ContextFelt,
        C: DPNContext<F>,
    {
        if path.is_empty() {
            if index_condition.is_empty() {
                *self = value.clone();
            } else {
                let combine_condition = index_condition
                    .iter()
                    .fold(ctx.op_true(), |acc, condition| ctx.op_bool_and(*condition, acc));
                *self = Self::select(ctx, &value, &self, &|ctx: &mut C, n: &F, o: &F| {
                    ctx.op_select(combine_condition, n.clone(), o.clone())
                });
            }
            return Ok(());
        }

        match &mut *self.borrow_mut() {
            CheckedValue::Array(_, arr) => {
                let index = path[0].as_felt().unwrap();
                let rest = &path[1..];
                if is_constant_op(ctx.get_op_type(*index)) {
                    let index = ctx.get_constant_value(*index) as usize;
                    if let Some(inner) = arr.get_mut(index) {
                        inner.set_path(ctx, rest, index_condition, value)?;
                    }
                } else {
                    let arr_size = ctx.op_const(arr.len() as u64);
                    let out_of_bounds = ctx.op_lt(*index, arr_size);
                    ctx.assert_true(out_of_bounds, "felt index out of bounds");
                    for i in 0..arr.len() {
                        let arr_index = ctx.op_const(i as u64);
                        let condition = ctx.op_eq(arr_index, *index);
                        index_condition.push(condition);
                        if let Some(inner) = arr.get_mut(i) {
                            inner.set_path(ctx, rest, index_condition, value.clone())?;
                        }
                        index_condition.pop();
                    }
                }
            }
            CheckedValue::Tuple { elements, .. } => {
                let index = path[0].as_normal().unwrap();
                let rest = &path[1..];
                if let Some((_, inner)) = elements.get_mut(*index) {
                    inner.set_path(ctx, rest, index_condition, value)?;
                }
            }
            CheckedValue::Struct(_, map) => {
                let key = Identifier::new(IdentId(*path[0].as_normal().unwrap()), Location::default());
                let rest = &path[1..];
                if let Some(inner) = map.get_mut(&key) {
                    inner.set_path(ctx, rest, index_condition, value)?;
                }
            }
            _ => {
                unreachable!()
            }
        }
        Ok(())
    }

    pub fn get_path<C>(&self, ctx: &mut C, path: &[IndexPath<F>]) -> Option<CheckedValueRef<F>>
    where
        F: Clone + From<u32> + ContextFelt,
        C: DPNContext<F>,
    {
        if path.is_empty() {
            return Some(self.clone());
        }

        let value = self.borrow();
        match &*value {
            CheckedValue::Array(_, arr) => {
                let index = path[0].clone().into_felt().unwrap();
                let rest = &path[1..];
                if is_constant_op(ctx.get_op_type(index)) {
                    let index = ctx.get_constant_value(index) as usize;
                    assert!(index < arr.len(), "array index must less than array length");
                    arr.get(index).and_then(|inner| inner.get_path(ctx, rest))
                } else {
                    let arr_size = ctx.op_const(arr.len() as u64);
                    let out_of_bounds = ctx.op_lt(index, arr_size);
                    ctx.assert_true(out_of_bounds, "felt index out of bounds");

                    let mut result = arr[0].clone();
                    for i in 1..arr.len() {
                        let arr_index = ctx.op_const(i as u64);
                        let condition = ctx.op_eq(arr_index, index);
                        result = Self::select(ctx, &arr[i], &result, &|ctx: &mut C, n: &F, o: &F| {
                            ctx.op_select(condition, n.clone(), o.clone())
                        });
                    }
                    result.get_path(ctx, rest)
                }
            }
            CheckedValue::Tuple { elements, .. } => {
                let index = path[0].clone().into_normal().unwrap();
                let rest = &path[1..];
                elements.get(index).and_then(|(_, inner)| inner.get_path(ctx, rest))
            }
            CheckedValue::Struct(_, map) => {
                let key = Identifier::new(IdentId(path[0].clone().into_normal().unwrap()), Location::default());
                let rest = &path[1..];
                map.get(&key).and_then(|inner| inner.get_path(ctx, rest))
            }
            _ => None,
        }
    }

    pub fn select<C>(
        ctx: &mut C,
        new_value: &CheckedValueRef<F>,
        old_value: &CheckedValueRef<F>,
        select_fn: &impl Fn(&mut C, &F, &F) -> F,
    ) -> CheckedValueRef<F>
    where
        F: Clone + From<u32> + ContextFelt,
        C: DPNContext<F>,
    {
        if old_value == new_value {
            return old_value.clone();
        }
        match (&*old_value.borrow(), &*new_value.borrow()) {
            (CheckedValue::Felt(o), CheckedValue::Felt(n)) => CheckedValueRef::new_rc(CheckedValue::Felt(select_fn(ctx, n, o))),
            (CheckedValue::Bool(o), CheckedValue::Bool(n)) => CheckedValueRef::new_rc(CheckedValue::Bool(select_fn(ctx, n, o))),
            (CheckedValue::U32(o), CheckedValue::U32(n)) => CheckedValueRef::new_rc(CheckedValue::U32(select_fn(ctx, n, o))),
            (CheckedValue::Array(lhs_type_id, o), CheckedValue::Array(_, n)) => {
                let mut arr_data = vec![];
                for (old_value, new_value) in o.iter().zip(n.iter()) {
                    arr_data.push(Self::select(ctx, new_value, old_value, select_fn));
                }
                CheckedValueRef::new_rc(CheckedValue::Array(lhs_type_id.clone(), arr_data))
            }
            (CheckedValue::Struct(lhs_type_id, o), CheckedValue::Struct(_, n)) => {
                let mut struct_map = IndexMap::new();
                for ((old_field_name, old_field_value), (new_field_name, new_field_value)) in o.iter().zip(n.iter()) {
                    assert_eq!(old_field_name, new_field_name);
                    struct_map.insert(old_field_name.clone(), Self::select(ctx, new_field_value, old_field_value, select_fn));
                }
                CheckedValueRef::new_rc(CheckedValue::Struct(lhs_type_id.clone(), struct_map))
            }
            (
                CheckedValue::Tuple {
                    type_id: lhs_tid,
                    elements: old_elements,
                },
                CheckedValue::Tuple { elements: new_elements, .. },
            ) => {
                assert_eq!(old_elements.len(), new_elements.len(), "Tuple size mismatch");

                let mut tuple_elements = vec![];
                for ((old_type_id, old_value), (_, new_value)) in old_elements.iter().zip(new_elements.iter()) {
                    tuple_elements.push((old_type_id.clone(), Self::select(ctx, new_value, old_value, select_fn)));
                }
                CheckedValueRef::new_rc(CheckedValue::Tuple {
                    type_id: lhs_tid.clone(),
                    elements: tuple_elements,
                })
            }
            (o, n) => {
                panic!("CheckedValueRef::select unsupported variant pair: old={:?}, new={:?}", o, n)
            }
        }
    }
}

#[derive(Clone, Debug, EnumAsInner)]
pub enum IndexPath<F: ContextFelt> {
    Normal(usize),
    Felt(F),
}

#[cfg(test)]
mod tests {
    use crate::{CheckedArrayNode, CheckedStructField, CheckedStructNode};

    use psy_vm::dpn::ops::exec_context::QExecContext;
    use psy_vm::dpn::ops::sym_felt::SymFeltRef;

    use super::*;

    fn felt_value(value: u64) -> CheckedValueRef<SymFeltRef> {
        let mut ctx = QExecContext::new();
        CheckedValueRef::from_felt(ctx.op_const(value))
    }

    fn bool_value(value: bool) -> CheckedValueRef<SymFeltRef> {
        let mut ctx = QExecContext::new();
        CheckedValueRef::from_bool(if value { ctx.op_true() } else { ctx.op_false() })
    }

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn checked_value_ref_from_felts_is_unimplemented() {
        // A function pointer keeps the stub from being inlined into the caller.
        let from_felts: fn(&[SymFeltRef]) -> CheckedValueRef<SymFeltRef> =
            <CheckedValueRef<SymFeltRef> as ToFelts<SymFeltRef>>::from_felts;
        let _ = from_felts(&[]);
    }

    #[test]
    fn scalar_conversions_round_trip_through_their_typed_accessors() {
        let felt = felt_value(7);
        let boolean = bool_value(true);
        let mut ctx = QExecContext::new();
        let u32_value = CheckedValueRef::<SymFeltRef>::from_u32(ctx.op_const_u32(9));

        // EnumAsInner accessors live on CheckedValue, reached through the guard.
        assert!(felt.borrow().is_felt());
        assert!(boolean.borrow().is_bool());
        assert!(u32_value.borrow().is_u32());

        // Typed accessors clone the backing felt of every scalar kind, and
        // to_value accepts them all.
        let _ = felt.to_felt();
        let _ = boolean.to_bool();
        let _ = u32_value.to_u32();
        let _ = felt.to_value();
        let _ = boolean.to_value();
        let _ = u32_value.to_value();

        assert_eq!(felt.type_id(), FELT_TYPE);
        assert_eq!(boolean.type_id(), BOOL_TYPE);
        assert_eq!(u32_value.type_id(), U32_TYPE);
    }

    #[test]
    fn array_conversion_flattens_elements_and_reports_the_array_type() {
        let mut ctx = QExecContext::new();
        let type_id = TypeId::from(42usize);
        let empty = CheckedValueRef::<SymFeltRef>::from_vec(type_id.clone(), []);
        assert_eq!(empty.type_id(), type_id);
        assert!(empty.to_vec().is_empty());
        assert!(empty.to_felts().is_empty());

        let array = CheckedValueRef::<SymFeltRef>::from_vec(type_id.clone(), [ctx.op_const(1), ctx.op_const(2), ctx.op_const(3)]);
        assert_eq!(array.type_id(), type_id);
        let felts = array.to_felts();
        assert_eq!(felts.len(), 3);
        let restored: [SymFeltRef; 3] = array.to_array();
        assert_eq!(restored.len(), 3);
    }

    #[test]
    fn equality_compares_scalars_by_value_and_containers_by_identity() {
        let a = felt_value(5);
        let b = felt_value(5);
        let c = felt_value(6);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, bool_value(true));

        let array_a = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![felt_value(1)],
        ));
        let clone = array_a.clone();
        let array_b = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![felt_value(1)],
        ));
        // Arrays compare by Arc identity: a clone shares storage, a fresh
        // allocation with equal contents does not.
        assert_eq!(array_a, clone);
        assert_ne!(array_a, array_b);
    }

    #[test]
    fn get_and_set_path_on_containers_without_conditions() {
        let mut ctx = QExecContext::new();
        let array = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![felt_value(10), felt_value(20)],
        ));

        // Arrays are indexed by felt paths, tuples and structs by Normal ones.
        let index = ctx.op_const(1);
        let second = array.get_path(&mut ctx, &[IndexPath::Felt(index)]).unwrap();
        assert_eq!(second, felt_value(20));
        assert_eq!(array.get_path(&mut ctx, &[]).unwrap(), array);

        let tuple = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Tuple {
            type_id: TypeId::from(0usize),
            elements: vec![(TypeId::from(1usize), felt_value(1)), (TypeId::from(2usize), felt_value(2))],
        });
        assert_eq!(tuple.get_path(&mut ctx, &[IndexPath::Normal(1)]).unwrap(), felt_value(2));
        assert!(tuple.get_path(&mut ctx, &[IndexPath::Normal(9)]).is_none());

        let mut fields = IndexMap::new();
        fields.insert(Identifier::new(IdentId(4), Location::default()), felt_value(7));
        let structure = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Struct(TypeId::from(3usize), fields));
        assert_eq!(structure.get_path(&mut ctx, &[IndexPath::Normal(4)]).unwrap(), felt_value(7));
        assert!(structure.get_path(&mut ctx, &[IndexPath::Normal(5)]).is_none());

        // A non-empty path into a scalar has nothing to traverse.
        let index = ctx.op_const(0);
        assert!(felt_value(0).get_path(&mut ctx, &[IndexPath::Felt(index)]).is_none());

        let mut target = felt_value(0);
        target.set_path(&mut ctx, &[], &mut vec![], felt_value(99)).unwrap();
        assert_eq!(target, felt_value(99));
    }

    #[test]
    fn clone_of_scalars_is_independent_while_arrays_share_storage() {
        let scalar = felt_value(3);
        let mut cloned = scalar.clone();
        cloned = felt_value(4);
        assert_eq!(scalar, felt_value(3));

        let array = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![felt_value(1)],
        ));
        let alias = array.clone();
        assert!(std::ptr::eq(
            Arc::as_ptr(&array.as_rc()),
            Arc::as_ptr(&alias.as_rc())
        ));
    }

    #[test]
    fn is_constant_op_classifies_the_constant_op_family() {
        use psy_vm::dpn::ops::op_types::DPNOpType;

        assert!(is_constant_op(DPNOpType::Constant));
        assert!(is_constant_op(DPNOpType::ConstantTrue));
        assert!(is_constant_op(DPNOpType::ConstantFalse));
        assert!(is_constant_op(DPNOpType::ConstantU32));
        assert!(!is_constant_op(DPNOpType::Add));
        assert!(!is_constant_op(DPNOpType::Eq));
    }

    #[test]
    fn to_felts_flattens_tuples_and_structs_in_declaration_order() {
        let mut ctx = QExecContext::new();
        let tuple = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Tuple {
            type_id: TypeId::from(0usize),
            elements: vec![(TypeId::from(1usize), felt_value(1)), (TypeId::from(2usize), felt_value(2))],
        });
        assert_eq!(tuple.to_felts().len(), 2);

        let mut fields = IndexMap::new();
        fields.insert(Identifier::new(IdentId(0), Location::default()), felt_value(5));
        fields.insert(Identifier::new(IdentId(1), Location::default()), felt_value(6));
        let structure = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Struct(TypeId::from(3usize), fields));
        assert_eq!(structure.to_felts().len(), 2);
        assert_eq!(structure.type_id(), TypeId::from(3usize));

        // Type values carry no data: VOID encodes as no felts at all.
        let void_value = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Type(VOID_TYPE));
        assert_eq!(void_value.as_type(), Some(VOID_TYPE));
        assert!(void_value.to_felts().is_empty());
        assert!(felt_value(0).as_type().is_none());
    }

    /// A symbolic (non-constant) index expression: built from an op so the
    /// op type is `Add`, exercising the conditional select paths rather than
    /// the constant-index fast path.
    fn symbolic_index(ctx: &mut QExecContext) -> SymFeltRef {
        let one = ctx.op_const(1);
        let two = ctx.op_const(1);
        ctx.op_add(one, two)
    }

    #[test]
    fn u32_array_accessors_round_trip() {
        let mut ctx = QExecContext::new();
        let type_id = TypeId::from(7usize);
        let array = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            type_id.clone(),
            vec![
                CheckedValueRef::from_u32(ctx.op_const_u32(3)),
                CheckedValueRef::from_u32(ctx.op_const_u32(4)),
            ],
        ));
        assert_eq!(array.to_u32_vec().len(), 2);
        let restored: [SymFeltRef; 2] = array.to_u32_array();
        assert_eq!(restored.len(), 2);
        assert_eq!(array.type_id(), type_id);
    }

    #[test]
    fn select_recombines_scalars_containers_and_short_circuits_on_identity() {
        let mut ctx = QExecContext::new();
        let condition = ctx.op_true();
        let select_fn = move |ctx: &mut QExecContext, n: &SymFeltRef, o: &SymFeltRef| ctx.op_select(condition.clone(), n.clone(), o.clone());

        // Identical values short-circuit before any recombination.
        let same = felt_value(1);
        assert_eq!(CheckedValueRef::select(&mut ctx, &same, &same.clone(), &select_fn), same);

        let merged = CheckedValueRef::select(&mut ctx, &felt_value(2), &felt_value(1), &select_fn);
        assert!(merged.borrow().is_felt());

        let bool_merged = CheckedValueRef::select(&mut ctx, &bool_value(true), &bool_value(false), &select_fn);
        assert!(bool_merged.borrow().is_bool());
        let u32_merged = CheckedValueRef::select(&mut ctx, &from_u32(5), &from_u32(6), &select_fn);
        assert!(u32_merged.borrow().is_u32());

        // Arrays merge element-wise.
        let old_array = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![felt_value(1), felt_value(2)],
        ));
        let new_array = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![felt_value(3), felt_value(4)],
        ));
        let merged_array = CheckedValueRef::select(&mut ctx, &new_array, &old_array, &select_fn);
        let merged_array = merged_array.borrow();
        assert!(merged_array.is_array());
        assert_eq!(merged_array.as_array().unwrap().1.len(), 2);

        // Structs merge field-wise; tuples merge element-wise.
        let struct_of = |a: u64, b: u64| {
            let mut fields = IndexMap::new();
            fields.insert(Identifier::new(IdentId(0), Location::default()), felt_value(a));
            fields.insert(Identifier::new(IdentId(1), Location::default()), felt_value(b));
            CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Struct(TypeId::from(3usize), fields))
        };
        let merged_struct = CheckedValueRef::select(&mut ctx, &struct_of(9, 9), &struct_of(1, 2), &select_fn);
        assert_eq!(merged_struct.to_felts().len(), 2);

        let tuple_of = |a: u64, b: u64| {
            CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Tuple {
                type_id: TypeId::from(4usize),
                elements: vec![(FELT_TYPE, felt_value(a)), (FELT_TYPE, felt_value(b))],
            })
        };
        let merged_tuple = CheckedValueRef::select(&mut ctx, &tuple_of(7, 7), &tuple_of(1, 2), &select_fn);
        assert_eq!(merged_tuple.to_felts().len(), 2);

        // Mismatched variant pairs are a programming error, not a silent fallthrough.
        let result = std::panic::catch_unwind(|| {
            let mut ctx = QExecContext::new();
            CheckedValueRef::<SymFeltRef>::select(&mut ctx, &felt_value(1), &bool_value(true), &|_ctx, n: &SymFeltRef, _o: &SymFeltRef| n.clone())
        });
        assert!(result.is_err(), "select of mismatched variants must panic");
    }

    fn from_u32(value: u32) -> CheckedValueRef<SymFeltRef> {
        let mut ctx = QExecContext::new();
        CheckedValueRef::from_u32(ctx.op_const_u32(value))
    }

    #[test]
    fn set_path_with_symbolic_index_builds_conditioned_writes() {
        let mut ctx = QExecContext::new();
        let mut array = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![felt_value(10), felt_value(20)],
        ));

        // A symbolic index cannot pick a slot, so every slot is written under
        // an index condition and combined with a select.
        let index = symbolic_index(&mut ctx);
        array
            .set_path(&mut ctx, &[IndexPath::Felt(index)], &mut vec![], felt_value(99))
            .expect("symbolic index write must build a conditioned write");
        assert_eq!(array.to_felts().len(), 2);

        // Nested symbolic writes accumulate conditions down the path.
        let mut grid = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![
                CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
                    TypeId::from(0usize),
                    vec![felt_value(1), felt_value(2)],
                )),
                CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
                    TypeId::from(0usize),
                    vec![felt_value(3), felt_value(4)],
                )),
            ],
        ));
        let outer = symbolic_index(&mut ctx);
        let inner = symbolic_index(&mut ctx);
        grid.set_path(&mut ctx, &[IndexPath::Felt(outer), IndexPath::Felt(inner)], &mut vec![], felt_value(77))
            .expect("nested symbolic writes must combine conditions");
        assert_eq!(grid.to_felts().len(), 4);

        // Constant indices keep the fast path.
        let constant = ctx.op_const(0);
        let mut direct = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![felt_value(1), felt_value(2)],
        ));
        direct.set_path(&mut ctx, &[IndexPath::Felt(constant)], &mut vec![], felt_value(42)).unwrap();
        let zero = ctx.op_const(0);
        let first = direct.get_path(&mut ctx, &[IndexPath::Felt(zero)]).unwrap();
        assert_eq!(first.to_felt(), felt_value(42).to_felt());

        // Struct and tuple paths are addressed by identifier / position.
        let mut fields = IndexMap::new();
        fields.insert(Identifier::new(IdentId(0), Location::default()), felt_value(1));
        let mut structure = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Struct(TypeId::from(3usize), fields));
        structure
            .set_path(&mut ctx, &[IndexPath::Normal(0)], &mut vec![], felt_value(8))
            .unwrap();
        assert_eq!(
            structure.get_path(&mut ctx, &[IndexPath::Normal(0)]).unwrap().to_felt(),
            felt_value(8).to_felt()
        );

        let mut tuple = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Tuple {
            type_id: TypeId::from(4usize),
            elements: vec![(FELT_TYPE, felt_value(1))],
        });
        tuple.set_path(&mut ctx, &[IndexPath::Normal(0)], &mut vec![], felt_value(6)).unwrap();
        assert_eq!(tuple.get_path(&mut ctx, &[IndexPath::Normal(0)]).unwrap().to_felt(), felt_value(6).to_felt());
    }

    #[test]
    fn get_path_with_symbolic_index_selects_across_elements() {
        let mut ctx = QExecContext::new();
        let array = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(0usize),
            vec![felt_value(11), felt_value(22), felt_value(33)],
        ));

        let index = symbolic_index(&mut ctx);
        let selected = array
            .get_path(&mut ctx, &[IndexPath::Felt(index)])
            .expect("symbolic index read must produce a selected value");
        assert!(selected.borrow().is_felt());
    }


    #[test]
    fn scalar_constructors_and_accessors_round_trip() {
        let v = SymFeltRef::from(7u32);

        let felt = CheckedValueRef::from_felt(v);
        assert_eq!(felt.to_felt(), v);
        assert_eq!(felt.type_id(), FELT_TYPE);
        assert_eq!(felt.to_value(), v);

        let boolean = CheckedValueRef::from_bool(v);
        assert_eq!(boolean.to_bool(), v);
        assert_eq!(boolean.type_id(), BOOL_TYPE);
        assert_eq!(boolean.to_value(), v);

        let word = CheckedValueRef::from_u32(v);
        assert_eq!(word.to_u32(), v);
        assert_eq!(word.type_id(), U32_TYPE);
        assert_eq!(word.to_value(), v);

        assert_eq!(CheckedValueRef::from_value(v, FELT_TYPE).to_felt(), v);
        assert_eq!(CheckedValueRef::from_value(v, BOOL_TYPE).to_bool(), v);
        assert_eq!(CheckedValueRef::from_value(v, U32_TYPE).to_u32(), v);
    }

    #[test]
    fn array_accessors_read_felt_and_u32_elements() {
        let v = SymFeltRef::from(5u32);

        let felts = CheckedValueRef::from_vec(TypeId::from(9usize), [v, v]);
        assert_eq!(felts.to_vec(), vec![v, v]);
        assert_eq!(felts.to_array::<2>(), [v, v]);
        assert_eq!(felts.type_id(), TypeId::from(9usize));

        let words = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Array(
            TypeId::from(10usize),
            vec![CheckedValueRef::from_u32(v), CheckedValueRef::from_u32(v)],
        ));
        assert_eq!(words.to_u32_vec(), vec![v, v]);
        assert_eq!(words.to_u32_array::<2>(), [v, v]);
    }

    #[test]
    fn type_values_share_state_and_render_empty() {
        let ty = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Type(VOID_TYPE));
        assert_eq!(ty.as_type(), Some(VOID_TYPE));
        assert_eq!(ty.type_id(), VOID_TYPE);
        assert!(ty.to_felts().is_empty());
        let _shared = ty.as_rc();

        let mut value = CheckedValueRef::<SymFeltRef>::from_felt(SymFeltRef::from(1u32));
        *value.borrow_mut() = CheckedValue::Bool(SymFeltRef::from(1u32));
        assert_eq!(value.to_bool(), SymFeltRef::from(1u32));
    }

    #[test]
    fn to_felts_and_encode_felts_flatten_composites() {
        let v = SymFeltRef::from(4u32);

        let tuple = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Tuple {
            type_id: TypeId::from(1usize),
            elements: vec![
                (FELT_TYPE, CheckedValueRef::from_felt(v)),
                (BOOL_TYPE, CheckedValueRef::from_bool(v)),
            ],
        });
        assert_eq!(tuple.to_felts(), vec![v, v]);
        assert_eq!(tuple.type_id(), TypeId::from(1usize));

        let mut fields = IndexMap::new();
        fields.insert(
            psy_ast::Identifier::new(psy_ast::IdentId(1), psy_ast::Location::default()),
            CheckedValueRef::from_felt(v),
        );
        let structure = CheckedValueRef::<SymFeltRef>::new_rc(CheckedValue::Struct(TypeId::from(2usize), fields));
        assert_eq!(structure.to_felts(), vec![v]);
        assert_eq!(structure.type_id(), TypeId::from(2usize));

        let encoded = <CheckedValueRef<SymFeltRef> as FeltRepr<SymFeltRef, QExecContext>>::encode_felts(&CheckedValueRef::from_felt(v));
        assert_eq!(encoded, vec![v]);
    }

    #[test]
    fn decode_felts_rebuilds_scalars_and_structs() {
        let mut context = TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(psy_ast::Program::new());
        let a = SymFeltRef::from(11u32);
        let b = SymFeltRef::from(22u32);

        let felt_ty = context.symbols.create_type(Type::Felt).unwrap();
        let bool_ty = context.symbols.create_type(Type::Bool).unwrap();
        let u32_ty = context.symbols.create_type(Type::U32).unwrap();
        assert_eq!(CheckedValueRef::decode_felts(&[a], &context, felt_ty).to_felt(), a);
        assert_eq!(CheckedValueRef::decode_felts(&[a], &context, bool_ty).to_bool(), a);
        assert_eq!(CheckedValueRef::decode_felts(&[a], &context, u32_ty).to_u32(), a);

        let mut fields = IndexMap::new();
        fields.insert(
            psy_ast::Identifier::new(psy_ast::IdentId(10), psy_ast::Location::default()),
            CheckedStructField::new(felt_ty, vec![], psy_ast::Visibility::Public, vec![], psy_ast::Location::default()),
        );
        fields.insert(
            psy_ast::Identifier::new(psy_ast::IdentId(11), psy_ast::Location::default()),
            CheckedStructField::new(felt_ty, vec![], psy_ast::Visibility::Public, vec![], psy_ast::Location::default()),
        );
        let struct_ty = context
            .symbols
            .create_type(Type::Struct(CheckedStructNode {
                name: psy_ast::Identifier::new(psy_ast::IdentId(12), psy_ast::Location::default()),
                generic_parameters: vec![],
                fields,
                scope_id: crate::ScopeId::root(),
                attrs: vec![],
                visibility: psy_ast::Visibility::Public,
                comments: vec![],
                location: psy_ast::Location::default(),
                type_id: TypeId::from(0usize),
            }))
            .unwrap();
        let decoded = CheckedValueRef::decode_felts(&[a, b], &context, struct_ty);
        assert_eq!(decoded.to_felts(), vec![a, b]);
        assert_eq!(decoded.type_id(), struct_ty);
    }

    #[test]
    fn decode_felts_rebuilds_arrays_and_tuples() {
        let mut context = TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(psy_ast::Program::new());
        let a = SymFeltRef::from(11u32);
        let b = SymFeltRef::from(22u32);

        let felt_ty = context.symbols.create_type(Type::Felt).unwrap();
        let array_ty = context
            .symbols
            .create_type(Type::Array(CheckedArrayNode {
                inner_ty: felt_ty,
                size_ty: felt_ty,
                scope_id: crate::ScopeId::root(),
            }))
            .unwrap();
        let decoded = CheckedValueRef::decode_felts(&[a, b], &context, array_ty);
        assert_eq!(decoded.to_vec(), vec![a, b]);
        assert_eq!(decoded.type_id(), array_ty);

        let tuple_ty = context.symbols.create_type(Type::Tuple(vec![felt_ty, felt_ty])).unwrap();
        let tuple = CheckedValueRef::decode_felts(&[a, b], &context, tuple_ty);
        assert_eq!(tuple.to_felts(), vec![a, b]);
        assert_eq!(tuple.type_id(), tuple_ty);
    }
    #[test]
    #[should_panic(expected = "Expected felt value")]
    fn to_felt_panics_on_a_bool_value() {
        bool_value(true).to_felt();
    }

    #[test]
    #[should_panic(expected = "Expected bool value")]
    fn to_bool_panics_on_a_felt_value() {
        felt_value(1).to_bool();
    }

    #[test]
    #[should_panic(expected = "Expected u32 value")]
    fn to_u32_panics_on_a_felt_value() {
        felt_value(1).to_u32();
    }

    #[test]
    #[should_panic(expected = "Expected felt/u32/bool value")]
    fn to_value_panics_on_an_array_value() {
        let mut context = TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(psy_ast::Program::new());
        let felt_ty = context.symbols.create_type(Type::Felt).unwrap();
        let array = CheckedValueRef::<SymFeltRef>::from_vec(felt_ty, [SymFeltRef::from(1u32)]);
        array.to_value();
    }

    #[test]
    #[should_panic(expected = "Expected array value")]
    fn to_vec_panics_on_a_felt_value() {
        felt_value(1).to_vec();
    }

    #[test]
    #[should_panic(expected = "Expected array value")]
    fn to_u32_vec_panics_on_a_felt_value() {
        felt_value(1).to_u32_vec();
    }
}
