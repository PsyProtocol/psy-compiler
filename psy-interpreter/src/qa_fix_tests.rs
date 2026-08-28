// Regression tests for QA-found bugs 1, 2, 10.
//
// Bug 1:  if-without-else used as a value silently returned the then-branch
//         unconditionally. Now rejected at sema with `IfWithoutElse`.
// Bug 2:  chained `mut self` methods miscompiled because the receiver was
//         evaluated twice (once to resolve the callee, once for the arg list),
//         double-applying every mutation. Fixed in `interpret_member_access`.
// Bug 10: ambiguous trait method (two traits providing the same method name)
//         silently picked the first impl. Now rejected with
//         `AmbiguousTraitMethod`.
//
// The shared `STD_PRIMITIVE_SCOPE_ID` singleton is reset after every case so
// the suite is hermetic. Every case is `#[serial]`.

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
    Accept,
    Reject(String),
    Panic(String),
}

fn compile(source: &str, label: &str) -> Outcome {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_qa_{label}_{unique}_{n}.psy"));
    fs::write(&path, source).unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interpreter.typecheck_single(path.clone())
    }));

    let _ = fs::remove_file(&path);
    #[allow(static_mut_refs)]
    unsafe {
        let _ = STD_PRIMITIVE_SCOPE_ID.take();
    }

    match result {
        Ok(Ok((_typechecker, _ctx))) => Outcome::Accept,
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

fn expect_accept(label: &str, source: &str) {
    match compile(source, label) {
        Outcome::Accept => {}
        Outcome::Reject(msg) => panic!("[{label}] expected accept, got reject:\n{msg}"),
        Outcome::Panic(msg) => panic!("[{label}] expected accept, got PANIC:\n{msg}"),
    }
}

fn expect_reject(label: &str, source: &str, needle: &str) {
    match compile(source, label) {
        Outcome::Accept => panic!("[{label}] expected reject mentioning `{needle}`, got accept"),
        Outcome::Reject(msg) => assert!(
            msg.contains(needle),
            "[{label}] unexpected error (wanted substring `{needle}`):\n{msg}"
        ),
        Outcome::Panic(msg) => panic!("[{label}] expected reject mentioning `{needle}`, got PANIC:\n{msg}"),
    }
}

/// Typecheck `source`, run `main` (no params) through the symbolic
/// interpreter, and return the folded constant value of each output felt.
fn run_main_outputs(source: &str, label: &str) -> Vec<u64> {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_qa_{label}_{unique}_{n}.psy"));
    fs::write(&path, source).unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let (mut typechecker, mut ctx) = interpreter
        .typecheck_single(path.clone())
        .unwrap_or_else(|e| panic!("[{label}] typecheck failed: {e:#}"));

    let _ = fs::remove_file(&path);

    // Resolve `main` in the root module scope.
    let scope_id = ctx.symbols[ModuleId::root()].scope_id;
    let main_name = ctx.intern("main");
    let main_type_id = ctx.symbols[scope_id]
        .types
        .get::<TypeKey>(&main_name.into())
        .cloned()
        .unwrap_or_else(|| panic!("[{label}] could not resolve `main`"));

    let (_name, _id, felts) = interpreter
        .__interpret__(&typechecker.program, main_type_id, vec![], &mut ctx)
        .unwrap_or_else(|e| panic!("[{label}] interpret failed: {e:#}"));

    #[allow(static_mut_refs)]
    unsafe {
        let _ = STD_PRIMITIVE_SCOPE_ID.take();
    }

    felts.iter().map(|f| f.get_constant_value()).collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 1: if-without-else as a value
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[serial]
fn b01_if_without_else_in_let_rejected() {
    expect_reject(
        "b01_if_without_else_in_let",
        "fn main() {\n    let x: Felt = if false { 7 };\n}",
        "IfWithoutElse",
    );
}

#[test]
#[serial]
fn b02_if_without_else_as_return_rejected() {
    // The if is the trailing expression of the function body block (value
    // position), so it must have an else branch.
    expect_reject(
        "b02_if_without_else_as_return",
        "fn main() -> Felt {\n    if false { 7 }\n}",
        "IfWithoutElse",
    );
}

#[test]
#[serial]
fn b03_if_without_else_as_call_arg_rejected() {
    expect_reject(
        "b03_if_without_else_as_arg",
        "fn id(x: Felt) -> Felt { return x; }\nfn main() {\n    id(if false { 7 });\n}",
        "IfWithoutElse",
    );
}

#[test]
#[serial]
fn b04_if_without_else_as_statement_accepted() {
    // As a statement (ExpressionStmt) an else branch is not required.
    expect_accept("b04_if_stmt_no_else", "fn main() {\n    if false {\n        let x = 1;\n    }\n}");
}

#[test]
#[serial]
fn b05_if_without_else_void_body_in_for_accepted() {
    // Mirrors `psy-std/storage.psy:689`: an if with a void body used as a
    // for-body block's trailing expression is side-effect only and must
    // typecheck even without an else branch.
    expect_accept(
        "b05_if_void_in_for",
        "fn main() {\n    for i in 0u32..3u32 {\n        if i == 0u32 {\n            let x = 1u32;\n        }\n    }\n}",
    );
}
#[test]
#[serial]
fn b06_if_with_else_as_value_accepted() {
    expect_accept("b06_if_with_else_value", "fn main() {\n    let x: Felt = if false { 7 } else { 9 };\n}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 2: chained mut-self method miscompile
// ═══════════════════════════════════════════════════════════════════════════

const CHAIN_SRC: &str = r#"
struct S {
    pub x: u32,
}

impl S {
    pub fn inc(mut self: Self) -> Self {
        self.x = self.x + 1u32;
        return self;
    }
    pub fn get(self: Self) -> u32 {
        return self.x;
    }
}

fn main() -> u32 {
    let s = S { x: 1u32 };
    return s.inc().inc().get();
}
"#;

#[test]
#[serial]
fn b07_chained_mut_self_returns_three() {
    // Before the fix the receiver of each link was evaluated twice (once for
    // callee resolution, once for the arg list), so every `inc` mutation was
    // applied twice per link. `s.inc().inc().get()` returned 7 instead of 3.
    let outs = run_main_outputs(CHAIN_SRC, "b07_chained_mut_self");
    assert_eq!(outs, vec![3], "chained mut-self method miscompiled");
}

#[test]
#[serial]
fn b08_single_mut_self_returns_two() {
    let src = r#"
struct S {
    pub x: u32,
}
impl S {
    pub fn inc(mut self: Self) -> Self {
        self.x = self.x + 1u32;
        return self;
    }
    pub fn get(self: Self) -> u32 {
        return self.x;
    }
}
fn main() -> u32 {
    let s = S { x: 1u32 };
    return s.inc().get();
}
"#;
    let outs = run_main_outputs(src, "b08_single_mut_self");
    assert_eq!(outs, vec![2]);
}

#[test]
#[serial]
fn b09_stepwise_mut_self_returns_three() {
    // Stepwise already worked; defends against regressing the non-chained path.
    let src = r#"
struct S {
    pub x: u32,
}
impl S {
    pub fn inc(mut self: Self) -> Self {
        self.x = self.x + 1u32;
        return self;
    }
    pub fn get(self: Self) -> u32 {
        return self.x;
    }
}
fn main() -> u32 {
    let s = S { x: 1u32 };
    let s2 = s.inc();
    let s3 = s2.inc();
    return s3.get();
}
"#;
    let outs = run_main_outputs(src, "b09_stepwise_mut_self");
    assert_eq!(outs, vec![3]);
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 10: ambiguous trait method silent pick
// ═══════════════════════════════════════════════════════════════════════════

const AMBIG_SRC: &str = r#"
trait A {
    pub fn val(self: Self) -> Felt;
}
trait B {
    pub fn val(self: Self) -> Felt;
}
struct S {
    pub x: Felt,
}
impl A for S {
    pub fn val(self: Self) -> Felt {
        return self.x;
    }
}
impl B for S {
    pub fn val(self: Self) -> Felt {
        return self.x;
    }
}
fn main() {
    let s = S { x: 1 };
    let v = s.val();
}
"#;

#[test]
#[serial]
fn b10_ambiguous_trait_method_rejected() {
    match compile(AMBIG_SRC, "b10_ambiguous_trait_method") {
        Outcome::Reject(message) => {
            assert!(message.contains("AmbiguousTraitMethod"), "unexpected error:\n{message}");
            assert!(message.contains("trait A") && message.contains("trait B"), "missing conflicting trait names:\n{message}");
        }
        Outcome::Accept => panic!("[b10_ambiguous_trait_method] expected rejection, got accept"),
        Outcome::Panic(message) => panic!("[b10_ambiguous_trait_method] expected rejection, got PANIC:\n{message}"),
    }
}

#[test]
#[serial]
fn b10b_exact_trait_impl_wins_over_earlier_generic_impl() {
    expect_accept(
        "b10b_exact_over_generic",
        r#"
trait GenericProvider { fn value(self: Self) -> bool; }
trait ExactProvider { fn value(self: Self) -> Felt; }
struct Box<T> { value: T }
impl<T> GenericProvider for Box<T> { fn value(self: Self) -> bool { true } }
impl ExactProvider for Box<Felt> { fn value(self: Self) -> Felt { 1 } }
fn main() { let value: Felt = Box::<Felt> { value: 1 }.value(); }
"#,
    );
}

#[test]
#[serial]
fn b10c_two_generic_trait_methods_are_ambiguous() {
    expect_reject(
        "b10c_generic_ambiguity",
        r#"
trait A { fn value(self: Self) -> Felt; }
trait B { fn value(self: Self) -> Felt; }
struct Box<T> { value: T }
impl<T> A for Box<T> { fn value(self: Self) -> Felt { 1 } }
impl<T> B for Box<T> { fn value(self: Self) -> Felt { 2 } }
fn main() { let value = Box::<Felt> { value: 1 }.value(); }
"#,
        "AmbiguousTraitMethod",
    );
}

#[test]
#[serial]
fn b11_single_trait_method_accepted() {
    // A single trait providing `val` must still resolve cleanly.
    let src = r#"
trait A {
    pub fn val(self: Self) -> Felt;
}
struct S {
    pub x: Felt,
}
impl A for S {
    pub fn val(self: Self) -> Felt {
        return self.x;
    }
}
fn main() {
    let s = S { x: 1 };
    let v = s.val();
}
"#;
    expect_accept("b11_single_trait_method", src);
}

#[test]
#[serial]
fn b12_inherent_plus_trait_no_ambiguity() {
    // An inherent impl method shadows trait methods; no ambiguity expected.
    let src = r#"
trait A {
    pub fn val(self: Self) -> Felt;
}
trait B {
    pub fn val(self: Self) -> Felt;
}
struct S {
    pub x: Felt,
}
impl S {
    pub fn val(self: Self) -> Felt {
        return self.x;
    }
}
impl A for S {
    pub fn val(self: Self) -> Felt {
        return self.x;
    }
}
impl B for S {
    pub fn val(self: Self) -> Felt {
        return self.x;
    }
}
fn main() {
    let s = S { x: 1 };
    let v = s.val();
}
"#;
    expect_accept("b12_inherent_shadows_traits", src);
}

// Suppress unused-import warning for `NodeType` (kept for potential future
// ancestor-position assertions).
const _: fn() = || {
    let _ = NodeType::IfExpr;
};
