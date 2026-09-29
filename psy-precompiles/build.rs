use std::{
    env, fs,
    path::Path,
};

use anyhow::Result;
use minijinja::{context, Environment};
use psy_config::{ContractConfigGoldilocks, PrecompilesBuildConfigGoldilocks};
use psy_abi::AbiExtractor;

fn main() -> Result<()> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let out_dir = env::var("OUT_DIR")?;
    let out_path = std::path::Path::new(&out_dir);

    // Use local precompiles.json configuration
    let precompiles_config_path = Path::new(&manifest_dir).join("precompiles.json");

    println!("cargo:rerun-if-changed={}", precompiles_config_path.display());
    let config = load_config(&precompiles_config_path)?;

    for contract in &config.contracts {
        println!("cargo:rerun-if-changed={}/", contract.path);
    }

    for contract in &config.contracts {
        compile_one_contract(contract, &manifest_dir, out_path)?;
    }

    generate_precompile_api(&config, out_path)?;
    generate_precompiled_contracts(&config, out_path)?;

    Ok(())
}

fn load_config(precompiles_config_path: &Path) -> Result<PrecompilesBuildConfigGoldilocks> {
    let config_content = fs::read_to_string(precompiles_config_path)
        .map_err(|e| anyhow::anyhow!("Failed to read precompiles.json: {}", e))?;
    serde_json::from_str(&config_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse precompiles.json: {}", e))
}

fn compile_one_contract(contract: &ContractConfigGoldilocks, manifest_dir: &str, out_path: &Path) -> Result<()> {
    let contract_dir = Path::new(manifest_dir).join(&contract.path);

    anyhow::ensure!(contract_dir.is_dir(), "Required contract directory {} does not exist", contract_dir.display());
    let dargo_toml = contract_dir.join("Dargo.toml");
    anyhow::ensure!(dargo_toml.is_file(), "Required Dargo.toml not found at {}", dargo_toml.display());
    let main_psy = contract_dir.join("src/main.psy");
    anyhow::ensure!(main_psy.is_file(), "Required src/main.psy not found at {}", main_psy.display());

    use dargo::{
        cli::{with_workspace, DargoConfig},
        compile_cmd::CompileOptions,
    };

    let target_dir = contract_dir.join("target");
    let dargo_config = DargoConfig {
        program_dir: contract_dir.clone(),
        target_dir: Some(target_dir.clone()),
    };

    let compile_options = CompileOptions {
        contract_name: Some(contract.contract_name.clone()),
        method_names: Some(contract.method_names.clone()),
        entry_path: Some(main_psy),
        debug: false,
    };

    Ok(with_workspace(compile_options, dargo_config, |opts, workspace| {
        use dargo::cli::resolve_crate_path_graph;
        let crate_path_graph = resolve_crate_path_graph(&workspace, opts.entry_path.clone());
        match psy_interpreter::interpret(opts.contract_name.clone(), opts.method_names.clone().unwrap_or_default(), crate_path_graph) {
            Ok(mut interpret_result) => {
                use dargo::cli::doc_cmd::extract_function_metadata_from_context;
                let _function_metadata = extract_function_metadata_from_context(
                    &mut interpret_result.ctx,
                    &mut interpret_result.typechecker,
                    &interpret_result.compile_results,
                );

                let extractor = AbiExtractor::new(contract.contract_name.clone());
                let inferred_height = extractor.compute_state_tree_height(&mut interpret_result.ctx.program);
                let state_tree_height = match contract.declared_state_tree_height {
                    Some(declared) => {
                        if inferred_height > u16::from(declared) || declared > 32 {
                            return Err(anyhow::anyhow!("Invalid declared state tree height {} for {} (inferred {})", declared, contract.name, inferred_height).into());
                        }
                        u16::from(declared)
                    }
                    None => inferred_height,
                };
                if state_tree_height > 32 {
                    return Err(anyhow::anyhow!("Invalid state tree height for {}", contract.name).into());
                }
                if interpret_result.compile_results.len() != contract.method_names.len()
                    || contract.method_names.iter().any(|name| interpret_result.compile_results.iter().filter(|function| &function.name == name).count() != 1) {
                    return Err(anyhow::anyhow!("Required precompile methods missing or duplicated for {}", contract.name).into());
                }
                let method_metadata = interpret_result.compile_results.iter()
                    .map(|f| (f.name.clone(), (f.method_id, f.is_view_function())))
                    .collect();
                let abi = extractor.extract_abi(&mut interpret_result.ctx.program, state_tree_height, &method_metadata)?;
                if abi.contract.state_tree_height != state_tree_height {
                    return Err(anyhow::anyhow!("ABI/artifact state tree height mismatch for {}", contract.name).into());
                }
                let artifact = serde_json::json!({
                    "state_tree_height": state_tree_height,
                    "circuit_definitions": &interpret_result.compile_results,
                    "abi": &abi,
                });
                use dargo::cli::save_build_artifact_to_file;
                save_build_artifact_to_file(&artifact, &contract.name, &target_dir)?;
                save_build_artifact_to_file(&artifact, &contract.name, out_path)?;
                let abi_name = format!("{}.abi", contract.name);
                save_build_artifact_to_file(&abi, &abi_name, &target_dir)?;
                save_build_artifact_to_file(&abi, &abi_name, out_path)?;
                Ok(())
            }
            Err(e) => {
                println!("cargo:warning=Failed to compile {} contract: {}", contract.name, e);
                Err(e.into())
            }
        }
    })?)
}


fn generate_precompile_api(config: &PrecompilesBuildConfigGoldilocks, out_path: &Path) -> Result<()> {
    let mut env = Environment::new();

    let template = r#"use psy_vm::dpn::vm::def::DPNFunctionCircuitDefinition;

// Generated at compile time - zero runtime parsing cost!
// Direct embedded contract definitions - no JSON parsing at runtime!

{% for contract in contracts -%}
// Runtime lazy loading for {{ contract.name }} contract
fn load_{{ contract.name }}_functions() -> Vec<DPNFunctionCircuitDefinition> {
    let json_data = include_str!(concat!(env!("OUT_DIR"), "/{{ contract.name }}.json"));
    let artifact: serde_json::Value = serde_json::from_str(json_data).expect("Invalid required precompile artifact");
    assert_eq!(artifact["state_tree_height"], artifact["abi"]["contract"]["state_tree_height"]);
    {% if contract.declared_state_tree_height is defined and contract.declared_state_tree_height is not none %}
    assert_eq!(artifact["state_tree_height"], {{ contract.declared_state_tree_height }});
    {% endif %}
    serde_json::from_value(artifact["circuit_definitions"].clone()).expect("Invalid required precompile functions")
}

static {{ contract.name | upper }}_FUNCTIONS_ONCE: std::sync::OnceLock<Vec<DPNFunctionCircuitDefinition>> = std::sync::OnceLock::new();

pub fn get_{{ contract.name }}_functions() -> &'static Vec<DPNFunctionCircuitDefinition> {
    {{ contract.name | upper }}_FUNCTIONS_ONCE.get_or_init(load_{{ contract.name }}_functions)
}

{% endfor -%}

/// Get precompiled contract functions by contract ID
pub fn get_precompiled_contract_functions(contract_id: u32) -> Option<&'static Vec<DPNFunctionCircuitDefinition>> {
    match contract_id {
        {%- for contract in contracts %}
        {{ contract.name | upper }}_CONTRACT_ID => Some(get_{{ contract.name }}_functions()),
        {%- endfor %}
        _ => None,
    }
}

/// Get contract ID by name
pub fn get_contract_id_by_name(contract_name: &str) -> Option<u32> {
    match contract_name {
        {%- for contract in contracts %}
        "{{ contract.name }}" => Some({{ contract.name | upper }}_CONTRACT_ID),
        {%- endfor %}
        _ => None,
    }
}

/// Get a specific function from a contract by name
pub fn get_precompiled_contract_function_by_name(
    contract_name: &str,
    method_name: &str
) -> Option<&'static DPNFunctionCircuitDefinition> {
    let contract_id = get_contract_id_by_name(contract_name)?;
    let functions = get_precompiled_contract_functions(contract_id)?;

    functions.iter().find(|f| f.name == method_name)
}

/// List all available contract names
pub fn list_available_contracts() -> Vec<String> {
    vec![
        {%- for contract in contracts %}
        "{{ contract.name }}".to_string(),
        {%- endfor %}
    ]
}

/// List all methods for a specific contract
pub fn list_contract_methods(contract_name: &str) -> Vec<String> {
    if let Some(contract_id) = get_contract_id_by_name(contract_name) {
        if let Some(functions) = get_precompiled_contract_functions(contract_id) {
            return functions.iter().map(|f| f.name.clone()).collect();
        }
    }
    Vec::new()
}
"#;

    env.add_template("api", template)?;
    let tmpl = env.get_template("api")?;

    let api_code = tmpl.render(context! {
        contracts => &config.contracts
    })?;

    let api_file = out_path.join("precompile_api.rs");
    std::fs::write(api_file, api_code)?;

    Ok(())
}

fn generate_precompiled_contracts(config: &PrecompilesBuildConfigGoldilocks, out_path: &Path) -> Result<()> {
    let mut constants = String::new();

    constants.push_str("// Generated contract ID constants\n\n");

    for (index, contract) in config.contracts.iter().enumerate() {
        constants.push_str(&format!("pub const {}_CONTRACT_ID: u32 = {};\n", contract.name.to_uppercase(), index));
    }

    let contracts_file = out_path.join("precompiled_contracts.rs");
    std::fs::write(contracts_file, constants)?;

    Ok(())
}
