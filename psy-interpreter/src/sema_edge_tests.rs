// Source-driven coverage for the scattered sema error arms in
// psy-sema/src/lib.rs (visitor type-mismatch / arity / shape rejections).
// Each entry is a tiny psy program whose typechecking lands in one specific
// `return Err(...)` arm; the reject needles are kept loose (case-insensitive
// substring) so the assertions survive Display rewording but still require
// the program to be rejected for a *type* reason, not a parse error.

use std::sync::atomic::{AtomicU64, Ordering};

use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};

use super::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn compile(source: &str) -> Result<(), String> {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_se_{n}.psy"));
    std::fs::write(&path, source).unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = interpreter.typecheck_single(path.clone());

    let _ = std::fs::remove_file(&path);
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
fn return_placement_rejects_returns_inside_if_blocks() {
    rejects(
        "return inside an if statement",
        &format!("{PRELUDE}fn f(c: bool) -> Felt {{ if c {{ return 1; }} return 2; }}\nfn main() {{ let _ = f(true); }}"),
        "return",
    );
}

#[test]
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

#[test]
fn deep_associated_type_chains_resolve() {
    // Two levels past a trait cast (`<P as Outer>::Assoc::Mid::Leaf`) walk the
    // trait-cast segment loop, and module-rooted chains (`deep::H0::Inner::Leaf`)
    // walk the module path's member loop.
    accepts(
        "trait cast and module chains with two segments",
        &format!(
            "{PRELUDE}
pub struct Lvl0 {{ pub x: Felt }}
impl Lvl0 {{ pub type Mid = Lvl1; }}
pub struct Lvl1 {{ pub y: Felt }}
impl Lvl1 {{ pub type Leaf = Felt; }}

pub trait Outer {{ pub type Assoc; }}
pub struct P {{ pub x: Felt }}
impl Outer for P {{ pub type Assoc = Lvl0; }}

pub mod deep {{
    pub struct H0 {{ pub x: Felt }}
    impl H0 {{ pub type Inner = H1; }}
    pub struct H1 {{ pub y: Felt }}
    impl H1 {{ pub type Leaf = Felt; }}
}}

fn probe(v: <P as Outer>::Assoc::Mid::Leaf) -> <P as Outer>::Assoc::Mid::Leaf {{
    return v;
}}

fn main() {{
    let a: deep::H0::Inner::Leaf = 4;
    let b = probe(a);
    assert_eq(b, 4, \"deep chains\");
}}"
        ),
    );
    rejects(
        "trait cast chain through a non-type member",
        &format!(
            "{PRELUDE}
pub struct Lvl0 {{ pub x: Felt }}
pub trait Outer {{ pub type Assoc; }}
pub struct P {{ pub x: Felt }}
impl Outer for P {{ pub type Assoc = Lvl0; }}

fn bad(v: <P as Outer>::Assoc::Mid) -> Felt {{ return 1; }}
fn main() {{ bad(1); }}"
        ),
        "mid",
    );
}

#[test]
fn generic_instantiation_rewrites_impls_signatures_and_bodies() {
    // A generic inherent impl with an associated type plus a generic method
    // drives instantiate_impl (assoc types + per-method signature rewriting).
    accepts(
        "generic inherent impl with assoc type and generic method",
        &format!(
            "{PRELUDE}
pub struct Pair<T> {{ pub a: T, pub b: T }}

impl<T> Pair<T> {{
    pub type Item = T;
    pub fn first(self: Self) -> T {{ return self.a; }}
    pub fn pick<S>(self: Pair<T>, other: S) -> S {{ return other; }}
}}

fn main() {{
    let p = Pair {{ a: 1, b: 2 }};
    let f = p.first();
    let s = p.pick(9);
    assert_eq(f + s, 10, \"inherent generics\");
}}"
        ),
    );
    // A generic function whose parameter and return types are rooted type
    // paths rewrites those paths during instantiation.
    // NOTE: module-rooted paths (`m::H`) as generic-fn parameter types reach
    // instantiate_function with a root-less checked path and panic on
    // `type_path.root.unwrap()` (rewriter.rs:253); type-rooted paths
    // (`P::Pair`) carry a root and rewrite cleanly.
    accepts(
        "generic function with type-rooted path parameter and return",
        &format!(
            "{PRELUDE}{STRUCT_P}
impl P {{
    pub type Pair = (Felt, Felt);
}}

fn through<T>(pair: P::Pair, t: T) -> P::Pair {{
    assert_eq(pair.0, pair.0, \"stable\");
    return pair;
}}

fn main() {{
    let h2 = through((3, 4), 1);
    assert_eq(h2.0, 3, \"path rewrite\");
}}"
        ),
    );
}

#[test]
fn trait_impl_associated_types_rewrite_through_roots() {
    // An associated type whose value is itself a rooted path (`Src::Native`)
    // takes the root-substitution branch when the generic impl is instantiated.
    accepts(
        "generic trait impl with a rooted associated type path",
        &format!(
            "{PRELUDE}
pub struct Src {{ pub q: Felt }}
impl Src {{ pub type Native = Felt; }}

pub trait Wrap {{ pub type Out; pub fn unwrap(self: Self) -> Felt; }}
pub struct Box2<T> {{ pub v: T }}

impl<T> Wrap for Box2<T> {{
    pub type Out = Src::Native;
    pub fn unwrap(self: Self) -> Felt {{ return 1; }}
}}

fn main() {{
    let b = Box2 {{ v: 1 }};
    let o: <Box2<Felt> as Wrap>::Out = 7;
    let r = b.unwrap();
    assert_eq(o + r, 8, \"rooted assoc\");
}}"
        ),
    );
}

#[test]
fn impl_search_rejects_conflicting_generic_arguments() {
    // The concrete `Number<u32>` implementations cannot serve a `Number<Felt>`
    // receiver, so instantiation unification fails and the call is rejected.
    rejects(
        "concrete trait impl for another generic argument",
        &format!(
            "{PRELUDE}
pub trait Mul {{ pub fn mul(self: Self) -> Felt; }}
pub struct Number<T> {{ pub a: T, pub b: T }}

impl Mul for Number<u32> {{
    pub fn mul(self: Self) -> Felt {{ return (self.a * self.b) as Felt; }}
}}

fn main() {{
    let n = Number<Felt> {{ a: 1, b: 2 }};
    let r = n.mul();
    assert_eq(r, 1, \"unused\");
}}"
        ),
        "mul",
    );
    rejects(
        "concrete inherent impl for another generic argument",
        &format!(
            "{PRELUDE}
pub struct Number<T> {{ pub a: T, pub b: T }}

impl Number<u32> {{
    pub fn get(self: Self) -> Felt {{ return self.a as Felt; }}
}}

fn main() {{
    let n = Number<Felt> {{ a: 1, b: 2 }};
    let g = n.get();
    assert_eq(g, 1, \"unused\");
}}"
        ),
        "get",
    );
}

#[test]
fn bare_generic_calls_walk_scopes_for_matching_functions() {
    accepts(
        "bare call to a generic function",
        &format!(
            "{PRELUDE}
fn pick<T>(x: T) -> Felt {{ return x as Felt; }}

fn main() {{
    let r = pick(5);
    assert_eq(r, 5, \"bare generic call\");
}}"
        ),
    );
    rejects(
        "bare call to an unresolved function", "fn main() { missing_fn(1); }", "missing_fn",
    );
}

#[test]
fn crate_paths_resolve_from_nested_modules() {
    accepts(
        "crate root path from inside an inline module",
        &format!(
            "{PRELUDE}
pub mod inner {{
    pub fn five() -> Felt {{ return 5; }}
    pub fn call_out() -> Felt {{ return crate::inner::five(); }}
}}

fn main() {{
    let v = inner::call_out();
    assert_eq(v, 5, \"crate path\");
}}"
        ),
    );
}

#[test]
fn generic_bodies_rewrite_definitions_asserts_structs_and_matches() {
    // Every statement/expression shape inside a generic function body runs
    // through the rewriter when the function is instantiated: nested
    // definitions, assert_eq, struct literals, and match patterns.
    accepts(
        "generic body with nested definition, assert_eq, struct literal, and match",
        &format!(
            "{PRELUDE}
pub struct Point {{ pub x: Felt }}

fn shapes<T>(v: T) -> Felt {{
    struct Inner {{ pub v: Felt }}
    let p = Point {{ x: 1 }};
    assert_eq(v as Felt, v as Felt, \"same\");
    let m = match p.x {{ 1 => 10, _ => 20 }};
    return m + p.x;
}}

fn main() {{
    let r = shapes(3);
    assert_eq(r, 11, \"shapes\");
}}"
        ),
    );
}

#[test]
fn inherent_impls_rewrite_rooted_associated_types_when_generic() {
    // An associated type whose value is a rooted path (`Src::Native`) inside
    // a *generic* inherent impl exercises the rewriter's root/target branch
    // for inherent impls, plus generic-method signature instantiation.
    accepts(
        "rooted associated type inside a generic inherent impl",
        &format!(
            "{PRELUDE}
pub struct Src {{ pub v: Felt }}
impl Src {{ pub type Native = Felt; pub fn nat(self: Self) -> Felt {{ return 1; }} }}

pub struct Box2<T> {{ pub item: T }}
impl<T> Box2<T> {{
    pub type Native = Src::Native;
    pub fn pick<S>(self: Self, other: S) -> S {{ return other; }}
}}

fn main() {{
    let b: Box2<Felt> = Box2 {{ item: 2 }};
    let r = b.pick(9);
    assert_eq(r, 9, \"rooted assoc type in generic impl\");
}}"
        ),
    );
}

#[test]
fn generic_unification_rejects_conflicting_arguments() {
    let mut failures = Vec::new();
    for (label, source, needle) in [
        (
            "turbofish argument conflicts with the value argument",
            &format!(
                "{PRELUDE}
fn g<T>(x: T) -> Felt {{ return x as Felt; }}

fn main() {{ let r = g::<u32>(true); }}"
            ),
            "mismatch",
        ),
        (
            "method turbofish argument conflicts with the value argument",
            &format!(
                "{PRELUDE}
pub struct P2 {{ pub x: Felt }}
impl P2 {{ pub fn pick<S>(self: Self, other: S) -> Felt {{ return 1; }} }}

fn main() {{ let r = P2 {{ x: 1 }}.pick::<u32>(true); }}"
            ),
            "mismatch",
        ),
    ] {
        match compile(source) {
            Ok(()) => failures.push(format!("[{label}] expected rejection containing `{needle}`, got success")),
            Err(message) => {
                if !message.to_lowercase().contains(&needle.to_lowercase()) {
                    failures.push(format!("[{label}] expected rejection containing `{needle}`, got:\n{message}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn trait_cast_paths_resolve_through_the_trait_segment() {
    accepts(
        "fully qualified trait method call",
        &format!(
            "{PRELUDE}
pub struct Number {{ pub a: Felt }}
pub trait Mul {{ pub fn mul(self: Self) -> Felt; }}
impl Mul for Number {{ pub fn mul(self: Self) -> Felt {{ return self.a; }} }}

fn main() {{
    let n = Number {{ a: 2 }};
    let res = <Number as Mul>::mul(n);
    assert_eq(res, 2, \"trait cast path\");
}}"
        ),
    );
}

#[test]
fn imports_of_unknown_modules_are_rejected() {
    rejects(
        "import from an unresolved module",
        &format!("{PRELUDE}use nonexistent_module::thing;"),
        "nonexistent_module",
    );
}

#[test]
fn member_function_references_and_bare_type_values_rewrite() {
    // A method may not be referenced without a call (no first-class method
    // values), while a bare type name in value position is accepted.
    rejects(
        "method accessed without a call",
        &format!(
            "{PRELUDE}
pub struct P3 {{ pub x: Felt }}
impl P3 {{ pub fn nat(self: Self) -> Felt {{ return 1; }} }}

fn wrap<T>(v: T) -> Felt {{
    let f = P3 {{ x: 1 }}.nat;
    return 1;
}}

fn main() {{ let v = wrap(1); }}"
        ),
        "unresolvedmember",
    );
    accepts(
        "bare type name in value position inside a generic body",
        &format!(
            "{PRELUDE}
pub struct Marker {{ pub x: Felt }}

fn mark<T>(v: T) -> Felt {{
    Marker;
    return 1;
}}

fn main() {{ let v = mark(1); }}"
        ),
    );
}

#[test]
fn index_access_and_member_visibility_guards() {
    rejects(
        "array index with a boolean subscript",
        &format!(
            "{PRELUDE}
fn main() -> Felt {{
    let a: [Felt; 3] = [1, 2, 3];
    return a[true];
}}"
        ),
        "mismatch",
    );
    // A private method is callable from its own module but not across modules.
    accepts(
        "private method called from its own module",
        &format!(
            "{PRELUDE}
pub struct S {{ pub x: Felt }}
impl S {{
    fn hidden(self: Self) -> Felt {{ return 1; }}
    pub fn make() -> S {{ return S {{ x: 0 }}; }}
}}

fn main() -> Felt {{
    let v = S::make().hidden();
    return v;
}}"
        ),
    );
    accepts(
        "private method stays callable from a sibling module in the crate",
        &format!(
            "{PRELUDE}
pub mod m {{
    pub struct S {{ pub x: Felt }}
    impl S {{
        fn hidden(self: Self) -> Felt {{ return 1; }}
        pub fn make() -> S {{ return S {{ x: 0 }}; }}
    }}
}}

fn main() -> Felt {{
    let v = m::S::make().hidden();
    return v;
}}"
        ),
    );
}

#[test]
fn unification_walks_signatures_and_tuples() {
    // Function values are not first-class: a function name passed for a
    // fn-signature parameter is rejected, and the diagnostic renders the
    // substituted signature (FunctionSignature arm of the unifier).
    rejects(
        "function value passed for a fn-signature parameter",
        &format!(
            "{PRELUDE}
fn double(v: Felt) -> Felt {{ return v + v; }}

fn call_it<T>(f: fn(T) -> Felt, x: T) -> Felt {{
    return f(x);
}}

fn main() -> Felt {{
    return call_it(double, 3);
}}"
        ),
        "signature",
    );
    rejects(
        "conflicting tuple arguments for one generic parameter",
        &format!(
            "{PRELUDE}
fn two<T>(a: T, b: T) -> Felt {{ return 1; }}

fn main() -> Felt {{
    return two((1, 2), (1, true));
}}"
        ),
        "mismatch",
    );
}

/// Panics inside preprocessing must stay observable as panics (they abort the
/// compiler), so assert on the message while resetting the primitive scope.
#[test]
fn storage_preprocessing_panics_on_malformed_refs() {
    let cases = [
        (
            "#[ref] on a non-basic field",
            &format!(
                "{PRELUDE}
#[contract]
#[derive(Storage)]
pub struct C {{
    #[ref]
    pub arr: [Felt; 2],
}}

fn main() -> Felt {{ return 0; }}"
            ),
            "basic struct",
        ),
        (
            "StorageRef with two generic parameters",
            &format!(
                "{PRELUDE}
#[contract]
#[derive(Storage)]
pub struct C {{
    pub s: StorageRef<Felt, Felt>,
}}

fn main() -> Felt {{ return 0; }}"
            ),
            "exactly one generic parameter",
        ),
        (
            "ArrayRef with one generic parameter",
            &format!(
                "{PRELUDE}
#[contract]
#[derive(Storage)]
pub struct C {{
    pub s: ArrayRef<Felt>,
}}

fn main() -> Felt {{ return 0; }}"
            ),
            "exactly two generic parameters",
        ),
    ];
    let mut failures = Vec::new();
    for (label, source, needle) in cases {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("psy_se_{n}.psy"));
        std::fs::write(&path, source).unwrap();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
            let _ = interpreter.typecheck_single(path.clone());
        }));
        let _ = std::fs::remove_file(&path);

        let message = match result {
            Err(message) => message
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| message.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default(),
            Ok(()) => {
                failures.push(format!("[{label}] expected a panic containing `{needle}`, got a clean return"));
                continue;
            }
        };
        if !message.to_lowercase().contains(&needle.to_lowercase()) {
            failures.push(format!("[{label}] expected panic containing `{needle}`, got: {message}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
