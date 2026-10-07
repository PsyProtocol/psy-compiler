// Deep QA for trait constraint handling: declaration, enforcement, resolution,
// propagation, relaxation, and interaction with generics, associated types,
// impl blocks, structs, arrays, tuples, Self, and extern fns.
//
// Drives the real parser + sema through `Interpreter::typecheck_single` with
// temporary `.psy` sources. Every case names the concrete contract it defends
// and asserts the exact accept/reject outcome.
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
    let path = std::env::temp_dir().join(format!("psy_constr_{unique}_{n}.psy"));
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
// (1) Single constraint — T::method() resolves via constraint
// ═══════════════════════════════════════════════════════════════════════════

/// Method call `x.m()` on a constrained type variable `T: Trait` resolves
/// through the constraint's trait scope (implementer.rs:328-340).
#[test]
fn c01_single_constraint_method_resolves() {
    expect_accept(
        "c01_single_constraint_method_resolves",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn use_it<T: T>(x: T) -> Felt { x.m() }
fn main() {}
"#,
    );
}

/// Associated function call `T::make()` on a constrained type variable
/// resolves through the constraint.
#[test]
fn c01b_single_constraint_assoc_fn_resolves() {
    expect_accept(
        "c01b_single_constraint_assoc_fn_resolves",
        r#"
trait T { fn make() -> Felt; }
fn use_it<T: T>() -> Felt { T::make() }
fn main() {}
"#,
    );
}

/// Constrained generic function with a body that doesn't use the constraint
/// still typechecks (constraint declared but unused).
#[test]
fn c01c_single_constraint_unused_typechecks() {
    expect_accept(
        "c01c_single_constraint_unused_typechecks",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn use_it<T: T>(x: T) -> T { x }
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (2) Multiple constraints — all checked
// ═══════════════════════════════════════════════════════════════════════════

/// Two constraints `T: A + B` — method from A is available.
#[test]
fn c02_multi_constraint_first_method() {
    expect_accept(
        "c02_multi_constraint_first_method",
        r#"
trait A { fn a(self: Self) -> Felt; }
trait B { fn b(self: Self) -> Felt; }
fn use_it<T: A + B>(x: T) -> Felt { x.a() }
fn main() {}
"#,
    );
}

/// Two constraints `T: A + B` — method from B is available.
#[test]
fn c02b_multi_constraint_second_method() {
    expect_accept(
        "c02b_multi_constraint_second_method",
        r#"
trait A { fn a(self: Self) -> Felt; }
trait B { fn b(self: Self) -> Felt; }
fn use_it<T: A + B>(x: T) -> Felt { x.b() }
fn main() {}
"#,
    );
}

/// Three constraints `T: A + B + C` — all methods available.
#[test]
fn c02c_three_constraints_all_methods() {
    expect_accept(
        "c02c_three_constraints_all_methods",
        r#"
trait A { fn a(self: Self) -> Felt; }
trait B { fn b(self: Self) -> Felt; }
trait C { fn c(self: Self) -> Felt; }
fn use_it<T: A + B + C>(x: T) -> Felt { x.a() }
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (3) Constraint on generic struct
// ═══════════════════════════════════════════════════════════════════════════

/// Declaration of generic struct with constraint typechecks.
#[test]
fn c03_generic_struct_with_constraint_decl() {
    expect_accept(
        "c03_generic_struct_with_constraint_decl",
        r#"
trait T { fn m(self: Self) -> Felt; }
struct S<T: T> { pub x: T }
fn main() {}
"#,
    );
}

/// Method access through a field of constrained generic struct type.
/// `s.x.m()` where `s: S<T>` and `S<T: T>` — `s.x` has type T with constraint T.
#[test]
fn c03b_generic_struct_field_method_access() {
    expect_accept(
        "c03b_generic_struct_field_method_access",
        r#"
trait T { fn m(self: Self) -> Felt; }
struct S<T: T> { pub x: T }
fn use_it<T: T>(s: S<T>) -> Felt { s.x.m() }
fn main() {}
"#,
    );
}

/// Struct literal construction with constrained generic type parameter.
#[test]
fn c03c_struct_literal_constrained_generic() {
    expect_accept(
        "c03c_struct_literal_constrained_generic",
        r#"
trait T { fn m(self: Self) -> Felt; }
struct S<T: T> { pub x: T }
impl<T: T> S<T> {
    fn make(x: T) -> S<T> { return S { x: x }; }
}
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (4) Constraint on impl — T's Trait methods resolve
// ═══════════════════════════════════════════════════════════════════════════

/// `impl<T: T> U for T` — T's trait method resolves via the constraint inside
/// the impl body.
#[test]
fn c04_impl_constraint_method_resolves() {
    expect_accept(
        "c04_impl_constraint_method_resolves",
        r#"
trait T { fn t_method(self: Self) -> Felt; }
trait U { fn u_method(self: Self) -> Felt; }
impl<T: T> U for T {
    fn u_method(self: Self) -> Felt { self.t_method() }
}
fn main() {}
"#,
    );
}

/// A type variable cannot be used as the trait in a blanket impl header.
#[test]
fn c04b_blanket_impl_constraint_rejected() {
    expect_reject("c04b_blanket_impl_constraint_rejected", r#"
trait T { fn m(self: Self) -> Felt; }
impl<T: T> T for T {
    fn m(self: Self) -> Felt { self.m() }
}
fn main() {}
"#, "type mismatch");
}

// ═══════════════════════════════════════════════════════════════════════════
// (5) Constraint propagation across function calls
// ═══════════════════════════════════════════════════════════════════════════

/// `fn f<T: T>` calls `fn g<T: T>` — constraint propagates.
#[test]
fn c05_constraint_propagation_same() {
    expect_accept(
        "c05_constraint_propagation_same",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn g<T: T>(x: T) -> Felt { x.m() }
fn f<T: T>(x: T) -> Felt { g::<T>(x) }
fn main() {}
"#,
    );
}

/// `fn f<T>` (no constraint) calls `fn g<T: T>` — should fail because
/// f's T doesn't satisfy g's constraint.
#[test]
fn c05b_constraint_propagation_missing_rejected() {
    expect_reject(
        "c05b_constraint_propagation_missing_rejected",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn g<T: T>(x: T) -> Felt { x.m() }
fn f<T>(x: T) -> Felt { g::<T>(x) }
fn main() {}
"#,
        "TypeMismatch",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (6) Constraint satisfaction with concrete types
// ═══════════════════════════════════════════════════════════════════════════

/// `fn use_it<T: T>(x: T)` called with `Foo` where `impl T for Foo` — works.
#[test]
fn c06_concrete_type_satisfies_constraint() {
    expect_accept(
        "c06_concrete_type_satisfies_constraint",
        r#"
trait T { fn m(self: Self) -> Felt; }
struct Foo { pub v: Felt }
impl T for Foo { fn m(self: Self) -> Felt { return self.v; } }
fn use_it<T: T>(x: T) -> Felt { x.m() }
fn main() { let f = Foo { v: 42 }; let r = use_it::<Foo>(f); }
"#,
    );
}

/// Calling a constrained generic function with a type that does NOT implement
/// the trait — should fail.
#[test]
fn c06b_concrete_type_missing_impl_rejected() {
    expect_reject(
        "c06b_concrete_type_missing_impl_rejected",
        r#"
trait T { fn m(self: Self) -> Felt; }
struct Foo { pub v: Felt }
struct Bar { v: Felt }
impl T for Foo { fn m(self: Self) -> Felt { return self.v; } }
fn use_it<T: T>(x: T) -> Felt { x.m() }
fn main() { let b = Bar { v: 42 }; let r = use_it::<Bar>(b); }
"#,
        "TypeMismatch",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (7) Missing constraint — calling trait method on T without constraint
// ═══════════════════════════════════════════════════════════════════════════

/// `fn f<T>(x: T) { x.m() }` where m is from Trait — rejected because T has
/// no constraint.
#[test]
fn c07_missing_constraint_rejected() {
    expect_reject(
        "c07_missing_constraint_rejected",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn use_it<T>(x: T) -> Felt { x.m() }
fn main() {}
"#,
        "UnresolvedMember",
    );
}

/// Missing constraint on associated function call — `T::make()` without T: T.
#[test]
fn c07b_missing_constraint_assoc_fn_rejected() {
    expect_reject(
        "c07b_missing_constraint_assoc_fn_rejected",
        r#"
trait T { fn make() -> Felt; }
fn use_it<T>() -> Felt { T::make() }
fn main() {}
"#,
        "UnresolvedMember",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (8) Constraint with associated types — T: Trait<AssocTy = Felt>
// ═══════════════════════════════════════════════════════════════════════════

/// `T: Trait<AssocTy = Felt>` — associated type binding in constraint.
/// UNIMPLEMENTED: The parser's `parse_generic_args` (ty.rs:214-249) only
/// parses comma-separated types, not `Name = Type` bindings. So this syntax
/// is not supported and will produce a parse error.
#[test]
fn c08_constraint_with_assoc_type_binding_unimplemented() {
    expect_reject(
        "c08_constraint_with_assoc_type_binding_unimplemented",
        r#"
trait T { type Item; fn m(self: Self) -> Felt; }
fn use_it<T: T<Item = Felt>>(x: T) -> Felt { x.m() }
fn main() {}
"#,
        "",  // any rejection
    );
}

/// Associated type WITH a constraint in trait declaration — `type Item: NewTrait`.
/// This IS supported (parse_trait_associated_type at item.rs:782-810).
#[test]
fn c08b_assoc_type_with_constraint_in_trait_decl() {
    expect_accept(
        "c08b_assoc_type_with_constraint_in_trait_decl",
        r#"
trait NewTrait { fn new() -> Self; }
trait HasItem { type Item: NewTrait; fn get() -> Self::Item; }
fn main() {}
"#,
    );
}

/// Associated type with constraint — impl override must satisfy the constraint.
/// Felt does not implement NewTrait, so the override `type Item = Felt` should
/// be rejected.
#[test]
fn c08c_assoc_type_constraint_override_rejected() {
    expect_reject(
        "c08c_assoc_type_constraint_override_rejected",
        r#"
trait NewTrait { fn new() -> Self; }
trait HasItem { type Item: NewTrait; }
struct Foo { pub v: Felt }
impl HasItem for Foo { type Item = Felt; }
fn main() {}
"#,
        "TypeMismatch",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (9) Trait object dispatch — static dispatch via <T as Trait>::method
// ═══════════════════════════════════════════════════════════════════════════

/// `<Foo as T>::m(f)` — static trait method dispatch on concrete type.
#[test]
fn c09_trait_static_dispatch_concrete() {
    expect_accept(
        "c09_trait_static_dispatch_concrete",
        r#"
trait T { fn m(self: Self) -> Felt; }
struct Foo { pub v: Felt }
impl T for Foo { fn m(self: Self) -> Felt { return self.v; } }
fn main() {
    let f = Foo { v: 42 };
    let r = <Foo as T>::m(f);
}
"#,
    );
}

/// `<T as Trait>::method()` on a constrained type variable — dispatch through
/// the constraint (lib.rs:1482-1500 checks constraint list for type variables).
#[test]
fn c09b_trait_cast_on_type_variable() {
    expect_accept(
        "c09b_trait_cast_on_type_variable",
        r#"
trait T { fn m(self: Self) -> Felt; }
trait U { fn u(self: Self) -> Felt; }
fn use_it<T: T + U>(x: T) -> Felt { <T as U>::u(x) }
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (10) Supertrait / trait inheritance
// ═══════════════════════════════════════════════════════════════════════════

/// `trait B: A {}` — supertrait declaration.
/// UNIMPLEMENTED: parse_trait_definition (item.rs:714-717) does not parse a
/// colon after the trait name. The `:` would be unexpected.
#[test]
fn c10_supertrait_unimplemented() {
    expect_reject(
        "c10_supertrait_unimplemented",
        r#"
trait A { fn a(self: Self) -> Felt; }
trait B: A { fn b(self: Self) -> Felt; }
fn main() {}
"#,
        "",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (11) Where clause
// ═══════════════════════════════════════════════════════════════════════════

/// `fn f<T>() where T: Trait {}` — where clause.
/// UNIMPLEMENTED: parse_function_definition (item.rs:283-292) does not parse
/// `where` after the return type. The `where` keyword would be unexpected.
#[test]
fn c11_where_clause_unimplemented() {
    expect_reject(
        "c11_where_clause_unimplemented",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn f<T>() where T: T {}
fn main() {}
"#,
        "",
    );
}

/// Where clause on a struct — also unimplemented.
#[test]
fn c11b_where_clause_on_struct_unimplemented() {
    expect_reject(
        "c11b_where_clause_on_struct_unimplemented",
        r#"
trait T { fn m(self: Self) -> Felt; }
struct S<T> where T: T { x: T }
fn main() {}
"#,
        "",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (12) Constraint relaxation — T: A+B calling T: A
// ═══════════════════════════════════════════════════════════════════════════

/// `fn f<T: A + B>` calls `fn g<T: A>` — relaxation works (T: A+B satisfies T: A).
#[test]
fn c12_constraint_relaxation_ab_to_a() {
    expect_accept(
        "c12_constraint_relaxation_ab_to_a",
        r#"
trait A { fn a(self: Self) -> Felt; }
trait B { fn b(self: Self) -> Felt; }
fn g<T: A>(x: T) -> Felt { x.a() }
fn f<T: A + B>(x: T) -> Felt { g::<T>(x) }
fn main() {}
"#,
    );
}

/// `fn f<T: A + B + C>` calls `fn g<T: A + B>` — relaxation works.
#[test]
fn c12b_constraint_relaxation_abc_to_ab() {
    expect_accept(
        "c12b_constraint_relaxation_abc_to_ab",
        r#"
trait A { fn a(self: Self) -> Felt; }
trait B { fn b(self: Self) -> Felt; }
trait C { fn c(self: Self) -> Felt; }
fn g<T: A + B>(x: T) -> Felt { x.a() }
fn f<T: A + B + C>(x: T) -> Felt { g::<T>(x) }
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (13) Duplicate constraint — T: A + A
// ═══════════════════════════════════════════════════════════════════════════

/// `T: A + A` — duplicate constraint. The parser allows it (no dedup check).
/// Sema's typecheck_generic_parameter (lib.rs:3378) checks all are traits,
/// which passes. No duplicate detection.
#[test]
fn c13_duplicate_constraint_accepted() {
    expect_accept(
        "c13_duplicate_constraint_accepted",
        r#"
trait A { fn a(self: Self) -> Felt; }
fn f<T: A + A>(x: T) -> Felt { x.a() }
fn main() {}
"#,
    );
}

/// Triple duplicate `T: A + A + A` — still accepted.
#[test]
fn c13b_triple_duplicate_constraint_accepted() {
    expect_accept(
        "c13b_triple_duplicate_constraint_accepted",
        r#"
trait A { fn a(self: Self) -> Felt; }
fn f<T: A + A + A>(x: T) -> Felt { x.a() }
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (14) Empty constraint list — T: (colon with nothing after)
// ═══════════════════════════════════════════════════════════════════════════

/// `fn f<T:>(x: T) {}` — colon with nothing after. The parser's
/// parse_generic_constraints (ty.rs:424-437) consumes `:` then calls
/// parse_path_ty, which fails on `{`.
#[test]
fn c14_empty_constraint_after_colon_rejected() {
    // This is a parse error — typecheck_single returns a parse-level error.
    // We accept any rejection.
    expect_reject(
        "c14_empty_constraint_after_colon_rejected",
        r#"
fn f<T:>(x: T) {}
fn main() {}
"#,
        "",
    );
}

/// `fn f<T: >(x: T) {}` — colon with space then nothing. Same rejection.
#[test]
fn c14b_empty_constraint_space_rejected() {
    expect_reject(
        "c14b_empty_constraint_space_rejected",
        r#"
fn f<T: >(x: T) {}
fn main() {}
"#,
        "",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (15) Constraint on trait method self parameter — where Self: OtherTrait
// ═══════════════════════════════════════════════════════════════════════════

/// `fn method(self: Self) where Self: OtherTrait` — where clause on method.
/// UNIMPLEMENTED: where clauses are not parsed (same as focus area 11).
#[test]
fn c15_self_where_clause_unimplemented() {
    expect_reject(
        "c15_self_where_clause_unimplemented",
        r#"
trait A { fn a(self: Self) -> Felt; }
trait B { fn b(self: Self) -> Felt; }
impl A for B {
    fn a(self: Self) -> Felt where Self: B { self.b() }
}
fn main() {}
"#,
        "",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (A) Constraint interaction with struct literal construction
// ═══════════════════════════════════════════════════════════════════════════

/// Struct literal `S { x: value }` where S<T: T> — constructing with a value
/// that satisfies the constraint.
#[test]
fn ca_struct_literal_with_constraint() {
    expect_accept(
        "ca_struct_literal_with_constraint",
        r#"
trait T { fn m(self: Self) -> Felt; }
struct Foo { pub v: Felt }
impl T for Foo { fn m(self: Self) -> Felt { return self.v; } }
struct S<T: T> { pub x: T }
fn main() { let f = Foo { v: 42 }; let s = S::<Foo> { x: f }; }
"#,
    );
}

/// Struct literal with bare generic and constraint — `S<Foo> { x: f }`.
#[test]
fn ca_struct_literal_bare_generic_constraint() {
    expect_accept(
        "ca_struct_literal_bare_generic_constraint",
        r#"
trait T { fn m(self: Self) -> Felt; }
struct Foo { pub v: Felt }
impl T for Foo { fn m(self: Self) -> Felt { return self.v; } }
struct S<T: T> { pub x: T }
fn main() { let f = Foo { v: 42 }; let s = S<Foo> { x: f }; }
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (B) Constraint on trait impl blocks — impl<T: Trait> Trait2 for T
// ═══════════════════════════════════════════════════════════════════════════

/// Blanket impl with constraint: `impl<T: T> U for T` where the U method body
/// calls T's method via the constraint.
#[test]
fn cb_trait_impl_block_constraint() {
    expect_accept(
        "cb_trait_impl_block_constraint",
        r#"
trait T { fn t_m(self: Self) -> Felt; }
trait U { fn u_m(self: Self) -> Felt; }
impl<T: T> U for T {
    fn u_m(self: Self) -> Felt { self.t_m() }
}
fn main() {}
"#,
    );
}

/// Multiple constraints on impl generic parameter.
#[test]
fn cb_impl_multi_constraint() {
    expect_accept(
        "cb_impl_multi_constraint",
        r#"
trait A { fn a(self: Self) -> Felt; }
trait B { fn b(self: Self) -> Felt; }
trait C { fn c(self: Self) -> Felt; }
impl<T: A + B> C for T {
    fn c(self: Self) -> Felt { self.a() }
}
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (C) Constraint with array types — fn f<T: Trait, N: u32>()
// ═══════════════════════════════════════════════════════════════════════════

/// `fn f<T: Trait, N: u32>()` — mixed type and const generic with constraints.
/// `N: u32` is valid per typecheck_generic_parameter (lib.rs:3379: exactly 1
/// basic type constraint).
#[test]
fn cc_array_constraint_mixed_type_const() {
    expect_accept(
        "cc_array_constraint_mixed_type_const",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn f<T: T, N: u32>() {}
fn main() {}
"#,
    );
}

/// Array type with constrained element type: `[T; N]` where `T: Trait`.
#[test]
fn cc_array_with_constrained_element() {
    expect_accept(
        "cc_array_with_constrained_element",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn f<T: T, N: Felt>(x: [T; N]) {}
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (D) Constraint on tuple types
// ═══════════════════════════════════════════════════════════════════════════

/// Tuple type with constrained generic: `fn f<T: T>(x: (T, Felt))`.
#[test]
fn cd_tuple_with_constrained_generic() {
    expect_accept(
        "cd_tuple_with_constrained_generic",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn f<T: T>(x: (T, Felt)) -> Felt { x.0.m() }
fn main() {}
"#,
    );
}

/// Tuple of two constrained generics: `(T, T)`.
#[test]
fn cd_tuple_two_constrained() {
    expect_accept(
        "cd_tuple_two_constrained",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn f<T: T>(x: (T, T)) -> Felt { x.0.m() }
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (E) Constraint with Self type
// ═══════════════════════════════════════════════════════════════════════════

/// Self type in trait method signature with constraint — basic Self works.
#[test]
fn ce_self_type_basic() {
    expect_accept(
        "ce_self_type_basic",
        r#"
trait T { fn m(self: Self) -> Self; }
struct Foo { pub v: Felt }
impl T for Foo { fn m(self: Self) -> Self { return self; } }
fn main() {}
"#,
    );
}

/// LIMITATION: Self::method() call inside a trait default method body does
/// not resolve — trait B's default `fn b { self.a() }` fails with
/// UnresolvedMember because sema does not look up sibling methods within
/// the same trait during default method typechecking. This is a sema
/// limitation, not a parser issue.
#[test]
fn ce_self_method_call_in_trait_rejected() {
    expect_reject(
        "ce_self_method_call_in_trait_rejected",
        r#"
trait A { fn a(self: Self) -> Felt; }
trait B { fn b(self: Self) -> Felt { return self.a(); } }
struct Foo { pub v: Felt }
impl A for Foo { fn a(self: Self) -> Felt { return self.v; } }
impl B for Foo { fn b(self: Self) -> Felt { return self.v; } }
fn main() {}
"#,
        "UnresolvedMember",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (F) Extern fn declarations with constraints
// ═══════════════════════════════════════════════════════════════════════════

/// `extern fn f<T: Trait>(x: T) -> Felt;` — extern fn with constraint, no body.
#[test]
fn cf_extern_fn_with_constraint() {
    expect_accept(
        "cf_extern_fn_with_constraint",
        r#"
trait T { fn m(self: Self) -> Felt; }
extern fn f<T: T>(x: T) -> Felt;
fn main() {}
"#,
    );
}

/// `const extern fn` with constraint — both qualifiers with constraint.
#[test]
fn cf_const_extern_fn_with_constraint() {
    expect_accept(
        "cf_const_extern_fn_with_constraint",
        r#"
trait T { fn m(self: Self) -> Felt; }
const extern fn f<T: T>(x: T) -> Felt;
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (G) Constraint checking order — predecl registers constraints before use
// ═══════════════════════════════════════════════════════════════════════════

/// Constraint is available during predecl: function with constraint that uses
/// the constraint in its own signature (return type references the trait).
#[test]
fn cg_constraint_available_in_predecl() {
    expect_accept(
        "cg_constraint_available_in_predecl",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn f<T: T>(x: T) -> T { x }
fn g<T: T>(x: T) -> Felt { f::<T>(x).m() }
fn main() {}
"#,
    );
}

/// Forward reference: function f uses constraint and is called before its
/// declaration in the module — predecl phase registers all functions first.
#[test]
fn cg_forward_ref_constraint() {
    expect_accept(
        "cg_forward_ref_constraint",
        r#"
trait T { fn m(self: Self) -> Felt; }
fn main() { let r = g::<Felt>(0); }
fn g<T: T>(x: T) -> Felt { x.m() }
impl T for Felt { fn m(self: Self) -> Felt { return 0; } }
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (H) Invalid constraint kinds — non-trait, non-basic-type constraints
// ═══════════════════════════════════════════════════════════════════════════

/// `T: Felt` — single basic type constraint. Valid per lib.rs:3379.
#[test]
fn ch_basic_type_constraint_valid() {
    expect_accept(
        "ch_basic_type_constraint_valid",
        r#"
fn f<T: Felt>(x: T) -> T { x }
fn main() {}
"#,
    );
}

/// `T: Bool` — Bool is a keyword token (TypeBool), not a user trait.
/// The sema resolves it via parse_basic_type -> Identifier::new(TYPE_BOOL),
/// but typecheck cannot find a registered type for IdentId(29) as a trait
/// constraint — it reports UnresolvedType. This is a limitation: only
/// Felt is accepted as a basic-type constraint (lib.rs:3379), not Bool/u32.
#[test]
fn ch_bool_constraint_rejected() {
    expect_reject(
        "ch_bool_constraint_rejected",
        r#"
fn f<T: Bool>(x: T) -> T { x }
fn main() {}
"#,
        "UnresolvedType",
    );
}

/// `T: u32` — single basic type constraint (u32). Valid.
#[test]
fn ch_u32_constraint_valid() {
    expect_accept(
        "ch_u32_constraint_valid",
        r#"
fn f<T: u32>(x: T) -> T { x }
fn main() {}
"#,
    );
}


/// `T: Struct` — constraint is a struct, not a trait. INVALID.
#[test]
fn ch_struct_constraint_rejected() {
    expect_reject(
        "ch_struct_constraint_rejected",
        r#"
struct S { pub v: Felt }
fn f<T: S>(x: T) -> T { x }
fn main() {}
"#,
        "InvalidGenericConstraint",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (I) Constraint with generic trait — T: Trait<Felt>
// ═══════════════════════════════════════════════════════════════════════════

/// `T: Trait<Felt>` — generic trait as constraint. The parser parses this as
/// a generic type, and sema checks it's a trait.
#[test]
fn ci_generic_trait_constraint() {
    expect_accept(
        "ci_generic_trait_constraint",
        r#"
trait T<A> { fn m(self: Self) -> A; }
fn f<T: T<Felt>>(x: T) -> Felt { x.m() }
fn main() {}
"#,
    );
}

/// Generic trait constraint with concrete type that implements the generic
/// trait with the right type argument.
#[test]
fn ci_generic_trait_constraint_satisfied() {
    expect_accept(
        "ci_generic_trait_constraint_satisfied",
        r#"
trait T<A> { fn m(self: Self) -> A; }
struct Foo { pub v: Felt }
impl T<Felt> for Foo { fn m(self: Self) -> Felt { return self.v; } }
fn f<T: T<Felt>>(x: T) -> Felt { x.m() }
fn main() { let foo = Foo { v: 42 }; let r = f::<Foo>(foo); }
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (J) Constraint on enum generic parameter
// ═══════════════════════════════════════════════════════════════════════════

/// Enum declarations are currently rejected with a regular sema error.
#[test]
fn cj_enum_constraint_decl_rejected() {
    expect_reject("cj_enum_constraint_decl_rejected", r#"
trait T { fn m(self: Self) -> Felt; }
enum E<T: T> { V(T) }
fn main() {}
"#, "unresolved type");
}

/// `T::Item` access on a constrained type variable where the trait has an
/// associated type. Defends: find_associated_type on type variable with
/// constraints (implementer.rs:328-340 fallthrough to associated type lookup).
#[test]
fn ck_assoc_type_access_on_constrained_var() {
    expect_accept(
        "ck_assoc_type_access_on_constrained_var",
        r#"
trait T { type Item; fn m(self: Self) -> Self::Item; }
fn use_it<T: T>(x: T) -> T::Item { x.m() }
fn main() {}
"#,
    );
}

/// LIMITATION: `<T as T>::Item` in return type position does not resolve
/// for type variables. The explicit trait cast path in typecheck reduces
/// TraitCast to the impl type only (lib.rs:3331), discarding the trait and
/// losing the associated type resolution. T::Item (without explicit cast)
/// works via constraint fallback (implementer.rs:328-340).
#[test]
fn ck_assoc_type_explicit_cast_constrained_rejected() {
    expect_reject(
        "ck_assoc_type_explicit_cast_constrained",
        r#"
trait T { type Item; fn make() -> Self::Item; }
fn use_it<T: T>(x: T) -> <T as T>::Item { T::make() }
fn main() {}
"#,
        "TypeMismatch",
    );
}
