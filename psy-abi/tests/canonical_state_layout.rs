use std::{collections::{BTreeSet, HashMap}, path::PathBuf};

use psy_abi::{Abi, AbiExtractor};
use psy_ast::{AttrNode, ConstValue, DefaultVisitorContext, DefinitionNode, Identifier, Location, Program, StructField, StructNode, UncheckedType, Visibility, VisitorContext};
use psy_common::Graph;
use psy_interpreter::interpret;
use psy_package::{resolve_workspace_from_toml, Dependency, Package};
use serde_json::json;
use serial_test::serial;

const TOKEN_METHODS: &[&str] = &[
    "withdraw", "claim_deposit", "mint", "transfer", "claim",
    "batch_transfer_2", "batch_transfer_5", "burn", "private_transfer", "private_claim",
];

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
        graph.add_edge(entry.clone(), dependency.entry_canonical_path());
        add_package(graph, dependency);
    }
}

fn assert_token_state_layout(package: &str, contract_name: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = root.parent().unwrap().join("psy-precompiles").join(package).join("Dargo.toml");
    let workspace = resolve_workspace_from_toml(&manifest).expect("resolve real token workspace");
    let mut graph = Graph::new();
    add_package(&mut graph, &workspace.package);
    let mut result = interpret(
        Some(contract_name.to_string()),
        TOKEN_METHODS.iter().map(|name| name.to_string()).collect(),
        graph,
    ).expect("compile the full Genesis token method set");
    let metadata = result.compile_results.iter().map(|function| {
        (function.name.clone(), (function.method_id, function.is_view_function()))
    }).collect::<HashMap<_, _>>();
    let extractor = AbiExtractor::new(contract_name.to_string());
    let height = extractor.compute_state_tree_height(&mut result.ctx.program);
    let abi = extractor.extract_abi(&mut result.ctx.program, height, &metadata)
        .expect("extract real token ABI");
    let method_names = abi.contract.methods.iter().map(|method| method.name.as_str()).collect::<BTreeSet<_>>();
    assert_eq!(method_names, TOKEN_METHODS.iter().copied().collect());
    assert_eq!(abi.contract.methods.len(), TOKEN_METHODS.len());
    assert_eq!(height, 32);

    let document = serde_json::to_value(&abi).expect("serialize token ABI");
    let layout = document.pointer("/contract/state_layout")
        .expect("node canonical ABI consumer requires contract.state_layout");
    assert_eq!(layout["layout_version"], 1);
    assert_eq!(layout["encoding_version"], 1);
    assert_eq!(layout["field_count"], 8);
    assert_eq!(layout["slot_count"], 8_594_128_984u64);
    let fields = layout["fields"].as_array().expect("canonical layout fields");
    assert_eq!(fields.len(), 8);

    // Counts are felts, not four-felt Merkle leaves. OtherUserInfo has two
    // felts; the final Map stores Hash values in a capacity * 4 felt region.
    let expected = [
        ("balance", 0u64, 1u64),
        ("reserved_1", 1, 1),
        ("reserved_2", 2, 1),
        ("other_user_info", 3, 8_589_934_592),
        ("note_count", 8_589_934_595, 1),
        ("note_root", 8_589_934_596, 4),
        ("last_path", 8_589_934_600, 80),
        ("state_map", 8_589_934_680, 4_194_304),
    ];
    assert_eq!(abi.contract.state.len(), expected.len());
    for (index, ((name, start, count), state)) in expected.iter().zip(&abi.contract.state).enumerate() {
        assert_eq!(state.name, *name);
        assert_eq!(state.offset as u64, *start);
        assert_eq!(state.felt_size as u64, *count);
        assert_eq!(fields[index]["field_id"], (index + 1) as u64);
        assert_eq!(fields[index]["start_slot"], *start);
        assert_eq!(fields[index]["payload_offset"], 0);
        assert_eq!(fields[index]["slot_count"], *count);
        assert_eq!(fields[index]["type"], serde_json::to_value(&state.ty).unwrap());
    }
    assert_eq!(fields[3]["type"], json!({
        "kind": "array", "item": {"kind": "struct", "name": "OtherUserInfo"},
        "length": 4_294_967_296u64, "item_felt_size": 2,
    }));
    assert_eq!(fields[7]["type"], json!({
        "kind": "map", "map_kind": "map",
        "key": {"kind": "primitive", "name": "Hash"},
        "value": {"kind": "primitive", "name": "Hash"},
        "capacity": 1_048_576, "value_felt_size": 4, "alignment_felts": 4,
    }));
    let other_user = abi.types.iter().find(|ty| ty.name == "OtherUserInfo")
        .expect("node consumer must resolve the array element struct");
    assert_eq!(other_user.felt_size, 2);
    assert_eq!(other_user.fields.len(), 2);
    assert_eq!(other_user.fields[0].offset_within_parent, 0);
    assert_eq!(other_user.fields[1].offset_within_parent, 1);
}

#[test]
#[serial]
fn token_abi_exports_canonical_state_layout() {
    assert_token_state_layout("token", "PsyTokenContract");
}

#[test]
#[serial]
fn usdt_token_abi_exports_canonical_state_layout() {
    assert_token_state_layout("usdt_token", "USDTTokenContract");
}

fn identifier(program: &mut Program<u32>, name: &str) -> Identifier {
    Identifier::new(program.interner.intern_ident(name), Location::default())
}

fn fixture(map_value: &str, nested_map: bool) -> Program<u32> {
    let mut program = Program::new();
    let name = identifier(&mut program, "LayoutFixture");
    let contract_attr = identifier(&mut program, "contract");
    let hidden = identifier(&mut program, "hidden");
    let entries = identifier(&mut program, "entries");
    let felt = UncheckedType::Basic(identifier(&mut program, "Felt"));
    let hash = UncheckedType::Basic(identifier(&mut program, "Hash"));
    let value = UncheckedType::Basic(identifier(&mut program, map_value));
    let map = UncheckedType::Generic(
        identifier(&mut program, "Map"),
        vec![hash, value, UncheckedType::Const(ConstValue::U32(8), Location::default())],
        Location::default(),
    );
    let map = if nested_map {
        UncheckedType::Array(Box::new(map), ConstValue::U32(2), Location::default())
    } else {
        map
    };
    let fields = [(hidden, felt, Visibility::Private), (entries, map, Visibility::Public)]
        .into_iter().map(|(name, ty, visibility)| (name, StructField {
            ty, attrs: vec![], visibility, comments: vec![], location: Location::default(),
        })).collect();
    let structure = StructNode {
        name, generic_parameters: vec![], fields,
        attrs: vec![AttrNode { path: vec![], name: contract_attr, properties: vec![], location: Location::default() }],
        visibility: Visibility::Public, comments: vec![], location: Location::default(), is_generated: false,
    };
    DefaultVisitorContext::<u32, ()>::new(&mut program).alloc_definition(DefinitionNode::Struct(structure));
    program
}

#[test]
fn canonical_layout_includes_private_fields_and_map_padding() {
    let mut program = fixture("Hash", false);
    let abi = AbiExtractor::new("LayoutFixture".to_string())
        .extract_abi(&mut program, 4, &HashMap::new()).unwrap();
    // Public ABI visibility stays unchanged; consensus layout still commits
    // the private field and the map's three leading padding felts.
    assert_eq!(abi.contract.state.len(), 1);
    assert_eq!(abi.contract.state[0].name, "entries");
    assert_eq!(abi.contract.state[0].offset, 4);
    let layout = abi.contract.state_layout.as_ref().unwrap();
    assert_eq!(layout.field_count, 2);
    assert_eq!(layout.slot_count, 36);
    assert_eq!((layout.fields[0].field_id, layout.fields[0].start_slot, layout.fields[0].payload_offset, layout.fields[0].slot_count), (1, 0, 0, 1));
    assert_eq!((layout.fields[1].field_id, layout.fields[1].start_slot, layout.fields[1].payload_offset, layout.fields[1].slot_count), (2, 1, 3, 35));
    let mut document = serde_json::to_value(&abi).unwrap();
    assert_eq!(serde_json::from_value::<Abi>(document.clone()).unwrap(), abi);
    document["contract"].as_object_mut().unwrap().remove("state_layout");
    assert!(serde_json::from_value::<Abi>(document).unwrap().contract.state_layout.is_none());
}

#[test]
fn canonical_layout_rejects_map_value_storage_mismatch() {
    let error = AbiExtractor::new("LayoutFixture".to_string())
        .extract_abi(&mut fixture("Felt", false), 4, &HashMap::new()).unwrap_err();
    assert!(error.to_string().contains("four-felt IMT storage entry"), "{error}");
}

#[test]
fn canonical_layout_rejects_nested_maps() {
    let error = AbiExtractor::new("LayoutFixture".to_string())
        .extract_abi(&mut fixture("Hash", true), 4, &HashMap::new()).unwrap_err();
    assert!(error.to_string().contains("maps to be top-level"), "{error}");
}

#[test]
fn canonical_layout_rejects_insufficient_tree_capacity() {
    let error = AbiExtractor::new("LayoutFixture".to_string())
        .extract_abi(&mut fixture("Hash", false), 3, &HashMap::new()).unwrap_err();
    assert!(error.to_string().contains("exceeds contract state tree capacity"), "{error}");
}
