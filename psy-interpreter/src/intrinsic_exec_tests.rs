// Execution-level coverage for the std::context / std::storage intrinsic
// wrappers. Typechecking alone never enters `interpret_intrinsic`; these
// tests run `main` through the interpreter so each CheckedIntrinsicExprNode
// arm of the runtime dispatch actually executes against QExecContext.

use serial_test::serial;

use super::*;

fn compile_stub(
    _context: &QExecContext,
    (name, method_id, outputs): (String, u32, Vec<SymFeltRef>),
) -> DPNFunctionCircuitDefinition {
    DPNFunctionCircuitDefinition {
        name,
        method_id,
        circuit_inputs: vec![],
        circuit_outputs: outputs.iter().map(|felt| felt.get_constant_value()).collect(),
        state_commands: vec![],
        state_command_resolution_indices: vec![],
        assertions: vec![],
        definitions: vec![],
        events: vec![],
    }
}

fn reset_primitive_scope() {
    #[allow(static_mut_refs)]
    unsafe {
        let _ = STD_PRIMITIVE_SCOPE_ID.take();
    }
}

fn exec_source(label: &str, source: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("{label}_intrinsics.psy"));
    std::fs::write(&path, source).unwrap();
    let path_arg = path.clone();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = {
        let (mut typechecker, mut ctx) = interpreter
            .typecheck_single(path_arg)
            .map_err(|e| format!("typecheck: {e:#}"))?;
        interpreter
            .interpret(&mut typechecker, &mut ctx, None::<Ident>, vec![], compile_stub)
            .map(|_| ())
            .map_err(|e| format!("interpret: {e:#}"))
    };
    let _ = std::fs::remove_file(&path);
    reset_primitive_scope();
    result
}

fn expect_intrinsics_exec(label: &str, source: &str) {
    if let Err(message) = exec_source(label, source) {
        panic!("[{label}] expected execution, got:\n{message}");
    }
}

fn expect_intrinsics_failure(label: &str, source: &str, needle: &str) {
    match exec_source(label, source) {
        Ok(()) => panic!("[{label}] expected failure mentioning `{needle}`, got success"),
        Err(message) => assert!(
            message.to_lowercase().contains(&needle.to_lowercase()),
            "[{label}] expected failure mentioning `{needle}`, got:\n{message}"
        ),
    }
}

/// Typecheck + interpret a virtual source whose std context module has
/// `extra_std` appended. Non-generic std-side wrappers added this way let
/// `interpret` reach raw-intrinsic runtime arms (`__ctx_get_checkpoint_stats`,
/// `__storage_write_range`) that no checked-in std wrapper exposes.
fn exec_override_source(label: &str, source: &str, extra_std: &str) -> Result<(), String> {
    let path = PathBuf::from(format!("/virtual/{label}_main.psy"));
    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = {
        let program = Program::new();
        program.file_resolver.add_file(path.clone(), Arc::from(source));
        let std_path = crate::std_override_tests::std_context_path();
        let original = std::fs::read_to_string(&std_path).map_err(|e| format!("read std: {e}"))?;
        program
            .file_resolver
            .add_file(std_path, Arc::from(format!("{original}\n{extra_std}")));
        let mut graph = Graph::new();
        graph.add_node(path);
        let (mut typechecker, mut ctx) = interpreter
            .typecheck_with_program(graph, program)
            .map_err(|e| format!("typecheck: {e:#}"))?;
        interpreter
            .interpret(&mut typechecker, &mut ctx, None::<Ident>, vec![], compile_stub)
            .map(|_| ())
            .map_err(|e| format!("interpret: {e:#}"))
    };
    reset_primitive_scope();
    result
}

#[test]
#[serial]
fn raw_checkpoint_stats_and_storage_write_range_execute() {
    let extra_std = r#"
pub fn probe_raw_stats_exec(checkpoint_id: Felt) {
    __ctx_get_checkpoint_stats(checkpoint_id);
    return;
}

pub fn probe_raw_write_range_exec(offset: Felt, values: [Felt; 3]) {
    __storage_write_range(offset, values);
    return;
}
"#;
    let source = r#"
        use std::prelude::*;

        fn main() -> Felt {
            let cp: Felt = get_checkpoint_id();
            probe_raw_stats_exec(cp);
            probe_raw_write_range_exec(0, [1, 2, 3]);
            return 0;
        }
    "#;
    if let Err(message) = exec_override_source("raw_stats", source, extra_std) {
        panic!("[raw_stats] expected execution, got:\n{message}");
    }
}

#[test]
#[serial]
fn context_identity_getters_execute() {
    expect_intrinsics_exec(
        "ctx_identity",
        r#"
        use std::prelude::*;

        fn main() -> Felt {
            let user: Felt = get_user_id();
            let contract: Felt = get_contract_id();
            let caller: Felt = get_caller_contract_id();
            let checkpoint: Felt = get_checkpoint_id();
            let nonce: Felt = get_last_nonce();
            let deployer: Hash = get_contract_deployer(contract);
            let height: Felt = get_contract_state_tree_height(contract);
            let pkh: Hash = get_user_public_key_hash();
            let session: Hash = get_session_proof_tree_root();
            let state: Hash = get_state_hash_at(0);
            let updated: Hash = cset_state_hash_at(0, state);
            return user + contract + caller + checkpoint + nonce + height;
        }
        "#,
    );
}

#[test]
#[serial]
fn imt_intrinsics_execute() {
    expect_intrinsics_exec(
        "imt_ops",
        r#"
        use std::prelude::*;

        fn main() -> Felt {
            let user: Felt = get_user_id();
            let contract: Felt = get_contract_id();
            let key: Hash = get_user_public_key_hash();
            let value: Hash = imt_get(key, 0, 4);
            let present: bool = imt_contains(key, 0, 4);
            let written: Hash = imt_set(key, value, 0, 4);
            let other_contract: Hash = get_other_contract_state_hash_at(1, contract, 0);
            let other_user: Hash = get_other_user_contract_state_hash_at(1, user, contract, 0);
            let other_value: Hash = imt_get_other_user(1, user, contract, key, 0, 4);
            let other_present: bool = imt_contains_other_user(1, user, contract, key, 0, 4);
            return 0;
        }
        "#,
    );
}

#[test]
#[serial]
fn checkpoint_stat_getters_execute() {
    expect_intrinsics_exec(
        "checkpoint_stats",
        r#"
        use std::prelude::*;

        fn main() -> Felt {
            let cp: Felt = get_checkpoint_id();
            let register_users: Hash = get_register_users_root(cp);
            let gutas: Hash = get_gutas_root(cp);
            let user_tree: Hash = get_checkpoint_user_tree_root(cp);
            let contract_tree: Hash = get_checkpoint_contract_tree_root(cp);
            let deposit_tree: Hash = get_checkpoint_deposit_tree_root(cp);
            let withdrawal_tree: Hash = get_checkpoint_withdrawal_tree_root(cp);
            let registration: Hash = get_checkpoint_user_registration_tree_root(cp);
            let deploys: Hash = get_deploy_contracts_root(cp);
            let guta_fees: Felt = get_guta_fees_collected(cp);
            let da_fees: Felt = get_da_fees_collected(cp);
            let user_ops: Felt = get_user_ops_processed(cp);
            let total_txs: Felt = get_total_transactions(cp);
            let slots: Felt = get_slots_modified(cp);
            let deploys_done: Felt = get_deploy_contracts_completed(cp);
            let registrations_done: Felt = get_register_users_completed(cp);
            let gutas_done: Felt = get_gutas_completed(cp);
            return guta_fees + da_fees + user_ops + total_txs + slots + deploys_done + registrations_done + gutas_done;
        }
        "#,
    );
}

#[test]
#[serial]
fn bit_intrinsics_execute() {
    expect_intrinsics_exec(
        "bit_ops",
        r#"
        use std::prelude::*;

        fn main() -> Felt {
            let bits: [Felt; 4] = split_bits(13, 4);
            let total: Felt = sum_bits(bits);
            return total;
        }
        "#,
    );
}

#[test]
#[serial]
fn crypto_and_invoke_intrinsics_execute() {
    expect_intrinsics_exec(
        "crypto_invoke",
        r#"
        use std::prelude::*;

        fn main() -> Felt {
            let key: Hash = get_user_public_key_hash();
            let verified: bool = secp256k1_verify([0u32; 16], key, [0u32; 16]);
            invoke_deferred(1, 2, [3]);
            return 0;
        }
        "#,
    );
}

#[test]
#[serial]
fn invoke_sync_with_generic_return_hits_size_calculation_guard() {
    // The std wrapper's generic return `T` is not substituted into the
    // CheckedIntrinsicExprNode, so the runtime output-size calculation
    // lands on a TypeVariable. Pin the guard: the deferred invoke before
    // it executes cleanly, then the sync invoke aborts.
    let outcome = std::panic::catch_unwind(|| {
        exec_source(
            "invoke_sync_generic",
            r#"
            use std::prelude::*;

            fn main() -> Felt {
                invoke_deferred(1, 2, [3]);
                let result: Felt = invoke_sync(1, 2, [3]);
                return 0;
            }
            "#,
        )
    });
    reset_primitive_scope();
    let payload = outcome.expect_err("generic invoke_sync must hit the size-calculation guard");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("panic payload was not a string");
    assert!(
        message.contains("Unsupported type for size calculation"),
        "unexpected failure: {message}"
    );
}

#[test]
#[serial]
fn derived_events_emit_at_runtime() {
    expect_intrinsics_exec(
        "event_emit",
        r#"
        use std::prelude::*;

        #[derive(Event)]
        pub struct Transferred {
            pub amount: Felt,
        }

        fn main() -> Felt {
            let event = Transferred { amount: 5 };
            event.emit();
            return 0;
        }
        "#,
    );
}

#[test]
#[serial]
fn split_bits_rejects_non_constant_lengths_at_runtime() {
    expect_intrinsics_failure(
        "split_bits_non_const",
        r#"
        use std::prelude::*;

        fn main() -> Felt {
            let nonce: Felt = get_last_nonce();
            let bits: [Felt; 4] = split_bits(13, nonce);
            return bits[0];
        }
        "#,
        "mismatch",
    );
}

#[test]
#[serial]
fn split_bits_rejects_oversized_lengths_at_runtime() {
    expect_intrinsics_failure(
        "split_bits_too_large",
        r#"
        use std::prelude::*;

        fn main() -> Felt {
            let bits: [Felt; 9999999] = split_bits(13, 9999999);
            return bits[0];
        }
        "#,
        "cannot materialize",
    );
}
