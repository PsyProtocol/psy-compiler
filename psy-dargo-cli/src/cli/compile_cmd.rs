use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
};

use clap::Args;
use psy_abi::{Abi, AbiExtractor};
use psy_package::{ResolvedSourceWorkspace, VfsPath, Workspace};
use psy_vm::dpn::{
    ops::{op_types::DPNOpType, state_cmd::data::DPNStateCmd},
    vm::def::DPNFunctionCircuitDefinition,
};
use serde::Serialize;

use crate::{
    cli::{
        doc_cmd::{extract_function_metadata_from_context, FunctionNode},
        save_build_artifact_to_file,
    },
    errors::Result,
};

/// Compile the program and its secret execution trace
#[derive(Debug, Clone, Args)]
pub struct CompileCommand {
    #[clap(flatten)]
    pub compile_options: CompileOptions,
}

pub fn run(args: CompileCommand, workspace: Workspace) -> Result<()> {
    compile_workspace_full(&workspace, &args.compile_options)?;
    Ok(())
}

pub struct CompilationResult {
    pub state_tree_height: u16,
    pub circuit_definitions: Vec<DPNFunctionCircuitDefinition>,
    pub function_metadata: HashMap<String, FunctionNode>,
}

#[derive(Serialize)]
struct CompilationArtifact {
    state_tree_height: u16,
    circuit_definitions: Vec<DPNFunctionCircuitDefinition>,
    abi: Abi,
}

/// Parse and compile the entire workspace, then report errors.
/// This is the main entry point used by all other commands that need
/// compilation.
pub fn compile_workspace_full(workspace: &Workspace, compile_options: &CompileOptions) -> Result<CompilationResult> {
    let crate_path_graph = super::resolve_crate_path_graph(workspace, compile_options.entry_path.clone());
    let method_names = resolve_workspace_method_names(workspace, compile_options)?;

    let mut interpret_result = psy_interpreter::interpret(compile_options.contract_name.clone(), method_names, crate_path_graph)?;

    let function_metadata = extract_function_metadata_from_context(
        &mut interpret_result.ctx,
        &mut interpret_result.typechecker,
        &interpret_result.compile_results,
    );
    let state_tree_height = AbiExtractor::new(compile_options.contract_name.clone().unwrap_or_else(|| "contract".to_string()))
        .compute_state_tree_height(&mut interpret_result.ctx.program);
    validate_static_state_accesses(state_tree_height, &interpret_result.compile_results)?;

    if compile_options.debug {
        println!("workspace: {:?}", workspace);
        println!("compile_result: {:?}", interpret_result.compile_results);
        println!("state_tree_height: {state_tree_height}");
    } else {
        let method_metadata: HashMap<String, (u32, bool)> = interpret_result
            .compile_results
            .iter()
            .map(|f| (f.name.clone(), (f.method_id, f.is_view_function())))
            .collect();
        let extractor = AbiExtractor::new(compile_options.contract_name.clone().unwrap_or_else(|| "contract".to_string()));
        let abi = extractor
            .extract_abi(&mut interpret_result.ctx.program, state_tree_height, &method_metadata)
            .map_err(|e| crate::errors::CliError::Generic(e.to_string()))?;

        save_build_artifact_to_file(
            &CompilationArtifact {
                state_tree_height,
                circuit_definitions: interpret_result.compile_results.clone(),
                abi,
            },
            &workspace.package.name.to_string(),
            &workspace.target_dir,
        )?;
    }

    Ok(CompilationResult {
        state_tree_height,
        circuit_definitions: interpret_result.compile_results,
        function_metadata,
    })
}

/// Compile a resolver-based workspace assembled from in-memory sources.
pub fn compile_source_workspace_full(workspace: &ResolvedSourceWorkspace, compile_options: &CompileOptions) -> Result<CompilationResult> {
    let mut crate_path_graph = psy_common::Graph::new();
    let method_names = resolve_source_workspace_method_names(workspace, compile_options)?;

    let package_entry_path = workspace
        .packages
        .iter()
        .map(|(id, pkg)| {
            (
                id.clone(),
                source_vfs_to_pathbuf(workspace.source_map.path(pkg.entry_file_id).expect("entry file id exists")),
            )
        })
        .collect::<HashMap<_, _>>();

    for package in workspace.packages.values() {
        let package_entry = package_entry_path.get(&package.package_id).expect("entry path exists").clone();
        if !crate_path_graph.contains_node(&package_entry) {
            crate_path_graph.add_node(package_entry.clone());
        }
        for dep_pkg_id in package.dependency_packages.values() {
            if let Some(dep_entry) = package_entry_path.get(dep_pkg_id) {
                crate_path_graph.add_edge(package_entry.clone(), dep_entry.clone());
            }
        }
    }

    let virtual_files = workspace
        .source_map
        .snapshot()
        .into_iter()
        .map(|(_, path, text)| (source_vfs_to_pathbuf(&path), text))
        .collect::<Vec<_>>();

    let mut interpret_result =
        psy_interpreter::interpret_virtual_files(compile_options.contract_name.clone(), method_names, crate_path_graph, virtual_files)?;

    let function_metadata = extract_function_metadata_from_context(
        &mut interpret_result.ctx,
        &mut interpret_result.typechecker,
        &interpret_result.compile_results,
    );
    let state_tree_height = AbiExtractor::new(compile_options.contract_name.clone().unwrap_or_else(|| "contract".to_string()))
        .compute_state_tree_height(&mut interpret_result.ctx.program);
    validate_static_state_accesses(state_tree_height, &interpret_result.compile_results)?;

    Ok(CompilationResult {
        state_tree_height,
        circuit_definitions: interpret_result.compile_results,
        function_metadata,
    })
}

fn validate_static_state_accesses(state_tree_height: u16, circuits: &[DPNFunctionCircuitDefinition]) -> Result<()> {
    let leaf_capacity = if state_tree_height >= u64::BITS as u16 {
        None
    } else {
        Some(1u64 << state_tree_height)
    };

    for circuit in circuits {
        let constants = circuit
            .definitions
            .iter()
            .filter_map(|definition| {
                let value = match definition.op_type {
                    DPNOpType::Constant | DPNOpType::ConstantU32 => definition.inputs.first().copied(),
                    DPNOpType::ConstantTrue => Some(1),
                    DPNOpType::ConstantFalse => Some(0),
                    _ => None,
                }?;
                Some((definition.get_combined_data_type_index(), value))
            })
            .collect::<HashMap<_, _>>();

        for (command_index, command) in circuit.state_commands.iter().enumerate() {
            let constant = |wire: &u64| constants.get(wire).copied();
            let access = match command {
                DPNStateCmd::SetContractStateSlotHash(command) => constant(&command.slot_index).map(|leaf| ("SetContractStateSlotHash", leaf)),
                DPNStateCmd::SetContractStateSlotSingle(command) => {
                    constant(&command.sub_slot_index).map(|felt| ("SetContractStateSlotSingle", felt / 4))
                }
                DPNStateCmd::SetContractStateSlotRange(command) => constant(&command.sub_slot_index).and_then(|start| {
                    let end = start.checked_add(command.value.len().saturating_sub(1) as u64)?;
                    Some(("SetContractStateSlotRange", end / 4))
                }),
                DPNStateCmd::GetSelfUserCurrentContractStateSlotHash(command) => {
                    constant(&command.slot_index).map(|leaf| ("GetSelfUserCurrentContractStateSlotHash", leaf))
                }
                DPNStateCmd::GetSelfUserCurrentContractStateSlotSingle(command) => {
                    constant(&command.sub_slot_index).map(|felt| ("GetSelfUserCurrentContractStateSlotSingle", felt / 4))
                }
                DPNStateCmd::GetSelfUserCurrentContractStateSlotRange(command) => constant(&command.sub_slot_index).and_then(|start| {
                    let end = start.checked_add(u64::from(command.length).saturating_sub(1))?;
                    Some(("GetSelfUserCurrentContractStateSlotRange", end / 4))
                }),
                DPNStateCmd::SetIMTContractStateValue(command) => {
                    static_imt_last_leaf(&constant, &command.base_offset, &command.capacity).map(|leaf| ("SetIMTContractStateValue", leaf))
                }
                DPNStateCmd::GetSelfUserCurrentIMTContractStateValue(command) => {
                    static_imt_last_leaf(&constant, &command.base_offset, &command.capacity)
                        .map(|leaf| ("GetSelfUserCurrentIMTContractStateValue", leaf))
                }
                DPNStateCmd::ContainsSelfUserCurrentIMTContractStateValue(command) => {
                    static_imt_last_leaf(&constant, &command.base_offset, &command.capacity)
                        .map(|leaf| ("ContainsSelfUserCurrentIMTContractStateValue", leaf))
                }
                _ => None,
            };

            if let (Some(capacity), Some((command_name, last_leaf))) = (leaf_capacity, access) {
                if last_leaf >= capacity {
                    return Err(crate::errors::CliError::Generic(format!(
                        "state-tree sanity check failed: method '{}' command #{} ({}) statically accesses leaf {}, \
                         but state_tree_height {} only provides leaves 0..{}",
                        circuit.name,
                        command_index,
                        command_name,
                        last_leaf,
                        state_tree_height,
                        capacity - 1,
                    )));
                }
            }
        }
    }

    Ok(())
}

fn static_imt_last_leaf(constant: &impl Fn(&u64) -> Option<u64>, base_offset_wire: &u64, capacity_wire: &u64) -> Option<u64> {
    let base_offset = constant(base_offset_wire)?;
    let capacity = constant(capacity_wire)?;
    if capacity == 0 {
        return None;
    }
    let last_felt = base_offset.checked_add(capacity.checked_mul(4)?.checked_sub(1)?)?;
    Some(last_felt / 4)
}

/// Options for the compile command
#[derive(Args, Clone, Debug, Default)]
pub struct CompileOptions {
    #[clap(short, long, default_value = None)]
    pub contract_name: Option<String>,
    #[clap(short, long, num_args = 1..)]
    pub method_names: Option<Vec<String>>,
    #[clap(long, hide = true, default_value = None)]
    pub entry_path: Option<PathBuf>,
    #[clap(long, hide = true, default_value = "false")]
    pub debug: bool,
}

fn source_vfs_to_pathbuf(path: &VfsPath) -> PathBuf {
    match path {
        VfsPath::Real(path) => path.clone(),
        VfsPath::Virtual(path) => PathBuf::from(path),
    }
}

pub(crate) fn resolve_workspace_method_names(workspace: &Workspace, compile_options: &CompileOptions) -> Result<Vec<String>> {
    if let Some(method_names) = &compile_options.method_names {
        if method_names.is_empty() {
            return Err(crate::errors::CliError::Generic(
                "method_names must not be empty when provided".to_string(),
            ));
        }
        return Ok(method_names.clone());
    }

    let mut root_package = workspace.package.clone();
    if let Some(entry_path) = &compile_options.entry_path {
        root_package.entry_path = entry_path.clone();
    }
    let root_source = std::fs::read_to_string(root_package.entry_canonical_path())?;
    if source_has_function_name(&root_source, "main") {
        return Ok(vec!["main".to_string()]);
    }

    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    queue.push_back(&workspace.package);

    let mut method_names = Vec::new();
    while let Some(package) = queue.pop_front() {
        let key = package.entry_canonical_path().display().to_string();
        if !visited.insert(key) {
            continue;
        }
        let source = std::fs::read_to_string(package.entry_canonical_path())?;
        extract_contract_method_names(&source, &mut method_names);
        for dep in package.dependencies.values() {
            match dep {
                psy_package::Dependency::Remote { package } | psy_package::Dependency::Local { package } => {
                    queue.push_back(package);
                }
            }
        }
    }

    method_names.sort();
    method_names.dedup();
    if method_names.is_empty() {
        return Err(crate::errors::CliError::Generic(
            "Unable to discover contract methods: add #[contract_method], #[contract::write_method], or #[contract::view_method] to at least one function or provide --method-names explicitly"
                .to_string(),
        ));
    }
    Ok(method_names)
}

pub(crate) fn resolve_source_workspace_method_names(workspace: &ResolvedSourceWorkspace, compile_options: &CompileOptions) -> Result<Vec<String>> {
    if let Some(method_names) = &compile_options.method_names {
        if method_names.is_empty() {
            return Err(crate::errors::CliError::Generic(
                "method_names must not be empty when provided".to_string(),
            ));
        }
        return Ok(method_names.clone());
    }

    let mut method_names = Vec::new();
    for (_, _, text) in workspace.source_map.snapshot() {
        extract_contract_method_names(&text, &mut method_names);
    }
    method_names.sort();
    method_names.dedup();
    if method_names.is_empty() {
        return Err(crate::errors::CliError::Generic(
            "Unable to discover contract methods: add #[contract_method], #[contract::write_method], or #[contract::view_method] to at least one function or provide --method-names explicitly"
                .to_string(),
        ));
    }
    Ok(method_names)
}

fn extract_contract_method_names(source: &str, method_names: &mut Vec<String>) {
    for marker in ["#[contract::write_method]", "#[contract::view_method]", "#[contract_method]"] {
        let mut search_start = 0usize;
        while let Some(relative) = source[search_start..].find(marker) {
            let attr_start = search_start + relative + marker.len();
            let rest = &source[attr_start..];
            if let Some(method_name) = extract_first_function_name(rest) {
                method_names.push(method_name);
            }
            search_start = attr_start;
        }
    }
}

fn extract_first_function_name(source: &str) -> Option<String> {
    extract_function_name(source, |_| true)
}

fn source_has_function_name(source: &str, expected_name: &str) -> bool {
    extract_function_name(source, |name| name == expected_name).is_some()
}

fn extract_function_name(source: &str, predicate: impl Fn(&str) -> bool) -> Option<String> {
    let marker = "fn ";
    let mut search_start = 0usize;
    while let Some(relative) = source[search_start..].find(marker) {
        let start = search_start + relative + marker.len();
        let rest = &source[start..];
        let end = rest
            .find(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
            .unwrap_or(rest.len());
        if end > 0 && predicate(&rest[..end]) {
            return Some(rest[..end].to_string());
        }
        search_start = start;
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use psy_package::{Package, ResolvedSourceWorkspace, SourceMap, VfsPath, Workspace};
    use psy_vm::dpn::{
        ops::{
            op_types::{DPNBuiltInDataType, DPNIndexedVarDef, DPNOpType},
            state_cmd::data::{
                DPNStateCmd, DPNStateCmdClearEntireTree, DPNStateCmdContainsSelfUserCurrentIMTContractStateValue,
                DPNStateCmdGetSelfUserCurrentContractStateSlotRange, DPNStateCmdGetSelfUserCurrentContractStateSlotSingle,
                DPNStateCmdGetSelfUserCurrentIMTContractStateValue, DPNStateCmdSetContractStateSlotHash,
                DPNStateCmdGetSelfUserCurrentContractStateSlotHash,
                DPNStateCmdSetContractStateSlotRange, DPNStateCmdSetContractStateSlotSingle, DPNStateCmdSetIMTContractStateValue,
            },
        },
        vm::def::DPNFunctionCircuitDefinition,
    };

    use super::{
        extract_contract_method_names, extract_function_name, resolve_source_workspace_method_names, resolve_workspace_method_names,
        source_has_function_name, source_vfs_to_pathbuf, validate_static_state_accesses, CompileCommand, CompileOptions,
    };

    fn constant_def(index: usize, op_type: DPNOpType, value: u64) -> DPNIndexedVarDef {
        DPNIndexedVarDef {
            data_type: DPNBuiltInDataType::Target,
            index,
            op_type,
            inputs: vec![value],
        }
    }

    fn circuit_with(name: &str, defs: Vec<DPNIndexedVarDef>, state_commands: Vec<DPNStateCmd<u64>>) -> DPNFunctionCircuitDefinition {
        DPNFunctionCircuitDefinition {
            name: name.to_string(),
            method_id: 0,
            circuit_inputs: vec![],
            circuit_outputs: vec![],
            state_commands,
            state_command_resolution_indices: vec![],
            assertions: vec![],
            definitions: defs,
            events: vec![],
        }
    }

    #[test]
    fn static_state_validation_accepts_leaves_inside_the_configured_height() {
        // Height 4 gives a capacity of 16 leaves; both commands below stay
        // within it, and the ConstantTrue/ConstantFalse/ConstantU32 arms all
        // feed constant resolution.
        let defs = vec![
            constant_def(0, DPNOpType::Constant, 15),
            constant_def(1, DPNOpType::ConstantTrue, 1),
            constant_def(2, DPNOpType::ConstantFalse, 0),
            constant_def(3, DPNOpType::ConstantU32, 63),
        ];
        let hash_wire = defs[0].get_combined_data_type_index();
        let true_wire = defs[1].get_combined_data_type_index();
        let false_wire = defs[2].get_combined_data_type_index();
        let single_wire = defs[3].get_combined_data_type_index();
        let commands = vec![
            DPNStateCmd::SetContractStateSlotHash(DPNStateCmdSetContractStateSlotHash {
                condition: 0,
                slot_index: hash_wire,
                value: [0, 0, 0, 0],
            }),
            DPNStateCmd::SetContractStateSlotSingle(DPNStateCmdSetContractStateSlotSingle {
                condition: 0,
                sub_slot_index: single_wire,
                value: 0,
            }),
            // Non-state command kinds are ignored entirely.
            DPNStateCmd::ClearEntireTree(DPNStateCmdClearEntireTree { condition: true_wire }),
            DPNStateCmd::ClearEntireTree(DPNStateCmdClearEntireTree { condition: false_wire }),
        ];
        assert!(validate_static_state_accesses(4, &[circuit_with("in_range", defs, commands)]).is_ok());
        assert!(validate_static_state_accesses(4, &[]).is_ok());
    }

    #[test]
    fn static_state_validation_rejects_each_command_kind_accessing_past_the_last_leaf() {
        for (name, defs, command) in [
            ("SetContractStateSlotHash", vec![constant_def(0, DPNOpType::Constant, 16)], {
                let wire = constant_def(0, DPNOpType::Constant, 16).get_combined_data_type_index();
                DPNStateCmd::SetContractStateSlotHash(DPNStateCmdSetContractStateSlotHash {
                    condition: 0,
                    slot_index: wire,
                    value: [0, 0, 0, 0],
                })
            }),
            ("SetContractStateSlotSingle", vec![constant_def(0, DPNOpType::Constant, 64)], {
                let wire = constant_def(0, DPNOpType::Constant, 64).get_combined_data_type_index();
                DPNStateCmd::SetContractStateSlotSingle(DPNStateCmdSetContractStateSlotSingle {
                    condition: 0,
                    sub_slot_index: wire,
                    value: 0,
                })
            }),
            ("SetContractStateSlotRange", vec![constant_def(0, DPNOpType::Constant, 60)], {
                let wire = constant_def(0, DPNOpType::Constant, 60).get_combined_data_type_index();
                DPNStateCmd::SetContractStateSlotRange(DPNStateCmdSetContractStateSlotRange {
                    condition: 0,
                    sub_slot_index: wire,
                    value: vec![0; 5],
                })
            }),
            (
                "GetSelfUserCurrentContractStateSlotHash",
                vec![constant_def(0, DPNOpType::Constant, 16)],
                {
                    let wire = constant_def(0, DPNOpType::Constant, 16).get_combined_data_type_index();
                    DPNStateCmd::GetSelfUserCurrentContractStateSlotHash(DPNStateCmdGetSelfUserCurrentContractStateSlotHash { slot_index: wire })
                },
            ),
            (
                "GetSelfUserCurrentContractStateSlotSingle",
                vec![constant_def(0, DPNOpType::Constant, 64)],
                {
                    let wire = constant_def(0, DPNOpType::Constant, 64).get_combined_data_type_index();
                    DPNStateCmd::GetSelfUserCurrentContractStateSlotSingle(DPNStateCmdGetSelfUserCurrentContractStateSlotSingle {
                        sub_slot_index: wire,
                    })
                },
            ),
            ("GetSelfUserCurrentContractStateSlotRange", vec![constant_def(0, DPNOpType::Constant, 60)], {
                let wire = constant_def(0, DPNOpType::Constant, 60).get_combined_data_type_index();
                DPNStateCmd::GetSelfUserCurrentContractStateSlotRange(DPNStateCmdGetSelfUserCurrentContractStateSlotRange {
                    sub_slot_index: wire,
                    length: 5,
                })
            }),
            ("SetIMTContractStateValue", vec![constant_def(0, DPNOpType::Constant, 60), constant_def(1, DPNOpType::Constant, 2)], {
                let base = constant_def(0, DPNOpType::Constant, 60).get_combined_data_type_index();
                let capacity = constant_def(1, DPNOpType::Constant, 2).get_combined_data_type_index();
                DPNStateCmd::SetIMTContractStateValue(DPNStateCmdSetIMTContractStateValue {
                    condition: 0,
                    base_offset: base,
                    capacity,
                    key: [0, 0, 0, 0],
                    value: [0, 0, 0, 0],
                })
            }),
            (
                "GetSelfUserCurrentIMTContractStateValue",
                vec![constant_def(0, DPNOpType::Constant, 60), constant_def(1, DPNOpType::Constant, 2)],
                {
                    let base = constant_def(0, DPNOpType::Constant, 60).get_combined_data_type_index();
                    let capacity = constant_def(1, DPNOpType::Constant, 2).get_combined_data_type_index();
                    DPNStateCmd::GetSelfUserCurrentIMTContractStateValue(DPNStateCmdGetSelfUserCurrentIMTContractStateValue {
                        base_offset: base,
                        capacity,
                        key: [0, 0, 0, 0],
                    })
                },
            ),
            (
                "ContainsSelfUserCurrentIMTContractStateValue",
                vec![constant_def(0, DPNOpType::Constant, 60), constant_def(1, DPNOpType::Constant, 2)],
                {
                    let base = constant_def(0, DPNOpType::Constant, 60).get_combined_data_type_index();
                    let capacity = constant_def(1, DPNOpType::Constant, 2).get_combined_data_type_index();
                    DPNStateCmd::ContainsSelfUserCurrentIMTContractStateValue(DPNStateCmdContainsSelfUserCurrentIMTContractStateValue {
                        base_offset: base,
                        capacity,
                        key: [0, 0, 0, 0],
                    })
                },
            ),
        ] {
            let circuit = circuit_with(name, defs, vec![command]);
            let error = validate_static_state_accesses(4, &[circuit])
                .expect_err("an access past the last leaf must be rejected");
            let message = error.to_string();
            assert!(message.contains("state-tree sanity check failed"), "{name}: {message}");
            assert!(message.contains(name), "{name}: {message}");
            assert!(message.contains("leaf 16"), "{name}: {message}");
        }
    }

    #[test]
    fn static_state_validation_skips_dynamic_overflowing_and_unbounded_cases() {
        // An IMT command with zero capacity covers no leaf.
        let zero_capacity_defs = vec![constant_def(0, DPNOpType::Constant, 60), constant_def(1, DPNOpType::Constant, 0)];
        let base = zero_capacity_defs[0].get_combined_data_type_index();
        let capacity = zero_capacity_defs[1].get_combined_data_type_index();
        let zero_capacity = circuit_with(
            "zero_capacity",
            zero_capacity_defs,
            vec![DPNStateCmd::SetIMTContractStateValue(DPNStateCmdSetIMTContractStateValue {
                condition: 0,
                base_offset: base,
                capacity,
                key: [0, 0, 0, 0],
                value: [0, 0, 0, 0],
            })],
        );

        // capacity * 4 overflows u64, so the last leaf cannot be computed.
        let huge = u64::MAX / 4 + 1;
        let overflow_defs = vec![constant_def(0, DPNOpType::Constant, huge), constant_def(1, DPNOpType::Constant, huge)];
        let base = overflow_defs[0].get_combined_data_type_index();
        let capacity = overflow_defs[1].get_combined_data_type_index();
        let overflowing = circuit_with(
            "overflowing",
            overflow_defs,
            vec![DPNStateCmd::GetSelfUserCurrentIMTContractStateValue(DPNStateCmdGetSelfUserCurrentIMTContractStateValue {
                base_offset: base,
                capacity,
                key: [0, 0, 0, 0],
            })],
        );

        // A range whose end overflows cannot be checked either.
        let range_overflow_defs = vec![constant_def(0, DPNOpType::Constant, u64::MAX)];
        let start = range_overflow_defs[0].get_combined_data_type_index();
        let range_overflow = circuit_with(
            "range_overflow",
            range_overflow_defs,
            vec![DPNStateCmd::SetContractStateSlotRange(DPNStateCmdSetContractStateSlotRange {
                condition: 0,
                sub_slot_index: start,
                value: vec![0; 2],
            })],
        );

        // Indices computed at runtime (no constant definition) are dynamic.
        let dynamic = circuit_with(
            "dynamic",
            vec![],
            vec![DPNStateCmd::SetContractStateSlotHash(DPNStateCmdSetContractStateSlotHash {
                condition: 0,
                slot_index: 123_456,
                value: [0, 0, 0, 0],
            })],
        );

        assert!(validate_static_state_accesses(4, &[zero_capacity, overflowing, range_overflow, dynamic]).is_ok());

        // A height of 64 (or more) leaves has no representable capacity bound,
        // so even a slot constant of u64::MAX is accepted.
        let unbounded_defs = vec![constant_def(0, DPNOpType::Constant, u64::MAX)];
        let slot = unbounded_defs[0].get_combined_data_type_index();
        let unbounded = circuit_with(
            "unbounded",
            unbounded_defs,
            vec![DPNStateCmd::SetContractStateSlotHash(DPNStateCmdSetContractStateSlotHash {
                condition: 0,
                slot_index: slot,
                value: [0, 0, 0, 0],
            })],
        );
        assert!(validate_static_state_accesses(64, &[unbounded]).is_ok());
    }

    #[test]
    fn vfs_paths_map_to_their_pathbuf_forms() {
        let real = std::path::PathBuf::from("/tmp/pkg/src/main.psy");
        assert_eq!(source_vfs_to_pathbuf(&VfsPath::Real(real.clone())), real);
        assert_eq!(
            source_vfs_to_pathbuf(&VfsPath::Virtual("/virtual/src/main.psy".to_string())),
            std::path::PathBuf::from("/virtual/src/main.psy")
        );
    }

    #[test]
    fn function_name_extraction_handles_missing_and_invalid_markers() {
        assert_eq!(extract_function_name("no function", |_| true), None);
        assert_eq!(extract_function_name("fn () {}", |_| true), None);
        assert_eq!(extract_function_name("fn _foo42() {}", |_| true), Some("_foo42".to_string()));
        assert_eq!(extract_function_name("fn first() {} fn second() {}", |name| name == "second"), Some("second".to_string()));
        assert_eq!(extract_function_name("fn first() {}", |name| name == "missing"), None);
    }

    #[test]
    fn source_main_detection_does_not_match_prefixes() {
        assert!(source_has_function_name("fn main() {}", "main"));
        assert!(!source_has_function_name("fn main_extra() {}", "main"));
    }

    #[test]
    fn contract_method_extraction_collects_all_supported_attributes() {
        let source = "
            #[contract::write_method] fn write_one() {}
            #[contract::view_method] fn view_one() {}
            #[contract_method] fn plain() {}
            #[contract::write_method] fn write_one() {}
        ";
        let mut methods = Vec::new();
        extract_contract_method_names(source, &mut methods);
        assert_eq!(methods, vec!["write_one", "write_one", "view_one", "plain"]);
    }

    #[test]
    fn contract_method_extraction_ignores_attributes_without_function_names() {
        let mut methods = Vec::new();
        extract_contract_method_names("#[contract_method]\nlet value = 1;", &mut methods);
        assert!(methods.is_empty());
    }

    #[test]
    fn explicit_method_names_are_required_to_be_non_empty() {
        let workspace = Workspace {
            package: Package::default(),
            ..Workspace::default()
        };
        let error = resolve_workspace_method_names(
            &workspace,
            &CompileOptions {
                method_names: Some(Vec::new()),
                ..CompileOptions::default()
            },
        )
        .expect_err("an explicitly empty method list must be rejected");
        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn source_workspace_rejects_an_explicitly_empty_method_list() {
        let workspace = ResolvedSourceWorkspace {
            root_package: psy_package::PackageId::Virtual("root".to_string()),
            packages: Default::default(),
            source_map: SourceMap::new(),
        };
        let error = resolve_source_workspace_method_names(
            &workspace,
            &CompileOptions {
                method_names: Some(Vec::new()),
                ..CompileOptions::default()
            },
        )
        .expect_err("an explicitly empty source-workspace method list must be rejected");
        assert!(error.to_string().contains("must not be empty"));
    }

    fn workspace_with_source(source: &str) -> (Workspace, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("psy-dargo-methods-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let entry = root.join("src/main.psy");
        fs::create_dir_all(entry.parent().unwrap()).unwrap();
        fs::write(&entry, source).unwrap();
        let workspace = Workspace {
            root_dir: root.clone(),
            package: Package {
                root_dir: root.clone(),
                entry_path: std::path::PathBuf::from("src/main.psy"),
                ..Package::default()
            },
            ..Workspace::default()
        };
        (workspace, root)
    }

    #[test]
    fn workspace_method_discovery_handles_main_attributes_and_missing_methods() {
        let (workspace, root) = workspace_with_source("fn main() {}\n#[contract_method] fn ignored() {}");
        assert_eq!(resolve_workspace_method_names(&workspace, &CompileOptions::default()).unwrap(), vec!["main"]);
        fs::remove_dir_all(root).unwrap();

        let (workspace, root) = workspace_with_source(
            "#[contract::view_method] fn zeta() {}\n#[contract::write_method] fn alpha() {}\n#[contract_method] fn zeta() {}",
        );
        assert_eq!(
            resolve_workspace_method_names(&workspace, &CompileOptions::default()).unwrap(),
            vec!["alpha", "zeta"]
        );
        fs::remove_dir_all(root).unwrap();

        let (workspace, root) = workspace_with_source("fn helper() {}");
        let error = resolve_workspace_method_names(&workspace, &CompileOptions::default()).expect_err("missing methods must fail");
        assert!(error.to_string().contains("Unable to discover contract methods"));
        fs::remove_dir_all(root).unwrap();
    }

    /// Materializes `source` as a throwaway binary workspace via a manifest,
    /// mirroring how the CLI resolves a real project directory.
    fn manifest_workspace(source: &str, label: &str) -> (Workspace, std::path::PathBuf) {
        use std::time::{SystemTime, UNIX_EPOCH};

        use psy_package::resolve_workspace_from_toml;

        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let dir = std::env::temp_dir().join(format!("psy_dargo_compile_{label}_{nanos}"));
        fs::create_dir_all(dir.join("src")).expect("create src");
        fs::write(dir.join("src").join("app.psy"), source).expect("write app.psy");
        fs::write(
            dir.join("Dargo.toml"),
            "[package]\nname = \"compiledemo\"\ntype = \"bin\"\nentry = \"src/app.psy\"\nauthors = [\"\"]\n\n[dependencies]\n",
        )
        .expect("write Dargo.toml");
        let workspace = resolve_workspace_from_toml(&dir.join("Dargo.toml")).expect("resolve workspace");
        (workspace, dir)
    }

    fn reset_std_scope() {
    }

    #[test]
    #[serial_test::serial]
    fn compile_workspace_full_runs_debug_mode_with_explicit_methods() {
        let (workspace, dir) = manifest_workspace("fn main(a: Felt) -> Felt { return a + 1; }", "debug_opts");
        let result = super::compile_workspace_full(
            &workspace,
            &CompileOptions {
                contract_name: None,
                method_names: Some(vec!["main".to_string()]),
                entry_path: None,
                debug: true,
            },
        )
        .expect("compile with debug options must succeed");
        assert_eq!(result.circuit_definitions.len(), 1);
        reset_std_scope();
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    #[serial_test::serial]
    fn compile_command_run_writes_the_workspace_artifact_in_default_mode() {
        let (workspace, dir) = manifest_workspace("fn main(a: Felt) -> Felt { return a + 1; }", "run_cmd");
        let artifact_path = workspace.target_dir.join(format!("{}.json", workspace.package.name));

        super::run(CompileCommand { compile_options: CompileOptions::default() }, workspace)
            .expect("the compile command must compile the workspace and write the artifact");

        assert!(artifact_path.is_file(), "non-debug compiles must write the workspace artifact");
        let artifact = fs::read_to_string(&artifact_path).expect("artifact must be readable");
        assert!(artifact.contains("\"main\""), "artifact must embed the compiled method: {artifact}");

        reset_std_scope();
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    #[serial_test::serial]
    fn compile_source_workspace_full_compiles_resolver_packages_with_dependencies() {
        use std::sync::Arc;

        use psy_package::{resolve_source_workspace, MemoryResolver, PackageId, PackageSources, RelativeFilePath};

        let mut resolver = MemoryResolver::default();
        let files = |source: &'static str| {
            let mut map = std::collections::BTreeMap::new();
            map.insert(RelativeFilePath::new("src/main.psy".to_string()), Arc::<str>::from(source));
            map
        };
        resolver.insert_package(
            PackageId::Virtual("root".to_string()),
            PackageSources {
                manifest: Arc::<str>::from("[package]\nname = \"root\"\ntype = \"bin\"\nentry = \"src/main.psy\"\nauthors = [\"\"]\n\n[dependencies]\nutil = \"util\"\n"),
                files: files("fn main() -> Felt { return 1; }"),
            },
        );
        resolver.insert_package(
            PackageId::Virtual("util".to_string()),
            PackageSources {
                manifest: Arc::<str>::from("[package]\nname = \"util\"\ntype = \"lib\"\nentry = \"src/main.psy\"\nauthors = [\"\"]\n\n[dependencies]\n"),
                files: files("pub fn helper() -> Felt { return 41; }"),
            },
        );
        resolver.insert_dependency(PackageId::Virtual("root".to_string()), "util", PackageId::Virtual("util".to_string()));
        let source_workspace = resolve_source_workspace(PackageId::Virtual("root".to_string()), &resolver)
            .expect("source workspace must resolve");

        let result = super::compile_source_workspace_full(
            &source_workspace,
            &CompileOptions {
                method_names: Some(vec!["main".to_string()]),
                ..CompileOptions::default()
            },
        )
        .expect("in-memory workspace must compile");
        assert_eq!(result.circuit_definitions.len(), 1);
        reset_std_scope();
    }
}
