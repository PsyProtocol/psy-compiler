use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, Once},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use psy_abi::{Abi, AbiExtractor};
use psy_common::Graph;
use psy_package::{resolve_source_workspace, MemoryResolver, PackageId, PackageSources, RelativeFilePath, VfsPath};
use psy_config::network_constants::VM_TYPE_STANRDARD_DAPEN_V1;
use psy_vm::dpn::{
    eval::executor::{ExecutionContext, ExecutionResult, InMemoryStateBackend, StateBackend, VmExecutor},
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
    abi: Abi,
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
    abi: Abi,
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
    abi: Abi,
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
            let state_tree_height = compute_state_tree_height(&mut result);
            let contract_code = match extract_contract_code(&compile_results_for_metadata, state_tree_height) {
                Ok(value) => Some(value),
                Err(error) => {
                    tracing::warn!("Contract code extraction failed: {error}");
                    None
                }
            };
            let abi_value = extract_abi(&mut result, state_tree_height, &compile_results_for_metadata)
                .ok()
                .and_then(|abi| {
                    serde_json::to_value(&abi)
                        .map_err(|error| tracing::warn!("ABI serialization failed: {error}"))
                        .ok()
                });

            if let Some(abi) = abi_value.clone() {
                if let Ok(parsed) = serde_json::from_value::<Abi>(abi) {
                    cache_compile(CachedCompile {
                        state_tree_height,
                        abi: parsed,
                        circuit_definitions: compile_results_for_metadata,
                    });
                }
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
            name: compile_output.abi.contract.name.clone(),
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
            .contract.methods
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
            let value = chain
                .state
                .get_contract_slot(user_id, contract_id, slot)
                .map_err(|error| error.to_string())?;
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
    compile_vfs_project_with_contract(files, entry_path, Vec::new(), None)
}

fn interpret_vfs_project(files: Vec<(PathBuf, String)>, entry_path: PathBuf, request: InterpretRequest) -> String {
    let method_name = request.method_name.clone().unwrap_or_else(|| "main".to_string());
    let source_index = build_source_index(&files);
    let vfs_shared_files = files.into_iter().map(|(path, content)| (path, Arc::<str>::from(content))).collect();

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
            let state_tree_height = compute_state_tree_height(&mut result);
            let contract_code = extract_contract_code(&compile_results_for_metadata, state_tree_height).ok();
            let abi_value = extract_abi(&mut result, state_tree_height, &compile_results_for_metadata)
                .ok()
                .and_then(|abi| {
                    serde_json::to_value(&abi)
                        .map_err(|error| tracing::warn!("ABI serialization failed: {error}"))
                        .ok()
                });

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
    let vfs_shared_files = files.into_iter().map(|(path, content)| (path, Arc::<str>::from(content))).collect();

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
            let state_tree_height = compute_state_tree_height(&mut result);
            let contract_code = match extract_contract_code(&compile_results_for_metadata, state_tree_height) {
                Ok(value) => Some(value),
                Err(error) => {
                    tracing::warn!("Contract code extraction failed: {error}");
                    None
                }
            };

            let abi_value = extract_abi(&mut result, state_tree_height, &compile_results_for_metadata)
                .ok()
                .and_then(|abi| {
                    serde_json::to_value(&abi)
                        .map_err(|error| tracing::warn!("ABI serialization failed: {error}"))
                        .ok()
                });

            if let Some(abi) = abi_value.clone() {
                if let Ok(parsed) = serde_json::from_value::<Abi>(abi) {
                    cache_compile(CachedCompile {
                        state_tree_height,
                        abi: parsed,
                        circuit_definitions: compile_results_for_metadata.clone(),
                    });
                }
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

fn resolve_method_names(entry_path: &PathBuf, method_names: Option<Vec<String>>, _vfs_files: &[(PathBuf, String)]) -> Result<Vec<String>, String> {
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

fn extract_abi(
    result: &mut psy_interpreter::InterpretResult,
    state_tree_height: u16,
    compile_results: &[DPNFunctionCircuitDefinition],
) -> Result<Abi, String> {
    let program = &mut result.ctx.program;
    let method_metadata = compile_results
        .iter()
        .map(|function| {
            (
                function.name.clone(),
                (function.method_id, function.is_view_function()),
            )
        })
        .collect::<HashMap<_, _>>();
    AbiExtractor::new("contract".to_string())
        .extract_abi(program, state_tree_height, &method_metadata)
        .map_err(|error| error.to_string())
}

fn extract_contract_code(compile_results: &[DPNFunctionCircuitDefinition], state_tree_height: u16) -> Result<serde_json::Value, String> {
    let functions = compile_results
        .iter()
        .map(|circuit| {
            let bytes = serde_cbor::to_vec(circuit).map_err(|error| error.to_string())?;
            Ok(JsContractFunctionCode {
                method_id: circuit.method_id,
                num_inputs: circuit.circuit_inputs.len(),
                num_outputs: circuit.circuit_outputs.len(),
                vm_type: VM_TYPE_STANRDARD_DAPEN_V1,
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

fn compute_state_tree_height(result: &mut psy_interpreter::InterpretResult) -> u16 {
    AbiExtractor::new("contract".to_string()).compute_state_tree_height(&mut result.ctx.program)
}

fn extract_error_offset(error_msg: &str, sources: &HashMap<String, Arc<str>>) -> Option<usize> {
    let sanitized_error = strip_ansi_csi_sequences(error_msg);
    let error_msg = sanitized_error.as_str();

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

fn strip_ansi_csi_sequences(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            output.push(ch);
            continue;
        }
        if chars.peek() != Some(&'[') {
            output.push(ch);
            continue;
        }
        chars.next();
        for sequence_char in chars.by_ref() {
            if ('@'..='~').contains(&sequence_char) {
                break;
            }
        }
    }
    output
}

fn extract_error_offset_from_line_col(error_msg: &str, sources: &HashMap<String, Arc<str>>) -> Option<usize> {
    let marker = "[ ";
    for (marker_start, _) in error_msg.match_indices(marker) {
        let rest = &error_msg[marker_start + marker.len()..];
        let Some(end) = rest.find(" ]") else {
            continue;
        };
        let Some((path_text, line, column)) = parse_location_triplet(&rest[..end]) else {
            continue;
        };
        let normalized_path = path_text.replace('\\', "/");
        let source = sources.get(&path_text).or_else(|| {
            sources
                .iter()
                .find(|(path, _)| path.replace('\\', "/") == normalized_path)
                .map(|(_, source)| source)
        });
        if let Some(offset) = source.and_then(|source| line_col_to_offset(source, line, column)) {
            return Some(offset);
        }
    }

    if sources.len() == 1 {
        let (_, line, column) = error_msg
            .match_indices(marker)
            .find_map(|(marker_start, _)| {
                let rest = &error_msg[marker_start + marker.len()..];
                let end = rest.find(" ]")?;
                let (_, line, column) = parse_location_triplet(&rest[..end])?;
                Some(((), line, column))
            })?;
        return sources
            .values()
            .next()
            .and_then(|source| line_col_to_offset(source, line, column));
    }

    None
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
            map.entry("circuit_definitions".to_string()).or_insert_with(|| compile_results.clone());
            map.entry("circuitDefinitions".to_string()).or_insert(compile_results);
        }

        let method_count = map
            .get("abi")
            .and_then(|abi| abi.get("contract"))
            .and_then(|contract| contract.get("methods"))
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
            .and_then(|abi| abi.get("contract"))
            .and_then(|contract| contract.get("state_tree_height"))
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
    use serial_test::serial;

    use super::*;

    #[derive(serde::Deserialize)]
    struct TestCompileResult {
        success: bool,
        error: Option<String>,
        error_offset: Option<usize>,
        entry_path: Option<String>,
        compile_results: Option<serde_json::Value>,
        contract_code: Option<serde_json::Value>,
        abi: Option<serde_json::Value>,
        method_count: Option<u64>,
    }

    fn parse_result(json: &str) -> TestCompileResult {
        serde_json::from_str(json).unwrap_or_else(|error| panic!("failed to parse result json: {error}\njson: {json}"))
    }

    #[test]
    fn error_offset_parser_handles_ansi_colored_locations() {
        let sources = HashMap::from([(
            "/vfs/src/main.psy".to_string(),
            Arc::<str>::from("a\nxyz"),
        )]);
        let error = "\u{1b}[31m[UnexpectedToken]\u{1b}[0m \u{1b}[38;5;246m╭─[\u{1b}[0m /vfs/src/main.psy:2:3 \u{1b}[38;5;246m]\u{1b}[0m";

        assert_eq!(extract_error_offset(error, &sources), Some(4));
    }

    #[test]
    fn ansi_stripping_preserves_plain_diagnostic_text() {
        assert_eq!(
            strip_ansi_csi_sequences("before \u{1b}[31mred\u{1b}[0m after"),
            "before red after"
        );
    }

    #[test]
    fn ansi_stripping_does_not_drop_non_csi_escape_characters() {
        assert_eq!(strip_ansi_csi_sequences("before \u{1b}x after"), "before \u{1b}x after");
    }

    #[test]
    fn error_offset_parser_prefers_the_last_explicit_offset() {
        let sources = HashMap::new();

        assert_eq!(
            extract_error_offset("inner error at offset 3; outer error at offset 17", &sources),
            Some(17)
        );
        assert_eq!(extract_error_offset("offset not-a-number", &sources), None);
    }

    #[test]
    fn error_offset_parser_accepts_windows_paths_and_normalized_source_keys() {
        let sources = HashMap::from([(
            "C:/project/src/main.psy".to_string(),
            Arc::<str>::from("first\nsecond"),
        )]);

        assert_eq!(
            extract_error_offset("[ C:\\project\\src\\main.psy:2:2 ]", &sources),
            Some(7)
        );
    }

    #[test]
    fn error_offset_parser_uses_single_source_fallback_for_display_paths() {
        let sources = HashMap::from([(
            "/vfs/internal/main.psy".to_string(),
            Arc::<str>::from("abc\ndef"),
        )]);

        assert_eq!(extract_error_offset("[ main.psy:2:1 ]", &sources), Some(4));
    }

    #[test]
    fn line_column_offsets_are_byte_offsets_and_validate_boundaries() {
        let source = "中a\nβ";

        assert_eq!(line_col_to_offset(source, 1, 1), Some(0));
        assert_eq!(line_col_to_offset(source, 1, 2), Some(3));
        assert_eq!(line_col_to_offset(source, 2, 1), Some(5));
        assert_eq!(line_col_to_offset(source, 2, 2), Some(source.len()));
        assert_eq!(line_col_to_offset(source, 0, 1), None);
        assert_eq!(line_col_to_offset(source, 1, 0), None);
        assert_eq!(line_col_to_offset(source, 3, 1), None);
    }

    #[test]
    fn malformed_location_triplets_are_rejected() {
        assert_eq!(parse_location_triplet("main.psy:2:3"), Some(("main.psy".to_string(), 2, 3)));
        assert_eq!(parse_location_triplet("C:\\src\\main.psy:2:3"), Some(("C:\\src\\main.psy".to_string(), 2, 3)));
        assert_eq!(parse_location_triplet("main.psy:two:3"), None);
        assert_eq!(parse_location_triplet("main.psy:2"), None);
        assert_eq!(
            parse_location_triplet("main.psy : 2 : 3"),
            Some(("main.psy".to_string(), 2, 3))
        );
    }

    #[test]
    fn ansi_stripping_handles_truncated_and_parameterized_sequences() {
        assert_eq!(strip_ansi_csi_sequences("end\u{1b}"), "end\u{1b}");
        assert_eq!(strip_ansi_csi_sequences("end\u{1b}["), "end");
        assert_eq!(strip_ansi_csi_sequences("\u{1b}[?25hvisible"), "visible");
        assert_eq!(strip_ansi_csi_sequences("\u{1b}[38;5;196mX\u{1b}[0m"), "X");
        assert_eq!(strip_ansi_csi_sequences("\u{1b}[ @"), "");
    }

    #[test]
    fn explicit_offset_wins_over_location_triplets() {
        let sources = HashMap::from([(
            "main.psy".to_string(),
            Arc::<str>::from("first\nsecond"),
        )]);

        assert_eq!(
            extract_error_offset("failed [ main.psy:1:1 ] at offset 5", &sources),
            Some(5)
        );
    }

    #[test]
    fn error_offset_line_col_arms_skip_unterminated_and_malformed_triplets() {
        let sources = HashMap::from([(
            "main.psy".to_string(),
            Arc::<str>::from("first\nsecond"),
        )]);

        // A "[ " marker without a closing " ]" never yields a triplet.
        assert_eq!(extract_error_offset("[ main.psy:2:1 never closed", &sources), None);
        // Bracketed text that does not parse as path:line:column is skipped,
        // including by the single-source fallback.
        assert_eq!(extract_error_offset("[ not a triplet ]", &sources), None);
        // A well-formed triplet outside the source bounds produces no offset.
        assert_eq!(extract_error_offset("[ main.psy:9:9 ]", &sources), None);
    }

    #[test]
    fn line_column_offsets_handle_empty_sources_and_crlf() {
        assert_eq!(line_col_to_offset("", 1, 1), Some(0));
        assert_eq!(line_col_to_offset("", 1, 2), None);
        assert_eq!(line_col_to_offset("a\r\nb", 2, 1), Some(3));
    }

    #[test]
    #[serial]
    fn compile_source_succeeds() {
        let result = parse_result(&compile_source(
            r#"
                fn main() {
                    let a = 1;
                    assert_eq(a, 1, "ok");
                }
                "#,
        ));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.entry_path.as_deref(), Some("/vfs/src/main.psy"));
        assert!(result.compile_results.as_ref().is_some_and(|items| items.as_array().is_some_and(|a| !a.is_empty())));
        assert_eq!(result.method_count, Some(1));
        assert!(result.contract_code.is_some(), "missing contract_code");
        assert!(result.abi.is_some(), "missing abi");
    }

    #[test]
    #[serial]
    fn compile_source_rejects_constant_out_of_bounds_array_write() {
        let result = parse_result(&compile_source(
            r#"
                fn main() {
                    let mut values: [Felt; 2] = [10, 20];
                    values[2] = 99;
                }
                "#,
        ));

        assert!(!result.success, "expected constant out-of-bounds array write to fail compilation");
        assert!(
            result.error.as_deref().unwrap_or_default().contains("IndexOutOfBounds"),
            "unexpected error: {:?}",
            result.error
        );
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
        assert!(result.compile_results.as_ref().is_some_and(|items| items.as_array().is_some_and(|a| !a.is_empty())));
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
        assert!(result.compile_results.as_ref().is_some_and(|items| items.as_array().is_some_and(|a| !a.is_empty())));
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
        assert!(result.error.as_ref().is_some_and(|msg| msg.contains("Explicit entry file was not found")));
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
        assert!(result.error.as_ref().is_some_and(|msg| msg.contains("without explicit method_names")));
    }

    #[test]
    #[serial]
    fn compile_project_defaults_entry_to_main_when_missing() {
        let files = serde_json::json!({
            "files": [
                [["main"], "use std::prelude::*;\n#[contract]\n#[derive(Storage)]\npub struct C { pub value: Felt }\n#[contract::write_method]\nfn set_value(v: Felt) { let _x = v; }"]
            ]
        });

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success");
    }

    #[test]
    #[serial]
    fn compile_source_reports_error_offset() {
        let result = parse_result(&compile_source(
            r#"
                fn main( {
                }
                "#,
        ));

        assert!(!result.success, "expected compile failure");
        assert!(result.error.is_some());
        assert!(
            result.error_offset.is_some(),
            "expected parse error offset; error was: {:?}",
            result.error
        );
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

            #[contract::write_method]
            pub fn set_balance() {
                let c = MapContractRef::new(ContractMetadata::current());
                let key: Hash = [1, 0, 0, 0];
                let value: Hash = [11, 22, 33, 44];
                c.balances.insert(key, value);
            }
            "#,
        ));

        assert!(result.success, "expected compile success, got {:?}", result.error);

        let contract_code = result.contract_code.expect("missing contract_code");
        // A map with capacity 128 requires seven Merkle levels.
        assert_eq!(contract_code["state_tree_height"].as_u64(), Some(7));
        assert!(contract_code["functions"].as_array().is_some_and(|items| !items.is_empty()));

        let abi = result.abi.expect("missing abi");
        assert_eq!(abi["contract"]["name"].as_str(), Some("MapContract"));
        assert_eq!(abi["contract"]["state_tree_height"].as_u64(), Some(7));
        let state = abi["contract"]["state"].as_array().expect("state array");
        assert!(!state.is_empty());
        assert_eq!(state[0]["name"].as_str(), Some("balances"));
        assert_eq!(state[0]["type"]["map_kind"].as_str(), Some("map"));
        assert!(abi["contract"]["methods"].is_array());
    }

    #[test]
    #[serial]
    fn compile_source_emits_canonical_dapen_vm_type() {
        // A stale zero here is rejected by the prover compile bridge.
        let result = parse_result(&compile_source(
            r#"
            #[contract]
            #[derive(Storage)]
            pub struct MapContract {
                pub balances: Map<Hash, Hash, 128u32>,
            }

            #[contract::write_method]
            pub fn set_balance() {
                let c = MapContractRef::new(ContractMetadata::current());
                let key: Hash = [1, 0, 0, 0];
                let value: Hash = [11, 22, 33, 44];
                c.balances.insert(key, value);
            }
            "#,
        ));

        assert!(result.success, "expected compile success, got {:?}", result.error);

        let contract_code = result.contract_code.expect("missing contract_code");
        let functions = contract_code["functions"]
            .as_array()
            .expect("contract_code functions must be a non-empty array");
        assert!(!functions.is_empty(), "contract_code must emit at least one function");

        for function in functions {
            let vm_type = function["vm_type"].as_u64();
            assert_eq!(
                vm_type,
                Some(VM_TYPE_STANRDARD_DAPEN_V1 as u64),
                "every emitted contract function must carry the canonical DAPEN VM type \
                 (VM_TYPE_STANRDARD_DAPEN_V1 = {}), got {:?}",
                VM_TYPE_STANRDARD_DAPEN_V1,
                vm_type,
            );
            assert_ne!(
                vm_type,
                Some(0),
                "legacy vm_type: 0 must no longer appear in compiler output",
            );
        }
    }

    #[test]
    #[serial]
    fn compile_source_emits_abi() {
        let result = parse_result(&compile_source(
            r#"
            #[contract]
            #[derive(Storage)]
            pub struct MapContract {
                pub balances: Map<Hash, Hash, 128u32>,
            }

            #[contract::write_method]
            pub fn set_balance() {
                let c = MapContractRef::new(ContractMetadata::current());
                let key: Hash = [1, 0, 0, 0];
                let value: Hash = [11, 22, 33, 44];
                c.balances.insert(key, value);
            }
            "#,
        ));

        assert!(result.success, "expected compile success, got {:?}", result.error);

        let abi = result.abi.expect("missing abi");
        assert_eq!(abi["schema_version"].as_str(), Some("2.0.0"));
        assert_eq!(abi["contract"]["name"].as_str(), Some("MapContract"));
        assert_eq!(abi["contract"]["state_tree_height"].as_u64(), Some(7));

        // State field should use TypeRef with kind: "map"
        let state = abi["contract"]["state"].as_array().expect("state array");
        assert!(!state.is_empty());
        assert_eq!(state[0]["name"].as_str(), Some("balances"));
        assert_eq!(state[0]["type"]["map_kind"].as_str(), Some("map"));

        // Methods should have explicit method_id and state_mutability
        let methods = abi["contract"]["methods"].as_array().expect("methods array");
        assert!(!methods.is_empty());
        assert!(methods[0]["method_id"].as_u64().is_some());
        assert!(methods[0]["state_mutability"].as_str().is_some());
        assert!(methods[0]["input_felt_count"].as_u64().is_some());
        assert!(methods[0]["output_felt_count"].as_u64().is_some());
    }

    #[test]
    #[serial]
    fn compile_source_expands_type_aliases_in_abi_layout() {
        let result = parse_result(&compile_source(include_str!("../../tests/abi_alias_test.psy")));

        assert!(result.success, "expected compile success, got {:?}", result.error);

        let abi = result.abi.expect("missing abi");
        let state = abi["contract"]["state"].as_array().expect("state array");
        assert_eq!(state.len(), 4);

        assert_eq!(state[0]["name"].as_str(), Some("amount"));
        assert_eq!(state[0]["type"]["kind"].as_str(), Some("primitive"));
        assert_eq!(state[0]["type"]["name"].as_str(), Some("Felt"));
        assert_eq!(state[0]["offset"].as_u64(), Some(0));
        assert_eq!(state[0]["felt_size"].as_u64(), Some(1));

        assert_eq!(state[1]["name"].as_str(), Some("history"));
        assert_eq!(state[1]["type"]["kind"].as_str(), Some("array"));
        assert_eq!(state[1]["type"]["item"]["kind"].as_str(), Some("primitive"));
        assert_eq!(state[1]["type"]["item"]["name"].as_str(), Some("Felt"));
        assert_eq!(state[1]["offset"].as_u64(), Some(1));
        assert_eq!(state[1]["felt_size"].as_u64(), Some(2));

        assert_eq!(state[2]["name"].as_str(), Some("account"));
        assert_eq!(state[2]["type"]["kind"].as_str(), Some("struct"));
        assert_eq!(state[2]["type"]["name"].as_str(), Some("AccountState"));
        assert_eq!(state[2]["offset"].as_u64(), Some(3));
        assert_eq!(state[2]["felt_size"].as_u64(), Some(5));

        assert_eq!(state[3]["name"].as_str(), Some("tail"));
        assert_eq!(state[3]["offset"].as_u64(), Some(8));
        assert_eq!(state[3]["felt_size"].as_u64(), Some(1));

        let account = abi["types"]
            .as_array()
            .expect("types array")
            .iter()
            .find(|ty| ty["name"].as_str() == Some("AccountState"))
            .expect("AccountState ABI type");
        assert_eq!(account["felt_size"].as_u64(), Some(5));
        assert_eq!(account["fields"][0]["offset_within_parent"].as_u64(), Some(0));
        assert_eq!(account["fields"][0]["felt_size"].as_u64(), Some(1));
        assert_eq!(account["fields"][1]["offset_within_parent"].as_u64(), Some(1));
        assert_eq!(account["fields"][1]["felt_size"].as_u64(), Some(4));
    }

    #[test]
    #[serial]
    fn compile_source_emits_abi_with_structs() {
        let result = parse_result(&compile_source(
            r#"
            #[derive(Storage)]
            struct OtherInfo {
                pub amount: Felt,
                pub claimed: Felt,
            }

            #[contract]
            #[derive(Storage)]
            pub struct TokenContract {
                pub balance: Felt,
                pub info: [OtherInfo; 1024],
            }

            fn main() {
                let c = TokenContractRef::new(ContractMetadata::current());
                c.balance = 42;
            }
            "#,
        ));

        assert!(result.success, "expected compile success, got {:?}", result.error);

        let abi = result.abi.expect("missing abi");
        assert_eq!(abi["contract"]["name"].as_str(), Some("TokenContract"));

        // State should have balance (primitive) and info (array of struct)
        let state = abi["contract"]["state"].as_array().expect("state array");
        assert_eq!(state.len(), 2);
        assert_eq!(state[0]["name"].as_str(), Some("balance"));
        assert_eq!(state[0]["type"]["kind"].as_str(), Some("primitive"));
        assert_eq!(state[0]["type"]["name"].as_str(), Some("Felt"));
        assert_eq!(state[1]["name"].as_str(), Some("info"));
        assert_eq!(state[1]["type"]["kind"].as_str(), Some("array"));

        // Types table should contain OtherInfo struct
        let types = abi["types"].as_array().expect("types array");
        let type_names: Vec<_> = types.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(type_names.contains(&"OtherInfo"), "OtherInfo not in types: {:?}", type_names);
    }

    #[test]
    #[serial]
    fn compile_source_discovers_two_standalone_contract_methods() {
        let result = parse_result(&compile_source(
            r#"
            #[contract]
            #[derive(Storage)]
            pub struct MethodContract {
                pub value: Felt,
            }

            #[contract::write_method]
            pub fn set_value(value: Felt) {
                let c = MethodContractRef::new(ContractMetadata::current());
                c.value = value;
            }

            #[contract::view_method]
            pub fn get_value() -> Felt {
                let c = MethodContractRef::new(ContractMetadata::current());
                c.value.get()
            }
            "#,
        ));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.method_count, Some(2));

        let compile_results = result
            .compile_results
            .as_ref()
            .and_then(serde_json::Value::as_array)
            .expect("missing compile_results");
        assert_eq!(compile_results.len(), 2);

        let contract_code = result.contract_code.as_ref().expect("missing contract_code");
        assert_eq!(contract_code["functions"].as_array().map(Vec::len), Some(2));

        let abi = result.abi.as_ref().expect("missing abi");
        let methods = abi["contract"]["methods"].as_array().expect("methods should be array");
        assert_eq!(methods.len(), 2);
        assert_eq!(method_by_name(methods, "get_value")["state_mutability"].as_str(), Some("view"));
        assert_eq!(method_by_name(methods, "set_value")["state_mutability"].as_str(), Some("external"));
    }

    #[test]
    #[serial]
    fn compile_project_discovers_two_standalone_contract_methods() {
        let project = serde_json::json!({
            "entry": ["main"],
            "files": [[[
                "main"
            ], r#"
            #[contract]
            #[derive(Storage)]
            pub struct ProjectContract {
                pub value: Felt,
            }

            #[contract::write_method]
            pub fn set_value(value: Felt) {
                let c = ProjectContractRef::new(ContractMetadata::current());
                c.value = value;
            }

            #[contract::view_method]
            pub fn get_value() -> Felt {
                let c = ProjectContractRef::new(ContractMetadata::current());
                c.value.get()
            }
            "#]]
        });
        let result = parse_result(&compile_project(&project.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.method_count, Some(2));
        assert_eq!(
            result.compile_results.as_ref().and_then(serde_json::Value::as_array).map(Vec::len),
            Some(2)
        );
        assert_eq!(
            result.contract_code.as_ref().and_then(|code| code["functions"].as_array()).map(Vec::len),
            Some(2)
        );
        assert_eq!(
            result.abi.as_ref().and_then(|abi| abi["contract"]["methods"].as_array()).map(Vec::len),
            Some(2)
        );
    }

    #[test]
    #[serial]
    fn compile_source_rejects_view_method_with_state_write() {
        let files = serde_json::json!({
            "files": [[["main"], r#"
            #[contract]
            #[derive(Storage)]
            pub struct BadViewContract {
                pub value: Felt,
            }

            #[contract::view_method]
            pub fn set_value(value: Felt) {
                let c = BadViewContractRef::new(ContractMetadata::current());
                c.value = value;
            }
            "#]]
        });
        let result = parse_result(&compile_project(&files.to_string()));

        assert!(!result.success, "expected compile failure");
        assert!(result.error.as_ref().is_some_and(|msg| msg.contains("marked #[contract::view_method]")));
    }

    fn method_by_name<'a>(methods: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
        methods
            .iter()
            .find(|method| method["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("missing method `{name}` in ABI methods: {methods:?}"))
    }

    fn method_names(methods: &[serde_json::Value]) -> Vec<&str> {
        methods.iter().filter_map(|method| method["name"].as_str()).collect()
    }

    /// Matrix cell 1/4/5/6/7/11: all three attribute forms are accepted; ABI
    /// `state_mutability` is body-driven via `is_view_function()` (writes →
    /// "external", pure reads → "view"), not by the attribute form alone.
    #[test]
    #[serial]
    fn compile_source_accepts_all_three_contract_method_attr_forms_in_abi() {
        let files = serde_json::json!({
            "files": [[["main"], r#"
            #[contract]
            #[derive(Storage)]
            pub struct AttrMatrixContract {
                pub value: Felt,
            }

            #[contract::write_method]
            pub fn write_attr_writes(value: Felt) {
                let c = AttrMatrixContractRef::new(ContractMetadata::current());
                c.value = value;
            }

            #[contract::write_method]
            pub fn write_attr_readonly() -> Felt {
                let c = AttrMatrixContractRef::new(ContractMetadata::current());
                c.value.get()
            }

            #[contract::view_method]
            pub fn view_attr_readonly() -> Felt {
                let c = AttrMatrixContractRef::new(ContractMetadata::current());
                c.value.get()
            }

            #[contract_method]
            pub fn bare_attr_writes(value: Felt) {
                let c = AttrMatrixContractRef::new(ContractMetadata::current());
                c.value = value;
            }

            #[contract_method]
            pub fn bare_attr_readonly() -> Felt {
                let c = AttrMatrixContractRef::new(ContractMetadata::current());
                c.value.get()
            }
            "#]]
        });
        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        let abi = result.abi.expect("missing abi");
        let methods = abi["contract"]["methods"].as_array().expect("methods should be array");
        let names = method_names(methods);
        for expected in [
            "write_attr_writes",
            "write_attr_readonly",
            "view_attr_readonly",
            "bare_attr_writes",
            "bare_attr_readonly",
        ] {
            assert!(names.contains(&expected), "expected `{expected}` in ABI methods, got {names:?}");
        }

        // write_method + state write → external
        assert_eq!(
            method_by_name(methods, "write_attr_writes")["state_mutability"].as_str(),
            Some("external")
        );
        // write_method on a read-only body is allowed (no write-attr enforcement);
        // ABI mutability follows the body, so pure reads are still "view".
        assert_eq!(
            method_by_name(methods, "write_attr_readonly")["state_mutability"].as_str(),
            Some("view")
        );
        // view_method read-only getter → view
        assert_eq!(
            method_by_name(methods, "view_attr_readonly")["state_mutability"].as_str(),
            Some("view")
        );
        // bare contract_method + state write → accepted, external
        assert_eq!(
            method_by_name(methods, "bare_attr_writes")["state_mutability"].as_str(),
            Some("external")
        );
        // bare contract_method read-only → accepted; mutability is body-driven ("view")
        assert_eq!(
            method_by_name(methods, "bare_attr_readonly")["state_mutability"].as_str(),
            Some("view")
        );
    }

    /// Matrix cell 3: #[contract::view_method] that invokes an external contract
    /// (mutating call path) must be rejected — not only direct storage writes.
    #[test]
    #[serial]
    fn compile_source_rejects_view_method_with_mutating_contract_call() {
        let files = serde_json::json!({
            "files": [[["main"], r#"
            #[contract]
            #[derive(Storage)]
            pub struct ViewInvokeContract {
                pub value: Felt,
            }

            #[contract::view_method]
            pub fn call_other() {
                invoke_deferred(0, 1, [1]);
            }
            "#]]
        });
        let result = parse_result(&compile_project(&files.to_string()));

        assert!(!result.success, "expected compile failure for view_method + invoke, got success with abi {:?}", result.abi);
        assert!(
            result.error.as_ref().is_some_and(|msg| {
                msg.contains("marked #[contract::view_method]")
                    && (msg.contains("writes state") || msg.contains("mutating contract call"))
            }),
            "unexpected error: {:?}",
            result.error
        );
    }

    /// Matrix cell 8: a public function with no contract-method attribute is
    /// not discovered as a contract API method and must not appear in the ABI.
    #[test]
    #[serial]
    fn compile_source_omits_functions_without_contract_method_attr_from_abi() {
        let files = serde_json::json!({
            "files": [[["main"], r#"
            #[contract]
            #[derive(Storage)]
            pub struct NoAttrContract {
                pub value: Felt,
            }

            #[contract::view_method]
            pub fn get_value() -> Felt {
                let c = NoAttrContractRef::new(ContractMetadata::current());
                c.value.get()
            }

            pub fn helper_not_exported() -> Felt {
                1
            }

            fn private_helper() -> Felt {
                2
            }
            "#]]
        });
        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        let abi = result.abi.expect("missing abi");
        let methods = abi["contract"]["methods"].as_array().expect("methods should be array");
        let names = method_names(methods);
        assert_eq!(names, vec!["get_value"], "only attributed public methods belong in ABI, got {names:?}");
        assert_eq!(method_by_name(methods, "get_value")["state_mutability"].as_str(), Some("view"));
    }

    /// Matrix cell 9: path must be exactly `contract`. `#[foo::view_method]` is
    /// not a contract API method (not in ABI) and is not view-enforced.
    #[test]
    #[serial]
    fn compile_source_ignores_malformed_contract_method_attr_path() {
        let files = serde_json::json!({
            "files": [[["main"], r#"
            #[contract]
            #[derive(Storage)]
            pub struct BadPathContract {
                pub value: Felt,
            }

            #[contract::view_method]
            pub fn get_value() -> Felt {
                let c = BadPathContractRef::new(ContractMetadata::current());
                c.value.get()
            }

            // Wrong path segment — must NOT be treated as a contract method.
            #[foo::view_method]
            pub fn not_a_contract_method(value: Felt) {
                let c = BadPathContractRef::new(ContractMetadata::current());
                c.value = value;
            }
            "#]]
        });
        let result = parse_result(&compile_project(&files.to_string()));

        assert!(
            result.success,
            "malformed path should not be treated as view_method (no enforcement); got {:?}",
            result.error
        );
        let abi = result.abi.expect("missing abi");
        let methods = abi["contract"]["methods"].as_array().expect("methods should be array");
        let names = method_names(methods);
        assert_eq!(names, vec!["get_value"], "#[foo::view_method] must not appear in ABI, got {names:?}");
    }

    /// Matrix cell 10: both bare `#[contract_method]` and `#[contract::view_method]`
    /// on the same fn — view declaration is recognized (`any` over attrs), so a
    /// writing body is rejected.
    #[test]
    #[serial]
    fn compile_source_rejects_dual_bare_and_view_attr_when_body_writes() {
        let files = serde_json::json!({
            "files": [[["main"], r#"
            #[contract]
            #[derive(Storage)]
            pub struct DualAttrContract {
                pub value: Felt,
            }

            #[contract_method]
            #[contract::view_method]
            pub fn dual_write(value: Felt) {
                let c = DualAttrContractRef::new(ContractMetadata::current());
                c.value = value;
            }
            "#]]
        });
        let result = parse_result(&compile_project(&files.to_string()));

        assert!(!result.success, "expected compile failure when view_method coexists with a writing body");
        assert!(
            result.error.as_ref().is_some_and(|msg| msg.contains("marked #[contract::view_method]")),
            "unexpected error: {:?}",
            result.error
        );
    }

    /// Matrix cell 10 (read-only): dual bare + view_method on a pure getter is
    /// accepted and ABI-marked view.
    #[test]
    #[serial]
    fn compile_source_accepts_dual_bare_and_view_attr_when_body_is_readonly() {
        let files = serde_json::json!({
            "files": [[["main"], r#"
            #[contract]
            #[derive(Storage)]
            pub struct DualAttrReadonlyContract {
                pub value: Felt,
            }

            #[contract_method]
            #[contract::view_method]
            pub fn dual_get() -> Felt {
                let c = DualAttrReadonlyContractRef::new(ContractMetadata::current());
                c.value.get()
            }
            "#]]
        });
        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        let abi = result.abi.expect("missing abi");
        let methods = abi["contract"]["methods"].as_array().expect("methods should be array");
        let names = method_names(methods);
        assert_eq!(names, vec!["dual_get"]);
        assert_eq!(method_by_name(methods, "dual_get")["state_mutability"].as_str(), Some("view"));
    }

    /// bare `#[contract_method]` may write state (no view enforcement).
    #[test]
    #[serial]
    fn compile_source_allows_bare_contract_method_with_state_write() {
        let files = serde_json::json!({
            "files": [[["main"], r#"
            #[contract]
            #[derive(Storage)]
            pub struct BareWriteContract {
                pub value: Felt,
            }

            #[contract_method]
            pub fn set_value(value: Felt) {
                let c = BareWriteContractRef::new(ContractMetadata::current());
                c.value = value;
            }
            "#]]
        });
        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        let abi = result.abi.expect("missing abi");
        let methods = abi["contract"]["methods"].as_array().expect("methods should be array");
        assert_eq!(method_names(methods), vec!["set_value"]);
        assert_eq!(
            method_by_name(methods, "set_value")["state_mutability"].as_str(),
            Some("external")
        );
    }

    /// `#[contract::write_method]` may write state and is ABI-marked external.
    #[test]
    #[serial]
    fn compile_source_allows_write_method_with_state_write() {
        let files = serde_json::json!({
            "files": [[["main"], r#"
            #[contract]
            #[derive(Storage)]
            pub struct WriteOkContract {
                pub value: Felt,
            }

            #[contract::write_method]
            pub fn set_value(value: Felt) {
                let c = WriteOkContractRef::new(ContractMetadata::current());
                c.value = value;
            }
            "#]]
        });
        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        let abi = result.abi.expect("missing abi");
        let methods = abi["contract"]["methods"].as_array().expect("methods should be array");
        assert_eq!(method_names(methods), vec!["set_value"]);
        assert_eq!(
            method_by_name(methods, "set_value")["state_mutability"].as_str(),
            Some("external")
        );
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
                        "src/main.psy": "#[contract]\npub struct C {}\n#[contract::write_method]\nfn main() { assert_eq(1, 1, \"ok\"); }"
                    },
                    "dependencies": {}
                }
            ]
        });

        let result = parse_result(&compile_dargo_project(&project.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert!(result.compile_results.as_ref().is_some_and(|items| items.as_array().is_some_and(|a| !a.is_empty())));
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

    #[test]
    fn ide_module_parts_to_path_builds_frontend_entry_paths() {
        assert_eq!(ide_module_parts_to_path(&[]), PathBuf::from("/vfs/src/main.psy"));
        assert_eq!(
            ide_module_parts_to_path(&["main".to_string()]),
            PathBuf::from("/vfs/src/main.psy")
        );
        assert_eq!(
            ide_module_parts_to_path(&["main.psy".to_string()]),
            PathBuf::from("/vfs/src/main.psy")
        );
        assert_eq!(
            ide_module_parts_to_path(&["grid".to_string()]),
            PathBuf::from("/vfs/src/grid.psy")
        );
        assert_eq!(
            ide_module_parts_to_path(&["mods".to_string(), "grid".to_string()]),
            PathBuf::from("/vfs/src/mods/grid.psy")
        );
        assert_eq!(
            ide_module_parts_to_path(&["mods".to_string(), "grid.psy".to_string()]),
            PathBuf::from("/vfs/src/mods/grid.psy")
        );
    }

    #[test]
    fn execution_context_input_defaults_missing_fields_to_zero() {
        let context = ExecutionContext::from(ExecutionContextInput {
            user_id: None,
            contract_id: None,
            caller_contract_id: None,
            checkpoint_id: None,
            nonce: None,
            user_public_key_hash: None,
        });
        assert_eq!(context.user_id, 0);
        assert_eq!(context.contract_id, 0);
        assert_eq!(context.caller_contract_id, 0);
        assert_eq!(context.checkpoint_id, 0);
        assert_eq!(context.nonce, 0);
        assert_eq!(context.user_public_key_hash, [0; 4]);

        let context = ExecutionContext::from(ExecutionContextInput {
            user_id: Some(7),
            contract_id: Some(9),
            caller_contract_id: Some(11),
            checkpoint_id: Some(13),
            nonce: Some(17),
            user_public_key_hash: Some([1, 2, 3, 4]),
        });
        assert_eq!(context.user_id, 7);
        assert_eq!(context.contract_id, 9);
        assert_eq!(context.caller_contract_id, 11);
        assert_eq!(context.checkpoint_id, 13);
        assert_eq!(context.nonce, 17);
        assert_eq!(context.user_public_key_hash, [1, 2, 3, 4]);

        let default = default_execution_context();
        assert_eq!(default.user_id, 0);
        assert_eq!(default.checkpoint_id, 0);
    }

    #[derive(serde::Deserialize)]
    struct TestInterpretResult {
        success: bool,
        error: Option<String>,
        error_offset: Option<usize>,
        entry_path: Option<String>,
        execution_result: Option<serde_json::Value>,
        outputs: Option<Vec<u64>>,
    }

    #[test]
    #[serial]
    fn interpret_source_runs_main_with_inputs_and_outputs() {
        let result: TestInterpretResult =
            serde_json::from_str(&interpret_source("fn main(q: Felt) -> Felt { return q + 1; }", r#"{"inputs":[7]}"#)).unwrap();

        assert!(result.success, "expected interpret success, got {:?}", result.error);
        assert_eq!(result.entry_path.as_deref(), Some("/vfs/src/main.psy"));
        let execution = result.execution_result.expect("missing execution_result");
        assert!(execution["success"].as_bool().is_some_and(|ok| ok), "execution failed: {execution}");
        assert_eq!(execution["outputs"].as_array().map(|items| items.len()), Some(1));
        assert_eq!(execution["outputs"][0].as_u64(), Some(8));
    }

    #[test]
    #[serial]
    fn interpret_source_honors_execution_context_and_hydrates_every_initial_state_kind() {
        let source = r#"
            use std::prelude::*;

            fn main() -> Felt {
                return get_user_id();
            }
        "#;
        let request = serde_json::json!({
            "inputs": [],
            "execution_context": { "user_id": 42 },
            "initial_state": {
                "slots": [{ "user_id": 1, "contract_id": 1, "slot_index": 0, "value": 7 }],
                "hashes": [{ "user_id": 1, "contract_id": 1, "slot_index": 0, "value": [1, 2, 3, 4] }],
                "deployers": [{ "contract_id": 1, "deployer": [1, 2, 3, 4] }],
                "checkpoint_stats": [{ "checkpoint_id": 1, "values": [1, 2] }],
                "contract_leaves": [{ "contract_id": 1, "values": [1, 2, 3, 4] }],
                "checkpoint_global_state_roots": [{ "checkpoint_id": 1, "values": [1, 2] }],
                "imt": [{ "user_id": 1, "contract_id": 1, "key": [1, 2, 3, 4], "value": [5, 6, 7, 8] }]
            }
        });

        let result: TestInterpretResult =
            serde_json::from_str(&interpret_source(source, &request.to_string())).unwrap();

        assert!(result.success, "expected interpret success, got {:?}", result.error);
        let execution = result.execution_result.expect("missing execution_result");
        assert_eq!(execution["outputs"][0].as_u64(), Some(42), "execution context user_id must reach the VM: {execution}");
    }

    #[test]
    #[serial]
    fn interpret_source_reports_invalid_requests_and_compile_errors() {
        let result: TestInterpretResult = serde_json::from_str(&interpret_source("fn main() {}", "not json")).unwrap();
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or_default().contains("Invalid interpret request JSON"), "{:?}", result.error);

        let result: TestInterpretResult =
            serde_json::from_str(&interpret_source("fn main( { }", r#"{"inputs":[]}"#)).unwrap();
        assert!(!result.success, "expected the parse error to fail interpretation");
        assert!(
            result.error_offset.is_some(),
            "expected a parse error offset; error was {:?}",
            result.error
        );
    }

    #[test]
    #[serial]
    fn interpret_project_interprets_files_json_and_reports_invalid_input() {
        let files = serde_json::json!({
            "entry": ["main"],
            "files": [[["main"], "fn main(q: Felt) -> Felt { return q * 2; }"]]
        });

        let result: TestInterpretResult =
            serde_json::from_str(&interpret_project(&files.to_string(), r#"{"inputs":[21]}"#)).unwrap();
        assert!(result.success, "expected interpret success, got {:?}", result.error);
        let execution = result.execution_result.expect("missing execution_result");
        assert_eq!(execution["outputs"][0].as_u64(), Some(42));

        let result: TestInterpretResult = serde_json::from_str(&interpret_project("{ bad json", r#"{}"#)).unwrap();
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or_default().contains("Invalid files JSON"), "{:?}", result.error);

        let result: TestInterpretResult =
            serde_json::from_str(&interpret_project(&files.to_string(), "{ bad json")).unwrap();
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or_default().contains("Invalid interpret request JSON"), "{:?}", result.error);
    }

    #[test]
    #[serial]
    fn chain_contract_lifecycle_covers_deploy_call_state_and_log() {
        init_chain();
        *LAST_COMPILE.lock().unwrap() = None;

        // Deploying before any compile is rejected.
        let result: serde_json::Value = serde_json::from_str(&deploy_contract(1)).unwrap();
        assert_eq!(result["success"], false);
        assert_eq!(result["error"].as_str(), Some("No compiled contract. Compile first."));

        // A compiled contract with a writer and a reader method.
        let compiled = parse_result(&compile_source(
            r#"
            #[contract]
            #[derive(Storage)]
            pub struct LifecycleContract {
                pub value: Felt,
            }

            #[contract::write_method]
            pub fn set_value(value: Felt) {
                let c = LifecycleContractRef::new(ContractMetadata::current());
                c.value = value;
            }

            #[contract::view_method]
            pub fn get_value() -> Felt {
                let c = LifecycleContractRef::new(ContractMetadata::current());
                c.value.get()
            }
            "#,
        ));
        assert!(compiled.success, "fixture must compile, got {:?}", compiled.error);

        let alice: serde_json::Value = serde_json::from_str(&create_account("Alice")).unwrap();
        let alice_id = alice["user_id"].as_u64().unwrap();

        // Deploying under an unknown account is rejected; under Alice it works.
        let result: serde_json::Value = serde_json::from_str(&deploy_contract(999)).unwrap();
        assert_eq!(result["success"], false);
        assert_eq!(result["error"].as_str(), Some("Account with ID 999 not found"));

        let result: serde_json::Value = serde_json::from_str(&deploy_contract(alice_id)).unwrap();
        assert_eq!(result["success"], true, "{result}");
        let contract_id = result["contract_id"].as_u64().unwrap();

        let contracts: serde_json::Value = serde_json::from_str(&get_contracts()).unwrap();
        let contracts = contracts.as_array().expect("contracts must be an array");
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0]["name"].as_str(), Some("LifecycleContract"));
        assert_eq!(contracts[0]["deployer_id"].as_u64(), Some(alice_id));

        let abi: serde_json::Value = serde_json::from_str(&get_contract_abi(contract_id)).unwrap();
        assert_eq!(abi["contract"]["name"].as_str(), Some("LifecycleContract"));
        let missing_abi: serde_json::Value = serde_json::from_str(&get_contract_abi(999)).unwrap();
        assert!(missing_abi["error"].as_str().is_some_and(|msg| msg.contains("Contract 999 not found")));

        // Invalid args JSON is rejected before execution.
        let result: serde_json::Value =
            serde_json::from_str(&call_contract(alice_id, contract_id, "set_value", "{bad json")).unwrap();
        assert_eq!(result["success"], false);
        assert!(result["error"].as_str().is_some_and(|msg| msg.contains("Invalid args")));

        // Unknown contract / method names produce explicit errors.
        let result: serde_json::Value = serde_json::from_str(&call_contract(alice_id, 999, "set_value", "[]")).unwrap();
        assert!(result["error"].as_str().is_some_and(|msg| msg.contains("Contract 999 not found")));
        let result: serde_json::Value =
            serde_json::from_str(&call_contract(alice_id, contract_id, "missing_method", "[]")).unwrap();
        assert!(result["error"].as_str().is_some_and(|msg| msg.contains("Method 'missing_method' not found")));

        // Write, then read the value back through the view method.
        let result: serde_json::Value = serde_json::from_str(&call_contract(alice_id, contract_id, "set_value", "[5]")).unwrap();
        assert_eq!(result["success"], true, "set_value failed: {result}");

        let result: serde_json::Value = serde_json::from_str(&call_contract(alice_id, contract_id, "get_value", "[]")).unwrap();
        assert_eq!(result["success"], true, "get_value failed: {result}");
        assert_eq!(
            result["outputs"].as_array().and_then(|items| items.first().and_then(serde_json::Value::as_u64)),
            Some(5),
            "stored value must be readable: {result}"
        );

        // State inspection endpoints.
        let entries: serde_json::Value = serde_json::from_str(&read_contract_state(contract_id, alice_id)).unwrap();
        assert!(entries.is_array(), "state entries must be an array: {entries}");
        let missing_state: serde_json::Value = serde_json::from_str(&read_contract_state(999, alice_id)).unwrap();
        assert!(missing_state["error"].as_str().is_some_and(|msg| msg.contains("Contract 999 not found")));

        let imt: serde_json::Value = serde_json::from_str(&read_imt_state(contract_id as u32, alice_id as u32)).unwrap();
        assert!(imt.is_array(), "imt entries must be an array: {imt}");
        let missing_imt: serde_json::Value = serde_json::from_str(&read_imt_state(999, 1)).unwrap();
        assert!(missing_imt["error"].as_str().is_some_and(|msg| msg.contains("Contract 999 not found")));

        // Both successful calls are recorded in order.
        let log: serde_json::Value = serde_json::from_str(&get_transaction_log()).unwrap();
        let log = log.as_array().expect("transaction log must be an array");
        assert_eq!(log.len(), 2, "expected one record per call: {log:?}");
        assert_eq!(log[0]["method_name"].as_str(), Some("set_value"));
        assert_eq!(log[1]["method_name"].as_str(), Some("get_value"));
        assert_eq!(log[0]["caller_name"].as_str(), Some("Alice"));
        assert_eq!(log[0]["contract_name"].as_str(), Some("LifecycleContract"));
        assert_eq!(log[0]["success"], true);
    }

    #[test]
    #[serial]
    fn chain_operations_require_initialization() {
        *CHAIN.lock().unwrap() = None;

        for result in [
            serde_json::from_str::<serde_json::Value>(&get_contracts()).unwrap(),
            serde_json::from_str::<serde_json::Value>(&get_transaction_log()).unwrap(),
            serde_json::from_str::<serde_json::Value>(&get_contract_abi(1)).unwrap(),
            serde_json::from_str::<serde_json::Value>(&read_imt_state(1, 1)).unwrap(),
            serde_json::from_str::<serde_json::Value>(&create_account("Uninitialized")).unwrap(),
            serde_json::from_str::<serde_json::Value>(&get_accounts()).unwrap(),
        ] {
            assert_eq!(
                result["error"].as_str(),
                Some("Chain not initialized. Call init_chain() first."),
                "uninitialized chain must produce the explicit error: {result}"
            );
        }

        // Restore a fresh chain for any test that runs afterwards.
        init_chain();
    }

    fn dargo_project(source: &str) -> serde_json::Value {
        serde_json::json!({
            "root": "root",
            "packages": [
                {
                    "id": "root",
                    "manifest": "[package]\nname = \"root\"\ntype = \"bin\"\n",
                    "files": { "src/main.psy": source },
                    "dependencies": {}
                }
            ]
        })
    }

    // NOTE: `main`/`init_logging`/`init_psy_ide` are deliberately NOT tested:
    // they install wasm-only logger/tracing subscribers that abort the native
    // test binary when any later `tracing::warn!` fires.

    #[test]
    fn compile_project_rejects_invalid_files_json() {
        let result = parse_result(&compile_project("not json"));
        assert!(!result.success);
        assert!(
            result.error.as_deref().unwrap_or_default().contains("Invalid files JSON"),
            "{:?}",
            result.error
        );
    }

    #[test]
    fn compile_dargo_project_reports_malformed_and_unresolvable_inputs() {
        let result = parse_result(&compile_dargo_project("not json"));
        assert!(!result.success);
        assert!(
            result.error.as_deref().unwrap_or_default().contains("Invalid dargo project JSON"),
            "{:?}",
            result.error
        );

        // Empty method list is rejected instead of silently compiling nothing.
        let mut project = dargo_project("fn main() { assert_eq(1, 1, \"ok\"); }");
        project["method_names"] = serde_json::json!([]);
        let result = parse_result(&compile_dargo_project(&project.to_string()));
        assert!(!result.success);
        assert!(
            result.error.as_deref().unwrap_or_default().contains("method_names must not be empty"),
            "{:?}",
            result.error
        );

        // A manifest whose entry file does not exist cannot resolve a workspace.
        let mut missing_entry = dargo_project("");
        missing_entry["packages"][0]["manifest"] =
            serde_json::json!("[package]\nname = \"root\"\ntype = \"bin\"\nentry = \"src/missing.psy\"\n");
        let result = parse_result(&compile_dargo_project(&missing_entry.to_string()));
        assert!(!result.success, "a missing entry file must fail resolution");
    }

    #[test]
    #[serial]
    fn compile_dargo_project_reports_source_errors_with_an_offset() {
        let project = dargo_project("fn main() -> Felt { return true; }");
        let result = parse_result(&compile_dargo_project(&project.to_string()));
        assert!(!result.success, "the type error must fail compilation");
        assert!(
            result.error_offset.is_some() || result.error.as_deref().unwrap_or_default().contains("TypeMismatch"),
            "expected a diagnostic with an offset or type mismatch: {:?}",
            result.error
        );
    }

    #[test]
    #[serial]
    fn compile_dargo_project_registers_dependency_packages_and_edges() {
        // Two packages: the root depends on a library package. Registering the
        // dependency exercises the resolver dependency loop and the crate-graph
        // edge between the two entry files while still compiling successfully.
        let project = serde_json::json!({
            "root": "root",
            "method_names": ["main"],
            "packages": [
                {
                    "id": "root",
                    "manifest": "[package]\nname = \"root\"\ntype = \"bin\"\n\n[dependencies]\ndep = { path = \"../dep\" }\n",
                    "files": {
                        "src/main.psy": "#[contract]\npub struct C {}\n#[contract::write_method]\nfn main() { assert_eq(1, 1, \"ok\"); }"
                    },
                    "dependencies": { "dep": "dep" }
                },
                {
                    "id": "dep",
                    "manifest": "[package]\nname = \"dep\"\ntype = \"lib\"\n",
                    "files": { "src/lib.psy": "pub fn helper() {}" },
                    "dependencies": {}
                }
            ]
        });

        let result = parse_result(&compile_dargo_project(&project.to_string()));
        assert!(result.success, "dependency project must compile: {:?}", result.error);
        assert!(
            result.entry_path.as_deref().is_some_and(|path| path.contains("root")),
            "the root package entry must be reported: {:?}",
            result.entry_path
        );
    }

    #[test]
    #[serial]
    fn deploy_contract_requires_an_existing_account_even_after_compiling() {
        init_chain();
        *LAST_COMPILE.lock().unwrap() = None;

        let compiled = parse_result(&compile_source(
            "#[contract]\npub struct C {}\n#[contract::write_method]\nfn main() { assert_eq(1, 1, \"ok\"); }",
        ));
        assert!(compiled.success, "fixture must compile, got {:?}", compiled.error);

        let result: serde_json::Value = serde_json::from_str(&deploy_contract(9999)).unwrap();
        assert_eq!(result["success"], false);
        assert_eq!(
            result["error"].as_str(),
            Some("Account with ID 9999 not found"),
            "deploying as a nonexistent account must be rejected: {result}"
        );

        *LAST_COMPILE.lock().unwrap() = None;
    }

    #[test]
    #[serial]
    fn interpret_project_reports_missing_method_and_input_mismatch() {
        #[derive(serde::Deserialize)]
        struct TestInterpretResult {
            success: bool,
            error: Option<String>,
        }

        let files = serde_json::json!({
            "entry": ["main"],
            "files": [[["main"], "fn main(q: Felt) -> Felt { return q * 2; }"]]
        });

        // A method name that matches no circuit fails interpretation with an
        // "undefined function" diagnostic rather than an empty success.
        let result: TestInterpretResult = serde_json::from_str(&interpret_project(
            &files.to_string(),
            r#"{"method_name": "does_not_exist", "inputs": []}"#,
        ))
        .unwrap();
        assert!(!result.success, "unknown method must not interpret: {:?}", result.error);
        assert!(
            result.error.as_deref().unwrap_or_default().to_lowercase().contains("undefined function"),
            "{:?}",
            result.error
        );

        // A failing constant assertion is rejected before execution.
        let failing = serde_json::json!({
            "entry": ["main"],
            "files": [[["main"], "fn main() { assert(false, \"boom\"); }"]]
        });
        let result: TestInterpretResult =
            serde_json::from_str(&interpret_project(&failing.to_string(), r#"{"inputs": []}"#)).unwrap();
        assert!(!result.success, "a failing assert must fail execution: {:?}", result.error);

        // NOTE: a *runtime* assertion failure (e.g. `assert(q == 0)` with
        // input 5) executes to Ok with a `failure` record inside
        // execution_result — it does NOT take the executor error path, so it
        // is deliberately not asserted as `success == false` here.
    }

    #[test]
    fn interpret_project_reports_vfs_build_errors() {
        #[derive(serde::Deserialize)]
        struct TestInterpretResult {
            success: bool,
            error: Option<String>,
        }

        // A project without any files cannot build a VFS.
        let empty = serde_json::json!({ "entry": ["main"], "files": [] });
        let result: TestInterpretResult =
            serde_json::from_str(&interpret_project(&empty.to_string(), r#"{"inputs": []}"#)).unwrap();
        assert!(!result.success);
        assert!(
            result.error.as_deref().unwrap_or_default().contains("at least one file"),
            "{:?}",
            result.error
        );

        // An entry module that is not among the uploaded files is rejected.
        let missing_entry = serde_json::json!({ "entry": ["main"], "files": [[["other"], "fn main() {}"]] });
        let result: TestInterpretResult =
            serde_json::from_str(&interpret_project(&missing_entry.to_string(), r#"{"inputs": []}"#)).unwrap();
        assert!(!result.success);
        assert!(
            result.error.as_deref().unwrap_or_default().contains("entry file was not found"),
            "{:?}",
            result.error
        );
    }

    // NOTE: circuit input arity is deliberately not asserted here — the VM
    // executor tolerates both extra inputs and missing ones (defaults to 0),
    // so `execute_circuit` only fails on genuine execution errors.

    #[test]
    #[serial]
    fn deploy_contract_requires_a_cached_compile() {
        init_chain();
        let account: serde_json::Value = serde_json::from_str(&create_account("Carol")).unwrap();
        let carol_id = account["user_id"].as_u64().unwrap();

        *LAST_COMPILE.lock().unwrap() = None;
        let result: serde_json::Value = serde_json::from_str(&deploy_contract(carol_id)).unwrap();
        assert_eq!(result["success"], false);
        assert_eq!(
            result["error"].as_str(),
            Some("No compiled contract. Compile first."),
            "deploy without a compile must be rejected: {result}"
        );
    }

    #[test]
    fn serialize_result_adds_backward_compatible_aliases() {
        let result = serialize_result(JsCompileResult {
            success: true,
            error: None,
            error_offset: None,
            entry_path: Some("main.psy".to_string()),
            compile_results: Some(serde_json::json!([{ "name": "main" }])),
            contract_code: Some(serde_json::json!({ "state_tree_height": 12 })),
            abi: None,
        });
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["circuit_definitions"], serde_json::json!([{ "name": "main" }]));
        assert_eq!(value["circuitDefinitions"], serde_json::json!([{ "name": "main" }]));
        assert_eq!(value["method_count"], 1);
        assert_eq!(value["methodCount"], 1);
        assert_eq!(value["state_tree_height"], 12);
        assert_eq!(value["stateTreeHeight"], 12);
    }

    #[test]
    #[serial]
    fn call_contract_reports_unknown_callers_and_runtime_failures() {
        init_chain();
        *LAST_COMPILE.lock().unwrap() = None;

        let compiled = parse_result(&compile_source(
            r#"
            #[contract]
            #[derive(Storage)]
            pub struct GuardContract {
                pub value: Felt,
            }

            #[contract::write_method]
            pub fn set_if_zero(value: Felt) {
                assert(value == 0, "value must be zero");
                let c = GuardContractRef::new(ContractMetadata::current());
                c.value = value;
            }
            "#,
        ));
        assert!(compiled.success, "fixture must compile, got {:?}", compiled.error);

        let alice: serde_json::Value = serde_json::from_str(&create_account("Alice")).unwrap();
        let alice_id = alice["user_id"].as_u64().unwrap();
        let result: serde_json::Value = serde_json::from_str(&deploy_contract(alice_id)).unwrap();
        assert_eq!(result["success"], true, "{result}");
        let contract_id = result["contract_id"].as_u64().unwrap();

        // A caller without an account falls back to a synthesized name and the
        // call still succeeds.
        let result: serde_json::Value =
            serde_json::from_str(&call_contract(999, contract_id, "set_if_zero", "[0]")).unwrap();
        assert_eq!(result["success"], true, "zero input must pass the guard: {result}");

        // A runtime assertion failure records a failure message instead of an error.
        let result: serde_json::Value =
            serde_json::from_str(&call_contract(alice_id, contract_id, "set_if_zero", "[5]")).unwrap();
        assert_eq!(result["success"], false, "guard must reject five: {result}");
        assert!(
            result["failure_message"].as_str().is_some_and(|msg| msg.contains("value must be zero")),
            "runtime failure must carry the assert message: {result}"
        );

        let log: serde_json::Value = serde_json::from_str(&get_transaction_log()).unwrap();
        let log = log.as_array().expect("transaction log must be an array");
        assert_eq!(log.len(), 2, "expected one record per call: {log:?}");
        assert_eq!(log[0]["caller_name"].as_str(), Some("User 999"));
        assert_eq!(log[0]["success"], true);
        assert!(
            log[1]["failure_message"]
                .as_str()
                .is_some_and(|msg| msg.contains("value must be zero")),
            "log must record the runtime failure: {log:?}"
        );
        assert_eq!(log[1]["success"], false);
    }

    /// Regression: a tuple-typed entry parameter used to panic with
    /// "primitive scope has not been initialized" because `to_input` still read
    /// the retired process-global primitive scope instead of the symbol table.
    #[test]
    #[serial]
    fn tuple_entry_inputs_compile() {
        init_chain();
        *LAST_COMPILE.lock().unwrap() = None;

        let result = parse_result(&compile_source(
            "fn main(t: (Felt, Felt)) -> Felt { return t.0; }",
        ));
        assert!(result.success, "tuple entry input must compile: {:?}", result.error);
        assert_eq!(result.method_count, Some(1));
    }

    #[test]
    #[serial]
    fn read_imt_state_lists_entries_written_by_contract_calls() {
        init_chain();
        *LAST_COMPILE.lock().unwrap() = None;

        let compiled = parse_result(&compile_source(
            r#"
            #[contract]
            #[derive(Storage)]
            pub struct RegistryContract {
                pub padding: Felt,
            }

            #[contract::write_method]
            pub fn register(key: Felt) {
                let k: Hash = [key, 0, 0, 0];
                let v: Hash = [key, 1, 2, 3];
                let offset: Felt = 0;
                let capacity: Felt = 128;
                imt_set(k, v, offset, capacity);
            }
            "#,
        ));
        assert!(compiled.success, "fixture must compile, got {:?}", compiled.error);

        let alice: serde_json::Value = serde_json::from_str(&create_account("Alice")).unwrap();
        let alice_id = alice["user_id"].as_u64().unwrap();
        let result: serde_json::Value = serde_json::from_str(&deploy_contract(alice_id)).unwrap();
        assert_eq!(result["success"], true, "{result}");
        let contract_id = result["contract_id"].as_u64().unwrap();

        let result: serde_json::Value =
            serde_json::from_str(&call_contract(alice_id, contract_id, "register", "[7001]")).unwrap();
        assert_eq!(result["success"], true, "register must execute: {result}");

        let imt: serde_json::Value = serde_json::from_str(&read_imt_state(contract_id as u32, alice_id as u32)).unwrap();
        let entries = imt.as_array().expect("imt entries must be an array");
        assert!(!entries.is_empty(), "imt write must be observable: {imt}");
        assert_eq!(entries[0]["key"].as_array().and_then(|k| k.first()).and_then(serde_json::Value::as_u64), Some(7001));
    }

    /// Regression: a function-typed entry parameter used to panic the compiler
    /// with "Unsupported type in to_input"; it must surface as a clean
    /// diagnostic instead.
    #[test]
    #[serial]
    fn fn_typed_entry_input_is_rejected_with_a_diagnostic() {
        init_chain();
        *LAST_COMPILE.lock().unwrap() = None;

        let result = parse_result(&compile_source(
            "fn main(f: fn(Felt) -> Felt) -> Felt { return f(1); }",
        ));
        assert!(!result.success, "fn-typed entry input must be rejected, got error: {:?}", result.error);
        assert!(
            result.error.as_deref().unwrap_or_default().contains("UnsupportedEntryPointInput"),
            "unexpected error: {:?}",
            result.error
        );
    }

    /// Regression: assertions under a *symbolic* branch condition (entry-input
    /// dependent) are witness-gated and satisfiable — they must compile. The
    /// eager constant-assert failure used to treat "not provably false" as
    /// "definitely executes" and wrongly rejected programs like
    /// `if a > b { assert(false) }`.
    #[test]
    #[serial]
    fn asserts_under_symbolic_branches_stay_gated() {
        init_chain();
        *LAST_COMPILE.lock().unwrap() = None;

        // Witness-dependent arm: satisfiable by choosing a <= b.
        let result = parse_result(&compile_source(
            "fn main(a: Felt, b: Felt) -> Felt {\n    if a > b {\n        assert(false, \"only reachable when a > b\");\n    };\n    return a + b;\n}\n",
        ));
        assert!(result.success, "symbolic-branch assert must stay gated: {:?}", result.error);

        // A provably dead arm nested under a symbolic arm cannot be proven
        // dead either (the conjunction does not fold), so it stays gated too.
        let result = parse_result(&compile_source(
            "fn main(a: Felt, b: Felt) -> Felt {\n    if a > b {\n        if false {\n            assert(false, \"dead arm\");\n        };\n    };\n    return a + b;\n}\n",
        ));
        assert!(result.success, "nested dead-arm assert must stay gated: {:?}", result.error);

        // A constant-false top-level arm stays gated (the original intent).
        let result = parse_result(&compile_source(
            "fn main() -> Felt {\n    if false {\n        assert(false, \"constant-false arm\");\n    };\n    return 1;\n}\n",
        ));
        assert!(result.success, "constant-false arm assert must stay gated: {:?}", result.error);
    }

    /// The eager failure itself must keep working where the branch IS provably
    /// executed: top-level and constant-true arms fail at compile time.
    #[test]
    #[serial]
    fn constant_asserts_in_definitely_executed_code_still_fail_eagerly() {
        init_chain();
        *LAST_COMPILE.lock().unwrap() = None;

        let result = parse_result(&compile_source(
            "fn main() -> Felt {\n    assert(false, \"top level must fail\");\n    return 1;\n}\n",
        ));
        assert!(!result.success, "top-level assert(false) must fail compilation");
        assert!(
            result.error.as_deref().unwrap_or_default().contains("top level must fail"),
            "unexpected error: {:?}",
            result.error
        );

        let result = parse_result(&compile_source(
            "fn main() -> Felt {\n    if 1 == 1 {\n        assert(false, \"constant-true arm must fail\");\n    };\n    return 1;\n}\n",
        ));
        assert!(!result.success, "constant-true arm assert(false) must fail compilation");
        assert!(
            result.error.as_deref().unwrap_or_default().contains("constant-true arm must fail"),
            "unexpected error: {:?}",
            result.error
        );
    }

    /// Regression: monomorphized function references (`id::<Felt>` and
    /// `mod::g::<Felt>` used as values) used to panic the compiler with
    /// `unreachable!()` in sema's generic-type arm; they must surface as a
    /// clean diagnostic instead.
    #[test]
    #[serial]
    fn turbofish_function_references_are_rejected_with_a_diagnostic() {
        init_chain();
        *LAST_COMPILE.lock().unwrap() = None;

        let result = parse_result(&compile_source(
            "fn id(x: Felt) -> Felt { return x; }\nfn main() -> Felt {\n    let f = id::<Felt>;\n    return f(1);\n}\n",
        ));
        assert!(!result.success, "fn-ref turbofish must be rejected, not panic");
        assert!(
            result.error.as_deref().unwrap_or_default().contains("generic arguments"),
            "unexpected error: {:?}",
            result.error
        );

        let result = parse_result(&compile_source(
            "pub mod m { pub fn g<T>(x: T) -> Felt { return 1; } }\nfn main() -> Felt {\n    let v = m::g::<Felt>;\n    return v(2);\n}\n",
        ));
        assert!(!result.success, "mod fn-ref turbofish must be rejected, not panic");
        assert!(
            result.error.as_deref().unwrap_or_default().contains("generic arguments"),
            "unexpected error: {:?}",
            result.error
        );
    }
}
