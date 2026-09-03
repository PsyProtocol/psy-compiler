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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use psy_ast::{Location, Program};
    use psy_common::FileId;
    use psy_parser::error::ExpectedToken;
    use psy_vm::dpn::ops::sym_felt::SymFeltRef;
    use psy_sema::{Type, TypeCheckerVisitorContext, TypeId};
    use psy_vm::dpn::ops::exec_context::QExecContext;

    use super::{
        format_expected_pretty, lowering_interpreter_error, lowering_parse_error, lowering_sema_error, parse_error_to_diagnostic,
        span_to_range, typecheck_error_to_diagnostic, Error,
    };

    #[test]
    fn expected_token_formatting_handles_all_list_lengths() {
        assert_eq!(format_expected_pretty(&[]), "(no expected tokens)");
        assert_eq!(format_expected_pretty(&[ExpectedToken::Ident]), "identifier");
        assert_eq!(format_expected_pretty(&[ExpectedToken::Ident, ExpectedToken::Literal]), "identifier or literal");
        assert_eq!(
            format_expected_pretty(&[ExpectedToken::Ident, ExpectedToken::Literal, ExpectedToken::Eof]),
            "identifier, literal or end of file"
        );
    }

    #[test]
    fn span_conversion_handles_start_end_cross_line_and_eof_offsets() {
        let location = Location::new(FileId(0), 0, 8);
        let range = span_to_range(&location, "first\nsecond");
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 1);
        assert_eq!(range.end.character, 2);

        let eof = span_to_range(&Location::new(FileId(0), 100, 100), "short");
        assert_eq!(eof.start.line, 1);
        assert_eq!(eof.start.character, 0);
    }

    #[test]
    fn non_located_parse_errors_lower_to_plain_text() {
        let program = Program::<SymFeltRef>::new();
        let error = psy_parser::Error::InvalidModuleName;
        assert_eq!(lowering_parse_error(&error, &program), "Invalid module name");

        let error = psy_parser::Error::NoEntryModule(PathBuf::from("missing.psy"));
        assert_eq!(lowering_parse_error(&error, &program), "missing.psy");
    }

    #[test]
    fn interpreter_errors_without_locations_have_stable_messages() {
        let ctx = TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(Program::new());
        let cases = [
            (Error::UndefinedFunction, "undefined function"),
            (Error::AssertionFailure { message: "failed".into(), location: None }, "Assertion failure: failed"),
            (Error::DivisionByZero { location: None }, "DivisionByZero: division or remainder by zero"),
            (Error::ArithmeticOverflow { location: None }, "ArithmeticOverflow: constant arithmetic overflow"),
            (Error::IndexOutOfBounds { index: 3, length: 2, location: None }, "Index out of bounds: index 3 >= length 2."),
            (Error::ArrayTooLarge { length: 10, limit: 5, location: None }, "Cannot materialize an array with 10 elements; the interpreter limit is 5."),
            (Error::ArrayAllocationFailed { length: 10, location: None }, "Cannot reserve storage for 10 array elements."),
            (Error::UnsupportedRecursion { location: None }, "Recursive calls are unsupported by the symbolic interpreter."),
        ];
        for (error, expected) in cases {
            let rendered = lowering_interpreter_error(error, &ctx).to_string();
            assert!(rendered.contains(expected), "expected {expected:?} in {rendered:?}");
        }
    }

    #[test]
    fn interpreter_errors_with_locations_render_source_reports() {
        let mut program = Program::<SymFeltRef>::new();
        let file_id = program.file_resolver.add_file(PathBuf::from("error.psy"), "fn main() {}\n");
        let location = Location::new(file_id, 0, 2);
        let ctx = TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(program);
        let cases = [
            (Error::UncertainLoopCondition { loop_location: location }, "UncertainLoopCondition"),
            (Error::AssertionFailure { message: "failed".into(), location: Some(location) }, "AssertionFailure"),
            (Error::DivisionByZero { location: Some(location) }, "DivisionByZero"),
            (Error::ArithmeticOverflow { location: Some(location) }, "ArithmeticOverflow"),
            (Error::IndexOutOfBounds { index: 3, length: 2, location: Some(location) }, "IndexOutOfBounds"),
            (Error::ArrayTooLarge { length: 10, limit: 5, location: Some(location) }, "ArrayTooLarge"),
            (Error::ArrayAllocationFailed { length: 10, location: Some(location) }, "ArrayAllocationFailed"),
            (Error::UnsupportedRecursion { location: Some(location) }, "UnsupportedRecursion"),
        ];
        for (error, code) in cases {
            let rendered = lowering_interpreter_error(error, &ctx).to_string();
            assert!(rendered.contains(code), "expected report code {code:?} in {rendered:?}");
            assert!(rendered.contains("error.psy"), "expected source path in {rendered:?}");
        }
    }

    #[test]
    fn parser_errors_become_diagnostics_with_and_without_locations() {
        let mut program = Program::<SymFeltRef>::new();
        let file_id = program.file_resolver.add_file(PathBuf::from("parse.psy"), "fn main() {}\n");
        let location = Location::new(file_id, 0, 2);

        let eof = parse_error_to_diagnostic(
            &psy_parser::Error::UnexpectedEof {
                expected: vec![ExpectedToken::Ident, ExpectedToken::Literal],
                location,
            },
            &program,
        );
        assert_eq!(eof.file.unwrap(), PathBuf::from("parse.psy"));
        assert!(eof.message.contains("identifier or literal"));
        assert!(eof.text_range.is_some());

        let token = parse_error_to_diagnostic(
            &psy_parser::Error::UnexpectedToken {
                found: "}".into(),
                expected: vec![ExpectedToken::Ident],
                location,
            },
            &program,
        );
        assert!(token.message.contains("Unexpected token '}'"));
        assert!(token.text_range.is_some());

        let unsupported = parse_error_to_diagnostic(
            &psy_parser::Error::UnsupportedSyntax {
                feature: "legacy syntax".into(),
                location,
            },
            &program,
        );
        assert_eq!(unsupported.message, "Unsupported syntax: legacy syntax");

        let lexical = parse_error_to_diagnostic(&psy_parser::Error::LexicalError { location }, &program);
        assert_eq!(lexical.message, "Lexical error");

        let plain = parse_error_to_diagnostic(&psy_parser::Error::InvalidModuleName, &program);
        assert!(plain.file.is_none());
        assert_eq!(plain.message, "Invalid module name");
    }

    #[test]
    fn located_parse_errors_render_source_reports() {
        let mut program = Program::<SymFeltRef>::new();
        let file_id = program.file_resolver.add_file(PathBuf::from("parse.psy"), "fn main() {}\n");
        let location = Location::new(file_id, 0, 2);

        let cases: Vec<(psy_parser::Error, &str)> = vec![
            (
                psy_parser::Error::UnexpectedEof {
                    expected: vec![ExpectedToken::Ident],
                    location,
                },
                "UnexpectedEof",
            ),
            (
                psy_parser::Error::UnexpectedToken {
                    found: "}".into(),
                    expected: vec![ExpectedToken::Ident],
                    location,
                },
                "UnexpectedToken",
            ),
            (
                psy_parser::Error::UnsupportedSyntax {
                    feature: "legacy syntax".into(),
                    location,
                },
                "UnsupportedSyntax",
            ),
            (psy_parser::Error::LexicalError { location }, "LexError"),
        ];
        for (error, code) in &cases {
            let rendered = lowering_parse_error(error, &program);
            assert!(rendered.contains(code), "expected report code {code:?} in {rendered:?}");
            assert!(rendered.contains("parse.psy"), "expected source path in {rendered:?}");
        }
    }

    #[test]
    fn every_non_located_parse_error_lowers_to_its_display_form() {
        let program = Program::<SymFeltRef>::new();
        let cases: Vec<(psy_parser::Error, &str)> = vec![
            (
                psy_parser::Error::CommonError(psy_common::Error::Message("common failure".into())),
                "common failure",
            ),
            (psy_parser::Error::IoError(std::io::Error::other("disk full")), "disk full"),
            (psy_parser::Error::FileUnresolved, "File could not be resolved"),
            (
                psy_parser::Error::FileParsedMultipleTimes(PathBuf::from("dup.psy")),
                // This arm lowers to the bare path, not the Display form.
                "dup.psy",
            ),
            (
                psy_parser::Error::ExternFnNotInStd,
                "Extern function can only be defined in std",
            ),
            (psy_parser::Error::FunctionBodyMissing, "Missing function body"),
            (psy_parser::Error::InvalidSelfParameter, "Invalid self parameter"),
        ];
        for (error, expected) in &cases {
            assert_eq!(lowering_parse_error(error, &program), *expected);
        }
    }

    fn sema_error_fixture() -> (TypeCheckerVisitorContext<SymFeltRef, QExecContext>, Location, psy_ast::IdentId, TypeId) {
        let mut program = Program::<SymFeltRef>::new();
        let file_id = program.file_resolver.add_file(PathBuf::from("sema.psy"), "fn main() {}\n");
        let location = Location::new(file_id, 0, 2);
        let mut ctx = TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(program);
        let ident = ctx.program.interner.intern_ident("counter");
        ctx.symbols.types.push(Type::Felt);
        (ctx, location, ident, TypeId::from(0))
    }

    #[test]
    fn every_sema_error_renders_a_located_report() {
        let (mut ctx, location, ident, ty) = sema_error_fixture();

        let cases: Vec<(psy_sema::Error, &str)> = vec![
            (
                psy_sema::Error::UnsupportedRecursion {
                    location,
                    what: "functions",
                },
                "UnsupportedRecursion",
            ),
            (
                psy_sema::Error::TypeMismatch {
                    location,
                    expected: vec![ty],
                    found: ty,
                },
                "TypeMismatch",
            ),
            (
                psy_sema::Error::InvalidPathSegment {
                    location,
                    segment: "::".into(),
                },
                "InvalidPathSegment",
            ),
            (
                psy_sema::Error::UnresolvedType {
                    location,
                    resolved_type: ident,
                },
                "UnresolvedType",
            ),
            (
                psy_sema::Error::TraitAlreadyImplemented {
                    location,
                    trait_ty: ty,
                    ty,
                },
                "TraitAlreadyImplemented",
            ),
            (
                psy_sema::Error::VariableAlreadyDefined {
                    location,
                    variable: ident,
                },
                "VariableAlreadyDefined",
            ),
            (
                psy_sema::Error::ImmutableVariable {
                    location,
                    variable: ident,
                },
                "ImmutableVariable",
            ),
            (
                psy_sema::Error::UnresolvedMember {
                    location,
                    member_name: ident,
                },
                "UnresolvedMember",
            ),
            (psy_sema::Error::NotCallable { location, ty }, "NotCallable"),
            (
                psy_sema::Error::UnresolvedTraitMethod {
                    method_location: location,
                    method_name: ident,
                    trait_name: ident,
                },
                "UnresolvedTraitMethod",
            ),
            (
                psy_sema::Error::InvalidGenericArguments {
                    location,
                    expected: "1".into(),
                    found: "2".into(),
                },
                "GenericParameterMismatch",
            ),
            (
                psy_sema::Error::InvalidFunctionArguments {
                    location,
                    method_name: ty,
                    expected: "1".into(),
                    found: "2".into(),
                },
                "InvalidFunctionCall",
            ),
            (
                psy_sema::Error::InvalidReturn {
                    location,
                    message: "bad return".into(),
                },
                "InvalidReturn",
            ),
            (psy_sema::Error::InvalidGenericConstraint { location }, "InvalidGenericConstraint"),
            (psy_sema::Error::UnreachableExpression { location }, "UnreachableExpression"),
            (
                psy_sema::Error::TypeAlreadyDefined {
                    location,
                    type_name: ident,
                },
                "TypeAlreadyDefined",
            ),
            (
                psy_sema::Error::MemberNotPublic {
                    location,
                    ty,
                    field: ident,
                },
                "MemberNotPublic",
            ),
            (
                psy_sema::Error::ModuleNotPublic {
                    location,
                    module: ident,
                },
                "ModuleNotPublic",
            ),
            (psy_sema::Error::TypeNotPublic { location, ty }, "TypeNotPublic"),
            (
                psy_sema::Error::IndexOutOfBounds {
                    location,
                    index: 3,
                    length: 2,
                },
                "IndexOutOfBounds",
            ),
            (
                psy_sema::Error::InvalidCast {
                    location,
                    expected: "Felt".into(),
                    found: "u32".into(),
                },
                "InvalidCast",
            ),
            (psy_sema::Error::DuplicateWildcard { location }, "DuplicateWildcard"),
            (
                psy_sema::Error::IncompleteMatch {
                    location,
                    message: "missing arms".into(),
                },
                "IncompleteMatch",
            ),
            (psy_sema::Error::NoParentModule { location }, "NoParentModule"),
            (
                psy_sema::Error::ModuleNotFound {
                    location,
                    module: ident,
                },
                "ModuleNotFound",
            ),
            (psy_sema::Error::SpecializationNotAllowed { location }, "SpecializationNotAllowed"),
            (
                psy_sema::Error::MissingAssociatedType {
                    location,
                    trait_name: ident,
                    type_name: ident,
                },
                "MissingAssociatedType",
            ),
            (
                psy_sema::Error::RawIntrinsicOutsideStd {
                    location,
                    name: "__raw_intrinsic",
                },
                "RawIntrinsicOutsideStd",
            ),
            (psy_sema::Error::IfWithoutElse { location }, "IfWithoutElse"),
            (
                psy_sema::Error::AmbiguousTraitMethod {
                    location,
                    method: ident,
                    traits: vec![ty],
                },
                "AmbiguousTraitMethod",
            ),
            (
                psy_sema::Error::AmbiguousAssociatedType {
                    location,
                    member: ident,
                    traits: vec![ty],
                },
                "AmbiguousAssociatedType",
            ),
        ];
        for (error, code) in &cases {
            let rendered = lowering_sema_error(error, &ctx);
            assert!(rendered.contains(code), "expected report code {code:?} in {rendered:?}");
            assert!(rendered.contains("sema.psy"), "expected source path in {rendered:?}");
        }

        // Wrapper variants lower to the wrapped Display form with no location.
        let plain_cases: Vec<(psy_sema::Error, &str)> = vec![
            (psy_sema::Error::AnyhowError(anyhow::anyhow!("sema boom")), "sema boom"),
            (
                psy_sema::Error::CommonError(psy_common::Error::Message("common sema failure".into())),
                "common sema failure",
            ),
        ];
        for (error, needle) in &plain_cases {
            let rendered = lowering_sema_error(error, &ctx);
            assert_eq!(rendered, *needle);
        }
    }

    #[test]
    fn sema_errors_become_diagnostics_with_ranges_or_fallback_messages() {
        let (mut ctx, location, ident, ty) = sema_error_fixture();

        let located_cases: Vec<(psy_sema::Error, &str)> = vec![
            (
                psy_sema::Error::TypeMismatch {
                    location,
                    expected: vec![ty],
                    found: ty,
                },
                "Type mismatch. Expected",
            ),
            (
                psy_sema::Error::InvalidPathSegment {
                    location,
                    segment: "::".into(),
                },
                "Invalid path segment: ::",
            ),
            (
                psy_sema::Error::UnresolvedType {
                    location,
                    resolved_type: ident,
                },
                "Unresolved type: counter",
            ),
            (
                psy_sema::Error::VariableAlreadyDefined {
                    location,
                    variable: ident,
                },
                "Variable already defined: counter",
            ),
            (
                psy_sema::Error::ImmutableVariable {
                    location,
                    variable: ident,
                },
                "Variable counter is immutable",
            ),
            (
                psy_sema::Error::InvalidReturn {
                    location,
                    message: "bad return".into(),
                },
                "Invalid return: bad return",
            ),
            (
                psy_sema::Error::RawIntrinsicOutsideStd {
                    location,
                    name: "__raw_intrinsic",
                },
                "Raw intrinsic `__raw_intrinsic`",
            ),
            (
                psy_sema::Error::InvalidCast {
                    location,
                    expected: "Felt".into(),
                    found: "u32".into(),
                },
                "Invalid cast. Expected Felt, found u32.",
            ),
            (psy_sema::Error::NoParentModule { location }, "no parent module"),
            (psy_sema::Error::UnreachableExpression { location }, "unreachable expression"),
            (psy_sema::Error::InvalidGenericConstraint { location }, "invalid generic constraint"),
            (psy_sema::Error::DuplicateWildcard { location }, "unreachable code"),
            (psy_sema::Error::SpecializationNotAllowed { location }, "Specialization not allowed"),
        ];
        for (error, needle) in &located_cases {
            let diagnostic = typecheck_error_to_diagnostic(error, &ctx);
            assert!(diagnostic.text_range.is_some(), "expected a range for {needle:?}");
            assert_eq!(diagnostic.file.as_ref().unwrap(), &PathBuf::from("sema.psy"));
            assert!(diagnostic.message.contains(needle), "expected {needle:?} in {:?}", diagnostic.message);
        }

        // Variants without a dedicated diagnostic arm fall back to the Display
        // form without a range.
        let fallback = typecheck_error_to_diagnostic(&psy_sema::Error::ModuleNotFound { location, module: ident }, &ctx);
        assert!(fallback.text_range.is_none());
        assert!(fallback.file.is_none());
        assert!(fallback.message.contains("module not found"));
    }

    #[test]
    fn passthrough_interpreter_errors_use_their_lowering_paths() {
        let mut program = Program::<SymFeltRef>::new();
        let file_id = program.file_resolver.add_file(PathBuf::from("pass.psy"), "fn main() {}\n");
        let location = Location::new(file_id, 0, 2);
        let ctx = TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(program);

        let io = lowering_interpreter_error(Error::IoError(std::io::Error::other("disk full")), &ctx).to_string();
        assert!(io.contains("disk full"), "expected io message in {io:?}");

        let parse = lowering_interpreter_error(Error::ParseError(psy_parser::Error::InvalidModuleName), &ctx).to_string();
        assert!(parse.contains("Invalid module name"), "expected parse message in {parse:?}");

        let sema = lowering_interpreter_error(Error::SemaError(psy_sema::Error::NoParentModule { location }), &ctx).to_string();
        assert!(sema.contains("NoParentModule"), "expected sema report in {sema:?}");
        assert!(sema.contains("pass.psy"), "expected source path in {sema:?}");
    }
}
