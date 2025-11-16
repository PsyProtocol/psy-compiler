use std::fmt::Display;

use crate::Location;

#[derive(Copy, Debug, Clone, PartialEq)]
pub struct Qualifier {
    pub is_extern: bool,
    pub is_const: bool,
    pub location: Location,
}

impl Default for Qualifier {
    fn default() -> Self {
        Self {
            is_extern: false,
            is_const: false,
            location: Location::default(),
        }
    }
}

impl Display for Qualifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_extern {
            write!(f, "extern ")?;
        }
        if self.is_const {
            write!(f, "const ")?;
        }
        Ok(())
    }
}

impl Qualifier {
    pub fn new(is_extern: bool, is_const: bool, location: Location) -> Self {
        Self {
            is_extern,
            is_const,
            location,
        }
    }
}

#[derive(Copy, Debug, Clone, PartialEq)]
pub struct TypeQualifier {
    pub is_mutable: bool,
    pub location: Location,
}

impl Display for TypeQualifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_mutable {
            write!(f, "mut ")?;
        }
        Ok(())
    }
}

impl TypeQualifier {
    pub fn new(is_mutable: bool, location: Location) -> TypeQualifier {
        TypeQualifier { is_mutable, location }
    }
}
