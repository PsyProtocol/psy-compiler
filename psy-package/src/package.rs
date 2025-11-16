use std::{collections::BTreeMap, fmt::Display, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

#[derive(Clone, Debug)]
pub enum Dependency {
    Local { package: Package },
    Remote { package: Package },
}

impl Dependency {
    pub fn is_binary(&self) -> bool {
        match self {
            Self::Local { package } | Self::Remote { package } => package.is_binary(),
        }
    }

    pub fn package_name(&self) -> &CrateName {
        match self {
            Self::Local { package } | Self::Remote { package } => &package.name,
        }
    }

    pub fn package(&self) -> &Package {
        match self {
            Self::Local { package } | Self::Remote { package } => package,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Package {
    pub version: Option<String>,
    pub root_dir: PathBuf,
    pub package_type: PackageType,
    pub entry_path: PathBuf,
    pub name: CrateName,
    pub dependencies: BTreeMap<CrateName, Dependency>,
}

impl Package {
    pub fn is_binary(&self) -> bool {
        self.package_type == PackageType::Binary
    }

    pub fn is_library(&self) -> bool {
        self.package_type == PackageType::Library
    }

    pub fn entry_canonical_path(&self) -> PathBuf {
        self.root_dir.join(&self.entry_path)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum PackageType {
    #[default]
    Library,
    Binary,
}

impl Display for PackageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Library => write!(f, "lib"),
            Self::Binary => write!(f, "bin"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize, Default)]
pub struct CrateName(SmolStr);

impl CrateName {
    fn is_valid_name(name: &str) -> bool {
        !name.is_empty() && name.chars().all(|n| !CHARACTER_BLACK_LIST.contains(&n))
    }
}

impl Display for CrateName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<CrateName> for String {
    fn from(crate_name: CrateName) -> Self {
        crate_name.0.into()
    }
}

impl From<&CrateName> for String {
    fn from(crate_name: &CrateName) -> Self {
        crate_name.0.clone().into()
    }
}

/// Creates a new CrateName rejecting any crate name that
/// has a character on the blacklist.
/// The difference between RA and this implementation is that
/// characters on the blacklist are never allowed; there is no normalization.
impl FromStr for CrateName {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if Self::is_valid_name(name) {
            Ok(Self(SmolStr::new(name)))
        } else {
            Err("Package names must be non-empty and cannot contain hyphens".into())
        }
    }
}

/// List of characters that are not allowed in a crate name
/// For example, Hyphen(-) is disallowed as it is similar to underscore(_)
/// and we do not want names that differ by a hyphen
pub const CHARACTER_BLACK_LIST: [char; 1] = ['-'];
