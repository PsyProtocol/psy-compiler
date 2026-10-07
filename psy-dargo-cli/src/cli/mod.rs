pub mod compile_cmd;
mod complete_cmd;
pub mod doc_cmd;
mod execute_cmd;
mod fmt_cmd;
mod generate_abi_cmd;
mod init_cmd;
mod new_cmd;
mod test_cmd;
pub(crate) mod test_helpers;

use std::{
    collections::{HashSet, VecDeque},
    path::{Component, Path, PathBuf},
};

use clap::{Args, Parser, Subcommand};
use psy_common::Graph;
use psy_package::{
    files::{find_file_manifest_root, get_package_manifest},
    resolve_workspace_from_toml, Dependency, Workspace,
};

use crate::errors::{CliError, Result};

pub(crate) async fn start_cli() -> Result<()> {
    let DargoCli { command, config } = DargoCli::parse();
    match command {
        DargoCommand::New(args) => new_cmd::run(args, config),
        DargoCommand::Init(args) => init_cmd::run(args, config),
        DargoCommand::Compile(args) => with_workspace(args, config, compile_cmd::run),
        DargoCommand::Execute(args) => with_workspace_async(args, config, execute_cmd::run).await,
        DargoCommand::Test(args) => test_cmd::run(args).await,
        DargoCommand::Fmt(args) => fmt_cmd::run(args),
        DargoCommand::Complete(args) => complete_cmd::run(args),
        DargoCommand::GenerateAbi(args) => with_workspace(args, config, generate_abi_cmd::run),
    }?;
    Ok(())
}

#[derive(Parser, Debug)]
#[command(name = "dargo", author, version, about, long_about = None)]
struct DargoCli {
    #[command(subcommand)]
    command: DargoCommand,

    #[clap(flatten)]
    config: DargoConfig,
}

#[non_exhaustive]
#[derive(Subcommand, Clone, Debug)]
enum DargoCommand {
    New(new_cmd::NewCommand),
    Init(init_cmd::InitCommand),
    #[command(alias = "build")]
    Compile(compile_cmd::CompileCommand),
    Execute(execute_cmd::ExecuteCommand),
    Test(test_cmd::TestCommand),
    Fmt(fmt_cmd::FmtCommand),
    Complete(complete_cmd::CompleteCommand),
    #[command(name = "generate-abi")]
    GenerateAbi(generate_abi_cmd::GenerateAbiCommand),
}

#[derive(Args, Clone, Debug)]
pub struct DargoConfig {
    // REMINDER: Also change this flag in the LSP test lens if renamed
    #[arg(long, hide = true, global = true, default_value = "./", value_parser = parse_path)]
    pub program_dir: PathBuf,

    /// Override the default target directory.
    #[arg(long, hide = true, global = true, value_parser = parse_path)]
    pub target_dir: Option<PathBuf>,
}

/// Parses a path and turns it into an absolute one by joining to the current
/// directory.
fn parse_path(path: &str) -> std::result::Result<PathBuf, String> {
    let mut path: PathBuf = path.parse().map_err(|e| format!("failed to parse path: {e}"))?;
    if !path.is_absolute() {
        path = std::env::current_dir().unwrap().join(path);
    }
    Ok(path)
}

pub fn with_workspace<C, R>(cmd: C, config: DargoConfig, run: R) -> Result<()>
where
    R: FnOnce(C, Workspace) -> Result<()>,
{
    // All commands need to run on the workspace level, because that's where the
    // `target` directory is.
    let package_dir = find_file_manifest_root(&config.program_dir)?;
    let toml_path = get_package_manifest(&package_dir)?;
    // Resolve the workspace from the toml file. It will download dependencies as
    // well.
    let mut workspace = resolve_workspace_from_toml(&toml_path)?;
    if let Some(target_dir) = &config.target_dir {
        workspace.target_dir = target_dir.clone();
    }
    run(cmd, workspace)
}

async fn with_workspace_async<C, R, Fut>(cmd: C, config: DargoConfig, run: R) -> Result<()>
where
    R: FnOnce(C, Workspace) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    // All commands need to run on the workspace level, because that's where the
    // `target` directory is.
    let package_dir = find_file_manifest_root(&config.program_dir)?;
    let toml_path = get_package_manifest(&package_dir)?;
    // Resolve the workspace from the toml file. It will download dependencies as
    // well.
    let mut workspace = resolve_workspace_from_toml(&toml_path)?;
    if let Some(target_dir) = &config.target_dir {
        workspace.target_dir = target_dir.clone();
    }
    run(cmd, workspace).await
}

pub fn resolve_crate_path_graph(workspace: &Workspace, entry_path: Option<PathBuf>) -> Graph<PathBuf> {
    let mut package = workspace.package.clone();
    let package_entry_path = match entry_path {
        Some(entry_path) => entry_path,
        None => package.entry_canonical_path(),
    };
    package.entry_path = package_entry_path;
    let mut graph = Graph::new();
    let mut package_stack = VecDeque::new();
    package_stack.push_back(&package);
    while let Some(package) = package_stack.pop_front() {
        let entry_path = package.entry_canonical_path();
        if graph.contains_node(&entry_path) {
            continue;
        }
        graph.add_node(entry_path.clone());
        for dep in package.dependencies.values() {
            match dep {
                Dependency::Remote { package } | Dependency::Local { package } => {
                    let dep_path = package.entry_canonical_path();
                    graph.add_edge(entry_path.clone(), dep_path.clone());
                    package_stack.push_back(package);
                }
            }
        }
    }
    graph
}

pub fn save_build_artifact_to_file<T: ?Sized + serde::Serialize>(build_artifact: &T, artifact_name: &str, output_dir: &Path) -> Result<PathBuf> {
    validate_artifact_name(artifact_name)?;
    let artifact_path = output_dir.join(artifact_name);
    let artifact_path = match artifact_path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => artifact_path,
        _ => artifact_path.with_file_name(format!(
            "{}.json",
            artifact_path
                .file_name()
                .ok_or_else(|| CliError::Generic("artifact name is missing a file name".to_string()))?
                .to_string_lossy()
        )),
    };
    let bytes = serde_json::to_vec(build_artifact)?;
    write_to_file(&bytes, &artifact_path)?;
    Ok(artifact_path)
}

pub fn validate_artifact_name(artifact_name: &str) -> Result<()> {
    let mut components = Path::new(artifact_name).components();
    let is_single_normal_component = matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if artifact_name.is_empty() || artifact_name.contains(['/', '\\']) || !is_single_normal_component {
        return Err(CliError::InvalidArtifactName(artifact_name.to_string()));
    }
    Ok(())
}

// Create the parent directory if needed and write the bytes to a file.
pub fn write_to_file(bytes: &[u8], path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod artifact_name_tests {
    use std::path::Path;

    use super::{save_build_artifact_to_file, validate_artifact_name};
    use crate::errors::CliError;

    #[test]
    fn accepts_single_artifact_file_names() {
        for name in ["contract", "contract.abi", "contract-v1.abi", "合约.abi"] {
            validate_artifact_name(name).unwrap();
        }
    }

    #[test]
    fn rejects_artifact_path_traversal_and_paths() {
        for name in [
            "",
            ".",
            "..",
            "../escaped",
            "nested/escaped",
            "/tmp/escaped",
            r"..\escaped",
            r"C:\escaped",
        ] {
            let error = validate_artifact_name(name).expect_err(name);
            assert!(matches!(error, CliError::InvalidArtifactName(ref invalid) if invalid == name));
        }
    }

    #[test]
    fn artifact_writer_rejects_paths_before_writing() {
        let error = save_build_artifact_to_file(&serde_json::json!({}), "../escaped", Path::new("unused-output-dir"))
            .expect_err("path traversal must be rejected");
        assert!(matches!(error, CliError::InvalidArtifactName(ref invalid) if invalid == "../escaped"));
    }

    #[test]
    fn artifact_writer_adds_json_extension_and_creates_output_directory() {
        let root = std::env::temp_dir().join(format!("psy-dargo-artifact-test-{}", std::process::id()));
        let output_dir = root.join("nested/output");
        let _ = std::fs::remove_dir_all(&root);

        let path = save_build_artifact_to_file(&serde_json::json!({"ok": true}), "artifact", &output_dir).unwrap();
        assert_eq!(path, output_dir.join("artifact.json"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"ok":true}"#);

        let existing = save_build_artifact_to_file(&serde_json::json!({"ok": false}), "existing.json", &output_dir).unwrap();
        assert_eq!(existing, output_dir.join("existing.json"));
        assert_eq!(std::fs::read_to_string(existing).unwrap(), r#"{"ok":false}"#);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod cli_path_tests {
    use std::path::PathBuf;

    use psy_package::{Dependency, Package, Workspace};

    use super::{parse_path, resolve_crate_path_graph};

    #[test]
    fn parse_path_resolves_relative_and_preserves_absolute_paths() {
        let relative = parse_path("nested/package").unwrap();
        assert!(relative.is_absolute());
        assert!(relative.ends_with("nested/package"));

        let absolute = if cfg!(windows) { r"C:\package" } else { "/package" };
        assert_eq!(parse_path(absolute).unwrap(), PathBuf::from(absolute));
    }

    #[test]
    fn crate_path_graph_honors_entry_override_and_deduplicates_dependencies() {
        let shared = Package {
            root_dir: PathBuf::from("shared"),
            entry_path: PathBuf::from("src/lib.psy"),
            ..Package::default()
        };
        let left = Package {
            root_dir: PathBuf::from("left"),
            entry_path: PathBuf::from("src/lib.psy"),
            dependencies: [("shared".parse().unwrap(), Dependency::Local { package: shared.clone() })]
                .into_iter()
                .collect(),
            ..Package::default()
        };
        let root = Package {
            root_dir: PathBuf::from("root"),
            entry_path: PathBuf::from("src/main.psy"),
            dependencies: [("left".parse().unwrap(), Dependency::Local { package: left.clone() })]
                .into_iter()
                .chain([("shared".parse().unwrap(), Dependency::Local { package: shared })])
                .collect(),
            ..Package::default()
        };
        let workspace = Workspace {
            package: root,
            ..Workspace::default()
        };

        let graph = resolve_crate_path_graph(&workspace, Some(PathBuf::from("root/src/alternate.psy")));
        assert_eq!(graph.nodes().len(), 3);
        assert!(graph.contains_node(&PathBuf::from("root/root/src/alternate.psy")));
        assert!(graph.contains_node(&PathBuf::from("left/src/lib.psy")));
        assert!(graph.contains_node(&PathBuf::from("shared/src/lib.psy")));
    }

    #[test]
    fn parse_path_turns_relative_paths_into_absolute_ones() {
        let relative = super::parse_path("some/relative.psy").expect("relative path must parse");
        assert!(relative.is_absolute(), "relative input must become absolute: {relative:?}");
        assert!(relative.ends_with("some/relative.psy"));

        let absolute = super::parse_path("/tmp/already-absolute.psy").expect("absolute path must parse");
        assert_eq!(absolute, std::path::PathBuf::from("/tmp/already-absolute.psy"));
    }

    #[test]
    fn with_workspace_resolves_the_manifest_and_applies_the_target_override() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let dir = std::env::temp_dir().join(format!("psy_dargo_ws_{nanos}"));
        std::fs::create_dir_all(dir.join("src")).expect("create src");
        std::fs::write(dir.join("src").join("app.psy"), "fn main() {}\n").expect("write app.psy");
        std::fs::write(
            dir.join("Dargo.toml"),
            "[package]\nname = \"wsdemo\"\ntype = \"bin\"\nentry = \"src/app.psy\"\nauthors = [\"\"]\n\n[dependencies]\n",
        )
        .expect("write Dargo.toml");

        let custom_target = dir.join("custom-target");
        let config = super::DargoConfig { program_dir: dir.clone(), target_dir: Some(custom_target.clone()) };
        let seen = super::with_workspace((), config, |_cmd, workspace: psy_package::Workspace| {
            assert_eq!(workspace.target_dir, custom_target, "target override must be applied");
            Ok(())
        });
        seen.expect("with_workspace must resolve the temp manifest and run the command");

        // A program dir without a manifest cannot resolve a workspace.
        let empty = std::env::temp_dir().join(format!("psy_dargo_ws_empty_{nanos}"));
        std::fs::create_dir_all(&empty).expect("create empty dir");
        let config = super::DargoConfig { program_dir: empty.clone(), target_dir: None };
        super::with_workspace((), config, |_cmd, _workspace: psy_package::Workspace| Ok(()))
            .expect_err("a manifest-less directory must fail resolution");

        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(empty).ok();
    }

    #[tokio::test]
    async fn with_workspace_async_resolves_the_manifest_and_applies_the_target_override() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let dir = std::env::temp_dir().join(format!("psy_dargo_ws_async_{nanos}"));
        std::fs::create_dir_all(dir.join("src")).expect("create src");
        std::fs::write(dir.join("src").join("app.psy"), "fn main() {}\n").expect("write app.psy");
        std::fs::write(
            dir.join("Dargo.toml"),
            "[package]\nname = \"wsasync\"\ntype = \"bin\"\nentry = \"src/app.psy\"\nauthors = [\"\"]\n\n[dependencies]\n",
        )
        .expect("write Dargo.toml");

        let custom_target = dir.join("custom-target");
        let config = super::DargoConfig { program_dir: dir.clone(), target_dir: Some(custom_target.clone()) };
        super::with_workspace_async((), config, |_cmd, workspace: psy_package::Workspace| async move {
            assert_eq!(workspace.target_dir, custom_target, "target override must be applied");
            Ok(())
        })
        .await
        .expect("with_workspace_async must resolve the temp manifest and run the command");

        std::fs::remove_dir_all(dir).ok();
    }
}
