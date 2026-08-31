use std::{
    path::{Component, Path, PathBuf},
    sync::{Arc, RwLock},
};

use indexmap::IndexMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub usize);

#[derive(Debug)]
pub struct FileResolver {
    state: RwLock<FileResolverState>,
}

#[derive(Clone, Debug)]
struct FileResolverState {
    file_contents: Vec<Arc<str>>,
    file_ids: IndexMap<PathBuf, FileId>,
    file_paths: Vec<PathBuf>,
}

impl Clone for FileResolver {
    fn clone(&self) -> Self {
        FileResolver {
            state: RwLock::new(self.state.read().expect("file resolver lock poisoned").clone()),
        }
    }
}

impl FileResolver {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(FileResolverState {
                file_contents: Vec::with_capacity(20),
                file_ids: IndexMap::new(),
                file_paths: Vec::with_capacity(20),
            }),
        }
    }

    pub fn resolve_id(&self, file_path: &Path) -> Option<FileId> {
        let file_path = normalize_resolved_path(file_path);
        self.state.read().expect("file resolver lock poisoned").file_ids.get(&file_path).copied()
    }

    pub fn resolve_path(&self, file_id: &FileId) -> Option<PathBuf> {
        self.state.read().expect("file resolver lock poisoned").file_paths.get(file_id.0).cloned()
    }

    pub fn resolve_file(&self, file_path: PathBuf) -> std::io::Result<FileId> {
        let file_path = normalize_resolved_path(&file_path);
        let mut state = self.state.write().expect("file resolver lock poisoned");
        if let Some(&file_id) = state.file_ids.get(&file_path) {
            return Ok(file_id);
        }

        let content = Arc::<str>::from(std::fs::read_to_string(&file_path)?);
        let file_id = FileId(state.file_contents.len());
        state.file_contents.push(content);
        state.file_paths.push(file_path.clone());
        state.file_ids.insert(file_path, file_id);
        Ok(file_id)
    }

    pub fn add_file(&self, file_path: PathBuf, content: impl Into<Arc<str>>) -> FileId {
        let file_path = normalize_resolved_path(&file_path);
        let content = content.into();
        let mut state = self.state.write().expect("file resolver lock poisoned");
        if let Some(&file_id) = state.file_ids.get(&file_path) {
            state.file_contents[file_id.0] = content;
            return file_id;
        }

        let file_id = FileId(state.file_contents.len());
        state.file_contents.push(content);
        state.file_paths.push(file_path.clone());
        state.file_ids.insert(file_path, file_id);
        file_id
    }

    pub fn resolve_content(&self, file_id: &FileId) -> Option<Arc<str>> {
        self.state.read().expect("file resolver lock poisoned").file_contents.get(file_id.0).cloned()
    }

    /// Return an owned handle so parser callers can keep source text alive
    /// while mutably borrowing the rest of the program.
    pub fn resolve_content_arc(&self, file_id: &FileId) -> Option<Arc<str>> {
        self.resolve_content(file_id)
    }

    pub fn resolve_path_content(&self, file_path: &Path) -> Option<Arc<str>> {
        let file_id = self.resolve_id(file_path)?;
        self.resolve_content(&file_id)
    }

    pub fn files(&self) -> Vec<(PathBuf, Arc<str>)> {
        let state = self.state.read().expect("file resolver lock poisoned");
        state
            .file_paths
            .iter()
            .cloned()
            .zip(state.file_contents.iter().cloned())
            .collect()
    }
}

impl Default for FileResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_resolved_path(path: &Path) -> PathBuf {
    #[cfg(miri)]
    {
        // Miri's default isolation mode does not support `realpath`.
        normalize_path(path)
    }

    #[cfg(not(miri))]
    {
        path.canonicalize().unwrap_or_else(|_| normalize_path(path))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();
    let mut normalized_path = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!("path cannot contain multiple prefixes"),
            Component::RootDir => normalized_path.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized_path.pop() {
                    normalized_path.push("..");
                }
            }
            Component::Normal(c) => normalized_path.push(c),
        }
    }

    normalized_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_reference_survives_update() {
        let resolver = FileResolver::new();
        let path = PathBuf::from("content_update_test.psy");
        let file_id = resolver.add_file(path.clone(), "old content");
        let old_content = resolver.resolve_content(&file_id).unwrap();

        let updated_file_id = resolver.add_file(path, "new content");

        assert_eq!(updated_file_id, file_id);
        assert_eq!(&*old_content, "old content");
        assert_eq!(&*resolver.resolve_content(&file_id).unwrap(), "new content");
    }

    #[test]
    fn concurrent_adds_keep_resolver_state_consistent() {
        let resolver = Arc::new(FileResolver::new());
        let mut threads = Vec::new();

        for thread_id in 0..8 {
            let resolver = Arc::clone(&resolver);
            threads.push(std::thread::spawn(move || {
                for file_index in 0..50 {
                    let path = PathBuf::from(format!("concurrent_{thread_id}_{file_index}.psy"));
                    resolver.add_file(path, format!("{thread_id}:{file_index}"));
                }
            }));
        }

        for thread in threads {
            thread.join().unwrap();
        }

        let files = resolver.files();
        assert_eq!(files.len(), 400);
        for (path, content) in files {
            let file_id = resolver.resolve_id(&path).unwrap();
            assert_eq!(resolver.resolve_path(&file_id).as_deref(), Some(path.as_path()));
            assert_eq!(resolver.resolve_content(&file_id).as_deref(), Some(content.as_ref()));
        }
    }

    #[test]
    fn cloned_resolvers_are_independent_consistent_snapshots() {
        let resolver = FileResolver::new();
        let clone = resolver.clone();
        let path = PathBuf::from("clone_update_test.psy");

        let file_id = resolver.add_file(path.clone(), "first");
        let updated_id = clone.add_file(path.clone(), "second");

        assert_eq!(updated_id, file_id);
        assert_eq!(resolver.resolve_id(&path), Some(file_id));
        assert_eq!(clone.resolve_path(&file_id).as_deref(), Some(path.as_path()));
        assert_eq!(resolver.resolve_content(&file_id).as_deref(), Some("first"));
        assert_eq!(clone.resolve_content(&file_id).as_deref(), Some("second"));
    }

    #[test]
    fn concurrent_updates_to_same_path_keep_one_file_id() {
        let resolver = Arc::new(FileResolver::new());
        let path = PathBuf::from("shared_concurrent_file.psy");
        let initial_id = resolver.add_file(path.clone(), "initial");
        let mut threads = Vec::new();

        for thread_id in 0..16 {
            let resolver = Arc::clone(&resolver);
            let path = path.clone();
            threads.push(std::thread::spawn(move || {
                resolver.add_file(path, format!("content-{thread_id}"))
            }));
        }

        for thread in threads {
            assert_eq!(thread.join().unwrap(), initial_id);
        }

        assert_eq!(resolver.files().len(), 1);
        assert_eq!(resolver.resolve_id(&path), Some(initial_id));
        assert!(resolver.resolve_content(&initial_id).unwrap().starts_with("content-"));
    }
}
