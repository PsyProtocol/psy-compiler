use psy_common::FileId;

#[derive(Copy, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    pub file_id: FileId,
    pub start: usize,
    pub end: usize,
}

impl Location {
    pub fn new(file_id: FileId, start: usize, end: usize) -> Self {
        Self { file_id, start, end }
    }
}

impl Default for Location {
    fn default() -> Self {
        Self {
            file_id: FileId(0),
            start: 0,
            end: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLocation {
    pub path: String,
    pub start: usize,
    pub end: usize,
}

impl Default for FileLocation {
    fn default() -> Self {
        Self {
            path: String::new(),
            start: 0,
            end: 0,
        }
    }
}

impl FileLocation {
    pub fn new(path: String, start: usize, end: usize) -> Self {
        Self { path, start, end }
    }
}

impl ariadne::Span for FileLocation {
    type SourceId = String;

    fn source(&self) -> &Self::SourceId {
        &self.path
    }

    fn start(&self) -> usize {
        self.start
    }

    fn end(&self) -> usize {
        self.end
    }
}
