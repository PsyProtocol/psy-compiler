use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use minijinja::{context, Environment};
use psy_config::{ContractConfig, PrecompilesBuildConfigGoldilocks};

fn main() -> Result<()> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let out_dir = env::var("OUT_DIR")?;
    let out_path = std::path::Path::new(&out_dir);

    // Use local precompiles.json configuration
    let precompiles_config_path = Path::new(&manifest_dir).join("precompiles.json");

    println!("cargo:rerun-if-changed={}", precompiles_config_path.display());

    // Load precompiles configuration
    let config_content = fs::read_to_string(&precompiles_config_path).map_err(|e| anyhow::anyhow!("Failed to read precompiles.json: {}", e))?;

    let config: PrecompilesBuildConfigGoldilocks =
        serde_json::from_str(&config_content).map_err(|e| anyhow::anyhow!("Failed to parse precompiles.json: {}", e))?;

    for contract in &config.contracts {
        println!("cargo:rerun-if-changed={}/", contract.path);
    }

    for contract in &config.contracts {
        let contract_dir = Path::new(&manifest_dir).join(&contract.path);

        if !contract_dir.exists() {
            println!("cargo:warning=Contract directory {} does not exist", contract_dir.display());
            continue;
        }

        let dargo_toml = contract_dir.join("Dargo.toml");
        if !dargo_toml.exists() {
            println!("cargo:warning=Dargo.toml not found at {}", dargo_toml.display());
            continue;
        }

        let main_psy = contract_dir.join("src/main.psy");
        if !main_psy.exists() {
            println!("cargo:warning=src/main.psy not found at {}", main_psy.display());
            continue;
        }

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
            method_names: contract.method_names.clone(),
            entry_path: Some(main_psy),
            debug: false,
        };

        with_workspace(compile_options, dargo_config, |opts, workspace| {
            use dargo::cli::resolve_crate_path_graph;
            let crate_path_graph = resolve_crate_path_graph(&workspace, opts.entry_path.clone());
            match psy_interpreter::interpret(opts.contract_name.clone(), opts.method_names.clone(), crate_path_graph) {
                Ok(mut interpret_result) => {
                    use dargo::cli::doc_cmd::extract_function_metadata_from_context;
                    let _function_metadata = extract_function_metadata_from_context(
                        &mut interpret_result.ctx,
                        &mut interpret_result.typechecker,
                        &interpret_result.compile_results,
                    );

                    use dargo::cli::save_build_artifact_to_file;

                    if let Err(e) = save_build_artifact_to_file(&interpret_result.compile_results, &contract.name, &target_dir) {
                        println!("cargo:warning=Failed to save build artifact to target dir for {}: {}", contract.name, e);
                    }

                    let out_dir = env::var("OUT_DIR").map_err(anyhow::Error::from)?;
                    let out_path = std::path::Path::new(&out_dir);

                    if let Err(e) = save_build_artifact_to_file(&interpret_result.compile_results, &contract.name, out_path) {
                        println!("cargo:warning=Failed to save build artifact to OUT_DIR for {}: {}", contract.name, e);
                    }

                    Ok(())
                }
                Err(e) => {
                    println!("cargo:warning=Failed to compile {} contract: {}", contract.name, e);
                    Err(e.into())
                }
            }
        })?;
    }

    generate_precompile_api(&config, out_path)?;
    generate_precompiled_contracts(&config, out_path)?;

    Ok(())
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
    serde_json::from_str(json_data).unwrap_or_else(|_| Vec::new())
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
