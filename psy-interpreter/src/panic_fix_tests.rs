// Regression tests for QA-found bugs 3-9 (panic-to-clean-error + sema gaps).
//
// Bug 3: member access on a non-struct (`1.foo`) panicked via `as_struct().unwrap()`.
// Bug 4: calling a non-function (`1u32(2u32)`) panicked via `Type::signature`'s
//        `unreachable!()`.
// Bug 5: const div/rem by zero (`const X = 1u32 / 0u32`) panicked in the VM
//        constant fold.
// Bug 6: const u32 overflow (`const X = 4294967295u32 + 1u32`) panicked in the
//        VM constant fold.
// Bug 7: runtime div by zero (`1u32 / 0u32` in a fn body) panicked.
// Bug 8: runtime array OOB (`a[5]` on a 3-element array) panicked.
// Bug 9: immutable assignment (`let x=1; x=2;`) was only checked on the
//        runtime/interpret path, silently accepted by non-test typecheck.
//
// Every case asserts a clean accept/reject and that no path panics. Each case owns
// an independent symbol table and uniquely named temporary file.

use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};

use super::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

enum Outcome {
    Accept(Compiled),
    Reject(String),
    Panic(String),
}

struct Compiled {
    interpreter: Interpreter<SymFeltRef, QExecContext>,
    typechecker: TypeChecker<SymFeltRef, QExecContext>,
    ctx: TypeCheckerVisitorContext<SymFeltRef, QExecContext>,
}

fn compile(source: &str, label: &str) -> Outcome {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Keep the filename short: some platforms reject very long temp paths and
    // that would surface as a spurious panic unrelated to the bug under test.
    let path = std::env::temp_dir().join(format!("psy_pf_{n}_{unique}.psy"));
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

fn find_function(c: &mut Compiled, name: &str) -> Option<TypeId> {
    let name_id = c.ctx.program.interner.intern_ident(name);
    let key: TypeKey = name_id.into();
    for module in c.ctx.symbols.modules() {
        if let Some(&tid) = c.ctx.symbols[module.scope_id].types.get(&key) {
            if c.ctx.symbols[tid].as_function().is_some() {
                return Some(tid);
            }
        }
    }
    None
}

/// After a clean typecheck, interpret `main` (no args) and return:
///   `Ok(None)` if it ran without error,
///   `Ok(Some(msg))` if the interpreter returned a clean error,
///   `Err(msg)` if interpreting panicked.
fn run_main(source: &str, label: &str) -> Result<Option<String>, String> {
    run_main_with_args(source, label, vec![])
}

/// Same as `run_main`, but interprets `main` with the given argument
/// count (each bound to a fresh symbolic input).
fn run_main_with_args(source: &str, label: &str, args: Vec<CheckedValueRef<SymFeltRef>>) -> Result<Option<String>, String> {
    let mut c = match compile(source, label) {
        Outcome::Accept(c) => c,
        Outcome::Reject(msg) => return Ok(Some(format!("[compile rejected] {msg}"))),
        Outcome::Panic(msg) => return Err(format!("[compile panicked] {msg}")),
    };
    let main_tid = match find_function(&mut c, "main") {
        Some(tid) => tid,
        None => return Ok(Some("[no main]".to_string())),
    };
    let program = &c.typechecker.program;
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.interpreter.interpret_function(program, main_tid, args, &mut c.ctx)
    }));
    match run {
        Ok(Ok(_)) => Ok(None),
        Ok(Err(e)) => Ok(Some(format!("{e:#}"))),
        Err(p) => {
            let msg = if let Some(s) = p.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = p.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic>".to_string()
            };
            Err(msg)
        }
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

fn expect_accept(label: &str, source: &str) {
    match compile(source, label) {
        Outcome::Accept(_) => {}
        Outcome::Reject(msg) => panic!("[{label}] expected accept, got reject:\n{msg}"),
        Outcome::Panic(msg) => panic!("[{label}] expected accept, got PANIC:\n{msg}"),
    }
}

fn expect_input_materialization_error(c: &mut Compiled, function_ty: TypeId, label: &str) {
    let function = c.ctx.symbols[function_ty].as_function().expect("function type");
    let parameter_ty = function.parameters[0].ty;
    let location = function.location;
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.interpreter.materialize_input(parameter_ty, &c.ctx.symbols, location)
    }));
    match run {
        Ok(Err(error)) => assert!(format!("{error:#}").contains("ArrayTooLarge"), "[{label}] unexpected error: {error:#}"),
        Ok(Ok(_)) => panic!("[{label}] expected ArrayTooLarge, got success"),
        Err(error) => panic!("[{label}] panicked: {}", panic_message(&error)),
    }
}

#[test]
fn b12_non_bool_not_is_rejected_at_typecheck() {
    expect_reject("b12_felt_not", "fn main() { let value = !1; }", "TypeMismatch");
    expect_reject("b12_u32_not", "fn main() { let value = !1u32; }", "TypeMismatch");
}

#[test]
fn b13_mixed_for_range_endpoint_types_are_rejected() {
    expect_reject(
        "b13_mixed_for_range",
        "fn main() { for i in 0u32..3 { } }",
        "TypeMismatch",
    );
}

/// A generic instantiated with a struct used as a for-range endpoint must
/// be rejected instead of panicking at interpretation (M2): the old check
/// unified (binding) the free variable to FELT, accepting anything.
#[test]
fn b19_generic_struct_for_range_endpoint_rejected() {
    expect_reject(
        "b19_struct_endpoint",
        "pub struct P { pub a: Felt }\nfn loopgen<T>(lo: T, hi: T) -> Felt { let mut s: Felt = 0; for i in lo..hi { s = s + 1; } return s; }\nfn main() { let z: Felt = loopgen::<P>(P { a: 0 }, P { a: 1 }); }",
        "TypeMismatch",
    );
    // The same generic instantiated with Felt keeps working.
    expect_accept(
        "b19_felt_endpoint",
        "fn loopgen<T>(lo: T, hi: T) -> Felt { let mut s: Felt = 0; for i in lo..hi { s = s + 1; } return s; }\nfn main() { let z: Felt = loopgen::<Felt>(0, 5); }",
    );
}

#[test]
fn b14_bool_invalid_compound_assignments_are_rejected() {
    for (label, operator) in [
        ("add", "+="),
        ("sub", "-="),
        ("mul", "*="),
        ("div", "/="),
        ("mod", "%="),
        ("bitand", "&="),
        ("bitor", "|="),
        ("shl", "<<="),
        ("shr", ">>="),
    ] {
        expect_reject(
            &format!("b14_bool_{label}_assign"),
            &format!("fn main() {{ let mut value = true; value {operator} false; }}"),
            "TypeMismatch",
        );
    }
}

#[test]
fn b14_bool_xor_assignment_executes_without_panicking() {
    let source = "fn main() { let mut value = true; value ^= true; assert_eq(value, false); }";
    match run_main(source, "b14_bool_xor_assign") {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[b14_bool_xor_assign] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[b14_bool_xor_assign] panicked:\n{msg}"),
    }
}

/// A runtime `split_bits` length must be refused, not silently turned
/// into a circuit whose bit count is the input variable's *index*.
#[test]
fn b16_split_bits_runtime_length_is_a_typecheck_error() {
    let source = "fn main(x: Felt, n: Felt) { let bits = split_bits(x, n); let b0 = bits[0]; }";
    expect_reject("b16_split_bits_runtime_length", source, "TypeMismatch");
}

/// Downcast a caught-panic payload to a printable message.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic>".to_string()
    }
}

/// A negated literal length (`-64` wraps to p-64) must report
/// ArrayTooLarge instead of aborting with `capacity overflow`.
#[test]
fn b17_split_bits_negative_length_is_a_typecheck_error() {
    expect_reject(
        "b17_split_bits_negative_length",
        "fn main() { let x: Felt = 7; let bits = split_bits(x, -64); let b0 = bits[0]; }",
        "TypeMismatch",
    );
}

/// `u32 **` overflow must report ArithmeticOverflow like the other
/// constant u32 ops, not panic inside the VM (M8).
#[test]
fn b18_u32_pow_overflow_is_arithmetic_overflow() {
    expect_runtime_error(
        "b18_u32_pow_overflow",
        "fn main() -> u32 { return 2u32 ** 40u32; }",
        "ArithmeticOverflow",
    );
    // The overflow guard used to evaluate this with unchecked u64::pow:
    // debug builds panicked in the guard, while release could wrap and let
    // the invalid operation reach the VM.
    expect_runtime_error(
        "b18_u32_pow_guard_overflow",
        "fn main() -> u32 { return 4294967295u32 ** 3u32; }",
        "ArithmeticOverflow",
    );
}

/// Non-overflowing u32 powers keep working (regression guard for the
/// pre-check itself).
#[test]
fn b18_u32_pow_in_range_executes() {
    let source = "fn main() { let value = 2u32 ** 5u32; assert_eq(value, 32u32, \"pow\"); }";
    match run_main(source, "b18_u32_pow_ok") {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[b18_u32_pow_ok] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[b18_u32_pow_ok] panicked: {msg}"),
    }
}

/// The materialization budget is global, not per-node: nested repeats and
/// huge input arrays must stop at the total cap instead of allocating
/// unbounded memory (M4/L2). Each layer here passes the per-node check.
#[test]
fn b20_total_materialization_budget_binds_nested_repeats() {
    expect_runtime_error(
        "b20_nested_repeats_budget",
        "fn main() { let big = [[0; 2048]; 2048]; }",
        "ArrayTooLarge",
    );
}

#[test]
fn b20_input_array_footprint_capped() {
    // The parameter's footprint (5M) exceeds the budget before any
    // allocation happens. Interpreted with a symbolic input bound to the
    // parameter, like b16.
    let source = "fn main(a: [Felt; 5000000]) -> Felt { return a[0]; }";
    let mut c = match compile(source, "b20_input_footprint") {
        Outcome::Accept(c) => c,
        Outcome::Reject(msg) => panic!("[b20_input_footprint] expected typecheck accept, got reject:\n{msg}"),
        Outcome::Panic(msg) => panic!("[b20_input_footprint] typecheck panicked: {msg}"),
    };
    let main_tid = find_function(&mut c, "main").expect("main not found");
    expect_input_materialization_error(&mut c, main_tid, "b20_input_footprint");
}

#[test]
fn b20_deep_input_array_footprint_cannot_bypass_budget() {
    let mut ty = "[Felt; 5000000]".to_string();
    for _ in 0..40 {
        ty = format!("[{ty}; 1]");
    }
    let source = format!("fn main(a: {ty}) -> Felt {{ return 0; }}");
    let mut c = match compile(&source, "b20_deep_input_footprint") {
        Outcome::Accept(c) => c,
        Outcome::Reject(msg) => panic!("[b20_deep_input_footprint] expected typecheck accept, got reject:\n{msg}"),
        Outcome::Panic(msg) => panic!("[b20_deep_input_footprint] typecheck panicked: {msg}"),
    };
    let main_tid = find_function(&mut c, "main").expect("main not found");
    expect_input_materialization_error(&mut c, main_tid, "b20_deep_input_footprint");
}

/// A large-but-legal array still compiles (the budget is 4M total; a
/// 1024x1024 nested repeat is 1M and must pass).
#[test]
fn b20_legal_large_array_still_works() {
    let source = "fn main() { let ok_size = [[0; 1024]; 1024]; assert_eq(ok_size[0][0], 0, \"zero\"); }";
    match run_main(source, "b20_legal_large") {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[b20_legal_large] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[b20_legal_large] panicked: {msg}"),
    }
}

/// Existing argument values are only rebound by internal calls; they are not
/// materialized again. At an already-full budget, forwarding the same array
/// through a helper must therefore remain valid (review P2).
#[test]
fn b28_internal_function_arguments_are_not_charged_again() {
    let source = "fn first(a: [Felt; 2]) -> Felt { return a[0]; }\nfn main() {}";
    let mut c = match compile(source, "b28_reused_function_argument") {
        Outcome::Accept(c) => c,
        Outcome::Reject(message) => panic!("[b28] expected typecheck accept, got reject:\n{message}"),
        Outcome::Panic(message) => panic!("[b28] typecheck panicked: {message}"),
    };
    let first_ty = find_function(&mut c, "first").expect("first not found");
    let array_ty = c.ctx.symbols[first_ty].parameters()[0].ty;
    let left = c.interpreter.context.add_input();
    let right = c.interpreter.context.add_input();
    let existing = CheckedValueRef::from_vec(array_ty, vec![left, right]);
    c.interpreter.materialized_elements = MAX_TOTAL_MATERIALIZED_ELEMENTS;

    let program = &c.typechecker.program;
    for _ in 0..2 {
        c.interpreter
            .interpret_function(program, first_ty, vec![existing.clone()], &mut c.ctx)
            .unwrap_or_else(|error| panic!("[b28] forwarding an existing argument must not be charged: {error:#}"));
    }
    assert_eq!(c.interpreter.materialized_elements, MAX_TOTAL_MATERIALIZED_ELEMENTS);
}

/// A whole-struct Storage::read materializes the struct's full range via
/// __storage_read_range — previously the last uncharged allocation path: a
/// contract with several 1M-array fields read gigabytes past the budget
/// with no cap (self-audit finding after P1/P2).
#[test]
fn b30_storage_read_range_is_charged_globally() {
    let source = "use std::prelude::*;\n#[contract]\n#[derive(Storage)]\npub struct Big { pub a: [Felt; 1000000], pub b: [Felt; 1000000], pub c: [Felt; 1000000], pub d: [Felt; 1000000], pub e: [Felt; 1000000] }\n#[contract_method]\npub fn read_all() -> Felt { let v = Big::read(0, 0, 0, 0); return v.a[0]; }\nfn main() {}";
    let mut graph = psy_common::Graph::new();
    graph.add_node(std::path::PathBuf::from("/virtual/src/main.psy"));
    let result = super::interpret_virtual_files(
        Some("BigRef".to_string()),
        vec!["read_all".to_string()],
        graph,
        vec![(std::path::PathBuf::from("/virtual/src/main.psy"), std::sync::Arc::from(source))],
    );
    match result {
        Err(err) => {
            let msg = format!("{err:#}");
            assert!(msg.contains("ArrayTooLarge"), "[b30_storage_read_range] unexpected error: {msg}");
        }
        Ok(_) => panic!("[b30_storage_read_range] expected ArrayTooLarge, got success"),
    }
}

/// `split_bits` creates a new array, so separate calls must accumulate in the
/// same global materialization budget even when each call is below its
/// per-array cap (review P1).
#[test]
fn b29_split_bits_arrays_are_charged_globally() {
    let source = "fn main() { let a = split_bits(3, 2); let b = split_bits(3, 2); }";
    let mut c = match compile(source, "b29_split_bits_global_budget") {
        Outcome::Accept(c) => c,
        Outcome::Reject(message) => panic!("[b29] expected typecheck accept, got reject:\n{message}"),
        Outcome::Panic(message) => panic!("[b29] typecheck panicked: {message}"),
    };
    let main_ty = find_function(&mut c, "main").expect("main not found");
    c.interpreter.materialized_elements = MAX_TOTAL_MATERIALIZED_ELEMENTS - 3;
    let program = &c.typechecker.program;
    match c.interpreter.interpret_function(program, main_ty, vec![], &mut c.ctx) {
        Err(error) => assert!(format!("{error:#}").contains("ArrayTooLarge"), "[b29] unexpected error: {error:#}"),
        Ok(_) => panic!("[b29] cumulative split_bits allocations must exceed the global budget"),
    }
}

/// Generic self-recursion must be diagnosed, not crash the typechecker
/// with a stack overflow (H1): instantiate_function rewrites the body
/// before registering the instance, so the recursive call inside re-entered
/// instantiation forever.
#[test]
fn b21_generic_self_recursion_diagnosed() {
    expect_reject(
        "b21_generic_self_recursion",
        "fn nest<T>(x: T) -> T { return nest(x); }\nfn main(q: Felt) { let r: Felt = nest(3); }",
        "UnsupportedRecursion",
    );
    // Mutual generic recursion hits the same cycle.
    expect_reject(
        "b21_generic_mutual_recursion",
        "fn a1<T>(x: T) -> T { return a2(x); }\nfn a2<T>(x: T) -> T { return a1(x); }\nfn main(q: Felt) { let r: Felt = a1(3); }",
        "UnsupportedRecursion",
    );
    // Repeated calls after an instance has completed are not recursion.
    expect_accept(
        "b21_finite_generic_chain",
        "fn id<T>(x: T) -> T { return x; }\nfn main() { let a = id(1); let b = id(a); let c = id(b); let d = id(c); }",
    );
}

/// A finite generic call chain may be deeper than the former arbitrary
/// limit of 8. Only a repeated active function is recursion.
#[test]
fn b23_deep_finite_generic_chain_still_works() {
    expect_accept(
        "b23_depth_ten",
        "fn g0<T>(x: T) -> T { return g1(x); }\n\
         fn g1<T>(x: T) -> T { return g2(x); }\n\
         fn g2<T>(x: T) -> T { return g3(x); }\n\
         fn g3<T>(x: T) -> T { return g4(x); }\n\
         fn g4<T>(x: T) -> T { return g5(x); }\n\
         fn g5<T>(x: T) -> T { return g6(x); }\n\
         fn g6<T>(x: T) -> T { return g7(x); }\n\
         fn g7<T>(x: T) -> T { return g8(x); }\n\
         fn g8<T>(x: T) -> T { return g9(x); }\n\
         fn g9<T>(x: T) -> T { return x; }\n\
         fn main() { let a: Felt = g0(1); assert_eq(a, 1, \"deep generic chain\"); }",
    );
}

/// The materialization footprint is structural: struct fields and
/// tuples count toward the budget too (M4 covered plain arrays).
#[test]
fn b25_struct_and_tuple_input_footprints_capped() {
    // Struct containing a huge array: footprint = 5M through the field.
    // Interpreted with one symbolic input bound to the parameter; the
    // footprint check fires during parameter binding, before evaluation.
    let source = "pub struct Big { pub data: [Felt; 5000000] }\nfn main(b: Big) -> Felt { return b.data[0]; }";
    let mut c = match compile(source, "b25_struct_huge_field") {
        Outcome::Accept(c) => c,
        Outcome::Reject(msg) => panic!("[b25_struct_huge_field] expected typecheck accept, got reject:\n{msg}"),
        Outcome::Panic(msg) => panic!("[b25_struct_huge_field] typecheck panicked: {msg}"),
    };
    let main_tid = find_function(&mut c, "main").expect("main not found");
    expect_input_materialization_error(&mut c, main_tid, "b25_struct_huge_field");

    // Tuple of two large arrays: 2.5M + 2.5M = 5M total (footprint sums).
    let source = "fn main(t: ([Felt; 2500000], [Felt; 2500000])) -> Felt { return t.0[0]; }";
    let mut c = match compile(source, "b25_tuple_footprint") {
        Outcome::Accept(c) => c,
        Outcome::Reject(msg) => panic!("[b25_tuple_footprint] expected typecheck accept, got reject:\n{msg}"),
        Outcome::Panic(msg) => panic!("[b25_tuple_footprint] typecheck panicked: {msg}"),
    };
    let main_tid = find_function(&mut c, "main").expect("main not found");
    expect_input_materialization_error(&mut c, main_tid, "b25_tuple_footprint");
}

/// for-range endpoints of other wrong kinds must be rejected too (M2
/// covered struct; bool and array endpoints are the same class).
#[test]
fn b24_bool_and_array_for_range_endpoints_rejected() {
    expect_reject(
        "b24_bool_endpoint",
        "fn main() { for i in 0..true { } }",
        "TypeMismatch",
    );
    expect_reject(
        "b24_array_endpoint",
        "fn main() { for i in 0..[1, 2] { } }",
        "TypeMismatch",
    );
}

/// A method name resolving to a struct/const must produce a diagnostic,
/// not `as_function().unwrap()` panic (M5). Drives the real interpret
/// entry (method-name resolution) via virtual files — no CLI needed.
#[test]
fn b22_method_name_resolving_to_struct_is_a_clean_error() {
    let source = "pub struct Foo { pub x: Felt }\nfn main() -> Felt { return Foo { x: 1 }.x; }\n";
    let mut graph = psy_common::Graph::new();
    graph.add_node(std::path::PathBuf::from("/virtual/src/main.psy"));
    let result = super::interpret_virtual_files(
        None,
        vec!["Foo".to_string()],
        graph,
        vec![(std::path::PathBuf::from("/virtual/src/main.psy"), std::sync::Arc::from(source))],
    );
    match result {
        // The old behavior aborted with `called Option::unwrap() on a
        // None value`; a bail! is the expected clean failure.
        Err(err) => {
            let msg = format!("{err:#}");
            assert!(msg.contains("non-function") || msg.contains("does not name a function"), "unexpected error: {msg}");
        }
        Ok(_) => panic!("[b22_struct_method_name] expected a clean error, got success"),
    }

    // The same file with the actual function name still compiles.
    let mut graph = psy_common::Graph::new();
    graph.add_node(std::path::PathBuf::from("/virtual/src/main.psy"));
    let result = super::interpret_virtual_files(
        None,
        vec!["main".to_string()],
        graph,
        vec![(std::path::PathBuf::from("/virtual/src/main.psy"), std::sync::Arc::from(source))],
    );
    if let Err(err) = result {
        panic!("[b22_function_method_name] expected success, got: {err:#}");
    }
}

/// Concrete recursion evaluates both branches symbolically, so even an
/// obvious base case cannot terminate it. It must return a compiler error
/// instead of overflowing the host stack (M3).
#[test]
fn b26_concrete_recursion_is_a_clean_error() {
    let source = "fn count(n: Felt) -> Felt { if n == 0 { 0 } else { count(n - 1) } }\nfn main() -> Felt { count(0) }";
    match run_main(source, "b26_concrete_recursion") {
        Ok(Some(message)) => assert!(message.contains("UnsupportedRecursion"), "unexpected error: {message}"),
        Ok(None) => panic!("[b26_concrete_recursion] expected recursion error, got success"),
        Err(message) => panic!("[b26_concrete_recursion] interpreter panicked: {message}"),
    }

    let mutual = "fn left(n: Felt) -> Felt { right(n) }\nfn right(n: Felt) -> Felt { left(n) }\nfn main() -> Felt { left(0) }";
    match run_main(mutual, "b26_mutual_concrete_recursion") {
        Ok(Some(message)) => assert!(message.contains("UnsupportedRecursion"), "unexpected error: {message}"),
        Ok(None) => panic!("[b26_mutual_concrete_recursion] expected recursion error, got success"),
        Err(message) => panic!("[b26_mutual_concrete_recursion] interpreter panicked: {message}"),
    }

    // Returning from a call must remove it from the active set: sequential
    // calls to the same non-recursive function remain valid.
    let sequential = "fn id(n: Felt) -> Felt { n }\nfn main() -> Felt { id(1) + id(2) }";
    match run_main(sequential, "b26_sequential_calls") {
        Ok(None) => {}
        Ok(Some(message)) => panic!("[b26_sequential_calls] unexpected error: {message}"),
        Err(message) => panic!("[b26_sequential_calls] interpreter panicked: {message}"),
    }
}

/// ABI/entry-point collection used to sort and deduplicate same-name
/// methods, silently dropping one overload (M6).
#[test]
fn b27_contract_method_overload_is_rejected() {
    let source = r#"
#[contract]
#[derive(Storage)]
pub struct C {}
impl C {
    #[contract_method]
    pub fn get(self: Self, x: Felt) -> Felt { x }
}
impl CRef {
    #[contract_method]
    pub fn get(self: Self, x: Felt, y: Felt) -> Felt { x + y }
}
fn main() {}
"#;
    let path = std::path::PathBuf::from("/virtual/src/main.psy");
    let mut graph = psy_common::Graph::new();
    graph.add_node(path.clone());
    let result = super::interpret_virtual_files(
        Some("C".to_string()),
        vec![],
        graph,
        vec![(path, std::sync::Arc::from(source))],
    );
    match result {
        Err(error) => assert!(format!("{error:#}").contains("overloaded contract method"), "unexpected error: {error:#}"),
        Ok(_) => panic!("[b27_contract_overload] expected a clean ambiguity error, got success"),
    }
}

#[test]
fn b15_hash_two_to_one_requires_two_hash_operands() {
    expect_accept(
        "b15_hash_two_to_one_valid",
        "fn main() { let value: Hash = hash_two_to_one([1, 2, 3, 4], [5, 6, 7, 8]); }",
    );
    expect_reject(
        "b15_hash_two_to_one_bad_left",
        "fn main() { let value = hash_two_to_one([1, 2, 3], [5, 6, 7, 8]); }",
        "TypeMismatch",
    );
    expect_reject(
        "b15_hash_two_to_one_bad_right",
        "fn main() { let value = hash_two_to_one([1, 2, 3, 4], [5, 6, 7, 8, 9]); }",
        "TypeMismatch",
    );
}

/// Interpret `main` and assert it returns a clean error whose message contains
/// `needle` — and crucially that it does NOT panic.
fn expect_runtime_error(label: &str, source: &str, needle: &str) {
    match run_main(source, label) {
        Ok(None) => panic!("[{label}] expected runtime error mentioning `{needle}`, got success"),
        Ok(Some(msg)) => assert!(
            msg.contains(needle),
            "[{label}] unexpected runtime error (wanted substring `{needle}`):\n{msg}"
        ),
        Err(msg) => panic!("[{label}] expected runtime error mentioning `{needle}`, got PANIC:\n{msg}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 3: member access on a non-struct type
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn b03_member_access_on_u32_no_panic() {
    // `1.foo` used as a value: previously `as_struct().unwrap()` panicked.
    expect_reject("b03_member_on_u32", "fn main() { let x = 1.foo; }", "UnresolvedMember");
}

#[test]
fn b03_member_call_on_u32_no_panic() {
    // `1.foo()` routes through find_member first; when it fails it must not
    // fall through to the struct unwrap.
    match compile("fn main() { let x = 1.foo(); }", "b03_member_call_on_u32") {
        Outcome::Accept(_) | Outcome::Reject(_) => {}
        Outcome::Panic(msg) => panic!("[b03_member_call_on_u32] panicked instead of clean error:\n{msg}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 4: calling a non-function value
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn b04_call_u32_value_no_panic() {
    // `1u32(2u32)`: callee is a U32 value; `Type::signature` used to
    // `unreachable!()`.
    expect_reject("b04_call_u32", "fn main() { let x = 1u32(2u32); }", "InvalidFunctionCall");
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 5: const div / rem by zero
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn b05_const_u32_div_by_zero_no_panic() {
    expect_reject("b05_const_u32_div0", "const X: u32 = 1u32 / 0u32;\nfn main() {}", "DivisionByZero");
}

#[test]
fn b05_const_u32_rem_by_zero_no_panic() {
    expect_reject("b05_const_u32_rem0", "const X: u32 = 1u32 % 0u32;\nfn main() {}", "DivisionByZero");
}

#[test]
fn b05_const_felt_div_by_zero_no_panic() {
    expect_reject("b05_const_felt_div0", "const X: Felt = 1 / 0;\nfn main() {}", "DivisionByZero");
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 6: const u32 arithmetic overflow
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn b06_const_u32_add_overflow_no_panic() {
    expect_reject(
        "b06_const_u32_add_ovf",
        "const X: u32 = 4294967295u32 + 1u32;\nfn main() {}",
        "ArithmeticOverflow",
    );
}

#[test]
fn b06_const_u32_mul_overflow_no_panic() {
    expect_reject(
        "b06_const_u32_mul_ovf",
        "const X: u32 = 4294967295u32 * 2u32;\nfn main() {}",
        "ArithmeticOverflow",
    );
}

#[test]
fn b06_const_u32_sub_underflow_no_panic() {
    expect_reject(
        "b06_const_u32_sub_unflow",
        "const X: u32 = 0u32 - 1u32;\nfn main() {}",
        "ArithmeticOverflow",
    );
}

#[test]
fn b06_const_felt_add_does_not_overflow() {
    // Felt is modular over the Goldilocks prime — large felt sums must NOT be
    // flagged as overflow (only u32 wrapping is rejected).
    expect_accept("b06_felt_big_add", "const X: Felt = 4294967295 + 1;\nfn main() {}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 7: runtime div / rem by zero
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn b07_runtime_u32_div_by_zero_no_panic() {
    expect_runtime_error(
        "b07_rt_u32_div0",
        "fn main() { let x = 1u32 / 0u32; }",
        "DivisionByZero",
    );
}

#[test]
fn b07_runtime_u32_rem_by_zero_no_panic() {
    expect_runtime_error(
        "b07_rt_u32_rem0",
        "fn main() { let x = 1u32 % 0u32; }",
        "DivisionByZero",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 8: runtime array index out of bounds
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn b08_runtime_array_oob_no_panic() {
    expect_runtime_error(
        "b08_rt_array_oob",
        "fn main() { let a = [1, 2, 3]; let x = a[5]; }",
        "IndexOutOfBounds",
    );
}

#[test]
fn b08_runtime_array_in_bounds_ok() {
    // Sanity: a valid in-bounds access must still succeed at runtime.
    match run_main("fn main() -> Felt { let a = [1, 2, 3]; return a[1]; }", "b08_rt_array_ok") {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[b08_rt_array_ok] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[b08_rt_array_ok] expected success, got PANIC:\n{msg}"),
    }
}

#[test]
fn b08_runtime_array_write_oob_no_panic() {
    expect_runtime_error(
        "b08_rt_array_write_oob",
        "fn main() { let mut a = [1, 2, 3]; a[5] = 7; }",
        "IndexOutOfBounds",
    );
}

#[test]
fn b08_runtime_array_write_at_last_index_ok() {
    match run_main(
        "fn main() -> Felt { let mut a = [1, 2, 3]; a[2] = 7; return a[2]; }",
        "b08_rt_array_write_last_ok",
    ) {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[b08_rt_array_write_last_ok] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[b08_rt_array_write_last_ok] expected success, got PANIC:\n{msg}"),
    }
}

#[test]
fn b08_nested_array_write_oob_no_panic() {
    expect_runtime_error(
        "b08_nested_array_write_oob",
        "fn main() { let mut a = [[1, 2], [3, 4]]; a[1][2] = 7; }",
        "IndexOutOfBounds",
    );
}

#[test]
fn b08b_huge_array_repeat_returns_error_without_allocating() {
    expect_runtime_error(
        "b08b_huge_array_repeat",
        "fn main() { let a = [0; 4294967296]; }",
        "ArrayTooLarge",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 9: immutable assignment checked in the typecheck path
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn b09_immutable_assign_rejected_at_typecheck() {
    // `let x=1; x=2;` must be rejected by sema (not only at runtime).
    expect_reject("b09_imm_assign", "fn main() { let x = 1; x = 2; }", "ImmutableVariable");
}

#[test]
fn b09_mutable_assign_accepted() {
    // `let mut x=1; x=2;` must still typecheck.
    expect_accept("b09_mut_assign", "fn main() { let mut x = 1; x = 2; }");
}

#[test]
fn b09_storage_ref_field_assign_immutable_local_ok() {
    // Assigning to a field of a storage-ref handle (`CRef`) is a storage write
    // routed through `eq_assign`, NOT a mutation of the local binding. The
    // local need not be `mut`, and this must still typecheck.
    expect_accept(
        "b09_storage_ref_field",
        "#[contract]\n#[derive(Storage)]\npub struct C { pub v: Felt }\nfn main() { let c = CRef::new(ContractMetadata::current()); c.v = 1; }",
    );
}

#[test]
fn felt_for_loop_executes_without_u32_conversion() {
    match run_main("fn main() { for i in 0..3 { let x = i; } }", "felt_for_loop") {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[felt_for_loop] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[felt_for_loop] expected success, got PANIC:\n{msg}"),
    }
}

#[test]
fn descending_u32_for_range_is_empty() {
    match run_main("fn main() { for i in 5u32..3u32 { let x = i; } }", "descending_u32_for_range") {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[descending_u32_for_range] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[descending_u32_for_range] expected success, got PANIC:\n{msg}"),
    }
}
