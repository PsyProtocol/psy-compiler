// Module-visibility test matrix (QA-Visibility).
//
// Drives the real parser + sema through `Interpreter::typecheck_single` with
// temporary `.psy` sources. Every case names the concrete visibility contract it
// defends and asserts the exact accept/reject outcome plus error category.
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

/// Typecheck `source` written to a throwaway temp file, then tear it down. Returns `None` if the
/// program typechecked, or `Some(formatted_error)` if it was rejected. Cleanup
/// runs before the caller panics, so a failing assertion can never leak global
/// state into the next test.
fn check(source: &str) -> Option<String> {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_vis_{unique}_{n}.psy"));
    fs::write(&path, source).unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = interpreter.typecheck_single(path.clone());

    // Tear down first, unconditionally.
    let _ = fs::remove_file(path);

    match result {
        Ok(_) => None,
        Err(e) => Some(format!("{e:#}")),
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

/// Assert the source typechecks cleanly.
fn expect_accept(name: &str, source: &str) {
    match check(source) {
        None => {}
        Some(msg) => panic!("[{name}] expected acceptance, but got error:\n{msg}"),
    }
}

// ---- (1) private function inaccessible from outside its module ----
// `inner` is a child of the root; the root is *outside* `inner`, so a private
// function is unreachable. `inner::private_fn()` has no intermediate type
// segments, so it flows through `resolve_module_type` -> `TypeNotPublic`
// (psy-sema/src/resolver.rs:446), not `MemberNotPublic`.
#[test]
fn private_fn_inaccessible_from_parent() {
    expect_reject(
        "private_fn_inaccessible_from_parent",
        r#"
mod inner {
    fn private_fn() {}
}

fn main() {
    inner::private_fn();
}
"#,
        "type not public",
    );
}

// ---- (1b) private function is reachable from inside its own module ----
#[test]
fn private_fn_accessible_within_own_module() {
    expect_accept(
        "private_fn_accessible_within_own_module",
        r#"
mod inner {
    fn private_fn() {}

    pub fn caller() {
        private_fn();
    }
}

fn main() {}
"#,
    );
}

// ---- (2) private struct inaccessible cross-module ----
// `inner::PrivateS { ... }` is a type-position path with no intermediate segments,
// so it goes through `resolve_module_type` -> `TypeNotPublic` (resolver.rs:446).
#[test]
fn private_struct_inaccessible_cross_module() {
    expect_reject(
        "private_struct_inaccessible_cross_module",
        r#"
mod inner {
    struct PrivateS {
        x: Felt,
    }
}

fn main() {
    let s = inner::PrivateS { x: 0 };
}
"#,
        "type not public",
    );
}

// ---- (3) private inline module inaccessible from a non-descendant ----
// `outer` is a private sibling of `far`; `far` may reference `outer` itself
// (sibling rule, symbol_table.rs:301) but cannot descend into `outer::inner`
// because `inner` is not visible to `far` -> `ModuleNotPublic` (resolver.rs:199).
// `access` is `pub` so `main` can call it; the reject comes from typechecking
// `access`'s body.
#[test]
fn private_inline_module_inaccessible_from_nondescendant() {
    expect_reject(
        "private_inline_module_inaccessible_from_nondescendant",
        r#"
mod outer {
    mod inner {
        pub fn f() {}
    }
}

mod far {
    pub fn access() {
        outer::inner::f();
    }
}

fn main() {
    far::access();
}
"#,
        "module not public",
    );
}

// ---- (3b) a sibling CAN reach a public item in a private sibling module ----
// Documents the sibling visibility rule (symbol_table.rs:301): `outer` is
// private yet visible to its sibling `far`, so a *public* item inside `outer` is
// reachable from `far`. This is looser than Rust and worth flagging.
#[test]
fn sibling_can_access_public_item_in_private_sibling_module() {
    expect_accept(
        "sibling_can_access_public_item_in_private_sibling_module",
        r#"
mod outer {
    pub fn f() {}
}

mod far {
    pub fn access() {
        outer::f();
    }
}

fn main() {
    far::access();
}
"#,
    );
}

// ---- (4) `use` of a private function is rejected ----
// resolve_use checks the target's type-key visibility -> `TypeNotPublic`
// (resolver.rs:317).
#[test]
fn use_of_private_fn_rejected() {
    expect_reject(
        "use_of_private_fn_rejected",
        r#"
mod inner {
    fn private_fn() {}
}

use inner::private_fn;

fn main() {}
"#,
        "type not public",
    );
}

// ---- (4b) `use` of a private struct is rejected ----
#[test]
fn use_of_private_struct_rejected() {
    expect_reject(
        "use_of_private_struct_rejected",
        r#"
mod inner {
    struct PrivateS {
        x: Felt,
    }
}

use inner::PrivateS;

fn main() {}
"#,
        "type not public",
    );
}

// ---- (4c) `use` of a private module is rejected ----
// traverse_path_segment -> is_module_visible fails -> `ModuleNotPublic`
// (resolver.rs:359).
#[test]
fn use_of_private_module_rejected() {
    expect_reject(
        "use_of_private_module_rejected",
        r#"
mod outer {
    mod inner {}
}

use outer::inner;

fn main() {}
"#,
        "module not public",
    );
}

// ---- (5) pub item accessible cross-module (positive control) ----
#[test]
fn pub_item_accessible_cross_module() {
    expect_accept(
        "pub_item_accessible_cross_module",
        r#"
mod inner {
    pub fn f() {}

    pub struct S {
        pub x: Felt,
    }
}

use inner::*;

fn main() {
    inner::f();
    let s = inner::S { x: 0 };
}
"#,
    );
}

// ---- (6) implicit `use std::prelude::*` does not bypass user visibility ----
// The prelude is auto-injected (psy-parser/src/lib.rs:167-175) as a private glob;
// `Felt` resolves via the prelude while the private user function stays
// unreachable (`TypeNotPublic`, resolver.rs:446).
#[test]
fn implicit_prelude_does_not_bypass_user_visibility() {
    expect_reject(
        "implicit_prelude_does_not_bypass_user_visibility",
        r#"
mod inner {
    fn private_fn() {}
}

fn main() {
    let v: Felt = 0;
    inner::private_fn();
}
"#,
        "type not public",
    );
}

// ---- (7) nested module: child cannot access parent's private items ----
// `super::parent_priv` from a child resolves the parent module but the private
// function is rejected by `resolve_module_type` -> `TypeNotPublic` (resolver.rs:446).
// There is no descendant exception for items.
#[test]
fn child_cannot_access_parent_private_fn() {
    expect_reject(
        "child_cannot_access_parent_private_fn",
        r#"
fn parent_priv() {}

mod child {
    fn access() {
        super::parent_priv();
    }
}

fn main() {}
"#,
        "type not public",
    );
}

// ---- (8) struct field visibility: private field inaccessible from outside ----
// Member access `s.y` enforces field visibility -> `MemberNotPublic`
// (psy-sema/src/lib.rs:225), with the same-module escape handled by
// `typecheck_member_access`.
#[test]
fn private_struct_field_inaccessible_from_outside() {
    expect_reject(
        "private_struct_field_inaccessible_from_outside",
        r#"
mod inner {
    pub struct S {
        pub x: Felt,
        y: Felt,
    }

    pub fn make() -> S {
        return S { x: 0, y: 0 };
    }
}

use inner::*;

fn main() {
    let s = make();
    let v = s.y;
}
"#,
        "member not public",
    );
}

// ---- (8b) public struct field is accessible from outside ----
#[test]
fn public_struct_field_accessible_from_outside() {
    expect_accept(
        "public_struct_field_accessible_from_outside",
        r#"
mod inner {
    pub struct S {
        pub x: Felt,
        y: Felt,
    }

    pub fn make() -> S {
        return S { x: 0, y: 0 };
    }
}

use inner::*;

fn main() {
    let s = make();
    let v: Felt = s.x;
}
"#,
    );
}

// ---- (8c) enum visibility: `use` of a private enum is rejected (but see 8d) ----
// A private enum imported via `use` is rejected with `ModuleNotPublic` ("E not a
// public module") rather than `TypeNotPublic`, because the enum is not found in
// the module's type table at resolver.rs:316 and falls through to the module
// branch (resolver.rs:326-333). The outcome is correct (private stays private)
// but, per (8d), this is the SAME import bug -- not a genuine visibility check.
#[test]
fn use_of_private_enum_rejected() {
    expect_reject(
        "use_of_private_enum_rejected",
        r#"
mod inner {
    enum E {
        A,
    }
}

use inner::E;

fn main() {}
"#,
        "module not public",
    );
}

// ---- (8d) BUG: enums are not importable via `use` regardless of visibility ----
// A PUBLIC enum imported via `use` is ALSO rejected with `ModuleNotPublic`
// ("E not a public module"). resolve_use's type-table lookup (resolver.rs:316)
// never finds enums, so it falls through to the module branch (resolver.rs:326)
// and reports the enum as a non-public module. This means:
//   * the private-enum rejection (8c) is NOT a visibility enforcement -- it is
//     this same import bug surfacing identically for private and public enums;
//   * enum visibility via `use` is effectively untested/unenforced today.
// This test pins the current (defective) behavior so the bug is visible.
#[test]
fn use_of_public_enum_also_rejected_bug() {
    expect_reject(
        "use_of_public_enum_also_rejected_bug",
        r#"
mod inner {
    pub enum E {
        A,
    }
}

use inner::E;

fn main() {}
"#,
        "module not public",
    );
}

// ---- (9) trait impl visibility: a private type's trait impl cannot be leveraged
// publicly. The private struct itself cannot be imported via `use` ->
// `TypeNotPublic` (resolver.rs:317), so the (public) trait method is never
// reachable. ----
#[test]
fn private_type_trait_impl_not_importable_publicly() {
    expect_reject(
        "private_type_trait_impl_not_importable_publicly",
        r#"
pub trait Greet {
    pub fn hello(self: Self) -> Felt;
}

mod inner {
    use super::Greet;

    struct S {
        pub v: Felt,
    }

    impl Greet for S {
        pub fn hello(self: Self) -> Felt {
            return self.v;
        }
    }
}

use inner::S;

fn main() {}
"#,
        "type not public",
    );
}

// ---- (9b) a PUBLIC type's trait impl IS usable across modules ----
#[test]
fn public_type_trait_impl_usable_across_modules() {
    expect_accept(
        "public_type_trait_impl_usable_across_modules",
        r#"
pub trait Greet {
    pub fn hello(self: Self) -> Felt;
}

mod inner {
    use super::Greet;

    pub struct S {
        pub v: Felt,
    }

    impl Greet for S {
        pub fn hello(self: Self) -> Felt {
            return self.v;
        }
    }
}

use inner::S;
use self::Greet;

fn main() {
    let s = S { v: 1 };
    let v: Felt = s.hello();
}
"#,
    );
}

// ---- (10) extern fn visibility: a private extern fn is inaccessible ----
// `extern fn` is a bodyless declaration (item.rs:265,288); a private one is
// rejected like a private function via `resolve_module_type` -> `TypeNotPublic`
// (resolver.rs:446).
#[test]
fn private_extern_fn_inaccessible_cross_module() {
    expect_reject(
        "private_extern_fn_inaccessible_cross_module",
        r#"
mod inner {
    extern fn ext() -> Felt;
}

fn main() {
    let v = inner::ext();
}
"#,
        "type not public",
    );
}

// ---- (10b) public extern fn is accessible cross-module ----
#[test]
fn public_extern_fn_accessible_cross_module() {
    expect_accept(
        "public_extern_fn_accessible_cross_module",
        r#"
mod inner {
    pub extern fn ext() -> Felt;
}

fn main() {
    let v = inner::ext();
}
"#,
    );
}

// ===== `use`-statement visibility (focused) =====

// ---- (U1) wildcard `use mod::*` imports only public items ----
// resolve_use with target=None filters to public keys/types (resolver.rs:338).
#[test]
fn wildcard_use_imports_only_public_items() {
    expect_accept(
        "wildcard_use_imports_only_public_items",
        r#"
mod inner {
    fn private_fn() {}
    pub fn public_fn() {}
}

use inner::*;

fn main() {
    public_fn();
}
"#,
    );
}

// ---- (U2) wildcard `use mod::*` does NOT import private items ----
// `private_fn` is filtered out by the public-only glob, so referencing it
// afterwards is `UnresolvedType` (resolver.rs:151).
#[test]
fn wildcard_use_does_not_import_private_items() {
    expect_reject(
        "wildcard_use_does_not_import_private_items",
        r#"
mod inner {
    fn private_fn() {}
    pub fn public_fn() {}
}

use inner::*;

fn main() {
    private_fn();
}
"#,
        "unresolved type",
    );
}

// ---- (U3) re-export via `pub use` makes an item publicly available ----
// `pub use a::f` re-inserts `f` into `c` with public visibility (lib.rs:3361), so
// `use c::f` from the root succeeds.
#[test]
fn pub_use_reexport_makes_item_public() {
    expect_accept(
        "pub_use_reexport_makes_item_public",
        r#"
mod a {
    pub fn f() {}
}

mod c {
    pub use super::a::f;
}

use c::f;

fn main() {
    f();
}
"#,
    );
}

// ---- (U4) private `use` re-export is NOT publicly available to others ----
// A non-`pub` `use a::f` re-inserts `f` into `c` as private (lib.rs:3361), so
// `use c::f` from the root is rejected -> `TypeNotPublic` (resolver.rs:317).
#[test]
fn private_use_reexport_not_public_to_others() {
    expect_reject(
        "private_use_reexport_not_public_to_others",
        r#"
mod a {
    pub fn f() {}
}

mod c {
    use super::a::f;
}

use c::f;

fn main() {
    f();
}
"#,
        "type not public",
    );
}

// ---- (U5) transitive use chain enforces visibility at each hop ----
// `a::b::f` where `b` is a PRIVATE module: even though `a` is public, `b` is not
// visible to the root, so `use a::b::f` is rejected at the `b` segment via
// traverse_path_segment -> `ModuleNotPublic` (resolver.rs:359).
#[test]
fn transitive_use_enforces_visibility_each_hop() {
    expect_reject(
        "transitive_use_enforces_visibility_each_hop",
        r#"
pub mod a {
    mod b {
        pub fn f() {}
    }
}

use a::b::f;

fn main() {
    f();
}
"#,
        "module not public",
    );
}

// ---- (U5b) transitive use through all-public modules succeeds ----
#[test]
fn transitive_use_all_public_succeeds() {
    expect_accept(
        "transitive_use_all_public_succeeds",
        r#"
pub mod a {
    pub mod b {
        pub fn f() {}
    }
}

use a::b::f;

fn main() {
    f();
}
"#,
    );
}

// ---- (U6) the implicit prelude glob does not elevate user-private items ----
// `use inner::private_fn` is still rejected even though `use std::prelude::*` is
// implicitly present in this module (psy-parser/src/lib.rs:172).
#[test]
fn prelude_glob_does_not_shadow_user_visibility() {
    expect_reject(
        "prelude_glob_does_not_shadow_user_visibility",
        r#"
mod inner {
    fn private_fn() {}
}

use inner::private_fn;

fn main() {}
"#,
        "type not public",
    );
}