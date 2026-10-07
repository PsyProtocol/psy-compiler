use std::path::{Component, Path, PathBuf};

pub trait NormalizePath {
    /// Replacement for `std::fs::canonicalize` that doesn't verify the path
    /// exists.
    ///
    /// Plucked from <https://github.com/rust-lang/cargo/blob/fede83ccf973457de319ba6fa0e36ead454d2e20/src/cargo/util/paths.rs#L61>
    ///
    /// Advice from <https://www.reddit.com/r/rust/comments/hkkquy/comment/fwtw53s/>
    fn normalize(&self) -> PathBuf;
}

impl NormalizePath for PathBuf {
    fn normalize(&self) -> PathBuf {
        let components = self.components();
        resolve_components(components)
    }
}

impl NormalizePath for &Path {
    fn normalize(&self) -> PathBuf {
        let components = self.components();
        resolve_components(components)
    }
}

fn resolve_components<'a>(components: impl Iterator<Item = Component<'a>>) -> PathBuf {
    let mut components = components.peekable();

    // Preserve path prefix if one exists.
    let mut normalized_path = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!("Path cannot contain multiple prefixes"),
            Component::RootDir => {
                normalized_path.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                normalized_path.pop();
            }
            Component::Normal(c) => {
                normalized_path.push(c);
            }
        }
    }

    normalized_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_removes_dot_and_resolves_parent_components() {
        assert_eq!(PathBuf::from("a/./b/../c").normalize(), PathBuf::from("a/c"));
        assert_eq!(Path::new("/a/../b/./c").normalize(), PathBuf::from("/b/c"));
    }

    #[test]
    fn normalize_does_not_escape_relative_or_absolute_root() {
        assert_eq!(PathBuf::from("../../a").normalize(), PathBuf::from("a"));
        assert_eq!(PathBuf::from("/../../a").normalize(), PathBuf::from("/a"));
    }
}
