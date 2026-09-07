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
        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            return false;
        };

        (first.is_ascii_alphabetic() || first == '_')
            && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
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

/// Creates a new CrateName using the ASCII identifier syntax.
impl FromStr for CrateName {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if Self::is_valid_name(name) {
            Ok(Self(SmolStr::new(name)))
        } else {
            Err("Package names must start with an ASCII letter or '_' and contain only ASCII letters, digits, or '_'".into())
        }
    }
}

/// Legacy blacklist export retained for downstream compatibility.
///
/// Crate names are now validated with an allowlist; new code should parse names as `CrateName` instead.
#[deprecated(note = "CrateName validation now uses an allowlist; parse the name as CrateName instead")]
pub const CHARACTER_BLACK_LIST: [char; 1] = ['-'];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_accepts_ascii_identifiers_and_rejects_invalid_names() {
        assert!(CrateName::from_str("").is_err());
        assert!(CrateName::from_str("not-valid").is_err());
        assert_eq!(CrateName::from_str("valid_name42").unwrap().to_string(), "valid_name42");
        assert_eq!(CrateName::from_str("_private").unwrap().to_string(), "_private");
        assert!(CrateName::from_str("42package").is_err());
    }

    #[test]
    fn crate_name_rejects_whitespace_punctuation_unicode_and_control_characters() {
        for name in ["a b", "a.b/c:d", "a\tb", "a\nb", "a\"b", "a\\b", "包_42", "a‐b", "a−b"] {
            assert!(CrateName::from_str(name).is_err(), "accepted invalid name {name:?}");
        }
        assert!(CrateName::from_str("a-b").is_err());
        assert!(CrateName::from_str("-").is_err());
    }

    #[test]
    fn crate_name_converts_to_owned_string_from_owned_or_borrowed_value() {
        let name = CrateName::from_str("package_name").unwrap();
        let borrowed: String = (&name).into();
        let owned: String = name.into();
        assert_eq!(borrowed, "package_name");
        assert_eq!(owned, "package_name");
    }

    #[test]
    fn package_type_and_entry_path_are_consistent() {
        let mut package = Package {
            root_dir: PathBuf::from("workspace/pkg"),
            entry_path: PathBuf::from("src/lib.psy"),
            name: CrateName::from_str("pkg").unwrap(),
            ..Package::default()
        };

        assert!(package.is_library());
        assert!(!package.is_binary());
        assert_eq!(package.package_type.to_string(), "lib");
        assert_eq!(package.entry_canonical_path(), PathBuf::from("workspace/pkg/src/lib.psy"));

        package.package_type = PackageType::Binary;
        assert!(package.is_binary());
        assert!(!package.is_library());
        assert_eq!(package.package_type.to_string(), "bin");
    }

    #[test]
    fn dependency_accessors_match_local_and_remote_variants() {
        let package = Package {
            name: CrateName::from_str("dep").unwrap(),
            package_type: PackageType::Binary,
            ..Package::default()
        };
        for dependency in [
            Dependency::Local { package: package.clone() },
            Dependency::Remote { package: package.clone() },
        ] {
            assert!(dependency.is_binary());
            assert_eq!(dependency.package_name().to_string(), "dep");
            assert_eq!(dependency.package().entry_path, PathBuf::new());
        }
    }
}
