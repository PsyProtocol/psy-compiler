//! Regression coverage for nested generic closers.

use psy_ast::{Identifier, Location, Program, Visibility};
use psy_parser::recursive::{parse_module_into, ParseModuleInput};
use psy_vm::dpn::ops::exec_context::QExecContext;

/// Parse a source snippet through the public recursive parser entry point.
fn parse_module(src: &str) -> Result<psy_ast::ModuleNode, psy_parser::Error> {
    let mut program = Program::new();
    let mut ctx = QExecContext::new();
    let file_id = program.file_resolver.add_file(std::path::PathBuf::from("probe.psy"), src);
    let module_name = Identifier::new(
        program.interner.intern_ident("probe"),
        Location::new(file_id, 0, 0),
    );
    parse_module_into(
        ParseModuleInput {
            source: src,
            file_id,
            module_name,
            visibility: Visibility::Public,
        },
        &mut program,
        &mut ctx,
    )
}

/// Empty generic argument lists are rejected by the syntax migration, so the
/// inner `B<>` in `A<B<>>` is no longer accepted.
#[test]
fn nested_empty_generics_rejected() {
    let src = "fn f() -> A<B<>> { }";
    parse_module(src).expect_err("A<B<>> must error: inner empty generic B<> is rejected");
}

#[test]
fn empty_generic_as_leading_argument_rejected() {
    let src = "fn f() -> A<B<>, C> { }";
    parse_module(src).expect_err("A<B<>, C> must error: leading empty generic B<> is rejected");
}

#[test]
fn nested_empty_generics_in_struct_field_rejected() {
    let src = "struct S { x: A<B<>> }";
    parse_module(src).expect_err("struct field of type A<B<>> must error: empty generic B<> is rejected");
}

#[test]
fn nested_nonempty_generics_parse() {
    let src = "fn f() -> A<B<C>> { }";
    parse_module(src).expect("A<B<C>> must parse");
}

#[test]
fn top_level_empty_generics_rejected() {
    let src = "fn f() -> A<> { }";
    parse_module(src).expect_err("A<> must error: empty generic argument list is rejected");
}

#[test]
fn trailing_comma_in_generics_parse() {
    let src = "fn f() -> A<T,> { }";
    parse_module(src).expect("A<T,> must parse");
}

#[test]
fn deep_nested_generics_parse() {
    let src = "fn f() -> A<B<C, D>> { }";
    parse_module(src).expect("A<B<C, D>> must parse");
}

/// A following `>>` remains a shift operator after parsing a generic type.
#[test]
fn shift_after_generic_type_annotation_parses() {
    let src = "fn f() { let x: A<B> = y >> 2; }";
    parse_module(src).expect(">> after a generic type annotation must parse as a shift expression");
}

/// An unclosed generic must return an error rather than panic.
#[test]
fn unclosed_generic_reports_error() {
    let src = "fn f() -> A<B { }";
    parse_module(src).expect_err("unclosed generic A<B must error");
}