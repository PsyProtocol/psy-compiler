use std::{collections::HashMap, path::PathBuf};

use psy_abi::{Abi, AbiExtractor, AbiMethod, AbiParam, PrimitiveTypeName, StateMutability, TypeRef};
use psy_common::Graph;
use psy_interpreter::interpret;
use psy_package::{resolve_workspace_from_toml, Dependency, Package};
use serial_test::serial;

fn compiler_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("psy-abi is inside compiler workspace")
        .to_path_buf()
}

fn add_package(graph: &mut Graph<PathBuf>, package: &Package) {
    let entry = package.entry_canonical_path();
    if graph.contains_node(&entry) {
        return;
    }
    graph.add_node(entry.clone());
    for dependency in package.dependencies.values() {
        let dependency = match dependency {
            Dependency::Local { package } | Dependency::Remote { package } => package,
        };
        let dependency_entry = dependency.entry_canonical_path();
        graph.add_edge(entry.clone(), dependency_entry);
        add_package(graph, dependency);
    }
}

fn compile_abi(package: &str, contract_name: &str, methods: &[&str]) -> (Abi, HashMap<String, (u32, bool)>) {
    let manifest = compiler_root()
        .join("psy-precompiles")
        .join(package)
        .join("Dargo.toml");
    let workspace = resolve_workspace_from_toml(&manifest).expect("resolve Dargo workspace");
    let mut graph = Graph::new();
    add_package(&mut graph, &workspace.package);

    let mut result = interpret(
        Some(contract_name.to_string()),
        methods.iter().map(|name| (*name).to_string()).collect(),
        graph,
    )
    .expect("compile bridge-critical methods");
    let metadata = result
        .compile_results
        .iter()
        .map(|function| {
            (
                function.name.clone(),
                (function.method_id, function.is_view_function()),
            )
        })
        .collect::<HashMap<_, _>>();
    let extractor = AbiExtractor::new(contract_name.to_string());
    let state_tree_height = extractor.compute_state_tree_height(&mut result.ctx.program);
    let abi = extractor
        .extract_abi(&mut result.ctx.program, state_tree_height, &metadata)
        .expect("extract canonical ABI");
    (abi, metadata)
}

fn method<'a>(abi: &'a Abi, name: &str) -> &'a AbiMethod {
    abi.contract
        .methods
        .iter()
        .find(|method| method.name == name)
        .unwrap_or_else(|| panic!("missing ABI method {name}"))
}

fn primitive(name: PrimitiveTypeName) -> TypeRef {
    TypeRef::Primitive { name }
}

fn array(item: TypeRef, length: u64, item_felt_size: usize) -> TypeRef {
    TypeRef::Array {
        item: Box::new(item),
        length,
        item_felt_size,
    }
}

fn param(name: &str, ty: TypeRef, felt_size: usize) -> AbiParam {
    AbiParam {
        name: name.to_string(),
        ty,
        felt_size,
    }
}

fn assert_method_contract(
    abi: &Abi,
    metadata: &HashMap<String, (u32, bool)>,
    name: &str,
    mutability: StateMutability,
    inputs: Vec<AbiParam>,
    outputs: Vec<AbiParam>,
) {
    let actual = method(abi, name);
    assert_eq!(actual.state_mutability, mutability, "{name} mutability");
    assert_eq!(actual.inputs, inputs, "{name} input ABI");
    assert_eq!(actual.outputs, outputs, "{name} output ABI");
    assert_eq!(
        actual.input_felt_count,
        actual.inputs.iter().map(|input| input.felt_size).sum::<usize>(),
        "{name} input felt count",
    );
    assert_eq!(
        actual.output_felt_count,
        actual.outputs.iter().map(|output| output.felt_size).sum::<usize>(),
        "{name} output felt count",
    );

    let &(compiled_id, compiled_is_view) = metadata
        .get(name)
        .unwrap_or_else(|| panic!("missing compiled method {name}"));
    assert_eq!(actual.method_id, compiled_id, "{name} method ID drifted between compile result and ABI");
    assert_eq!(
        actual.state_mutability.is_view(),
        compiled_is_view,
        "{name} mutability drifted between compile result and ABI",
    );
}

#[test]
#[serial]
fn claim_deposit_canonical_abi_preserves_bridge_identity_and_proof_shape() {
    let (abi, metadata) = compile_abi("token", "PsyTokenContract", &["claim_deposit"]);
    assert_eq!(abi.schema_version, "2.0.0");
    assert_eq!(abi.contract.name, "PsyTokenContract");

    assert_method_contract(
        &abi,
        &metadata,
        "claim_deposit",
        StateMutability::External,
        vec![
            param("nullifier_hash", primitive(PrimitiveTypeName::Hash), 4),
            param("shield_address", primitive(PrimitiveTypeName::Hash), 4),
            param("token_address", array(primitive(PrimitiveTypeName::U32), 8, 1), 8),
            param("amount", array(primitive(PrimitiveTypeName::U32), 8, 1), 8),
            param("source_chain_index", primitive(PrimitiveTypeName::Felt), 1),
            param("deposit_root", array(primitive(PrimitiveTypeName::U32), 8, 1), 8),
            param("note_commitment", primitive(PrimitiveTypeName::Hash), 4),
            param("deposit_index", primitive(PrimitiveTypeName::Felt), 1),
            param("r0", primitive(PrimitiveTypeName::Felt), 1),
            param("r1", primitive(PrimitiveTypeName::Felt), 1),
            param("proof", TypeRef::Struct { name: "ShieldClaimProof".to_string() }, 65),
        ],
        vec![],
    );

    let proof = abi
        .types
        .iter()
        .find(|ty| ty.name == "ShieldClaimProof")
        .expect("ShieldClaimProof type entry");
    assert_eq!(proof.felt_size, 65);
    assert_eq!(proof.fields.len(), 2);
    assert_eq!(proof.fields[0].name, "siblings");
    assert_eq!(
        proof.fields[0].ty,
        array(primitive(PrimitiveTypeName::Hash), 16, 4),
    );
    assert_eq!(proof.fields[0].offset_within_parent, 0);
    assert_eq!(proof.fields[0].felt_size, 64);
    assert_eq!(proof.fields[1].name, "index");
    assert_eq!(proof.fields[1].ty, primitive(PrimitiveTypeName::Felt));
    assert_eq!(proof.fields[1].offset_within_parent, 64);
    assert_eq!(proof.fields[1].felt_size, 1);
}

#[test]
#[serial]
fn append_withdrawal_canonical_abi_preserves_live_slot_field_order() {
    let (abi, metadata) = compile_abi(
        "withdrawal_tree",
        "PsyWithdrawalTreeContract",
        &["append_withdrawal", "get_root", "get_chain_root"],
    );

    assert_method_contract(
        &abi,
        &metadata,
        "append_withdrawal",
        StateMutability::External,
        vec![
            param("sender_user_id", primitive(PrimitiveTypeName::Felt), 1),
            param("token_contract_id", primitive(PrimitiveTypeName::Felt), 1),
            param("destination_chain_index", primitive(PrimitiveTypeName::Felt), 1),
            param("token_address", array(primitive(PrimitiveTypeName::U32), 8, 1), 8),
            param("amount", array(primitive(PrimitiveTypeName::U32), 8, 1), 8),
            param("recipient", array(primitive(PrimitiveTypeName::U32), 8, 1), 8),
            param("nonce", array(primitive(PrimitiveTypeName::U32), 8, 1), 8),
        ],
        vec![],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "get_root",
        StateMutability::View,
        vec![],
        vec![param("return", array(primitive(PrimitiveTypeName::U32), 8, 1), 8)],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "get_chain_root",
        StateMutability::View,
        vec![param(
            "destination_chain_index",
            primitive(PrimitiveTypeName::Felt),
            1,
        )],
        vec![param("return", array(primitive(PrimitiveTypeName::U32), 8, 1), 8)],
    );
}

#[test]
#[serial]
fn deposit_tree_canonical_abi_classifies_reads_and_mutations_without_name_heuristics() {
    let methods = [
        "poseidon_two_to_one",
        "set_chain_root",
        "get_root",
        "get_chain_root",
        "append_leaf",
        "batch_append_deposits_2",
        "batch_append_deposits_5",
        "append_deposit",
        "is_known_root",
        "is_known_root_hash",
    ];
    let (abi, metadata) = compile_abi("deposit_tree", "PsyDepositTreeContract", &methods);

    let u32x8 = || array(primitive(PrimitiveTypeName::U32), 8, 1);
    assert_method_contract(
        &abi,
        &metadata,
        "poseidon_two_to_one",
        StateMutability::View,
        vec![param("left", u32x8(), 8), param("right", u32x8(), 8)],
        vec![param("return", u32x8(), 8)],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "set_chain_root",
        StateMutability::External,
        vec![
            param("chain_index", primitive(PrimitiveTypeName::Felt), 1),
            param("total_count", primitive(PrimitiveTypeName::Felt), 1),
            param("new_chain_root", u32x8(), 8),
        ],
        vec![],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "get_root",
        StateMutability::View,
        vec![],
        vec![param("return", u32x8(), 8)],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "get_chain_root",
        StateMutability::View,
        vec![param("chain_index", primitive(PrimitiveTypeName::Felt), 1)],
        vec![param("return", u32x8(), 8)],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "append_leaf",
        StateMutability::External,
        vec![param("leaf", u32x8(), 8)],
        vec![],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "batch_append_deposits_2",
        StateMutability::External,
        vec![
            param("chain_index", primitive(PrimitiveTypeName::Felt), 1),
            param("count", primitive(PrimitiveTypeName::Felt), 1),
            param("leaf_data", array(primitive(PrimitiveTypeName::U32), 16, 1), 16),
        ],
        vec![],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "batch_append_deposits_5",
        StateMutability::External,
        vec![
            param("chain_index", primitive(PrimitiveTypeName::Felt), 1),
            param("count", primitive(PrimitiveTypeName::Felt), 1),
            param("leaf_data", array(primitive(PrimitiveTypeName::U32), 40, 1), 40),
        ],
        vec![],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "append_deposit",
        StateMutability::External,
        vec![
            param("chain_index", primitive(PrimitiveTypeName::Felt), 1),
            param("leaf", u32x8(), 8),
        ],
        vec![],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "is_known_root",
        StateMutability::View,
        vec![
            param("chain_index", primitive(PrimitiveTypeName::Felt), 1),
            param("root", u32x8(), 8),
        ],
        vec![param("return", primitive(PrimitiveTypeName::Felt), 1)],
    );
    assert_method_contract(
        &abi,
        &metadata,
        "is_known_root_hash",
        StateMutability::View,
        vec![
            param("chain_index", primitive(PrimitiveTypeName::Felt), 1),
            param("root_lo", primitive(PrimitiveTypeName::Hash), 4),
            param("root_hi", primitive(PrimitiveTypeName::Hash), 4),
        ],
        vec![param("return", primitive(PrimitiveTypeName::Felt), 1)],
    );
}
