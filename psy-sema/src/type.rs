use derivative::Derivative;
use enum_as_inner::EnumAsInner;
use psy_ast::{ExprId, IdentId, Identifier, Location, Visibility};
use psy_common::define_arena_id;

use crate::{
    CheckedArrayNode, CheckedConstNode, CheckedEnumNode, CheckedFunctionNode, CheckedFunctionParameter, CheckedFunctionSignature,
    CheckedGenericParameter, CheckedLambdaFunctionNode, CheckedStructNode, CheckedTraitNode, ConstId, ScopeId,
};

define_arena_id!(TypeId);

impl TypeId {
    pub fn is_std_type(&self) -> bool {
        self.0 <= 9
    }
}

pub const UNKOWN_TYPE: TypeId = TypeId(0);
pub const VOID_TYPE: TypeId = TypeId(1);
pub const BOOL_TYPE: TypeId = TypeId(2);
pub const FELT_TYPE: TypeId = TypeId(3);
pub const U32_TYPE: TypeId = TypeId(4);
pub const STORAGE_REF_TYPE: TypeId = TypeId(30);

// pub const T_TYPE: TypeId = TypeId(5);
// pub const N_TYPE: TypeId = TypeId(6);
pub const ARRAY_TYPE: TypeId = TypeId(7);
// pub const HASH_TYPE_LEN: TypeId = TypeId(8);

pub const HASH_TYPE: TypeId = TypeId(9);

use once_cell::sync::Lazy;

pub static PRIMITIVE_TYPES: Lazy<Vec<Type>> = Lazy::new(|| vec![Type::Unknown, Type::VOID, Type::Bool, Type::Felt, Type::U32]);

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum Type {
    Unknown,
    VOID,
    Felt,
    Bool,
    U32,
    Array(CheckedArrayNode),
    Struct(CheckedStructNode),
    Enum(CheckedEnumNode),
    Function(CheckedFunctionNode),
    Trait(CheckedTraitNode),
    Const(CheckedConstNode),
    LambdaFunction(CheckedLambdaFunctionNode),
    FunctionSignature(CheckedFunctionSignature),
    TypeVariable(CheckedGenericParameter),
    Tuple(Vec<TypeId>),
}

#[derive(Copy, Debug, Clone, PartialEq)]
pub enum TypeKind {
    Unknown,
    VOID,

    Felt,
    Bool,
    U32,
    Array,
    Struct,
    Enum,
    Tuple,
    Function,
    Trait,
    Const,

    LambdaFunction,
    FunctionSignature,
    TypeVariable,
}

#[derive(Debug, Clone, Eq, Derivative)]
#[derivative(PartialEq, Hash)]
pub struct TypeKey {
    pub name: Option<IdentId>,
    pub underlying_type_id: Option<TypeId>,
    pub generic_parameters: Vec<TypeId>,
    pub consts: Vec<ConstId>,

    pub parameters: Vec<TypeId>,
    pub return_type: Option<TypeId>,

    #[derivative(PartialEq = "ignore", Hash = "ignore")]
    pub visibility: Visibility,
}

impl TypeKey {
    pub fn new(
        name: Option<IdentId>,
        underlying_type_id: Option<TypeId>,
        generic_parameters: Vec<TypeId>,
        consts: Vec<ConstId>,
        parameters: Vec<TypeId>,
        return_type: Option<TypeId>,
    ) -> Self {
        Self {
            name,
            underlying_type_id,
            generic_parameters,
            consts,
            parameters,
            return_type,
            visibility: Visibility::Public,
        }
    }
}

impl From<IdentId> for TypeKey {
    fn from(value: IdentId) -> Self {
        TypeKey::new(Some(value), None, vec![], vec![], vec![], None)
    }
}

impl From<Identifier> for TypeKey {
    fn from(value: Identifier) -> Self {
        TypeKey::new(Some(value.id), None, vec![], vec![], vec![], None)
    }
}

impl From<&Identifier> for TypeKey {
    fn from(value: &Identifier) -> Self {
        TypeKey::new(Some(value.id), None, vec![], vec![], vec![], None)
    }
}

impl From<ConstId> for TypeKey {
    fn from(value: ConstId) -> Self {
        TypeKey::new(None, None, vec![], vec![value], vec![], None)
    }
}

impl Type {
    pub fn key(&self) -> TypeKey {
        let (name, underlying_type_id, generic_parameters, consts, parameters, return_type) = match self {
            Type::Unknown => (Some(IdentId::TYPE_UNKNOWN), None, vec![], vec![], vec![], None),
            Type::VOID => (Some(IdentId::TYPE_VOID), None, vec![], vec![], vec![], None),
            Type::Felt => (Some(IdentId::TYPE_FELT), None, vec![], vec![], vec![], None),
            Type::Bool => (Some(IdentId::TYPE_BOOL), None, vec![], vec![], vec![], None),
            Type::U32 => (Some(IdentId::TYPE_U32), None, vec![], vec![], vec![], None),
            Type::Array(CheckedArrayNode { inner_ty, size_ty: size, .. }) => (
                Some(IdentId::TYPE_ARRAY),
                None,
                vec![inner_ty.clone(), size.clone()],
                vec![],
                vec![],
                None,
            ),
            Type::Struct(CheckedStructNode {
                name, generic_parameters, ..
            }) => (Some(name.id), None, generic_parameters.clone(), vec![], vec![], None),
            Type::Enum(CheckedEnumNode {
                name, generic_parameters, ..
            }) => (Some(name.id), None, generic_parameters.clone(), vec![], vec![], None),
            Type::Tuple(elements) => (Some(IdentId::TYPE_TUPLE), None, vec![], vec![], elements.clone(), None),
            Type::Function(CheckedFunctionNode {
                name,
                generic_parameters,
                parameters,
                return_type,
                ..
            }) => (
                Some(name.id),
                None,
                generic_parameters.clone(),
                vec![],
                parameters.iter().map(|parameter| parameter.ty.clone()).collect(),
                Some(return_type.clone()),
            ),
            Type::LambdaFunction(CheckedLambdaFunctionNode {
                name,
                parameters,
                return_type,
                ..
            }) => (
                Some(name.id),
                None,
                vec![],
                vec![],
                parameters.iter().map(|parameter| parameter.ty.clone()).collect(),
                Some(return_type.clone()),
            ),
            Type::FunctionSignature(CheckedFunctionSignature { parameters, return_type }) => (
                None,
                None,
                vec![],
                vec![],
                parameters.clone(),
                match return_type {
                    &VOID_TYPE => None,
                    ty => Some(ty.clone()),
                },
            ),
            Type::Trait(CheckedTraitNode {
                name, generic_parameters, ..
            }) => (Some(name.id), None, generic_parameters.clone(), vec![], vec![], None),
            Type::Const(CheckedConstNode { name, .. }) => (name.map(|name| name.id), None, vec![], vec![], vec![], None),
            _ => panic!("Type::key called on TypeVariable type"),
        };
        TypeKey::new(name, underlying_type_id, generic_parameters, consts, parameters, return_type)
    }

    // Note: return the outer scope id for Felt, Bool, U32, Const, TypeVariable
    // but return the inner scope id for Array, Struct, Enum, Function, Trait
    pub fn scope_id(&self) -> ScopeId {
        match self {
            Type::Array(CheckedArrayNode { scope_id, .. }) => *scope_id,
            Type::Tuple(_) => ScopeId::primitive(),
            Type::Struct(CheckedStructNode { scope_id, .. }) => *scope_id,
            Type::Enum(CheckedEnumNode { scope_id, .. }) => *scope_id,
            Type::Function(CheckedFunctionNode { scope_id, .. }) => *scope_id,
            Type::Trait(CheckedTraitNode { scope_id, .. }) => *scope_id,
            Type::Const(CheckedConstNode { scope_id, .. }) => *scope_id,
            Type::LambdaFunction(CheckedLambdaFunctionNode { scope_id, .. }) => *scope_id,
            Type::Felt => ScopeId::primitive(),
            Type::Bool => ScopeId::primitive(),
            Type::U32 => ScopeId::primitive(),
            Type::TypeVariable(CheckedGenericParameter { scope_id, .. }) => *scope_id,
            _ => panic!("Type::scope_id called on non-composite type: {:?}", self),
        }
    }

    pub fn visibility(&self) -> Visibility {
        match self {
            Type::Struct(CheckedStructNode { visibility, .. }) => *visibility,
            Type::Enum(CheckedEnumNode { visibility, .. }) => *visibility,
            Type::Function(CheckedFunctionNode { visibility, .. }) => *visibility,
            Type::Trait(CheckedTraitNode { visibility, .. }) => *visibility,
            Type::Const(CheckedConstNode { visibility, .. }) => *visibility,
            _ => Visibility::Public,
        }
    }

    pub fn name(&self) -> IdentId {
        match self {
            Type::Felt => IdentId::TYPE_FELT,
            Type::Bool => IdentId::TYPE_BOOL,
            Type::U32 => IdentId::TYPE_U32,
            Type::Struct(CheckedStructNode { name, .. }) => name.id,
            Type::Enum(CheckedEnumNode { name, .. }) => name.id,
            Type::Function(CheckedFunctionNode { name, .. }) => name.id,
            Type::Trait(CheckedTraitNode { name, .. }) => name.id,
            // Type::Const(CheckedConstNode { name, .. }) => name.unwrap().id,
            Type::Array(_) => IdentId::TYPE_ARRAY,
            Type::TypeVariable(CheckedGenericParameter { name, .. }) => name.clone(),
            _ => unreachable!(),
        }
    }

    pub fn body(&self) -> Option<ExprId> {
        match self {
            Type::Function(CheckedFunctionNode { body, .. }) => body.clone(),
            Type::LambdaFunction(CheckedLambdaFunctionNode { body, .. }) => Some(*body),
            _ => None,
        }
    }

    pub fn generic_parameters(&self) -> Vec<TypeId> {
        match self {
            Type::Struct(CheckedStructNode { generic_parameters, .. }) => generic_parameters.to_vec(),
            Type::Enum(CheckedEnumNode { generic_parameters, .. }) => generic_parameters.to_vec(),
            Type::Function(CheckedFunctionNode { generic_parameters, .. }) => generic_parameters.to_vec(),
            Type::Trait(CheckedTraitNode { generic_parameters, .. }) => generic_parameters.to_vec(),
            Type::Array(CheckedArrayNode { inner_ty, size_ty: size, .. }) => {
                vec![inner_ty.clone(), size.clone()]
            }
            _ => vec![],
        }
    }

    pub fn parameters(&self) -> Vec<CheckedFunctionParameter> {
        match self {
            Type::Function(CheckedFunctionNode { parameters, .. }) => parameters.to_vec(),
            Type::LambdaFunction(CheckedLambdaFunctionNode { parameters, .. }) => parameters.to_vec(),
            _ => unreachable!(),
        }
    }

    pub fn signature(&self) -> CheckedFunctionSignature {
        match self {
            Type::Function(f) => f.signature(),
            Type::LambdaFunction(l) => l.signature(),
            Type::FunctionSignature(s) => s.clone(),
            _ => unreachable!(),
        }
    }

    pub fn kind(&self) -> TypeKind {
        match self {
            Type::Unknown => TypeKind::Unknown,
            Type::VOID => TypeKind::VOID,
            Type::Felt => TypeKind::Felt,
            Type::Bool => TypeKind::Bool,
            Type::U32 => TypeKind::U32,
            Type::Array(_) => TypeKind::Array,
            Type::Struct(_) => TypeKind::Struct,
            Type::Enum(_) => TypeKind::Enum,
            Type::Tuple(_) => TypeKind::Tuple,
            Type::Function(_) => TypeKind::Function,
            Type::Trait(_) => TypeKind::Trait,
            Type::Const(_) => TypeKind::Const,
            Type::LambdaFunction(_) => TypeKind::LambdaFunction,
            Type::FunctionSignature(_) => TypeKind::FunctionSignature,
            Type::TypeVariable(_) => TypeKind::TypeVariable,
        }
    }

    pub fn location(&self) -> Location {
        match self {
            Type::Unknown => Location::default(),
            Type::VOID => Location::default(),
            Type::Felt => Location::default(),
            Type::Bool => Location::default(),
            Type::U32 => Location::default(),
            Type::Array(_) => Location::default(),
            Type::Struct(struct_node) => struct_node.location,
            Type::Enum(enum_node) => enum_node.location,
            Type::Tuple(_) => Location::default(),
            Type::Function(function_node) => function_node.location,
            Type::Trait(trait_node) => trait_node.location,
            Type::Const(_const_node) => Location::default(),
            Type::LambdaFunction(lambda_function_node) => lambda_function_node.location,
            Type::FunctionSignature(_function_signature) => Location::default(),
            Type::TypeVariable(generic_parameter) => generic_parameter.location,
        }
    }
}
