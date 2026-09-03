// Tests for the rewriter's generic-instantiation paths (psy-sema/src/
// rewriter.rs). Instantiating a generic function must rewrite every kind of
// statement and expression in its body — including calls into std context
// wrappers whose bodies lower to intrinsics — and accept only Felt/u32 loop
// endpoints once generic types become concrete.

use serial_test::serial;

use super::*;

const SOURCE: &str = r#"
use std::prelude::*;

pub struct Point {
    pub x: Felt,
    pub y: Felt,
}

fn probe_context<T>(value: T) -> T {
    let user_id: Felt = get_user_id();
    let contract_id: Felt = get_contract_id();
    let deployer: Hash = get_contract_deployer(contract_id);
    let tree_height: Felt = get_contract_state_tree_height(contract_id);
    let caller: Felt = get_caller_contract_id();
    let checkpoint: Felt = get_checkpoint_id();
    let nonce: Felt = get_last_nonce();
    let public_key_hash: Hash = get_user_public_key_hash();
    let session_root: Hash = get_session_proof_tree_root();
    let state_hash: Hash = get_state_hash_at(0);
    let register_users_root: Hash = get_register_users_root(checkpoint);
    let gutas_root: Hash = get_gutas_root(checkpoint);
    let user_tree_root: Hash = get_checkpoint_user_tree_root(checkpoint);
    let contract_tree_root: Hash = get_checkpoint_contract_tree_root(checkpoint);
    let deposit_tree_root: Hash = get_checkpoint_deposit_tree_root(checkpoint);
    let withdrawal_tree_root: Hash = get_checkpoint_withdrawal_tree_root(checkpoint);
    let registration_root: Hash = get_checkpoint_user_registration_tree_root(checkpoint);
    let deploy_contracts_root: Hash = get_deploy_contracts_root(checkpoint);
    let guta_fees: Felt = get_guta_fees_collected(checkpoint);
    let da_fees: Felt = get_da_fees_collected(checkpoint);
    let user_ops: Felt = get_user_ops_processed(checkpoint);
    let total_txs: Felt = get_total_transactions(checkpoint);
    let slots_modified: Felt = get_slots_modified(checkpoint);
    let deploys_completed: Felt = get_deploy_contracts_completed(checkpoint);
    let registrations_completed: Felt = get_register_users_completed(checkpoint);
    let gutas_completed: Felt = get_gutas_completed(checkpoint);
    let found: Hash = imt_get(public_key_hash, 0, 4);
    let other_user: Hash = imt_get_other_user(tree_height, user_id, contract_id, public_key_hash, 0, 4);
    let other_contract: Hash = get_other_contract_state_hash_at(tree_height, contract_id, 0);
    let other_user_contract: Hash = get_other_user_contract_state_hash_at(tree_height, user_id, contract_id, 0);
    let updated: Hash = cset_state_hash_at(0, state_hash);
    let written: Hash = imt_set(public_key_hash, state_hash, 0, 4);
    let contains: bool = imt_contains(public_key_hash, 0, 4);
    let contains_other: bool = imt_contains_other_user(tree_height, user_id, contract_id, public_key_hash, 0, 4);
    assert(user_id > 0, "user id is positive");
    assert_eq(deployer[0 as Felt], state_hash[0 as Felt], "hash limbs agree");
    clear_entire_tree();
    return value;
}

fn probe_expressions<T>(flag: bool, left: T, right: T) -> T {
    let count: u32 = 1u32;
    let negated: bool = !(count == 1u32);
    let pair = Point { x: 1, y: 2 };
    let field: Felt = pair.x;
    let grid: [Felt; 2] = [1, 2];
    let mut acc: Felt = grid[0 as Felt];
    let mut i: u32 = 2u32;
    while i > 0u32 {
        i -= 1u32;
    }
    for j in 0u32..2u32 {
        acc += grid[j as Felt];
    }
    let shifted: Felt = (count + i) as Felt;
    assert_eq(acc, 3, "grid sum");
    let chosen: T = if flag {
        left
    }
    else {
        right
    };
    return chosen;
}

fn main(q: Felt) -> Felt {
    let probed: Felt = probe_context(q);
    let chosen: Felt = probe_expressions(true, q, 2);
    let invoked: Felt = invoke_sync(1, 2, q);
    invoke_deferred(1, 2, q);
    return probed + chosen + invoked;
}
"#;

fn typecheck_accepts(source: &str) -> (TypeChecker<SymFeltRef, QExecContext>, TypeCheckerVisitorContext<SymFeltRef, QExecContext>) {
    super::with_primitive_scope_reset(|| {
        let path = PathBuf::from("/virtual/src/main.psy");
        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
        let program = Program::new();
        program.file_resolver.add_file(path.clone(), Arc::from(source));
        let mut graph = Graph::new();
        graph.add_node(path);
        let (typechecker, ctx) = interpreter
            .typecheck_with_program(graph, program)
            .map_err(|error| anyhow::anyhow!("generic instantiation fixture must typecheck: {error:#}"))?;
        Ok((typechecker, ctx))
    })
    .expect("typecheck within primitive scope reset")
}

fn count_definitions_named(
    typechecker: &TypeChecker<SymFeltRef, QExecContext>,
    ctx: &mut TypeCheckerVisitorContext<SymFeltRef, QExecContext>,
    name: &str,
) -> usize {
    let name_id = ctx.program.interner.intern_ident(name);
    typechecker
        .program
        .defs
        .iter()
        .filter(|def| matches!(def, CheckedDefinitionNode::Function(node) if node.name.id == name_id))
        .count()
}

#[test]
#[serial]
fn generic_bodies_with_every_statement_and_expression_kind_instantiate() {
    let (typechecker, mut ctx) = typecheck_accepts(SOURCE);

    // Each probe was instantiated from main: next to the polymorphic
    // original there is at least one monomorphic copy in the checked program.
    for probe in ["probe_context", "probe_expressions"] {
        let count = count_definitions_named(&typechecker, &mut ctx, probe);
        assert!(count >= 2, "expected an instantiated copy of {probe}, found {count} definitions");
    }
}

#[test]
#[serial]
fn non_felt_for_endpoints_in_generic_loops_are_rejected() {
    // A generic for-loop whose endpoint becomes a struct after substitution
    // must be rejected during rewriting, not at interpretation time.
    let source = r#"
        use std::prelude::*;

        pub struct Point {
            pub x: Felt,
            pub y: Felt,
        }

        fn loopgen<T>(value: T) -> T {
            for _i in value..1 {
                let one: Felt = 1;
            }
            return value;
        }

        fn main(q: Felt) -> Felt {
            let p = Point { x: 1, y: 2 };
            let r = loopgen(p);
            return r.x;
        }
    "#;
    let message = super::with_primitive_scope_reset(|| {
        let path = PathBuf::from("/virtual/src/main.psy");
        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
        let program = Program::new();
        program.file_resolver.add_file(path.clone(), Arc::from(source));
        let mut graph = Graph::new();
        graph.add_node(path);
        match interpreter.typecheck_with_program(graph, program) {
            Ok(_) => anyhow::bail!("generic loop with struct endpoint must be rejected"),
            Err(error) => Ok(format!("{error:#}")),
        }
    })
    .expect("primitive scope reset");
    assert!(
        message.contains("TypeMismatch") || message.contains("type mismatch"),
        "unexpected rejection message: {message}"
    );
}
