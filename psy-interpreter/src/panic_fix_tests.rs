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
// Every case asserts a clean accept/reject and that no path panics. The shared
// `STD_PRIMITIVE_SCOPE_ID` singleton is reset after every case so the suite is
// hermetic. Every case is `#[serial]`.

use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};
use serial_test::serial;

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
    #[allow(static_mut_refs)]
    unsafe {
        let _ = STD_PRIMITIVE_SCOPE_ID.take();
    }

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
        c.interpreter.interpret_function(program, main_tid, vec![], &mut c.ctx)
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

#[test]
#[serial]
fn b12_non_bool_not_is_rejected_at_typecheck() {
    expect_reject("b12_felt_not", "fn main() { let value = !1; }", "TypeMismatch");
    expect_reject("b12_u32_not", "fn main() { let value = !1u32; }", "TypeMismatch");
}

#[test]
#[serial]
fn b13_mixed_for_range_endpoint_types_are_rejected() {
    expect_reject(
        "b13_mixed_for_range",
        "fn main() { for i in 0u32..3 { } }",
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
#[serial]
fn b03_member_access_on_u32_no_panic() {
    // `1.foo` used as a value: previously `as_struct().unwrap()` panicked.
    expect_reject("b03_member_on_u32", "fn main() { let x = 1.foo; }", "UnresolvedMember");
}

#[test]
#[serial]
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
#[serial]
fn b04_call_u32_value_no_panic() {
    // `1u32(2u32)`: callee is a U32 value; `Type::signature` used to
    // `unreachable!()`.
    expect_reject("b04_call_u32", "fn main() { let x = 1u32(2u32); }", "InvalidFunctionCall");
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 5: const div / rem by zero
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[serial]
fn b05_const_u32_div_by_zero_no_panic() {
    expect_reject("b05_const_u32_div0", "const X: u32 = 1u32 / 0u32;\nfn main() {}", "DivisionByZero");
}

#[test]
#[serial]
fn b05_const_u32_rem_by_zero_no_panic() {
    expect_reject("b05_const_u32_rem0", "const X: u32 = 1u32 % 0u32;\nfn main() {}", "DivisionByZero");
}

#[test]
#[serial]
fn b05_const_felt_div_by_zero_no_panic() {
    expect_reject("b05_const_felt_div0", "const X: Felt = 1 / 0;\nfn main() {}", "DivisionByZero");
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 6: const u32 arithmetic overflow
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[serial]
fn b06_const_u32_add_overflow_no_panic() {
    expect_reject(
        "b06_const_u32_add_ovf",
        "const X: u32 = 4294967295u32 + 1u32;\nfn main() {}",
        "ArithmeticOverflow",
    );
}

#[test]
#[serial]
fn b06_const_u32_mul_overflow_no_panic() {
    expect_reject(
        "b06_const_u32_mul_ovf",
        "const X: u32 = 4294967295u32 * 2u32;\nfn main() {}",
        "ArithmeticOverflow",
    );
}

#[test]
#[serial]
fn b06_const_u32_sub_underflow_no_panic() {
    expect_reject(
        "b06_const_u32_sub_unflow",
        "const X: u32 = 0u32 - 1u32;\nfn main() {}",
        "ArithmeticOverflow",
    );
}

#[test]
#[serial]
fn b06_const_felt_add_does_not_overflow() {
    // Felt is modular over the Goldilocks prime — large felt sums must NOT be
    // flagged as overflow (only u32 wrapping is rejected).
    expect_accept("b06_felt_big_add", "const X: Felt = 4294967295 + 1;\nfn main() {}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 7: runtime div / rem by zero
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[serial]
fn b07_runtime_u32_div_by_zero_no_panic() {
    expect_runtime_error(
        "b07_rt_u32_div0",
        "fn main() { let x = 1u32 / 0u32; }",
        "DivisionByZero",
    );
}

#[test]
#[serial]
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
#[serial]
fn b08_runtime_array_oob_no_panic() {
    expect_runtime_error(
        "b08_rt_array_oob",
        "fn main() { let a = [1, 2, 3]; let x = a[5]; }",
        "IndexOutOfBounds",
    );
}

#[test]
#[serial]
fn b08_runtime_array_in_bounds_ok() {
    // Sanity: a valid in-bounds access must still succeed at runtime.
    match run_main("fn main() -> Felt { let a = [1, 2, 3]; return a[1]; }", "b08_rt_array_ok") {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[b08_rt_array_ok] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[b08_rt_array_ok] expected success, got PANIC:\n{msg}"),
    }
}

#[test]
#[serial]
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
#[serial]
fn b09_immutable_assign_rejected_at_typecheck() {
    // `let x=1; x=2;` must be rejected by sema (not only at runtime).
    expect_reject("b09_imm_assign", "fn main() { let x = 1; x = 2; }", "ImmutableVariable");
}

#[test]
#[serial]
fn b09_mutable_assign_accepted() {
    // `let mut x=1; x=2;` must still typecheck.
    expect_accept("b09_mut_assign", "fn main() { let mut x = 1; x = 2; }");
}

#[test]
#[serial]
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
#[serial]
fn felt_for_loop_executes_without_u32_conversion() {
    match run_main("fn main() { for i in 0..3 { let x = i; } }", "felt_for_loop") {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[felt_for_loop] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[felt_for_loop] expected success, got PANIC:\n{msg}"),
    }
}

#[test]
#[serial]
fn descending_u32_for_range_is_empty() {
    match run_main("fn main() { for i in 5u32..3u32 { let x = i; } }", "descending_u32_for_range") {
        Ok(None) => {}
        Ok(Some(msg)) => panic!("[descending_u32_for_range] expected success, got error:\n{msg}"),
        Err(msg) => panic!("[descending_u32_for_range] expected success, got PANIC:\n{msg}"),
    }
}
