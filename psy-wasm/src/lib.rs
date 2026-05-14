use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, Once},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use psy_abi::{AbiExtractor, ContractCompatAbi};
use psy_common::Graph;
use psy_package::{resolve_source_workspace, MemoryResolver, PackageId, PackageSources, RelativeFilePath, VfsPath};
use psy_vm::dpn::{
    eval::executor::{ExecutionContext, ExecutionResult, InMemoryStateBackend, StateBackend, VmExecutor},
    ops::state_cmd::data::DPNStateCmd,
    vm::def::DPNFunctionCircuitDefinition,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const VFS_ROOT: &str = "/vfs";

#[derive(Serialize)]
struct JsCompileResult {
    success: bool,
    error: Option<String>,
    error_offset: Option<usize>,
    entry_path: Option<String>,
    compile_results: Option<serde_json::Value>,
    contract_code: Option<serde_json::Value>,
    abi: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct JsInterpretResult {
    success: bool,
    error: Option<String>,
    error_offset: Option<usize>,
    entry_path: Option<String>,
    execution_result: Option<serde_json::Value>,
    compile_results: Option<serde_json::Value>,
    contract_code: Option<serde_json::Value>,
    abi: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct JsContractFunctionCode {
    method_id: u32,
    num_inputs: usize,
    num_outputs: usize,
    vm_type: u32,
    code_base64: String,
}

#[derive(Serialize)]
struct JsContractCode {
    state_tree_height: u16,
    functions: Vec<JsContractFunctionCode>,
}

#[derive(Serialize)]
struct SerializeFallback<'a> {
    success: bool,
    error: &'a str,
}

static CHAIN: Mutex<Option<InMemoryChain>> = Mutex::new(None);
static LAST_COMPILE: Mutex<Option<CachedCompile>> = Mutex::new(None);
static INIT: Once = Once::new();

struct CachedCompile {
    state_tree_height: u16,
    abi: ContractCompatAbi,
    circuit_definitions: Vec<DPNFunctionCircuitDefinition>,
}

struct InMemoryChain {
    accounts: Vec<Account>,
    contracts: Vec<DeployedContract>,
    state: InMemoryStateBackend,
    transaction_log: Vec<TransactionRecord>,
    next_user_id: u64,
    next_contract_id: u64,
    checkpoint_id: u64,
}

#[derive(Clone)]
struct Account {
    user_id: u64,
    name: String,
}

struct DeployedContract {
    contract_id: u64,
    name: String,
    deployer_id: u64,
    abi: ContractCompatAbi,
    circuit_definitions: Vec<DPNFunctionCircuitDefinition>,
    state_tree_height: u16,
}

#[derive(Serialize, Clone)]
struct TransactionRecord {
    tx_id: u64,
    caller_id: u64,
    caller_name: String,
    contract_id: u64,
    contract_name: String,
    method_name: String,
    args: Vec<u64>,
    success: bool,
    failure_message: Option<String>,
    state_reads: usize,
    state_writes: usize,
    total_ops: usize,
    outputs: Vec<u64>,
}

#[derive(Serialize)]
struct JsAccount {
    user_id: u64,
    name: String,
}

#[derive(Serialize)]
struct JsDeployedContract {
    contract_id: u64,
    name: String,
    deployer_id: u64,
    abi: ContractCompatAbi,
}

#[derive(Serialize)]
struct JsDeployResult {
    success: bool,
    error: Option<String>,
    contract_id: Option<u64>,
}

#[derive(Serialize)]
struct JsCallResult {
    success: bool,
    error: Option<String>,
    failure_message: Option<String>,
    state_reads: Vec<psy_vm::dpn::eval::executor::StateRead>,
    state_writes: Vec<psy_vm::dpn::eval::executor::StateWrite>,
    outputs: Vec<u64>,
    total_ops: usize,
}

#[derive(Serialize)]
struct JsStateEntry {
    slot_index: u64,
    value: u64,
}

#[derive(Serialize)]
struct JsImtEntry {
    key: [u64; 4],
    value: [u64; 4],
}

#[wasm_bindgen(start)]
pub fn main() {
    INIT.call_once(|| {
        console_error_panic_hook::set_once();
        wasm_logger::init(wasm_logger::Config::default());
        wasm_tracing::set_as_global_default();
    });
}

#[wasm_bindgen]
pub fn init_logging() {
    main();
}

#[wasm_bindgen]
pub fn init_psy_ide() {
    main();
}

#[wasm_bindgen]
pub fn init_chain() {
    if let Ok(mut guard) = CHAIN.lock() {
        *guard = Some(InMemoryChain::new());
    }
}

#[wasm_bindgen]
pub fn reset_chain() {
    init_chain();
}

#[wasm_bindgen]
pub fn compile_source(source: &str) -> String {
    let entry_path = PathBuf::from(VFS_ROOT).join("src/main.psy");
    let files = vec![(entry_path.clone(), source.to_string())];
    compile_vfs_project(files, entry_path)
}

#[wasm_bindgen]
pub fn compile_project(files_json: &str) -> String {
    match parse_project_input(files_json) {
        Ok(project_input) => {
            match build_vfs_files(project_input) {
                Ok((entry_path, method_names, vfs_files)) => {
                    compile_vfs_project_with_methods(vfs_files, entry_path, method_names)
                }
                Err(error) => serialize_result(JsCompileResult {
                    success: false,
                    error: Some(error),
                    error_offset: None,
                    entry_path: None,
                    compile_results: None,
                    contract_code: None,
                    abi: None,
                }),
            }
        }
        Err(error) => serialize_result(JsCompileResult {
            success: false,
            error: Some(format!("Invalid files JSON: {error}")),
            error_offset: None,
            entry_path: None,
            compile_results: None,
            contract_code: None,
            abi: None,
        }),
    }
}

#[wasm_bindgen]
pub fn interpret_source(source: &str, request_json: &str) -> String {
    let request = match parse_interpret_request(request_json) {
        Ok(request) => request,
        Err(error) => return serialize_interpret_error(format!("Invalid interpret request JSON: {error}"), None, None, None, None),
    };

    let entry_path = PathBuf::from(VFS_ROOT).join("src/main.psy");
    let files = vec![(entry_path.clone(), source.to_string())];
    interpret_vfs_project(files, entry_path, request)
}

#[wasm_bindgen]
pub fn interpret_project(files_json: &str, request_json: &str) -> String {
    let project_input = match parse_project_input(files_json) {
        Ok(input) => input,
        Err(error) => return serialize_interpret_error(format!("Invalid files JSON: {error}"), None, None, None, None),
    };
    let request = match parse_interpret_request(request_json) {
        Ok(request) => request,
        Err(error) => return serialize_interpret_error(format!("Invalid interpret request JSON: {error}"), None, None, None, None),
    };

    match build_vfs_files(project_input) {
        Ok((entry_path, _, vfs_files)) => interpret_vfs_project(vfs_files, entry_path, request),
        Err(error) => serialize_interpret_error(error, None, None, None, None),
    }
}

#[wasm_bindgen]
pub fn compile_dargo_project(project_json: &str) -> String {
    let input: DargoProjectInput = match serde_json::from_str(project_json) {
        Ok(input) => input,
        Err(error) => {
            return serialize_result(JsCompileResult {
                success: false,
                error: Some(format!("Invalid dargo project JSON: {error}")),
                error_offset: None,
                entry_path: None,
                compile_results: None,
                contract_code: None,
                abi: None,
            });
        }
    };

    let root_package = PackageId::Virtual(input.root.clone());
    let mut resolver = MemoryResolver::default();
    for package in &input.packages {
        let files = package
            .files
            .iter()
            .map(|(path, source)| (RelativeFilePath::new(path.clone()), Arc::<str>::from(source.as_str())))
            .collect::<std::collections::BTreeMap<_, _>>();
        resolver.insert_package(
            PackageId::Virtual(package.id.clone()),
            PackageSources {
                manifest: Arc::<str>::from(package.manifest.as_str()),
                files,
            },
        );
        for (dep_name, dep_package_id) in &package.dependencies {
            resolver.insert_dependency(
                PackageId::Virtual(package.id.clone()),
                dep_name.clone(),
                PackageId::Virtual(dep_package_id.clone()),
            );
        }
    }

    let workspace = match resolve_source_workspace(root_package.clone(), &resolver) {
        Ok(workspace) => workspace,
        Err(error) => {
            return serialize_result(JsCompileResult {
                success: false,
                error: Some(error.to_string()),
                error_offset: None,
                entry_path: None,
                compile_results: None,
                contract_code: None,
                abi: None,
            });
        }
    };

    let method_names = if let Some(method_names) = input.method_names {
        if method_names.is_empty() {
            return serialize_result(JsCompileResult {
                success: false,
                error: Some("method_names must not be empty when provided".to_string()),
                error_offset: None,
                entry_path: None,
                compile_results: None,
                contract_code: None,
                abi: None,
            });
        }
        method_names
    } else {
        vec!["main".to_string()]
    };
    let mut crate_path_graph: Graph<PathBuf> = Graph::new();
    let package_entries = workspace
        .packages
        .iter()
        .filter_map(|(package_id, package)| {
            workspace
                .source_map
                .path(package.entry_file_id)
                .map(|entry| (package_id.clone(), source_vfs_to_pathbuf(entry)))
        })
        .collect::<HashMap<_, _>>();
    for package in workspace.packages.values() {
        let Some(entry_path) = package_entries.get(&package.package_id) else {
            continue;
        };
        if !crate_path_graph.contains_node(entry_path) {
            crate_path_graph.add_node(entry_path.clone());
        }
        for dep_pkg_id in package.dependency_packages.values() {
            if let Some(dep_entry_path) = package_entries.get(dep_pkg_id) {
                crate_path_graph.add_edge(entry_path.clone(), dep_entry_path.clone());
            }
        }
    }

    let root_entry_path = match package_entries.get(&root_package) {
        Some(path) => path.clone(),
        None => {
            return serialize_result(JsCompileResult {
                success: false,
                error: Some("Root package entry not found".to_string()),
                error_offset: None,
                entry_path: None,
                compile_results: None,
                contract_code: None,
                abi: None,
            });
        }
    };

    let vfs_shared_files = workspace
        .source_map
        .snapshot()
        .into_iter()
        .map(|(_, path, text)| (source_vfs_to_pathbuf(&path), text))
        .collect::<Vec<_>>();
    let source_index = vfs_shared_files
        .iter()
        .map(|(path, content)| (path.display().to_string(), Arc::<str>::from(content.as_ref())))
        .collect::<HashMap<_, _>>();

    match psy_interpreter::interpret_vfs_files(input.contract_name, method_names, crate_path_graph, vfs_shared_files) {
        Ok(mut result) => {
            let compile_results = match serde_json::to_value(&result.compile_results) {
                Ok(value) => value,
                Err(error) => {
                    return serialize_result(JsCompileResult {
                        success: false,
                        error: Some(format!("Failed to serialize compile results: {error}")),
                        error_offset: None,
                        entry_path: Some(root_entry_path.display().to_string()),
                        compile_results: None,
                        contract_code: None,
                        abi: None,
                    });
                }
            };

            let compile_results_for_metadata = result.compile_results.clone();
            let state_tree_height = derive_state_tree_height(&compile_results_for_metadata);
            let contract_code = match extract_contract_code(&compile_results_for_metadata, state_tree_height) {
                Ok(value) => Some(value),
                Err(error) => {
                    tracing::warn!("Contract code extraction failed: {error}");
                    None
                }
            };
            let abi = match extract_contract_abi(&mut result, state_tree_height, &compile_results_for_metadata) {
                Ok(value) => Some(value),
                Err(error) => {
                    tracing::warn!("ABI extraction failed: {error}");
                    None
                }
            };
            let abi_value = abi
                .as_ref()
                .and_then(|abi| serde_json::to_value(abi).map_err(|error| tracing::warn!("ABI serialization failed: {error}")).ok());

            if let Some(abi) = abi.clone() {
                cache_compile(CachedCompile {
                    state_tree_height,
                    abi,
                    circuit_definitions: compile_results_for_metadata,
                });
            }

            serialize_result(JsCompileResult {
                success: true,
                error: None,
                error_offset: None,
                entry_path: Some(root_entry_path.display().to_string()),
                compile_results: Some(compile_results),
                contract_code,
                abi: abi_value,
            })
        }
        Err(error) => {
            let error_text = format!("{error:#}");
            serialize_result(JsCompileResult {
                success: false,
                error: Some(error_text.clone()),
                error_offset: extract_error_offset(&error_text, &source_index),
                entry_path: Some(root_entry_path.display().to_string()),
                compile_results: None,
                contract_code: None,
                abi: None,
            })
        }
    }
}

#[wasm_bindgen]
pub fn create_account(name: &str) -> String {
    match with_chain(|chain| {
        let account = chain.create_account(name);
        Ok(JsAccount {
            user_id: account.user_id,
            name: account.name,
        })
    }) {
        Ok(account) => serialize_json(&account),
        Err(error) => serialize_error_message(error),
    }
}

#[wasm_bindgen]
pub fn get_accounts() -> String {
    match with_chain(|chain| {
        Ok(chain
            .accounts
            .iter()
            .map(|account| JsAccount {
                user_id: account.user_id,
                name: account.name.clone(),
            })
            .collect::<Vec<_>>())
    }) {
        Ok(accounts) => serialize_json(&accounts),
        Err(error) => serialize_error_message(error),
    }
}

#[wasm_bindgen]
pub fn deploy_contract(deployer_id: u64) -> String {
    let compile_output = match LAST_COMPILE.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(output) => CachedCompile {
                state_tree_height: output.state_tree_height,
                abi: output.abi.clone(),
                circuit_definitions: output.circuit_definitions.clone(),
            },
            None => {
                return serialize_json(&JsDeployResult {
                    success: false,
                    error: Some("No compiled contract. Compile first.".to_string()),
                    contract_id: None,
                });
            }
        },
        Err(error) => {
            return serialize_json(&JsDeployResult {
                success: false,
                error: Some(error.to_string()),
                contract_id: None,
            });
        }
    };

    match with_chain(|chain| {
        if chain.find_account(deployer_id).is_none() {
            return Err(format!("Account with ID {deployer_id} not found"));
        }

        let contract_id = chain.next_contract_id;
        chain.next_contract_id += 1;
        chain.contracts.push(DeployedContract {
            contract_id,
            name: compile_output.abi.contract_name.clone(),
            deployer_id,
            abi: compile_output.abi,
            circuit_definitions: compile_output.circuit_definitions,
            state_tree_height: compile_output.state_tree_height,
        });

        Ok(JsDeployResult {
            success: true,
            error: None,
            contract_id: Some(contract_id),
        })
    }) {
        Ok(result) => serialize_json(&result),
        Err(error) => serialize_json(&JsDeployResult {
            success: false,
            error: Some(error),
            contract_id: None,
        }),
    }
}

#[wasm_bindgen]
pub fn get_contracts() -> String {
    match with_chain(|chain| {
        Ok(chain
            .contracts
            .iter()
            .map(|contract| JsDeployedContract {
                contract_id: contract.contract_id,
                name: contract.name.clone(),
                deployer_id: contract.deployer_id,
                abi: contract.abi.clone(),
            })
            .collect::<Vec<_>>())
    }) {
        Ok(contracts) => serialize_json(&contracts),
        Err(error) => serialize_error_message(error),
    }
}

#[wasm_bindgen]
pub fn call_contract(caller_id: u64, contract_id: u64, method_name: &str, args_json: &str) -> String {
    let args: Vec<u64> = match serde_json::from_str(args_json) {
        Ok(args) => args,
        Err(error) => {
            return serialize_json(&JsCallResult {
                success: false,
                error: Some(format!("Invalid args: {error}")),
                failure_message: None,
                state_reads: vec![],
                state_writes: vec![],
                outputs: vec![],
                total_ops: 0,
            });
        }
    };

    match with_chain(|chain| {
        let contract = chain
            .find_contract(contract_id)
            .ok_or_else(|| format!("Contract {contract_id} not found"))?;
        let method = contract
            .abi
            .methods
            .iter()
            .find(|method| method.name == method_name)
            .ok_or_else(|| format!("Method '{method_name}' not found"))?;
        let circuit = contract
            .circuit_definitions
            .iter()
            .find(|circuit| circuit.method_id == method.method_id)
            .ok_or_else(|| format!("Circuit for method '{method_name}' not found"))?;

        let caller_name = chain
            .find_account(caller_id)
            .map(|account| account.name.clone())
            .unwrap_or_else(|| format!("User {caller_id}"));
        let contract_name = contract.name.clone();
        let context = ExecutionContext {
            user_id: caller_id,
            contract_id,
            caller_contract_id: 0,
            checkpoint_id: chain.checkpoint_id,
            nonce: chain.transaction_log.len() as u64,
            user_public_key_hash: [0; 4],
        };

        let mut executor = VmExecutor::new(chain.state.clone());
        let result = executor
            .execute(circuit, &context, &args)
            .map_err(|error| format!("Execution error: {error:#}"))?;

        if result.success {
            chain.state.apply_overlay(executor.write_overlay());
            chain.state.apply_imt_overlay(executor.imt_write_overlay());
        }

        chain.transaction_log.push(TransactionRecord {
            tx_id: chain.transaction_log.len() as u64 + 1,
            caller_id,
            caller_name,
            contract_id,
            contract_name,
            method_name: method_name.to_string(),
            args: args.clone(),
            success: result.success,
            failure_message: result.failure.as_ref().map(|failure| failure.message.clone()),
            state_reads: result.state_reads.len(),
            state_writes: result.state_writes.len(),
            total_ops: result.op_counts.total_operations,
            outputs: result.outputs.clone(),
        });

        Ok(JsCallResult {
            success: result.success,
            error: None,
            failure_message: result.failure.as_ref().map(|failure| failure.message.clone()),
            state_reads: result.state_reads,
            state_writes: result.state_writes,
            outputs: result.outputs,
            total_ops: result.op_counts.total_operations,
        })
    }) {
        Ok(result) => serialize_json(&result),
        Err(error) => serialize_json(&JsCallResult {
            success: false,
            error: Some(error),
            failure_message: None,
            state_reads: vec![],
            state_writes: vec![],
            outputs: vec![],
            total_ops: 0,
        }),
    }
}

#[wasm_bindgen]
pub fn read_contract_state(contract_id: u64, user_id: u64) -> String {
    match with_chain(|chain| {
        let contract = chain
            .find_contract(contract_id)
            .ok_or_else(|| format!("Contract {contract_id} not found"))?;
        let mut entries = Vec::new();
        let total_slots = 1u64 << contract.state_tree_height;
        for slot in 0..total_slots.min(256) {
            let value = chain.state.get_contract_slot(user_id, contract_id, slot).map_err(|error| error.to_string())?;
            if value != 0 {
                entries.push(JsStateEntry { slot_index: slot, value });
            }
        }
        Ok(entries)
    }) {
        Ok(entries) => serialize_json(&entries),
        Err(error) => serialize_error_message(error),
    }
}

#[wasm_bindgen]
pub fn read_imt_state(contract_id: u32, user_id: u32) -> String {
    match with_chain(|chain| {
        let contract_id = contract_id as u64;
        let user_id = user_id as u64;
        chain
            .find_contract(contract_id)
            .ok_or_else(|| format!("Contract {contract_id} not found"))?;
        Ok(chain
            .state
            .imt_entries_for(user_id, contract_id)
            .map(|(key, value)| JsImtEntry { key: *key, value: *value })
            .collect::<Vec<_>>())
    }) {
        Ok(entries) => serialize_json(&entries),
        Err(error) => serialize_error_message(error),
    }
}

#[wasm_bindgen]
pub fn get_transaction_log() -> String {
    match with_chain(|chain| Ok(chain.transaction_log.clone())) {
        Ok(log) => serialize_json(&log),
        Err(error) => serialize_error_message(error),
    }
}

#[wasm_bindgen]
pub fn get_contract_abi(contract_id: u64) -> String {
    match with_chain(|chain| {
        let contract = chain
            .find_contract(contract_id)
            .ok_or_else(|| format!("Contract {contract_id} not found"))?;
        Ok(contract.abi.clone())
    }) {
        Ok(abi) => serialize_json(&abi),
        Err(error) => serialize_error_message(error),
    }
}

#[derive(Deserialize)]
struct ProjectInput {
    entry: Option<Vec<String>>,
    method_names: Option<Vec<String>>,
    files: Vec<(Vec<String>, String)>,
}

#[derive(Deserialize)]
struct DargoProjectInput {
    root: String,
    #[serde(default)]
    method_names: Option<Vec<String>>,
    #[serde(default)]
    contract_name: Option<String>,
    packages: Vec<DargoPackageInput>,
}

#[derive(Deserialize)]
struct DargoPackageInput {
    id: String,
    manifest: String,
    files: HashMap<String, String>,
    #[serde(default)]
    dependencies: HashMap<String, String>,
}

#[derive(Deserialize)]
struct InterpretRequest {
    #[serde(default)]
    method_name: Option<String>,
    #[serde(default)]
    inputs: Vec<u64>,
    #[serde(default)]
    execution_context: Option<ExecutionContextInput>,
    #[serde(default)]
    initial_state: Option<InitialStateInput>,
}

#[derive(Deserialize)]
struct ExecutionContextInput {
    #[serde(default)]
    user_id: Option<u64>,
    #[serde(default)]
    contract_id: Option<u64>,
    #[serde(default)]
    caller_contract_id: Option<u64>,
    #[serde(default)]
    checkpoint_id: Option<u64>,
    #[serde(default)]
    nonce: Option<u64>,
    #[serde(default)]
    user_public_key_hash: Option<[u64; 4]>,
}

#[derive(Deserialize, Default)]
struct InitialStateInput {
    #[serde(default)]
    slots: Vec<SlotValueInput>,
    #[serde(default)]
    hashes: Vec<HashValueInput>,
    #[serde(default)]
    deployers: Vec<DeployerInput>,
    #[serde(default)]
    checkpoint_stats: Vec<CheckpointVecInput>,
    #[serde(default)]
    contract_leaves: Vec<ContractLeafInput>,
    #[serde(default)]
    checkpoint_global_state_roots: Vec<CheckpointVecInput>,
    #[serde(default)]
    imt: Vec<ImtValueInput>,
}

#[derive(Deserialize)]
struct SlotValueInput {
    user_id: u64,
    contract_id: u64,
    slot_index: u64,
    value: u64,
}

#[derive(Deserialize)]
struct HashValueInput {
    user_id: u64,
    contract_id: u64,
    slot_index: u64,
    value: [u64; 4],
}

#[derive(Deserialize)]
struct DeployerInput {
    contract_id: u64,
    deployer: [u64; 4],
}

#[derive(Deserialize)]
struct CheckpointVecInput {
    checkpoint_id: u64,
    values: Vec<u64>,
}

#[derive(Deserialize)]
struct ContractLeafInput {
    contract_id: u64,
    values: Vec<u64>,
}

#[derive(Deserialize)]
struct ImtValueInput {
    user_id: u64,
    contract_id: u64,
    key: [u64; 4],
    value: [u64; 4],
}

impl InMemoryChain {
    fn new() -> Self {
        Self {
            accounts: Vec::new(),
            contracts: Vec::new(),
            state: InMemoryStateBackend::new(),
            transaction_log: Vec::new(),
            next_user_id: 1,
            next_contract_id: 1,
            checkpoint_id: 100,
        }
    }

    fn create_account(&mut self, name: &str) -> Account {
        let account = Account {
            user_id: self.next_user_id,
            name: name.to_string(),
        };
        self.next_user_id += 1;
        self.accounts.push(account.clone());
        account
    }

    fn find_account(&self, user_id: u64) -> Option<&Account> {
        self.accounts.iter().find(|account| account.user_id == user_id)
    }

    fn find_contract(&self, contract_id: u64) -> Option<&DeployedContract> {
        self.contracts.iter().find(|contract| contract.contract_id == contract_id)
    }
}

fn with_chain<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut InMemoryChain) -> Result<R, String>,
{
    let mut guard = CHAIN.lock().map_err(|error| error.to_string())?;
    let chain = guard
        .as_mut()
        .ok_or_else(|| "Chain not initialized. Call init_chain() first.".to_string())?;
    f(chain)
}

fn parse_project_input(files_json: &str) -> Result<ProjectInput, serde_json::Error> {
    serde_json::from_str::<ProjectInput>(files_json)
}

fn parse_interpret_request(request_json: &str) -> Result<InterpretRequest, serde_json::Error> {
    serde_json::from_str::<InterpretRequest>(request_json)
}

fn cache_compile(cached: CachedCompile) {
    if let Ok(mut guard) = LAST_COMPILE.lock() {
        *guard = Some(cached);
    }
}

fn compile_vfs_project(files: Vec<(PathBuf, String)>, entry_path: PathBuf) -> String {
    compile_vfs_project_with_contract(files, entry_path, vec!["main".to_string()], None)
}

fn interpret_vfs_project(files: Vec<(PathBuf, String)>, entry_path: PathBuf, request: InterpretRequest) -> String {
    let method_name = request.method_name.clone().unwrap_or_else(|| "main".to_string());
    let source_index = build_source_index(&files);
    let vfs_shared_files = files
        .into_iter()
        .map(|(path, content)| (path, Arc::<str>::from(content)))
        .collect();

    let mut crate_path_graph = Graph::new();
    crate_path_graph.add_node(entry_path.clone());

    match psy_interpreter::interpret_vfs_files(None, vec![method_name.clone()], crate_path_graph, vfs_shared_files) {
        Ok(mut result) => {
            let compile_results_value = match serde_json::to_value(&result.compile_results) {
                Ok(value) => value,
                Err(error) => {
                    return serialize_interpret_error(
                        format!("Failed to serialize compile results: {error}"),
                        Some(entry_path.display().to_string()),
                        None,
                        None,
                        None,
                    )
                }
            };

            let compile_results_for_metadata = result.compile_results.clone();
            let state_tree_height = derive_state_tree_height(&compile_results_for_metadata);
            let contract_code = extract_contract_code(&compile_results_for_metadata, state_tree_height).ok();
            let abi = extract_contract_abi(&mut result, state_tree_height, &compile_results_for_metadata).ok();
            let abi_value = abi
                .as_ref()
                .and_then(|abi| serde_json::to_value(abi).map_err(|error| tracing::warn!("ABI serialization failed: {error}")).ok());

            let Some(circuit) = result.compile_results.first() else {
                return serialize_interpret_error(
                    "No compiled method available for interpretation".to_string(),
                    Some(entry_path.display().to_string()),
                    None,
                    contract_code,
                    abi_value,
                );
            };

            match execute_circuit(circuit, request) {
                Ok(execution_result) => {
                    let execution_value = match serde_json::to_value(execution_result) {
                        Ok(value) => value,
                        Err(error) => {
                            return serialize_interpret_error(
                                format!("Failed to serialize execution result: {error}"),
                                Some(entry_path.display().to_string()),
                                Some(compile_results_value),
                                contract_code.clone(),
                                abi_value,
                            )
                        }
                    };

                    serialize_interpret_result(JsInterpretResult {
                        success: true,
                        error: None,
                        error_offset: None,
                        entry_path: Some(entry_path.display().to_string()),
                        execution_result: Some(execution_value),
                        compile_results: Some(compile_results_value),
                        contract_code,
                        abi: abi_value,
                    })
                }
                Err(error) => serialize_interpret_error(
                    error,
                    Some(entry_path.display().to_string()),
                    Some(compile_results_value),
                    contract_code,
                    abi_value,
                ),
            }
        }
        Err(error) => {
            let error_text = format!("{error:#}");
            serialize_interpret_result(JsInterpretResult {
                success: false,
                error: Some(error_text.clone()),
                error_offset: extract_error_offset(&error_text, &source_index),
                entry_path: Some(entry_path.display().to_string()),
                execution_result: None,
                compile_results: None,
                contract_code: None,
                abi: None,
            })
        }
    }
}

fn compile_vfs_project_with_methods(files: Vec<(PathBuf, String)>, entry_path: PathBuf, method_names: Vec<String>) -> String {
    compile_vfs_project_with_contract(files, entry_path, method_names, None)
}

fn compile_vfs_project_with_contract(
    files: Vec<(PathBuf, String)>,
    entry_path: PathBuf,
    method_names: Vec<String>,
    contract_name: Option<String>,
) -> String {
    let mut crate_path_graph = Graph::new();
    crate_path_graph.add_node(entry_path.clone());
    let source_index = build_source_index(&files);
    let vfs_shared_files = files
        .into_iter()
        .map(|(path, content)| (path, Arc::<str>::from(content)))
        .collect();

    match psy_interpreter::interpret_vfs_files(contract_name, method_names, crate_path_graph, vfs_shared_files) {
        Ok(mut result) => {
            let compile_results = match serde_json::to_value(&result.compile_results) {
                Ok(value) => value,
                Err(error) => {
                    return serialize_result(JsCompileResult {
                        success: false,
                        error: Some(format!("Failed to serialize compile results: {error}")),
                        error_offset: None,
                        entry_path: Some(entry_path.display().to_string()),
                        compile_results: None,
                        contract_code: None,
                        abi: None,
                    });
                }
            };

            let compile_results_for_metadata = result.compile_results.clone();
            let state_tree_height = derive_state_tree_height(&compile_results_for_metadata);
            let contract_code = match extract_contract_code(&compile_results_for_metadata, state_tree_height) {
                Ok(value) => Some(value),
                Err(error) => {
                    tracing::warn!("Contract code extraction failed: {error}");
                    None
                }
            };

            let abi = match extract_contract_abi(&mut result, state_tree_height, &compile_results_for_metadata) {
                Ok(value) => Some(value),
                Err(error) => {
                    tracing::warn!("ABI extraction failed: {error}");
                    None
                }
            };
            let abi_value = abi
                .as_ref()
                .and_then(|abi| serde_json::to_value(abi).map_err(|error| tracing::warn!("ABI serialization failed: {error}")).ok());

            if let Some(abi) = abi.clone() {
                cache_compile(CachedCompile {
                    state_tree_height,
                    abi,
                    circuit_definitions: compile_results_for_metadata.clone(),
                });
            }

            serialize_result(JsCompileResult {
                success: true,
                error: None,
                error_offset: None,
                entry_path: Some(entry_path.display().to_string()),
                compile_results: Some(compile_results),
                contract_code,
                abi: abi_value,
            })
        }
        Err(error) => {
            let error_text = format!("{error:#}");
            serialize_result(JsCompileResult {
                success: false,
                error: Some(error_text.clone()),
                error_offset: extract_error_offset(&error_text, &source_index),
                entry_path: Some(entry_path.display().to_string()),
                compile_results: None,
                contract_code: None,
                abi: None,
            })
        }
    }
}

fn build_source_index(files: &[(PathBuf, String)]) -> HashMap<String, Arc<str>> {
    files
        .iter()
        .map(|(path, content)| (path.display().to_string(), Arc::<str>::from(content.as_str())))
        .collect()
}

fn source_vfs_to_pathbuf(path: &VfsPath) -> PathBuf {
    match path {
        VfsPath::Real(path) => path.clone(),
        VfsPath::Virtual(path) => PathBuf::from(path),
    }
}

fn build_vfs_files(input: ProjectInput) -> Result<(PathBuf, Vec<String>, Vec<(PathBuf, String)>), String> {
    if input.files.is_empty() {
        return Err("Project must contain at least one file".to_string());
    }

    let explicit_entry_path = input
        .entry
        .as_ref()
        .map(|parts| module_parts_to_path(parts))
        .unwrap_or_else(|| module_parts_to_path(&[]));
    let mut saw_explicit_entry = false;
    let mut vfs_files = Vec::with_capacity(input.files.len());

    for (parts, content) in input.files {
        let path = module_parts_to_path(&parts);
        if explicit_entry_path == path {
            saw_explicit_entry = true;
        }
        vfs_files.push((path, content));
    }

    if !saw_explicit_entry {
        return Err("Explicit entry file was not found in project files".to_string());
    }

    let entry_path = explicit_entry_path;

    let method_names = resolve_method_names(&entry_path, input.method_names, &vfs_files)?;

    Ok((entry_path, method_names, vfs_files))
}

fn ide_module_parts_to_path(parts: &[String]) -> PathBuf {
    let mut path = PathBuf::from(VFS_ROOT).join("src");
    if parts.is_empty() {
        path.push("main.psy");
        return path;
    }

    for (index, part) in parts.iter().enumerate() {
        let is_last = index + 1 == parts.len();
        if is_last {
            if part.ends_with(".psy") {
                path.push(part);
            } else {
                path.push(format!("{part}.psy"));
            }
        } else {
            path.push(part);
        }
    }
    path
}

fn resolve_method_names(
    entry_path: &PathBuf,
    method_names: Option<Vec<String>>,
    _vfs_files: &[(PathBuf, String)],
) -> Result<Vec<String>, String> {
    if let Some(method_names) = method_names {
        if method_names.is_empty() {
            return Err("method_names must not be empty when provided".to_string());
        }
        return Ok(method_names);
    }

    // No frontend-side method discovery. Compiler/interpreter resolves
    // contract methods from AST/semantic data.
    let is_main_entry = entry_path.file_stem().and_then(|s| s.to_str()) == Some("main");
    if !is_main_entry {
        return Err(format!(
            "Unable to resolve method names for {} without explicit method_names",
            entry_path.display()
        ));
    }
    Ok(Vec::new())
}

fn module_parts_to_path(parts: &[String]) -> PathBuf {
    let mut path = PathBuf::from(VFS_ROOT).join("src");
    if parts.is_empty() {
        path.push("main.psy");
        return path;
    }

    for (index, part) in parts.iter().enumerate() {
        let is_last = index + 1 == parts.len();
        if is_last {
            if part.ends_with(".psy") {
                path.push(part);
            } else {
                path.push(format!("{part}.psy"));
            }
        } else {
            path.push(part);
        }
    }
    path
}

fn execute_circuit(circuit: &DPNFunctionCircuitDefinition, request: InterpretRequest) -> Result<ExecutionResult, String> {
    let mut state = InMemoryStateBackend::new();
    if let Some(initial_state) = request.initial_state {
        hydrate_initial_state(&mut state, initial_state);
    }

    let context = request
        .execution_context
        .map(ExecutionContext::from)
        .unwrap_or_else(default_execution_context);

    let mut executor = VmExecutor::new(state);
    executor.execute(circuit, &context, &request.inputs).map_err(|error| error.to_string())
}

fn hydrate_initial_state(state: &mut InMemoryStateBackend, initial_state: InitialStateInput) {
    for slot in initial_state.slots {
        state.set_slot(slot.user_id, slot.contract_id, slot.slot_index, slot.value);
    }
    for hash in initial_state.hashes {
        state.set_hash(hash.user_id, hash.contract_id, hash.slot_index, hash.value);
    }
    for deployer in initial_state.deployers {
        state.set_deployer(deployer.contract_id, deployer.deployer);
    }
    for checkpoint_stats in initial_state.checkpoint_stats {
        state.set_checkpoint_stats(checkpoint_stats.checkpoint_id, checkpoint_stats.values);
    }
    for contract_leaf in initial_state.contract_leaves {
        state.set_contract_leaf(contract_leaf.contract_id, contract_leaf.values);
    }
    for roots in initial_state.checkpoint_global_state_roots {
        state.set_checkpoint_global_state_roots(roots.checkpoint_id, roots.values);
    }
    for imt in initial_state.imt {
        state.set_imt(imt.user_id, imt.contract_id, imt.key, imt.value);
    }
}

fn default_execution_context() -> ExecutionContext {
    ExecutionContext {
        user_id: 0,
        contract_id: 0,
        caller_contract_id: 0,
        checkpoint_id: 0,
        nonce: 0,
        user_public_key_hash: [0; 4],
    }
}

impl From<ExecutionContextInput> for ExecutionContext {
    fn from(value: ExecutionContextInput) -> Self {
        ExecutionContext {
            user_id: value.user_id.unwrap_or(0),
            contract_id: value.contract_id.unwrap_or(0),
            caller_contract_id: value.caller_contract_id.unwrap_or(0),
            checkpoint_id: value.checkpoint_id.unwrap_or(0),
            nonce: value.nonce.unwrap_or(0),
            user_public_key_hash: value.user_public_key_hash.unwrap_or([0; 4]),
        }
    }
}

fn extract_contract_abi(
    result: &mut psy_interpreter::InterpretResult,
    state_tree_height: u16,
    compile_results: &[DPNFunctionCircuitDefinition],
) -> Result<ContractCompatAbi, String> {
    let program = &mut result.ctx.program;
    let method_metadata = compile_results
        .iter()
        .map(|function| (function.name.clone(), (function.method_id, function.is_view_function())))
        .collect::<HashMap<_, _>>();
    AbiExtractor::new("contract".to_string())
        .extract_contract_abi(program, state_tree_height, &method_metadata)
        .map_err(|error| error.to_string())
}

fn extract_contract_code(
    compile_results: &[DPNFunctionCircuitDefinition],
    state_tree_height: u16,
) -> Result<serde_json::Value, String> {
    let functions = compile_results
        .iter()
        .map(|circuit| {
            let bytes = bincode::serialize(circuit).map_err(|error| error.to_string())?;
            Ok(JsContractFunctionCode {
                method_id: circuit.method_id,
                num_inputs: circuit.circuit_inputs.len(),
                num_outputs: circuit.circuit_outputs.len(),
                vm_type: 0,
                code_base64: BASE64_STANDARD.encode(bytes),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    serde_json::to_value(JsContractCode {
        state_tree_height,
        functions,
    })
    .map_err(|error| error.to_string())
}

fn derive_state_tree_height(compile_results: &[DPNFunctionCircuitDefinition]) -> u16 {
    let mut max_slot = None::<u64>;

    for circuit in compile_results {
        for command in &circuit.state_commands {
            let slot = match command {
                DPNStateCmd::SetContractStateSlotHash(cmd) => Some(cmd.slot_index),
                DPNStateCmd::SetContractStateSlotSingle(cmd) => Some(cmd.sub_slot_index),
                DPNStateCmd::SetContractStateSlotRange(cmd) => Some(cmd.sub_slot_index.saturating_add(cmd.value.len().saturating_sub(1) as u64)),
                DPNStateCmd::SetIMTContractStateValue(cmd) => {
                    let span = cmd.capacity.saturating_mul(4);
                    Some(cmd.base_offset.saturating_add(span.saturating_sub(1)))
                }
                _ => None,
            };

            if let Some(slot) = slot {
                max_slot = Some(max_slot.map_or(slot, |current| current.max(slot)));
            }
        }
    }

    let computed = match max_slot {
        Some(max_slot) => ceil_log2(max_slot.saturating_add(1)),
        None => 0,
    };

    computed.max(4)
}

fn ceil_log2(value: u64) -> u16 {
    if value <= 1 {
        return 0;
    }
    (u64::BITS - (value - 1).leading_zeros()) as u16
}

fn extract_error_offset(error_msg: &str, sources: &HashMap<String, Arc<str>>) -> Option<usize> {
    for pattern in ["at offset ", "offset "] {
        if let Some(pos) = error_msg.rfind(pattern) {
            let after = &error_msg[pos + pattern.len()..];
            let number: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(offset) = number.parse::<usize>() {
                return Some(offset);
            }
        }
    }

    extract_error_offset_from_line_col(error_msg, sources)
}

fn extract_error_offset_from_line_col(error_msg: &str, sources: &HashMap<String, Arc<str>>) -> Option<usize> {
    let marker = "[ ";
    let start = error_msg.find(marker)? + marker.len();
    let rest = &error_msg[start..];
    let end = rest.find(" ]")?;
    let location = &rest[..end];
    let (path_text, line, column) = parse_location_triplet(location)?;
    let source = sources.get(&path_text)?;
    line_col_to_offset(source, line, column)
}

fn parse_location_triplet(location: &str) -> Option<(String, usize, usize)> {
    let mut parts = location.rsplitn(3, ':');
    let column = parts.next()?.trim().parse::<usize>().ok()?;
    let line = parts.next()?.trim().parse::<usize>().ok()?;
    let path = parts.next()?.trim().to_string();
    Some((path, line, column))
}

fn line_col_to_offset(source: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut current_line = 1usize;
    let mut current_col = 1usize;

    for (offset, ch) in source.char_indices() {
        if current_line == line && current_col == column {
            return Some(offset);
        }

        if ch == '\n' {
            current_line += 1;
            current_col = 1;
        } else {
            current_col += 1;
        }
    }

    if current_line == line && current_col == column {
        return Some(source.len());
    }

    None
}

fn serialize_result(result: JsCompileResult) -> String {
    let mut value = match serde_json::to_value(&result) {
        Ok(value) => value,
        Err(error) => {
            return serde_json::to_string(&SerializeFallback {
                success: false,
                error: &format!("Serialization error: {error}"),
            })
            .unwrap_or_else(|_| "{\"success\":false,\"error\":\"Serialization error\"}".to_string())
        }
    };

    if let serde_json::Value::Object(map) = &mut value {
        // Backward-compatible aliases for older frontend/runtime consumers.
        if let Some(compile_results) = map.get("compile_results").cloned() {
            map.entry("circuit_definitions".to_string())
                .or_insert_with(|| compile_results.clone());
            map.entry("circuitDefinitions".to_string())
                .or_insert(compile_results);
        }

        let method_count = map
            .get("abi")
            .and_then(|abi| abi.get("methods"))
            .and_then(|methods| methods.as_array())
            .map(|methods| methods.len() as u64)
            .or_else(|| {
                map.get("compile_results")
                    .and_then(|compile_results| compile_results.as_array())
                    .map(|items| items.len() as u64)
            });

        if let Some(method_count) = method_count {
            map.entry("method_count".to_string())
                .or_insert_with(|| serde_json::Value::from(method_count));
            map.entry("methodCount".to_string())
                .or_insert_with(|| serde_json::Value::from(method_count));
        }

        let state_tree_height = map
            .get("abi")
            .and_then(|abi| abi.get("state_tree_height"))
            .and_then(|height| height.as_u64())
            .or_else(|| {
                map.get("contract_code")
                    .and_then(|contract_code| contract_code.get("state_tree_height"))
                    .and_then(|height| height.as_u64())
            });

        if let Some(state_tree_height) = state_tree_height {
            map.entry("state_tree_height".to_string())
                .or_insert_with(|| serde_json::Value::from(state_tree_height));
            map.entry("stateTreeHeight".to_string())
                .or_insert_with(|| serde_json::Value::from(state_tree_height));
        }
    }

    serde_json::to_string(&value).unwrap_or_else(|error| {
        serde_json::to_string(&SerializeFallback {
            success: false,
            error: &format!("Serialization error: {error}"),
        })
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"Serialization error\"}".to_string())
    })
}

fn serialize_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| {
        serde_json::to_string(&SerializeFallback {
            success: false,
            error: &format!("Serialization error: {error}"),
        })
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"Serialization error\"}".to_string())
    })
}

fn serialize_error_message(error: String) -> String {
    serialize_json(&serde_json::json!({ "error": error }))
}

fn serialize_interpret_result(result: JsInterpretResult) -> String {
    serde_json::to_string(&result).unwrap_or_else(|error| {
        serde_json::to_string(&SerializeFallback {
            success: false,
            error: &format!("Serialization error: {error}"),
        })
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"Serialization error\"}".to_string())
    })
}

fn serialize_interpret_error(
    error: String,
    entry_path: Option<String>,
    compile_results: Option<serde_json::Value>,
    contract_code: Option<serde_json::Value>,
    abi: Option<serde_json::Value>,
) -> String {
    serialize_interpret_result(JsInterpretResult {
        success: false,
        error: Some(error),
        error_offset: None,
        entry_path,
        execution_result: None,
        compile_results,
        contract_code,
        abi,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[derive(serde::Deserialize)]
    struct TestCompileResult {
        success: bool,
        error: Option<String>,
        error_offset: Option<usize>,
        entry_path: Option<String>,
        compile_results: Option<Vec<serde_json::Value>>,
        contract_code: Option<serde_json::Value>,
        abi: Option<serde_json::Value>,
    }

    fn parse_result(json: &str) -> TestCompileResult {
        serde_json::from_str(json).unwrap_or_else(|error| panic!("failed to parse result json: {error}\njson: {json}"))
    }

    #[test]
    #[serial]
    fn compile_source_succeeds() {
        let result = parse_result(
            &compile_source(
                r#"
                fn main() {
                    let a = 1;
                    assert_eq(a, 1, "ok");
                }
                "#,
            ),
        );

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.entry_path.as_deref(), Some("/vfs/src/main.psy"));
        assert!(result.compile_results.as_ref().is_some_and(|items| !items.is_empty()));
    }

    #[test]
    #[serial]
    fn compile_project_with_module_succeeds() {
        let files = serde_json::json!({
            "entry": ["main"],
            "method_names": ["main"],
            "files": [
                [["main"], "mod foo;\nfn main() { foo::run(); }"],
                [["foo"], "pub fn run() {}"]
            ]
        });

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.entry_path.as_deref(), Some("/vfs/src/main.psy"));
        assert!(result.compile_results.as_ref().is_some_and(|items| !items.is_empty()));
    }

    #[test]
    #[serial]
    fn compile_project_with_explicit_entry_succeeds() {
        let files = serde_json::json!({
            "entry": ["main"],
            "method_names": ["main"],
            "files": [
                [["main"], "mod foo;\nfn main() { foo::run(); }"],
                [["foo"], "pub fn run() {}"]
            ]
        });

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.entry_path.as_deref(), Some("/vfs/src/main.psy"));
        assert!(result.compile_results.as_ref().is_some_and(|items| !items.is_empty()));
    }

    #[test]
    #[serial]
    fn compile_project_rejects_missing_explicit_entry() {
        let files = serde_json::json!({
            "entry": ["missing"],
            "method_names": ["main"],
            "files": [
                [["main"], "fn main() {}"]
            ]
        });

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(!result.success, "expected compile failure");
        assert!(result
            .error
            .as_ref()
            .is_some_and(|msg| msg.contains("Explicit entry file was not found")));
    }

    #[test]
    #[serial]
    fn compile_project_rejects_non_main_entry_without_method_names() {
        let files = serde_json::json!({
            "entry": ["lib"],
            "files": [
                [["lib"], "pub fn run() {}"]
            ]
        });

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(!result.success, "expected compile failure");
        assert!(result
            .error
            .as_ref()
            .is_some_and(|msg| msg.contains("without explicit method_names")));
    }

    #[test]
    #[serial]
    fn compile_project_defaults_entry_to_main_when_missing() {
        let files = serde_json::json!({
            "files": [
                [["main"], "use std::prelude::*;\n#[contract]\n#[derive(Storage)]\npub struct C { pub value: Felt }\n#[contract_method]\nfn set_value(v: Felt) { let _x = v; }"]
            ]
        });

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success");
    }

    #[test]
    #[serial]
    fn compile_source_reports_error_offset() {
        let result = parse_result(
            &compile_source(
                r#"
                fn main( {
                }
                "#,
            ),
        );

        assert!(!result.success, "expected compile failure");
        assert!(result.error.is_some());
        assert!(result.error_offset.is_some(), "expected parse error offset");
    }

    #[test]
    #[serial]
    fn compile_source_emits_contract_code_and_compat_abi() {
        let result = parse_result(&compile_source(
            r#"
            #[contract]
            #[derive(Storage)]
            pub struct MapContract {
                pub balances: Map<Hash, Hash, 128u32>,
            }

            fn main() {
                let c = MapContractRef::new(ContractMetadata::current());
                let key: Hash = [1, 0, 0, 0];
                let value: Hash = [11, 22, 33, 44];
                c.balances.insert(key, value);
            }
            "#,
        ));

        assert!(result.success, "expected compile success, got {:?}", result.error);

        let contract_code = result.contract_code.expect("missing contract_code");
        assert_eq!(contract_code["state_tree_height"].as_u64(), Some(4));
        assert!(contract_code["functions"].as_array().is_some_and(|items| !items.is_empty()));

        let abi = result.abi.expect("missing abi");
        assert_eq!(abi["contract_name"].as_str(), Some("MapContract"));
        assert_eq!(abi["state_tree_height"].as_u64(), Some(4));
        assert!(abi["state_layout"].as_array().is_some_and(|items| !items.is_empty()));
        assert_eq!(abi["state_layout"][0]["is_imt_map"].as_bool(), Some(true));
        assert!(abi["methods"].is_array());
    }

    #[test]
    #[serial]
    fn compile_dargo_project_succeeds() {
        let project = serde_json::json!({
            "root": "root",
            "packages": [
                {
                    "id": "root",
                    "manifest": "[package]\nname = \"root\"\ntype = \"bin\"\n",
                    "files": {
                        "src/main.psy": "#[contract]\npub struct C {}\n#[contract_method]\nfn main() { assert_eq(1, 1, \"ok\"); }"
                    },
                    "dependencies": {}
                }
            ]
        });

        let result = parse_result(&compile_dargo_project(&project.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert!(result.compile_results.as_ref().is_some_and(|items| !items.is_empty()));
    }

    #[test]
    #[serial]
    fn chain_account_lifecycle_works() {
        init_chain();
        let alice: serde_json::Value = serde_json::from_str(&create_account("Alice")).unwrap();
        let bob: serde_json::Value = serde_json::from_str(&create_account("Bob")).unwrap();
        assert_eq!(alice["user_id"].as_u64(), Some(1));
        assert_eq!(bob["user_id"].as_u64(), Some(2));

        let accounts: serde_json::Value = serde_json::from_str(&get_accounts()).unwrap();
        assert_eq!(accounts.as_array().map(|items| items.len()), Some(2));

        reset_chain();
        let accounts_after_reset: serde_json::Value = serde_json::from_str(&get_accounts()).unwrap();
        assert_eq!(accounts_after_reset.as_array().map(|items| items.len()), Some(0));
    }
}
