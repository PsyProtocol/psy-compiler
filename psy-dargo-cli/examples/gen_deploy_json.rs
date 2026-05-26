/// Tool to convert one or more compiled contract JSON files (Vec<DPNFunctionCircuitDefinition>)
/// into genesis_contracts.json (Vec<PQBCDeployContract>) for use in genesis config.
///
/// Usage:
///   cargo run --release --example gen_deploy_json -- <output.json> <input1.json>[:<deployer_hex>[:<name>]] [<input2.json>[:<deployer_hex>[:<name>]]] ...
///
/// Example:
///   cargo run --release --example gen_deploy_json -- \
///     ../../parth-generic-v1/genesis_contracts.json \
///     ../psy-precompiles/token/target/token.json:token \
///     ../psy-precompiles/mining_rewards/target/mining_rewards.json:mining_rewards
///
/// Each input can optionally specify deployer and name as: path:deployer_hex:name
/// If only deployer is given (path:deployer_hex), name defaults to the file stem
/// If neither deployer nor name is given (path), both default to their defaults

use std::{env, fs, io::Write, path::Path, str::FromStr};

use psy_common::data::qhashout::QHashOut;
use zstd::stream::write::Encoder;
use psy_data::config::store_config::{C, D};
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_vm::dpn::vm::def::DPNFunctionCircuitDefinition;

// Default genesis deployer (from existing genesis_contracts.json)
const DEFAULT_DEPLOYER: &str = "f83aa03c3e21321421696202b90f4dab0a9f87237c231bbba58b8f93c799126e";
const DEFAULT_STATE_TREE_HEIGHT: u8 = 32;

fn get_file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("contract")
        .to_string()
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: gen_deploy_json <output.json> <input1.json>[:<deployer_hex>[:<name>]] [<input2.json>[:<deployer_hex>[:<name>]]] ...");
        eprintln!();
        eprintln!("Each input can optionally specify deployer and name as: path:deployer_hex:name");
        eprintln!("Default deployer: {}", DEFAULT_DEPLOYER);
        eprintln!("Default name: file stem (e.g. token.json -> token)");
        std::process::exit(1);
    }

    let output_path = &args[1];
    let inputs = &args[2..];

    let mut contract_objects = Vec::new();

    for (i, input_arg) in inputs.iter().enumerate() {
        // Parse "path:deployer_hex:name" or "path:deployer_hex" or just "path"
        let parts: Vec<&str> = input_arg.split(':').collect();
        let input_path = parts[0];

        let (deployer_hex, name) = match parts.len() {
            1 => (DEFAULT_DEPLOYER, get_file_stem(input_path)),
            2 => (parts[1], get_file_stem(input_path)),
            3 => (parts[1], parts[2].to_string()),
            _ => {
                anyhow::bail!("Invalid input format: {}. Use path:deployer_hex:name", input_arg);
            }
        };

        let deployer = QHashOut::from_str(deployer_hex)
            .map_err(|e| anyhow::anyhow!("Invalid deployer hex for {}: {}", input_path, e))?;

        // Read input JSON (Vec<DPNFunctionCircuitDefinition>)
        let input_json = fs::read_to_string(input_path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", input_path, e))?;
        let defs: Vec<DPNFunctionCircuitDefinition> = serde_json::from_str(&input_json)
            .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", input_path, e))?;

        let (_circuits, deploy_contract) =
            gen_contract_deploy_and_circuits_for_functions::<C, D>(deployer, DEFAULT_STATE_TREE_HEIGHT, &defs)?;

        println!(
            "[{}] contract={} name={} functions={} whitelist={} deployer={}",
            i,
            input_path,
            name,
            defs.len(),
            deploy_contract.function_whitelist.len(),
            deployer_hex
        );

        // Serialize deploy_contract and wrap with name.
        // Manually construct the JSON to avoid serde_json's wide format for Vec<u64>,
        // which uses ~12 chars/val (12 spaces prefix) instead of ~3.5 chars/val.
        let mut contract_json_str = String::from("{\n");
        contract_json_str.push_str(&format!("    \"code_definition\": {{\n"));
        contract_json_str.push_str(&format!("      \"state_tree_height\": {},\n", deploy_contract.code_definition.state_tree_height));
        contract_json_str.push_str("      \"functions\": [\n");

        let functions = &deploy_contract.code_definition.functions;
        for (fi, func) in functions.iter().enumerate() {
            contract_json_str.push_str("        {\n");
            contract_json_str.push_str(&format!("          \"method_id\": {},\n", func.method_id));
            contract_json_str.push_str(&format!("          \"num_inputs\": {},\n", func.num_inputs));
            contract_json_str.push_str(&format!("          \"num_outputs\": {},\n", func.num_outputs));
            contract_json_str.push_str(&format!("          \"vm_type\": {},\n", func.vm_type));

            // Serialize code array compactly
            let code_str = serde_json::to_string(&func.code)?;
            contract_json_str.push_str("          \"code\": ");
            contract_json_str.push_str(&code_str);
            contract_json_str.push('\n');
            contract_json_str.push_str("        }");
            if fi + 1 != functions.len() {
                contract_json_str.push(',');
            }
            contract_json_str.push('\n');
        }

        contract_json_str.push_str("      ]\n");
        contract_json_str.push_str("    },\n");
        contract_json_str.push_str(&format!("    \"code_root\": \"{}\",\n", deploy_contract.code_root));
        contract_json_str.push_str(&format!("    \"deployer\": \"{}\",\n", deploy_contract.deployer));
        contract_json_str.push_str(&format!("    \"function_whitelist\": {},\n", &serde_json::to_string(&deploy_contract.function_whitelist).unwrap()));
        contract_json_str.push_str(&format!("    \"name\": \"{}\"\n", name));
        contract_json_str.push('}');

        contract_objects.push(contract_json_str);
    }

    let mut output_json = String::from("[\n");
    for (i, json_str) in contract_objects.iter().enumerate() {
        output_json.push_str("  ");
        output_json.push_str(json_str);
        if i + 1 != contract_objects.len() {
            output_json.push(',');
        }
        output_json.push('\n');
    }
    output_json.push(']');
    // Write as zstd-compressed JSON
    let mut encoder = Encoder::new(
        fs::File::create(output_path)
            .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", output_path, e))?,
        9, // max compression
    )
    .map_err(|e| anyhow::anyhow!("Failed to create zstd encoder: {}", e))?;
    encoder
        .write_all(output_json.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", output_path, e))?;
    encoder
        .finish()
        .map_err(|e| anyhow::anyhow!("Failed to finish zstd: {}", e))?;

    println!();
    println!("Written {} contract(s) to {}", contract_objects.len(), output_path);

    Ok(())
}


