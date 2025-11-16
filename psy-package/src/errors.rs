use std::path::PathBuf;

use thiserror::Error;

use crate::package::CrateName;

/// Errors covering situations where a package is either missing, malformed or
/// does not pass semver validation checks.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// Package doesn't have a manifest file
    #[error("cannot find a Dargo.toml for {0}")]
    MissingFile(PathBuf),

    #[error("Cannot read file {0} - does it exist?")]
    ReadFailed(PathBuf),

    #[error("Dargo.toml is missing a parent directory")]
    MissingParent,

    #[error("Missing `type` field in {0}")]
    MissingPackageType(PathBuf),

    #[error("Cannot use `{1}` for `type` field in {0}")]
    InvalidPackageType(PathBuf, String),

    /// Package manifest is unreadable.
    #[error("Dargo.toml is badly formed, could not parse.\n\n {0}")]
    MalformedFile(#[from] toml::de::Error),

    #[error("{} found in {toml}", if name.is_empty() { "Empty package name".into() } else { format!("Invalid package name `{name}`") })]
    InvalidPackageName { toml: PathBuf, name: String },

    #[error("{} found in {toml}", if name.is_empty() { "Empty dependency name".into() } else { format!("Invalid dependency name `{name}`") })]
    InvalidDependencyName { toml: PathBuf, name: String },

    #[error("Invalid directory path {directory} in {toml}: It must point to a subdirectory")]
    InvalidDirectory { toml: PathBuf, directory: PathBuf },

    /// Encountered error while downloading git repository.
    #[error("{0}")]
    GitError(String),

    #[error("Package `{0}` has type `bin` but you cannot depend on binary packages")]
    BinaryDependency(CrateName),

    #[error("Missing `name` field in {toml}")]
    MissingNameField { toml: PathBuf },

    #[error("No common ancestor between {root} and {current}")]
    NoCommonAncestor { root: PathBuf, current: PathBuf },

    #[error(transparent)]
    SemverError(SemverError),

    #[error("Cyclic package dependency found when processing {cycle}")]
    CyclicDependency { cycle: String },

    #[error("Failed to parse expression width with the following error: {0}")]
    ParseExpressionWidth(String),
}

#[allow(clippy::enum_variant_names)]
#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum SemverError {
    #[error("Invalid value for `compiler_version` in package {package_name}. Requirements may only refer to full releases")]
    InvalidCompilerVersionRequirement {
        package_name: CrateName,
        required_compiler_version: String,
    },
    #[error(
        "Incompatible compiler version in package {package_name}. Required compiler version is {required_compiler_version} but the compiler version is {compiler_version_found}.\n Update the compiler_version field in Dargo.toml to >={required_compiler_version} or compile this project with version {required_compiler_version}"
    )]
    IncompatibleVersion {
        package_name: CrateName,
        required_compiler_version: String,
        compiler_version_found: String,
    },
    #[error("Could not parse the required compiler version for package {package_name} in Dargo.toml. Error: {error}")]
    CouldNotParseRequiredVersion { package_name: String, error: String },
    #[error("Could not parse the package version for package {package_name} in Dargo.toml. Error: {error}")]
    CouldNotParsePackageVersion { package_name: String, error: String },
}
