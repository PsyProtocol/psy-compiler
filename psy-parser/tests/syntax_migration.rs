//! Focused coverage for the PSY syntax migration:
//! - path-based struct literals `Type { ... }` (no `new`), qualified/generic.
//! - `new` is no longer syntax: `new Person {}` is rejected, while `new`
//!   remains usable as an identifier (`fn new()`, `let new = ...`,
//!   `Person::new()`).
//! - turbofish call monomorphization `callee::<T>(...)` for free and member
//!   calls, and qualified generic paths `mod::<T>::fn` / `<T as Trait>::fn`.
//! - obsolete `#<...>` monomorphization is rejected.
//! - empty generic argument/parameter lists are rejected.
//! - non-empty nested `>>` generic closers still parse.

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

// ─── Struct literals ───────────────────────────────────────────────────────

#[test]
fn bare_struct_literal_parses() {
    let src = "fn f() { let p = Foo { }; }";
    parse_module(src).expect("Foo { } must parse as a path-based struct literal");
}

#[test]
fn struct_literal_with_fields_parses() {
    let src = "fn f() { let p = Foo { a: 1, b: 2 }; }";
    parse_module(src).expect("Foo { a: 1, b: 2 } must parse as a struct literal");
}

#[test]
fn struct_literal_as_expression_statement_parses() {
    let src = "fn f() { Foo { a: 1 }; }";
    parse_module(src).expect("a bare struct literal statement must parse");
}

#[test]
fn qualified_struct_literal_parses() {
    let src = "fn f() { let p = outer::Foo { a: 1 }; }";
    parse_module(src).expect("outer::Foo { ... } must parse as a qualified struct literal");
}

#[test]
fn generic_struct_literal_bare_parses() {
    let src = "fn f() { let p = Foo<Felt> { a: 1 }; }";
    parse_module(src).expect("Foo<Felt> { ... } must parse as a generic struct literal");
}

#[test]
fn generic_struct_literal_turbofish_parses() {
    let src = "fn f() { let p = Foo::<Felt> { a: 1 }; }";
    parse_module(src).expect("Foo::<Felt> { ... } must parse as a turbofish generic struct literal");
}

#[test]
fn qualified_generic_struct_literal_parses() {
    let src = "fn f() { let p = outer::Foo<Felt> { a: 1 }; }";
    parse_module(src).expect("outer::Foo<Felt> { ... } must parse as a qualified generic struct literal");
}

// ─── `new` is no longer syntax ─────────────────────────────────────────────

#[test]
fn new_struct_syntax_rejected() {
    // `new Person {}` must fail: `new` is no longer a keyword, so it parses as
    // an identifier path followed by a stray `Person` rather than a struct form.
    let src = "struct Person { } fn f() { let p = new Person { }; }";
    parse_module(src).expect_err("new Person {} must be rejected as obsolete syntax");
}

#[test]
fn fn_new_parses() {
    let src = "fn new() { }";
    parse_module(src).expect("fn new() must parse: `new` is a valid function name");
}

#[test]
fn new_as_identifier_parses() {
    let src = "fn f() { let new = 5; }";
    parse_module(src).expect("`new` must be usable as a normal identifier");
}

#[test]
fn person_new_method_call_parses() {
    let src = "struct Person { } fn f() { let p = Person::new(); }";
    parse_module(src).expect("Person::new() must parse as a qualified method call");
}

// ─── Turbofish call monomorphization ───────────────────────────────────────

#[test]
fn free_call_turbofish_parses() {
    let src = "fn f() { let x = foo::<Felt>(1); }";
    parse_module(src).expect("foo::<Felt>(1) must parse as a free turbofish call");
}

#[test]
fn member_call_turbofish_parses() {
    let src = "fn f() { let x = obj.m::<Felt>(1); }";
    parse_module(src).expect("obj.m::<Felt>(1) must parse as a member turbofish call");
}

#[test]
fn free_call_const_turbofish_parses() {
    let src = "fn f() { let x = foo::<3>(1); }";
    parse_module(src).expect("foo::<3>(1) must parse: const turbofish argument");
}

// ─── Obsolete `#<...>` is rejected ─────────────────────────────────────────

#[test]
fn obsolete_pound_turbofish_rejected() {
    let src = "fn f() { let x = foo#<Felt>(1); }";
    parse_module(src).expect_err("foo#<Felt>(1) must be rejected: obsolete #<...> syntax");
}

// ─── Qualified generic paths ───────────────────────────────────────────────

#[test]
fn qualified_generic_path_expr_parses() {
    let src = "fn f() { let x = mod1::<Felt>::bar(); }";
    parse_module(src).expect("mod1::<Felt>::bar() must parse as a qualified generic path call");
}

#[test]
fn trait_cast_qualified_path_parses() {
    let src = "fn f() { let x = <Felt as Trait>::bar(); }";
    parse_module(src).expect("<Felt as Trait>::bar() must parse as a trait-cast qualified path");
}

#[test]
fn qualified_generic_path_type_parses() {
    let src = "fn f() -> outer::<Felt>::Ty { }";
    parse_module(src).expect("outer::<Felt>::Ty must parse as a qualified generic path type");
}

// ─── Empty generic rejection ───────────────────────────────────────────────

#[test]
fn empty_generic_args_type_rejected() {
    let src = "fn f() -> A<> { }";
    parse_module(src).expect_err("A<> must error: empty generic argument list in type context");
}

#[test]
fn empty_generic_args_call_rejected() {
    let src = "fn f() { let x = foo::<>(1); }";
    parse_module(src).expect_err("foo::<>(1) must error: empty generic argument list in call context");
}

#[test]
fn empty_generic_params_rejected() {
    let src = "fn f<>() { }";
    parse_module(src).expect_err("fn f<>() must error: empty generic parameter declaration");
}

/// `Foo<> { }` must produce a clear "empty generic argument list" diagnostic,
/// not a misleading comparison-operator fallback.
#[test]
fn empty_generic_struct_literal_rejected_with_clear_error() {
    let src = "struct Foo { } fn f() { let p = Foo<> { }; }";
    let err = parse_module(src).expect_err("Foo<> {} must be rejected");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("empty generic argument list"),
        "expected empty-generic diagnostic, got: {msg}"
    );
}

/// `a <> b` is a comparison expression, NOT an empty-generic struct literal.
/// Must not report "empty generic argument list".
#[test]
fn comparison_not_treated_as_empty_generic() {
    let src = "fn f() { let x = a <> b; }";
    let err = parse_module(src).expect_err("a <> b must error (invalid comparison)");
    let msg = format!("{err:#}");
    assert!(
        !msg.contains("empty generic argument list"),
        "comparison a <> b must not report empty-generic error: {msg}"
    );
}

// ─── Nested `>>` closers (non-empty) ───────────────────────────────────────

#[test]
fn nested_nonempty_generics_expr_parses() {
    let src = "fn f() { let x: A<B<C>> = y; }";
    parse_module(src).expect("A<B<C>> must parse: nested non-empty generic closers");
}

#[test]
fn nested_generic_struct_literal_parses() {
    let src = "fn f() { let p = Foo<A<B<C>>> { }; }";
    parse_module(src).expect("Foo<A<B<C>>> { } must parse: triple nested generic closers in a struct literal");
}

// ─── Comparisons preserved (struct-literal probe must not steal `<`) ────────

#[test]
fn comparison_less_than_parses() {
    let src = "fn f() { let x = a < b; }";
    parse_module(src).expect("a < b must parse as a comparison, not a struct literal probe");
}

#[test]
fn if_less_than_condition_parses() {
    let src = "fn f() { if a < b { } }";
    parse_module(src).expect("if a < b { } must parse: condition is the comparison a < b");
}

#[test]
fn chained_comparison_parses() {
    let src = "fn f() { let x = a < b > c; }";
    parse_module(src).expect("a < b > c must parse as chained comparisons");
}

// ─── Control-flow brace ambiguity ──────────────────────────────────────────
// `path {` must NOT become a struct literal when the `{` is a control-flow body.

#[test]
fn for_range_end_brace_is_body() {
    let src = "fn f() { for i in 0u32..N { } }";
    parse_module(src).expect("for i in 0u32..N { } must parse: N is the range end, { } is the body");
}

#[test]
fn for_range_end_generic_not_struct() {
    let src = "fn f() { for i in 0u32..vec::<Felt> { } }";
    parse_module(src).expect("for ..vec::<Felt> { } must parse: { } is the body, not a struct literal");
}

#[test]
fn while_condition_brace_is_body() {
    let src = "fn f() { while a < b { x = 1; } }";
    parse_module(src).expect("while a < b { } must parse: { } is the body");
}

#[test]
fn if_condition_with_block_body_and_stmt() {
    let src = "fn f() { if a < b { same = false; } }";
    parse_module(src).expect("if a < b { same = false; } must parse with { } as the if-body");
}

#[test]
fn match_scrutinee_brace_is_arms() {
    let src = "fn f() { let r = match a < b { _ => 1 }; }";
    parse_module(src).expect("match a < b { _ => 1 } must parse: { ... } is the arms block");
}

/// Regression for storage.psy: `for i in 0u32..N { if self[i as Felt] != rhs[i as Felt] { ... } }`
#[test]
fn for_if_index_access_regression_parses() {
    let src = "fn f() { for i in 0u32..N { if self[i as Felt] != rhs[i as Felt] { same = false; } } }";
    parse_module(src).expect("for { if self[..] != rhs[..] { .. } } must parse (storage.psy regression)");
}

/// Struct literals are re-enabled inside delimiters even within a predicate.
#[test]
fn struct_literal_in_call_arg_within_predicate_parses() {
    let src = "fn f() { if foo(Bar { x: 1 }) { } }";
    parse_module(src).expect("if foo(Bar { x: 1 }) { } must parse: struct literal allowed inside call args");
}

/// Struct literals remain allowed in an if-body.
#[test]
fn struct_literal_in_if_body_parses() {
    let src = "fn f() { if a { let p = Foo { x: 1 }; } }";
    parse_module(src).expect("struct literal must parse inside an if-body");
}

/// `mod::<Type<Generic>>::method` must parse as a trait-cast path segment,
/// not a turbofish applied to the module. Regression for path_test.psy:112.
#[test]
fn qualified_trait_cast_path_with_nested_generics_parses() {
    let src = "fn f() { let x = mod1::<Test<Felt>>::get_test::<u32>(666u32); }";
    parse_module(src).expect("mod1::<Test<Felt>>::get_test::<u32>(666u32) must parse as a trait-cast path");
}

/// `mod::<Type>::method()` — single generic type in a trait-cast path segment.
#[test]
fn qualified_trait_cast_path_single_generic_parses() {
    let src = "fn f() { let x = mod1::<Felt>::bar(); }";
    parse_module(src).expect("mod1::<Felt>::bar() must parse: trait-cast path segment followed by call");
}

#[test]
fn public_bit_helper_names_are_regular_identifiers() {
    let src = r#"
fn sum_bits(value: Felt) -> Felt { value }
fn split_bits(value: Felt) -> Felt { value }
fn main() {
    let sum_bits = 1;
    let split_bits = 2;
}
"#;
    parse_module(src).expect("sum_bits and split_bits must not be reserved lexer tokens");
}
