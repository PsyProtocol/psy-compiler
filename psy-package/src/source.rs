use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
};

use serde::Deserialize;

use crate::errors::ManifestError;
use crate::{CrateName, PackageType};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackageId {
    Real(PathBuf),
    Virtual(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelativeFilePath(String);

impl RelativeFilePath {
    pub fn new(path: impl Into<String>) -> Self {
        Self(normalize_relative_path(path.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for RelativeFilePath {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for RelativeFilePath {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

pub type SourceText = Arc<str>;

#[derive(Clone, Debug, Default)]
pub struct PackageSources {
    pub manifest: SourceText,
    pub files: BTreeMap<RelativeFilePath, SourceText>,
}

#[derive(Clone, Debug)]
pub struct ResolvedSourcePackage {
    pub package_id: PackageId,
    pub name: CrateName,
    pub package_type: PackageType,
    pub entry_relative_path: RelativeFilePath,
    pub entry_file_id: FileId,
    pub dependency_packages: BTreeMap<CrateName, PackageId>,
    pub files: BTreeMap<RelativeFilePath, FileId>,
}

#[derive(Clone, Debug)]
pub struct ResolvedSourceWorkspace {
    pub root_package: PackageId,
    pub packages: BTreeMap<PackageId, ResolvedSourcePackage>,
    pub source_map: SourceMap,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VfsPath {
    Real(PathBuf),
    Virtual(String),
}

impl VfsPath {
    pub fn virtual_path(path: impl Into<String>) -> Self {
        Self::Virtual(normalize_virtual_path(path.into()))
    }

    pub fn parent(&self) -> Option<Self> {
        match self {
            Self::Real(path) => path.parent().map(|p| Self::Real(p.to_path_buf())),
            Self::Virtual(path) => parent_virtual_path(path).map(|p| Self::Virtual(p.to_string())),
        }
    }

    pub fn join(&self, relative: &str) -> Option<Self> {
        match self {
            Self::Real(path) => Some(Self::Real(path.join(relative))),
            Self::Virtual(path) => {
                let base = parent_virtual_path(path)?;
                Some(Self::Virtual(join_virtual_path(base, relative)))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

pub struct AnchoredPath<'a> {
    pub anchor: FileId,
    pub relative: &'a str,
}

#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    path_to_id: BTreeMap<VfsPath, FileId>,
    paths: Vec<VfsPath>,
    texts: Vec<SourceText>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: VfsPath, text: SourceText) -> FileId {
        if let Some(id) = self.path_to_id.get(&path).copied() {
            self.texts[id.0 as usize] = text;
            return id;
        }

        let id = FileId(self.paths.len() as u32);
        self.path_to_id.insert(path.clone(), id);
        self.paths.push(path);
        self.texts.push(text);
        id
    }

    pub fn file_id(&self, path: &VfsPath) -> Option<FileId> {
        self.path_to_id.get(path).copied()
    }

    pub fn path(&self, file_id: FileId) -> Option<&VfsPath> {
        self.paths.get(file_id.0 as usize)
    }

    pub fn text(&self, file_id: FileId) -> Option<&str> {
        self.texts.get(file_id.0 as usize).map(AsRef::as_ref)
    }

    pub fn resolve_path(&self, anchored: AnchoredPath<'_>) -> Option<FileId> {
        let anchor_path = self.path(anchored.anchor)?;
        let candidate_path = anchor_path.join(anchored.relative)?;
        self.file_id(&candidate_path)
    }

    pub fn snapshot(&self) -> Vec<(FileId, VfsPath, SourceText)> {
        self.paths
            .iter()
            .cloned()
            .zip(self.texts.iter().cloned())
            .enumerate()
            .map(|(idx, (path, text))| (FileId(idx as u32), path, text))
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct SourcePackageMetadata {
    name: String,
    #[serde(alias = "type")]
    package_type: String,
    entry: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SourceManifest {
    package: SourcePackageMetadata,
    #[serde(default)]
    dependencies: BTreeMap<String, toml::Value>,
}

pub trait PackageResolver {
    fn package_sources(&self, package: &PackageId) -> Result<PackageSources, ManifestError>;
    fn resolve_dependency(&self, from: &PackageId, dependency_name: &str) -> Result<PackageId, ManifestError>;
}

#[derive(Default)]
pub struct MemoryResolver {
    pub packages: BTreeMap<PackageId, PackageSources>,
    pub dependencies: BTreeMap<(PackageId, String), PackageId>,
}

impl MemoryResolver {
    pub fn insert_package(&mut self, package_id: PackageId, sources: PackageSources) {
        self.packages.insert(package_id, sources);
    }

    pub fn insert_dependency(&mut self, from: PackageId, dependency_name: impl Into<String>, to: PackageId) {
        self.dependencies.insert((from, dependency_name.into()), to);
    }
}

impl PackageResolver for MemoryResolver {
    fn package_sources(&self, package: &PackageId) -> Result<PackageSources, ManifestError> {
        self.packages.get(package).cloned().ok_or_else(|| {
            let synthetic = match package {
                PackageId::Real(path) => path.join("Dargo.toml"),
                PackageId::Virtual(id) => PathBuf::from(format!("virtual://{id}/Dargo.toml")),
            };
            ManifestError::ReadFailed(synthetic)
        })
    }

    fn resolve_dependency(&self, from: &PackageId, dependency_name: &str) -> Result<PackageId, ManifestError> {
        self.dependencies
            .get(&(from.clone(), dependency_name.to_string()))
            .cloned()
            .ok_or_else(|| ManifestError::ReadFailed(PathBuf::from(format!("virtual://{dependency_name}/Dargo.toml"))))
    }
}

pub fn resolve_source_workspace(root_package: PackageId, resolver: &impl PackageResolver) -> Result<ResolvedSourceWorkspace, ManifestError> {
    let mut state = ResolveState {
        resolver,
        source_map: SourceMap::new(),
        packages: BTreeMap::new(),
        stack: Vec::new(),
    };
    state.resolve_package(root_package.clone())?;
    Ok(ResolvedSourceWorkspace {
        root_package,
        packages: state.packages,
        source_map: state.source_map,
    })
}

pub fn package_file_to_vfs_path(package_id: &PackageId, relative: &RelativeFilePath) -> VfsPath {
    match package_id {
        PackageId::Real(root) => VfsPath::Real(root.join(relative.as_str())),
        PackageId::Virtual(id) => VfsPath::virtual_path(format!("/workspace/{id}/{}", relative.as_str())),
    }
}

fn normalize_relative_path(path: String) -> String {
    let path = path.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for component in path.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            let _ = parts.pop();
            continue;
        }
        parts.push(component);
    }
    parts.join("/")
}

fn normalize_virtual_path(path: String) -> String {
    let mut output = String::from("/");
    let normalized = normalize_relative_path(path);
    if !normalized.is_empty() {
        if normalized.starts_with('/') {
            output.push_str(normalized.trim_start_matches('/'));
        } else {
            output.push_str(&normalized);
        }
    }
    if output.len() > 1 && output.ends_with('/') {
        output.pop();
    }
    output
}

fn parent_virtual_path(path: &str) -> Option<&str> {
    if path == "/" {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    if let Some(idx) = trimmed.rfind('/') {
        if idx == 0 {
            Some("/")
        } else {
            Some(&trimmed[..idx])
        }
    } else {
        None
    }
}

fn join_virtual_path(base_dir: &str, relative: &str) -> String {
    let mut base = String::from(base_dir);
    if !base.ends_with('/') {
        base.push('/');
    }
    base.push_str(relative);
    normalize_virtual_path(base)
}

struct ResolveState<'a, R: PackageResolver> {
    resolver: &'a R,
    source_map: SourceMap,
    packages: BTreeMap<PackageId, ResolvedSourcePackage>,
    stack: Vec<PackageId>,
}

impl<'a, R: PackageResolver> ResolveState<'a, R> {
    fn resolve_package(&mut self, package_id: PackageId) -> Result<(), ManifestError> {
        if self.packages.contains_key(&package_id) {
            return Ok(());
        }
        if self.stack.contains(&package_id) {
            let cycle = self
                .stack
                .iter()
                .cloned()
                .chain(std::iter::once(package_id.clone()))
                .map(package_id_label)
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(ManifestError::CyclicDependency { cycle });
        }

        self.stack.push(package_id.clone());
        let package_sources = self.resolver.package_sources(&package_id)?;
        let parsed_manifest: SourceManifest = toml::from_str(&package_sources.manifest)?;
        let package_name: CrateName =
            parsed_manifest
                .package
                .name
                .parse()
                .map_err(|_| ManifestError::InvalidPackageName {
                    toml: pseudo_manifest_path(&package_id),
                    name: parsed_manifest.package.name.clone(),
                })?;
        let package_type = match parsed_manifest.package.package_type.as_str() {
            "lib" => PackageType::Library,
            "bin" => PackageType::Binary,
            invalid => {
                return Err(ManifestError::InvalidPackageType(
                    pseudo_manifest_path(&package_id),
                    invalid.to_string(),
                ));
            }
        };

        let entry_relative_path = parsed_manifest
            .package
            .entry
            .as_ref()
            .map(RelativeFilePath::new)
            .unwrap_or_else(|| default_entry_for_type(package_type));

        let mut files = BTreeMap::new();
        for (relative_path, content) in package_sources.files {
            let vfs_path = package_file_to_vfs_path(&package_id, &relative_path);
            let file_id = self.source_map.insert(vfs_path, content);
            files.insert(relative_path, file_id);
        }
        let entry_file_id = files.get(&entry_relative_path).copied().ok_or_else(|| {
            ManifestError::MissingFile(pseudo_file_path(&package_id, entry_relative_path.as_str()))
        })?;

        let mut dependency_packages = BTreeMap::new();
        for dependency_name in parsed_manifest.dependencies.keys() {
            let dependency_crate_name: CrateName =
                dependency_name
                    .parse()
                    .map_err(|_| ManifestError::InvalidDependencyName {
                        toml: pseudo_manifest_path(&package_id),
                        name: dependency_name.clone(),
                    })?;
            let dependency_package_id = self.resolver.resolve_dependency(&package_id, dependency_name)?;
            self.resolve_package(dependency_package_id.clone())?;
            dependency_packages.insert(dependency_crate_name, dependency_package_id);
        }

        let resolved = ResolvedSourcePackage {
            package_id: package_id.clone(),
            name: package_name,
            package_type,
            entry_relative_path,
            entry_file_id,
            dependency_packages,
            files,
        };
        self.packages.insert(package_id.clone(), resolved);
        let _ = self.stack.pop();
        Ok(())
    }
}

fn default_entry_for_type(package_type: PackageType) -> RelativeFilePath {
    match package_type {
        PackageType::Library => RelativeFilePath::new("src/lib.psy"),
        PackageType::Binary => RelativeFilePath::new("src/main.psy"),
    }
}

fn pseudo_manifest_path(package_id: &PackageId) -> PathBuf {
    pseudo_file_path(package_id, "Dargo.toml")
}

fn pseudo_file_path(package_id: &PackageId, relative: &str) -> PathBuf {
    match package_id {
        PackageId::Real(root) => root.join(relative),
        PackageId::Virtual(id) => PathBuf::from(format!("virtual://{id}/{relative}")),
    }
}

fn package_id_label(package_id: PackageId) -> String {
    match package_id {
        PackageId::Real(path) => path.display().to_string(),
        PackageId::Virtual(id) => format!("virtual://{id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_normalizes() {
        let path = RelativeFilePath::new("./src/./foo/../main.psy");
        assert_eq!(path.as_str(), "src/main.psy");
    }

    #[test]
    fn source_map_resolves_relative_virtual_paths() {
        let mut map = SourceMap::new();
        let main = map.insert(
            VfsPath::virtual_path("/workspace/root/src/main.psy"),
            Arc::from("mod foo;"),
        );
        let foo = map.insert(
            VfsPath::virtual_path("/workspace/root/src/foo.psy"),
            Arc::from("pub fn run() {}"),
        );

        let resolved = map.resolve_path(AnchoredPath {
            anchor: main,
            relative: "foo.psy",
        });
        assert_eq!(resolved, Some(foo));
    }

    #[test]
    fn package_path_mapping_is_stable() {
        let package = PackageId::Virtual("root".to_string());
        let path = RelativeFilePath::new("src/main.psy");
        let vfs = package_file_to_vfs_path(&package, &path);
        assert_eq!(vfs, VfsPath::virtual_path("/workspace/root/src/main.psy"));
    }

    #[test]
    fn resolve_source_workspace_builds_dependency_graph() {
        let mut resolver = MemoryResolver::default();
        resolver.insert_package(
            PackageId::Virtual("root".to_string()),
            PackageSources {
                manifest: Arc::from(
                    r#"
                    [package]
                    name = "root"
                    type = "bin"

                    [dependencies]
                    dep = { path = "../dep" }
                    "#,
                ),
                files: BTreeMap::from([(
                    RelativeFilePath::new("src/main.psy"),
                    Arc::from("fn main() {}"),
                )]),
            },
        );
        resolver.insert_package(
            PackageId::Virtual("dep".to_string()),
            PackageSources {
                manifest: Arc::from(
                    r#"
                    [package]
                    name = "dep"
                    type = "lib"
                    "#,
                ),
                files: BTreeMap::from([(
                    RelativeFilePath::new("src/lib.psy"),
                    Arc::from("pub fn helper() {}"),
                )]),
            },
        );
        resolver.insert_dependency(
            PackageId::Virtual("root".to_string()),
            "dep",
            PackageId::Virtual("dep".to_string()),
        );

        let workspace = resolve_source_workspace(PackageId::Virtual("root".to_string()), &resolver).unwrap();
        assert_eq!(workspace.packages.len(), 2);
        let root = workspace
            .packages
            .get(&PackageId::Virtual("root".to_string()))
            .expect("root package exists");
        assert!(root
            .dependency_packages
            .contains_key(&"dep".parse::<CrateName>().unwrap()));
    }
}
