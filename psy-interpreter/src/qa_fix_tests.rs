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
// Each case owns an independent symbol table and uniquely named temporary file.

use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};

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

    felts.iter().map(|f| f.get_constant_value()).collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug 1: if-without-else as a value
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn b01_if_without_else_in_let_rejected() {
    expect_reject(
        "b01_if_without_else_in_let",
        "fn main() {\n    let x: Felt = if false { 7 };\n}",
        "IfWithoutElse",
    );
}

#[test]
fn b01b_bool_match_single_literal_is_incomplete() {
    expect_reject(
        "b01b_bool_match_single_literal",
        "fn main() -> Felt { match true { true => 1 } }",
        "IncompleteMatch",
    );
}

#[test]
fn b01c_bool_match_two_equal_literals_is_incomplete() {
    expect_reject(
        "b01c_bool_match_duplicate_literal",
        "fn main() -> Felt { match true { true => 1, true => 2 } }",
        "IncompleteMatch",
    );
}

#[test]
fn b01d_bool_match_false_and_wildcard_is_complete() {
    expect_accept(
        "b01d_bool_match_false_wildcard",
        "fn main() -> Felt { match true { false => 1, _ => 2 } }",
    );
}

#[test]
fn b01e_bool_match_wildcard_only_is_complete() {
    expect_accept(
        "b01e_bool_match_wildcard_only",
        "fn main() -> Felt { match true { _ => 2 } }",
    );
}

#[test]
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
fn b03_if_without_else_as_call_arg_rejected() {
    expect_reject(
        "b03_if_without_else_as_arg",
        "fn id(x: Felt) -> Felt { return x; }\nfn main() {\n    id(if false { 7 });\n}",
        "IfWithoutElse",
    );
}

#[test]
fn b04_if_without_else_as_statement_accepted() {
    // As a statement (ExpressionStmt) an else branch is not required.
    expect_accept("b04_if_stmt_no_else", "fn main() {\n    if false {\n        let x = 1;\n    }\n}");
}

#[test]
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
fn b07_chained_mut_self_returns_three() {
    // Before the fix the receiver of each link was evaluated twice (once for
    // callee resolution, once for the arg list), so every `inc` mutation was
    // applied twice per link. `s.inc().inc().get()` returned 7 instead of 3.
    let outs = run_main_outputs(CHAIN_SRC, "b07_chained_mut_self");
    assert_eq!(outs, vec![3], "chained mut-self method miscompiled");
}

#[test]
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

/// Two impls of the same *generic* trait for one type are distinct
/// providers: `<W as Conv<..>>::conv` must select the requested impl,
/// not silently collapse to whichever was declared first.
#[test]
fn b10d_same_generic_trait_two_impls_dispatch_by_trait_args() {
    // Both disambiguated calls select their own impl and run.
    let outputs = run_main_outputs(
        r#"
trait Conv<R> { fn conv(self: Self) -> R; }
pub struct W { pub v: Felt }
impl Conv<u32> for W { fn conv(self: Self) -> u32 { 3u32 } }
impl Conv<Felt> for W { fn conv(self: Self) -> Felt { self.v } }
fn main() -> (Felt, u32) {
    let w = W { v: 7 };
    return (<W as Conv<Felt>>::conv(w), <W as Conv<u32>>::conv(w));
}
"#,
        "b10d_both_impls_dispatch",
    );
    assert_eq!(outputs, vec![7, 3], "felt impl then u32 impl");

    // Asking for the Felt impl must not run the u32 impl's body: the
    // old collapse picked the first-declared impl regardless of trait
    // args, so the u32 body fed a Felt slot and failed to typecheck.
    expect_accept(
        "b10d_conv_felt_only",
        r#"
trait Conv<R> { fn conv(self: Self) -> R; }
pub struct W { pub v: Felt }
impl Conv<u32> for W { fn conv(self: Self) -> u32 { 3u32 } }
impl Conv<Felt> for W { fn conv(self: Self) -> Felt { self.v } }
fn main() {
    let w = W { v: 7 };
    let a: Felt = <W as Conv<Felt>>::conv(w);
    assert_eq(a, 7, "felt impl body");
}
"#,
    );
}

/// The associated-type half of the same bug: two impls of one generic
/// trait for one type must expose *their own* associated types, not
/// whichever impl registered first (implementer.rs provider dedup
/// used to collapse these too).
#[test]
fn b10e_same_generic_trait_assoc_types_dispatch() {
    let outputs = run_main_outputs(
        r#"
trait Out<R> { type T; fn get(self: Self) -> Self::T; }
pub struct W { pub v: Felt }
impl Out<u32> for W { type T = u32; fn get(self: Self) -> u32 { 3u32 } }
impl Out<Felt> for W { type T = Felt; fn get(self: Self) -> Felt { self.v } }
fn main() -> (Felt, u32) {
    let w = W { v: 7 };
    return (<W as Out<Felt>>::get(w), <W as Out<u32>>::get(w));
}
"#,
        "b10e_assoc_types_dispatch",
    );
    assert_eq!(outputs, vec![7, 3], "each impl's assoc type and body");
}

/// Three impls of the same generic trait: every instantiation must pick
/// its own (no first-wins collapse at any arity).
#[test]
fn b10f_three_impls_of_same_generic_trait() {
    let outputs = run_main_outputs(
        r#"
trait Pick<R> { fn pick(self: Self) -> R; }
pub struct W { pub v: Felt }
impl Pick<u32> for W { fn pick(self: Self) -> u32 { 1u32 } }
impl Pick<Felt> for W { fn pick(self: Self) -> Felt { 2 } }
impl Pick<bool> for W { fn pick(self: Self) -> bool { true } }
fn main() -> (u32, Felt) {
    let w = W { v: 0 };
    let a: u32 = <W as Pick<u32>>::pick(w);
    let b: Felt = <W as Pick<Felt>>::pick(w);
    let c: Felt = if <W as Pick<bool>>::pick(w) { 100 } else { 200 };
    return (a, b + c);
}
"#,
        "b10f_three_impls",
    );
    assert_eq!(outputs, vec![1, 102], "u32=1, felt=2, bool branch adds 100");
}

/// The undecorated method call with two same-trait impls in scope is
/// genuinely ambiguous and must be reported, not silently resolved.
#[test]
fn b10g_undecorated_call_with_same_trait_impls_is_ambiguous() {
    expect_reject(
        "b10g_undecorated_ambiguous",
        r#"
trait Conv<R> { fn conv(self: Self) -> R; }
pub struct W { pub v: Felt }
impl Conv<u32> for W { fn conv(self: Self) -> u32 { 3u32 } }
impl Conv<Felt> for W { fn conv(self: Self) -> Felt { self.v } }
fn main() {
    let w = W { v: 7 };
    let a = w.conv();
}
"#,
        "Ambiguous",
    );
}

#[test]
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
