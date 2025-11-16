use crate::{IdentId, Location, UncheckedType};

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParameter {
    pub name: IdentId,
    pub constraints: Vec<UncheckedType>,
    pub location: Location,
}

impl GenericParameter {
    pub fn new(name: IdentId, constraints: Vec<UncheckedType>, location: Location) -> Self {
        Self { name, constraints, location }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericQualifier {
    pub is_const: bool,
}

impl GenericQualifier {
    pub fn new(is_const: bool) -> Self {
        Self { is_const }
    }
}
