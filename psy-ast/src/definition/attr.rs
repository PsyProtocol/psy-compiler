use crate::{IdentId, Identifier, Location};

#[derive(Clone, Debug, PartialEq)]
pub struct AttrNode {
    pub path: Vec<Identifier>,
    pub name: Identifier,
    pub properties: Vec<Identifier>,
    pub location: Location,
}

impl AttrNode {
    pub fn is_derive(&self) -> bool {
        self.name == IdentId::DERIVE && self.path.is_empty()
    }

    pub fn is_test(&self) -> bool {
        self.name == IdentId::TEST && self.path.is_empty()
    }

    pub fn is_should_panic(&self) -> bool {
        self.name == IdentId::SHOULD_PANIC && self.path.is_empty()
    }

    pub fn is_contract_view_method(&self) -> bool {
        self.name == IdentId::VIEW_METHOD && self.path.len() == 1 && self.path[0] == IdentId::CONTRACT
    }

    pub fn is_contract_write_method(&self) -> bool {
        self.name == IdentId::WRITE_METHOD && self.path.len() == 1 && self.path[0] == IdentId::CONTRACT
    }

    pub fn is_contract_api_method(&self) -> bool {
        self.is_contract_write_method() || self.is_contract_view_method()
    }
}
