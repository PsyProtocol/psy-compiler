use std::path::PathBuf;

use clap::Args;
use psy_package::{CrateName, PackageType};

use crate::{
    cli::{init_cmd::initialize_project, DargoConfig},
    errors::{CliError, Result},
};

#[allow(rustdoc::broken_intra_doc_links)]
/// Create a project in a new directory.
#[derive(Debug, Clone, Args)]
pub(crate) struct NewCommand {
    /// The path to save the new project
    path: PathBuf,

    /// Name of the package [default: package directory name]
    #[clap(long)]
    name: Option<CrateName>,

    /// Use a library template
    #[arg(long, conflicts_with = "bin", conflicts_with = "contract")]
    pub(crate) lib: bool,

    /// Use a binary template [default]
    #[arg(long, conflicts_with = "lib", conflicts_with = "contract")]
    pub(crate) bin: bool,

    /// Use a contract template
    #[arg(long, conflicts_with = "lib", conflicts_with = "bin")]
    pub(crate) contract: bool,
}

pub(crate) fn run(args: NewCommand, config: DargoConfig) -> Result<()> {
    let package_dir = config.program_dir.join(&args.path);

    if package_dir.exists() {
        return Err(CliError::DestinationAlreadyExists(package_dir));
    }

    let package_name = match args.name {
        Some(name) => name,
        None => {
            let name = args.path.file_name().unwrap().to_str().unwrap();
            name.parse().map_err(|_| CliError::InvalidPackageName(name.into()))?
        }
    };
    let package_type = if args.lib { PackageType::Library } else { PackageType::Binary };
    initialize_project(package_dir, package_name, package_type);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn new_rejects_an_existing_destination_before_name_resolution() {
        let root = std::env::temp_dir().join(format!("psy-dargo-new-test-{}", std::process::id()));
        let destination = root.join("existing");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&destination).unwrap();

        let error = run(
            NewCommand {
                path: PathBuf::from("existing"),
                name: Some("demo".parse().unwrap()),
                lib: false,
                bin: false,
                contract: false,
            },
            DargoConfig {
                program_dir: root.clone(),
                target_dir: None,
            },
        )
        .expect_err("existing destination must be rejected");
        assert!(matches!(error, CliError::DestinationAlreadyExists(path) if path == destination));
        fs::remove_dir_all(root).unwrap();
    }

    fn config_under(root: &std::path::Path) -> DargoConfig {
        DargoConfig { program_dir: root.to_path_buf(), target_dir: None }
    }

    #[test]
    fn new_creates_a_library_project_with_an_explicit_name() {
        let root = std::env::temp_dir().join(format!("psy-dargo-new-test-lib-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        run(
            NewCommand {
                path: PathBuf::from("fresh_lib"),
                name: Some("demo".parse().unwrap()),
                lib: true,
                bin: false,
                contract: false,
            },
            config_under(&root),
        )
        .expect("a fresh destination must be initialized");

        let project = root.join("fresh_lib");
        let manifest = fs::read_to_string(project.join("Dargo.toml")).unwrap();
        assert!(manifest.contains("name = \"demo\""));
        assert!(manifest.contains("type = \"lib\""));
        assert!(project.join("src").join("lib.psy").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_derives_the_package_name_from_the_path() {
        let root = std::env::temp_dir().join(format!("psy-dargo-new-test-derive-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        run(
            NewCommand { path: PathBuf::from("fresh"), name: None, lib: false, bin: false, contract: true },
            config_under(&root),
        )
        .expect("a fresh destination must be initialized");

        let project = root.join("fresh");
        let manifest = fs::read_to_string(project.join("Dargo.toml")).unwrap();
        assert!(manifest.contains("name = \"fresh\""));
        assert!(project.join("src").join("main.psy").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_rejects_a_path_that_is_not_a_valid_package_name() {
        let root = std::env::temp_dir().join(format!("psy-dargo-new-test-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let error = run(
            NewCommand { path: PathBuf::from("0fresh"), name: None, lib: false, bin: false, contract: false },
            config_under(&root),
        )
        .expect_err("a path starting with a digit is not a valid package name");
        assert!(matches!(error, CliError::InvalidPackageName(name) if name == "0fresh"));
        fs::remove_dir_all(root).unwrap();
    }
}
