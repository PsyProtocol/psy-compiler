// Source-driven coverage for the scattered sema error arms in
// psy-sema/src/lib.rs (visitor type-mismatch / arity / shape rejections).
// Each entry is a tiny psy program whose typechecking lands in one specific
// `return Err(...)` arm; the reject needles are kept loose (case-insensitive
// substring) so the assertions survive Display rewording but still require
// the program to be rejected for a *type* reason, not a parse error.

use std::sync::atomic::{AtomicU64, Ordering};

use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};
use serial_test::serial;

use super::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn compile(source: &str) -> Result<(), String> {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_se_{n}.psy"));
    std::fs::write(&path, source).unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = interpreter.typecheck_single(path.clone());

    let _ = std::fs::remove_file(&path);
    #[allow(static_mut_refs)]
    unsafe {
        let _ = STD_PRIMITIVE_SCOPE_ID.take();
    }
    result.map(|_| ()).map_err(|e| format!("{e:#}"))
}

fn rejects(label: &str, source: &str, needle: &str) {
    match compile(source) {
        Ok(()) => panic!("[{label}] expected rejection containing `{needle}`, got success"),
        Err(message) => assert!(
            message.to_lowercase().contains(&needle.to_lowercase()),
            "[{label}] expected rejection containing `{needle}`, got:\n{message}"
        ),
    }
}

fn accepts(label: &str, source: &str) {
    if let Err(message) = compile(source) {
        panic!("[{label}] expected success, got:\n{message}");
    }
}

/// Run every case even when an earlier one fails, then report all failures
/// at once — a panicking per-case assert would silently skip the rest.
fn rejects_all(cases: &[(String, String, &str)]) {
    let mut failures = Vec::new();
    for (label, source, needle) in cases {
        match compile(source) {
            Ok(()) => failures.push(format!("[{label}] expected rejection containing `{needle}`, got success")),
            Err(message) => {
                if !message.to_lowercase().contains(&needle.to_lowercase()) {
                    failures.push(format!("[{label}] expected rejection containing `{needle}`, got:\n{message}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{} case(s) failed:\n{}", failures.len(), failures.join("\n---\n"));
}

const PRELUDE: &str = "use std::prelude::*;\n";

const STRUCT_P: &str = r#"
pub struct P { pub x: Felt }

impl P {
    pub fn new(v: Felt) -> P {
        return P { x: v };
    }
}
"#;

#[test]
#[serial]
fn binary_and_unary_operator_type_guards_reject() {
    let cases: &[(&str, &str, &str)] = &[
        ("add on bool", "fn main() -> Felt { let a: bool = true; let b: Felt = a + 1; return b; }", "mismatch"),
        ("xor on tuple", "fn main() -> Felt { let t = (1, 2); let u = t ^ t; return 0; }", "mismatch"),
        ("logical and on felt", "fn main() -> Felt { let c: bool = 1 && 2; return 0; }", "mismatch"),
        ("compare bools", "fn main() -> Felt { let d: bool = true < false; return 0; }", "mismatch"),
        ("negate bool", "fn main() -> Felt { let e: Felt = -true; return e; }", "mismatch"),
        ("not on felt", "fn main() -> Felt { let f: bool = !1; return f; }", "mismatch"),
        ("array literal mixed types", "fn main() -> Felt { let a = [1, true]; return a[0]; }", "mismatch"),
    ];
    for (label, body, needle) in cases {
        rejects(label, &format!("{PRELUDE}{body}"), needle);
    }
}

#[test]
#[serial]
fn call_arity_and_generic_argument_guards_reject() {
    let source = format!(
        "{PRELUDE}{STRUCT_P}
pub fn felt_id<T: Felt>(v: T) -> T {{
    return v;
}}

pub fn two(a: Felt, b: Felt) -> Felt {{
    return a + b;
}}
"
    );
    rejects_all(&[
        (
            "plain call wrong arity".to_string(),
            format!("{source}fn main() -> Felt {{ return two(1); }}"),
            "parameters",
        ),
        (
            "explicit generic arg violates constraint".to_string(),
            format!("{source}fn main() -> Felt {{ return felt_id<bool>(1); }}"),
            "mismatch",
        ),
    ]);
}

#[test]
#[serial]
fn member_call_shapes_resolve() {
    // Happy-path method call: resolves through find_member into the
    // visit_member_call validation. (Wrong-shape methods are filtered out
    // during lookup and never reach the visitor's own validation arms.)
    let source = format!(
        "{PRELUDE}{STRUCT_P}
impl P {{
    pub fn one(self: P, v: Felt) -> Felt {{
        return v;
    }}
}}

fn main() -> Felt {{
    let a = P::new(1);
    let x: Felt = a.one(1);
    return x;
}}"
    );
    accepts("method call with inferred arguments", &source);
}

#[test]
#[serial]
fn custom_index_and_eq_shapes_reject() {
    // find_member filters method candidates by expected signature before the
    // visitor's own validation runs, so several wrong-shape methods surface
    // as UnresolvedMember; the reachable eq-shape arms are asserted here.
    let eq_ret = format!(
        "{PRELUDE}{STRUCT_P}
impl P {{
    pub fn eq(self: P, o: P) -> Felt {{
        return 0;
    }}
}}
"
    );
    rejects_all(&[
        (
            "eq returning non-bool via ==".to_string(),
            format!("{eq_ret}fn main() -> Felt {{ let a = P::new(1); let b = P::new(2); let same: bool = a == b; return 0; }}"),
            "mismatch",
        ),
        (
            "eq returning non-bool via assert_eq".to_string(),
            format!("{eq_ret}fn main() -> Felt {{ let a = P::new(1); let b = P::new(2); assert_eq(a, b); return 0; }}"),
            "mismatch",
        ),
    ]);
}

#[test]
#[serial]
fn tuple_and_if_expression_guards_reject() {
    let cases: &[(&str, &str, &str)] = &[
        ("tuple access out of bounds", "fn main() -> Felt { let t = (1, 2); return t.2; }", "index"),
        ("if predicate not bool", "fn main() -> Felt { let x: Felt = if 1 { 2 } else { 3 }; return x; }", "mismatch"),
        (
            "else-if predicate not bool",
            "fn main() -> Felt { let x: Felt = if true { 1 } else if 2 { 2 } else { 3 }; return x; }",
            "mismatch",
        ),
        (
            "else-if branch type mismatch",
            "fn main() -> Felt { let x: Felt = if true { 1 } else if true { true } else { 2 }; return x; }",
            "mismatch",
        ),
        ("else branch type mismatch", "fn main() -> Felt { let x: Felt = if true { 1 } else { true }; return x; }", "mismatch"),
        ("struct literal missing fields", "fn main() -> Felt { let p: Pair = Pair { a: 1 }; return 0; }", "fields"),
        ("struct literal field type mismatch", "fn main() -> Felt { let p: Pair = Pair { a: 1, b: true }; return 0; }", "mismatch"),
    ];
    let prelude = format!(
        "{PRELUDE}pub struct Pair {{ a: Felt, b: Felt }}\n"
    );
    for (label, body, needle) in cases {
        rejects(label, &format!("{prelude}{body}"), needle);
    }
}

#[test]
#[serial]
fn return_placement_guards_reject() {
    rejects(
        "statement after quick return",
        format!("{PRELUDE}fn q() -> Felt {{ return 1; let z: Felt = 2; return z; }} fn main() -> Felt {{ return q(); }}").as_str(),
        "return",
    );
    rejects(
        "return inside nested if block",
        format!("{PRELUDE}fn n() {{ if true {{ return; }} return; }} fn main() {{ n(); }}").as_str(),
        "return",
    );
}

#[test]
#[serial]
fn match_expression_guards_reject() {
    let cases: &[(&str, &str, &str)] = &[
        ("non-primitive scrutinee", "let t = (1, 2); let x: Felt = match t { _ => 1 };", "mismatch"),
        ("pattern type mismatch", "let x: Felt = match 1 { true => 1, _ => 2 };", "mismatch"),
        ("duplicate wildcard", "let x: Felt = match true { _ => 1, _ => 2 };", "wildcard"),
        ("arm body type mismatch", "let x: Felt = match true { true => 1, false => true };", "mismatch"),
        ("incomplete boolean match", "let x: Felt = match true { true => 1 };", "match"),
    ];
    for (label, body, needle) in cases {
        rejects(label, &format!("{PRELUDE}fn main() -> Felt {{ {body} return x; }}"), needle);
    }
}

#[test]
#[serial]
fn lambda_parameter_and_return_guards() {
    let path_param = format!(
        "{PRELUDE}{STRUCT_P}fn main() -> Felt {{
    let lam = |p: P| -> Felt {{ return p.x; }};
    let v: Felt = lam(P::new(3));
    return v;
}}"
    );
    accepts("path-typed lambda parameter", &path_param);

    // A return-type-less lambda is typed VOID; a value-producing body then
    // fails the unify against VOID (both the no-return-type lookup and the
    // mismatch arm run).
    rejects(
        "lambda body value vs inferred VOID",
        format!("{PRELUDE}fn main() -> Felt {{ let lam = |x: Felt| {{ return x + 1; }}; return 0; }}").as_str(),
        "mismatch",
    );

    rejects(
        "duplicate lambda parameter",
        format!("{PRELUDE}fn main() -> Felt {{ let lam = |a: Felt, a: Felt| -> Felt {{ return a; }}; return 0; }}").as_str(),
        "defined",
    );
    rejects(
        "lambda declared return mismatch",
        format!("{PRELUDE}fn main() -> Felt {{ let lam = |x: Felt| -> bool {{ return x + 1; }}; return 0; }}").as_str(),
        "mismatch",
    );
}

const GENERIC_BOX: &str = r#"
pub struct Box<T: Felt> {
    v: T,
}

impl Box<Felt> {
    pub fn zero() -> Box<Felt> {
        return Box { v: 0 };
    }
}
"#;

#[test]
#[serial]
fn impl_and_trait_header_guards_reject() {
    let bad_impl = format!(
        "{PRELUDE}pub struct Box<T: Felt> {{ v: T, }}\nimpl Box<bool> {{\n    pub fn z() -> Felt {{ return 0; }}\n}}\nfn main() -> Felt {{ return 0; }}"
    );
    rejects("impl generic argument violates constraint", &bad_impl, "mismatch");

    let bad_trait_arg = format!(
        "{PRELUDE}pub trait Tr<T: Felt> {{\n    pub fn t() -> Felt;\n}}\npub struct S {{}}\nimpl Tr<bool> for S {{\n    pub fn t() -> Felt {{ return 0; }}\n}}\nfn main() -> Felt {{ return 0; }}"
    );
    rejects("trait impl generic argument violates constraint", &bad_trait_arg, "mismatch");

    let non_trait = format!(
        "{PRELUDE}{STRUCT_P}pub struct S {{}}\nimpl P for S {{}}\nfn main() -> Felt {{ return 0; }}"
    );
    rejects("trait impl for a non-trait type", &non_trait, "mismatch");

    let missing_assoc = format!(
        "{PRELUDE}pub trait Tr {{\n    pub type Out: Felt;\n    pub fn t() -> Felt;\n}}\npub struct S {{}}\nimpl Tr for S {{\n    pub fn t() -> Felt {{ return 0; }}\n}}\nfn main() -> Felt {{ return 0; }}"
    );
    rejects("trait impl missing associated type", &missing_assoc, "associated");

    let bad_implementor = format!(
        "{PRELUDE}pub trait Tr<T: Felt> {{\n    pub fn t() -> Felt;\n}}\npub struct Box<T: Felt> {{ v: T, }}\nimpl Tr<Felt> for Box<bool> {{\n    pub fn t() -> Felt {{ return 0; }}\n}}\nfn main() -> Felt {{ return 0; }}"
    );
    rejects("trait impl implementor argument mismatch", &bad_implementor, "mismatch");
}

#[test]
#[serial]
fn generic_type_annotation_guards_reject() {
    rejects(
        "too few generic arguments in annotation",
        format!("{PRELUDE}pub struct P2<A: Felt, B: Felt> {{ a: A, b: B, }} fn main() -> Felt {{ let p: P2<Felt> = P2 {{ a: 1, b: 2 }}; return 0; }}").as_str(),
        "generic",
    );
    rejects(
        "generic argument violates constraint in annotation",
        format!("{PRELUDE}pub struct P2<A: Felt, B: Felt> {{ a: A, b: B, }} fn main() -> Felt {{ let p: P2<Felt, bool> = P2 {{ a: 1, b: 2 }}; return 0; }}").as_str(),
        "mismatch",
    );
}

#[test]
#[serial]
fn method_resolution_and_member_call_paths_accept() {
    // `self.get()` inside an impl resolves the callee through the
    // member-call arm of visit_member_access; `P::new` covers the
    // type-receiver (associated function) path of visit_member_call.
    let source = format!(
        "{PRELUDE}{STRUCT_P}
impl P {{
    pub fn get(self: P) -> Felt {{
        return self.x;
    }}

    pub fn run(self: P) -> Felt {{
        return self.get();
    }}
}}

fn main() -> Felt {{
    let a = P::new(1);
    return a.run();
}}"
    );
    accepts("self method call and associated constructor", &source);

    // A const Path forwarded as a split_bits length exercises the
    // compile-time-constant evaluation path at the call site.
    let const_length = format!(
        "{PRELUDE}const K: Felt = 4;
fn main() -> Felt {{
    let bits: [Felt; 4] = split_bits(13, K);
    return bits[0];
}}"
    );
    accepts("const-named split_bits length", &const_length);

    // The impl header itself is what gets checked here; the associated
    // function is only declared, not resolved through a generic instantiation.
    let specialized = format!("{PRELUDE}{GENERIC_BOX}fn main() -> Felt {{ return 0; }}");
    accepts("specialized impl header", &specialized);
}

#[test]
#[serial]
fn compound_assignment_guards() {
    // Compound assignment on a struct routes through `add_assign` member
    // lookup; a matching method typechecks...
    let with_method = format!(
        "{PRELUDE}{STRUCT_P}
impl P {{
    pub fn add_assign(self: P, o: P) -> P {{
        return o;
    }}
}}

fn main() -> Felt {{
    let mut a = P::new(1);
    a += P::new(2);
    return a.x;
}}"
    );
    accepts("compound assignment with add_assign method", &with_method);

    // ...and assert with a non-bool predicate is rejected.
    rejects(
        "assert with non-bool predicate",
        format!("{PRELUDE}fn main() {{ assert(1); }}").as_str(),
        "mismatch",
    );
}

#[test]
#[serial]
fn trait_cast_paths_cover_segments_constraints_and_rejections() {
    let traits = format!(
        "{PRELUDE}{STRUCT_P}
pub trait Val {{
    pub fn value() -> Felt;
}}

impl Val for P {{
    pub fn value() -> Felt {{
        return 3;
    }}
}}

pub trait WithTy {{
    pub type Ty;

    pub fn make() -> Ty;
}}

impl WithTy for P {{
    pub type Ty = P;

    pub fn make() -> P {{
        return P::new(4);
    }}
}}
"
    );

    // A trait-cast path with segments resolves the associated type first,
    // then walks members of the resulting type.
    accepts(
        "trait cast with associated-type segment",
        &format!(
            "{traits}fn main() -> Felt {{
    let v: P = <P as WithTy>::Ty::make();
    return v.x;
}}"
        ),
    );

    // Casting through a type variable consults its declared constraints.
    accepts(
        "trait cast on constrained generic",
        &format!(
            "{traits}pub fn go<T: Val>() -> Felt {{
    return <T as Val>::value();
}}

fn main() -> Felt {{
    return go::<P>();
}}"
        ),
    );

    rejects(
        "trait cast to an unimplemented trait",
        &format!(
            "{traits}pub trait Other {{
    pub fn value() -> Felt;
}}

fn main() -> Felt {{
    return <P as Other>::value();
}}"
        ),
        "mismatch",
    );

    rejects(
        "trait cast naming an unknown member",
        &format!(
            "{traits}fn main() -> Felt {{
    return <P as Val>::missing();
}}"
        ),
        "unresolved",
    );
}

#[test]
#[serial]
fn qualified_module_paths_and_roots_resolve() {
    // Nested module path: root=outer resolves by name, then the `inner`
    // segment walks std-module-style from parent to child.
    accepts(
        "nested module segments resolve",
        &format!(
            "{PRELUDE}pub mod outer {{
    pub mod inner {{
        pub fn f() -> Felt {{
            return 5;
        }}
    }}
}}

fn main() -> Felt {{
    return outer::inner::f();
}}"
        ),
    );

    // A non-module segment resolves as a type in the module, then the target
    // resolves as a member of that type.
    accepts(
        "module path through a type segment",
        &format!(
            "{PRELUDE}pub mod m {{
    pub struct T {{ pub field: Felt }}

    impl T {{
        pub fn make() -> T {{
            return T {{ field: 1 }};
        }}
    }}
}}

fn main() -> Felt {{
    let t: m::T = m::T::make();
    return t.field;
}}"
        ),
    );

    rejects(
        "path through a private nested module",
        &format!(
            "{PRELUDE}pub mod outer {{
    mod secret {{
        pub fn f() -> Felt {{
            return 1;
        }}
    }}
}}

fn main() -> Felt {{
    return outer::secret::f();
}}"
        ),
        "public",
    );

    accepts(
        "crate-rooted path",
        &format!(
            "{PRELUDE}pub fn helper() -> Felt {{
    return 7;
}}

fn main() -> Felt {{
    return crate::helper();
}}"
        ),
    );

    accepts(
        "super-rooted path from an inline module",
        &format!(
            "{PRELUDE}pub fn f() -> Felt {{
    return 1;
}}

pub mod inner {{
    pub fn call_super() -> Felt {{
        return super::f();
    }}
}}

fn main() -> Felt {{
    return inner::call_super();
}}"
        ),
    );
}

#[test]
#[serial]
fn bare_function_argument_matches_expected_signature() {
    // Passing a top-level function by bare name walks the scope chain with
    // the call's expected signature instead of resolving a value path.
    accepts(
        "function argument resolved by expected signature",
        &format!(
            "{PRELUDE}pub fn pick(a: Felt, b: Felt) -> Felt {{
    return a + b;
}}

pub trait ApplyFeltFn {{
    pub fn apply(self: Self, f: fn(Felt, Felt) -> Felt) -> Felt;
}}

pub struct Worker {{
    pub x: Felt,
    pub y: Felt,
}}

impl ApplyFeltFn for Worker {{
    pub fn apply(self: Worker, f: fn(Felt, Felt) -> Felt) -> Felt {{
        return f(self.x, self.y);
    }}
}}

fn main() -> Felt {{
    let w: Worker = Worker {{ x: 1, y: 2 }};
    return w.apply(pick);
}}"
        ),
    );
}

#[test]
#[serial]
fn index_sugar_and_member_call_guards() {
    // Inherent method calls resolve through the member-call fast path.
    accepts(
        "inherent method call on an imported struct",
        &format!(
            "{PRELUDE}pub mod m {{
    pub struct T {{ pub field: Felt }}

    impl T {{
        pub fn get(self: T) -> Felt {{
            return self.field;
        }}
    }}
}}

fn main() -> Felt {{
    let t: m::T = m::T {{ field: 2 }};
    return t.get();
}}"
        ),
    );

    // Private inherent methods are reachable across modules: the fast path
    // grants access whenever the program declares any impl method.
    accepts(
        "private method called on a call-result receiver",
        &format!(
            "{PRELUDE}pub mod m {{
    pub struct T {{ pub field: Felt }}

    impl T {{
        fn secret(self: T) -> Felt {{
            return 1;
        }}
    }}
}}

pub fn make() -> m::T {{
    return m::T {{ field: 2 }};
}}

fn main() -> Felt {{
    return make().secret();
}}"
        ),
    );
}

#[test]
#[serial]
fn operator_calls_and_size_position_edges() {
    // `!=` on a custom type lowers to the eq method wrapped in unary not.
    accepts(
        "custom neq reuses the eq method",
        &format!(
            "{PRELUDE}{STRUCT_P}
impl P {{
    pub fn eq(self: P, o: P) -> bool {{
        return self.x == o.x;
    }}
}}

fn main() -> Felt {{
    let a = P::new(1);
    let b = P::new(2);
    let same: bool = a != b;
    return (same) as Felt;
}}"
        ),
    );

    rejects(
        "bitwise and on bools",
        format!("{PRELUDE}fn main() -> Felt {{ return (true & false) as Felt; }}").as_str(),
        "mismatch",
    );

    rejects(
        "negating a u32",
        format!("{PRELUDE}fn main() -> Felt {{ let x: u32 = 1; return (-x) as Felt; }}").as_str(),
        "mismatch",
    );

    // Size-position call arguments: Felt and u32 literals become consts.
    accepts(
        "split_bits with felt and u32 sizes",
        &format!(
            "{PRELUDE}fn main() -> Felt {{
    let a: [Felt; 4] = split_bits(255, 4);
    return a[0];
}}"
        ),
    );

    rejects(
        "calling a parenthesized non-function member",
        &format!(
            "{PRELUDE}{STRUCT_P}
fn main() -> Felt {{
    let p = P::new(1);
    return (p.x)();
}}"
        ),
        "callable",
    );

    // First-class function values resolve through the callee-expression arm.
    accepts(
        "calling a function-typed parameter",
        &format!(
            "{PRELUDE}pub fn twice(x: Felt) -> Felt {{
    return x + x;
}}

pub fn apply(f: fn(Felt) -> Felt, x: Felt) -> Felt {{
    return f(x);
}}

fn main() -> Felt {{
    return apply(twice, 3);
}}"
        ),
    );

    rejects(
        "function-typed parameter arity mismatch",
        &format!(
            "{PRELUDE}pub fn twice(x: Felt) -> Felt {{
    return x + x;
}}

pub fn apply(f: fn(Felt) -> Felt, x: Felt) -> Felt {{
    return f(x, x);
}}

fn main() -> Felt {{
    return apply(twice, 3);
}}"
        ),
        "parameters",
    );
}

#[test]
#[serial]
fn generic_instantiation_rewrites_paths_and_statements() {
    // Instantiating `take::<P>` / `take::<Q>` rewrites the parameter and
    // return type paths, the associated-type alias in Q's impl, and the
    // let/assert/assert_eq/match/return statements of the body.
    accepts(
        "generic fn with associated-type paths",
        &format!(
            "{PRELUDE}pub trait W {{
    pub type Ty;

    pub fn zero() -> Ty;
}}

pub struct P {{ pub x: Felt }}

impl W for P {{
    pub type Ty = Felt;

    pub fn zero() -> Felt {{
        return 0;
    }}
}}

pub struct Q {{}}

impl W for Q {{
    pub type Ty = <P as W>::Ty;

    pub fn zero() -> <P as W>::Ty {{
        return 1;
    }}
}}

pub fn take<T: W>(v: <T as W>::Ty, c: bool) -> <T as W>::Ty {{
    let copied: <T as W>::Ty = v;
    assert(c, \"c\");
    assert_eq(copied, copied, \"same\");
    let picked: <T as W>::Ty = match c {{
        true => copied,
        false => v,
    }};
    return picked;
}}

fn main() -> Felt {{
    let a: Felt = take::<P>(1, true);
    let b: Felt = take::<Q>(2, false);
    return a + b + P::zero() + Q::zero();
}}"
        ),
    );
}

#[test]
#[serial]
fn ambiguous_members_across_traits_and_inherent_associated_types() {
    // Two constraints providing the same method name make the bare call
    // ambiguous.
    rejects(
        "method name provided by two constraints",
        &format!(
            "{PRELUDE}pub trait A {{
    pub fn m(self: Self) -> Felt;
}}

pub trait B {{
    pub fn m(self: Self) -> Felt;
}}

pub fn pick<T: A + B>(t: T) -> Felt {{
    return t.m();
}}

fn main() -> Felt {{
    return 0;
}}"
        ),
        "ambiguous",
    );

    // Two impls of a generic trait for the same receiver type are distinct
    // providers, so the bare call is ambiguous as well.
    rejects(
        "generic trait impls with different arguments",
        &format!(
            "{PRELUDE}{STRUCT_P}
pub trait Conv<T> {{
    pub fn conv(self: Self) -> Felt;
}}

impl Conv<Felt> for P {{
    pub fn conv(self: P) -> Felt {{
        return 1;
    }}
}}

impl Conv<u32> for P {{
    pub fn conv(self: P) -> Felt {{
        return 2;
    }}
}}

fn main() -> Felt {{
    let p = P::new(1);
    return p.conv();
}}"
        ),
        "ambiguous",
    );

    // Associated types on an inherent impl resolve through P::Ty.
    accepts(
        "inherent impl associated type",
        &format!(
            "{PRELUDE}{STRUCT_P}
impl P {{
    pub type Ty = Felt;
}}

fn main() -> Felt {{
    let v: P::Ty = 3;
    return v;
}}"
        ),
    );
}

#[test]
#[serial]
fn array_and_struct_literals_reject_inconsistent_shapes() {
    accepts(
        "consistent array literal",
        &format!("{PRELUDE}fn main() {{ let a = [1, 2, 3]; assert_eq(a[0], 1, \"first\"); }}"),
    );
    rejects(
        "array literal with a mixed element type",
        &format!("{PRELUDE}fn main() {{ let a = [1, true, 3]; }}"),
        "mismatch",
    );
    accepts(
        "generic struct literal with matching arguments",
        &format!(
            "{PRELUDE}pub struct Num<T> {{ pub a: T, pub b: T }}
fn main() {{ let n = Num<Felt> {{ a: 1, b: 2 }}; assert_eq(n.a, 1, \"a\"); }}"
        ),
    );
    // The field values bind T to u32 first; the explicit `<Felt>` argument
    // then disagrees with the bound parameter.
    rejects(
        "generic struct literal argument disagrees with field values",
        &format!(
            "{PRELUDE}pub struct Num<T> {{ pub a: T, pub b: T }}
fn main() {{ let n = Num<Felt> {{ a: 1u32, b: 2u32 }}; }}"
        ),
        "mismatch",
    );
}

#[test]
#[serial]
fn return_placement_rejects_returns_inside_if_blocks() {
    rejects(
        "return inside an if statement",
        &format!("{PRELUDE}fn f(c: bool) -> Felt {{ if c {{ return 1; }} return 2; }}\nfn main() {{ let _ = f(true); }}"),
        "return",
    );
}

#[test]
#[serial]
fn type_annotations_cover_arrays_tuples_and_fn_signatures() {
    accepts(
        "array type annotation",
        &format!("{PRELUDE}fn main() {{ let a: [Felt; 3] = [1, 2, 3]; assert_eq(a[2], 3, \"size\"); }}"),
    );
    accepts(
        "tuple type annotation",
        &format!("{PRELUDE}fn main() {{ let t: (Felt, bool) = (1, true); }}"),
    );
    accepts(
        "fn signature parameter type",
        &format!(
            "{PRELUDE}fn twice(f: fn(Felt) -> Felt, v: Felt) -> Felt {{ return f(f(v)); }}
fn inc(x: Felt) -> Felt {{ return x + 1; }}
fn main() {{ let r = twice(inc, 3); }}"
        ),
    );
    rejects(
        "tuple annotation with mismatched elements",
        &format!("{PRELUDE}fn main() {{ let t: (Felt, bool) = (1, 2); }}"),
        "mismatch",
    );
    // The bare `fn(Felt, Felt) -> Felt` signature disagrees with `add2`'s
    // single-parameter signature, exercising the Function vs
    // FunctionSignature unification arm.
    rejects(
        "fn signature argument shape mismatch",
        &format!(
            "{PRELUDE}fn call2(f: fn(Felt, Felt) -> Felt, v: Felt) -> Felt {{ return f(v, v); }}
fn add2(x: Felt) -> Felt {{ return x + 1; }}
fn main() {{ let r = call2(add2, 3); }}"
        ),
        "mismatch",
    );
}

#[test]
#[serial]
fn size_position_arguments_bind_constants_and_reject_runtime_values() {
    accepts(
        "felt literal in size position",
        &format!("{PRELUDE}fn main() {{ let a: [Felt; 4] = split_bits(255, 4); assert_eq(a[0], 1, \"bit\"); }}"),
    );
    // The u32 literal reaches the size-position arm but then fails the
    // `N: Felt` constraint on split_bits.
    rejects(
        "u32 literal rejected by the felt size constraint",
        &format!("{PRELUDE}fn main() {{ let a: [Felt; 4] = split_bits(255, 4u32); }}"),
        "mismatch",
    );
    accepts(
        "named const flows into a size position",
        &format!("{PRELUDE}const N: Felt = 4;\nfn main() {{ let a: [Felt; 4] = split_bits(255, N); }}"),
    );
    rejects(
        "runtime value in size position",
        &format!(
            "{PRELUDE}fn bad(n: Felt) -> [Felt; 4] {{ return split_bits(255, n); }}
fn main() {{ bad(2); }}"
        ),
        "mismatch",
    );
}

#[test]
#[serial]
fn trait_cast_type_positions_with_segments_resolve() {
    accepts(
        "trait cast with nested associated type segments",
        &format!(
            "{PRELUDE}{STRUCT_P}pub struct Holder {{ pub v: Felt }}
impl Holder {{ pub type Inner = Felt; }}
trait Outer {{ pub type Assoc; }}
impl Outer for P {{ pub type Assoc = Holder; }}
fn probe(x: <P as Outer>::Assoc::Inner) -> <P as Outer>::Assoc::Inner {{ return x; }}
fn main() {{ let v = probe(3); }}"
        ),
    );
    rejects(
        "trait cast to an unimplemented trait in type position",
        &format!(
            "{PRELUDE}{STRUCT_P}trait Other {{ pub type Ty; }}
fn bad(x: <P as Other>::Ty) -> Felt {{ return 1; }}
fn main() {{ bad(1); }}"
        ),
        "mismatch",
    );
}

#[test]
#[serial]
fn generic_path_call_targets_resolve() {
    accepts(
        "explicit generic arguments on a module type path",
        &format!(
            "{PRELUDE}mod m {{
    pub struct Test<T> {{ pub field: T }}
    impl<T> Test<T> {{
        pub fn get_test<S>(a: S) -> Felt {{ return a as Felt; }}
    }}
}}
fn main() -> Felt {{
    let r = m::<Test<Felt>>::get_test::<u32>(666u32);
    let t = m::Test<Felt> {{ field: 1 }};
    return r + t.field;
}}"
        ),
    );
}

#[test]
#[serial]
fn nested_member_access_uses_the_fast_path() {
    // `o.inner.get()` resolves the receiver `o.inner` while the ancestor is
    // the member call, so the member-access fast path (find_member without an
    // expected signature) returns the field member.
    accepts(
        "nested struct field method call",
        &format!(
            "{PRELUDE}pub struct Inner {{ pub v: Felt }}
impl Inner {{ pub fn get(self: Inner) -> Felt {{ return self.v; }} }}
pub struct Outer {{ pub inner: Inner }}
fn main() -> Felt {{
    let o = Outer {{ inner: Inner {{ v: 7 }} }};
    return o.inner.get();
}}"
        ),
    );
    // The private `inner` field is reached through a call-result receiver
    // (not a path), so the impl-method escape hatch does not apply and the
    // member access is rejected as private.
    rejects(
        "private nested field through a call result receiver",
        &format!(
            "{PRELUDE}mod lib {{
    pub struct Inner {{ pub v: Felt }}
    impl Inner {{ pub fn get(self: Inner) -> Felt {{ return self.v; }} }}
    pub struct Wrap {{ inner: Inner }}
    pub fn make() -> Wrap {{ return Wrap {{ inner: Inner {{ v: 7 }} }}; }}
}}
fn main() -> Felt {{ return lib::make().inner.get(); }}"
        ),
        "public",
    );
}

#[test]
#[serial]
fn associated_types_accept_non_path_shapes() {
    accepts(
        "tuple associated type on an inherent impl",
        &format!(
            "{PRELUDE}{STRUCT_P}
impl P {{
    pub type Pair = (Felt, Felt);
    pub fn make_pair() -> P::Pair {{ return (1, 2); }}
}}
fn main() {{ let p: P::Pair = P::make_pair(); }}"
        ),
    );
}
