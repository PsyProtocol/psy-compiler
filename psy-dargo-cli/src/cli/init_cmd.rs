use std::path::PathBuf;

use clap::Args;
use psy_package::{CrateName, PackageType};

use super::{write_to_file, DargoConfig};
use crate::errors::{CliError, Result};

/// Create a project in the current directory.
#[derive(Debug, Clone, Args)]
pub(crate) struct InitCommand {
    /// Name of the package [default: current directory name]
    #[clap(long)]
    name: Option<CrateName>,

    /// Use a library template
    #[arg(long, conflicts_with = "bin")]
    pub(crate) lib: bool,

    /// Use a binary template [default]
    #[arg(long, conflicts_with = "lib")]
    pub(crate) bin: bool,
}

const BIN_EXAMPLE: &str = include_str!("./template_files/binary.psy");
const LIB_EXAMPLE: &str = include_str!("./template_files/library.psy");

pub(crate) fn run(args: InitCommand, config: DargoConfig) -> Result<()> {
    let package_name = match args.name {
        Some(name) => name,
        None => {
            let name = config.program_dir.file_name().unwrap().to_str().unwrap();
            name.parse().map_err(|_| CliError::InvalidPackageName(name.into()))?
        }
    };

    let package_type = if args.lib { PackageType::Library } else { PackageType::Binary };
    initialize_project(config.program_dir, package_name, package_type);
    Ok(())
}

/// Initializes a new project in `package_dir`.
pub(crate) fn initialize_project(package_dir: PathBuf, package_name: CrateName, package_type: PackageType) {
    let src_dir = package_dir.join("src");

    let toml_contents = format!(
        r#"[package]
name = "{package_name}"
type = "{package_type}"
authors = [""]

[dependencies]"#
    );

    write_to_file(toml_contents.as_bytes(), &package_dir.join("Dargo.toml")).unwrap();
    // This uses the `match` syntax instead of `if` so we get a compile error when
    // we add new package types (which likely need new template files)
    match package_type {
        PackageType::Binary => {
            write_to_file(BIN_EXAMPLE.as_bytes(), &src_dir.join("main.psy")).unwrap();
        }
        PackageType::Library => {
            write_to_file(LIB_EXAMPLE.as_bytes(), &src_dir.join("lib.psy")).unwrap();
        }
    };
    println!("Project successfully created! It is located at {}", package_dir.display());
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn temporary_project_dir(suffix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("psy-dargo-init-test-{}-{suffix}", std::process::id()))
    }

    #[test]
    fn initialize_project_writes_binary_and_library_templates() {
        for (suffix, package_type, entry) in [("bin", PackageType::Binary, "main.psy"), ("lib", PackageType::Library, "lib.psy")] {
            let directory = temporary_project_dir(suffix);
            let _ = fs::remove_dir_all(&directory);
            initialize_project(directory.clone(), "demo".parse().unwrap(), package_type);

            let manifest = fs::read_to_string(directory.join("Dargo.toml")).unwrap();
            assert!(manifest.contains("name = \"demo\""));
            assert!(manifest.contains(&format!("type = \"{package_type}\"")));
            assert!(directory.join("src").join(entry).is_file());
            fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn run_uses_the_explicit_name_and_the_requested_template() {
        let directory = temporary_project_dir("run-explicit");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();

        run(
            InitCommand { name: Some("demo".parse().unwrap()), lib: true, bin: false },
            DargoConfig { program_dir: directory.clone(), target_dir: None },
        )
        .expect("explicit name with the lib flag must initialize a library project");

        let manifest = fs::read_to_string(directory.join("Dargo.toml")).unwrap();
        assert!(manifest.contains("name = \"demo\""));
        assert!(manifest.contains("type = \"lib\""));
        assert!(directory.join("src").join("lib.psy").is_file());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn run_derives_the_package_name_from_a_valid_directory_name() {
        let root = std::env::temp_dir().join(format!("psy-dargo-init-test-{}-run-default", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let directory = root.join("derived_name");
        fs::create_dir_all(&directory).unwrap();

        run(
            InitCommand { name: None, lib: false, bin: true },
            DargoConfig { program_dir: directory.clone(), target_dir: None },
        )
        .expect("a valid directory name must initialize a binary project");

        let manifest = fs::read_to_string(directory.join("Dargo.toml")).unwrap();
        assert!(manifest.contains("name = \"derived_name\""));
        assert!(manifest.contains("type = \"bin\""));
        assert!(directory.join("src").join("main.psy").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_rejects_a_directory_name_that_is_not_a_valid_package_name() {
        // The temporary directory stem contains '-', which CrateName rejects.
        let directory = temporary_project_dir("run-invalid");
        fs::create_dir_all(&directory).unwrap();

        let error = run(
            InitCommand { name: None, lib: false, bin: false },
            DargoConfig { program_dir: directory.clone(), target_dir: None },
        )
        .expect_err("an unparseable directory name must be rejected");
        assert!(matches!(error, CliError::InvalidPackageName(name) if name.contains("run-invalid")));
        fs::remove_dir_all(directory).unwrap();
    }
}
