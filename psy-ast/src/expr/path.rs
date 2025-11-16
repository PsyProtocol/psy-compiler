use crate::{IdentId, Location, NodeInfo, NodeType, UncheckedType};

#[derive(Clone, Debug, PartialEq)]
pub struct PathNode {
    pub root: Option<UncheckedType>,
    pub segments: Vec<UncheckedType>,
    pub target: UncheckedType,
    pub location: Location,
}

impl PathNode {
    pub fn from_target(target: UncheckedType) -> Self {
        Self {
            root: None,
            segments: vec![],
            target: target.clone(),
            location: target.location(),
        }
    }

    pub fn is_receiver(&self) -> bool {
        self.root.is_none() && self.segments.is_empty() && self.target.is_basic() && self.target.as_basic().unwrap().id == IdentId::SELF
    }
}

impl NodeInfo for PathNode {
    fn node_type(&self) -> NodeType {
        NodeType::PathExpr
    }
}
