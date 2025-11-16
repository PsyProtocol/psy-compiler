pub mod compile_cmd;
mod complete_cmd;
pub mod doc_cmd;
mod execute_cmd;
mod fmt_cmd;
mod generate_abi_cmd;
mod init_cmd;
mod new_cmd;
mod test_cmd;

use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
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
#[command(name="dargo", author, about, long_about = None)]
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
    let artifact_path = output_dir.join(artifact_name).with_extension("json");
    let bytes = serde_json::to_vec(build_artifact)?;
    write_to_file(&bytes, &artifact_path)?;
    Ok(artifact_path)
}

// Create the parent directory if needed and write the bytes to a file.
pub fn write_to_file(bytes: &[u8], path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}
