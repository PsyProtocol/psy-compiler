use std::fmt::Display;

use crate::{ExprId, Location, NodeInfo, NodeType};

#[derive(Copy, Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Pow,

    Mod,

    BitShr,
    BitShl,

    BitAnd,
    BitOr,
    BitXor,

    And,
    Or,

    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinaryNode {
    pub lhs: ExprId,
    pub operator: BinaryOperator,
    pub rhs: ExprId,
    pub location: Location,
}

impl Display for BinaryOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BinaryOperator::Add => write!(f, "+"),
            BinaryOperator::Sub => write!(f, "-"),
            BinaryOperator::Mul => write!(f, "*"),
            BinaryOperator::Div => write!(f, "/"),
            BinaryOperator::Pow => write!(f, "**"),
            BinaryOperator::Mod => write!(f, "%"),
            BinaryOperator::BitShr => write!(f, ">>"),
            BinaryOperator::BitShl => write!(f, "<<"),
            BinaryOperator::BitAnd => write!(f, "&"),
            BinaryOperator::BitOr => write!(f, "|"),
            BinaryOperator::BitXor => write!(f, "^"),
            BinaryOperator::And => write!(f, "&&"),
            BinaryOperator::Or => write!(f, "||"),
            BinaryOperator::Eq => write!(f, "=="),
            BinaryOperator::Neq => write!(f, "!="),
            BinaryOperator::Lt => write!(f, "<"),
            BinaryOperator::Lte => write!(f, "<="),
            BinaryOperator::Gt => write!(f, ">"),
            BinaryOperator::Gte => write!(f, ">="),
        }
    }
}

impl BinaryNode {
    pub fn new(lhs: ExprId, operator: BinaryOperator, rhs: ExprId, location: Location) -> Self {
        Self {
            lhs,
            operator,
            rhs,
            location,
        }
    }
}

impl NodeInfo for BinaryNode {
    fn node_type(&self) -> NodeType {
        NodeType::BinaryExpr
    }
}
