mod array;
mod r#const;
mod r#enum;
mod function;
mod r#impl;
mod r#struct;
mod r#trait;
mod type_alias;

pub use array::*;
use enum_as_inner::EnumAsInner;
pub use function::*;
use psy_ast::{IdentId, NodeInfo, NodeType, UseNode};
pub use r#const::*;
pub use r#enum::*;
pub use r#impl::*;
pub use r#struct::*;
pub use r#trait::*;
pub use type_alias::*;

use crate::TypeId;

pub type CheckedUseNode = UseNode;

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum CheckedDefinitionNode {
    Function(CheckedFunctionNode),
    Struct(CheckedStructNode),
    Enum(CheckedEnumNode),
    Impl(CheckedImplNode),
    TraitImpl(CheckedTraitImplNode),
    Trait(CheckedTraitNode),
    TypeAlias(CheckedTypeAliasNode),
    Const(CheckedConstNode),
    Use(CheckedUseNode),
}

impl CheckedDefinitionNode {
    pub fn name(&self) -> IdentId {
        match self {
            Self::Function(node) => node.name.id,
            Self::Struct(node) => node.name.id,
            Self::Enum(node) => node.name.id,
            Self::Trait(node) => node.name.id,
            Self::TypeAlias(node) => node.name.id,
            Self::Const(node) => node.name.unwrap().id,
            _ => unreachable!(),
        }
    }

    pub fn type_id(&self) -> TypeId {
        match self {
            Self::Function(node) => node.type_id,
            Self::Struct(node) => node.type_id,
            Self::Trait(node) => node.type_id,
            _ => unreachable!(),
        }
    }
}

impl NodeInfo for CheckedDefinitionNode {
    fn node_type(&self) -> NodeType {
        match self {
            Self::Function(node) => node.node_type(),
            Self::Struct(node) => node.node_type(),
            Self::Enum(node) => node.node_type(),
            Self::Impl(node) => node.node_type(),
            Self::TraitImpl(node) => node.node_type(),
            Self::Trait(node) => node.node_type(),
            Self::TypeAlias(node) => node.node_type(),
            Self::Const(node) => node.node_type(),
            Self::Use(node) => node.node_type(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use indexmap::IndexMap;
    use psy_ast::{
        Comment, DefId, IdentId, Location, NodeType, Qualifier, TypeQualifier, UseNode, Visibility,
    };

    use crate::{ConstId, Identifier, ScopeId, UNKOWN_TYPE};

    use super::*;

    fn ident(id: usize) -> Identifier {
        Identifier::new(IdentId(id), Location::default())
    }

    fn comment() -> Comment {
        Comment::new_line("doc".to_string(), Location::default())
    }

    fn field(ty: TypeId) -> CheckedStructField {
        CheckedStructField::new(ty, vec![], Visibility::Public, vec![], Location::default())
    }

    /// A function whose first parameter and return type are `UNKOWN_TYPE`,
    /// plus one concretely typed parameter.
    fn function_node() -> CheckedFunctionNode {
        CheckedFunctionNode {
            name: ident(1),
            parameters: vec![
                CheckedFunctionParameter::new(
                    ident(2),
                    TypeQualifier::new(false, Location::default()),
                    UNKOWN_TYPE,
                    None,
                    Location::default(),
                ),
                CheckedFunctionParameter::new(
                    ident(3),
                    TypeQualifier::new(false, Location::default()),
                    TypeId(3),
                    None,
                    Location::default(),
                ),
            ],
            generic_parameters: vec![],
            body: None,
            qualifier: Qualifier::default(),
            return_type: UNKOWN_TYPE,
            return_type_path: None,
            scope_id: ScopeId(1),
            visibility: Visibility::Public,
            attrs: vec![],
            type_id: TypeId(11),
            comments: vec![comment()],
            location: Location::default(),
        }
    }

    fn struct_node() -> CheckedStructNode {
        let mut fields = IndexMap::new();
        fields.insert(ident(3), field(TypeId(5)));
        CheckedStructNode {
            name: ident(1),
            generic_parameters: vec![],
            fields,
            scope_id: ScopeId(2),
            attrs: vec![],
            visibility: Visibility::Public,
            comments: vec![comment()],
            location: Location::default(),
            type_id: TypeId(12),
        }
    }

    fn enum_node() -> CheckedEnumNode {
        let mut variant_fields = IndexMap::new();
        variant_fields.insert(ident(4), field(TypeId(6)));
        CheckedEnumNode {
            name: ident(1),
            generic_parameters: vec![TypeId(7)],
            variants: vec![
                CheckedEnumVariant::Basic(ident(2), TypeId(8)),
                CheckedEnumVariant::Tuple(ident(3), vec![TypeId(9), TypeId(10)]),
                CheckedEnumVariant::Struct(ident(4), variant_fields),
            ],
            scope_id: ScopeId(3),
            visibility: Visibility::Public,
            comments: vec![],
            location: Location::default(),
        }
    }

    fn associated_type_value() -> CheckedAssociatedTypeValue {
        CheckedAssociatedTypeValue {
            root: Some(TypeId(13)),
            target: Some(IdentId(5)),
            type_id: TypeId(14),
            visibility: Visibility::Public,
            comments: vec![comment()],
            location: Location::default(),
        }
    }

    fn impl_node() -> CheckedImplNode {
        let mut associated_types = IndexMap::new();
        associated_types.insert(ident(6), associated_type_value());
        CheckedImplNode {
            generic_parameters: vec![],
            associated_types,
            ty: TypeId(15),
            body: vec![DefId(1), DefId(2)],
            scope_id: ScopeId(4),
            comments: vec![],
            location: Location::default(),
        }
    }

    fn trait_impl_node() -> CheckedTraitImplNode {
        let mut associated_types = IndexMap::new();
        associated_types.insert(ident(7), associated_type_value());
        CheckedTraitImplNode {
            generic_parameters: vec![TypeId(16)],
            associated_types,
            trait_ty: TypeId(17),
            ty: TypeId(18),
            body: vec![DefId(3)],
            scope_id: ScopeId(5),
            comments: vec![],
            location: Location::default(),
        }
    }

    fn trait_node() -> CheckedTraitNode {
        let mut associated_types = IndexMap::new();
        associated_types.insert(
            ident(8),
            CheckedAssociatedType {
                type_id: TypeId(19),
                constraints: vec![TypeId(20)],
                visibility: Visibility::Public,
                comments: vec![],
                location: Location::default(),
            },
        );
        CheckedTraitNode {
            name: ident(1),
            associated_types,
            generic_parameters: vec![],
            body: vec![DefId(4)],
            unchecked_body: vec![DefId(5)],
            scope_id: ScopeId(6),
            visibility: Visibility::Public,
            comments: vec![comment()],
            location: Location::default(),
            type_id: TypeId(21),
        }
    }

    fn type_alias_node() -> CheckedTypeAliasNode {
        CheckedTypeAliasNode {
            name: ident(1),
            ty: TypeId(22),
            comments: vec![comment()],
            visibility: Visibility::Public,
        }
    }

    fn const_node() -> CheckedConstNode {
        CheckedConstNode {
            name: Some(ident(1)),
            ty: TypeId(23),
            value: ConstId(9),
            visibility: Visibility::Public,
            scope_id: ScopeId(7),
        }
    }

    fn use_node() -> CheckedUseNode {
        UseNode {
            visibility: Visibility::Public,
            kind: ident(1),
            segments: vec![ident(2), ident(3)],
            target: Some(ident(4)),
            comments: vec![comment()],
            location: Location::default(),
        }
    }

    #[test]
    fn name_extracts_the_identifier_of_named_variants() {
        assert_eq!(CheckedDefinitionNode::Function(function_node()).name(), IdentId(1));
        assert_eq!(CheckedDefinitionNode::Struct(struct_node()).name(), IdentId(1));
        assert_eq!(CheckedDefinitionNode::Enum(enum_node()).name(), IdentId(1));
        assert_eq!(CheckedDefinitionNode::Trait(trait_node()).name(), IdentId(1));
        assert_eq!(CheckedDefinitionNode::TypeAlias(type_alias_node()).name(), IdentId(1));
        assert_eq!(CheckedDefinitionNode::Const(const_node()).name(), IdentId(1));
    }

    #[test]
    #[should_panic(expected = "entered unreachable code")]
    fn name_panics_for_variants_without_a_name() {
        let _ = CheckedDefinitionNode::Impl(impl_node()).name();
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn name_panics_for_anonymous_const() {
        let mut node = const_node();
        node.name = None;
        let _ = CheckedDefinitionNode::Const(node).name();
    }

    #[test]
    fn type_id_extracts_function_struct_and_trait_type_ids() {
        assert_eq!(CheckedDefinitionNode::Function(function_node()).type_id(), TypeId(11));
        assert_eq!(CheckedDefinitionNode::Struct(struct_node()).type_id(), TypeId(12));
        assert_eq!(CheckedDefinitionNode::Trait(trait_node()).type_id(), TypeId(21));
    }

    #[test]
    #[should_panic(expected = "entered unreachable code")]
    fn type_id_panics_for_variants_without_a_type_id() {
        let _ = CheckedDefinitionNode::Enum(enum_node()).type_id();
    }

    #[test]
    fn node_type_reports_every_definition_kind() {
        assert_eq!(CheckedDefinitionNode::Function(function_node()).node_type(), NodeType::FunctionDef);
        assert_eq!(CheckedDefinitionNode::Struct(struct_node()).node_type(), NodeType::StructDef);
        assert_eq!(CheckedDefinitionNode::Enum(enum_node()).node_type(), NodeType::EnumDef);
        assert_eq!(CheckedDefinitionNode::Impl(impl_node()).node_type(), NodeType::ImplDef);
        assert_eq!(CheckedDefinitionNode::TraitImpl(trait_impl_node()).node_type(), NodeType::TraitImplDef);
        assert_eq!(CheckedDefinitionNode::Trait(trait_node()).node_type(), NodeType::TraitDef);
        assert_eq!(CheckedDefinitionNode::TypeAlias(type_alias_node()).node_type(), NodeType::TypeAliasDef);
        assert_eq!(CheckedDefinitionNode::Const(const_node()).node_type(), NodeType::ConstDef);
        assert_eq!(CheckedDefinitionNode::Use(use_node()).node_type(), NodeType::UseDef);
    }

    #[test]
    fn clones_round_trip_and_equality_detects_differences() {
        let function = function_node();
        assert_eq!(function.clone(), function);
        let structure = struct_node();
        assert_eq!(structure.clone(), structure);
        let enumeration = enum_node();
        assert_eq!(enumeration.clone(), enumeration);
        let implementation = impl_node();
        assert_eq!(implementation.clone(), implementation);
        let trait_impl = trait_impl_node();
        assert_eq!(trait_impl.clone(), trait_impl);
        let tr = trait_node();
        assert_eq!(tr.clone(), tr);
        let alias = type_alias_node();
        assert_eq!(alias.clone(), alias);
        let konst = const_node();
        assert_eq!(konst.clone(), konst);
        let use_ = use_node();
        assert_eq!(use_.clone(), use_);
        let array = CheckedArrayNode { inner_ty: TypeId(41), size_ty: TypeId(42), scope_id: ScopeId(8) };
        assert_eq!(array.clone(), array);
        let value = associated_type_value();
        assert_eq!(value.clone(), value);

        let mut altered = struct_node();
        altered.type_id = TypeId(999);
        assert_ne!(structure, altered);
    }

    #[test]
    fn trait_impl_signature_replaces_unknown_types_with_the_implementor() {
        let node = function_node();
        assert_eq!(
            node.signature(),
            CheckedFunctionSignature { parameters: vec![UNKOWN_TYPE, TypeId(3)], return_type: UNKOWN_TYPE }
        );
        assert_eq!(
            node.trait_impl_signature(TypeId(30)),
            CheckedFunctionSignature { parameters: vec![TypeId(30), TypeId(3)], return_type: TypeId(30) }
        );

        let mut concrete_return = function_node();
        concrete_return.return_type = TypeId(40);
        assert_eq!(
            concrete_return.trait_impl_signature(TypeId(30)).return_type,
            TypeId(40),
            "a known return type must not be replaced by the implementor"
        );
    }

    #[test]
    fn signatures_hash_and_compare_by_value() {
        let signature = function_node().signature();
        let mut seen = HashSet::new();
        assert!(seen.insert(signature.clone()));
        assert!(!seen.insert(signature));
    }

    #[test]
    fn parameter_and_field_constructors_preserve_metadata() {
        let parameter = CheckedFunctionParameter::new(
            ident(2),
            TypeQualifier::new(true, Location::default()),
            TypeId(31),
            None,
            Location::default(),
        );
        assert_eq!(parameter.name.id, IdentId(2));
        assert!(parameter.qualifier.is_mutable);
        assert_eq!(parameter.ty, TypeId(31));

        let struct_field = CheckedStructField::new(
            TypeId(32),
            vec![],
            Visibility::Private,
            vec![comment()],
            Location::default(),
        );
        assert_eq!(struct_field.ty, TypeId(32));
        assert_eq!(struct_field.visibility, Visibility::Private);
        assert_eq!(struct_field.comments.len(), 1);
    }
}
