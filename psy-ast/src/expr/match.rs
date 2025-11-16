use enum_as_inner::EnumAsInner;

use crate::{ExprId, Location, NodeInfo, NodeType};

#[derive(Debug, Clone, PartialEq)]
pub struct MatchNode {
    pub scrutinee: ExprId,
    pub arms: Vec<MatchArm>,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: ExprId,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum MatchPattern {
    Value(ExprId, Location),
    PlaceHolder(Location),
}

impl MatchPattern {
    pub fn location(&self) -> Location {
        match self {
            MatchPattern::Value(_, location) => location.clone(),
            MatchPattern::PlaceHolder(location) => location.clone(),
        }
    }
}
impl NodeInfo for MatchNode {
    fn node_type(&self) -> NodeType {
        NodeType::MatchExpr
    }
}
