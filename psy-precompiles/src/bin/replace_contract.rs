/// Replace contract[3] (withdrawal_tree) in genesis_contracts.json
/// Uses streaming serde_json and zstd level 1 for speed.
use std::{env, fs, io::Write};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: replace_contract <genesis_contracts.json> <new_wt_deploy.json> <output.json>");
        std::process::exit(1);
    }

    eprintln!("Decompressing genesis_contracts.json...");
    let compressed = fs::read(&args[1])?;
    let decompressed = zstd::stream::decode_all(&compressed[..])?;
    eprintln!("Decompressed: {} MiB", decompressed.len() / 1024 / 1024);

    eprintln!("Parsing JSON...");
    let mut contracts: serde_json::Value = serde_json::from_slice(&decompressed)?;
    let arr = contracts.as_array_mut().unwrap();
    eprintln!("Found {} contracts", arr.len());

    eprintln!("Decompressing new withdrawal tree...");
    let new_compressed = fs::read(&args[2])?;
    let new_decompressed = zstd::stream::decode_all(&new_compressed[..])?;
    let new_value: serde_json::Value = serde_json::from_slice(&new_decompressed)?;
    let new_wt = &new_value[0];

    let old_name = arr[3]["name"].as_str().unwrap_or("?").to_string();
    let new_name = new_wt["name"].as_str().unwrap_or("?").to_string();
    eprintln!("Replacing [3]: '{}' -> '{}'", old_name, new_name);
    eprintln!("code_root: {} -> {}",
        arr[3]["code_root"].as_str().unwrap_or("?"),
        new_wt["code_root"].as_str().unwrap_or("?")
    );

    arr[3] = new_wt.clone();

    eprintln!("Serializing and compressing (zstd level 1)...");
    let f = fs::File::create(&args[3])?;
    let mut encoder = zstd::stream::write::Encoder::new(f, 1)?; // level 1 = fast
    // Use serde_json::to_writer with compact output
    serde_json::to_writer(&mut encoder, &contracts)?;
    encoder.finish()?;

    let out_size = fs::metadata(&args[3])?.len();
    eprintln!("Done! Output: {} ({} MiB compressed)", args[3], out_size / 1024 / 1024);
    Ok(())
}
