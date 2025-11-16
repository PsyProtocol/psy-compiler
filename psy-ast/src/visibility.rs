use std::fmt::Display;

use enum_as_inner::EnumAsInner;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumAsInner)]
pub enum Visibility {
    Public,
    Private,
}

impl Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Visibility::Public => write!(f, "pub"),
            Visibility::Private => Ok(()),
        }
    }
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Private
    }
}
