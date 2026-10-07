use psy_ast::{ExprId, Location, NodeInfo, NodeType};

use crate::TypeId;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedCase {
    pub predicate: ExprId,
    pub type_id: TypeId,
    pub body: ExprId,
}

impl CheckedCase {
    pub fn new(predicate: ExprId, type_id: TypeId, body: ExprId) -> Self {
        Self { predicate, type_id, body }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedIfExprNode {
    pub if_branch: CheckedCase,
    pub elseif_branches: Vec<CheckedCase>,
    pub else_branch: Option<ExprId>,
    pub type_id: TypeId,
    pub location: Location,
}

impl NodeInfo for CheckedIfExprNode {
    fn node_type(&self) -> NodeType {
        NodeType::IfExpr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_case_new_stores_all_fields() {
        // A function pointer keeps the tiny constructor from being inlined
        // into the caller so its own body executes out-of-line.
        let new_fn: fn(ExprId, TypeId, ExprId) -> CheckedCase = CheckedCase::new;
        let case = new_fn(ExprId(1), TypeId(2), ExprId(3));
        assert_eq!(case.predicate, ExprId(1));
        assert_eq!(case.type_id, TypeId(2));
        assert_eq!(case.body, ExprId(3));
    }
}
