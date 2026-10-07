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

#[cfg(test)]
mod tests {
    use ariadne::Span;
    use psy_common::FileId;

    use super::*;

    #[test]
    fn locations_construct_and_default() {
        let location = Location::new(FileId(3), 5, 9);
        assert_eq!((location.file_id, location.start, location.end), (FileId(3), 5, 9));

        let default = Location::default();
        assert_eq!((default.file_id, default.start, default.end), (FileId(0), 0, 0));
    }

    #[test]
    fn file_locations_construct_default_and_render_as_spans() {
        let file_location = FileLocation::new("main.psy".to_string(), 2, 7);
        assert_eq!(file_location.path, "main.psy");
        assert_eq!(file_location.start(), 2);
        assert_eq!(file_location.end(), 7);
        assert_eq!(file_location.source(), "main.psy");

        let default = FileLocation::default();
        assert_eq!((default.path.as_str(), default.start, default.end), ("", 0, 0));
        assert_eq!(default.source(), "");
    }
}
