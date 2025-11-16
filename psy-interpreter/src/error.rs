use core::fmt;
use std::io::Error as IoError;

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
    // #[error("index out of bounds")]
    // IndexOutOfBounds,
    // #[error("type mismatch")]
    // TypeMismatch,
}

pub type Result<T> = std::result::Result<T, Error>;

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
        ariadne::FnCache::new(|x: &String| std::fs::read_to_string(std::path::Path::new(x.as_str()))),
        &mut output,
    )?;

    Ok(String::from_utf8(output).unwrap())
}

pub fn lowering_parse_error<F: Clone + From<u32> + ContextFelt>(error: &psy_parser::Error, program: &Program<F>) -> String {
    match error {
        ParseError::LexicalError(error) => format!("{}", error),
        ParseError::CommonError(error) => format!("{}", error),
        ParseError::IoError(error) => format!("{}", error),
        ParseError::FileUnresolved => format!("{}", error),
        ParseError::FileParsedMultipleTimes(path) => format!("{}", error),
        ParseError::NoEntryModule(path) => format!("{}", error),
        ParseError::InvalidModuleName => format!("{}", error),
        ParseError::ExternFnNotInStd => format!("{}", error),
        ParseError::FunctionBodyMissing => format!("{}", error),
        ParseError::InvalidSelfParameter => format!("{}", error),
        ParseError::InvalidToken { location } => {
            build_report(location.clone(), "InvalidToken", "Invalid Token.", program).unwrap_or_else(|e| format!("Failed to build report: {}", e))
        }
        ParseError::UnrecognizedEof { expected, location } => {
            build_report(location.clone(), "UnrecognizedEof", format!("Expected {:?}.", expected), program)
                .unwrap_or_else(|e| format!("Failed to build report: {}", e))
        }
        ParseError::UnrecognizedToken { token, expected, location } => build_report(
            location.clone(),
            "UnrecognizedToken",
            format!("Found unrecognized token {}, expected {:?}.", token, expected,),
            program,
        )
        .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
        ParseError::ExtraToken { token, location } => build_report(location.clone(), "ExtraToken", format!("Extra token {} found.", token), program)
            .unwrap_or_else(|e| format!("Failed to build report: {}", e)),
    }
}

fn span_to_range(location: &Location, source: &str) -> TextRange {
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

    let (range, file, message) = match error {
        InvalidToken { location } | UnrecognizedEof { location, .. } | UnrecognizedToken { location, .. } | ExtraToken { location, .. } => {
            let message = match error {
                InvalidToken { .. } => "Invalid token".to_string(),
                UnrecognizedEof { expected, .. } => {
                    format!("Unexpected EOF. Expected one of: {}", format_expected_pretty(expected))
                }
                UnrecognizedToken { token, expected, .. } => {
                    format!("Unrecognized token '{}', expected one of: {}", token, format_expected_pretty(expected),)
                }
                ExtraToken { token, .. } => format!("Extra token '{}'", token),
                _ => unreachable!(),
            };

            // Convert location to LSP range
            let file_content = program.file_resolver.resolve_content(&location.file_id).unwrap_or_default();

            let range = span_to_range(location, file_content);
            let file_path = program.file_resolver.resolve_path(&location.file_id).cloned();
            (Some(range), file_path, message)
        }

        // Other errors without location info
        other => (None, None, format!("{other}")),
    };
    TypeCheckerErrorDescriptor {
        file,
        text_range: range,
        message,
    }
}
fn format_expected_pretty(expected: &[String]) -> String {
    if expected.is_empty() {
        return "(no expected tokens)".to_string();
    }

    let items: Vec<String> = expected
        .iter()
        .map(|s| s.trim_matches('"').replace("\\\"", "\""))
        .map(|s| s.to_string())
        .collect();

    match items.len() {
        1 => items[0].clone(),
        2 => format!("{} or {}", items[0], items[1]),
        _ => {
            let all_but_last = &items[..items.len() - 1];
            let last = &items[items.len() - 1];
            format!("{} or {}", all_but_last.join(", "), last)
        }
    }
}
pub fn lowering_sema_error<F: Clone + From<u32> + ContextFelt, C>(error: &psy_sema::Error, ctx: &TypeCheckerVisitorContext<F, C>) -> String {
    match error {
        SemaError::AnyhowError(error) => format!("{}", error),
        SemaError::CommonError(error) => format!("{}", error),
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
            let file = ctx.program.file_resolver.resolve_path(&location.file_id).cloned();
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
            ctx.program.file_resolver.resolve_path(&location.file_id).cloned(),
            format!("Invalid path segment: {}", segment),
        ),

        SemaError::UnresolvedType { location, resolved_type } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id).cloned(),
            format!("Unresolved type: {}", ctx.ident(*resolved_type)),
        ),

        SemaError::VariableAlreadyDefined { location, variable } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id).cloned(),
            format!("Variable already defined: {}", ctx.ident(*variable)),
        ),

        SemaError::ImmutableVariable { location, variable } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id).cloned(),
            format!("Variable {} is immutable", ctx.ident(*variable)),
        ),

        SemaError::InvalidReturn { location, message } => (
            Some(span_to_range(
                location,
                ctx.program.file_resolver.resolve_content(&location.file_id).unwrap_or_default(),
            )),
            ctx.program.file_resolver.resolve_path(&location.file_id).cloned(),
            format!("Invalid return: {}", message),
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
                ctx.program.file_resolver.resolve_path(&location.file_id).cloned(),
                label,
            )
        }

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
    };

    anyhow::Error::from(error).context(context)
}
