pub mod errors;
pub mod files;
pub mod fm;
mod git;
pub mod package;
mod semver;
pub mod source;
pub mod workspace;

pub use errors::{ManifestError, SemverError};
pub use files::*;
pub use package::*;
// Individual re-exports for backward compatibility
pub use package::{CrateName, Dependency, Package, PackageType};
pub use source::*;
pub use workspace::Workspace;

pub const FILE_EXTENSION: &str = "psy";

// Re-exports for backward compatibility
pub mod manifest {
    pub use crate::{
        errors::{ManifestError, SemverError},
        resolve_workspace_from_toml, try_clone_std,
    };
}

// Main functionality - internal imports
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{fm::NormalizePath, git::clone_git_repo};

#[derive(Debug, Deserialize, Clone)]
struct PackageConfig {
    package: PackageMetadata,
    #[serde(default)]
    dependencies: BTreeMap<String, DependencyConfig>,
}

const STD_GIT_PATH_HTTPS: &str = "https://github.com/PsyProtocol/psy-v1";
const STD_GIT_PATH_SSH: &str = "git@github.com:PsyProtocol/psy-v1.git";
const TAG_LATEST: &str = "latest";
const STD_FILE: &str = "psy_compiler/psy-std/std.psy";

impl PackageConfig {
    fn resolve_to_package(&self, root_dir: &Path, processed: &mut Vec<PathBuf>) -> Result<crate::package::Package, ManifestError> {
        let name: crate::package::CrateName = if let Some(name) = &self.package.name {
            name.parse().map_err(|_| ManifestError::InvalidPackageName {
                toml: root_dir.join("Dargo.toml"),
                name: name.into(),
            })?
        } else {
            return Err(ManifestError::MissingNameField {
                toml: root_dir.join("Dargo.toml"),
            });
        };

        if std::env::var("DARGO_STD_PATH").is_err() {
            let std_path = resolve_std_path()?;
            unsafe {
                std::env::set_var("DARGO_STD_PATH", std_path);
            }
        }

        let mut dependencies: BTreeMap<crate::package::CrateName, crate::package::Dependency> = BTreeMap::new();
        for (name, dep_config) in self.dependencies.iter() {
            let name = name.parse().map_err(|_| ManifestError::InvalidDependencyName {
                toml: root_dir.join("Dargo.toml"),
                name: name.into(),
            })?;
            let resolved_dep = dep_config.resolve_to_dependency(root_dir, processed)?;
            dependencies.insert(name, resolved_dep);
        }

        let package_type = match self.package.package_type.as_deref() {
            Some("lib") => crate::package::PackageType::Library,
            Some("bin") => crate::package::PackageType::Binary,
            Some(invalid) => {
                return Err(ManifestError::InvalidPackageType(root_dir.join("Dargo.toml"), invalid.to_string()));
            }
            None => {
                return Err(ManifestError::MissingPackageType(root_dir.join("Dargo.toml")));
            }
        };

        let entry_path = if let Some(entry_path) = &self.package.entry {
            let custom_entry_path = root_dir.join(entry_path);
            custom_entry_path
        } else {
            let default_entry_path = match package_type {
                crate::package::PackageType::Library => root_dir.join("src").join("lib").with_extension(FILE_EXTENSION),
                crate::package::PackageType::Binary => root_dir.join("src").join("main").with_extension(FILE_EXTENSION),
            };
            default_entry_path
        };

        if let Some(version) = &self.package.version {
            semver::parse_semver_compatible_version(version).map_err(|err| {
                ManifestError::SemverError(SemverError::CouldNotParsePackageVersion {
                    package_name: name.to_string(),
                    error: err.to_string(),
                })
            })?;
        }

        Ok(crate::package::Package {
            version: self.package.version.clone(),
            root_dir: root_dir.to_path_buf(),
            entry_path,
            package_type,
            name,
            dependencies,
        })
    }
}

/// Try to resolve the std library path from multiple sources:
/// 1. Check DARGO_STD_PATH environment variable
/// 2. Search relative paths from current file location (using file!() macro)
/// 3. Search relative paths from CARGO_MANIFEST_DIR
/// 4. As a last resort, try to clone from git
fn resolve_std_path() -> Result<PathBuf, ManifestError> {
    // 1. Check environment variable first
    if let Ok(std_path) = std::env::var("DARGO_STD_PATH") {
        let path = PathBuf::from(std_path);
        if path.exists() {
            return Ok(path);
        }
    }

    // 2. Try to find std.psy relative to current source file
    let current_file = std::path::Path::new(file!());
    if let Some(package_dir) = current_file.parent().and_then(|p| p.parent()) {
        // From psy-package/src/lib.rs -> psy_compiler/psy-package -> psy_compiler
        let std_path = package_dir.join("psy-std/std.psy");
        if std_path.exists() {
            if let Ok(canonical) = std_path.canonicalize() {
                return Ok(canonical);
            }
        }
    }

    // 3. Try to find std.psy relative to CARGO_MANIFEST_DIR
    if let Ok(cargo_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let candidates = [
            "../psy-std/std.psy",                 // from psy-package -> psy_compiler/psy-std
            "../../psy-std/std.psy",              // fallback
            "../../../psy-std/std.psy",           // fallback
            "../psy_compiler/psy-std/std.psy",    // from workspace root
            "../../psy_compiler/psy-std/std.psy", // from nested dirs
        ];

        for candidate in &candidates {
            let std_path = PathBuf::from(&cargo_dir).join(candidate);
            if std_path.exists() {
                if let Ok(canonical) = std_path.canonicalize() {
                    return Ok(canonical);
                }
            }
        }
    }

    // 4. Last resort: try to clone from git
    eprintln!("[Psy] Could not find psy-std locally, attempting to clone from git...");
    try_clone_std("main")
}

pub fn try_clone_std(tag: &str) -> Result<PathBuf, ManifestError> {
    match clone_git_repo(STD_GIT_PATH_HTTPS, tag) {
        Ok(path) => return Ok(path.join(STD_FILE)),
        Err(e) => eprintln!("[Psy] HTTPS clone failed: {}", e),
    }

    match clone_git_repo(STD_GIT_PATH_SSH, tag) {
        Ok(path) => Ok(path.join(STD_FILE)),
        Err(e) => {
            eprintln!("[Psy] SSH clone failed: {}", e);
            Err(ManifestError::GitError(format!(
                "Both HTTPS and SSH clone failed for psy-std (tag = {})",
                tag
            )))
        }
    }
}

#[derive(Default, Debug, Deserialize, Clone)]
#[allow(dead_code)]
struct PackageMetadata {
    name: Option<String>,
    version: Option<String>,
    #[serde(alias = "type")]
    package_type: Option<String>,
    entry: Option<PathBuf>,
    description: Option<String>,
    authors: Option<Vec<String>>,
    license: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum DependencyConfig {
    Github { git: String, tag: String, directory: Option<String> },
    Path { path: String },
}

impl DependencyConfig {
    fn resolve_to_dependency(&self, pkg_root: &Path, processed: &mut Vec<PathBuf>) -> Result<crate::package::Dependency, ManifestError> {
        let dep = match self {
            Self::Github { git, tag, directory } => {
                let dir_path = clone_git_repo(git, tag).map_err(ManifestError::GitError)?;
                let project_path = if let Some(directory) = directory {
                    let internal_path = dir_path.join(directory).normalize();
                    if !internal_path.starts_with(&dir_path) {
                        return Err(ManifestError::InvalidDirectory {
                            toml: pkg_root.join("Dargo.toml"),
                            directory: directory.into(),
                        });
                    }
                    internal_path
                } else {
                    dir_path
                };
                let toml_path = project_path.join("Dargo.toml");
                let package = resolve_package_from_toml(&toml_path, processed)?;
                crate::package::Dependency::Remote { package }
            }
            Self::Path { path } => {
                let dir_path = pkg_root.join(path);
                let toml_path = dir_path.join("Dargo.toml");
                let package = resolve_package_from_toml(&toml_path, processed)?;
                crate::package::Dependency::Local { package }
            }
        };
        Ok(dep)
    }
}

/// Resolves a Dargo.toml file into a `Workspace` struct.
pub fn resolve_workspace_from_toml(toml_path: &Path) -> Result<crate::workspace::Workspace, ManifestError> {
    let canonical_toml = toml_path
        .canonicalize()
        .map_err(|_| ManifestError::ReadFailed(toml_path.normalize()))?;
    let dargo_toml = read_toml(&canonical_toml)?;
    let mut resolved = vec![canonical_toml];
    let workspace = match dargo_toml.config {
        Config::Package { package_config } => {
            let member = package_config.resolve_to_package(&dargo_toml.root_dir, &mut resolved)?;
            let target_dir = dargo_toml.root_dir.join("target").normalize();
            crate::workspace::Workspace {
                root_dir: dargo_toml.root_dir,
                target_dir,
                package: member,
            }
        }
    };
    Ok(workspace)
}

fn resolve_package_from_toml(toml_path: &Path, processed: &mut Vec<PathBuf>) -> Result<crate::package::Package, ManifestError> {
    let canonical_toml = toml_path
        .canonicalize()
        .map_err(|_| ManifestError::ReadFailed(toml_path.normalize()))?;
    if processed.contains(&canonical_toml) {
        let mut cycle = false;
        let mut message = String::new();
        for toml in processed {
            cycle = cycle || toml == &canonical_toml;
            if cycle {
                message += &format!("{} referencing ", toml.display());
            }
        }
        message += &canonical_toml.display().to_string();
        return Err(ManifestError::CyclicDependency { cycle: message });
    }

    processed.push(canonical_toml.clone());

    let dargo_toml = read_toml(&canonical_toml)?;
    let result = match dargo_toml.config {
        Config::Package { package_config } => package_config.resolve_to_package(&dargo_toml.root_dir, processed),
    };
    let pos = processed.iter().position(|toml| toml == &canonical_toml).expect("added package must be here");
    processed.remove(pos);
    result
}

struct DargoToml {
    root_dir: PathBuf,
    config: Config,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum Config {
    Package {
        #[serde(flatten)]
        package_config: PackageConfig,
    },
}

impl TryFrom<String> for Config {
    type Error = toml::de::Error;

    fn try_from(toml: String) -> Result<Self, Self::Error> {
        toml::from_str(&toml)
    }
}

impl TryFrom<&str> for Config {
    type Error = toml::de::Error;

    fn try_from(toml: &str) -> Result<Self, Self::Error> {
        toml::from_str(toml)
    }
}

fn read_toml(toml_path: &Path) -> Result<DargoToml, ManifestError> {
    let toml_path = toml_path.normalize();
    let toml_as_string = std::fs::read_to_string(&toml_path).map_err(|_| ManifestError::ReadFailed(toml_path.to_path_buf()))?;
    let root_dir = toml_path.parent().ok_or(ManifestError::MissingParent)?;
    let dargo_toml = DargoToml {
        root_dir: root_dir.to_path_buf(),
        config: toml_as_string.try_into()?,
    };
    Ok(dargo_toml)
}

#[cfg(test)]
mod dependency_cycle_tests {
    use super::*;

    fn write_plain_manifest(path: &Path, contents: &str) -> PathBuf {
        std::fs::create_dir_all(path).unwrap();
        let manifest = path.join("Dargo.toml");
        std::fs::write(&manifest, contents).unwrap();
        manifest
    }

    fn write_manifest(path: &Path, name: &str, dependency: &str) {
        let contents = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\ntype = \"lib\"\n\n[dependencies]\ndep = {{ path = \"{dependency}\" }}\n"
        );
        std::fs::write(path.join("Dargo.toml"), contents).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn detects_symlink_self_dependency() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        write_manifest(temp.path(), "self_cycle", "self");
        symlink(temp.path(), temp.path().join("self")).unwrap();

        let error = resolve_workspace_from_toml(&temp.path().join("Dargo.toml")).unwrap_err();
        assert!(matches!(error, ManifestError::CyclicDependency { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn detects_two_package_cycle_through_symlink_alias() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        write_manifest(&a, "a", "../b");
        write_manifest(&b, "b", "../alias-a");
        symlink(&a, temp.path().join("alias-a")).unwrap();

        let error = resolve_workspace_from_toml(&a.join("Dargo.toml")).unwrap_err();
        assert!(matches!(error, ManifestError::CyclicDependency { .. }));
    }

    #[test]
    fn detects_cycle_through_parent_directory_alias() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        write_manifest(&a, "a", "../b");
        write_manifest(&b, "b", "../a/../a");

        let error = resolve_workspace_from_toml(&a.join("Dargo.toml")).unwrap_err();
        assert!(matches!(error, ManifestError::CyclicDependency { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn accepts_non_cyclic_dependency_reached_through_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let dependency = temp.path().join("dependency");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&dependency).unwrap();
        write_manifest(&root, "root", "../dependency-link");
        std::fs::write(
            dependency.join("Dargo.toml"),
            "[package]\nname = \"dependency\"\nversion = \"0.1.0\"\ntype = \"lib\"\n",
        )
        .unwrap();
        symlink(&dependency, temp.path().join("dependency-link")).unwrap();

        resolve_workspace_from_toml(&root.join("Dargo.toml")).unwrap();
    }

    #[test]
    fn config_parses_owned_and_borrowed_toml() {
        let text = "[package]\nname = \"demo\"\ntype = \"bin\"";
        assert!(Config::try_from(text).is_ok());
        assert!(Config::try_from(text.to_string()).is_ok());
        assert!(Config::try_from("not = [valid").is_err());
    }

    #[test]
    fn std_path_falls_back_to_workspace_checkout_when_env_missing_or_stale() {
        let saved = std::env::var("DARGO_STD_PATH").ok();

        // A stale env value must not shadow the workspace checkout.
        unsafe { std::env::set_var("DARGO_STD_PATH", "/definitely/not/a/real/std/path") };
        let stale = resolve_std_path().expect("fallback lookup must succeed past a stale env value");
        assert!(stale.is_file());

        // Without the env var, the CARGO_MANIFEST_DIR candidates resolve.
        unsafe { std::env::remove_var("DARGO_STD_PATH") };
        let resolved = resolve_std_path().expect("workspace checkout contains psy-std");
        assert!(resolved.is_file());

        // Manifest resolution seeds the env var from the same fallback.
        let temp = tempfile::tempdir().unwrap();
        let manifest = write_plain_manifest(temp.path(), "[package]\nname = \"root\"\ntype = \"lib\"\n");
        resolve_workspace_from_toml(&manifest).expect("manifest resolves without the env var");

        match saved {
            Some(value) => unsafe { std::env::set_var("DARGO_STD_PATH", value) },
            None => unsafe { std::env::remove_var("DARGO_STD_PATH") },
        }
    }

    #[test]
    fn standard_library_path_resolves_to_an_existing_file() {
        let path = resolve_std_path().expect("workspace checkout contains psy-std");
        assert!(path.is_file());
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("std.psy"));
    }

    #[test]
    fn workspace_resolves_defaults_custom_entries_and_local_dependencies() {
        let temp = tempfile::tempdir().unwrap();
        let dependency = temp.path().join("dependency");
        let dependency_manifest = write_plain_manifest(
            &dependency,
            "[package]\nname = \"dependency\"\nversion = \"1.2.3-beta+build\"\ntype = \"lib\"\nentry = \"custom.psy\"",
        );
        let root = temp.path().join("root");
        let root_manifest = write_plain_manifest(
            &root,
            "[package]\nname = \"root\"\ntype = \"bin\"\n[dependencies]\ndependency = { path = \"../dependency\" }",
        );

        let workspace = resolve_workspace_from_toml(&root_manifest).unwrap();
        assert_eq!(workspace.root_dir, root.canonicalize().unwrap());
        assert_eq!(workspace.target_dir, workspace.root_dir.join("target"));
        assert_eq!(workspace.package.entry_path, workspace.root_dir.join("src/main.psy"));
        let dependency = workspace
            .package
            .dependencies
            .get(&"dependency".parse().unwrap())
            .unwrap()
            .package();
        assert_eq!(
            dependency.entry_path,
            dependency_manifest.parent().unwrap().canonicalize().unwrap().join("custom.psy")
        );
        assert_eq!(dependency.version.as_deref(), Some("1.2.3-beta+build"));
    }

    #[test]
    fn workspace_reports_missing_and_invalid_package_fields() {
        let cases = [
            (
                "[package]\ntype = \"lib\"",
                "missing-name",
            ),
            (
                "[package]\nname = \"bad-name\"\ntype = \"lib\"",
                "invalid-name",
            ),
            (
                "[package]\nname = \"valid\"",
                "missing-type",
            ),
            (
                "[package]\nname = \"valid\"\ntype = \"plugin\"",
                "invalid-type",
            ),
            (
                "[package]\nname = \"valid\"\ntype = \"lib\"\nversion = \"broken\"",
                "invalid-version",
            ),
        ];

        for (contents, expected) in cases {
            let temp = tempfile::tempdir().unwrap();
            let manifest = write_plain_manifest(temp.path(), contents);
            let error = resolve_workspace_from_toml(&manifest).unwrap_err();
            match expected {
                "missing-name" => assert!(matches!(error, ManifestError::MissingNameField { .. })),
                "invalid-name" => assert!(matches!(error, ManifestError::InvalidPackageName { .. })),
                "missing-type" => assert!(matches!(error, ManifestError::MissingPackageType(_))),
                "invalid-type" => assert!(matches!(error, ManifestError::InvalidPackageType(_, ref ty) if ty == "plugin")),
                "invalid-version" => assert!(matches!(error, ManifestError::SemverError(_))),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn workspace_reports_manifest_io_parse_and_dependency_name_errors() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing.toml");
        assert!(matches!(
            resolve_workspace_from_toml(&missing).unwrap_err(),
            ManifestError::ReadFailed(path) if path == missing
        ));

        let malformed = write_plain_manifest(temp.path(), "[package");
        assert!(matches!(
            resolve_workspace_from_toml(&malformed).unwrap_err(),
            ManifestError::MalformedFile(_)
        ));

        let invalid_dependency = write_plain_manifest(
            temp.path(),
            "[package]\nname = \"root\"\ntype = \"lib\"\n[dependencies]\n\"bad-name\" = { path = \"dep\" }",
        );
        assert!(matches!(
            resolve_workspace_from_toml(&invalid_dependency).unwrap_err(),
            ManifestError::InvalidDependencyName { .. }
        ));

        let missing_dependency = write_plain_manifest(
            temp.path(),
            "[package]\nname = \"root\"\ntype = \"lib\"\n[dependencies]\ndep = { path = \"missing\" }",
        );
        assert!(matches!(
            resolve_workspace_from_toml(&missing_dependency).unwrap_err(),
            ManifestError::ReadFailed(path) if path.ends_with("missing/Dargo.toml")
        ));
    }
}

#[cfg(test)]
mod read_toml_tests {
    use std::path::Path;

    use super::{read_toml, ManifestError};

    #[test]
    fn read_toml_reports_missing_files() {
        let missing = Path::new("/nonexistent-psy-package-probe/Dargo.toml");
        let err = match read_toml(missing) {
            Ok(_) => panic!("a missing manifest must fail to load"),
            Err(err) => err,
        };
        assert!(matches!(err, ManifestError::ReadFailed(_)), "unexpected error: {err:?}");
    }
}
