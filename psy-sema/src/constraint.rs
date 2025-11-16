use crate::TypeId;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Constraint {
    pub constraints: Vec<TypeId>,
}

impl Constraint {
    pub fn new(constraints: Vec<TypeId>) -> Self {
        Self { constraints }
    }
}
