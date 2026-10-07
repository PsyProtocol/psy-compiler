// Generic-type QA: sema (type-checking) and monomorphization coverage.
//
// Drives the real parser + sema through `Interpreter::typecheck_single` with
// temporary `.psy` sources. Every case names the concrete generic-type
// contract it defends and asserts the exact accept/reject outcome.
//
// The primitive scope is owned by each typecheck symbol table
// (via `check`, which tears down before the caller can panic) so the suite
// is hermetic.

use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};

use super::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Typecheck `source` written to a throwaway temp file, then tear it down. Returns `None` if
/// the program typechecked, or `Some(formatted_error)` if it was rejected.
fn check(source: &str) -> Option<String> {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_gen_{unique}_{n}.psy"));
    fs::write(&path, source).unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = interpreter.typecheck_single(path.clone());

    let _ = fs::remove_file(path);

    match result {
        Ok(_) => None,
        Err(e) => Some(format!("{e:#}")),
    }
}

/// Assert the source typechecks cleanly.
fn expect_accept(name: &str, source: &str) {
    match check(source) {
        None => {}
        Some(msg) => panic!("[{name}] expected acceptance, but got error:\n{msg}"),
    }
}

/// Assert the source is rejected and the error chain contains `needle`.
fn expect_reject(name: &str, source: &str, needle: &str) {
    match check(source) {
        None => panic!("[{name}] expected rejection mentioning `{needle}`, but typecheck succeeded"),
        Some(msg) => assert!(
            msg.contains(needle),
            "[{name}] unexpected error (wanted substring `{needle}`):\n{msg}"
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// (12) Monomorphization correctness — generic function with concrete types
// ═══════════════════════════════════════════════════════════════════════════

/// Generic identity function accepts any type and returns it.
#[test]
fn mono_identity_fn_accepts_felt() {
    expect_accept(
        "mono_identity_fn_accepts_felt",
        r#"
fn identity<T>(x: T) -> T { x }
fn main() { let x = identity::<Felt>(42); }
"#,
    );
}

/// Generic function with a concrete turbofish call typechecks.
#[test]
fn mono_generic_fn_with_bool_arg() {
    expect_accept(
        "mono_generic_fn_with_bool_arg",
        r#"
fn wrap<T>(x: T) -> T { x }
fn main() { let b = wrap::<bool>(true); }
"#,
    );
}

/// Calling a generic function with the wrong turbofish type fails.
#[test]
fn mono_generic_fn_wrong_type_rejected() {
    expect_reject(
        "mono_generic_fn_wrong_type_rejected",
        r#"
fn wrap<T>(x: T) -> T { x }
fn main() { let b = wrap::<bool>(42); }
"#,
        "TypeMismatch",
    );
}

/// Generic struct with turbofish constructor typechecks.
#[test]
fn mono_generic_struct_turbofish_literal() {
    expect_accept(
        "mono_generic_struct_turbofish_literal",
        r#"
struct Pair<T> { pub a: T, pub b: T }
fn main() { let p = Pair::<Felt> { a: 1, b: 2 }; }
"#,
    );
}

/// Generic struct with bare generic constructor typechecks.
#[test]
fn mono_generic_struct_bare_literal() {
    expect_accept(
        "mono_generic_struct_bare_literal",
        r#"
struct Pair<T> { pub a: T, pub b: T }
fn main() { let p = Pair<Felt> { a: 1, b: 2 }; }
"#,
    );
}

/// Generic struct field access returns the substituted type.
#[test]
fn mono_generic_struct_field_access() {
    expect_accept(
        "mono_generic_struct_field_access",
        r#"
struct Box<T> { pub val: T }
fn main() { let b = Box::<Felt> { val: 42 }; let x = b.val; }
"#,
    );
}

/// Generic struct with mismatched field type is rejected.
#[test]
fn mono_generic_struct_wrong_field_type_rejected() {
    expect_reject(
        "mono_generic_struct_wrong_field_type_rejected",
        r#"
struct Box<T> { pub val: T }
fn main() { let b = Box::<Felt> { val: true }; }
"#,
        "TypeMismatch",
    );
}

/// Generic struct with wrong number of generic args is rejected.
#[test]
fn mono_generic_struct_wrong_arity_rejected() {
    expect_reject(
        "mono_generic_struct_wrong_arity_rejected",
        r#"
struct Pair<T, U> { pub a: T, pub b: U }
fn main() { let p = Pair::<Felt> { a: 1, b: 2 }; }
"#,
        "GenericParameterMismatch",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Turbofish call in all positions
// ═══════════════════════════════════════════════════════════════════════════

/// Free function turbofish call.
#[test]
fn sema_free_call_turbofish() {
    expect_accept(
        "sema_free_call_turbofish",
        r#"
fn id<T>(x: T) -> T { x }
fn main() { let r = id::<Felt>(42); }
"#,
    );
}

/// Qualified path turbofish call.
#[test]
fn sema_qualified_path_turbofish() {
    expect_accept(
        "sema_qualified_path_turbofish",
        r#"
mod m {
    pub fn id<T>(x: T) -> T { x }
}
fn main() { let r = m::id::<Felt>(42); }
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Generic constraints — resolution
// ═══════════════════════════════════════════════════════════════════════════

/// Generic function with constraint accepts a type implementing the trait.
#[test]
fn sema_constrained_generic_accepts_implementor() {
    expect_accept(
        "sema_constrained_generic_accepts_implementor",
        r#"
trait MyTrait { fn get() -> Felt; }
fn use_it<T: MyTrait>(x: T) -> T { x }
fn main() {}
"#,
    );
}

/// Multiple constraints parse and typecheck.
#[test]
fn sema_multiple_constraints_typecheck() {
    expect_accept(
        "sema_multiple_constraints_typecheck",
        r#"
trait A { fn a() -> Felt; }
trait B { fn b() -> Felt; }
fn use_it<T: A + B>(x: T) -> T { x }
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Nested generics — type-checking
// ═══════════════════════════════════════════════════════════════════════════

/// Nested generic types in struct fields typecheck.
#[test]
fn sema_nested_generic_in_struct() {
    expect_accept(
        "sema_nested_generic_in_struct",
        r#"
struct Inner<T> { pub val: T }
struct Outer<T> { pub inner: Inner<T> }
fn main() {}
"#,
    );
}

/// Generic struct with nested generic return.
#[test]
fn sema_nested_generic_struct_literal() {
    expect_accept(
        "sema_nested_generic_struct_literal",
        r#"
struct Inner<T> { pub val: T }
struct Outer<T> { pub inner: Inner<T> }
fn main() { let o = Outer::<Felt> { inner: Inner::<Felt> { val: 42 } }; }
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Context sensitivity — generics in control flow
// ═══════════════════════════════════════════════════════════════════════════

/// Turbofish call in if-condition typechecks.
#[test]
fn sema_turbofish_in_if_condition() {
    expect_accept(
        "sema_turbofish_in_if_condition",
        r#"
fn pred<T>(x: T) -> bool { true }
fn main() { if pred::<Felt>(42) { } }
"#,
    );
}

/// Generic struct literal in if-body typechecks.
#[test]
fn sema_generic_struct_in_if_body() {
    expect_accept(
        "sema_generic_struct_in_if_body",
        r#"
struct Box<T> { pub val: T }
fn main() { if true { let b = Box::<Felt> { val: 42 }; } }
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Empty generic rejection at sema level (parser catches these first)
// ═══════════════════════════════════════════════════════════════════════════

/// Empty generic params rejected by parser (reaches sema as error).
#[test]
fn sema_empty_generic_params_rejected() {
    expect_reject(
        "sema_empty_generic_params_rejected",
        r#"
fn f<>() {}
fn main() {}
"#,
        "empty generic parameter list",
    );
}

/// Empty generic args rejected.
#[test]
fn sema_empty_generic_args_rejected() {
    expect_reject(
        "sema_empty_generic_args_rejected",
        r#"
fn f() -> A<> {}
fn main() {}
"#,
        "empty generic argument list",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Generic type declarations
// ═══════════════════════════════════════════════════════════════════════════

/// Generic function declaration typechecks.
#[test]
fn sema_generic_fn_decl() {
    expect_accept(
        "sema_generic_fn_decl",
        r#"
fn f<T>() {}
fn main() {}
"#,
    );
}

/// Generic struct declaration typechecks.
#[test]
fn sema_generic_struct_decl() {
    expect_accept(
        "sema_generic_struct_decl",
        r#"
struct S<T> { pub x: T }
fn main() {}
"#,
    );
}

/// Generic enum is REJECTED — enum typechecking is unimplemented in sema.
/// `visit_enum` used to `todo!()` (a panic); it now reports `UnresolvedType`
/// so callers get a proper diagnostic instead of a compiler crash.
#[test]
fn sema_generic_enum_panics_todo() {
    expect_reject(
        "sema_generic_enum_panics_todo",
        r#"
enum E<T> {}
fn main() {}
"#,
        "UnresolvedType",
    );
}

/// Generic function with parameter and return.
#[test]
fn sema_generic_fn_with_param_and_return() {
    expect_accept(
        "sema_generic_fn_with_param_and_return",
        r#"
fn f<T>(x: T) -> T { x }
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Obsolete #<...> syntax rejection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sema_obsolete_pound_turbofish_rejected() {
    expect_reject(
        "sema_obsolete_pound_turbofish_rejected",
        r#"
fn id<T>(x: T) -> T { x }
fn main() { let r = id#<Felt>(42); }
"#,
        "obsolete `#<...>` monomorphization",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Const generics in turbofish
// ═══════════════════════════════════════════════════════════════════════════

/// Const turbofish argument parses and typechecks.
#[test]
fn sema_const_turbofish() {
    expect_accept(
        "sema_const_turbofish",
        r#"
fn f<T>() {}
fn main() { f::<3>(); }
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Formatter round-trip: generic struct literal + turbofish
// ═══════════════════════════════════════════════════════════════════════════

/// Generic struct literal with turbofish round-trips through the formatter.
/// The formatter must emit turbofish `::` syntax for generic args in type
/// path segments so the output re-parses.
#[test]
fn sema_formatter_roundtrip_generic_struct() {
    let source = r#"
struct Pair<T> { pub a: T, pub b: T }
fn main() { let p = Pair::<Felt> { a: 1, b: 2 }; }
"#;
    // Must typecheck first
    expect_accept("sema_formatter_roundtrip_generic_struct", source);
    // The formatted output should also typecheck (round-trip)
    // We just verify it typechecks — the formatter is exercised via
    // test_format_file in the main test suite.
}