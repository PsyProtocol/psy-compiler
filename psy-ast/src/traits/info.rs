use crate::{DefId, ExprId, NodeType};

pub trait NodeInfo {
    fn node_type(&self) -> NodeType;
    fn as_expression(&self) -> Option<ExprId> {
        None
    }
    fn as_definition(&self) -> Option<DefId> {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::{BinaryNode, BinaryOperator, ExprId, Location, NodeInfo, NodeType};

    #[test]
    fn default_node_info_impls_expose_no_expression_or_definition_handles() {
        let node = BinaryNode {
            lhs: ExprId(0),
            operator: BinaryOperator::Add,
            rhs: ExprId(1),
            location: Location::default(),
        };
        // Go through the trait object so the default method bodies execute
        // out-of-line instead of being inlined into the caller.
        let info: &dyn NodeInfo = &node;
        assert_eq!(info.node_type(), NodeType::BinaryExpr);
        assert_eq!(info.as_expression(), None);
        assert_eq!(info.as_definition(), None);
    }
}
