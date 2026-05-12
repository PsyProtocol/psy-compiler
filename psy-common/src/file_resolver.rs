use std::{
    cell::UnsafeCell,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use indexmap::IndexMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub usize);

#[derive(Debug)]
pub struct FileResolver {
    file_contents: UnsafeCell<Vec<Arc<str>>>,
    file_ids: UnsafeCell<IndexMap<PathBuf, FileId>>,
    file_paths: UnsafeCell<Vec<PathBuf>>,
}

unsafe impl Sync for FileResolver {}

impl Clone for FileResolver {
    fn clone(&self) -> Self {
        let file_contents = unsafe { &*self.file_contents.get() };
        let file_ids = unsafe { &*self.file_ids.get() };
        let file_paths = unsafe { &*self.file_paths.get() };

        FileResolver {
            file_contents: UnsafeCell::new(file_contents.clone()),
            file_ids: UnsafeCell::new(file_ids.clone()),
            file_paths: UnsafeCell::new(file_paths.clone()),
        }
    }
}

impl FileResolver {
    pub fn new() -> Self {
        Self {
            file_contents: UnsafeCell::new(Vec::with_capacity(20)),
            file_ids: UnsafeCell::new(IndexMap::new()),
            file_paths: UnsafeCell::new(Vec::with_capacity(20)),
        }
    }

    pub fn resolve_id(&self, file_path: &Path) -> Option<&FileId> {
        let file_path = normalize_resolved_path(file_path);
        unsafe {
            let file_ids = &mut *self.file_ids.get();
            file_ids.get(&file_path)
        }
    }

    pub fn resolve_path(&self, file_id: &FileId) -> Option<&PathBuf> {
        unsafe {
            let file_paths = &mut *self.file_paths.get();
            let file_id = file_id.0;
            file_paths.get(file_id)
        }
    }

    pub fn resolve_file(&self, file_path: PathBuf) -> std::io::Result<FileId> {
        let file_path = normalize_resolved_path(&file_path);
        unsafe {
            let file_ids = &mut *self.file_ids.get();
            if let Some(&file_id) = file_ids.get(&file_path) {
                return Ok(file_id);
            }

            let file_contents = &mut *self.file_contents.get();
            let file_paths = &mut *self.file_paths.get();
            let file_id = FileId(file_contents.len());
            file_contents.push(Arc::<str>::from(std::fs::read_to_string(&file_path)?));
            file_paths.push(file_path.clone());
            file_ids.insert(file_path, file_id);
            Ok(file_id)
        }
    }

    pub fn add_file(&self, file_path: PathBuf, content: impl Into<Arc<str>>) -> FileId {
        let file_path = normalize_resolved_path(&file_path);
        let content = content.into();
        unsafe {
            let file_ids = &mut *self.file_ids.get();
            if let Some(&file_id) = file_ids.get(&file_path) {
                let file_contents = &mut *self.file_contents.get();
                file_contents[file_id.0] = content;
                return file_id;
            }

            let file_contents = &mut *self.file_contents.get();
            let file_paths = &mut *self.file_paths.get();
            let file_id = FileId(file_contents.len());
            file_contents.push(content);
            file_paths.push(file_path.clone());
            file_ids.insert(file_path, file_id);
            file_id
        }
    }

    pub fn resolve_content(&self, file_id: &FileId) -> Option<&str> {
        unsafe {
            let file_contents = &*self.file_contents.get();
            file_contents.get(file_id.0).map(|s| s.as_ref())
        }
    }

    pub fn resolve_path_content(&self, file_path: &Path) -> Option<&str> {
        let file_id = self.resolve_id(file_path).copied()?;
        self.resolve_content(&file_id)
    }

    pub fn files(&self) -> Vec<(PathBuf, Arc<str>)> {
        unsafe {
            let file_paths = &*self.file_paths.get();
            let file_contents = &*self.file_contents.get();
            file_paths
                .iter()
                .cloned()
                .zip(file_contents.iter().cloned())
                .collect()
        }
    }
}

impl Default for FileResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_resolved_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| normalize_path(path))
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
