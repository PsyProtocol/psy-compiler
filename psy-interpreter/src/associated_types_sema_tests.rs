// Deep QA for associated types, associated functions (methods), and constants.
//
// Drives the real parser + sema through `Interpreter::typecheck_single` with
// temporary `.psy` sources. Every case names the concrete contract it defends
// and asserts the exact accept/reject outcome.
//
// The shared `STD_PRIMITIVE_SCOPE_ID` singleton is reset after *every* case
// (via `check`, which tears down before the caller can panic) so the suite
// is hermetic.

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};
use serial_test::serial;

use super::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Typecheck `source` written to a throwaway temp file, then ALWAYS tear down
/// the file and reset the shared primitive-scope singleton. Returns `None` if
/// the program typechecked, or `Some(formatted_error)` if it was rejected.
fn check(source: &str) -> Option<String> {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_assoc_{unique}_{n}.psy"));
    fs::write(&path, source).unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = interpreter.typecheck_single(path.clone());

    let _ = fs::remove_file(path);
    #[allow(static_mut_refs)]
    unsafe {
        let _ = STD_PRIMITIVE_SCOPE_ID.take();
    }

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
// (1) Associated type declaration in trait + override in impl
// ═══════════════════════════════════════════════════════════════════════════

/// Trait declares an associated type; impl overrides it with a concrete type.
/// The override must correctly replace the trait's type variable.
/// Defends: `visit_trait_impl` (lib.rs:2844-2875) unifies the trait's associated
/// type variable with the impl's concrete type.
#[test]
#[serial]
fn at01_assoc_type_decl_and_override_typechecks() {
    expect_accept(
        "at01_assoc_type_decl_and_override_typechecks",
        r#"
pub trait HasTy {
    pub type Ty;
    pub fn get() -> Self::Ty;
}
pub struct Foo { pub v: Felt }
impl HasTy for Foo {
    pub type Ty = Felt;
    pub fn get() -> Self::Ty { return 42; }
}
fn main() {}
"#,
    );
}

/// Associated type override with a different concrete type than the trait
/// variable's constraints allow should fail when a constraint is present.
/// Defends: the unification at lib.rs:2857 enforces trait constraints on the
/// associated type override.
#[test]
#[serial]
fn at02_assoc_type_override_with_wrong_constraint_rejected() {
    expect_reject(
        "at02_assoc_type_override_with_wrong_constraint_rejected",
        r#"
pub trait NewTrait { pub fn new() -> Self; }
pub trait HasItem {
    pub type Item: NewTrait;
}
pub struct Foo { pub v: Felt }
impl HasItem for Foo {
    pub type Item = Felt;
}
fn main() {}
"#,
        // Felt does not implement NewTrait, so the constraint unification
        // should reject.  The exact error depends on how the type variable
        // constraint is enforced during unification.
        "TypeMismatch",
    );
}

/// Trait with no associated type, impl also has none — typechecks.
#[test]
#[serial]
fn at03_trait_no_assoc_type_typechecks() {
    expect_accept(
        "at03_trait_no_assoc_type_typechecks",
        r#"
pub trait T { pub fn m(self: Self) -> Felt; }
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub fn m(self: Self) -> Felt { return self.v; }
}
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (2) Associated type path access <T as Trait>::Ty
// ═══════════════════════════════════════════════════════════════════════════

/// `<Foo as HasTy>::Ty` in type position resolves to the concrete override.
/// Defends: resolve_path trait-cast branch (resolver.rs:24-91) with no
/// segments resolves the associated type via find_member_with_flags.
#[test]
#[serial]
fn at04_trait_cast_assoc_type_in_type_position() {
    expect_accept(
        "at04_trait_cast_assoc_type_in_type_position",
        r#"
pub trait HasTy { pub type Ty; }
pub struct Foo { pub v: Felt }
impl HasTy for Foo { pub type Ty = Felt; }
fn main() {
    let x: <Foo as HasTy>::Ty = 42;
}
"#,
    );
}

/// `<Foo as HasTy>::Ty` where Ty is overridden to a struct type.
#[test]
#[serial]
fn at05_trait_cast_assoc_type_to_struct_type() {
    expect_accept(
        "at05_trait_cast_assoc_type_to_struct_type",
        r#"
pub trait HasTy { pub type Ty; }
pub struct Inner { pub v: Felt }
pub struct Foo { pub v: Felt }
impl HasTy for Foo { pub type Ty = Inner; }
fn main() {
    let x: <Foo as HasTy>::Ty = Inner { v: 42 };
}
"#,
    );
}

/// `Self::Ty` inside an impl method resolves to the overridden type.
/// Defends: Self type is bound to the implementor (lib.rs:2840), and
/// Self::Ty resolves through find_associated_type on the implementor.
#[test]
#[serial]
fn at06_self_assoc_type_in_impl_method() {
    expect_accept(
        "at06_self_assoc_type_in_impl_method",
        r#"
pub trait HasTy {
    pub type Ty;
    pub fn get() -> Self::Ty;
    pub fn set(x: Self::Ty);
}
pub struct Foo { pub v: Felt }
impl HasTy for Foo {
    pub type Ty = Felt;
    pub fn get() -> Self::Ty { return 42; }
    pub fn set(x: Self::Ty) {}
}
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (3) Associated function / method calls
// ═══════════════════════════════════════════════════════════════════════════

/// `Type::method()` — inherent associated function call.
/// Defends: resolve_path with root=Some(Type), no segments, target=method
/// (resolver.rs:94-114) resolves through find_member on the impl.
#[test]
#[serial]
fn at07_inherent_method_call() {
    expect_accept(
        "at07_inherent_method_call",
        r#"
pub struct Foo { pub v: Felt }
impl Foo {
    pub fn make() -> Foo { return Foo { v: 42 }; }
}
fn main() {
    let f = Foo::make();
}
"#,
    );
}

/// `<Foo as Trait>::method()` — trait method dispatch.
/// Defends: resolve_path trait-cast with no segments (resolver.rs:71-91).
#[test]
#[serial]
fn at08_trait_method_dispatch() {
    expect_accept(
        "at08_trait_method_dispatch",
        r#"
pub trait T { pub fn m(self: Self) -> Felt; }
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub fn m(self: Self) -> Felt { return self.v; }
}
fn main() {
    let f = Foo { v: 42 };
    let r = <Foo as T>::m(f);
}
"#,
    );
}

/// Calling a trait method on a type that does not implement the trait fails.
///
/// FINDING (minor): `visit_call` (lib.rs:1472-1526) bypasses the
/// `implements_trait` check that `resolve_path` has (resolver.rs:28). When
/// `<Bar as T>::m(b)` is called and Bar doesn't implement T, the error is
/// `UnresolvedMember` (method not found on Bar) rather than `TypeMismatch`
/// (trait not implemented). The call IS rejected, so type safety is
/// maintained, but the error message is misleading. The root cause is that
/// `visit_call` calls `self.typecheck(root, ctx)` on the trait-cast type,
/// which for `UncheckedType::TraitCast` just typechecks the impl type and
/// discards the trait (lib.rs:3327), then calls `find_member` directly
/// without the trait implementation check.
#[test]
#[serial]
fn at09_trait_method_on_non_implementor_rejected() {
    expect_reject(
        "at09_trait_method_on_non_implementor_rejected",
        r#"
pub trait T { pub fn m(self: Self) -> Felt; }
pub struct Foo { pub v: Felt }
pub struct Bar { pub v: Felt }
impl T for Foo {
    pub fn m(self: Self) -> Felt { return self.v; }
}
fn main() {
    let b = Bar { v: 42 };
    let r = <Bar as T>::m(b);
}
"#,
        "TypeMismatch",
    );
}

/// Associated function with no self parameter (static method) via trait cast.
#[test]
#[serial]
fn at10_trait_static_method_via_cast() {
    expect_accept(
        "at10_trait_static_method_via_cast",
        r#"
pub trait T { pub fn make() -> Felt; }
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub fn make() -> Felt { return 42; }
}
fn main() {
    let r = <Foo as T>::make();
}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (4) Default method implementation in trait
// ═══════════════════════════════════════════════════════════════════════════

/// Default method body in trait; impl does NOT override it. The default body
/// should be copied into the impl and be callable.
/// Defends: visit_trait_impl (lib.rs:2909-2922) copies unimplemented trait
/// methods as generated default methods.
#[test]
#[serial]
fn at11_default_method_not_overridden_typechecks() {
    expect_accept(
        "at11_default_method_not_overridden_typechecks",
        r#"
pub trait T {
    pub fn m() -> Felt { return 42; }
}
pub struct Foo { pub v: Felt }
impl T for Foo {}
fn main() {
    let r = <Foo as T>::m();
}
"#,
    );
}

/// Default method overridden — the override should be used instead.
/// Defends: visit_trait_impl (lib.rs:2881-2907) matches override methods by
/// name and removes them from unimplemented_methods.
#[test]
#[serial]
fn at12_default_method_overridden_typechecks() {
    expect_accept(
        "at12_default_method_overridden_typechecks",
        r#"
pub trait T {
    pub fn m() -> Felt { return 42; }
}
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub fn m() -> Felt { return 99; }
}
fn main() {
    let r = <Foo as T>::m();
}
"#,
    );
}

/// Default method that uses Self::AssocTy — the default body references the
/// associated type, which is resolved when the default is copied into the impl.
/// Based on trait_default_associated_type_test.psy pattern.
#[test]
#[serial]
fn at13_default_method_using_assoc_type() {
    expect_accept(
        "at13_default_method_using_assoc_type",
        r#"
pub trait NewTrait { pub fn new() -> Self; }
pub struct FeltNum { pub v: Felt }
impl NewTrait for FeltNum {
    pub fn new() -> Self { return FeltNum { v: 777 }; }
}
pub trait Factory {
    pub type Item: NewTrait;
    pub fn make() -> Self::Item { Self::Item::new() }
}
impl Factory for FeltNum {
    pub type Item = FeltNum;
    pub fn make() -> Self::Item { Self::Item::new() }
}
fn main() {
    let x: FeltNum = <FeltNum as Factory>::make();
}
"#,
    );
}

/// Default method using Self::Item without overriding make — relies on the
/// trait's default body being copied into the impl.
#[test]
#[serial]
fn at14_default_method_not_overridden_with_assoc_type() {
    expect_accept(
        "at14_default_method_not_overridden_with_assoc_type",
        r#"
pub trait NewTrait { pub fn new() -> Self; }
pub struct FeltNum { pub v: Felt }
impl NewTrait for FeltNum {
    pub fn new() -> Self { return FeltNum { v: 777 }; }
}
pub trait Factory {
    pub type Item: NewTrait;
    pub fn make() -> Self::Item { Self::Item::new() }
}
impl Factory for FeltNum {
    pub type Item = FeltNum;
}
fn main() {
    let x: FeltNum = <FeltNum as Factory>::make();
}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (5) Generic trait associated types
// ═══════════════════════════════════════════════════════════════════════════

/// `trait Container<G> { pub type Item; }` with `impl<G> Container<G> for Foo
/// { pub type Item = G; }` — the impl's generic param G is substituted into
/// the associated type.
/// Defends: visit_trait_impl (lib.rs:2797-2838) unifies trait and impl generic
/// parameters, and the associated type value G resolves in the impl scope.
#[test]
#[serial]
fn at15_generic_trait_assoc_type_substitution() {
    expect_accept(
        "at15_generic_trait_assoc_type_substitution",
        r#"
pub trait Container<G> {
    pub type Item;
}
pub struct Box { pub v: Felt }
impl<G> Container<G> for Box {
    pub type Item = G;
}
fn main() {}
"#,
    );
}

/// Generic trait with concrete instantiation — `impl Container<Felt> for Box`.
#[test]
#[serial]
fn at16_generic_trait_concrete_assoc_type() {
    expect_accept(
        "at16_generic_trait_concrete_assoc_type",
        r#"
pub trait Container<G> {
    pub type Item;
}
pub struct Box { pub v: Felt }
impl Container<Felt> for Box {
    pub type Item = Felt;
}
fn main() {}
"#,
    );
}

/// Generic trait associated type accessed via trait cast with concrete generic.
#[test]
#[serial]
fn at17_generic_trait_assoc_type_access() {
    expect_accept(
        "at17_generic_trait_assoc_type_access",
        r#"
pub trait Container<G> {
    pub type Item;
}
pub struct Box { pub v: Felt }
impl<G> Container<G> for Box {
    pub type Item = G;
}
fn main() {
    let x: <Box as Container<Felt>>::Item = 42;
}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (6) Associated type in derived traits (Storage derive RefType pattern)
// ═══════════════════════════════════════════════════════════════════════════

/// NOTE: `#[derive(Storage)]` cannot be tested via `typecheck_single` because
/// the generated code references std types (Storage trait, StorageRef,
/// StorageNew, ContractMetadata) that are not loaded in single-file mode.
/// Instead, we test the same associated-type pattern (RefType) manually.

/// Manual RefType pattern: a trait with `pub type RefType;` and an impl that
/// overrides it — the same pattern Storage derive generates.
/// Defends: the RefType associated type pattern used by Storage derive
/// (preprocess.rs:155-170) works when written manually.
#[test]
#[serial]
fn at18_manual_reftype_pattern_typechecks() {
    expect_accept(
        "at18_manual_reftype_pattern_typechecks",
        r#"
pub trait HasRef {
    pub type RefType;
    pub fn get_ref() -> Self::RefType;
}
pub struct Foo { pub v: Felt }
impl HasRef for Foo {
    pub type RefType = Felt;
    pub fn get_ref() -> Self::RefType { return 42; }
}
fn main() {}
"#,
    );
}

/// `Foo::RefType` used as a type annotation — the associated type must be
/// resolvable in type position, even when private (no `pub`).
/// Defends: resolve_member_type (resolver.rs:420-424) skips visibility for
/// is_ty paths, so a private associated type is accessible in type position.
#[test]
#[serial]
fn at19_private_reftype_in_type_annotation() {
    expect_accept(
        "at19_private_reftype_in_type_annotation",
        r#"
pub struct Foo { pub v: Felt }
impl Foo {
    type RefType = Felt;
}
fn main() {
    let x: Foo::RefType = 42;
}
"#,
    );
}

#[test]
#[serial]
fn at19b_ambiguous_associated_type_rejected() {
    expect_reject(
        "at19b_ambiguous_associated_type",
        r#"
trait A { type Item; }
trait B { type Item; }
struct S {}
impl A for S { type Item = Felt; }
impl B for S { type Item = u32; }
fn main() { let value: S::Item = 1; }
"#,
        "AmbiguousAssociatedType",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (7) Chained associated type access <T as A>::Ty::method()
// ═══════════════════════════════════════════════════════════════════════════

/// `<Number as TraitWithType>::Ty::get_a(num)` — chained access: resolve the
/// associated type Ty through the trait cast, then call get_a on it.
/// Based on path_test.psy:110 pattern.
/// Defends: resolve_path trait-cast with segments (resolver.rs:40-70) resolves
/// Ty via find_member, then get_a via find_member_with_flags.
#[test]
#[serial]
fn at20_chained_assoc_type_method_call() {
    expect_accept(
        "at20_chained_assoc_type_method_call",
        r#"
pub trait TraitWithType {
    pub type Ty;
    pub fn test_fn() {}
}
pub struct Number { pub a: Felt, pub b: Felt }
impl TraitWithType for Number {
    pub type Ty = Number;
    pub fn test_fn() {}
}
impl Number {
    pub fn get_a(self: Self) -> Felt { return self.a; }
}
fn main() {
    let num = Number { a: 1, b: 2 };
    let r = <Number as TraitWithType>::Ty::get_a(num);
}
"#,
    );
}

/// Chained access with generic types — `<Number<Felt> as Trait>::Ty::get_a()`.
/// Based on path_test.psy:110 with generics.
#[test]
#[serial]
fn at21_chained_assoc_type_with_generics() {
    expect_accept(
        "at21_chained_assoc_type_with_generics",
        r#"
pub trait TraitWithType {
    pub type Ty;
    pub fn test_fn() {}
}
pub struct Number<T> { pub a: T, pub b: T }
impl<T> TraitWithType for Number<T> {
    pub type Ty = Number<T>;
    pub fn test_fn() {}
}
impl<T> Number<T> {
    pub fn get_a(self: Self) -> Felt { return self.a as Felt; }
}
fn main() {
    let num = Number::<Felt> { a: 1, b: 2 };
    let r = <Number<Felt> as TraitWithType>::Ty::get_a(num);
}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (8) Associated type visibility — private associated type cross-module
// ═══════════════════════════════════════════════════════════════════════════

/// A private associated type in an inherent impl is accessible in type
/// position cross-module, because resolve_member_type skips visibility for
/// is_ty paths (resolver.rs:423).
/// Defends: the is_ty visibility skip at resolver.rs:423.
#[test]
#[serial]
fn at22_private_assoc_type_accessible_in_type_position() {
    expect_accept(
        "at22_private_assoc_type_accessible_in_type_position",
        r#"
pub mod m {
    pub struct Foo { pub v: Felt }
    impl Foo { type PrivTy = Felt; }
}
fn main() {
    let x: m::Foo::PrivTy = 42;
}
"#,
    );
}

/// A private associated type in an impl, accessed in expression position
/// (e.g. calling a method on it) — this should still work because associated
/// type resolution in find_associated_type does not check visibility.
/// The visibility check in resolve_member_type only applies to non-is_ty paths.
#[test]
#[serial]
fn at23_private_assoc_type_method_still_resolves() {
    expect_accept(
        "at23_private_assoc_type_method_still_resolves",
        r#"
pub mod m {
    pub struct Foo { pub v: Felt }
    impl Foo {
        type PrivTy = Felt;
        pub fn get_ty() -> Self::PrivTy { return 42; }
    }
}
fn main() {
    let r = m::Foo::get_ty();
}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (9) Associated type in function signatures — param, return type
// ═══════════════════════════════════════════════════════════════════════════

/// Associated type used as function parameter type via Self::Ty.
/// Defends: Self::Ty resolves in the impl method signature context.
#[test]
#[serial]
fn at24_assoc_type_as_param_type() {
    expect_accept(
        "at24_assoc_type_as_param_type",
        r#"
pub trait T {
    pub type Ty;
    pub fn convert(x: Self::Ty) -> Felt;
}
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub type Ty = Felt;
    pub fn convert(x: Self::Ty) -> Felt { return x; }
}
fn main() {}
"#,
    );
}

/// Associated type used as return type via Self::Ty.
#[test]
#[serial]
fn at25_assoc_type_as_return_type() {
    expect_accept(
        "at25_assoc_type_as_return_type",
        r#"
pub trait T {
    pub type Ty;
    pub fn produce() -> Self::Ty;
}
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub type Ty = Felt;
    pub fn produce() -> Self::Ty { return 42; }
}
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (10) Missing associated type in impl — error
// ═══════════════════════════════════════════════════════════════════════════

/// Impl that does not provide a required associated type fails with
/// MissingAssociatedType.
/// Defends: visit_trait_impl (lib.rs:2845-2849) returns MissingAssociatedType
/// when the impl doesn't override a declared associated type.
#[test]
#[serial]
fn at26_missing_assoc_type_rejected() {
    expect_reject(
        "at26_missing_assoc_type_rejected",
        r#"
pub trait T {
    pub type Ty;
    pub fn m() -> Felt;
}
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub fn m() -> Felt { return 42; }
}
fn main() {}
"#,
        "missing associated type",
    );
}

/// Multiple associated types declared, one missing — error names the missing one.
#[test]
#[serial]
fn at27_partial_missing_assoc_type_rejected() {
    expect_reject(
        "at27_partial_missing_assoc_type_rejected",
        r#"
pub trait T {
    pub type Ty;
    pub type Ty2;
    pub fn m() -> Felt;
}
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub type Ty = Felt;
    pub fn m() -> Felt { return 42; }
}
fn main() {}
"#,
        "missing associated type",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (11) Associated type used in struct fields
// ═══════════════════════════════════════════════════════════════════════════

/// Struct field with associated type path `T::Ty` where T is a generic param
/// with a trait constraint.
/// Defends: resolve_path on a type variable root with constraint, resolving
/// the associated type through get_trait_member (implementer.rs:207-213).
#[test]
#[serial]
fn at28_assoc_type_in_struct_field() {
    expect_accept(
        "at28_assoc_type_in_struct_field",
        r#"
pub trait HasTy { pub type Ty; }
pub struct Foo { pub v: Felt }
impl HasTy for Foo { pub type Ty = Felt; }
pub struct Wrapper<X: HasTy> {
    pub field: X::Ty,
}
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (12) Associated type in generic constraints — T::AssocTy in generic fn
// ═══════════════════════════════════════════════════════════════════════════

/// Generic function with trait constraint, using T::AssocTy in the body.
/// Defends: find_member on a type variable (implementer.rs:328-335) resolves
/// the associated type through the trait constraint.
#[test]
#[serial]
fn at29_assoc_type_in_generic_fn_body() {
    expect_accept(
        "at29_assoc_type_in_generic_fn_body",
        r#"
pub trait HasTy {
    pub type Ty;
    pub fn get() -> Self::Ty;
}
pub struct Foo { pub v: Felt }
impl HasTy for Foo {
    pub type Ty = Felt;
    pub fn get() -> Self::Ty { return 42; }
}
fn use_it<T: HasTy>() {
    let x = <T as HasTy>::get();
}
fn main() {
    use_it::<Foo>();
}
"#,
    );
}

/// Generic function returning T::AssocTy.
#[test]
#[serial]
fn at30_generic_fn_return_assoc_type() {
    expect_accept(
        "at30_generic_fn_return_assoc_type",
        r#"
pub trait HasTy {
    pub type Ty;
    pub fn get() -> Self::Ty;
}
pub struct Foo { pub v: Felt }
impl HasTy for Foo {
    pub type Ty = Felt;
    pub fn get() -> Self::Ty { return 42; }
}
fn use_it<T: HasTy>() -> T::Ty {
    return <T as HasTy>::get();
}
fn main() {
    let x = use_it::<Foo>();
}
"#,
    );
}

/// Trait method called on a generic type variable without trait cast —
/// `T::get()` where `T: HasTy`. This uses the constraint-based method lookup
/// (implementer.rs:328-335).
#[test]
#[serial]
fn at31_generic_type_method_via_constraint() {
    expect_accept(
        "at31_generic_type_method_via_constraint",
        r#"
pub trait HasTy {
    pub type Ty;
    pub fn get() -> Self::Ty;
}
pub struct Foo { pub v: Felt }
impl HasTy for Foo {
    pub type Ty = Felt;
    pub fn get() -> Self::Ty { return 42; }
}
fn use_it<T: HasTy>() {
    let x = T::get();
}
fn main() {
    use_it::<Foo>();
}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (extra) Trait method not in trait — UnresolvedTraitMethod
// ═══════════════════════════════════════════════════════════════════════════

/// Impl provides a method not declared in the trait — should fail with
/// UnresolvedTraitMethod.
/// Defends: visit_trait_impl (lib.rs:2890-2904) matches impl methods against
/// trait methods by name and errors if no match.
#[test]
#[serial]
fn at32_impl_method_not_in_trait_rejected() {
    expect_reject(
        "at32_impl_method_not_in_trait_rejected",
        r#"
pub trait T {
    pub fn m() -> Felt;
}
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub fn m() -> Felt { return 42; }
    pub fn extra() -> Felt { return 99; }
}
fn main() {}
"#,
        "unresolved trait method",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (extra) Associated type with generic struct override
// ═══════════════════════════════════════════════════════════════════════════

/// Associated type overridden with a generic struct type.
#[test]
#[serial]
fn at33_assoc_type_override_with_generic_struct() {
    expect_accept(
        "at33_assoc_type_override_with_generic_struct",
        r#"
pub trait HasTy { pub type Ty; }
pub struct Pair<T> { pub a: T, pub b: T }
pub struct Foo { pub v: Felt }
impl HasTy for Foo { pub type Ty = Pair<Felt>; }
fn main() {
    let x: <Foo as HasTy>::Ty = Pair::<Felt> { a: 1, b: 2 };
}
"#,
    );
}

/// Associated type overridden with a generic type parameter of the impl.
/// `impl<T> HasTy for Foo<T> { pub type Ty = T; }`
#[test]
#[serial]
fn at34_assoc_type_override_with_impl_generic() {
    expect_accept(
        "at34_assoc_type_override_with_impl_generic",
        r#"
pub trait HasTy { pub type Ty; }
pub struct Box<T> { pub v: T }
impl<T> HasTy for Box<T> {
    pub type Ty = T;
}
fn main() {
    let x: <Box<Felt> as HasTy>::Ty = 42;
}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (extra) Multiple associated types in one trait
// ═══════════════════════════════════════════════════════════════════════════

/// Trait with multiple associated types, all overridden.
#[test]
#[serial]
fn at35_multiple_assoc_types_all_overridden() {
    expect_accept(
        "at35_multiple_assoc_types_all_overridden",
        r#"
pub trait T {
    pub type A;
    pub type B;
    pub fn get_a() -> Self::A;
    pub fn get_b() -> Self::B;
}
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub type A = Felt;
    pub type B = bool;
    pub fn get_a() -> Self::A { return 42; }
    pub fn get_b() -> Self::B { return true; }
}
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (extra) Associated type used in trait method default body
// ═══════════════════════════════════════════════════════════════════════════

/// Default method body uses Self::AssocTy as a local variable type.
#[test]
#[serial]
fn at36_default_body_uses_assoc_type_as_local() {
    expect_accept(
        "at36_default_body_uses_assoc_type_as_local",
        r#"
pub trait T {
    pub type Ty;
    pub fn m() -> Felt {
        let x: Self::Ty = 0;
        return x as Felt;
    }
}
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub type Ty = Felt;
}
fn main() {
    let r = <Foo as T>::m();
}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (extra) Trait-cast with wrong trait type
// ═══════════════════════════════════════════════════════════════════════════

/// `<Foo as B>::n(f)` where Foo implements A (not B) — rejected.
///
/// FINDING (minor): Same as at09 — `visit_call` bypasses the
/// `implements_trait` check. The error is `UnresolvedMember` (method `n`
/// not found on Foo) rather than `TypeMismatch` (trait B not implemented).
/// The call is correctly rejected; the error category is misleading.
#[test]
#[serial]
fn at37_trait_cast_wrong_trait_rejected() {
    expect_reject(
        "at37_trait_cast_wrong_trait_rejected",
        r#"
pub trait A { pub fn m(self: Self) -> Felt; }
pub trait B { pub fn n(self: Self) -> Felt; }
pub struct Foo { pub v: Felt }
impl A for Foo { pub fn m(self: Self) -> Felt { return self.v; } }
fn main() {
    let f = Foo { v: 42 };
    let r = <Foo as B>::n(f);
}
"#,
        "TypeMismatch",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// CONSTANTS — declarations, visibility, expressions, type positions
// ═══════════════════════════════════════════════════════════════════════════

/// Basic const declaration with Felt type typechecks.
/// Defends: visit_const (lib.rs:2538-2569) typechecks the LHS and RHS and
/// unifies them.
#[test]
#[serial]
fn con01_basic_const_felt_typechecks() {
    expect_accept(
        "con01_basic_const_felt_typechecks",
        r#"
const C: Felt = 42;
fn main() {}
"#,
    );
}

/// Const declaration with bool type.
#[test]
#[serial]
fn con02_const_bool_typechecks() {
    expect_accept(
        "con02_const_bool_typechecks",
        r#"
const C: bool = true;
fn main() {}
"#,
    );
}

/// Const declaration with u32 type — requires a u32 literal (`42u32`), not a
/// bare integer (which defaults to Felt).
/// LIMITATION: integer literals without a suffix default to Felt. A bare `42`
/// has type Felt, so `const C: u32 = 42;` fails with TypeMismatch. You must
/// write `42u32` explicitly.
#[test]
#[serial]
fn con03_const_u32_typechecks() {
    expect_accept(
        "con03_const_u32_typechecks",
        r#"
const C: u32 = 42u32;
fn main() {}
"#,
    );
}

/// Const with type mismatch — bool value declared as Felt.
/// Defends: visit_const unifies lhs_ty and rhs_ty (lib.rs:2545).
#[test]
#[serial]
fn con04_const_type_mismatch_rejected() {
    expect_reject(
        "con04_const_type_mismatch_rejected",
        r#"
const C: Felt = true;
fn main() {}
"#,
        "TypeMismatch",
    );
}

/// Const with u32 value declared as Felt — type mismatch.
#[test]
#[serial]
fn con05_const_u32_as_felt_rejected() {
    expect_reject(
        "con05_const_u32_as_felt_rejected",
        r#"
const C: Felt = 42u32;
fn main() {}
"#,
        "TypeMismatch",
    );
}

/// LIMITATION: const declaration type is restricted to Felt, Bool, u32 only.
/// `parse_const_declaration_type` (item.rs:224-237) rejects any other type,
/// including user-defined types, Self, paths, and generics.
/// A const of a struct type should fail at parse time.
#[test]
#[serial]
fn con06_const_non_primitive_type_rejected() {
    expect_reject(
        "con06_const_non_primitive_type_rejected",
        r#"
pub struct Foo { pub v: Felt }
const C: Foo = Foo { v: 42 };
fn main() {}
"#,
        // Parse error from parse_const_declaration_type rejecting Foo
        "expected",
    );
}

/// LIMITATION: const declaration does not accept Self as the type.
/// parse_const_declaration_type (item.rs:224-237) only accepts TypeFelt,
/// TypeBool, TypeU32 — not TypeSelf. (Note: parse_const_type used in casts
/// DOES accept Self, but the const declaration path does not.)
#[test]
#[serial]
fn con07_const_self_type_rejected() {
    // Self is not valid at top level anyway, so this should error.
    // The point is that parse_const_declaration_type is more restrictive
    // than parse_const_type.
    expect_reject(
        "con07_const_self_type_rejected",
        r#"
const C: Self = 42;
fn main() {}
"#,
        "expected",
    );
}

/// Const used in expression position — `let x = C;`.
/// The const is registered as a Type::Const with name C (lib.rs:2567), and
/// resolve_path finds it via get_type_id.
#[test]
#[serial]
fn con08_const_in_expression_typechecks() {
    expect_accept(
        "con08_const_in_expression_typechecks",
        r#"
const C: Felt = 42;
fn main() {
    let x = C;
}
"#,
    );
}

/// Const used in a binary expression — `let x = C + 1;`.
#[test]
#[serial]
fn con09_const_in_binary_expression_typechecks() {
    expect_accept(
        "con09_const_in_binary_expression_typechecks",
        r#"
const C: Felt = 42;
fn main() {
    let x = C + 1;
}
"#,
    );
}

/// Const used as array size — `[Felt; N]` where N is a Felt const ident.
/// The parser converts `[Felt; N]` to Generic(Array, [Felt, N]) when N is an
/// ident (ty.rs:172-183). Sema typechecks this by unifying the array's
/// size_ty (a type variable with Felt constraint) with N's type.
#[test]
#[serial]
fn con10_const_as_array_size_typechecks() {
    expect_accept(
        "con10_const_as_array_size_typechecks",
        r#"
const N: Felt = 3;
fn main() {
    let a: [Felt; N] = [1, 2, 3];
}
"#,
    );
}

/// Const with computed expression value — `const C: Felt = 1 + 2;`.
/// visit_const evaluates the expression (lib.rs:2553).
#[test]
#[serial]
fn con11_const_computed_expression_typechecks() {
    expect_accept(
        "con11_const_computed_expression_typechecks",
        r#"
const C: Felt = 1 + 2;
fn main() {}
"#,
    );
}

/// Public const accessible cross-module via use.
#[test]
#[serial]
fn con12_public_const_cross_module_typechecks() {
    expect_accept(
        "con12_public_const_cross_module_typechecks",
        r#"
pub mod m {
    pub const C: Felt = 42;
}
use m::C;
fn main() {
    let x = C;
}
"#,
    );
}

/// Private const inaccessible cross-module.
/// Defends: resolve_use (resolver.rs:316-322) checks type visibility.
#[test]
#[serial]
fn con13_private_const_cross_module_rejected() {
    expect_reject(
        "con13_private_const_cross_module_rejected",
        r#"
pub mod m {
    const C: Felt = 42;
}
use m::C;
fn main() {}
"#,
        "type not public",
    );
}

/// Private const accessible within its own module.
#[test]
#[serial]
fn con14_private_const_within_module_typechecks() {
    expect_accept(
        "con14_private_const_within_module_typechecks",
        r#"
pub mod m {
    const C: Felt = 42;
    pub fn use_it() {
        let x = C;
    }
}
fn main() {}
"#,
    );
}

/// Const generic parameter — `fn f<N: u32>()` typechecks.
/// Defends: typecheck_generic_parameter (lib.rs:3343-3344) accepts a single
/// Felt/Bool/U32 constraint as a const generic parameter.
#[test]
#[serial]
fn con15_const_generic_param_typechecks() {
    expect_accept(
        "con15_const_generic_param_typechecks",
        r#"
fn f<N: u32>() {}
fn main() {}
"#,
    );
}

/// Const generic used in array type inside generic function.
/// `[Felt; N]` where N is a const generic param.
#[test]
#[serial]
fn con16_const_generic_in_array_typechecks() {
    expect_accept(
        "con16_const_generic_in_array_typechecks",
        r#"
fn f<N: Felt>() {
    let a: [Felt; N] = [1, 2, 3];
}
fn main() {}
"#,
    );
}

/// Const turbofish argument — `f::<3>()`.
/// Defends: populate_constant (lib.rs:3128) creates a Type::Const from a
/// ConstValue when used as a generic argument.
#[test]
#[serial]
fn con17_const_turbofish_arg_typechecks() {
    expect_accept(
        "con17_const_turbofish_arg_typechecks",
        r#"
fn f<T>() {}
fn main() {
    f::<3>();
}
"#,
    );
}

/// Const felt turbofish argument — `f::<42>()`.
#[test]
#[serial]
fn con18_felt_const_turbofish_typechecks() {
    expect_accept(
        "con18_felt_const_turbofish_typechecks",
        r#"
fn f<T>() {}
fn main() {
    f::<42>();
}
"#,
    );
}

/// Bool const turbofish argument — `f::<true>()`.
#[test]
#[serial]
fn con19_bool_const_turbofish_typechecks() {
    expect_accept(
        "con19_bool_const_turbofish_typechecks",
        r#"
fn f<T>() {}
fn main() {
    f::<true>();
}
"#,
    );
}

/// Const fn qualifier — `const fn f() -> Felt { 42 }` parses and typechecks.
/// Defends: parse_function_definition (item.rs:262-264) handles the const
/// qualifier on functions.
#[test]
#[serial]
fn con20_const_fn_qualifier_typechecks() {
    expect_accept(
        "con20_const_fn_qualifier_typechecks",
        r#"
const fn f() -> Felt {
    return 42;
}
fn main() {}
"#,
    );
}

/// LIMITATION: const declaration does not accept array types.
/// `const A: [Felt; 3] = ...` should fail at parse time because
/// parse_const_declaration_type only accepts Felt/Bool/u32.
#[test]
#[serial]
fn con21_const_array_type_rejected() {
    expect_reject(
        "con21_const_array_type_rejected",
        r#"
const A: [Felt; 3] = [1, 2, 3];
fn main() {}
"#,
        "expected",
    );
}

/// LIMITATION: const declaration does not accept generic types.
#[test]
#[serial]
fn con22_const_generic_type_rejected() {
    expect_reject(
        "con22_const_generic_type_rejected",
        r#"
pub struct Pair<T> { pub a: T, pub b: T }
const P: Pair<Felt> = Pair { a: 1, b: 2 };
fn main() {}
"#,
        "expected",
    );
}

/// Const used in cast expression — `x as u32` where the cast target is a
/// primitive type (parse_const_type accepts these).
#[test]
#[serial]
fn con23_cast_to_u32_typechecks() {
    expect_accept(
        "con23_cast_to_u32_typechecks",
        r#"
fn main() {
    let x: Felt = 42;
    let y = x as u32;
}
"#,
    );
}

/// Cast from u32 to Felt — requires u32 source value (`42u32`).
#[test]
#[serial]
fn con24_cast_u32_to_felt_typechecks() {
    expect_accept(
        "con24_cast_u32_to_felt_typechecks",
        r#"
fn main() {
    let x: u32 = 42u32;
    let y = x as Felt;
}
"#,
    );
}

/// Cast to a non-primitive type (struct) — should fail.
///
/// FINDING: `visit_cast` (lib.rs:1720-1726) returns `Error::TypeMismatch` for
/// invalid casts, NOT `Error::InvalidCast`. The `InvalidCast` variant
/// (error.rs:69-70) is defined but NEVER used — it is dead code. The cast IS
/// correctly rejected, but with the wrong error category.
#[test]
#[serial]
fn con25_cast_to_struct_rejected() {
    expect_reject(
        "con25_cast_to_struct_rejected",
        r#"
pub struct Foo { pub v: Felt }
fn main() {
    let x: Felt = 42;
    let y = x as Foo;
}
"#,
        "InvalidCast",
    );
}

/// Two consts with the same name in the same module — should fail.
/// Defends: add_type_id (lib.rs:2567) should detect duplicate type names.
#[test]
#[serial]
fn con26_duplicate_const_rejected() {
    expect_reject(
        "con26_duplicate_const_rejected",
        r#"
const C: Felt = 42;
const C: Felt = 43;
fn main() {}
"#,
        "already defined",
    );
}

/// Const with value referencing another const — `const D: Felt = C;`.
/// This may or may not work depending on whether the evaluator can resolve
/// a const-path expression at const evaluation time.
#[test]
#[serial]
fn con27_const_referencing_another_const() {
    // This is a probe — it may pass or fail. The evaluator (lib.rs:2553)
    // evaluates the expression; if it can resolve a const-path expression,
    // this succeeds.
    expect_accept(
        "con27_const_referencing_another_const",
        r#"
const C: Felt = 42;
const D: Felt = C;
fn main() {}
"#,
    );
}

/// Const used as an array literal size in a generic struct —
/// `struct S<T, N: Felt> { pub arr: [T; N] }`.
#[test]
#[serial]
fn con28_const_generic_in_struct_field_typechecks() {
    expect_accept(
        "con28_const_generic_in_struct_field_typechecks",
        r#"
pub struct S<T, N: Felt> {
    pub arr: [T; N],
}
fn main() {}
"#,
    );
}

/// Const generic constraint with non-primitive, non-trait type rejected.
/// `fn f<N: SomeStruct>()` — SomeStruct is neither a trait nor a primitive,
/// so InvalidGenericConstraint.
/// Defends: typecheck_generic_parameter (lib.rs:3343-3347).
#[test]
#[serial]
fn con29_invalid_generic_constraint_rejected() {
    expect_reject(
        "con29_invalid_generic_constraint_rejected",
        r#"
pub struct Foo { pub v: Felt }
fn f<N: Foo>() {}
fn main() {}
"#,
        "InvalidGenericConstraint",
    );
}

/// Mixed constraint (trait + primitive) rejected — a generic param cannot
/// have both a trait constraint and a primitive constraint.
/// Defends: typecheck_generic_parameter (lib.rs:3343-3344) requires either ALL
/// constraints are traits OR exactly one primitive constraint.
#[test]
#[serial]
fn con30_mixed_constraint_rejected() {
    expect_reject(
        "con30_mixed_constraint_rejected",
        r#"
pub trait T { pub fn m() -> Felt; }
fn f<N: T + u32>() {}
fn main() {}
"#,
        "InvalidGenericConstraint",
    );
}

/// Multiple primitive constraints rejected — only one primitive allowed.
#[test]
#[serial]
fn con31_multiple_primitive_constraints_rejected() {
    expect_reject(
        "con31_multiple_primitive_constraints_rejected",
        r#"
fn f<N: u32 + Felt>() {}
fn main() {}
"#,
        "InvalidGenericConstraint",
    );
}

/// Const extern fn — `const extern fn f() -> Felt;` (extern const function
/// declaration without body). This should parse as a function with both
/// const and extern qualifiers.
/// Defends: parse_function_definition (item.rs:262-267) handles const then
/// extern qualifiers in sequence.
#[test]
#[serial]
fn con32_const_extern_fn_typechecks() {
    expect_accept(
        "con32_const_extern_fn_typechecks",
        r#"
const extern fn f() -> Felt;
fn main() {}
"#,
    );
}

/// Const used in a function parameter default — PSY does not support default
/// parameter values, so this is not applicable. Instead, test a const used
/// as a standalone expression statement.
#[test]
#[serial]
fn con33_const_as_expression_statement() {
    expect_accept(
        "con33_const_as_expression_statement",
        r#"
const C: Felt = 42;
fn main() {
    C;
}
"#,
    );
}

/// Const with a felt literal that is a large number.
#[test]
#[serial]
fn con34_const_large_felt_typechecks() {
    expect_accept(
        "con34_const_large_felt_typechecks",
        r#"
const C: Felt = 999999999999;
fn main() {}
"#,
    );
}

/// Const zero value.
#[test]
#[serial]
fn con35_const_zero_value_typechecks() {
    expect_accept(
        "con35_const_zero_value_typechecks",
        r#"
const C: Felt = 0;
fn main() {}
"#,
    );
}

/// Const false value.
#[test]
#[serial]
fn con36_const_false_typechecks() {
    expect_accept(
        "con36_const_false_typechecks",
        r#"
const C: bool = false;
fn main() {}
"#,
    );
}

/// Const used in an if condition.
#[test]
#[serial]
fn con37_const_in_if_condition_typechecks() {
    expect_accept(
        "con37_const_in_if_condition_typechecks",
        r#"
const C: bool = true;
fn main() {
    if C {
        let x = 1;
    }
}
"#,
    );
}

/// Const u32 used in a for-loop range — both bounds must be u32.
/// `5u32` for the const, and `0u32` for the range start (bare `0` is Felt).
/// The for-loop check (lib.rs:2579-2580) requires both start and end to be
/// the same type (both Felt or both u32).
#[test]
#[serial]
fn con38_const_u32_in_for_range_typechecks() {
    expect_accept(
        "con38_const_u32_in_for_range_typechecks",
        r#"
const N: u32 = 5u32;
fn main() {
    for i in 0u32..N {
        let x = i;
    }
}
"#,
    );
}

/// Const Felt used in a for-loop range.
#[test]
#[serial]
fn con39_const_felt_in_for_range_typechecks() {
    expect_accept(
        "con39_const_felt_in_for_range_typechecks",
        r#"
const N: Felt = 5;
fn main() {
    for i in 0..N {
        let x = i;
    }
}
"#,
    );
}

/// LIMITATION: const declaration type does not accept path types.
/// `const C: mod::Type = ...` should fail at parse time.
#[test]
#[serial]
fn con40_const_path_type_rejected() {
    expect_reject(
        "con40_const_path_type_rejected",
        r#"
pub mod m {
    pub type T = Felt;
}
const C: m::T = 42;
fn main() {}
"#,
        "expected",
    );
}

/// LIMITATION: const declaration type does not accept tuple types.
#[test]
#[serial]
fn con41_const_tuple_type_rejected() {
    expect_reject(
        "con41_const_tuple_type_rejected",
        r#"
const C: (Felt, Felt) = (1, 2);
fn main() {}
"#,
        "expected",
    );
}

/// Const used in assert_eq — `assert_eq(C, 42, "msg")`.
#[test]
#[serial]
fn con42_const_in_assert_typechecks() {
    expect_accept(
        "con42_const_in_assert_typechecks",
        r#"
const C: Felt = 42;
fn main() {
    assert_eq(C, 42, "C should be 42");
}
"#,
    );
}

/// Const used as a struct field initializer.
#[test]
#[serial]
fn con43_const_in_struct_literal_typechecks() {
    expect_accept(
        "con43_const_in_struct_literal_typechecks",
        r#"
pub struct Foo { pub v: Felt }
const C: Felt = 42;
fn main() {
    let f = Foo { v: C };
}
"#,
    );
}

/// Const used in a function call argument.
#[test]
#[serial]
fn con44_const_in_call_arg_typechecks() {
    expect_accept(
        "con44_const_in_call_arg_typechecks",
        r#"
const C: Felt = 42;
fn identity(x: Felt) -> Felt { return x; }
fn main() {
    let x = identity(C);
}
"#,
    );
}

/// Const generic with felt constraint — `fn f<N: Felt>()`.
#[test]
#[serial]
fn con45_const_generic_felt_constraint_typechecks() {
    expect_accept(
        "con45_const_generic_felt_constraint_typechecks",
        r#"
fn f<N: Felt>() {}
fn main() {}
"#,
    );
}

/// Const generic with bool constraint — `fn f<N: bool>()`.
#[test]
#[serial]
fn con46_const_generic_bool_constraint_typechecks() {
    expect_accept(
        "con46_const_generic_bool_constraint_typechecks",
        r#"
fn f<N: bool>() {}
fn main() {}
"#,
    );
}

/// Const generic instantiation via turbofish with a u32 literal.
/// `f::<3u32>()` where `fn f<N: u32>()` — requires `3u32` (not bare `3`
/// which defaults to Felt and fails to unify with the u32 constraint).
#[test]
#[serial]
fn con47_const_generic_turbofish_u32_typechecks() {
    expect_accept(
        "con47_const_generic_turbofish_u32_typechecks",
        r#"
fn f<N: u32>() {}
fn main() {
    f::<3u32>();
}
"#,
    );
}

/// Multiple const generic parameters.
/// `fn f<N: u32, M: u32>()`.
#[test]
#[serial]
fn con48_multiple_const_generics_typechecks() {
    expect_accept(
        "con48_multiple_const_generics_typechecks",
        r#"
fn f<N: u32, M: u32>() {}
fn main() {}
"#,
    );
}

/// Mixed generic and const generic parameters.
/// `fn f<T, N: u32>()`.
#[test]
#[serial]
fn con49_mixed_generic_and_const_generic_typechecks() {
    expect_accept(
        "con49_mixed_generic_and_const_generic_typechecks",
        r#"
fn f<T, N: u32>() {}
fn main() {}
"#,
    );
}

/// Struct with both type generic and const generic parameters.
/// `struct S<T, N: Felt> { pub arr: [T; N] }` instantiated with turbofish.
#[test]
#[serial]
fn con50_struct_mixed_generics_instantiated_typechecks() {
    expect_accept(
        "con50_struct_mixed_generics_instantiated_typechecks",
        r#"
pub struct S<T, N: Felt> {
    pub arr: [T; N],
}
fn main() {
    let s = S::<Felt, 3> { arr: [1, 2, 3] };
}
"#,
    );
}

/// Const used in array type inside a struct field with a named const.
/// `const N: Felt = 3; struct S { pub arr: [Felt; N] }`.
#[test]
#[serial]
fn con51_named_const_in_struct_array_field_typechecks() {
    expect_accept(
        "con51_named_const_in_struct_array_field_typechecks",
        r#"
const N: Felt = 3;
pub struct S {
    pub arr: [Felt; N],
}
fn main() {}
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional edge cases and limitation probes
// ═══════════════════════════════════════════════════════════════════════════

/// LIMITATION: bare integer literals default to Felt, so `const C: u32 = 42;`
/// fails with TypeMismatch (42 is Felt, declared type is u32). Must use `42u32`.
/// This is by design (lexer regex `(?:0|[1-9]\d*)u32` for u32, bare integers
/// are U64 → Felt), but it's a common gotcha.
#[test]
#[serial]
fn con52_bare_int_defaults_to_felt_not_u32() {
    expect_reject(
        "con52_bare_int_defaults_to_felt_not_u32",
        r#"
const C: u32 = 42;
fn main() {}
"#,
        "TypeMismatch",
    );
}

/// Both `const extern fn` and `extern const fn` are accepted; the parser
/// accepts qualifiers in any order (item.rs loop-based parsing).
#[test]
#[serial]
fn con53_extern_before_const_typechecks() {
    expect_accept(
        "con53_extern_before_const_typechecks",
        r#"
extern const fn f() -> Felt;
fn main() {}
"#,
    );
}

/// LIMITATION: const declaration cannot use a u32 value for a Felt array size
/// even when the value fits. The array size must be Felt, and a u32 const
/// cannot be used (type mismatch on the array's size_ty unification).
#[test]
#[serial]
fn con54_u32_const_as_array_size_rejected() {
    expect_reject(
        "con54_felt_const_as_array_size_rejected",
        r#"
const N: u32 = 3u32;
fn main() {
    let a: [Felt; N] = [1, 2, 3];
}
"#,
        "TypeMismatch",
    );
}

/// Const with negative value — Felt supports negative literals.
#[test]
#[serial]
fn con55_const_negative_felt_typechecks() {
    expect_accept(
        "con55_const_negative_felt_typechecks",
        r#"
const C: Felt = -42;
fn main() {}
"#,
    );
}

/// Associated type override with an array type.
#[test]
#[serial]
fn at38_assoc_type_override_with_array_type() {
    expect_accept(
        "at38_assoc_type_override_with_array_type",
        r#"
pub trait HasTy { pub type Ty; }
pub struct Foo { pub v: Felt }
impl HasTy for Foo { pub type Ty = [Felt; 3]; }
fn main() {
    let x: <Foo as HasTy>::Ty = [1, 2, 3];
}
"#,
    );
}

/// Associated type override with a tuple type.
#[test]
#[serial]
fn at39_assoc_type_override_with_tuple_type() {
    expect_accept(
        "at39_assoc_type_override_with_tuple_type",
        r#"
pub trait HasTy { pub type Ty; }
pub struct Foo { pub v: Felt }
impl HasTy for Foo { pub type Ty = (Felt, bool); }
fn main() {
    let x: <Foo as HasTy>::Ty = (42, true);
}
"#,
    );
}

/// Trait with associated type used in a method signature cross-referencing
/// another associated type — `fn convert(x: Self::A) -> Self::B`.
#[test]
#[serial]
fn at40_cross_assoc_type_in_method_sig() {
    expect_accept(
        "at40_cross_assoc_type_in_method_sig",
        r#"
pub trait T {
    pub type A;
    pub type B;
    pub fn convert(x: Self::A) -> Self::B;
}
pub struct Foo { pub v: Felt }
impl T for Foo {
    pub type A = Felt;
    pub type B = bool;
    pub fn convert(x: Self::A) -> Self::B { return x > 0; }
}
fn main() {}
"#,
    );
}
