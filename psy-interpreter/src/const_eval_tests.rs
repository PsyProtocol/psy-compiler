// Deep QA for compile-time constant evaluation in PSY.
//
// Drives the real parser + sema + interpreter through
// `Interpreter::typecheck_single` with temporary `.psy` sources. Const values
// are folded by the *interpreter's* `Evaluator::evaluate_expr` (invoked from
// `visit_const` during the predecl phase). This suite reads the folded value
// back out of the symbol table to verify the compile-time computation, and
// separately checks accept/reject (including panic detection, since some
// unsupported const expressions hit `unreachable!()`/`todo!()` inside
// `__interpret_expr__` instead of returning a clean error).
//
// Each case owns an independent symbol table and uniquely named temporary file.

use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};

use super::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Outcome of compiling a source snippet.
enum Outcome {
    /// Typechecked cleanly; holds the live interpreter + symbol tables.
    Accept(Compiled),
    /// Typechecker returned a structured error.
    Reject(String),
    /// Compilation panicked (e.g. `unreachable!()` / `todo!()` in the const
    /// interpreter path) instead of yielding a clean error.
    Panic(String),
}

struct Compiled {
    #[allow(dead_code)]
    interpreter: Interpreter<SymFeltRef, QExecContext>,
    #[allow(dead_code)]
    typechecker: TypeChecker<SymFeltRef, QExecContext>,
    ctx: TypeCheckerVisitorContext<SymFeltRef, QExecContext>,
}

fn compile(source: &str, label: &str) -> Outcome {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_const_{label}_{unique}_{n}.psy"));
    fs::write(&path, source).unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interpreter.typecheck_single(path.clone())
    }));

    let _ = fs::remove_file(path);

    match result {
        Ok(Ok((typechecker, ctx))) => Outcome::Accept(Compiled { interpreter, typechecker, ctx }),
        Ok(Err(e)) => Outcome::Reject(format!("{e:#}")),
        Err(p) => {
            let msg = if let Some(s) = p.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = p.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic>".to_string()
            };
            Outcome::Panic(msg)
        }
    }
}

/// Read back the compile-time folded value of a top-level (or module-level)
/// const by name. Returns the `CheckedValue` variant tag and the folded u64
/// payload. `None` if the name does not resolve to a `Type::Const`.
fn const_value(c: &mut Compiled, name: &str) -> Option<(&'static str, u64)> {
    let name_id = c.ctx.program.interner.intern_ident(name);
    let key: TypeKey = name_id.into();

    // Robust lookup: scan every module's scope type table for a TypeKey whose
    // name is this ident. (After typecheck the "current scope" is no longer
    // reliable — scope_stack/module_stack have been popped.)
    let mut found: Option<TypeId> = None;
    for module in c.ctx.symbols.modules() {
        if let Some(&tid) = c.ctx.symbols[module.scope_id].types.get(&key) {
            found = Some(tid);
            break;
        }
    }

    let type_id = found?;
    let const_node = c.ctx.symbols[type_id].as_const()?.clone();
    let vref = c.ctx.symbols.get_constant_value(const_node.value);
    let b = vref.borrow();
    match &*b {
        CheckedValue::Felt(f) => Some(("Felt", f.get_constant_value())),
        CheckedValue::U32(f) => Some(("U32", f.get_constant_value())),
        CheckedValue::Bool(f) => Some(("Bool", f.get_constant_value())),
        _ => None,
    }
}

fn expect_accept(label: &str, source: &str) {
    match compile(source, label) {
        Outcome::Accept(_) => {}
        Outcome::Reject(msg) => panic!("[{label}] expected accept, got reject:\n{msg}"),
        Outcome::Panic(msg) => panic!("[{label}] expected accept, got PANIC:\n{msg}"),
    }
}

fn expect_reject(label: &str, source: &str, needle: &str) {
    match compile(source, label) {
        Outcome::Accept(_) => panic!("[{label}] expected reject mentioning `{needle}`, got accept"),
        Outcome::Reject(msg) => assert!(
            msg.contains(needle),
            "[{label}] unexpected error (wanted substring `{needle}`):\n{msg}"
        ),
        Outcome::Panic(msg) => panic!("[{label}] expected reject mentioning `{needle}`, got PANIC:\n{msg}"),
    }
}

/// Compile, then assert a top-level const folded to the expected variant and
/// u64 value.
fn expect_const_value(label: &str, source: &str, name: &str, want_variant: &str, want_val: u64) {
    let mut c = match compile(source, label) {
        Outcome::Accept(c) => c,
        Outcome::Reject(msg) => panic!("[{label}] expected accept, got reject:\n{msg}"),
        Outcome::Panic(msg) => panic!("[{label}] expected accept, got PANIC:\n{msg}"),
    };
    match const_value(&mut c, name) {
        Some((variant, val)) => {
            assert_eq!(
                variant, want_variant,
                "[{label}] const `{name}` variant mismatch: got {variant} want {want_variant}"
            );
            assert_eq!(
                val, want_val,
                "[{label}] const `{name}` value mismatch: got {val:#x} want {want_val:#x}"
            );
        }
        None => panic!("[{label}] const `{name}` not found / not a foldable numeric const"),
    }
    drop(c);
}

// ═══════════════════════════════════════════════════════════════════════════
// (1) Const arithmetic — Felt
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c01_felt_add() {
    expect_const_value("c01_felt_add", "const A: Felt = 1 + 2;\nfn main() {}", "A", "Felt", 3);
}

#[test]
fn c02_felt_sub() {
    expect_const_value("c02_felt_sub", "const A: Felt = 10 - 3;\nfn main() {}", "A", "Felt", 7);
}

#[test]
fn c03_felt_mul() {
    expect_const_value("c03_felt_mul", "const A: Felt = 4 * 5;\nfn main() {}", "A", "Felt", 20);
}

#[test]
fn c04_felt_div() {
    expect_const_value("c04_felt_div", "const A: Felt = 20 / 4;\nfn main() {}", "A", "Felt", 5);
}

#[test]
fn c05_felt_mod() {
    expect_const_value("c05_felt_mod", "const A: Felt = 17 % 5;\nfn main() {}", "A", "Felt", 2);
}

#[test]
fn c06_felt_precedence_mul_before_add() {
    // 1 + 2 * 3 == 7, not 9
    expect_const_value("c06_felt_precedence", "const A: Felt = 1 + 2 * 3;\nfn main() {}", "A", "Felt", 7);
}

#[test]
fn c07_felt_parens_override_precedence() {
    // (1 + 2) * 3 == 9
    expect_const_value("c07_felt_parens", "const A: Felt = (1 + 2) * 3;\nfn main() {}", "A", "Felt", 9);
}

#[test]
fn c08_felt_nested_expr() {
    // ((2 + 3) * (4 - 1)) == 15
    expect_const_value("c08_felt_nested", "const A: Felt = (2 + 3) * (4 - 1);\nfn main() {}", "A", "Felt", 15);
}

// ═══════════════════════════════════════════════════════════════════════════
// (1b) Const-to-const transitive folding
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c09_const_references_const() {
    expect_const_value(
        "c09_const_refs",
        "const A: Felt = 3;\nconst B: Felt = A * 4;\nfn main() {}",
        "B", "Felt", 12,
    );
}

#[test]
fn c10_const_transitive_chain() {
    // A=2, B=A+3=5, C=B*B=25
    expect_const_value(
        "c10_transitive_chain",
        "const A: Felt = 2;\nconst B: Felt = A + 3;\nconst C: Felt = B * B;\nfn main() {}",
        "C", "Felt", 25,
    );
}

#[test]
fn c11_const_mixed_with_literal() {
    // A=5, B=A*2 + 1 = 11
    expect_const_value(
        "c11_mixed_literal",
        "const A: Felt = 5;\nconst B: Felt = A * 2 + 1;\nfn main() {}",
        "B", "Felt", 11,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (2) All binary operators on Felt / u32 / bool consts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c12_felt_eq_true() {
    expect_const_value("c12_felt_eq_true", "const A: bool = 3 == 3;\nfn main() {}", "A", "Bool", 1);
}

#[test]
fn c13_felt_eq_false() {
    expect_const_value("c13_felt_eq_false", "const A: bool = 3 == 4;\nfn main() {}", "A", "Bool", 0);
}

#[test]
fn c14_felt_neq() {
    expect_const_value("c14_felt_neq", "const A: bool = 5 != 5;\nfn main() {}", "A", "Bool", 0);
}

#[test]
fn c15_felt_lt() {
    expect_const_value("c15_felt_lt", "const A: bool = 2 < 3;\nfn main() {}", "A", "Bool", 1);
}

#[test]
fn c16_felt_gt() {
    expect_const_value("c16_felt_gt", "const A: bool = 3 > 2;\nfn main() {}", "A", "Bool", 1);
}

#[test]
fn c17_felt_lte() {
    expect_const_value("c17_felt_lte", "const A: bool = 3 <= 3;\nfn main() {}", "A", "Bool", 1);
}

#[test]
fn c18_felt_gte() {
    expect_const_value("c18_felt_gte", "const A: bool = 3 >= 4;\nfn main() {}", "A", "Bool", 0);
}

#[test]
fn c19_u32_add() {
    expect_const_value("c19_u32_add", "const A: u32 = 10u32 + 5u32;\nfn main() {}", "A", "U32", 15);
}

#[test]
fn c20_u32_mul() {
    expect_const_value("c20_u32_mul", "const A: u32 = 6u32 * 7u32;\nfn main() {}", "A", "U32", 42);
}

#[test]
fn c21_u32_sub() {
    expect_const_value("c21_u32_sub", "const A: u32 = 100u32 - 37u32;\nfn main() {}", "A", "U32", 63);
}

#[test]
fn c22_u32_div() {
    expect_const_value("c22_u32_div", "const A: u32 = 84u32 / 4u32;\nfn main() {}", "A", "U32", 21);
}

#[test]
fn c23_u32_mod() {
    expect_const_value("c23_u32_mod", "const A: u32 = 17u32 % 5u32;\nfn main() {}", "A", "U32", 2);
}

#[test]
fn c24_u32_eq() {
    expect_const_value("c24_u32_eq", "const A: bool = 7u32 == 7u32;\nfn main() {}", "A", "Bool", 1);
}

#[test]
fn c25_u32_lt() {
    expect_const_value("c25_u32_lt", "const A: bool = 2u32 < 9u32;\nfn main() {}", "A", "Bool", 1);
}

#[test]
fn c26_bool_and() {
    expect_const_value("c26_bool_and", "const A: bool = true && false;\nfn main() {}", "A", "Bool", 0);
}

#[test]
fn c27_bool_or() {
    expect_const_value("c27_bool_or", "const A: bool = true || false;\nfn main() {}", "A", "Bool", 1);
}

#[test]
fn c28_bool_not() {
    expect_const_value("c28_bool_not", "const A: bool = !true;\nfn main() {}", "A", "Bool", 0);
}

#[test]
fn c29_bool_from_comparison_chain() {
    // (3 < 5) && (5 > 2) == true
    expect_const_value(
        "c29_bool_chain",
        "const A: bool = (3 < 5) && (5 > 2);\nfn main() {}",
        "A", "Bool", 1,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (3) Cross-module const references (visibility + ordering)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c30_cross_module_pub_const() {
    // Child module `m` is predecl'd before the parent (post-order traversal),
    // so `m::C` is registered before `D` is evaluated.
    expect_const_value(
        "c30_cross_module",
        "pub mod m { pub const C: Felt = 7; }\nconst D: Felt = m::C + 1;\nfn main() {}",
        "D", "Felt", 8,
    );
}

#[test]
fn c31_cross_module_transitive() {
    expect_const_value(
        "c31_cross_module_trans",
        "pub mod m { pub const A: Felt = 2; pub const B: Felt = A * 5; }\nconst C: Felt = m::B + 1;\nfn main() {}",
        "C", "Felt", 11,
    );
}

#[test]
fn c32_private_const_in_same_module() {
    // non-pub const usable within the same (root) module
    expect_const_value(
        "c32_private_const",
        "const INNER: Felt = 9;\nconst OUTER: Felt = INNER + 1;\nfn main() {}",
        "OUTER", "Felt", 10,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (3b) Ordering limitation — forward references are NOT supported
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c33_forward_const_reference_rejected() {
    // B references A, but A is declared AFTER B. During predecl, definitions
    // are processed in source order, so A is not yet registered when B's value
    // is evaluated. This must be rejected (ordering dependency — a limitation,
    // not a computation bug).
    expect_reject(
        "c33_forward_ref",
        "const B: Felt = A + 1;\nconst A: Felt = 5;\nfn main() {}",
        "UnresolvedType",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (4) Const as array size
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c34_named_const_array_size_typechecks() {
    expect_accept(
        "c34_named_const_array_size",
        "const N: Felt = 5;\nfn main() {\n    let arr: [Felt; N] = [0, 0, 0, 0, 0];\n}",
    );
}

#[test]
fn c35_literal_array_size_typechecks() {
    expect_accept("c35_literal_array_size", "fn main() {\n    let arr: [Felt; 4] = [0, 0, 0, 0];\n}");
}

#[test]
fn c36_named_const_array_size_value() {
    expect_accept(
        "c36_array_size_uses_const_value",
        "const N: Felt = 3;\nfn main() {\n    let arr: [Felt; N] = [0, 0, 0];\n}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (5) Const in turbofish
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c37_turbofish_literal_const_arg() {
    // foo::<3u32>(x): literal u32 const arg via parse_monomorphization_ty.
    // Const generics are declared as `<N: u32>` (a u32-constrained type
    // variable), NOT `<const N: u32>` — the `const` keyword is rejected.
    expect_accept(
        "c37_turbofish_literal",
        "fn foo<N: u32>(x: Felt) -> Felt { return x; }\nfn main() { let r = foo::<3u32>(5); }",
    );
}

#[test]
fn c38_turbofish_u32_literal_const_arg() {
    expect_accept(
        "c38_turbofish_u32_literal",
        "fn foo<N: u32>(x: Felt) -> Felt { return x; }\nfn main() { let r = foo::<3u32>(5); }",
    );
}

#[test]
fn c39_turbofish_named_const_arg() {
    // foo::<N>(x) where N is a named const — parse_monomorphization_ty falls
    // through to parse_path_ty, resolving N to its Type::Const.
    expect_accept(
        "c39_turbofish_named",
        "const N: u32 = 3u32;\nfn foo<M: u32>(x: Felt) -> Felt { return x; }\nfn main() { let r = foo::<N>(5); }",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (6) Const in struct field array sizes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c40_struct_field_named_const_array_size() {
    expect_accept(
        "c40_struct_field_const_size",
        "const N: Felt = 4;\npub struct S { arr: [Felt; N] }\nfn main() {}",
    );
}

#[test]
fn c41_struct_field_literal_array_size() {
    expect_accept(
        "c41_struct_field_literal_size",
        "pub struct S { arr: [Felt; 4] }\nfn main() {}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (7) Const u32 vs Felt type correctness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c42_u32_const_value() {
    expect_const_value("c42_u32_const", "const C: u32 = 42u32;\nfn main() {}", "C", "U32", 42);
}

#[test]
fn c43_felt_const_value() {
    expect_const_value("c43_felt_const", "const C: Felt = 42;\nfn main() {}", "C", "Felt", 42);
}

#[test]
fn c44_u32_const_type_mismatch_rejected() {
    // Declaring a u32 const with a Felt literal value: `1` (no suffix) is a
    // Felt. unify(u32, felt) should fail.
    expect_reject("c44_u32_with_felt_literal", "const C: u32 = 1;\nfn main() {}", "TypeMismatch");
}

#[test]
fn c45_felt_const_type_mismatch_rejected() {
    // Declaring a Felt const with a u32 literal: `1u32` is u32. unify(felt, u32)
    // should fail.
    expect_reject("c45_felt_with_u32_literal", "const C: Felt = 1u32;\nfn main() {}", "TypeMismatch");
}

#[test]
fn c46_felt_const_used_as_array_size() {
    expect_accept(
        "c46_felt_const_array_size",
        "const C: Felt = 3;\nfn main() {\n    let arr: [Felt; C] = [0, 0, 0];\n}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (8) Const bool used in if conditions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c47_const_bool_in_if() {
    expect_accept(
        "c47_const_bool_in_if",
        "const FLAG: bool = true;\nfn main() {\n    if FLAG {\n        let x = 1;\n    }\n}",
    );
}

#[test]
fn c48_const_false_in_if() {
    expect_accept(
        "c48_const_false_in_if",
        "const FLAG: bool = false;\nfn main() {\n    if FLAG {\n        let x = 1;\n    }\n}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (9) Const zero / false edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c49_felt_zero() {
    expect_const_value("c49_felt_zero", "const Z: Felt = 0;\nfn main() {}", "Z", "Felt", 0);
}

#[test]
fn c50_u32_zero() {
    expect_const_value("c50_u32_zero", "const Z: u32 = 0u32;\nfn main() {}", "Z", "U32", 0);
}

#[test]
fn c51_bool_false() {
    expect_const_value("c51_bool_false", "const F: bool = false;\nfn main() {}", "F", "Bool", 0);
}

#[test]
fn c52_bool_true() {
    expect_const_value("c52_bool_true", "const T: bool = true;\nfn main() {}", "T", "Bool", 1);
}

#[test]
fn c53_zero_identity_add() {
    // 0 + 5 == 5 (exercises the `b == 0` shortcut AND the constant fold)
    expect_const_value("c53_zero_add", "const A: Felt = 0 + 5;\nfn main() {}", "A", "Felt", 5);
}

#[test]
fn c54_zero_mul() {
    // 0 * 5 == 0
    expect_const_value("c54_zero_mul", "const A: Felt = 0 * 5;\nfn main() {}", "A", "Felt", 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// (10) Const in for-loop ranges
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c55_const_in_for_range_u32() {
    expect_accept(
        "c55_for_range_u32",
        "const N: u32 = 3u32;\nfn main() {\n    for i in 0u32..N {\n        let x = i;\n    }\n}",
    );
}

#[test]
fn c56_const_in_for_range_felt() {
    expect_accept(
        "c56_for_range_felt",
        "const N: Felt = 3;\nfn main() {\n    for i in 0..N {\n        let x = i;\n    }\n}",
    );
}

#[test]
fn c57_literal_for_range() {
    expect_accept("c57_for_range_literal", "fn main() {\n    for i in 0u32..5u32 {\n        let x = i;\n    }\n}");
}

/// Size positions can be nested: `fn f<N>(x: [[Felt; N]; 2])` — the
/// literal must still promote N to a Const through the nesting.
#[test]
fn c90_nested_array_size_position_promotes() {
    expect_accept(
        "c90_nested_size_position",
        "fn take<N: Felt>(grid: [[Felt; N]; 2]) -> Felt { return grid[0][0]; }\nfn main() { let g: [[Felt; 3]; 2] = [[1, 2, 3], [4, 5, 6]]; let v = take(g); }",
    );
}

/// Const-literal promotion is limited to size-position parameters. Plain
/// type parameters keep normal inference: `two<T>(a: T, b: T)` with two
/// *different* literals must infer `T = Felt`, not bind T to `Const(1)`
/// and then reject the second literal (H3 regression).
#[test]
fn c86_plain_generic_literals_do_not_promote() {
    expect_accept(
        "c86_plain_generic_literals",
        "fn two<T>(a: T, b: T) -> T { return a; }\nfn main() { let z = two(1, 2); }",
    );
    expect_accept(
        "c86_plain_generic_same_literal",
        "fn two<T>(a: T, b: T) -> T { return a; }\nfn main() { let z = two(1, 1); }",
    );
    expect_accept(
        "c86_plain_generic_mixed",
        "fn two<T>(a: T, b: T) -> T { return a; }\nfn main(q: Felt) { let z = two(q, 2); }",
    );
}

/// The size-position promotion still works where it matters: the std
/// `split_bits(x, 64)` wrapper binds N to `Const(64)` and evaluates.
#[test]
fn c87_split_bits_const_length_still_works() {
    expect_accept(
        "c87_split_bits_const",
        "fn main(x: Felt) {\n    let bits = split_bits(x, 64);\n    let b0 = bits[0];\n}",
    );
    expect_accept(
        "c87_split_bits_const_expr",
        "fn main(x: Felt) {\n    let bits = split_bits(x, 30 + 34);\n    let b0 = bits[0];\n}",
    );
}

/// A user-defined generic wrapper can forward its unresolved const generic
/// to the std `split_bits` wrapper. `M` becomes `Const(4)` only when `wrap`
/// is instantiated; rejecting `m: M` while checking the generic body is a
/// regression from the pre-gate behavior.
#[test]
fn c91_split_bits_const_generic_can_be_forwarded_through_user_wrapper() {
    expect_accept(
        "c91_split_bits_generic_wrapper",
        "fn wrap<M: Felt>(x: Felt, m: M) -> [Felt; M] { return split_bits(x, m); }\n\
         fn main() { let bits: [Felt; 4] = wrap::<4>(15, 4); assert_eq(sum_bits(bits), 15, \"wrapped split\"); }",
    );
}

/// A runtime length must be rejected during typechecking, before circuit
/// interpretation can mistake a symbolic input index for a bit count (H4).
#[test]
fn c88_split_bits_runtime_length_rejected_at_typecheck() {
    expect_reject(
        "c88_split_bits_runtime_rejected",
        "fn main(x: Felt, n: Felt) {\n    let bits = split_bits(x, n);\n    let b0 = bits[0];\n}",
        "TypeMismatch",
    );
}

/// A negated literal length is rejected during typechecking rather than
/// wrapping in the field and reaching an attempted huge allocation (H9).
#[test]
fn c89_split_bits_negative_length_rejected_at_typecheck() {
    expect_reject(
        "c89_split_bits_negative_rejected",
        "fn main(x: Felt) {\n    let bits = split_bits(x, -64);\n    let b0 = bits[0];\n}",
        "TypeMismatch",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (11) Nested const in runtime expressions (const ↔ variable interaction)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c58_const_plus_runtime_var() {
    expect_accept(
        "c58_const_plus_var",
        "const C: Felt = 5;\nfn main(b: Felt) -> Felt {\n    let x = C + b;\n    return x;\n}",
    );
}

#[test]
fn c59_runtime_var_times_const() {
    expect_accept(
        "c59_var_times_const",
        "const C: Felt = 3;\nfn main(b: Felt) -> Felt {\n    let z = b * C;\n    return z;\n}",
    );
}

#[test]
fn c60_const_in_let_binding() {
    expect_accept(
        "c60_const_in_let",
        "const C: Felt = 7;\nfn main() -> Felt {\n    let x = C;\n    return x;\n}",
    );
}

#[test]
fn c61_const_as_fn_arg() {
    expect_accept(
        "c61_const_as_fn_arg",
        "const C: Felt = 5;\nfn f(x: Felt) -> Felt { return x; }\nfn main() -> Felt {\n    return f(C);\n}",
    );
}

#[test]
fn c62_const_mixed_arithmetic_with_vars() {
    // CONST_A + CONST_B * 2 + runtime
    expect_accept(
        "c62_mixed_arith",
        "const A: Felt = 2;\nconst B: Felt = 3;\nfn main(b: Felt) -> Felt {\n    let x = A + B * 2 + b;\n    return x;\n}",
    );
}

#[test]
fn c63_u32_const_plus_runtime_u32() {
    expect_accept(
        "c63_u32_const_plus_var",
        "const C: u32 = 5u32;\nfn main(b: u32) -> u32 {\n    let x = C + b;\n    return x;\n}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (12) Const generic parameter used as array size inside a generic fn
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c64_generic_param_as_array_size() {
    expect_accept(
        "c64_generic_param_array_size",
        "fn f<N: Felt>() {\n    let a: [Felt; N] = [0, 0, 0];\n}\nfn main() { f::<3>(); }",
    );
}

#[test]
fn c65_generic_param_array_size_in_struct_field() {
    expect_accept(
        "c65_generic_struct_field_array_size",
        "pub struct S<N: Felt> { arr: [Felt; N] }\nfn main() {}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (13) Large felt const values
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c66_large_felt_const() {
    expect_const_value(
        "c66_large_felt",
        "const BIG: Felt = 1000000000;\nfn main() {}",
        "BIG", "Felt", 1_000_000_000,
    );
}

#[test]
fn c67_large_felt_arithmetic() {
    // 1_000_000_000 * 2 == 2_000_000_000
    expect_const_value(
        "c67_large_felt_arith",
        "const A: Felt = 1000000000;\nconst B: Felt = A * 2;\nfn main() {}",
        "B", "Felt", 2_000_000_000,
    );
}

#[test]
fn c68_felt_add_near_i64_max_no_panic() {
    // Felt arithmetic is modular over the Goldilocks prime (p = 2^64 - 2^32 + 1).
    // 2^63 + 1 is far under p, so the folded result equals the plain sum:
    // no overflow panic and no spurious modular wrap.
    expect_const_value(
        "c68_felt_add_big",
        "const A: Felt = 9223372036854775807;\nconst B: Felt = A + 1;\nfn main() {}",
        "B", "Felt", 9_223_372_036_854_775_808u64,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (14) Folding correctness / loss-of-precision probes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c69_felt_bitxor_does_not_fold() {
    // FINDING (folding gap): `^` is BitXor, not Pow. Felt bitwise ops
    // (BitAnd/BitOr/BitXor/BitShl/BitShr) in interpret_binary (lib.rs:856-860)
    // route to `op_u32_*` opcodes, and `op_std_binary_op_u32`
    // (exec_context.rs:252-255) ONLY folds operands of op-type ConstantU32.
    // A pair of Felt *constants* (op-type `Constant`) therefore does NOT fold:
    // the const compiles (accepts) but silently holds a non-constant symbolic
    // store reference instead of the value 2 XOR 3 = 1. If this gap is ever
    // fixed, this test will fail and must be updated to assert 1.
    let mut c = match compile("const A: Felt = 2 ^ 3;\nfn main() {}", "c69_felt_bitxor") {
        Outcome::Accept(c) => c,
        Outcome::Reject(msg) => panic!("[c69_felt_bitxor] expected accept, got reject:\n{msg}"),
        Outcome::Panic(msg) => panic!("[c69_felt_bitxor] expected accept, got PANIC:\n{msg}"),
    };
    match const_value(&mut c, "A") {
        Some(("Felt", val)) => assert_ne!(
            val, 1,
            "[c69_felt_bitxor] Felt BitXor folded to the correct value 1 — the documented \
             folding gap appears fixed; update this test to assert 1"
        ),
        other => panic!("[c69_felt_bitxor] unexpected const result: {other:?}"),
    }
}

#[test]
fn c70_u32_bitxor_folds() {
    // `^` is BitXor (not Pow): 2 XOR 3 == 1. u32 bitwise ops DO fold because
    // both operands are ConstantU32 (op_std_binary_op_u32, exec_context.rs:255).
    expect_const_value("c70_u32_bitxor", "const A: u32 = 2u32 ^ 3u32;\nfn main() {}", "A", "U32", 1);
}

#[test]
fn c71_neg_felt_const() {
    // -5 as a felt: op_neg(5). Felt is modular over the Goldilocks prime
    // p = 2^64 - 2^32 + 1, so -5 == p - 5.
    expect_const_value(
        "c71_neg_felt",
        "const A: Felt = -5;\nfn main() {}",
        "A", "Felt",
        // p - 5 where p = 2^64 - 2^32 + 1 = 18446744069414584321
        18_446_744_069_414_584_321u64 - 5,
    );
}

#[test]
fn c72_chained_subtraction() {
    // 100 - 30 - 20 == 50 (left-assoc)
    expect_const_value(
        "c72_chained_sub",
        "const A: Felt = 100 - 30 - 20;\nfn main() {}",
        "A", "Felt", 50,
    );
}

#[test]
fn c73_div_then_mul_roundtrip() {
    // (20 / 4) * 4 == 20
    expect_const_value(
        "c73_div_mul_roundtrip",
        "const A: Felt = 20 / 4 * 4;\nfn main() {}",
        "A", "Felt", 20,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Casts: `42u32 as Felt` in const position
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c74_u32_as_felt_const_typechecks() {
    expect_accept(
        "c74_u32_as_felt_const",
        "const A: Felt = 42u32 as Felt;\nfn main() {}",
    );
}

#[test]
fn c75_u32_as_felt_const_value() {
    expect_const_value(
        "c75_u32_as_felt_value",
        "const A: Felt = 42u32 as Felt;\nfn main() {}",
        "A", "Felt", 42,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Mixed-type const arithmetic — probes for the `unreachable!()` panic risk in
// interpret_binary (lib.rs:896) when operand CheckedValue variants don't
// match any arm.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c76_mixed_u32_felt_add_no_panic() {
    // 1u32 + 1 : lhs u32, rhs felt literal (no suffix). Either a clean accept
    // or a clean reject is acceptable; a panic in the const interpreter is a
    // bug.
    match compile("const A: u32 = 1u32 + 1;\nfn main() {}", "c76_mixed_add") {
        Outcome::Accept(_) | Outcome::Reject(_) => {}
        Outcome::Panic(msg) => panic!(
            "[c76_mixed_add] const mixed-type arithmetic panicked instead of clean error:\n{msg}"
        ),
    }
}

#[test]
fn c77_mixed_felt_u32_add_no_panic() {
    match compile("const A: Felt = 1 + 1u32;\nfn main() {}", "c77_mixed_add2") {
        Outcome::Accept(_) | Outcome::Reject(_) => {}
        Outcome::Panic(msg) => panic!(
            "[c77_mixed_add2] const mixed-type arithmetic panicked instead of clean error:\n{msg}"
        ),
    }
}

#[test]
fn c78_const_referencing_type_not_value_no_panic() {
    // const X: Felt = S (a struct type); sema should reject via TypeMismatch
    // before evaluate_expr can run, but if not it must not panic.
    match compile("pub struct S {}\nconst X: Felt = S;\nfn main() {}", "c78_const_is_type") {
        Outcome::Accept(_) | Outcome::Reject(_) => {}
        Outcome::Panic(msg) => panic!("[c78_const_is_type] panicked instead of clean error:\n{msg}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Existing baseline regression (matches tests/const_test.psy shape)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c79_baseline_const_test_shape() {
    // Mirrors tests/const_test.psy: cross-module pub const + top-level const +
    // const used in a runtime expression with a parameter.
    expect_accept(
        "c79_baseline_shape",
        "pub mod const_mod {\n    pub const c: Felt = 2;\n}\nconst a: Felt = 1;\nfn main(b: Felt) -> Felt {\n    return a + b + const_mod::c;\n}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Computed u32 const as array size — confirms the *folded* value (not the
// unevaluated expression) flows into the array size, exercising the full
// path: visit_const folds → Type::Const holds the literal → array-size
// unification reads the folded const.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn c80_computed_felt_const_as_array_size() {
    // N folds to 5 at compile time; the array type [Felt; N] must accept a
    // 5-element literal. This proves the folded const value (not the
 // expression `2 + 3`) is what the size unification consumes.
    expect_accept(
        "c80_computed_const_array_size",
        "const N: Felt = 2 + 3;\nfn main() {\n    let arr: [Felt; N] = [0, 0, 0, 0, 0];\n}",
    );
}

#[test]
fn c81_array_size_mismatch_rejected() {
    // FIXED: The unify for Type::Const now compares both the inner type AND
    // the actual constant value. A 4-element literal against a declared size
    // of 5 now correctly fails with TypeMismatch.
    expect_reject(
        "c81_size_mismatch",
        "const N: Felt = 5;\nfn main() {\n    let arr: [Felt; N] = [0, 0, 0, 0];\n}",
        "TypeMismatch",
    );
}

#[test]
fn c82_const_array_size_checked_at_function_call() {
    expect_reject(
        "c82_const_size_function_arg",
        "const N: Felt = 3;\nfn consume(value: [Felt; N]) {}\nfn main() { consume([1, 2]); }",
        "TypeMismatch",
    );
}

#[test]
fn c83_equal_const_array_sizes_pass_through_function_call() {
    expect_accept(
        "c83_equal_const_size_function_arg",
        "const N: Felt = 3;\nfn consume(value: [Felt; N]) {}\nfn main() { consume([1, 2, 3]); }",
    );
}

#[test]
fn c84_nested_const_array_inner_size_mismatch_rejected() {
    expect_reject(
        "c84_nested_const_inner_size",
        "const INNER: Felt = 2;\nconst OUTER: Felt = 2;\nfn main() { let value: [[Felt; INNER]; OUTER] = [[1, 2], [3, 4, 5]]; }",
        "TypeMismatch",
    );
}

#[test]
fn c85_distinct_named_consts_with_equal_values_unify() {
    expect_accept(
        "c85_equal_named_const_values",
        "const A: Felt = 2;\nconst B: Felt = 2;\nfn consume(value: [Felt; A]) {}\nfn main() { let value: [Felt; B] = [1, 2]; consume(value); }",
    );
}

/// A const parameter can occur below arbitrarily many type constructors.
/// This specifically guards against the former 16-level traversal cutoff:
/// the first argument must be promoted to `Const(3)` before the deeply
/// nested array argument is unified with the signature.
#[test]
fn c90_const_size_position_beyond_sixteen_type_levels() {
    const WRAPPERS: usize = 17;

    let mut nested_ty = "[Felt; N]".to_string();
    let mut nested_value = "[1, 2, 3]".to_string();
    for _ in 0..WRAPPERS {
        nested_ty = format!("[{nested_ty}; 1]");
        nested_value = format!("[{nested_value}]");
    }

    let source = format!(
        "fn consume<N: Felt>(n: N, value: {nested_ty}) {{}}\nfn main() {{ consume(3, {nested_value}); }}"
    );
    expect_accept("c90_deep_const_size_position", &source);
}
