use std::path::PathBuf;

use psy_ast::{IdentId, Location, TextRange};
use thiserror::Error;

use crate::TypeId;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0:?}")]
    AnyhowError(#[from] anyhow::Error),
    #[error("{0:?}")]
    CommonError(#[from] psy_common::Error),
    #[error("type mismatch")]
    TypeMismatch {
        location: Location,
        expected: Vec<TypeId>,
        found: TypeId,
    },
    #[error("invalid path segment")]
    InvalidPathSegment { location: Location, segment: String },
    #[error("unresolved type")]
    UnresolvedType { location: Location, resolved_type: IdentId },
    #[error("variable already defined")]
    VariableAlreadyDefined { location: Location, variable: IdentId },
    #[error("immutable variable")]
    ImmutableVariable { location: Location, variable: IdentId },
    #[error("unresolved member")]
    UnresolvedMember { location: Location, member_name: IdentId },
    #[error("unresolved trait method")]
    UnresolvedTraitMethod {
        method_location: Location,
        method_name: IdentId,
        trait_name: IdentId,
    },
    #[error("invalid generic arguments")]
    InvalidGenericArguments { location: Location, expected: String, found: String },
    #[error("invalid function arguments")]
    InvalidFunctionArguments {
        location: Location,
        method_name: TypeId,
        expected: String,
        found: String,
    },
    #[error("invalid return")]
    InvalidReturn { location: Location, message: String },
    #[error("invalid generic constraint")]
    InvalidGenericConstraint { location: Location },
    #[error("unreachable expression")]
    UnreachableExpression { location: Location },
    #[error("type already defined")]
    TypeAlreadyDefined { location: Location, type_name: IdentId },
    #[error("member not public")]
    MemberNotPublic { location: Location, ty: TypeId, field: IdentId },
    #[error("module not public")]
    ModuleNotPublic { location: Location, module: IdentId },
    #[error("type not public")]
    TypeNotPublic { location: Location, ty: TypeId },
    #[error("trait already implemented")]
    TraitAlreadyImplemented { location: Location, trait_ty: TypeId, ty: TypeId },
    #[error("missing associated type")]
    MissingAssociatedType {
        location: Location,
        trait_name: IdentId,
        type_name: IdentId,
    },
    #[error("index out of bounds")]
    IndexOutOfBounds { location: Location, index: usize, length: usize },
    #[error("invalid cast")]
    InvalidCast { location: Location, expected: String, found: String },
    #[error("no parent module")]
    NoParentModule { location: Location },
    #[error("module not found: module: {module:?}, location: {location:?}")]
    ModuleNotFound { location: Location, module: IdentId },
    #[error("unreachable code")]
    DuplicateWildcard { location: Location },
    #[error("Incomplete Match")]
    IncompleteMatch { location: Location, message: String },
    #[error("Specialization not allowed")]
    SpecializationNotAllowed { location: Location },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct TypeCheckerErrorDescriptor {
    pub file: Option<PathBuf>,
    pub text_range: Option<TextRange>,
    pub message: String,
}
#[derive(Debug)]
pub enum TypeCheckError {
    Parse(TypeCheckerErrorDescriptor),
    TypeCheck(TypeCheckerErrorDescriptor),
    Cycle(String),
    StoragePreprocess(String),
}
