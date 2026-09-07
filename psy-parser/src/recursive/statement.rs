//! Statement parsing for the recursive-descent parser.
//! Handles: let, assignment, return, while, for, definitions, expression
//! statements, and intrinsic statements.

use psy_ast::{
    AssignmentNode, AssignmentOperator, Comment, ExprNode, ForNode, ReturnNode, StmtNode, TypeQualifier, UncheckedType, VariableNode, WhileNode,
};
use psy_lexer::Token;
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};

use crate::{
    error::{Error, ExpectedToken, Result},
    recursive::ParsedModuleItem,
};

impl<'src, 'p, F, C> super::ModuleParser<'src, 'p, F, C>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    /// Parse one statement, including its leading comments.
    pub fn parse_statement(&mut self) -> Result<StmtNode> {
        let comments = self.cursor.take_leading_comments();
        self.parse_statement_with_comments(comments)
    }

    pub(crate) fn parse_statement_with_comments(&mut self, comments: Vec<Comment>) -> Result<StmtNode> {
        if matches!(
            self.cursor.peek(),
            Some(Token::IntrinsicAssert | Token::IntrinsicAssertEq | Token::IntrinsicCtxClearEntireTree)
        ) {
            let Some(result) = crate::recursive::intrinsic::try_parse_intrinsic_stmt(self, comments) else {
                return Err(self.unexpected(vec![ExpectedToken::Statement]));
            };
            return result;
        }

        match self.cursor.peek() {
            Some(Token::KeywordLet) => self.parse_variable_statement(comments),
            Some(Token::KeywordReturn) => self.parse_return_statement(comments),
            Some(Token::KeywordWhile) => self.parse_while_statement(comments),
            Some(Token::KeywordFor) => self.parse_for_statement(comments),
            Some(_) if self.starts_definition_statement() => match crate::recursive::item::parse_module_item(self, comments)? {
                ParsedModuleItem::Definition(definition) => Ok(StmtNode::Definition(self.alloc_def(definition))),
                ParsedModuleItem::ExternalModule(_, _, _) | ParsedModuleItem::InlineModule(_) => {
                    Err(self.unexpected(vec![ExpectedToken::Statement]))
                }
            },
            Some(_) => {
                let lhs = self.parse_expression_struct_allowed()?;
                if self.cursor.at(&Token::Assign) || Self::assignment_operator(self.cursor.peek()).is_some() {
                    self.parse_assignment_after_lhs(comments, lhs)
                } else {
                    self.parse_expression_statement_after_expr(comments, lhs)
                }
            }
            None => Err(Error::UnexpectedEof {
                expected: vec![ExpectedToken::Statement],
                location: self.cursor.eof_location(),
            }),
        }
    }

    /// Parse `let mut name: Type = value;`.
    pub fn parse_variable_statement(&mut self, comments: Vec<Comment>) -> Result<StmtNode> {
        let start = self.cursor.expect(&Token::KeywordLet)?.0;
        let qualifier_start = self.cursor.peek_start();
        let is_mutable = self.cursor.eat(&Token::KeywordMut).is_some();
        let qualifier_end = self.cursor.peek_start();
        let name = self.parse_identifier()?;
        let ty = if self.cursor.eat(&Token::Colon).is_some() {
            self.parse_path_ty()?
        } else {
            UncheckedType::Unknown
        };
        self.cursor.expect(&Token::Assign)?;
        let value = self.parse_expression_struct_allowed()?;
        let value = self.alloc_expr(value);
        let end = self.cursor.expect(&Token::Semicolon)?.1;

        Ok(StmtNode::Variable(VariableNode {
            name,
            ty,
            qualifier: TypeQualifier::new(is_mutable, self.location(qualifier_start, qualifier_end)),
            value,
            comments,
            location: self.location(start, end),
        }))
    }

    /// Parse an assignment statement from its leading comments.
    pub fn parse_assignment_statement(&mut self, comments: Vec<Comment>) -> Result<StmtNode> {
        let lhs = self.parse_expression_struct_allowed()?;
        self.parse_assignment_after_lhs(comments, lhs)
    }

    pub(crate) fn parse_assignment_after_lhs(&mut self, comments: Vec<Comment>, lhs: ExprNode<F>) -> Result<StmtNode> {
        let start = comments
            .first()
            .map(|comment| comment.location().start)
            .unwrap_or_else(|| self.expression_location(&lhs).start);
        let operator = match self.cursor.peek() {
            Some(Token::Assign) => AssignmentOperator::Eq,
            token => Self::assignment_operator(token)
                .ok_or_else(|| self.unexpected(vec![ExpectedToken::Symbol("assignment operator".into())]))?,
        };
        self.cursor.advance();
        let value = self.parse_expression_struct_allowed()?;
        let target = self.alloc_expr(lhs);
        let value = self.alloc_expr(value);
        let end = self.cursor.expect(&Token::Semicolon)?.1;
        Ok(StmtNode::Assignment(AssignmentNode::new(
            target,
            operator,
            value,
            comments,
            self.location(start, end),
        )))
    }

    /// Parse `return;` or `return value;`.
    pub fn parse_return_statement(&mut self, comments: Vec<Comment>) -> Result<StmtNode> {
        let start = self.cursor.expect(&Token::KeywordReturn)?.0;
        let expr_id = if self.cursor.at(&Token::Semicolon) {
            None
        } else {
            let expr = self.parse_expression_struct_allowed()?;
            Some(self.alloc_expr(expr))
        };
        let end = self.cursor.expect(&Token::Semicolon)?.1;
        Ok(StmtNode::Return(ReturnNode {
            expr_id,
            comments,
            location: self.location(start, end),
        }))
    }

    /// Parse `while predicate { ... }`.
    pub fn parse_while_statement(&mut self, comments: Vec<Comment>) -> Result<StmtNode> {
        let start = self.cursor.expect(&Token::KeywordWhile)?.0;
        let predicate = self.parse_expression_no_struct()?;
        let predicate = self.alloc_expr(predicate);
        let body = self.parse_block_expression()?;
        let end = self.expression_location(&body).end;
        let body = self.alloc_expr(body);
        Ok(StmtNode::While(WhileNode {
            predicate,
            body,
            comments,
            location: self.location(start, end),
        }))
    }

    /// Parse `for variable in start..end { ... }`.
    pub fn parse_for_statement(&mut self, comments: Vec<Comment>) -> Result<StmtNode> {
        let start = self.cursor.expect(&Token::KeywordFor)?.0;
        let variable = self.parse_identifier()?;
        self.cursor.expect(&Token::KeywordIn)?;
        let range_start = self.parse_expression_no_struct()?;
        self.cursor.expect(&Token::DoubleDot)?;
        let range_end = self.parse_expression_no_struct()?;
        let range_start = self.alloc_expr(range_start);
        let range_end = self.alloc_expr(range_end);
        let body = self.parse_block_expression()?;
        let end = self.expression_location(&body).end;
        let body = self.alloc_expr(body);
        Ok(StmtNode::For(ForNode {
            variable,
            start: range_start,
            end: range_end,
            body,
            comments,
            location: self.location(start, end),
        }))
    }

    /// Parse `expression;`.
    pub fn parse_expression_statement(&mut self, comments: Vec<Comment>) -> Result<StmtNode> {
        let expr = self.parse_expression_struct_allowed()?;
        self.parse_expression_statement_after_expr(comments, expr)
    }

    fn parse_expression_statement_after_expr(&mut self, _comments: Vec<Comment>, expr: ExprNode<F>) -> Result<StmtNode> {
        self.cursor.expect(&Token::Semicolon)?;
        Ok(StmtNode::Expression(self.alloc_expr(expr)))
    }

    pub(crate) fn assignment_operator(token: Option<&Token<'src>>) -> Option<AssignmentOperator> {
        match token? {
            Token::OperatorAddAssign => Some(AssignmentOperator::AddAssign),
            Token::OperatorSubAssign => Some(AssignmentOperator::SubAssign),
            Token::OperatorMulAssign => Some(AssignmentOperator::MulAssign),
            Token::OperatorDivAssign => Some(AssignmentOperator::DivAssign),
            Token::OperatorModAssign => Some(AssignmentOperator::ModAssign),
            Token::OperatorBitAndAssign => Some(AssignmentOperator::BitAndAssign),
            Token::OperatorBitOrAssign => Some(AssignmentOperator::BitOrAssign),
            Token::OperatorBitXorAssign => Some(AssignmentOperator::BitXorAssign),
            Token::OperatorBitShlAssign => Some(AssignmentOperator::BitShlAssign),
            Token::OperatorBitShrAssign => Some(AssignmentOperator::BitShrAssign),
            _ => None,
        }
    }

    pub(crate) fn starts_unambiguous_statement(&self) -> bool {
        matches!(
            self.cursor.peek(),
            Some(Token::KeywordLet)
                | Some(Token::KeywordReturn)
                | Some(Token::KeywordWhile)
                | Some(Token::KeywordFor)
                | Some(Token::IntrinsicAssert)
                | Some(Token::IntrinsicAssertEq)
                | Some(Token::IntrinsicCtxClearEntireTree)
        ) || self.starts_definition_statement()
    }

    fn starts_definition_statement(&self) -> bool {
        matches!(
            self.cursor.peek_past_comments(),
            Some(Token::KeywordConst)
                | Some(Token::KeywordFn)
                | Some(Token::KeywordStruct)
                | Some(Token::KeywordEnum)
                | Some(Token::KeywordImpl)
                | Some(Token::KeywordTrait)
                | Some(Token::KeywordType)
                | Some(Token::KeywordUse)
                | Some(Token::KeywordExtern)
                | Some(Token::KeywordPub)
                | Some(Token::Pound)
        )
    }

}

#[cfg(test)]
mod tests {
    use psy_ast::{Identifier, IdentId, Location, Program, StmtNode, Visibility};
    use psy_common::FileId;
    use psy_vm::dpn::ops::exec_context::QExecContext;
    use psy_vm::dpn::ops::sym_felt::SymFeltRef;

    use super::super::{ModuleParser, ParseModuleInput};

    fn parser_for<'src, 'p>(
        src: &'src str,
        program: &'p mut Program<SymFeltRef>,
        ctx: &'p mut QExecContext,
    ) -> ModuleParser<'src, 'p, SymFeltRef, QExecContext> {
        ModuleParser::new(
            ParseModuleInput {
                source: src,
                file_id: FileId(0),
                module_name: Identifier::new(IdentId::STD, Location::default()),
                visibility: Visibility::Public,
            },
            program,
            ctx,
        )
        .expect("construct module parser")
    }

    fn parse_with(src: &str, f: impl for<'a, 'b> FnOnce(&mut ModuleParser<'a, 'b, SymFeltRef, QExecContext>) -> psy_ast::StmtNode) -> StmtNode {
        let mut program = Program::new();
        let mut ctx = QExecContext::new();
        let mut parser = parser_for(src, &mut program, &mut ctx);
        f(&mut parser)
    }

    /// The public statement entry points are not wired through
    /// `parse_module` (block parsing calls the `_with_comments` variants
    /// directly), so exercise them here.
    #[test]
    fn statement_entry_points_parse_each_statement_shape() {
        let stmt = parse_with("// lead\nreturn 1;", |p| p.parse_statement().expect("parse_statement"));
        assert!(matches!(stmt, StmtNode::Return(_)), "got {stmt:?}");

        let stmt = parse_with("x = 2;", |p| {
            p.parse_assignment_statement(Vec::new()).expect("plain assignment statement")
        });
        assert!(matches!(stmt, StmtNode::Assignment(_)), "got {stmt:?}");

        let stmt = parse_with("x += 2;", |p| {
            p.parse_assignment_statement(Vec::new()).expect("compound assignment statement")
        });
        assert!(matches!(stmt, StmtNode::Assignment(_)), "got {stmt:?}");

        let stmt = parse_with("1 + 2;", |p| {
            p.parse_expression_statement(Vec::new()).expect("expression statement")
        });
        assert!(matches!(stmt, StmtNode::Expression(_)), "got {stmt:?}");
    }
}
