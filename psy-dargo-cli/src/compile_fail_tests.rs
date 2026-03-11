//! Compile-fail test harness: each `.psy` file under `tests/compile-fail/` must
//! fail to compile. This ensures the compiler reports errors instead of
//! panicking or incorrectly accepting invalid code.

use std::path::PathBuf;

use psy_package::{find_file_manifest_root, get_package_manifest, resolve_workspace_from_toml, Workspace};

use crate::cli::compile_cmd::{compile_workspace_full, CompileOptions};

fn tests_compile_fail_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .join("..")
        .join("tests")
        .join("compile-fail")
        .canonicalize()
        .expect("tests/compile-fail directory must exist")
}

fn workspace_for_tests() -> Workspace {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tests_dir = manifest_dir.join("..").join("tests").canonicalize().expect("tests dir");
    let package_root = find_file_manifest_root(&tests_dir).expect("tests has Dargo.toml");
    let toml_path = get_package_manifest(&package_root).expect("Dargo.toml");
    resolve_workspace_from_toml(&toml_path).expect("resolve workspace")
}

/// Ensure DARGO_STD_PATH is set so parser can find std when running under `cargo test`.
fn ensure_std_path() {
    if std::env::var("DARGO_STD_PATH").is_ok() {
        return;
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let std_psy = manifest_dir.join("..").join("psy-std").join("std.psy");
    if std_psy.exists() {
        std::env::set_var("DARGO_STD_PATH", std_psy.canonicalize().unwrap());
    }
}

#[test]
fn compile_fail_each_file_fails() {
    ensure_std_path();
    let workspace = workspace_for_tests();
    let compile_fail_dir = tests_compile_fail_dir();

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&compile_fail_dir)
        .expect("read compile-fail dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "psy"))
        .map(|e| e.path())
        .collect();
    entries.sort();

    assert!(
        !entries.is_empty(),
        "no .psy files in tests/compile-fail/ - add at least one compile-fail case"
    );

    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy();
        let entry_path = path
            .strip_prefix(workspace.package.root_dir.as_path())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| PathBuf::from(path.file_name().unwrap()));

        let opts = CompileOptions {
            contract_name: None,
            method_names: vec!["main".to_string()],
            entry_path: Some(entry_path),
            debug: false,
        };

        let result = compile_workspace_full(&workspace, &opts);
        assert!(
            result.is_err(),
            "compile-fail case `{}` should have failed to compile but succeeded. Error cases must produce a compiler error.",
            name
        );
    }
}
