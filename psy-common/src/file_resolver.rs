use std::{cell::UnsafeCell, path::PathBuf};

use indexmap::IndexMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub usize);

#[derive(Debug)]
pub struct FileResolver {
    file_contents: UnsafeCell<Vec<String>>,
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

    pub fn resolve_id(&self, file_path: &PathBuf) -> Option<&FileId> {
        let file_path = file_path.canonicalize().ok()?;
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
        let file_path = file_path.canonicalize()?;
        unsafe {
            let file_ids = &mut *self.file_ids.get();
            if let Some(&file_id) = file_ids.get(&file_path) {
                return Ok(file_id);
            }

            let file_contents = &mut *self.file_contents.get();
            let file_paths = &mut *self.file_paths.get();
            let file_id = FileId(file_contents.len());
            file_contents.push(std::fs::read_to_string(&file_path)?);
            file_paths.push(file_path.clone());
            file_ids.insert(file_path, file_id);
            Ok(file_id)
        }
    }

    pub fn resolve_content(&self, file_id: &FileId) -> Option<&str> {
        unsafe {
            let file_contents = &*self.file_contents.get();
            file_contents.get(file_id.0).map(|s| s.as_str())
        }
    }
}

impl Default for FileResolver {
    fn default() -> Self {
        Self::new()
    }
}
