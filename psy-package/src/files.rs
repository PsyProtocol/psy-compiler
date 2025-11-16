use std::path::{Component, Path, PathBuf};

use crate::errors::ManifestError;

/// Searches for a `Dargo.toml` file in the current directory and all parent
/// directories. For example, if the current directory is
/// `/workspace/package/src`, then this function will search for a `Dargo.toml`
/// file in
/// * `/workspace/package/src`,
/// * `/workspace/package`,
/// * `/workspace`.
///
/// Returns the [PathBuf] of the `Dargo.toml` file if found, otherwise returns
/// None.
///
/// It will return innermost `Dargo.toml` file, which is the one closest to the
/// current directory. For example, if the current directory is
/// `/workspace/package/src`, then this function will return the `Dargo.toml`
/// file in `/workspace/package/Dargo.toml`
pub fn find_file_manifest(current_path: &Path) -> Option<PathBuf> {
    for path in current_path.ancestors() {
        if let Ok(toml_path) = get_package_manifest(path) {
            return Some(toml_path);
        }
    }
    None
}

/// Returns the [PathBuf] of the directory containing the `Dargo.toml` by
/// searching from `current_path` to the root of its [Path], returning at the
/// innermost directory found, i.e. the one corresponding to the package that
/// contains the `current_path`.
///
/// Returns a [ManifestError] if no parent directories of `current_path` contain
/// a manifest file.
pub fn find_file_manifest_root(current_path: &Path) -> Result<PathBuf, ManifestError> {
    match find_file_manifest(current_path) {
        Some(manifest_path) => {
            let package_root = manifest_path.parent().expect("infallible: manifest file path can't be root directory");
            Ok(package_root.to_path_buf())
        }
        None => Err(ManifestError::MissingFile(current_path.to_path_buf())),
    }
}

/// Get the root of path, for example:
/// * `C:\foo\bar` -> `C:\foo`
/// * `//shared/foo/bar` -> `//shared/foo`
/// * `/foo` -> `/foo` otherwise empty path.
pub fn path_root(path: &Path) -> PathBuf {
    let mut components = path.components();
    match (components.next(), components.next()) {
        // Preserve prefix if one exists
        (Some(prefix @ Component::Prefix(_)), Some(root @ Component::RootDir)) => PathBuf::from(prefix.as_os_str()).join(root.as_os_str()),
        (Some(root @ Component::RootDir), _) => PathBuf::from(root.as_os_str()),
        _ => PathBuf::new(),
    }
}

/// Returns the [PathBuf] of the `Dargo.toml` file by searching from
/// `current_path` and stopping at `root_path`.
///
/// Returns a [ManifestError] if no parent directories of `current_path` contain
/// a manifest file.
pub fn find_package_manifest(root_path: &Path, current_path: &Path) -> Result<PathBuf, ManifestError> {
    if current_path.starts_with(root_path) {
        let mut found_toml_paths = Vec::new();
        for path in current_path.ancestors() {
            if let Ok(toml_path) = get_package_manifest(path) {
                found_toml_paths.push(toml_path);
            }
            // While traversing, break once we process the root specified
            if path == root_path {
                break;
            }
        }

        // Return the shallowest Dargo.toml, which will be the last in the list
        found_toml_paths
            .pop()
            .ok_or_else(|| ManifestError::MissingFile(current_path.to_path_buf()))
    } else {
        Err(ManifestError::NoCommonAncestor {
            root: root_path.to_path_buf(),
            current: current_path.to_path_buf(),
        })
    }
}

/// Returns the [PathBuf] of the `Dargo.toml` file in the `current_path`
/// directory.
///
/// Returns a [ManifestError] if `current_path` does not contain a manifest
/// file.
pub fn get_package_manifest(current_path: &Path) -> Result<PathBuf, ManifestError> {
    let toml_path = current_path.join("Dargo.toml");
    if toml_path.exists() {
        Ok(toml_path)
    } else {
        Err(ManifestError::MissingFile(current_path.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        str::FromStr,
    };

    use crate::{errors::ManifestError, files::find_file_manifest_root};

    /// Test that `find_file_manifest_root` handles all kinds of prefixes.
    #[test]
    fn test_find_manifest_root_does_not_panic() {
        let assert_error = |path: &str| {
            let path = PathBuf::from_str(path).unwrap();
            let error = find_file_manifest_root(&path).expect_err("non-existing paths");
            assert!(matches!(error, ManifestError::MissingFile(_)));
        };
        assert_error("C:\\foo\\bar");
        assert_error("//shared/foo/bar");
        assert_error("/foo/bar");
        assert_error("bar/baz");
    }

    /// Test to demonstrate how `find_root` works.
    #[test]
    fn test_find_root_example() {
        const INDENT_SIZE: usize = 4;
        /// Create directories and files according to a YAML-like layout below
        fn setup(layout: &str, root: &Path) {
            fn is_dir(item: &str) -> bool {
                !item.contains('.')
            }
            let mut current_dir = root.to_path_buf();
            let mut current_indent = 0;
            let mut last_item: Option<String> = None;

            for line in layout.lines() {
                if let Some((prefix, item)) = line.split_once('-') {
                    let item = item.replace(std::path::MAIN_SEPARATOR, "_").trim().to_string();

                    let indent = prefix.len() / INDENT_SIZE;

                    if last_item.is_none() {
                        current_indent = indent;
                    }

                    assert!(
                        indent <= current_indent + 1,
                        "cannot increase indent by more than {INDENT_SIZE}; item = {item}, current_dir={}",
                        current_dir.display()
                    );

                    // Go into the last created directory
                    if indent > current_indent && last_item.is_some() {
                        let last_item = last_item.unwrap();
                        assert!(is_dir(&last_item), "last item was not a dir: {last_item}");
                        current_dir.push(last_item);
                        current_indent += 1;
                    }
                    // Go back into an ancestor directory
                    while indent < current_indent {
                        current_dir.pop();
                        current_indent -= 1;
                    }
                    // Create a file or a directory
                    let item_path = current_dir.join(&item);
                    if is_dir(&item) {
                        std::fs::create_dir(&item_path).unwrap_or_else(|e| panic!("failed to create dir {}: {e}", item_path.display()));
                    } else {
                        std::fs::write(&item_path, "").expect("failed to create file");
                    }

                    last_item = Some(item);
                }
            }
        }

        // Temporary directory to hold the project.
        let tmp = tempfile::tempdir().unwrap();
        // Join a string path to the tmp dir
        let path = |p: &str| tmp.path().join(p);
        // Check that an expected root is found
        let assert_ok = |current_dir: &str, exp: &str| {
            let root = find_file_manifest_root(&path(current_dir)).expect("should find a manifest root");
            assert_eq!(root, path(exp));
        };
        let assert_err = |current_dir: &str| {
            find_file_manifest_root(&path(current_dir)).expect_err("shouldn't find a manifest root");
        };
        // Check that a root is not found
        let layout = r"
            - project
                - docs
                - foo
                    - Dargo.toml
                    - src
                        - main.psy
                        - lib.psy
                - bar
                    - src
                        - dummy.txt
            ";

        // Set up the file system.
        setup(layout, tmp.path());
        assert_ok("project/foo", "project/foo");
        assert_ok("project/foo/src", "project/foo");
        assert_err("project/baz");
        assert_err("project/baz/src");
    }
}
