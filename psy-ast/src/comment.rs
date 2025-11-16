use crate::Location;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comment {
    Line { content: String, location: Location },
    Block { content: String, location: Location },
}

impl Comment {
    pub fn new_line(content: String, location: Location) -> Self {
        Self::Line { content, location }
    }

    pub fn new_block(content: String, location: Location) -> Self {
        Self::Block { content, location }
    }
    pub fn content(&self) -> &str {
        match self {
            Self::Line { content, .. } => content,
            Self::Block { content, .. } => content,
        }
    }
    pub fn location(&self) -> Location {
        match self {
            Self::Line { location, .. } => *location,
            Self::Block { location, .. } => *location,
        }
    }
}

impl std::fmt::Display for Comment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Line { content, .. } => write!(f, "// {}", content),
            Self::Block { content, .. } => write!(f, "/* {} */", content),
        }
    }
}
