use std::fmt::Display;

use crate::{Comment, ExprId, Location, NodeInfo, NodeType};

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AssignmentOperator {
    Eq,

    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,

    ModAssign,

    BitAndAssign,
    BitOrAssign,
    BitXorAssign,

    BitShlAssign,
    BitShrAssign,
}

impl Display for AssignmentOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssignmentOperator::Eq => write!(f, "="),
            AssignmentOperator::AddAssign => write!(f, "+="),
            AssignmentOperator::SubAssign => write!(f, "-="),
            AssignmentOperator::MulAssign => write!(f, "*="),
            AssignmentOperator::DivAssign => write!(f, "/="),
            AssignmentOperator::ModAssign => write!(f, "%="),
            AssignmentOperator::BitAndAssign => write!(f, "&="),
            AssignmentOperator::BitOrAssign => write!(f, "|="),
            AssignmentOperator::BitXorAssign => write!(f, "^="),
            AssignmentOperator::BitShlAssign => write!(f, "<<="),
            AssignmentOperator::BitShrAssign => write!(f, ">>="),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AssignmentNode {
    pub target: ExprId,
    pub operator: AssignmentOperator,
    pub value: ExprId,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl AssignmentNode {
    pub fn new(target: ExprId, operator: AssignmentOperator, value: ExprId, comments: Vec<Comment>, location: Location) -> Self {
        Self {
            target,
            operator,
            value,
            comments,
            location,
        }
    }
}

impl NodeInfo for AssignmentNode {
    fn node_type(&self) -> NodeType {
        NodeType::AssignmentStmt
    }
}
