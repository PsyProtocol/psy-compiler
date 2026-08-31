//! Deep QA coverage for generic-type handling after the syntax migration.
//!
//! Covers all 12 focus areas from the QA assignment:
//!  1. Turbofish call monomorphization in all positions
//!  2. Generic struct literals (bare, turbofish, qualified)
//!  3. Generic type declarations (fn, struct, enum)
//!  4. Generic path types (qualified, trait-cast)
//!  5. Nested generics (deep, mixed turbofish + bare)
//!  6. Generic constraints (single, multiple `+`)
//!  7. Const generics (turbofish, array types)
//!  8. Generic parameter shadowing and scoping
//!  9. Contextual `>>` splitting (expressions, types, mixed)
//! 10. Empty generic rejection at every entry point
//! 11. Interaction with struct-literal context sensitivity in control-flow
//! 12. (Monomorphization correctness is covered in the interpreter test suite)

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
// (1) Turbofish call monomorphization — all positions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t01_free_call_turbofish_single_arg() {
    accept("t01", "fn f() { let x = foo::<Felt>(1); }");
}

#[test]
fn t01b_free_call_turbofish_multi_arg() {
    accept("t01b", "fn f() { let x = foo::<Felt, Bool>(1, true); }");
}

#[test]
fn t01c_free_call_turbofish_nested_type_arg() {
    accept("t01c", "fn f() { let x = foo::<A<B>>(1); }");
}

#[test]
fn t02_member_call_turbofish() {
    accept("t02", "fn f() { let x = obj.m::<Felt>(1); }");
}

#[test]
fn t02b_member_call_turbofish_nested() {
    accept("t02b", "fn f() { let x = obj.m::<A<B<C>>>(1); }");
}

#[test]
fn t03_qualified_path_turbofish_call() {
    accept("t03", "fn f() { let x = mod1::func::<Felt>(1); }");
}

#[test]
fn t03b_deep_qualified_path_turbofish_call() {
    accept("t03b", "fn f() { let x = outer::inner::func::<Felt>(1); }");
}

#[test]
fn t04_turbofish_in_type_position_return() {
    accept("t04", "fn f() -> outer::<Felt>::Ty { }");
}

#[test]
fn t04b_turbofish_in_type_position_let() {
    accept("t04b", "fn f() { let x: mod1::<Felt>::Ty = y; }");
}

#[test]
fn t04c_turbofish_in_type_position_nested_generic() {
    accept("t04c", "fn f() -> mod1::<A<B>>::Ty { }");
}

#[test]
fn t05_chained_turbofish_calls() {
    accept("t05", "fn f() { let x = a::<Felt>(1).m::<Bool>(true); }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (2) Generic struct literals — bare, turbofish, qualified
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t10_generic_struct_literal_bare() {
    accept("t10", "fn f() { let p = Foo<Felt> { }; }");
}

#[test]
fn t10b_generic_struct_literal_bare_with_fields() {
    accept("t10b", "fn f() { let p = Foo<Felt> { a: 1 }; }");
}

#[test]
fn t11_generic_struct_literal_turbofish() {
    accept("t11", "fn f() { let p = Foo::<Felt> { }; }");
}

#[test]
fn t11b_generic_struct_literal_turbofish_with_fields() {
    accept("t11b", "fn f() { let p = Foo::<Felt> { a: 1 }; }");
}

#[test]
fn t12_qualified_generic_struct_literal_bare() {
    accept("t12", "fn f() { let p = outer::Foo<Felt> { }; }");
}

#[test]
fn t12b_qualified_generic_struct_literal_turbofish() {
    accept("t12b", "fn f() { let p = outer::Foo::<Felt> { }; }");
}

#[test]
fn t13_nested_generic_struct_literal() {
    accept("t13", "fn f() { let p = Foo<A<B<C>>> { }; }");
}

#[test]
fn t14_generic_struct_literal_multi_arg() {
    accept("t14", "fn f() { let p = Foo<A, B> { }; }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (3) Generic type declarations — fn, struct, enum
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t20_generic_fn_declaration() {
    accept("t20", "fn f<T>() {}");
}

#[test]
fn t20b_generic_fn_declaration_multi_param() {
    accept("t20b", "fn f<T, U>() {}");
}

#[test]
fn t20c_generic_fn_with_return() {
    accept("t20c", "fn f<T>() -> T {}");
}

#[test]
fn t21_generic_struct_declaration() {
    accept("t21", "struct S<T> { x: T }");
}

#[test]
fn t21b_generic_struct_declaration_multi_param() {
    accept("t21b", "struct S<T, U> { x: T, y: U }");
}

#[test]
fn t22_generic_enum_declaration() {
    accept("t22", "enum E<T> { Variant(T) }");
}

#[test]
fn t22b_generic_enum_declaration_multi_param() {
    accept("t22b", "enum E<T, U> { A(T), B(U) }");
}

#[test]
fn t23_generic_fn_with_params_and_body() {
    accept("t23", "fn f<T>(x: T) -> T { x }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (4) Generic path types — qualified, trait-cast
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t30_qualified_generic_path_type() {
    accept("t30", "fn f() -> mod1::<Felt>::Ty { }");
}

#[test]
fn t30b_qualified_generic_path_type_nested() {
    accept("t30b", "fn f() -> mod1::<A<B>>::Ty { }");
}

#[test]
fn t31_trait_cast_qualified_path_type() {
    accept("t31", "fn f() -> <Felt as Trait>::Ty { }");
}

#[test]
fn t31b_trait_cast_qualified_path_expr() {
    accept("t31b", "fn f() { let x = <Felt as Trait>::bar(); }");
}

#[test]
fn t32_trait_cast_with_generic_trait() {
    accept("t32", "fn f() -> <Felt as Trait<Felt>>::Ty { }");
}

#[test]
fn t33_qualified_generic_path_with_trailing_call() {
    accept("t33", "fn f() { let x = mod1::<Felt>::bar(); }");
}

#[test]
fn t34_deeply_qualified_generic_path() {
    accept("t34", "fn f() { let x = mod1::<Test<Felt>>::get_test::<u32>(666u32); }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (5) Nested generics — deep, mixed turbofish + bare
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t40_nested_two_levels() {
    accept("t40", "fn f() -> A<B<C>> { }");
}

#[test]
fn t40b_nested_three_levels() {
    accept("t40b", "fn f() -> A<B<C<D>>> { }");
}

#[test]
fn t40c_nested_four_levels() {
    accept("t40c", "fn f() -> A<B<C<D<E>>>> { }");
}

#[test]
fn t41_nested_multi_arg_inner() {
    accept("t41", "fn f() -> A<B<C, D>> { }");
}

#[test]
fn t41b_nested_multi_arg_outer() {
    accept("t41b", "fn f() -> A<B<C>, D> { }");
}

#[test]
fn t42_mixed_turbofish_and_bare_in_type() {
    accept("t42", "fn f() -> mod1::<A<B>>::Ty { }");
}

#[test]
fn t42b_mixed_turbofish_nested_in_expr() {
    accept("t42b", "fn f() { let x = mod1::<A<B<C>>>::method(); }");
}

#[test]
fn t43_nested_generic_in_struct_field() {
    accept("t43", "struct S { x: A<B<C>> }");
}

#[test]
fn t43b_nested_generic_in_fn_param() {
    accept("t43b", "fn f(x: A<B<C>>) {}");
}

#[test]
fn t44_nested_generic_in_let_annotation() {
    accept("t44", "fn f() { let x: A<B<C>> = y; }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (6) Generic constraints — single, multiple
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t50_single_constraint() {
    accept("t50", "fn f<T: Trait>() {}");
}

#[test]
fn t50b_multiple_constraints() {
    accept("t50b", "fn f<T: A + B>() {}");
}

#[test]
fn t50c_three_constraints() {
    accept("t50c", "fn f<T: A + B + C>() {}");
}

#[test]
fn t51_constraint_on_second_param() {
    accept("t51", "fn f<T, U: Trait>() {}");
}

#[test]
fn t51b_mixed_constrained_unconstrained() {
    accept("t51b", "fn f<T: A, U: B + C>() {}");
}

#[test]
fn t52_generic_struct_with_constraint() {
    accept("t52", "struct S<T: Trait> { x: T }");
}

#[test]
fn t53_generic_enum_with_constraint() {
    accept("t53", "enum E<T: Trait> { V(T) }");
}

#[test]
fn t54_constraint_with_generic_trait() {
    accept("t54", "fn f<T: Trait<Felt>>() {}");
}

#[test]
fn t54b_constraint_with_qualified_trait() {
    accept("t54b", "fn f<T: mod1::Trait>() {}");
}

// ═══════════════════════════════════════════════════════════════════════════
// (7) Const generics — turbofish, array types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t60_const_generic_in_turbofish() {
    accept("t60", "fn f() { let x = foo::<3>(1); }");
}

#[test]
fn t60b_const_generic_u32_in_turbofish() {
    accept("t60b", "fn f() { let x = foo::<3u32>(1); }");
}

#[test]
fn t61_array_type_with_const_size() {
    accept("t61", "fn f() -> [Felt; 8] { }");
}

#[test]
fn t61b_array_type_with_named_size() {
    accept("t61b", "fn f() -> [Felt; N] { }");
}

#[test]
fn t62_array_type_bracket_syntax() {
    // PSY uses [T; N] not Array<T, N> — the bracket form is the canonical array type.
    accept("t62", "fn f() -> [Felt; 8] { }");
}

#[test]
fn t62b_array_keyword_generic_rejected() {
    // `Array` is a keyword token (TypeArray), not a user-facing generic-arg
    // type. parse_basic_type doesn't handle TypeArray, so `Array<Felt, 8>`
    // is rejected. This is by design — arrays use [Felt; 8].
    reject("t62b", "fn f() -> Array<Felt, 8> { }");
}

#[test]
fn t63_const_generic_mixed_with_type() {
    accept("t63", "fn f() { let x = foo::<Felt, 3>(1); }");
}

#[test]
fn t64_nested_generic_with_const() {
    accept("t64", "fn f() -> A<B<3>> { }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (8) Generic parameter shadowing and scoping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t70_same_param_name_in_two_fns() {
    accept("t70", "fn f<T>() {} fn g<T>() {}");
}

#[test]
fn t71_param_name_shadows_type_in_body() {
    accept("t71", "fn f<T>() { let x: T = y; }");
}

#[test]
fn t72_struct_and_fn_share_param_name() {
    accept("t72", "struct S<T> { x: T } fn f<T>(s: S<T>) -> T { s.x }");
}

#[test]
fn t73_nested_generic_same_name() {
    accept("t73", "fn f<T>() -> A<T> { y }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (9) Contextual >> splitting — expressions, types, mixed
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t80_nested_generics_split() {
    accept("t80", "fn f() -> A<B<C>> { }");
}

#[test]
fn t80b_nested_generics_three_split() {
    accept("t80b", "fn f() -> A<B<C<D>>> { }");
}

#[test]
fn t81_shift_after_generic_type() {
    accept("t81", "fn f() { let x: A<B> = y >> 2; }");
}

#[test]
fn t81b_shift_after_nested_generic_type() {
    accept("t81b", "fn f() { let x: A<B<C>> = y >> 2; }");
}

#[test]
fn t82_shift_assign_after_generic_type() {
    accept("t82", "fn f() { let x: A<B> = y; x >>= 2; }");
}

#[test]
fn t83_comparison_lt_gt_not_generic() {
    accept("t83", "fn f() { let x = a < b > c; }");
}

#[test]
fn t84_nested_generic_then_shift() {
    accept("t84", "fn f() { let x: A<B<C>> = y; let z = x >> 2; }");
}

#[test]
fn t85_generic_in_turbofish_then_shift() {
    accept("t85", "fn f() { let x = foo::<A<B>>(1); let y = x >> 2; }");
}

#[test]
fn t86_shift_in_if_condition_with_generic_var() {
    accept("t86", "fn f() { if a < b { let x: A<B> = y; } }");
}

#[test]
fn t87_triple_nested_then_comparison() {
    accept("t87", "fn f() { let x: A<B<C<D>>> = y; let z = a < b; }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (10) Empty generic rejection — every entry point
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t90_empty_generic_args_type() {
    reject_msg("t90", "fn f() -> A<> { }", "empty generic argument list");
}

#[test]
fn t91_empty_generic_args_call() {
    reject_msg("t91", "fn f() { let x = foo::<>(1); }", "empty generic argument list");
}

#[test]
fn t92_empty_generic_params() {
    reject_msg("t92", "fn f<>() { }", "empty generic parameter list");
}

#[test]
fn t93_empty_generic_struct_literal() {
    reject_msg("t93", "struct Foo { } fn f() { let p = Foo<> { }; }", "empty generic argument list");
}

#[test]
fn t94_nested_empty_generic_inner() {
    reject("t94", "fn f() -> A<B<>> { }");
}

#[test]
fn t94b_nested_empty_generic_leading() {
    reject("t94b", "fn f() -> A<B<>, C> { }");
}

#[test]
fn t94c_nested_empty_generic_in_struct_field() {
    reject("t94c", "struct S { x: A<B<>> }");
}

#[test]
fn t95_empty_generic_in_turbofish_call() {
    reject("t95", "fn f() { let x = foo::<>(1); }");
}

#[test]
fn t96_empty_generic_struct_turbofish() {
    reject("t96", "fn f() { let p = Foo::<> { }; }");
}

#[test]
fn t97_comparison_not_empty_generic() {
    // `a <> b` should NOT report "empty generic argument list"
    let err = parse_module("fn f() { let x = a <> b; }").expect_err("must error");
    let msg = format!("{err:#}");
    assert!(
        !msg.contains("empty generic argument list"),
        "comparison a <> b must not report empty-generic: {msg}"
    );
}

#[test]
fn t98_empty_generic_enum() {
    reject_msg("t98", "enum E<> { V }", "empty generic parameter list");
}

#[test]
fn t99_empty_generic_struct_decl() {
    reject_msg("t99", "struct S<> { x: Felt }", "empty generic parameter list");
}

// ═══════════════════════════════════════════════════════════════════════════
// (11) Context sensitivity — generics in control-flow predicates vs bodies
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t100_if_condition_with_lt() {
    accept("t100", "fn f() { if a < b { } }");
}

#[test]
fn t100b_while_condition_with_lt() {
    accept("t100b", "fn f() { while a < b { } }");
}

#[test]
fn t101_for_range_end_generic_not_struct() {
    accept("t101", "fn f() { for i in 0u32..vec::<Felt> { } }");
}

#[test]
fn t102_generic_type_in_if_body() {
    accept("t102", "fn f() { if a { let p = Foo<Felt> { x: 1 }; } }");
}

#[test]
fn t102b_generic_struct_literal_in_if_body() {
    accept("t102b", "fn f() { if a { let p = Foo::<Felt> { x: 1 }; } }");
}

#[test]
fn t103_struct_literal_in_call_arg_in_predicate() {
    accept("t103", "fn f() { if foo(Bar { x: 1 }) { } }");
}

#[test]
fn t104_match_scrutinee_with_lt() {
    accept("t104", "fn f() { let r = match a < b { _ => 1 }; }");
}

#[test]
fn t105_for_if_index_access_regression() {
    accept("t105", "fn f() { for i in 0u32..N { if self[i as Felt] != rhs[i as Felt] { same = false; } } }");
}

#[test]
fn t106_generic_call_in_if_condition() {
    accept("t106", "fn f() { if foo::<Felt>(1) { } }");
}

#[test]
fn t107_generic_call_in_while_condition() {
    accept("t107", "fn f() { while foo::<Felt>(1) { } }");
}

#[test]
fn t108_turbofish_in_for_range() {
    accept("t108", "fn f() { for i in foo::<Felt>(0)..N { } }");
}

#[test]
fn t109_nested_generic_type_in_while_body() {
    accept("t109", "fn f() { while a { let x: A<B<C>> = y; } }");
}

#[test]
fn t110_generic_struct_literal_in_while_body() {
    accept("t110", "fn f() { while a { let p = Foo<Felt> { }; } }");
}

#[test]
fn t111_generic_type_annotation_in_if_condition() {
    // `if a < B > c` — the < > should be comparison, not generics
    accept("t111", "fn f() { if a < B > c { } }");
}

// ═══════════════════════════════════════════════════════════════════════════
// (12) Edge cases — unclosed, trailing comma, multiple contexts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t120_unclosed_generic() {
    reject("t120", "fn f() -> A<B { }");
}

#[test]
fn t121_trailing_comma_in_generics() {
    accept("t121", "fn f() -> A<T,> { }");
}

#[test]
fn t121b_trailing_comma_multi_arg() {
    accept("t121b", "fn f() -> A<T, U,> { }");
}

#[test]
fn t122_trailing_comma_in_generic_params() {
    accept("t122", "fn f<T,>() {}");
}

#[test]
fn t123_generic_fn_param_with_nested() {
    accept("t123", "fn f(x: A<B<C>>) {}");
}

#[test]
fn t124_generic_return_with_nested() {
    accept("t124", "fn f() -> A<B<C<D>>> { }");
}

#[test]
fn t125_generic_in_tuple_type() {
    accept("t125", "fn f() -> (A<B>, C<D>) { }");
}

#[test]
fn t126_generic_in_fn_signature_type() {
    accept("t126", "fn f() -> fn(A<B>) -> C<D> { }");
}

#[test]
fn t127_obsolete_pound_turbofish_rejected() {
    reject_msg("t127", "fn f() { let x = foo#<Felt>(1); }", "obsolete `#<...>` monomorphization");
}

#[test]
fn t128_generic_call_then_member_access() {
    accept("t128", "fn f() { let x = foo::<Felt>(1).field; }");
}

#[test]
fn t129_generic_call_then_index() {
    accept("t129", "fn f() { let x = foo::<Felt>(1)[0]; }");
}

#[test]
fn t130_generic_call_then_cast() {
    accept("t130", "fn f() { let x = foo::<Felt>(1) as Bool; }");
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional >> splitting edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t131_shift_assign_after_nested_generic() {
    // `A<B<C>>` followed by `>>=` — the `>>=` is shift-assign, not a
    // generic closer. The cursor's eat_type_gt splits `>>=` into `>` + `>=`.
    accept("t131", "fn f() { let x: A<B<C>> = y; x >>= 2; }");
}

#[test]
fn t132_triple_nested_with_shift_after() {
    accept("t132", "fn f() { let x: A<B<C<D>>> = y; let z = x >> 1; }");
}

#[test]
fn t133_generic_then_geq_comparison() {
    // `A<B> >= C` — the `>=` is comparison, not a split generic closer.
    accept("t133", "fn f() { let x = a < b >= c; }");
}

#[test]
fn t134_nested_generic_in_turbofish_call_arg() {
    accept("t134", "fn f() { let x = foo::<A<B<C>>>(y); }");
}

#[test]
fn t135_generic_struct_literal_with_nested_type() {
    accept("t135", "fn f() { let p = Foo<A<B>> { }; }");
}

#[test]
fn t136_generic_struct_literal_turbofish_nested() {
    accept("t136", "fn f() { let p = Foo::<A<B>> { }; }");
}

#[test]
fn t137_mixed_bare_and_turbofish_in_same_expr() {
    accept("t137", "fn f() { let x = Foo<A> { }; let y = Bar::<A> { }; }");
}

#[test]
fn t138_qualified_generic_struct_with_nested_turbofish() {
    accept("t138", "fn f() { let p = outer::Foo::<A<B>> { }; }");
}

#[test]
fn t139_turbofish_closer_adjacent_to_gte() {
    accept("t139", "fn f() { let value = foo::<T>>=limit; }");
}

#[test]
fn t140_nested_turbofish_closers_adjacent_to_gte() {
    accept("t140", "fn f() { let value = foo::<A<B>>>=limit; }");
}

#[test]
fn t141_member_turbofish_closer_adjacent_to_gte() {
    accept("t141", "fn f() { let value = object.method::<A<B>>>=limit; }");
}

#[test]
fn t142_turbofish_gte_with_comments_after_rhs() {
    accept(
        "t142",
        "fn f() { let value = foo::<T>>=/* rhs begins */limit && other; }",
    );
}

#[test]
fn t143_adjacent_gte_followed_by_nested_generic_expression() {
    accept(
        "t143",
        "fn f() { let value = foo::<A<B<C>>>>=bar::<D<E<F>>>(arg); }",
    );
}

#[test]
fn t139_empty_generic_in_nested_turbofish_rejected() {
    reject("t139", "fn f() { let x = foo::<A<>> (1); }");
}

#[test]
fn t140_empty_generic_in_qualified_path_rejected() {
    reject("t140", "fn f() -> mod::<>::Ty { }");
}

#[test]
fn t144_comments_between_nested_generic_closers() {
    accept(
        "t144",
        "fn f(value: Outer<Middle<Inner> /* inner */ >) -> Outer<Middle<Inner> /* return */ > { value }",
    );
}

#[test]
fn t145_deep_qualified_nested_generics_with_const_and_array() {
    accept(
        "t145",
        "fn f(value: root::Outer<left::Middle<[Felt; 2]>, right::Leaf<3u32>>) { }",
    );
}

#[test]
fn t146_nested_member_turbofish_with_tuple_and_array_types() {
    accept(
        "t146",
        "fn f() { let value = object.method::<Outer<(Inner<Felt>, [u32; 2])>>([1u32, 2u32]); }",
    );
}

#[test]
fn t147_deep_turbofish_gte_then_shift_expression() {
    accept(
        "t147",
        "fn f() { let value = foo::<A<B<C<D>>>>>=limit >> shift; }",
    );
}

#[test]
fn t148_two_adjacent_deep_generic_calls_in_comparison() {
    accept(
        "t148",
        "fn f() { let value = foo::<A<B<C>>>(x) >= bar::<D<E<F>>>(y); }",
    );
}
