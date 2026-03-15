/// Tool to convert one or more compiled contract JSON files (Vec<DPNFunctionCircuitDefinition>)
/// into genesis_contracts.json (Vec<PQBCDeployContract>) for use in genesis config.
///
/// Usage:
///   cargo run --release --example gen_deploy_json -- <output.json> <input1.json>[:<deployer_hex>] [<input2.json>[:<deployer_hex>]] ...
///
/// Example (two contracts):
///   cargo run --release --example gen_deploy_json -- \
///     ../../parth-generic-v1/genesis_contracts.json \
///     ../psy-precompiles/token/target/token.json \
///     ../psy-precompiles/mining_rewards/target/mining_rewards.json

use std::{env, fs, str::FromStr};

use psy_common::data::qhashout::QHashOut;
use psy_data::config::store_config::{C, D};
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_vm::dpn::vm::def::DPNFunctionCircuitDefinition;

// Default genesis deployer (from existing genesis_contracts.json)
const DEFAULT_DEPLOYER: &str = "f83aa03c3e21321421696202b90f4dab0a9f87237c231bbba58b8f93c799126e";
const DEFAULT_STATE_TREE_HEIGHT: u8 = 32;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: gen_deploy_json <output.json> <input1.json>[:<deployer_hex>] [<input2.json>[:<deployer_hex>]] ...");
        eprintln!();
        eprintln!("Each input can optionally specify a deployer as: path:deployer_hex");
        eprintln!("Default deployer: {}", DEFAULT_DEPLOYER);
        eprintln!();
        eprintln!("Example:");
        eprintln!("  gen_deploy_json genesis_contracts.json token.json mining_rewards.json");
        std::process::exit(1);
    }

    let output_path = &args[1];
    let inputs = &args[2..];

    let mut deploy_contracts = Vec::new();

    for (i, input_arg) in inputs.iter().enumerate() {
        // Parse "path:deployer_hex" or just "path"
        let (input_path, deployer_hex) = if let Some(colon) = input_arg.find(':') {
            (&input_arg[..colon], &input_arg[colon + 1..])
        } else {
            (input_arg.as_str(), DEFAULT_DEPLOYER)
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
            "[{}] contract={} functions={} whitelist={} deployer={}",
            i,
            input_path,
            defs.len(),
            deploy_contract.function_whitelist.len(),
            deployer_hex
        );
        deploy_contracts.push(deploy_contract);
    }

    // Keep output as valid JSON array while making each contract object occupy
    // exactly one line for easier review and diff.
    let mut output_json = String::from("[\n");
    for (i, deploy_contract) in deploy_contracts.iter().enumerate() {
        output_json.push_str("  ");
        output_json.push_str(&serde_json::to_string(deploy_contract)?);
        if i + 1 != deploy_contracts.len() {
            output_json.push(',');
        }
        output_json.push('\n');
    }
    output_json.push(']');
    fs::write(output_path, &output_json)
        .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", output_path, e))?;

    println!();
    println!("Written {} contract(s) to {}", deploy_contracts.len(), output_path);

    Ok(())
}
