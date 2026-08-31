//! Deep QA for trait constraint parsing: single, multiple, struct, impl,
//! enum, extern, associated types, duplicates, empty constraints, where
//! clauses, supertraits, and invalid constraint kinds.
//!
//! Covers the parser-level (parse_module) aspects of all 15 focus areas.

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

/// Assert the source parses without error.
fn accept(name: &str, src: &str) {
    parse_module(src).unwrap_or_else(|e| panic!("[{name}] expected parse success, got: {e:#}"));
}

/// Assert the source is rejected.
fn reject(name: &str, src: &str) {
    parse_module(src).expect_err(&format!("[{name}] expected parse rejection"));
}

/// Assert the source is rejected and the error message contains `needle`.
fn reject_msg(name: &str, src: &str, needle: &str) {
    match parse_module(src) {
        Ok(_) => panic!("[{name}] expected rejection mentioning `{needle}`, but parse succeeded"),
        Err(err) => {
            let msg = format!("{err:#}");
            assert!(
                msg.contains(needle),
                "[{name}] expected error containing `{needle}`, got: {msg}"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// (1) Single constraint — parse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn p01_single_constraint_fn() {
    accept("p01", "trait T { fn m(self: Self) -> Felt; } fn f<T: T>(x: T) -> Felt { x.m() }");
}

#[test]
fn p01b_single_constraint_struct() {
    accept("p01b", "trait T { fn m(self: Self) -> Felt; } struct S<T: T> { x: T }");
}

#[test]
fn p01c_single_constraint_enum() {
    accept("p01c", "trait T { fn m(self: Self) -> Felt; } enum E<T: T> { V(T) }");
}

#[test]
fn p01d_single_constraint_impl() {
    accept("p01d", "trait T { fn m(self: Self) -> Felt; } trait U { fn u(self: Self) -> Felt; } impl<T: T> U for T { fn u(self: Self) -> Felt { self.m() } }");
}

#[test]
fn p01e_single_constraint_extern_fn() {
    accept("p01e", "trait T { fn m(self: Self) -> Felt; } extern fn f<T: T>(x: T) -> Felt;");
}

// ═══════════════════════════════════════════════════════════════════════════
// (2) Multiple constraints — parse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn p02_two_constraints() {
    accept("p02", "trait A { fn a(self: Self) -> Felt; } trait B { fn b(self: Self) -> Felt; } fn f<T: A + B>(x: T) -> Felt { x.a() }");
}

#[test]
fn p02b_three_constraints() {
    accept("p02b", "trait A { fn a(self: Self) -> Felt; } trait B { fn b(self: Self) -> Felt; } trait C { fn c(self: Self) -> Felt; } fn f<T: A + B + C>(x: T) -> Felt { x.a() }");
}

#[test]
fn p02c_multi_constraint_on_impl() {
    accept("p02c", "trait A { fn a(self: Self) -> Felt; } trait B { fn b(self: Self) -> Felt; } trait C { fn c(self: Self) -> Felt; } impl<T: A + B> C for T { fn c(self: Self) -> Felt { self.a() } }");
}

#[test]
fn p02d_multi_constraint_on_struct() {
    accept("p02d", "trait A { fn a(self: Self) -> Felt; } trait B { fn b(self: Self) -> Felt; } struct S<T: A + B> { x: T }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (3) Constraint on generic struct — parse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn p03_struct_constraint_basic() {
    accept("p03", "trait T { fn m(self: Self) -> Felt; } struct S<T: T> { x: T }");
}

#[test]
fn p03b_struct_multi_param_mixed_constraints() {
    accept("p03b", "trait T { fn m(self: Self) -> Felt; } struct S<T: T, U> { x: T, y: U }");
}

#[test]
fn p03c_struct_all_constrained() {
    accept("p03c", "trait A { fn a(self: Self) -> Felt; } trait B { fn b(self: Self) -> Felt; } struct S<T: A, U: B> { x: T, y: U }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (4) Constraint on impl — parse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn p04_impl_constraint_blanket() {
    accept("p04", "trait T { fn m(self: Self) -> Felt; } trait U { fn u(self: Self) -> Felt; } impl<T: T> U for T { fn u(self: Self) -> Felt { self.m() } }");
}

#[test]
fn p04b_impl_constraint_multi() {
    accept("p04b", "trait A { fn a(self: Self) -> Felt; } trait B { fn b(self: Self) -> Felt; } impl<T: A + B> A for T { fn a(self: Self) -> Felt { self.b() } }");
}

#[test]
fn p04c_impl_constraint_inherent() {
    accept("p04c", "trait T { fn m(self: Self) -> Felt; } struct S<T: T> { x: T } impl<T: T> S<T> { fn make(x: T) -> S<T> { return S { x: x }; } }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (8) Constraint with associated types — parse
// ═══════════════════════════════════════════════════════════════════════════

/// `T: Trait<AssocTy = Felt>` — associated type binding in constraint.
/// UNIMPLEMENTED: parse_generic_args only parses types, not `Name = Type`.
/// The parser will accept `T: Trait<Felt>` (generic trait constraint) but
/// not `T: Trait<AssocTy = Felt>` (associated type binding).
#[test]
fn p08_assoc_type_binding_in_constraint_unimplemented() {
    reject("p08", "trait T { type Item; fn m(self: Self) -> Felt; } fn f<T: T<Item = Felt>>(x: T) -> Felt { x.m() }");
}

/// `T: Trait<Felt>` — generic trait constraint IS supported.
#[test]
fn p08b_generic_trait_constraint() {
    accept("p08b", "trait T<A> { fn m(self: Self) -> A; } fn f<T: T<Felt>>(x: T) -> Felt { x.m() }");
}

/// Associated type with constraint in trait body: `type Item: NewTrait;`
/// This IS supported (parse_trait_associated_type via parse_one_generic_parameter).
#[test]
fn p08c_assoc_type_with_constraint_in_trait() {
    accept("p08c", "trait NewTrait { fn new() -> Self; } trait HasItem { type Item: NewTrait; fn get() -> Self::Item; }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (10) Supertrait / trait inheritance — parse
// ═══════════════════════════════════════════════════════════════════════════

/// `trait B: A {}` — supertrait syntax. UNIMPLEMENTED: parse_trait_definition
/// does not parse a colon after the trait name (item.rs:714-717).
#[test]
fn p10_supertrait_unimplemented() {
    reject("p10", "trait A { fn a(self: Self) -> Felt; } trait B: A { fn b(self: Self) -> Felt; }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (11) Where clause — parse
// ═══════════════════════════════════════════════════════════════════════════

/// `fn f<T>() where T: Trait {}` — where clause. UNIMPLEMENTED:
/// parse_function_definition does not parse `where` (item.rs:283-292).
#[test]
fn p11_where_clause_fn_unimplemented() {
    reject("p11", "trait T { fn m(self: Self) -> Felt; } fn f<T>() where T: T {}");
}

/// Where clause on struct. UNIMPLEMENTED.
#[test]
fn p11b_where_clause_struct_unimplemented() {
    reject("p11b", "trait T { fn m(self: Self) -> Felt; } struct S<T> where T: T { x: T }");
}

/// Where clause on impl. UNIMPLEMENTED.
#[test]
fn p11c_where_clause_impl_unimplemented() {
    reject("p11c", "trait T { fn m(self: Self) -> Felt; } impl<T> T for T where T: T { fn m(self: Self) -> Felt { self.m() } }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (13) Duplicate constraint — parse
// ═══════════════════════════════════════════════════════════════════════════

/// `T: A + A` — parser accepts (no dedup check).
#[test]
fn p13_duplicate_constraint_accepted() {
    accept("p13", "trait A { fn a(self: Self) -> Felt; } fn f<T: A + A>(x: T) -> Felt { x.a() }");
}

#[test]
fn p13b_triple_duplicate_accepted() {
    accept("p13b", "trait A { fn a(self: Self) -> Felt; } fn f<T: A + A + A>(x: T) -> Felt { x.a() }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (14) Empty constraint list — T: (colon with nothing after)
// ═══════════════════════════════════════════════════════════════════════════

/// `fn f<T:>(x: T) {}` — colon with nothing after. parse_generic_constraints
/// consumes `:` then calls parse_path_ty which fails on `{`.
#[test]
fn p14_empty_constraint_rejected() {
    reject("p14", "fn f<T:>(x: T) {}");
}

/// `fn f<T: , U>(x: T) {}` — colon then comma.
#[test]
fn p14b_empty_constraint_comma_rejected() {
    reject("p14b", "fn f<T: , U>(x: T) {}");
}

/// `fn f<T:>(x: T)` — colon then close `>`.
#[test]
fn p14c_empty_constraint_gt_rejected() {
    reject("p14c", "fn f<T:>(x: T)");
}

// ═══════════════════════════════════════════════════════════════════════════
// (15) Constraint on trait method self parameter — where Self: OtherTrait
// ═══════════════════════════════════════════════════════════════════════════

/// `fn method(self: Self) where Self: OtherTrait` — UNIMPLEMENTED (where
/// clauses not parsed on methods).
#[test]
fn p15_self_where_clause_unimplemented() {
    reject("p15", "trait A { fn a(self: Self) -> Felt; } trait B { fn b(self: Self) -> Felt where Self: A; }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (A) Constraint with array types — fn f<T: Trait, N: u32>()
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pa_array_constraint_mixed() {
    accept("pa", "trait T { fn m(self: Self) -> Felt; } fn f<T: T, N: u32>() {}");
}

#[test]
fn pa_array_with_constrained_element() {
    accept("pa_b", "trait T { fn m(self: Self) -> Felt; } fn f<T: T, N: u32>(x: [T; N]) {}");
}

// ═══════════════════════════════════════════════════════════════════════════
// (B) Constraint on tuple types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pb_tuple_constrained_generic() {
    accept("pb", "trait T { fn m(self: Self) -> Felt; } fn f<T: T>(x: (T, Felt)) -> Felt { x.0.m() }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (C) Extern fn with constraints
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pc_extern_fn_constraint() {
    accept("pc", "trait T { fn m(self: Self) -> Felt; } extern fn f<T: T>(x: T) -> Felt;");
}

#[test]
fn pc_const_extern_fn_constraint() {
    accept("pc_b", "trait T { fn m(self: Self) -> Felt; } const extern fn f<T: T>(x: T) -> Felt;");
}

#[test]
fn pc_extern_const_fn_constraint() {
    accept("pc_c", "trait T { fn m(self: Self) -> Felt; } extern const fn f<T: T>(x: T) -> Felt;");
}

// ═══════════════════════════════════════════════════════════════════════════
// (D) Basic type constraints (Felt, Bool, u32) — parse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pd_felt_constraint() {
    accept("pd", "fn f<T: Felt>(x: T) -> T { x }");
}

#[test]
fn pd_bool_constraint() {
    accept("pd_b", "fn f<T: Bool>(x: T) -> T { x }");
}

#[test]
fn pd_u32_constraint() {
    accept("pd_c", "fn f<T: u32>(x: T) -> T { x }");
}

/// `T: Felt + Bool` — two basic constraints. Parser accepts this (it just
/// parses two path types). Sema rejects it (InvalidGenericConstraint).
#[test]
fn pd_two_basic_constraints_parse_ok() {
    accept("pd_d", "fn f<T: Felt + Bool>(x: T) -> T { x }");
}

/// `T: Trait + Felt` — mixed. Parser accepts, sema rejects.
#[test]
fn pd_mixed_trait_basic_parse_ok() {
    accept("pd_e", "trait T { fn m(self: Self) -> Felt; } fn f<T: T + Felt>(x: T) -> T { x }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (E) Constraint with qualified trait path — T: mod::Trait
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pe_qualified_trait_constraint() {
    accept("pe", "pub mod m { pub trait T { fn mk(self: Self) -> Felt; } } fn f<T: m::T>(x: T) -> Felt { x.mk() }");
}

#[test]
fn pe_qualified_generic_trait_constraint() {
    accept("pe_b", "pub mod m { pub trait T<A> { fn mk(self: Self) -> A; } } fn f<T: m::T<Felt>>(x: T) -> Felt { x.mk() }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (F) Empty generic params vs empty constraints — distinction
// ═══════════════════════════════════════════════════════════════════════════

/// `fn f<>() {}` — empty generic parameter list. Rejected by parser
/// (parse_generic_parameters, ty.rs:397-403).
#[test]
fn pf_empty_generic_params_rejected() {
    reject_msg("pf", "fn f<>() {}", "empty generic parameter list");
}

/// `fn f<T>() {}` — no constraint (no colon). Valid — empty constraints.
#[test]
fn pf_no_constraint_valid() {
    accept("pf_b", "fn f<T>(x: T) -> T { x }");
}

/// `fn f<T: Trait,>() {}` — trailing comma after constraint. Valid.
#[test]
fn pf_trailing_comma_after_constraint() {
    accept("pf_c", "trait T { fn m(self: Self) -> Felt; } fn f<T: T,>(x: T) -> Felt { x.m() }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (G) Associated type access on constrained type variable — parse
// ═══════════════════════════════════════════════════════════════════════════

/// `T::Item` on constrained type variable — parse.
#[test]
fn pg_assoc_type_on_constrained_var() {
    accept("pg", "trait T { type Item; fn m(self: Self) -> Self::Item; } fn f<T: T>(x: T) -> T::Item { x.m() }");
}

/// `<T as T>::Item` on constrained type variable — parse.
#[test]
fn pg_assoc_type_explicit_cast_constrained() {
    accept("pg_b", "trait T { type Item; fn make() -> Self::Item; } fn f<T: T>(x: T) -> <T as T>::Item { T::make() }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (H) Self type with constraints — parse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ph_self_in_trait_method() {
    accept("ph", "trait T { fn m(self: Self) -> Self; }");
}

#[test]
fn ph_self_in_impl_method() {
    accept("ph_b", "trait T { fn m(self: Self) -> Self; } struct Foo { v: Felt } impl T for Foo { fn m(self: Self) -> Self { return self; } }");
}

/// Self::method() call inside trait method body — parse.
#[test]
fn ph_self_method_call_in_trait_body() {
    accept("ph_c", "trait A { fn a(self: Self) -> Felt; } trait B { fn b(self: Self) -> Felt { return self.a(); } }");
}