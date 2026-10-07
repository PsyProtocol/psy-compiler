/// Tool to convert one or more compiled contract JSON artifacts into
/// genesis_contracts.json (Vec<PQBCDeployContract>) for use in genesis config.
///
/// Usage:
///   cargo run --release --example gen_deploy_json -- \
///     --deployer-user-id <ASCII-decimal-u64> <output.json> \
///     <input1.json>[:<name>] [<input2.json>[:<name>]] ...
///
/// The deployer user id applies to every contract. No default or per-artifact
/// deployer is accepted. Set GEN_DEPLOY_JSON_COMPACT=1 to write compact JSON.
use std::{env, fs, path::Path, str::FromStr};

use psy_data::config::store_config::{C, D};
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_vm::dpn::vm::def::DPNFunctionCircuitDefinition;
use serde::Deserialize;

const DEFAULT_STATE_TREE_HEIGHT: u8 = 32;

#[derive(Deserialize)]
struct CompilationArtifact {
    state_tree_height: u16,
    circuit_definitions: Vec<DPNFunctionCircuitDefinition>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CompilationArtifactInput {
    Current(CompilationArtifact),
    Legacy(Vec<DPNFunctionCircuitDefinition>),
}

fn get_file_stem(path: &str) -> String {
    Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("contract").to_string()
}

fn main() -> anyhow::Result<()> {
    let compact = env_flag_enabled("GEN_DEPLOY_JSON_COMPACT");
    let args: Vec<String> = env::args().collect();
    let (deployer_user_id, output_path, inputs) = parse_generation_args(&args)?;

    let mut contract_objects = Vec::new();

    for (i, input_arg) in inputs.iter().enumerate() {
        let (input_path, name) = match input_arg.split_once(':') {
            Some((path, name)) if !name.is_empty() && !name.contains(':') => (path, name.to_string()),
            None => (input_arg.as_str(), get_file_stem(input_arg)),
            Some(_) => anyhow::bail!("Invalid input format: {}. Use path[:name]", input_arg),
        };

        let input_json = fs::read_to_string(input_path).map_err(|e| anyhow::anyhow!("Failed to read {}: {}", input_path, e))?;
        let artifact: CompilationArtifactInput =
            serde_json::from_str(&input_json).map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", input_path, e))?;
        let (state_tree_height, defs) = match artifact {
            CompilationArtifactInput::Current(artifact) => (
                u8::try_from(artifact.state_tree_height)
                    .map_err(|_| anyhow::anyhow!("state_tree_height {} in {} exceeds u8", artifact.state_tree_height, input_path))?,
                artifact.circuit_definitions,
            ),
            CompilationArtifactInput::Legacy(defs) => (DEFAULT_STATE_TREE_HEIGHT, defs),
        };

        let (_circuits, deploy_contract) =
            gen_contract_deploy_and_circuits_for_functions::<C, D>(deployer_user_id, state_tree_height, &defs)?;

        println!(
            "[{}] contract={} name={} functions={} state_tree_height={} whitelist={} deployer_user_id={}",
            i,
            input_path,
            name,
            defs.len(),
            state_tree_height,
            deploy_contract.function_whitelist.len(),
            deployer_user_id
        );

        let mut contract_json: serde_json::Value = serde_json::to_value(&deploy_contract)?;
        contract_json["name"] = serde_json::json!(name);

        contract_objects.push(contract_json);
    }

    let output_json = if compact {
        let mut output_json = String::from("[\n");
        for (i, deploy_contract) in contract_objects.iter().enumerate() {
            output_json.push_str("  ");
            output_json.push_str(&serde_json::to_string(deploy_contract)?);
            if i + 1 != contract_objects.len() {
                output_json.push(',');
            }
            output_json.push('\n');
        }
        output_json.push(']');
        output_json
    } else {
        serde_json::to_string_pretty(&contract_objects)?
    };
    let compressed = zstd::encode_all(output_json.as_bytes(), 3)
        .map_err(|e| anyhow::anyhow!("Failed to zstd-compress output: {}", e))?;
    fs::write(output_path, &compressed).map_err(|e| anyhow::anyhow!("Failed to write {}: {}", output_path, e))?;

    println!();
    println!("Written {} contract(s) to {}", contract_objects.len(), output_path);

    Ok(())
}

fn parse_generation_args(args: &[String]) -> anyhow::Result<(u64, &str, &[String])> {
    if args.len() >= 5 && args[1] == "--deployer-user-id" {
        let deployer_user_id = canonical_decimal_u64(&args[2])?;
        return Ok((deployer_user_id, &args[3], &args[4..]));
    }
    anyhow::bail!(
        "Usage: gen_deploy_json --deployer-user-id <ASCII-decimal-u64> <output.json> <input1.json>[:<name>] [<input2.json>[:<name>]] ..."
    )
}

fn canonical_decimal_u64(value: &str) -> anyhow::Result<u64> {
    let canonical = value == "0" || (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) && !value.starts_with('0'));
    if !canonical {
        anyhow::bail!("deployer user id must be one canonical ASCII decimal u64");
    }
    u64::from_str(value).map_err(|_| anyhow::anyhow!("deployer user id exceeds u64"))
}

fn env_flag_enabled(name: &str) -> bool {
    env::var(name)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"))
        .unwrap_or(false)
}
