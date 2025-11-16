use crate::{IdentId, Identifier, Location};

#[derive(Clone, Debug, PartialEq)]
pub struct AttrNode {
    pub name: Identifier,
    pub properties: Vec<Identifier>,
    pub location: Location,
}

impl AttrNode {
    pub fn is_derive(&self) -> bool {
        self.name == IdentId::DERIVE
    }

    pub fn is_test(&self) -> bool {
        self.name == IdentId::TEST
    }

    pub fn is_should_panic(&self) -> bool {
        self.name == IdentId::SHOULD_PANIC
    }
}
