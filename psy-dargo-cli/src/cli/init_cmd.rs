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
