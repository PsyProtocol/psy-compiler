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
            Type::Unknown => IdentId::TYPE_UNKNOWN,
            Type::VOID => IdentId::TYPE_VOID,
            Type::Felt => IdentId::TYPE_FELT,
            Type::Bool => IdentId::TYPE_BOOL,
            Type::U32 => IdentId::TYPE_U32,
            Type::Struct(CheckedStructNode { name, .. }) => name.id,
            Type::Enum(CheckedEnumNode { name, .. }) => name.id,
            Type::Function(CheckedFunctionNode { name, .. }) => name.id,
            Type::Trait(CheckedTraitNode { name, .. }) => name.id,
            Type::Const(CheckedConstNode { name, .. }) => name.map(|n| n.id).unwrap_or(IdentId::TYPE_UNKNOWN),
            Type::Array(_) => IdentId::TYPE_ARRAY,
            Type::Tuple(_) => IdentId::TYPE_TUPLE,
            Type::LambdaFunction(CheckedLambdaFunctionNode { name, .. }) => name.id,
            Type::FunctionSignature(_) => IdentId::TYPE_UNKNOWN,
            Type::TypeVariable(CheckedGenericParameter { name, .. }) => name.clone(),
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
        self.try_signature().expect("signature requested for a non-callable type")
    }

    pub fn try_signature(&self) -> Option<CheckedFunctionSignature> {
        match self {
            Type::Function(f) => Some(f.signature()),
            Type::LambdaFunction(l) => Some(l.signature()),
            Type::FunctionSignature(s) => Some(s.clone()),
            _ => None,
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

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use psy_ast::{Comment, DefId, ExprId, IdentId, Location, Visibility};

    use super::*;
    use crate::{CheckedStructField, ConstId};

    fn ident(id: usize) -> Identifier {
        Identifier::new(IdentId(id), Location::default())
    }

    fn located(offset: usize) -> Location {
        Location::new(psy_common::FileId(0), offset, offset + 1)
    }

    fn array() -> CheckedArrayNode {
        CheckedArrayNode { inner_ty: TypeId(5), size_ty: TypeId(6), scope_id: ScopeId(1) }
    }

    fn enumeration() -> CheckedEnumNode {
        CheckedEnumNode {
            name: ident(20),
            generic_parameters: vec![TypeId(7)],
            variants: vec![crate::CheckedEnumVariant::Basic(ident(21), TypeId(5))],
            scope_id: ScopeId(2),
            visibility: Visibility::Private,
            comments: vec![],
            location: located(10),
        }
    }

    fn function() -> CheckedFunctionNode {
        CheckedFunctionNode {
            name: ident(22),
            parameters: vec![CheckedFunctionParameter {
                name: ident(23),
                qualifier: psy_ast::TypeQualifier::new(false, Location::default()),
                ty: TypeId(5),
                path: None,
                location: Location::default(),
            }],
            generic_parameters: vec![TypeId(8)],
            body: Some(ExprId(3)),
            qualifier: psy_ast::Qualifier::default(),
            return_type: TypeId(9),
            return_type_path: None,
            scope_id: ScopeId(3),
            visibility: Visibility::Private,
            attrs: vec![],
            type_id: TypeId(40),
            comments: vec![],
            location: located(20),
        }
    }

    fn lambda() -> CheckedLambdaFunctionNode {
        CheckedLambdaFunctionNode {
            name: ident(24),
            parameters: vec![CheckedFunctionParameter {
                name: ident(25),
                qualifier: psy_ast::TypeQualifier::new(false, Location::default()),
                ty: TypeId(5),
                path: None,
                location: Location::default(),
            }],
            body: ExprId(4),
            return_type: TypeId(9),
            return_type_path: None,
            scope_id: ScopeId(4),
            type_id: TypeId(41),
            location: located(30),
        }
    }

    fn trait_node() -> CheckedTraitNode {
        CheckedTraitNode {
            name: ident(26),
            associated_types: IndexMap::new(),
            generic_parameters: vec![TypeId(10)],
            body: vec![DefId(1)],
            unchecked_body: vec![],
            scope_id: ScopeId(5),
            visibility: Visibility::Private,
            comments: vec![],
            location: located(40),
            type_id: TypeId(42),
        }
    }

    fn konst(named: bool) -> CheckedConstNode {
        CheckedConstNode {
            name: named.then(|| ident(27)),
            ty: TypeId(5),
            value: ConstId(9),
            visibility: Visibility::Private,
            scope_id: ScopeId(6),
        }
    }

    fn struct_node() -> crate::CheckedStructNode {
        let mut fields = IndexMap::new();
        fields.insert(
            ident(28),
            CheckedStructField::new(TypeId(5), vec![], Visibility::Public, vec![], located(50)),
        );
        crate::CheckedStructNode {
            name: ident(29),
            generic_parameters: vec![TypeId(11)],
            fields,
            scope_id: ScopeId(7),
            attrs: vec![],
            visibility: Visibility::Private,
            comments: vec![Comment::new_line("doc".to_string(), Location::default())],
            location: located(60),
            type_id: TypeId(43),
        }
    }

    #[test]
    fn keys_describe_every_composite_shape() {
        let key = Type::Array(array()).key();
        assert_eq!(key.name, Some(IdentId::TYPE_ARRAY));
        assert_eq!(key.generic_parameters, vec![TypeId(5), TypeId(6)]);

        let key = Type::Struct(struct_node()).key();
        assert_eq!(key.name, Some(IdentId(29)));
        assert_eq!(key.generic_parameters, vec![TypeId(11)]);

        let key = Type::Enum(enumeration()).key();
        assert_eq!(key.name, Some(IdentId(20)));
        assert_eq!(key.generic_parameters, vec![TypeId(7)]);

        let key = Type::Tuple(vec![TypeId(5), TypeId(6)]).key();
        assert_eq!(key.name, Some(IdentId::TYPE_TUPLE));
        assert_eq!(key.parameters, vec![TypeId(5), TypeId(6)]);

        let key = Type::Function(function()).key();
        assert_eq!(key.name, Some(IdentId(22)));
        assert_eq!(key.generic_parameters, vec![TypeId(8)]);
        assert_eq!(key.parameters, vec![TypeId(5)]);
        assert_eq!(key.return_type, Some(TypeId(9)));

        let key = Type::LambdaFunction(lambda()).key();
        assert_eq!(key.name, Some(IdentId(24)));
        assert_eq!(key.parameters, vec![TypeId(5)]);
        assert_eq!(key.return_type, Some(TypeId(9)));

        // A VOID return type is normalized away in the key of a bare signature.
        let key = Type::FunctionSignature(CheckedFunctionSignature {
            parameters: vec![TypeId(5)],
            return_type: VOID_TYPE,
        })
        .key();
        assert_eq!(key.name, None);
        assert_eq!(key.return_type, None);

        // Any other return type is preserved.
        let key = Type::FunctionSignature(CheckedFunctionSignature {
            parameters: vec![],
            return_type: TypeId(12),
        })
        .key();
        assert_eq!(key.return_type, Some(TypeId(12)));

        let key = Type::Trait(trait_node()).key();
        assert_eq!(key.name, Some(IdentId(26)));
        assert_eq!(key.generic_parameters, vec![TypeId(10)]);

        let key = Type::Const(konst(true)).key();
        assert_eq!(key.name, Some(IdentId(27)));

        let key = Type::Const(konst(false)).key();
        assert_eq!(key.name, None);
    }

    #[test]
    #[should_panic(expected = "Type::key called on TypeVariable type")]
    fn key_panics_for_type_variables() {
        let variable = CheckedGenericParameter::new(IdentId(30), vec![], ScopeId(8), Location::default());
        let _ = Type::TypeVariable(variable).key();
    }

    fn empty_signature() -> CheckedFunctionSignature {
        CheckedFunctionSignature { parameters: vec![], return_type: VOID_TYPE }
    }

    #[test]
    fn scope_ids_come_from_the_node_for_composites() {
        // The primitive scope global may not have been populated yet when this test runs first.
        #[allow(static_mut_refs)]
        unsafe {
            if crate::STD_PRIMITIVE_SCOPE_ID.get().is_none() {
                crate::STD_PRIMITIVE_SCOPE_ID.set(ScopeId(99)).unwrap();
            }
        }

        assert_eq!(Type::Array(array()).scope_id(), ScopeId(1));
        assert_eq!(Type::Tuple(vec![]).scope_id(), ScopeId::primitive());
        assert_eq!(Type::Struct(struct_node()).scope_id(), ScopeId(7));
        assert_eq!(Type::Enum(enumeration()).scope_id(), ScopeId(2));
        assert_eq!(Type::Function(function()).scope_id(), ScopeId(3));
        assert_eq!(Type::Trait(trait_node()).scope_id(), ScopeId(5));
        assert_eq!(Type::Const(konst(true)).scope_id(), ScopeId(6));
        assert_eq!(Type::LambdaFunction(lambda()).scope_id(), ScopeId(4));

        let variable = CheckedGenericParameter::new(IdentId(31), vec![], ScopeId(9), Location::default());
        assert_eq!(Type::TypeVariable(variable).scope_id(), ScopeId(9));

        for primitive in [Type::Felt, Type::Bool, Type::U32] {
            assert_eq!(primitive.scope_id(), ScopeId::primitive());
        }
    }

    #[test]
    #[should_panic(expected = "Type::scope_id called on non-composite type")]
    fn scope_id_panics_for_bare_signatures() {
        let _ = Type::FunctionSignature(empty_signature()).scope_id();
    }

    #[test]
    fn visibility_defaults_to_public_for_primitive_kinds() {
        assert_eq!(Type::Struct(struct_node()).visibility(), Visibility::Private);
        assert_eq!(Type::Enum(enumeration()).visibility(), Visibility::Private);
        assert_eq!(Type::Function(function()).visibility(), Visibility::Private);
        assert_eq!(Type::Trait(trait_node()).visibility(), Visibility::Private);
        assert_eq!(Type::Const(konst(true)).visibility(), Visibility::Private);
        assert_eq!(Type::Felt.visibility(), Visibility::Public);
        assert_eq!(Type::Tuple(vec![]).visibility(), Visibility::Public);
    }

    #[test]
    fn names_identify_primitives_and_composites() {
        assert_eq!(Type::Unknown.name(), IdentId::TYPE_UNKNOWN);
        assert_eq!(Type::VOID.name(), IdentId::TYPE_VOID);
        assert_eq!(Type::Felt.name(), IdentId::TYPE_FELT);
        assert_eq!(Type::Bool.name(), IdentId::TYPE_BOOL);
        assert_eq!(Type::U32.name(), IdentId::TYPE_U32);
        assert_eq!(Type::Array(array()).name(), IdentId::TYPE_ARRAY);
        assert_eq!(Type::Tuple(vec![]).name(), IdentId::TYPE_TUPLE);
        assert_eq!(Type::Struct(struct_node()).name(), IdentId(29));
        assert_eq!(Type::Enum(enumeration()).name(), IdentId(20));
        assert_eq!(Type::Function(function()).name(), IdentId(22));
        assert_eq!(Type::Trait(trait_node()).name(), IdentId(26));
        assert_eq!(Type::Const(konst(true)).name(), IdentId(27));
        assert_eq!(Type::Const(konst(false)).name(), IdentId::TYPE_UNKNOWN);
        assert_eq!(Type::LambdaFunction(lambda()).name(), IdentId(24));
        assert_eq!(Type::FunctionSignature(empty_signature()).name(), IdentId::TYPE_UNKNOWN);

        let variable = CheckedGenericParameter::new(IdentId(32), vec![], ScopeId(8), Location::default());
        assert_eq!(Type::TypeVariable(variable).name(), IdentId(32));
    }

    #[test]
    fn bodies_belong_to_functions_and_lambdas() {
        assert_eq!(Type::Function(function()).body(), Some(ExprId(3)));
        assert_eq!(Type::LambdaFunction(lambda()).body(), Some(ExprId(4)));
        assert_eq!(Type::Felt.body(), None);
        assert_eq!(Type::Struct(struct_node()).body(), None);
    }

    #[test]
    fn generic_parameters_cover_arrays_structs_enums_functions_and_traits() {
        assert_eq!(Type::Array(array()).generic_parameters(), vec![TypeId(5), TypeId(6)]);
        assert_eq!(Type::Struct(struct_node()).generic_parameters(), vec![TypeId(11)]);
        assert_eq!(Type::Enum(enumeration()).generic_parameters(), vec![TypeId(7)]);
        assert_eq!(Type::Function(function()).generic_parameters(), vec![TypeId(8)]);
        assert_eq!(Type::Trait(trait_node()).generic_parameters(), vec![TypeId(10)]);
        assert_eq!(Type::Felt.generic_parameters(), vec![]);
    }

    #[test]
    fn parameters_are_available_for_callables() {
        let params = Type::Function(function()).parameters();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].ty, TypeId(5));

        let params = Type::LambdaFunction(lambda()).parameters();
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn signatures_resolve_for_callables_and_fail_for_others() {
        assert_eq!(Type::Function(function()).signature().parameters, vec![TypeId(5)]);
        assert_eq!(Type::LambdaFunction(lambda()).signature().return_type, TypeId(9));

        let signature = CheckedFunctionSignature { parameters: vec![TypeId(6)], return_type: TypeId(9) };
        assert_eq!(Type::FunctionSignature(signature.clone()).try_signature(), Some(signature));
        assert_eq!(Type::Felt.try_signature(), None);
    }

    #[test]
    fn kinds_mirror_every_variant() {
        assert_eq!(Type::Unknown.kind(), TypeKind::Unknown);
        assert_eq!(Type::VOID.kind(), TypeKind::VOID);
        assert_eq!(Type::Felt.kind(), TypeKind::Felt);
        assert_eq!(Type::Bool.kind(), TypeKind::Bool);
        assert_eq!(Type::U32.kind(), TypeKind::U32);
        assert_eq!(Type::Array(array()).kind(), TypeKind::Array);
        assert_eq!(Type::Struct(struct_node()).kind(), TypeKind::Struct);
        assert_eq!(Type::Enum(enumeration()).kind(), TypeKind::Enum);
        assert_eq!(Type::Tuple(vec![]).kind(), TypeKind::Tuple);
        assert_eq!(Type::Function(function()).kind(), TypeKind::Function);
        assert_eq!(Type::Trait(trait_node()).kind(), TypeKind::Trait);
        assert_eq!(Type::Const(konst(true)).kind(), TypeKind::Const);
        assert_eq!(Type::LambdaFunction(lambda()).kind(), TypeKind::LambdaFunction);
        assert_eq!(Type::FunctionSignature(empty_signature()).kind(), TypeKind::FunctionSignature);
        assert_eq!(
            Type::TypeVariable(CheckedGenericParameter::new(IdentId(33), vec![], ScopeId(8), Location::default())).kind(),
            TypeKind::TypeVariable
        );
    }

    #[test]
    fn locations_default_for_primitives_and_come_from_nodes_for_composites() {
        assert_eq!(Type::Felt.location(), Location::default());
        assert_eq!(Type::Array(array()).location(), Location::default());
        assert_eq!(Type::Tuple(vec![]).location(), Location::default());
        assert_eq!(Type::Const(konst(true)).location(), Location::default());
        assert_eq!(
            Type::FunctionSignature(empty_signature()).location(),
            Location::default()
        );

        assert_eq!(Type::Struct(struct_node()).location(), located(60));
        assert_eq!(Type::Enum(enumeration()).location(), located(10));
        assert_eq!(Type::Function(function()).location(), located(20));
        assert_eq!(Type::Trait(trait_node()).location(), located(40));
        assert_eq!(Type::LambdaFunction(lambda()).location(), located(30));

        let variable = CheckedGenericParameter::new(IdentId(34), vec![], ScopeId(8), located(70));
        assert_eq!(Type::TypeVariable(variable).location(), located(70));
    }

    #[test]
    fn type_ids_below_ten_are_std_types() {
        assert!(TypeId(0).is_std_type());
        assert!(TypeId(9).is_std_type());
        assert!(!TypeId(10).is_std_type());
    }

    #[test]
    fn type_keys_convert_from_idents_and_consts() {
        let from_id = TypeKey::from(IdentId(5));
        assert_eq!(from_id.name, Some(IdentId(5)));

        let from_identifier = TypeKey::from(ident(6));
        assert_eq!(from_identifier.name, Some(IdentId(6)));

        let from_ref = TypeKey::from(&ident(7));
        assert_eq!(from_ref.name, Some(IdentId(7)));

        let from_const = TypeKey::from(ConstId(8));
        assert_eq!(from_const.consts, vec![ConstId(8)]);
        assert_eq!(from_const.name, None);
    }
}
