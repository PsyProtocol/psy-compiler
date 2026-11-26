use std::{fs, process::Command};

#[test]
fn storage_map_arity_is_a_compiler_diagnostic() {
    let file = std::env::temp_dir().join(format!("storage_map_arity_{}.psy", std::process::id()));
    fs::write(&file, "#[derive(Storage)]\nstruct Bad {\n    x: Map<Felt>,\n}\nfn main() {}").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dargo")).args(["test", "--file"]).arg(&file).output().unwrap();
    fs::remove_file(&file).unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("3 generic parameters for Map, but found 1"), "{stderr}");
    assert!(stderr.contains(file.file_name().unwrap().to_str().unwrap()), "{stderr}");
}
