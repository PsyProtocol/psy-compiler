/// Tool to gather generated contract ABI JSON files into a genesis ABI directory.
///
/// Mirrors `gen_deploy_json`: it reads each generated `<name>.abi.json`,
/// copies it into `<output_dir>/<ContractName>.json` (using the contract name
/// embedded inside the ABI), and writes an `abi_list.json` manifest describing
/// the precompiles (contract_id, name, deployer, abi_path, state_tree_height)
/// for use in genesis config.
///
/// Usage:
///   cargo run --release --example gen_deploy_abi_json -- <output_dir> \
///     <input1.abi.json>[:<name>] [<input2.abi.json>[:<name>]] ...
///
/// Each input copies `<ContractName>.json` (read from the ABI's `contract.name`)
/// into <output_dir>. The optional `<name>` is the friendly name recorded in
/// abi_list.json (defaults to the input file stem).
///
/// Example:
///   cargo run --release --example gen_deploy_abi_json -- \
///     ../../psy-genesis/genesis_abi \
///     ../psy-precompiles/token/target/token.abi.json:token \
///     ../psy-precompiles/mining_rewards/target/mining_rewards.abi.json:mining_rewards
use std::{env, fs, path::Path};

// Same default genesis deployer as gen_deploy_json.
const DEFAULT_DEPLOYER: &str = "f83aa03c3e21321421696202b90f4dab0a9f87237c231bbba58b8f93c799126e";

fn get_file_stem(path: &str) -> String {
    Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("contract").to_string()
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: gen_deploy_abi_json <output_dir> <input1.abi.json>[:<name>] [<input2.abi.json>[:<name>]] ...");
        eprintln!();
        eprintln!("Each input copies <ContractName>.json (read from the ABI) into <output_dir>.");
        eprintln!("The optional `<name>` is recorded in abi_list.json (default: input file stem).");
        eprintln!("Default deployer: {}", DEFAULT_DEPLOYER);
        std::process::exit(1);
    }

    let output_dir = &args[1];
    let inputs = &args[2..];

    fs::create_dir_all(output_dir)
        .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", output_dir, e))?;

    let mut precompiles = Vec::new();

    for (i, input_arg) in inputs.iter().enumerate() {
        let (input_path, name) = match input_arg.split_once(':') {
            Some((path, n)) => (path, n.to_string()),
            None => (input_arg.as_str(), get_file_stem(input_arg)),
        };

        // Read the ABI verbatim so its exact byte layout is preserved on copy.
        let raw = fs::read_to_string(input_path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", input_path, e))?;
        let abi: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", input_path, e))?;

        let contract_name = abi["contract"]["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("ABI {} is missing contract.name", input_path))?;
        let state_tree_height = state_tree_height_from_abi(&abi, input_path)?;

        let abi_filename = format!("{}.json", contract_name);
        let abi_path = Path::new(output_dir).join(&abi_filename);
        fs::write(&abi_path, &raw)
            .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", abi_path.display(), e))?;

        println!("[{}] contract={} name={} abi_path={}", i, contract_name, name, abi_filename);

        precompiles.push(ManifestPrecompile {
            contract_id: i as u32,
            name,
            deployer: DEFAULT_DEPLOYER.to_string(),
            abi_path: abi_filename,
            state_tree_height,
        });
    }

    let manifest = build_manifest(&precompiles);
    let manifest_path = Path::new(output_dir).join("abi_list.json");
    fs::write(&manifest_path, &manifest)
        .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", manifest_path.display(), e))?;

    println!();
    println!("Written {} ABI file(s) + abi_list.json to {}", precompiles.len(), output_dir);

    Ok(())
}

fn state_tree_height_from_abi(abi: &serde_json::Value, input_path: &str) -> anyhow::Result<u8> {
    let height = abi["contract"]["state_tree_height"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("ABI {} is missing a valid contract.state_tree_height", input_path))?;
    u8::try_from(height).map_err(|_| anyhow::anyhow!("contract.state_tree_height {} in {} exceeds u8", height, input_path))
}

struct ManifestPrecompile {
    contract_id: u32,
    name: String,
    deployer: String,
    abi_path: String,
    state_tree_height: u8,
}

/// Build abi_list.json by hand so the key order is deterministic regardless of
/// serde_json's `preserve_order` feature.
fn build_manifest(precompiles: &[ManifestPrecompile]) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"users\": [],\n");
    out.push_str("  \"precompiles\": [\n");
    for (i, p) in precompiles.iter().enumerate() {
        let comma = if i + 1 == precompiles.len() { "" } else { "," };
        out.push_str("    {\n");
        out.push_str(&format!("      \"contract_id\": {},\n", p.contract_id));
        out.push_str(&format!("      \"name\": \"{}\",\n", p.name));
        out.push_str(&format!("      \"deployer\": \"{}\",\n", p.deployer));
        out.push_str(&format!("      \"abi_path\": \"{}\",\n", p.abi_path));
        out.push_str(&format!("      \"state_tree_height\": {}\n", p.state_tree_height));
        out.push_str(&format!("    }}{}\n", comma));
    }
    out.push_str("  ],\n");
    out.push_str("  \"contracts\": []\n");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_state_tree_height_from_contract_abi() {
        let abi = serde_json::json!({"contract": {"state_tree_height": 23}});
        assert_eq!(state_tree_height_from_abi(&abi, "mining_rewards.abi.json").unwrap(), 23);
    }

    #[test]
    fn rejects_missing_or_out_of_range_state_tree_height() {
        let missing = serde_json::json!({"contract": {}});
        assert!(state_tree_height_from_abi(&missing, "missing.abi.json").is_err());

        let too_large = serde_json::json!({"contract": {"state_tree_height": 256}});
        assert!(state_tree_height_from_abi(&too_large, "large.abi.json").is_err());
    }
}
