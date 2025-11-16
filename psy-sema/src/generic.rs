use crate::{IdentId, Location, ScopeId, TypeId};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CheckedGenericParameter {
    pub name: IdentId,
    pub constraints: Vec<TypeId>,
    pub scope_id: ScopeId,
    pub location: Location,
}

impl CheckedGenericParameter {
    pub fn new(name: IdentId, constraints: Vec<TypeId>, scope_id: ScopeId, location: Location) -> Self {
        Self {
            name,
            constraints,
            scope_id,
            location,
        }
    }
}
