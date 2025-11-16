use std::path::PathBuf;

use psy_package::ManifestError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, CliError>;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Generic(String),

    /// Error from Manifest
    #[error(transparent)]
    ManifestError(#[from] ManifestError),

    /// Wrapper error for any other error type
    #[error(transparent)]
    AnyhowError(#[from] anyhow::Error),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error("Failed to deserialize artifact from JSON")]
    DeserializationError(#[from] serde_json::Error),

    #[error("Invalid package name {0}. Did you mean to use `--name`?")]
    InvalidPackageName(String),

    #[error("Error: destination {} already exists", .0.display())]
    DestinationAlreadyExists(PathBuf),

    #[error("Cannot find file {entry} which was specified as the `entry` field in {toml}")]
    MissingEntryFile { toml: PathBuf, entry: PathBuf },

    #[error("Semantic error: {0}")]
    SemanticError(#[from] psy_sema::Error),

    #[error("Common error: {0}")]
    CommonError(#[from] psy_common::Error),
}
