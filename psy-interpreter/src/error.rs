use core::fmt;
use std::{io::Error as IoError, path::Path};

use ariadne::{Label, Report, ReportKind};
use psy_ast::{Location, Program, TextPosition, TextRange, VisitorContext};
use psy_parser::Error as ParseError;
use psy_sema::{AstVisualizer, Error as SemaError, TypeCheckerErrorDescriptor, TypeCheckerVisitorContext};
use psy_vm::dpn::ops::context_trait::ContextFelt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("parse error: {0}")]
    ParseError(#[from] ParseError),
    #[error("io error: {0}")]
    IoError(#[from] IoError),
    #[error("sema error: {0}")]
    SemaError(#[from] SemaError),
    #[error("undefined function")]
    UndefinedFunction,
    #[error("uncertain loop condition")]
    UncertainLoopCondition { loop_location: Location },
    #[error("assertion failure: {message}")]
    AssertionFailure { message: String, location: Option<Location> },
    #[error("DivisionByZero: division or remainder by zero")]
    DivisionByZero { location: Option<Location> },
    #[error("ArithmeticOverflow: constant arithmetic overflow")]
    ArithmeticOverflow { location: Option<Location> },
    #[error("IndexOutOfBounds: index {index} >= length {length}")]
    IndexOutOfBounds { index: usize, length: usize, location: Option<Location> },
    #[error("ArrayTooLarge: cannot materialize an array with {length} elements (limit: {limit})")]
    ArrayTooLarge { length: u64, limit: u64, location: Option<Location> },
    #[error("ArrayAllocationFailed: cannot reserve storage for {length} array elements")]
    ArrayAllocationFailed { length: usize, location: Option<Location> },
    #[error("UnsupportedRecursion: recursive function calls cannot be interpreted")]
    UnsupportedRecursion { location: Option<Location> },
    // #[error("type mismatch")]
    // TypeMismatch,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

fn build_report<F: Clone + From<u32> + ContextFelt>(
    location: Location,
    code: impl fmt::Display,
    message: impl fmt::Display,
    program: &Program<F>,
) -> Result<String> {
    let file_location = program.convert_location(&location);
    let report = Report::build(ReportKind::Error, file_location.clone())
        .with_code(code)
        .with_label(Label::new(file_location).with_message(message))
        .finish();

    let mut output = Vec::new();

    report.write(
        ariadne::FnCache::new(|x: &String| {
            program
                .file_resolver
                .resolve_path_content(Path::new(x))
                .map(|source| source.to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("missing source for {x}")))
        }),
        &mut output,
    )?;

    Ok(String::from_utf8(output).unwrap())
}

pub fn lowering_parse_error<F: Clone + From<u32> + ContextFelt>(error: &psy_parser::Error, program: &Program<F>) -> String {
    match error {
        ParseError::CommonError(error) => format!("{}", error),
        ParseError::IoError(error) => format!("{}", error),
        ParseError::FileUnresolved => format!("{}", error),
        ParseError::FileParsedMultipleTimes(path) => format!("{}", path.display()),
        ParseError::NoEntryModule(path) => format!("{}", path.display()),
        ParseError::InvalidModuleName
        | ParseError::ExternFnNotInStd
        | ParseError::FunctionBodyMissing
        | ParseError::InvalidSelfParameter => format!("{}", error),
        ParseError::UnexpectedEof { expected, location } => {
            build_report(*location, "UnexpectedEof", format!("Expected {:?}.", expected), program)
                .unwrap_or_else(|e| format!("Failed to build report: {}", e))
        }
        ParseError::UnexpectedToken { found, expected, location } => build_report(
            *location,
            "UnexpectedToken",
            format!("Found unexpected token {}, expected {:?}.", found, expected),
            program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        ParseError::UnsupportedSyntax { feature, location } => build_report(
            *location,
            "UnsupportedSyntax",
            format!("Unsupported syntax: {}.", feature),
            program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        ParseError::LexicalError { location } => build_report(*location, "LexError", "Lexical error.", program)
            .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
    }
}

fn span_to_range(location: &Location, source: impl AsRef<str>) -> TextRange {
    let source = source.as_ref();
    fn offset_to_text_position(offset: usize, text: &str) -> TextPosition {
        let mut line = 0;
        let mut current_offset = 0;

        for l in text.lines() {
            let line_len = l.len() + 1; // assuming '\n', not covering Windows '\r\n' here
            if current_offset + line_len > offset {
                let character = offset - current_offset;
                return TextPosition {
                    line,
                    character: character as u32,
                };
            }

            current_offset += line_len;
            line += 1;
        }

        // Fallback: offset past end of file
        TextPosition { line, character: 0 }
    }

    TextRange {
        start: offset_to_text_position(location.start, source),
        end: offset_to_text_position(location.end, source),
    }
}
pub fn parse_error_to_diagnostic<F: Clone + From<u32> + ContextFelt>(error: &ParseError, program: &Program<F>) -> TypeCheckerErrorDescriptor {
    use ParseError::*;

    let located = match error {
        UnexpectedEof { expected, location } => Some((
            location,
            format!("Unexpected EOF. Expected one of: {}", format_expected_pretty(expected)),
        )),
        UnexpectedToken { found, expected, location } => Some((
            location,
            format!("Unexpected token '{}', expected one of: {}", found, format_expected_pretty(expected)),
        )),
        UnsupportedSyntax { feature, location } => Some((location, format!("Unsupported syntax: {}", feature))),
        LexicalError { location } => Some((location, "Lexical error".to_string())),
        _ => None,
    };

    let (range, file, message) = if let Some((location, message)) = located {
        let file_content = program.file_resolver.resolve_content(&location.file_id).unwrap_or_default();
        let range = span_to_range(location, file_content);
        let file_path = program.file_resolver.resolve_path(&location.file_id);
        (Some(range), file_path, message)
    } else {
        (None, None, format!("{error}"))
    };

    TypeCheckerErrorDescriptor {
        file,
        text_range: range,
        message,
    }
}

fn format_expected_pretty(expected: &[psy_parser::error::ExpectedToken]) -> String {
    if expected.is_empty() {
        return "(no expected tokens)".to_string();
    }

    let items = expected.iter().map(ToString::to_string).collect::<Vec<_>>();
    match items.len() {
        1 => items[0].clone(),
        2 => format!("{} or {}", items[0], items[1]),
        _ => format!("{} or {}", items[..items.len() - 1].join(", "), items.last().unwrap()),
    }
}
pub fn lowering_sema_error<F: Clone + From<u32> + ContextFelt, C>(error: &psy_sema::Error, ctx: &TypeCheckerVisitorContext<F, C>) -> String {
    match error {
        SemaError::AnyhowError(error) => format!("{}", error),
        SemaError::CommonError(error) => format!("{}", error),
        SemaError::UnsupportedRecursion { location, what } => build_report(
            location.clone(),
            "UnsupportedRecursion",
            format!("Unsupported recursion: {what}."),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::TypeMismatch { location, expected, found } => build_report(
            location.clone(),
            "TypeMismatch",
            format!(
                "Expected {}, but found {}.",
                expected.into_iter().map(|ty| ctx.debug_type(ty.clone())).collect::<Vec<_>>().join(","),
                ctx.debug_type(found.clone())
            ),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::InvalidPathSegment { location, segment } => build_report(
            location.clone(),
            "InvalidPathSegment",
            format!("Invalid path segment {}.", segment),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::UnresolvedType { location, resolved_type } => build_report(
            location.clone(),
            "UnresolvedType",
            format!("Unresolved type {}.", ctx.ident(resolved_type.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::TraitAlreadyImplemented { location, trait_ty, ty } => build_report(
            location.clone(),
            "TraitAlreadyImplemented",
            format!(
                "Trait {} already implemented for {}.",
                ctx.debug_type(trait_ty.clone()),
                ctx.debug_type(ty.clone())
            ),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::VariableAlreadyDefined { location, variable } => build_report(
            location.clone(),
            "VariableAlreadyDefined",
            format!("Variable {} already defined.", ctx.ident(variable.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::ImmutableVariable { location, variable } => build_report(
            location.clone(),
            "ImmutableVariable",
            format!("Variable {} is immutable.", ctx.ident(variable.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::UnresolvedMember { location, member_name } => build_report(
            location.clone(),
            "UnresolvedMember",
            format!("Unresolved member {}.", ctx.ident(member_name.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::NotCallable { location, ty } => build_report(
            location.clone(),
            "NotCallable",
            format!("Type {} is not callable.", ctx.debug_type(ty.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::UnresolvedTraitMethod {
            method_location,
            method_name,
            trait_name,
        } => build_report(
            method_location.clone(),
            "UnresolvedTraitMethod",
            format!(
                "Unresolved trait method {} in trait {}.",
                ctx.ident(method_name.clone()),
                ctx.ident(trait_name.clone())
            ),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::InvalidGenericArguments { location, expected, found } => build_report(
            location.clone(),
            "GenericParameterMismatch",
            format!("Expected {}, but found {}.", expected, found),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::InvalidFunctionArguments {
            location,
            method_name: _method_name,
            expected,
            found,
        } => build_report(
            location.clone(),
            "InvalidFunctionCall",
            format!("Expected {} parameters, but found {}.", expected, found),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::InvalidReturn { location, message } => {
            build_report(location.clone(), "InvalidReturn", message, &ctx.program).unwrap_or_else(|e| format!("Failed to build report: {}", e))
        }
        SemaError::InvalidGenericConstraint { location } => build_report(
            location.clone(),
            "InvalidGenericConstraint",
            "Generic constraint should either be a concrete type or a list of trait requirements",
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::UnreachableExpression { location } => {
            build_report(location.clone(), "UnreachableExpression", "Unreachable Expression.", &ctx.program)
                .unwrap_or_else(|e| format!("Failed to build report: {}", e))
        }
        SemaError::TypeAlreadyDefined { location, type_name } => build_report(
            location.clone(),
            "TypeAlreadyDefined",
            format!("Type {} already defined.", ctx.ident(type_name.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::MemberNotPublic { location, ty, field } => build_report(
            location.clone(),
            "MemberNotPublic",
            format!("{} not a public member of {}.", ctx.ident(field.clone()), ctx.debug_type(ty.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::ModuleNotPublic { location, module } => build_report(
            location.clone(),
            "ModuleNotPublic",
            format!("{} not a public module.", ctx.ident(module.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::TypeNotPublic { location, ty } => build_report(
            location.clone(),
            "TypeNotPublic",
            format!("{} not public.", ctx.debug_type(ty.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::IndexOutOfBounds { location, index, length } => build_report(
            location.clone(),
            "IndexOutOfBounds",
            format!("Index {} Out Of Bounds {}.", index, length),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::InvalidCast { location, expected, found } => build_report(
            location.clone(),
            "InvalidCast",
            format!("Expected {}, but found {}.", expected, found),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::DuplicateWildcard { location } => build_report(location.clone(), "DuplicateWildcard", "Duplicate Wildcard.", &ctx.program)
            .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::IncompleteMatch { location, message } => {
            build_report(location.clone(), "IncompleteMatch", message, &ctx.program).unwrap_or_else(|e| format!("Failed to build report: {}", e))
        }
        SemaError::NoParentModule { location } => build_report(location.clone(), "NoParentModule", "No parent module.", &ctx.program)
            .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::ModuleNotFound { location, module } => build_report(
            location.clone(),
            "ModuleNotFound",
            format!("Module {} not found.", ctx.ident(module.clone())),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::SpecializationNotAllowed { location } => {
            build_report(location.clone(), "SpecializationNotAllowed", "Specialization not allowed.", &ctx.program)
                .unwrap_or_else(|e| format!("Failed to build report: {}", e))
        }
        SemaError::MissingAssociatedType {
            location,
            trait_name,
            type_name,
        } => build_report(
            location.clone(),
            "MissingAssociatedType",
            format!(
                "Missing associated type {} for {}.",
                ctx.ident(type_name.clone()),
                ctx.ident(trait_name.clone())
            ),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::RawIntrinsicOutsideStd { location, name } => build_report(
            *location,
            "RawIntrinsicOutsideStd",
            format!("Raw intrinsic `{name}` is only available inside the std module tree; use a public std API instead."),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::IfWithoutElse { location } => build_report(
            *location,
            "IfWithoutElse",
            "if expression without else branch cannot be used as a value: the result is undefined when the condition is false.",
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::AmbiguousTraitMethod { location, method, traits } => build_report(
            *location,
            "AmbiguousTraitMethod",
            format!(
                "Ambiguous trait method `{}`: multiple trait impls provide it ({}). Disambiguate with `<T as Trait>::method()`.",
                ctx.ident(*method),
                traits.iter().map(|ty| ctx.debug_type(ty.clone())).collect::<Vec<_>>().join(", ")
            ),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        SemaError::AmbiguousAssociatedType { location, member, traits } => build_report(
            *location,
            "AmbiguousAssociatedType",
            format!(
                "Ambiguous associated type `{}`: multiple trait impls provide it ({}). Disambiguate with `<T as Trait>::Type`.",
                ctx.ident(*member),
                traits.iter().map(|ty| ctx.debug_type(*ty)).collect::<Vec<_>>().join(", ")
            ),
            &ctx.program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
    }
}
pub fn typecheck_error_to_diagnostic<F: Clone + From<u32> + ContextFelt, C>(
    error: &psy_sema::Error,
    ctx: &TypeCheckerVisitorContext<F, C>,
) -> TypeCheckerErrorDescriptor {
    use psy_sema::Error as SemaError;

    let (range, file, message) = match error {
        SemaError::TypeMismatch { location, expected, found } => {
            let msg = format!(
                "Type mismatch. Expected {}, found {}.",
                expected.iter().map(|ty| ctx.debug_type(ty.clone())).collect::<Vec<_>>().join(", "),
                ctx.debug_type(found.clone())
            );
            let file = ctx.program.file_resolver.resolve_path(&location.file_id);
            (
                Some(span_to_range(
                    location,
                    ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
                )),
                file,
                msg,
            )
        }

        SemaError::InvalidPathSegment { location, segment } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id),
            format!("Invalid path segment: {}", segment),
        ),

        SemaError::UnresolvedType { location, resolved_type } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id),
            format!("Unresolved type: {}", ctx.ident(*resolved_type)),
        ),

        SemaError::VariableAlreadyDefined { location, variable } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id),
            format!("Variable already defined: {}", ctx.ident(*variable)),
        ),

        SemaError::ImmutableVariable { location, variable } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id),
            format!("Variable {} is immutable", ctx.ident(*variable)),
        ),

        SemaError::InvalidReturn { location, message } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id),
            format!("Invalid return: {}", message),
        ),

        SemaError::RawIntrinsicOutsideStd { location, name } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id),
            format!("Raw intrinsic `{name}` is only available inside the std module tree; use a public std API instead."),
        ),

        SemaError::NoParentModule { location }
        | SemaError::UnreachableExpression { location }
        | SemaError::InvalidGenericConstraint { location }
        | SemaError::DuplicateWildcard { location }
        | SemaError::SpecializationNotAllowed { location } => {
            let label = format!("{error}");
            (
                Some(span_to_range(
                    location,
                    ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
                )),
                ctx.program.file_resolver.resolve_path(&location.file_id),
                label,
            )
        }

        SemaError::InvalidCast { location, expected, found } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id),
            format!("Invalid cast. Expected {expected}, found {found}."),
        ),
        _ => (None, None, format!("{error}")),
    };

    TypeCheckerErrorDescriptor {
        file,
        text_range: range,
        message,
    }
}

pub fn lowering_interpreter_error<F: Clone + From<u32> + ContextFelt, C>(error: Error, ctx: &TypeCheckerVisitorContext<F, C>) -> anyhow::Error {
    let context = match &error {
        Error::ParseError(error) => lowering_parse_error(error, &ctx.program),
        Error::IoError(error) => format!("{}", error),
        Error::SemaError(error) => lowering_sema_error(error, ctx),
        Error::UndefinedFunction => format!("{}", error),
        Error::UncertainLoopCondition { loop_location } => {
            build_report(loop_location.clone(), "UncertainLoopCondition", "Uncertain Loop Condition", &ctx.program)
                .unwrap_or_else(|e| format!("Failed to build report: {}", e))
        }
        Error::AssertionFailure { message, location } => {
            if let Some(location) = location {
                build_report(location.clone(), "AssertionFailure", message, &ctx.program).unwrap_or_else(|e| format!("Failed to build report: {}", e))
            } else {
                format!("Assertion failure: {}", message)
            }
        }
        Error::DivisionByZero { location } => {
            if let Some(location) = location {
                build_report(location.clone(), "DivisionByZero", "Division or remainder by zero.", &ctx.program)
                    .unwrap_or_else(|e| format!("Failed to build report: {}", e))
            } else {
                format!("{}", error)
            }
        }
        Error::ArithmeticOverflow { location } => {
            if let Some(location) = location {
                build_report(location.clone(), "ArithmeticOverflow", "Arithmetic overflow.", &ctx.program)
                    .unwrap_or_else(|e| format!("Failed to build report: {}", e))
            } else {
                format!("{}", error)
            }
        }
        Error::IndexOutOfBounds { index, length, location } => {
            let msg = format!("Index out of bounds: index {} >= length {}.", index, length);
            if let Some(location) = location {
                build_report(location.clone(), "IndexOutOfBounds", msg, &ctx.program)
                    .unwrap_or_else(|e| format!("Failed to build report: {}", e))
            } else {
                msg
            }
        }
        Error::ArrayTooLarge { length, limit, location } => {
            let msg = format!("Cannot materialize an array with {} elements; the interpreter limit is {}.", length, limit);
            if let Some(location) = location {
                build_report(*location, "ArrayTooLarge", msg, &ctx.program).unwrap_or_else(|e| format!("Failed to build report: {}", e))
            } else {
                msg
            }
        }
        Error::ArrayAllocationFailed { length, location } => {
            let msg = format!("Cannot reserve storage for {} array elements.", length);
            if let Some(location) = location {
                build_report(*location, "ArrayAllocationFailed", msg, &ctx.program).unwrap_or_else(|e| format!("Failed to build report: {}", e))
            } else {
                msg
            }
        }
        Error::UnsupportedRecursion { location } => {
            let msg = "Recursive calls are unsupported by the symbolic interpreter.";
            if let Some(location) = location {
                build_report(*location, "UnsupportedRecursion", msg, &ctx.program)
                    .unwrap_or_else(|e| format!("Failed to build report: {}", e))
            } else {
                msg.to_string()
            }
        }
    };

    anyhow::Error::from(error).context(context)
}
