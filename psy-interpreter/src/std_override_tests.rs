// The rewriter (psy-sema/src/rewriter.rs) only walks a function body when the
// function is generic and gets instantiated. Every raw `__ctx_*`/`__storage_*`
// intrinsic lives inside psy-std wrapper bodies, which are not generic — so
// their rewriter arms can never fire from user code. These tests override the
// std `context.psy` through the per-program FileResolver (pre-registered files
// win over disk reads) with copies that add *generic* probes around the same
// raw intrinsics. Instantiating a probe from `main` rewrites its body and
// exercises every intrinsic arm, while the std-ancestry gate still passes
// because the probe really is part of the std module.

use serial_test::serial;

use super::*;

const GENERIC_CONTEXT_PROBES: &str = r#"

// --- Generic raw-intrinsic probes (test-only additions to std context) ---

pub fn probe_raw_context_getters<T>(v: T) -> T {
    let user_id: Felt = __ctx_get_user_id();
    let contract_id: Felt = __ctx_get_contract_id();
    let deployer: Hash = __ctx_get_contract_deployer(contract_id);
    let height: Felt = __ctx_get_contract_state_tree_height(contract_id);
    let caller: Felt = __ctx_get_caller_contract_id();
    let checkpoint: Felt = __ctx_get_checkpoint_id();
    let nonce: Felt = __ctx_get_last_nonce();
    let pkh: Hash = __ctx_get_user_public_key_hash();
    let session: Hash = __ctx_get_session_proof_tree_root();
    let state: Hash = __ctx_get_state_hash_at(0);
    let other_contract: Hash = __ctx_get_other_contract_state_hash_at(height, contract_id, 0);
    let other_user_contract: Hash = __ctx_get_other_user_contract_state_hash_at(height, user_id, contract_id, 0);
    let updated: Hash = __ctx_set_state_hash_at(0, state);
    let contains_other: bool = __imt_contains_other_user(height, user_id, contract_id, pkh, 0, 4);
    let verified: bool = __secp256k1_verify([0u32; 16], pkh, [0u32; 16]);
    __emit(v);
    return v;
}

pub fn probe_raw_checkpoint_stats<T>(v: T) -> T {
    let checkpoint: Felt = __ctx_get_checkpoint_id();
    __ctx_get_checkpoint_stats(checkpoint);
    let register_users: Hash = __ctx_get_register_users_root(checkpoint);
    let gutas: Hash = __ctx_get_gutas_root(checkpoint);
    let user_tree: Hash = __ctx_get_checkpoint_user_tree_root(checkpoint);
    let contract_tree: Hash = __ctx_get_checkpoint_contract_tree_root(checkpoint);
    let deposit_tree: Hash = __ctx_get_checkpoint_deposit_tree_root(checkpoint);
    let withdrawal_tree: Hash = __ctx_get_checkpoint_withdrawal_tree_root(checkpoint);
    let registration: Hash = __ctx_get_checkpoint_user_registration_tree_root(checkpoint);
    let deploys: Hash = __ctx_get_deploy_contracts_root(checkpoint);
    let guta_fees: Felt = __ctx_get_guta_fees_collected(checkpoint);
    let da_fees: Felt = __ctx_get_da_fees_collected(checkpoint);
    let user_ops: Felt = __ctx_get_user_ops_processed(checkpoint);
    let total_txs: Felt = __ctx_get_total_transactions(checkpoint);
    let slots: Felt = __ctx_get_slots_modified(checkpoint);
    let deploys_done: Felt = __ctx_get_deploy_contracts_completed(checkpoint);
    let registrations_done: Felt = __ctx_get_register_users_completed(checkpoint);
    let gutas_done: Felt = __ctx_get_gutas_completed(checkpoint);
    return v;
}

pub fn probe_raw_storage_roundtrip<T>(v: T) -> T {
    let height: Felt = __ctx_get_contract_state_tree_height(0);
    let user_id: Felt = __ctx_get_user_id();
    let contract_id: Felt = __ctx_get_contract_id();
    let read: Felt = __storage_read(height, user_id, contract_id, 0);
    __storage_write(0, read);
    __ctx_clear_entire_tree();
    return v;
}

pub fn probe_rewrite_shapes<T>(v: T) -> T {
    assert_eq(1, 1);
    let pair = (1, 2);
    let first: Felt = pair.0;
    let repeated: [Felt; 2] = [3; 2];
    let branch: Felt = if first > 0 {
        1
    }
    else if first > 1 {
        2
    }
    else {
        3
    };
    let double = |x: Felt| -> Felt {
        return x + x;
    };
    let applied: Felt = double(first) + repeated[0 as Felt] + branch;
    return v;
}

pub fn probe_rewrite_void<T>(v: T) {
    assert(1 > 0);
    return;
}
"#;

/// Resolve the std root the parser will actually use: `DARGO_STD_PATH` wins
/// when set (toolchain installs point elsewhere), otherwise the checked-in
/// `psy-std/` next to this workspace. The override must land on the exact
/// `context.psy` the parser resolves for `mod context;` in the std prelude
/// (FileResolver keys are canonicalized).
pub(crate) fn std_context_path() -> PathBuf {
    if let Ok(std_path) = std::env::var("DARGO_STD_PATH") {
        if let Some(parent) = PathBuf::from(std_path).parent() {
            return parent.join("context.psy");
        }
    }
    let mut dir = Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    while let Some(current) = dir {
        let candidate = current.join("psy-std").join("context.psy");
        if candidate.exists() {
            return candidate;
        }
        dir = current.parent().map(std::path::Path::to_path_buf);
    }
    panic!("psy-std/context.psy not found above {}", env!("CARGO_MANIFEST_DIR"));
}

/// Register a copy of the std context module plus the generic probes. Only
/// this program instance sees the override; other tests are unaffected.
fn register_std_context_override(program: &Program<SymFeltRef>) {
    let original = std::fs::read_to_string(std_context_path()).expect("read psy-std/context.psy");
    let overridden: Arc<str> = Arc::from(format!("{original}\n{GENERIC_CONTEXT_PROBES}"));
    program.file_resolver.add_file(std_context_path(), overridden);
}

fn typecheck_with_std_override(source: &str) -> anyhow::Result<()> {
    with_primitive_scope_reset(|| {
        let path = PathBuf::from("/virtual/src/main.psy");
        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
        let program = Program::new();
        program.file_resolver.add_file(path.clone(), Arc::from(source));
        register_std_context_override(&program);
        let mut graph = Graph::new();
        graph.add_node(path);
        interpreter.typecheck_with_program(graph, program).map(|_| ())
    })
}

#[test]
#[serial]
fn generic_std_probes_with_raw_intrinsics_typecheck_and_instantiate() {
    let source = r#"
        use std::prelude::*;

        fn main(q: Felt) -> Felt {
            let a: Felt = probe_raw_context_getters(q);
            let b: Felt = probe_raw_checkpoint_stats(q);
            let c: Felt = probe_raw_storage_roundtrip(q);
            let d: Felt = probe_rewrite_shapes(q);
            probe_rewrite_void(q);
            return a + b + c + d;
        }
    "#;
    // A pass here means the raw-intrinsic arms of visit_intrinsic_expr and of
    // the rewriter ran during checking and instantiation of the probe bodies.
    typecheck_with_std_override(source).expect("std override fixture must typecheck");
}

/// Typecheck a program whose std context additionally defines one generic
/// `bad` probe (called from main), expecting failure. Returns the lowered
/// error message.
fn typecheck_override_fails(bad_probe_body: &str) -> String {
    let extra = format!("pub fn bad<T>(v: T) -> T {{\n    {bad_probe_body}\n    return v;\n}}\n");
    let source = r#"
        use std::prelude::*;

        fn main(q: Felt) -> Felt {
            let r: Felt = bad(q);
            return r;
        }
    "#;
    with_primitive_scope_reset(|| {
        let path = PathBuf::from("/virtual/src/main.psy");
        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
        let program = Program::new();
        program.file_resolver.add_file(path.clone(), Arc::from(source));
        let original = std::fs::read_to_string(std_context_path()).expect("read psy-std/context.psy");
        program
            .file_resolver
            .add_file(std_context_path(), Arc::from(format!("{original}\n{extra}")));
        let mut graph = Graph::new();
        graph.add_node(path);
        match interpreter.typecheck_with_program(graph, program) {
            Ok(_) => anyhow::bail!("bad intrinsic call must be rejected: {bad_probe_body}"),
            Err(error) => Ok(format!("{error:#}")),
        }
    })
    .expect("primitive scope reset")
}

#[test]
#[serial]
fn raw_std_intrinsics_reject_mismatched_argument_types() {
    let bad_calls: &[(&str, &str)] = &[
        ("let x: Hash = __ctx_get_contract_deployer(true);", "deployer contract id must be Felt"),
        ("let x: Felt = __ctx_get_contract_state_tree_height(true);", "tree height contract id must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_get(0, 0, 4);", "imt_get key must be Hash"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_get(h, true, 4);", "imt_get base offset must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_get(h, 0, true);", "imt_get capacity must be Felt"),
        (
            "let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_get_other_user(true, 0, 0, h, 0, 4);",
            "imt_get_other_user height must be Felt",
        ),
        ("let x: Hash = __ctx_get_other_contract_state_hash_at(true, 0, 0);", "other contract height must be Felt"),
        (
            "let x: Hash = __ctx_get_other_user_contract_state_hash_at(0, true, 0, 0);",
            "other user contract user id must be Felt",
        ),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __ctx_set_state_hash_at(0, 0);", "set_state_hash_at value must be Hash"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_set(0, h, 0, 4);", "imt_set key must be Hash"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_set(h, 0, 0, 4);", "imt_set value must be Hash"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_set(h, h, true, 4);", "imt_set offset must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: bool = __imt_contains_other_user(true, 0, 0, h, 0, 4);", "imt_contains_other_user height must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: bool = __imt_contains(0, 0, 4);", "imt_contains key must be Hash"),
        ("let x: Felt = __storage_read(true, 0, 0, 0);", "storage_read height must be Felt"),
        ("let x: Felt = __storage_read(0, 0, 0, true);", "storage_read offset must be Felt"),
        ("let x: Felt = __storage_read_range(true, 0, 0, 0, 1);", "storage_read_range height must be Felt"),
        ("__storage_write(true, 0);", "storage_write offset must be Felt"),
        ("__storage_write(0, true);", "storage_write value must be Felt"),
        ("__storage_write_range(true, 0);", "storage_write_range offset must be Felt"),
        ("let h: Hash = __ctx_get_state_hash_at(0); let x: Hash = __ctx_get_state_hash_at(true);", "state hash slot must be Felt"),
        // Every remaining parameter position of the multi-argument raw
        // intrinsics: one bad argument per arm keeps each TypeMismatch branch
        // (including short-circuit order) exercised independently.
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_get_other_user(0, true, 0, h, 0, 4);", "imt_get_other_user user id must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_get_other_user(0, 0, true, h, 0, 4);", "imt_get_other_user contract id must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_get_other_user(0, 0, 0, 0, 0, 4);", "imt_get_other_user key must be Hash"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_get_other_user(0, 0, 0, h, true, 4);", "imt_get_other_user base offset must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_get_other_user(0, 0, 0, h, 0, true);", "imt_get_other_user capacity must be Felt"),
        ("let x: Hash = __ctx_get_other_contract_state_hash_at(0, true, 0);", "other contract contract id must be Felt"),
        ("let x: Hash = __ctx_get_other_contract_state_hash_at(0, 0, true);", "other contract slot must be Felt"),
        ("let x: Hash = __ctx_get_other_user_contract_state_hash_at(true, 0, 0, 0);", "other user contract height must be Felt"),
        ("let x: Hash = __ctx_get_other_user_contract_state_hash_at(0, 0, true, 0);", "other user contract contract id must be Felt"),
        ("let x: Hash = __ctx_get_other_user_contract_state_hash_at(0, 0, 0, true);", "other user contract slot must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __ctx_set_state_hash_at(true, h);", "set_state_hash_at slot must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: Hash = __imt_set(h, h, 0, true);", "imt_set capacity must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: bool = __imt_contains_other_user(0, true, 0, h, 0, 4);", "imt_contains_other_user user id must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: bool = __imt_contains_other_user(0, 0, true, h, 0, 4);", "imt_contains_other_user contract id must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: bool = __imt_contains_other_user(0, 0, 0, 0, 0, 4);", "imt_contains_other_user key must be Hash"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: bool = __imt_contains_other_user(0, 0, 0, h, true, 4);", "imt_contains_other_user base offset must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: bool = __imt_contains(h, true, 4);", "imt_contains base offset must be Felt"),
        ("let h: Hash = __ctx_get_user_public_key_hash(); let x: bool = __imt_contains(h, 0, true);", "imt_contains capacity must be Felt"),
        ("let x: Felt = __storage_read(0, true, 0, 0);", "storage_read user id must be Felt"),
        ("let x: Felt = __storage_read(0, 0, true, 0);", "storage_read contract id must be Felt"),
        ("let x: Felt = __storage_read_range(0, true, 0, 0, 1);", "storage_read_range user id must be Felt"),
        ("let x: Felt = __storage_read_range(0, 0, true, 0, 1);", "storage_read_range contract id must be Felt"),
        ("let x: Felt = __storage_read_range(0, 0, 0, true, 1);", "storage_read_range offset must be Felt"),
        ("let x: Felt = __storage_read_range(0, 0, 0, 0, true);", "storage_read_range length must be Felt"),
        ("let n: Felt = __ctx_get_last_nonce(); let b = __split_bits(13, n);", "split_bits length must be a compile-time constant"),
    ];
    for (call, label) in bad_calls {
        let message = typecheck_override_fails(call);
        assert!(
            message.contains("TypeMismatch") || message.contains("type mismatch"),
            "{label}: unexpected rejection message: {message}"
        );
    }
}

#[test]
#[serial]
fn instantiated_probe_bodies_appear_in_the_checked_program() {
    let source = r#"
        use std::prelude::*;

        fn main(q: Felt) -> Felt {
            let c: Felt = probe_raw_storage_roundtrip(q);
            return c;
        }
    "#;
    let (typechecker, mut ctx) = with_primitive_scope_reset(|| {
        let path = PathBuf::from("/virtual/src/main.psy");
        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
        let program = Program::new();
        program.file_resolver.add_file(path.clone(), Arc::from(source));
        register_std_context_override(&program);
        let mut graph = Graph::new();
        graph.add_node(path);
        interpreter.typecheck_with_program(graph, program)
    })
    .expect("std override fixture must typecheck");

    let name_id = ctx.program.interner.intern_ident("probe_raw_storage_roundtrip");
    let instances = typechecker
        .program
        .defs
        .iter()
        .filter(|def| matches!(def, CheckedDefinitionNode::Function(node) if node.name.id == name_id))
        .count();
    assert!(instances >= 2, "expected an instantiated probe copy, found {instances}");
}
