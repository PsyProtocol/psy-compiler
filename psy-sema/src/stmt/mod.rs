mod assignment;
mod r#for;
mod intrinsic;
mod r#return;
mod variable;
mod r#while;

pub use assignment::*;
use enum_as_inner::EnumAsInner;
pub use intrinsic::*;
use psy_ast::{DefId, ExprId, NodeInfo, NodeType};
pub use r#for::*;
pub use r#return::*;
pub use r#while::*;
pub use variable::*;

use crate::{CheckedDefinitionNode, CheckedExprNode};

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum CheckedStmtNode {
    While(CheckedWhileNode),
    For(CheckedForNode),
    Assignment(CheckedAssignmentNode),
    Variable(CheckedVariableNode),
    Definition(DefId),
    Expression(ExprId),
    Return(CheckedReturnNode),
    Intrinsic(CheckedIntrinsicStmtNode),
}

impl NodeInfo for CheckedStmtNode {
    fn node_type(&self) -> NodeType {
        match self {
            Self::While(node) => node.node_type(),
            Self::For(node) => node.node_type(),
            Self::Assignment(node) => node.node_type(),
            Self::Variable(node) => node.node_type(),
            Self::Definition(_node) => NodeType::DefinitionStmt,
            Self::Expression(_node) => NodeType::ExpressionStmt,
            Self::Return(node) => node.node_type(),
            Self::Intrinsic(node) => node.node_type(),
        }
    }

    fn as_expression(&self) -> Option<ExprId> {
        match self {
            Self::Expression(expr) => Some(*expr),
            _ => None,
        }
    }

    fn as_definition(&self) -> Option<DefId> {
        match self {
            Self::Definition(def) => Some(*def),
            _ => None,
        }
    }
}

impl From<ExprId> for CheckedStmtNode {
    fn from(value: ExprId) -> Self {
        Self::Expression(value)
    }
}

impl From<DefId> for CheckedStmtNode {
    fn from(value: DefId) -> Self {
        Self::Definition(value)
    }
}

impl<F> From<CheckedExprNode<F>> for CheckedStmtNode {
    fn from(_value: CheckedExprNode<F>) -> Self {
        todo!()
    }
}

impl From<CheckedDefinitionNode> for CheckedStmtNode {
    fn from(_value: CheckedDefinitionNode) -> Self {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use psy_ast::{
        AssignmentOperator, Comment, DefId, ExprId, IdentId, Identifier, Location, NodeInfo, NodeType, TypeQualifier,
    };

    use super::*;
    use crate::{ScopeId, TypeId};

    #[test]
    fn checked_statements_report_their_node_types() {
        let location = Location::default();

        let while_node = CheckedWhileNode {
            predicate: ExprId(0),
            type_id: TypeId(0),
            body: ExprId(1),
            comments: vec![],
            location,
        };
        assert_eq!(CheckedStmtNode::While(while_node.clone()).node_type(), NodeType::WhileStmt);
        assert_eq!(while_node.node_type(), NodeType::WhileStmt);

        let for_node = CheckedForNode {
            variable: Identifier::new(IdentId(0), location),
            start: ExprId(0),
            end: ExprId(1),
            body: ExprId(2),
            scope_id: ScopeId(0),
            comments: vec![],
            location,
        };
        assert_eq!(CheckedStmtNode::For(for_node.clone()).node_type(), NodeType::ForStmt);
        assert_eq!(for_node.node_type(), NodeType::ForStmt);

        let assignment = CheckedAssignmentNode {
            target: ExprId(0),
            operator: AssignmentOperator::AddAssign,
            value: ExprId(1),
            type_id: TypeId(0),
            comments: vec![],
            location,
        };
        assert_eq!(CheckedStmtNode::Assignment(assignment.clone()).node_type(), NodeType::AssignmentStmt);
        assert_eq!(assignment.node_type(), NodeType::AssignmentStmt);

        let variable = CheckedVariableNode {
            name: Identifier::new(IdentId(1), location),
            ty: TypeId(0),
            qualifier: TypeQualifier::new(true, location),
            value: ExprId(1),
            scope_id: ScopeId(0),
            comments: vec![],
            location,
        };
        assert_eq!(CheckedStmtNode::Variable(variable.clone()).node_type(), NodeType::VariableStmt);
        assert_eq!(variable.node_type(), NodeType::VariableStmt);

        let ret = CheckedReturnNode {
            ret: Some(ExprId(3)),
            comments: vec![],
            location,
        };
        assert_eq!(CheckedStmtNode::Return(ret.clone()).node_type(), NodeType::ReturnStmt);
        assert_eq!(ret.node_type(), NodeType::ReturnStmt);

        let assert_stmt = CheckedIntrinsicStmtNode::Assert {
            left: ExprId(0),
            message: Some("boom".to_string()),
            comments: vec![Comment::new_line("note".to_string(), location)],
            location,
        };
        assert_eq!(CheckedStmtNode::Intrinsic(assert_stmt.clone()).node_type(), NodeType::IntrinsicStmt);
        assert_eq!(assert_stmt.node_type(), NodeType::IntrinsicStmt);

        let assert_eq_stmt = CheckedIntrinsicStmtNode::AssertEq {
            left: ExprId(0),
            right: ExprId(1),
            message: None,
            comments: vec![],
            location,
        };
        assert_eq!(assert_eq_stmt.node_type(), NodeType::IntrinsicStmt);

        let clear = CheckedIntrinsicStmtNode::ClearEntireTree {
            comments: vec![],
            location,
        };
        assert_eq!(clear.node_type(), NodeType::IntrinsicStmt);

        assert_eq!(CheckedStmtNode::Definition(DefId(0)).node_type(), NodeType::DefinitionStmt);
        assert_eq!(CheckedStmtNode::Expression(ExprId(4)).node_type(), NodeType::ExpressionStmt);
    }

    #[test]
    fn checked_statements_expose_expression_and_definition_handles() {
        let expression: CheckedStmtNode = ExprId(7).into();
        assert_eq!(expression.as_expression(), Some(&ExprId(7)));
        assert!(expression.as_definition().is_none());

        let definition: CheckedStmtNode = DefId(3).into();
        assert_eq!(definition.as_definition(), Some(&DefId(3)));
        assert!(definition.as_expression().is_none());

        let other = CheckedStmtNode::Return(CheckedReturnNode {
            ret: None,
            comments: vec![],
            location: Location::default(),
        });
        assert!(other.as_expression().is_none());
        assert!(other.as_definition().is_none());
    }

    #[test]
    fn checked_statements_expose_handles_through_trait_objects() {
        // Dispatch through `dyn NodeInfo` so the small accessors execute
        // out-of-line instead of being inlined into the caller.
        let expression: CheckedStmtNode = ExprId(7).into();
        let info: &dyn NodeInfo = &expression;
        assert_eq!(info.as_expression(), Some(ExprId(7)));
        assert!(info.as_definition().is_none());

        let definition: CheckedStmtNode = DefId(3).into();
        let info: &dyn NodeInfo = &definition;
        assert_eq!(info.as_definition(), Some(DefId(3)));
        assert!(info.as_expression().is_none());
    }

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn checked_statement_from_expression_node_is_unimplemented() {
        let convert: fn(CheckedExprNode<psy_vm::dpn::ops::sym_felt::SymFeltRef>) -> CheckedStmtNode = CheckedStmtNode::from;
        let _ = convert(CheckedExprNode::Value(crate::CheckedValueNode::Bool(
            psy_vm::dpn::ops::sym_felt::SymFeltRef(1),
            Location::default(),
        )));
    }

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn checked_statement_from_definition_node_is_unimplemented() {
        let convert: fn(CheckedDefinitionNode) -> CheckedStmtNode = CheckedStmtNode::from;
        let _ = convert(CheckedDefinitionNode::Const(crate::CheckedConstNode {
            name: None,
            ty: TypeId(0),
            value: crate::ConstId(0),
            visibility: psy_ast::Visibility::Private,
            scope_id: ScopeId(0),
        }));
    }
}
