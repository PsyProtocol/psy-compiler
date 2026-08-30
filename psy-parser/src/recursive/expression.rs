//! Expression parsing for the recursive-descent parser.
//!
//! The parser uses precedence climbing for binary operators and handles
//! postfix operators in a single left-to-right chain.

use indexmap::IndexMap;
use psy_ast::{
    BinaryNode, BinaryOperator, BlockExprNode, CallNode, Case, CastNode, ConstValue, ExprNode, FunctionParameter, IdentId, Identifier, IfExprNode,
    IndexAccessNode, LambdaFunctionNode, Location, MatchArm, MatchNode, MatchPattern, MemberAccessNode, MemberCallNode, PathNode, TupleAccessNode,
    TupleExprNode, TypeQualifier, UnaryNode, UnaryOperator, UncheckedType, ValueNode,
};
use psy_lexer::Token;
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};

use crate::error::{Error, ExpectedToken, Result};

impl<'src, 'p, F, C> super::ModuleParser<'src, 'p, F, C>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    /// Parse an expression at the lowest precedence.
    pub fn parse_expression(&mut self) -> Result<ExprNode<F>> {
        self.parse_precedence(0)
    }

    /// Parse an expression in a context where `Type { ... }` struct literals
    /// are not recognized (control-flow predicates/ranges, where `{` would
    /// otherwise be stolen as a struct body).
    pub(super) fn parse_expression_no_struct(&mut self) -> Result<ExprNode<F>> {
        let saved = std::mem::replace(&mut self.struct_literals_allowed, false);
        let result = self.parse_expression();
        self.struct_literals_allowed = saved;
        result
    }

    /// Parse an expression in a context where struct literals are
    /// unambiguously allowed (inside delimiters, after `=`/`return`/`=>`).
    pub(super) fn parse_expression_struct_allowed(&mut self) -> Result<ExprNode<F>> {
        let saved = std::mem::replace(&mut self.struct_literals_allowed, true);
        let result = self.parse_expression();
        self.struct_literals_allowed = saved;
        result
    }

    /// Parse a complete postfix chain rooted at `lhs`.
    pub fn parse_postfix_chain(&mut self, mut lhs: ExprNode<F>) -> Result<ExprNode<F>> {
        let start = self.expression_location(&lhs).start;

        loop {
            match self.cursor.peek() {
                Some(Token::Dot) => {
                    self.cursor.advance();
                    let target = self.alloc_expr(lhs);
                    lhs = match self.cursor.peek() {
                        Some(Token::U64(index)) => {
                            let index = usize::try_from(*index).map_err(|_| Error::UnsupportedSyntax {
                                feature: "tuple index does not fit usize".into(),
                                location: self.cursor.current_location(),
                            })?;
                            let end = self.cursor.peek_end();
                            self.cursor.advance();
                            ExprNode::TupleAccess(TupleAccessNode {
                                target,
                                index,
                                location: self.location(start, end),
                            })
                        }
                        Some(Token::Ident(_)) => {
                            let field = self.parse_identifier()?;
                            ExprNode::MemberAccess(MemberAccessNode {
                                target,
                                field,
                                generic_parameters: Vec::new(),
                                location: self.location(start, field.location.end),
                            })
                        }
                        _ => return Err(self.unexpected(vec![ExpectedToken::Ident, ExpectedToken::Literal])),
                    };
                }
                Some(Token::LBracket) => {
                    self.cursor.advance();
                    let index = self.parse_expression_struct_allowed()?;
                    let index = self.alloc_expr(index);
                    let end = self.cursor.expect(&Token::RBracket)?.1;
                    lhs = ExprNode::IndexAccess(IndexAccessNode {
                        target: self.alloc_expr(lhs),
                        index,
                        location: self.location(start, end),
                    });
                }
                Some(Token::KeywordAs) => {
                    self.cursor.advance();
                    let target_type = self.parse_const_type()?;
                    let end = target_type.location().end;
                    lhs = ExprNode::Cast(CastNode::new(self.alloc_expr(lhs), target_type, self.location(start, end)));
                }
                Some(Token::LParen) => {
                    lhs = self.parse_call_postfix(lhs, Vec::new(), start)?;
                }
                Some(Token::Pound) if matches!(self.cursor.lookahead(1), Some(Token::OperatorLt)) => {
                    return Err(Error::UnsupportedSyntax {
                        feature: "obsolete `#<...>` monomorphization syntax; use `::<...>` instead".into(),
                        location: self.cursor.current_location(),
                    });
                }
                Some(Token::DoubleColon) if matches!(self.cursor.lookahead(1), Some(Token::OperatorLt)) => {
                    let generic_parameters = self.parse_turbofish_call_args()?;
                    if self.cursor.at(&Token::LParen) {
                        lhs = self.parse_call_postfix(lhs, generic_parameters, start)?;
                    } else if let ExprNode::MemberAccess(mut member) = lhs {
                        // `object.method::<T>` with no call parens: a bare
                        // monomorphized method reference. Record the generic
                        // args on the member access and continue the chain.
                        member.generic_parameters = generic_parameters;
                        member.location = self.location(start, self.cursor.peek_start());
                        lhs = ExprNode::MemberAccess(member);
                    } else {
                        return Err(self.unexpected(vec![ExpectedToken::Symbol("(".into())]));
                    }
                }
                _ => break,
            }
        }

        Ok(lhs)
    }

    /// Parse a primary expression.
    pub fn parse_primary(&mut self) -> Result<ExprNode<F>> {
        if let Some(result) = crate::recursive::intrinsic::try_parse_intrinsic_expr(self) {
            return result;
        }

        match self.cursor.peek() {
            Some(Token::U64(_))
            | Some(Token::U32(_))
            | Some(Token::Bool(_))
            | Some(Token::LBracket)
            | Some(Token::LParen) => self.parse_value_expression(),
            Some(Token::LBrace) => self.parse_block_expression(),
            Some(Token::KeywordIf) => self.parse_if_expression(),
            Some(Token::KeywordMatch) => self.parse_match_expression(),
            Some(Token::OperatorOr) | Some(Token::OperatorBitOr) => self.parse_lambda_expression(),
            Some(Token::Ident(_))
            | Some(Token::KeywordSelf)
            | Some(Token::KeywordSuper)
            | Some(Token::KeywordCrate)
            | Some(Token::TypeFelt)
            | Some(Token::TypeBool)
            | Some(Token::TypeU32)
            | Some(Token::TypeSelf)
            | Some(Token::OperatorLt) => self.parse_path_expression(),
            Some(_) => Err(self.unexpected(vec![ExpectedToken::Expression])),
            None => Err(self.unexpected_eof(vec![ExpectedToken::Expression])),
        }
    }

    /// Parse `{ statements... optional_tail }`.
    pub fn parse_block_expression(&mut self) -> Result<ExprNode<F>> {
        let start = self.cursor.expect(&Token::LBrace)?.0;
        let mut stmts = Vec::new();
        let mut tail = None;
        let mut tail_comments = Vec::new();

        loop {
            let comments = self.cursor.take_leading_comments();
            if self.cursor.at(&Token::RBrace) {
                tail_comments = comments;
                let end = self.cursor.expect(&Token::RBrace)?.1;
                return Ok(ExprNode::BlockExpr(BlockExprNode {
                    stmts,
                    expr: tail,
                    expr_comments: tail_comments,
                    location: self.location(start, end),
                }));
            }
            if self.cursor.at_end() {
                return Err(self.unexpected_eof(vec![ExpectedToken::Symbol("}".into())]));
            }

            if self.starts_unambiguous_statement() {
                let stmt = self.parse_statement_with_comments(comments)?;
                stmts.push(self.alloc_stmt(stmt));
                continue;
            }

            let expr = self.parse_expression_struct_allowed()?;
            if self.cursor.at(&Token::Assign) || Self::assignment_operator(self.cursor.peek()).is_some() {
                let stmt = self.parse_assignment_after_lhs(comments, expr)?;
                stmts.push(self.alloc_stmt(stmt));
            } else if self.cursor.eat(&Token::Semicolon).is_some() {
                let expr = self.alloc_expr(expr);
                stmts.push(self.alloc_stmt(psy_ast::StmtNode::Expression(expr)));
            } else if self.cursor.at(&Token::RBrace) {
                tail = Some(self.alloc_expr(expr));
                tail_comments = comments;
                let end = self.cursor.expect(&Token::RBrace)?.1;
                return Ok(ExprNode::BlockExpr(BlockExprNode {
                    stmts,
                    expr: tail,
                    expr_comments: tail_comments,
                    location: self.location(start, end),
                }));
            } else {
                return Err(self.unexpected(vec![ExpectedToken::Symbol(";".into()), ExpectedToken::Symbol("}".into())]));
            }
        }
    }

    /// Parse an if / else-if / else expression.
    pub fn parse_if_expression(&mut self) -> Result<ExprNode<F>> {
        let start = self.cursor.expect(&Token::KeywordIf)?.0;
        let predicate = self.parse_expression_no_struct()?;
        let predicate = self.alloc_expr(predicate);
        let body = self.parse_block_expression()?;
        let body_end = self.expression_location(&body).end;
        let body = self.alloc_expr(body);
        let if_branch = Case::new(predicate, body, self.location(start, body_end));
        let mut elseif_branches = Vec::new();
        let mut else_branch = None;

        while let Some((else_start, _)) = self.cursor.eat(&Token::KeywordElse) {
            if self.cursor.eat(&Token::KeywordIf).is_some() {
                let predicate = self.parse_expression_no_struct()?;
                let predicate = self.alloc_expr(predicate);
                let body = self.parse_block_expression()?;
                let branch_end = self.expression_location(&body).end;
                let body = self.alloc_expr(body);
                elseif_branches.push(Case::new(predicate, body, self.location(else_start, branch_end)));
            } else {
                let body = self.parse_block_expression()?;
                else_branch = Some(self.alloc_expr(body));
                break;
            }
        }

        let end = self.cursor.peek_start();
        Ok(ExprNode::IfExpr(IfExprNode {
            if_branch,
            elseif_branches,
            else_branch,
            location: self.location(start, end),
        }))
    }

    /// Parse a match expression.
    pub fn parse_match_expression(&mut self) -> Result<ExprNode<F>> {
        let start = self.cursor.expect(&Token::KeywordMatch)?.0;
        let scrutinee = self.parse_expression_no_struct()?;
        let scrutinee = self.alloc_expr(scrutinee);
        self.cursor.expect(&Token::LBrace)?;
        let mut arms = Vec::new();

        while !self.cursor.at(&Token::RBrace) {
            self.cursor.take_leading_comments();
            if self.cursor.at(&Token::RBrace) {
                break;
            }
            let arm_start = self.cursor.peek_start();
            let pattern = if let Some((pattern_start, pattern_end)) = self.cursor.eat(&Token::Placeholder) {
                MatchPattern::PlaceHolder(self.location(pattern_start, pattern_end))
            } else {
                let pattern_start = self.cursor.peek_start();
                let pattern_expr = self.parse_expression_struct_allowed()?;
                let pattern_end = self.cursor.peek_start();
                MatchPattern::Value(self.alloc_expr(pattern_expr), self.location(pattern_start, pattern_end))
            };
            self.cursor.expect(&Token::FatArrow)?;
            let body = self.parse_expression_struct_allowed()?;
            let body_end = self.cursor.peek_start();
            arms.push(MatchArm {
                pattern,
                body: self.alloc_expr(body),
                location: self.location(arm_start, body_end),
            });
            if self.cursor.eat(&Token::Comma).is_none() {
                break;
            }
        }

        let end = self.cursor.expect(&Token::RBrace)?.1;
        Ok(ExprNode::Match(MatchNode {
            scrutinee,
            arms,
            location: self.location(start, end),
        }))
    }

    /// Parse `|| -> T { ... }` or `|params| -> T { ... }`.
    pub fn parse_lambda_expression(&mut self) -> Result<ExprNode<F>> {
        let start = self.cursor.peek_start();
        let mut parameters = Vec::new();

        if self.cursor.eat(&Token::OperatorOr).is_none() {
            self.cursor.expect(&Token::OperatorBitOr)?;
            if !self.cursor.at(&Token::OperatorBitOr) {
                loop {
                    parameters.push(self.parse_lambda_parameter()?);
                    if self.cursor.eat(&Token::Comma).is_none() {
                        break;
                    }
                    if self.cursor.at(&Token::OperatorBitOr) {
                        break;
                    }
                }
            }
            self.cursor.expect(&Token::OperatorBitOr)?;
        }

        let return_type = self.parse_function_return()?;
        let body = self.parse_block_expression()?;
        let end = self.expression_location(&body).end;
        let body = self.alloc_expr(body);
        Ok(ExprNode::LambdaFunction(LambdaFunctionNode {
            parameters,
            body,
            return_type,
            location: self.location(start, end),
        }))
    }

    /// Parse literals, arrays, and tuples/parentheses.
    pub fn parse_value_expression(&mut self) -> Result<ExprNode<F>> {
        let start = self.cursor.peek_start();
        match self.cursor.peek() {
            Some(Token::U64(value)) => {
                let value = *value;
                let end = self.cursor.peek_end();
                self.cursor.advance();
                let literal = self.ctx().op_const(value);
                Ok(ExprNode::Value(ValueNode::Felt(literal, self.location(start, end))))
            }
            Some(Token::U32(value)) => {
                let value = *value;
                let end = self.cursor.peek_end();
                self.cursor.advance();
                let literal = self.ctx().op_const_u32(value);
                Ok(ExprNode::Value(ValueNode::U32(literal, self.location(start, end))))
            }
            Some(Token::Bool(value)) => {
                let value = *value;
                let end = self.cursor.peek_end();
                self.cursor.advance();
                let literal = if value { self.ctx().op_true() } else { self.ctx().op_false() };
                Ok(ExprNode::Value(ValueNode::Bool(literal, self.location(start, end))))
            }
            Some(Token::LBracket) => self.parse_array_expression(),
            Some(Token::LParen) => self.parse_tuple_or_parentheses(),
            Some(_) => Err(self.unexpected(vec![ExpectedToken::Literal])),
            None => Err(self.unexpected_eof(vec![ExpectedToken::Literal])),
        }
    }

    /// Parse a path expression, including path-based struct literals
    /// `Type { ... }` (with optional qualified/generic paths) and turbofish
    /// generic path segments.
    ///
    /// A struct literal is recognized when a path is immediately followed by
    /// `{`. Bare `<` on the target is only treated as generic args when a
    /// complete `<...>` is followed by `{`, so `a < b > c` still parses as a
    /// comparison. `::<...>` is the turbofish form for generic path segments
    /// and (in the postfix chain) call monomorphization.
    pub fn parse_path_expression(&mut self) -> Result<ExprNode<F>> {
        let start = self.cursor.peek_start();
        let mut components: Vec<UncheckedType> = Vec::new();
        let target;
        let mut struct_literal = false;

        loop {
            let component = self.parse_path_component()?;

            // `Type { ... }` struct literal with no generics on the target.
            // Only recognized when struct literals are allowed in the current
            // expression context (disabled in control-flow predicates/ranges
            // where `{` introduces the construct's body).
            if self.cursor.at(&Token::LBrace) {
                target = component;
                struct_literal = self.struct_literals_allowed;
                break;
            }

            // Bare generic args on the target: `Foo<T> { ... }` (struct literal)
            // vs `Foo < T ...` (comparison). Only claim the `<...>` when it
            // closes cleanly and is followed by `{`, and struct literals are
            // allowed.
            if self.cursor.at(&Token::OperatorLt) {
                if self.struct_literals_allowed {
                    if let Some(seg) = self.probe_struct_generic_target(&component)? {
                        target = seg;
                        struct_literal = true;
                        break;
                    }
                }
                target = component;
                break;
            }

            let before_colon = self.cursor.checkpoint();
            if self.cursor.eat(&Token::DoubleColon).is_none() {
                target = component;
                break;
            }

            // `::<...>` after a path segment is ambiguous: it may be a
            // turbofish (call `callee::<T>(...)` or struct literal
            // `Type::<T> { ... }`) or a trait-cast path segment
            // (`mod::<Type>::method`, `<Type as Trait>::method`). Probe with
            // backtracking: parse the generics, then decide by the follow.
            if self.cursor.at(&Token::OperatorLt) {
                let turbo_cp = self.cursor.checkpoint();
                self.cursor.advance(); // consume `<`
                let generics = self.parse_generic_args()?;

                // Call monomorphization `callee::<T>(...)` is handled by the
                // postfix chain; restore the `::<...>` so it sees it.
                if self.cursor.at(&Token::LParen) {
                    self.cursor.rewind(before_colon);
                    target = component;
                    break;
                }

                // Turbofish struct literal: `Type::<T> { ... }`.
                if self.struct_literals_allowed && self.cursor.at(&Token::LBrace) {
                    let seg = self.apply_generic_args(&component, generics)?;
                    target = seg;
                    struct_literal = true;
                    break;
                }

                // Not a turbofish: `<...>` is a trait-cast path segment
                // (e.g. `mod::<Type>::method`). Rewind and let the loop's
                // `parse_path_component` handle the `<...>` via the standard
                // trait-cast path.
                if self.cursor.at(&Token::DoubleColon) {
                    self.cursor.rewind(turbo_cp);
                    components.push(component);
                    continue;
                }

                // `foo::<T>` followed by an expression boundary (operator,
                // `;`, ...): a monomorphized function reference used as a
                // value, e.g. the LHS of `foo::<T>>=limit`. Attach the
                // generic args to this final segment and finish the path.
                let seg = self.apply_generic_args(&component, generics)?;
                target = seg;
                break;
            }

            components.push(component);
        }

        let end = self.cursor.peek_start();
        let (root, segments) = Self::split_path_components(components);

        if struct_literal {
            let path_ty = if root.is_none() && segments.is_empty() {
                target
            } else {
                UncheckedType::Path(Box::new(PathNode {
                    root,
                    segments,
                    target,
                    is_ty: true,
                    location: self.location(start, end),
                }))
            };
            let name = self.alloc_expr(ExprNode::Path(PathNode::from_target_ty(path_ty)));
            return self.parse_struct_fields_after_name(name, start);
        }

        Ok(ExprNode::Path(PathNode {
            root,
            segments,
            target,
            is_ty: false,
            location: self.location(start, end),
        }))
    }

    /// Speculatively parse `<...>` generic args on `component` and return the
    /// resulting generic type only when a struct-literal `{` follows. On
    /// failure the cursor is rewound to before the `<`; on success the cursor
    /// is left at the `{`.
    fn probe_struct_generic_target(&mut self, component: &UncheckedType) -> Result<Option<UncheckedType>> {
        let generic_start = self.cursor.checkpoint();
        // The caller verified `<`; consume it so `parse_generic_args` (which
        // expects to run after `<`) can parse the argument list and closer.
        if self.cursor.eat(&Token::OperatorLt).is_none() {
            return Ok(None);
        }
        let generic_args = match self.parse_generic_args() {
            Ok(args) => args,
            // An UnsupportedSyntax error (e.g. empty generics `<>`) should
            // only propagate when this is genuinely a struct-literal
            // context — i.e. the `<>` is followed by `{`. Otherwise the `<`
            // belongs to a comparison expression (e.g. `a <> b`) and the
            // probe should fail silently, letting precedence parsing handle it.
            Err(error @ Error::UnsupportedSyntax { .. }) => {
                self.cursor.rewind(generic_start);
                self.cursor.eat(&Token::OperatorLt);
                let empty_args_precede_struct_body = self.cursor.at(&Token::OperatorGt)
                    && matches!(self.cursor.lookahead(1), Some(Token::LBrace));
                self.cursor.rewind(generic_start);
                if empty_args_precede_struct_body {
                    return Err(error);
                }
                return Ok(None);
            }
            // Parse failure: not a struct literal, rewind and fall through.
            Err(_) => {
                self.cursor.rewind(generic_start);
                return Ok(None);
            }
        };
        if !self.cursor.at(&Token::LBrace) {
            self.cursor.rewind(generic_start);
            return Ok(None);
        }
        match self.apply_generic_args(component, generic_args) {
            Ok(target) => Ok(Some(target)),
            Err(_) => {
                self.cursor.rewind(generic_start);
                Ok(None)
            }
        }
    }

    /// Parse an array value (`[a, b]` or `[a; N]`).
    pub fn parse_array_expression(&mut self) -> Result<ExprNode<F>> {
        let start = self.cursor.expect(&Token::LBracket)?.0;
        if self.cursor.at(&Token::RBracket) {
            let end = self.cursor.expect(&Token::RBracket)?.1;
            return Ok(ExprNode::Value(ValueNode::Array(
                ConstValue::Felt(0),
                Vec::new(),
                self.location(start, end),
            )));
        }

        let first = self.parse_expression_struct_allowed()?;
        if self.cursor.eat(&Token::Semicolon).is_some() {
            let count = match self.cursor.peek() {
                Some(Token::U64(count)) => {
                    let count = *count;
                    self.cursor.advance();
                    count
                }
                Some(_) => return Err(self.unexpected(vec![ExpectedToken::Literal])),
                None => return Err(self.unexpected_eof(vec![ExpectedToken::Literal])),
            };
            let end = self.cursor.expect(&Token::RBracket)?.1;
            let first = self.alloc_expr(first);
            return Ok(ExprNode::Value(ValueNode::ArrayRepeat(
                first,
                ConstValue::Felt(count),
                self.location(start, end),
            )));
        }

        let mut elements = vec![self.alloc_expr(first)];
        while self.cursor.eat(&Token::Comma).is_some() {
            if self.cursor.at(&Token::RBracket) {
                break;
            }
            let element = self.parse_expression_struct_allowed()?;
            elements.push(self.alloc_expr(element));
        }
        let end = self.cursor.expect(&Token::RBracket)?.1;
        let count = u64::try_from(elements.len()).map_err(|_| Error::UnsupportedSyntax {
            feature: "array literal contains more than u64::MAX elements".into(),
            location: self.location(start, end),
        })?;
        Ok(ExprNode::Value(ValueNode::Array(
            ConstValue::Felt(count),
            elements,
            self.location(start, end),
        )))
    }

    fn parse_precedence(&mut self, min_precedence: u8) -> Result<ExprNode<F>> {
        // Every parenthesized/nested operand recurses through here, so this
        // is the depth chokepoint: unbounded recursion previously aborted
        // with a stack overflow (~150 parens on a 2 MB stack, ~400 on
        // wasm32) instead of returning an error (M7). The limit is well
        // above any legitimate expression.
        const MAX_EXPRESSION_DEPTH: u32 = 128;
        if self.expression_depth >= MAX_EXPRESSION_DEPTH {
            return Err(Error::UnsupportedSyntax {
                feature: "expression nesting too deep".into(),
                location: self.cursor.current_location(),
            });
        }
        self.expression_depth += 1;
        let result = self.parse_precedence_inner(min_precedence);
        self.expression_depth -= 1;
        result
    }

    fn parse_precedence_inner(&mut self, min_precedence: u8) -> Result<ExprNode<F>> {
        if min_precedence == 0 && matches!(self.cursor.peek(), Some(Token::OperatorOr) | Some(Token::OperatorBitOr)) {
            return self.parse_lambda_expression();
        }

        let mut lhs = self.parse_prefix()?;

        loop {
            // Comments between an operand and a binary operator (e.g.
            // `a >= /* note */ b`) carry no semantics; drop them so the
            // operator check sees the real next token.
            if matches!(self.cursor.peek(), Some(Token::LineComment(_) | Token::BlockComment(_))) {
                self.cursor.take_leading_comments();
            }
            let Some((operator, precedence, right_associative)) = Self::binary_operator(self.cursor.peek()) else {
                break;
            };
            if precedence < min_precedence {
                break;
            }

            self.cursor.advance();
            let rhs = self.parse_precedence(if right_associative { precedence } else { precedence + 1 })?;
            let start = self.expression_location(&lhs).start;
            let end = self.expression_location(&rhs).end;
            lhs = ExprNode::Binary(BinaryNode::new(
                self.alloc_expr(lhs),
                operator,
                self.alloc_expr(rhs),
                self.location(start, end),
            ));
        }

        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> Result<ExprNode<F>> {
        // An operand may be preceded by comments after an operator or open
        // delimiter (`a >= /* note */ b`); drop them before classifying.
        self.cursor.take_leading_comments();
        let start = self.cursor.peek_start();
        let operator = match self.cursor.peek() {
            Some(Token::OperatorNot) => Some(UnaryOperator::Not),
            Some(Token::OperatorSub) => Some(UnaryOperator::Neg),
            _ => None,
        };

        if let Some(operator) = operator {
            self.cursor.advance();
            let rhs = self.parse_prefix()?;
            let end = self.expression_location(&rhs).end;
            return Ok(ExprNode::Unary(UnaryNode {
                operator,
                rhs: self.alloc_expr(rhs),
                location: self.location(start, end),
            }));
        }

        let primary = self.parse_primary()?;
        self.parse_postfix_chain(primary)
    }

    fn parse_struct_fields_after_name(&mut self, name: psy_ast::ExprId, start: usize) -> Result<ExprNode<F>> {
        self.cursor.expect(&Token::LBrace)?;
        let mut fields = IndexMap::new();
        while !self.cursor.at(&Token::RBrace) {
            self.cursor.take_leading_comments();
            if self.cursor.at(&Token::RBrace) {
                break;
            }
            let field = self.parse_identifier()?;
            let value = if self.cursor.eat(&Token::Colon).is_some() {
                let value = self.parse_expression_struct_allowed()?;
                self.alloc_expr(value)
            } else {
                self.alloc_expr(ExprNode::Path(PathNode::from_target(UncheckedType::Basic(field))))
            };
            fields.insert(field, value);
            if self.cursor.eat(&Token::Comma).is_none() {
                break;
            }
        }
        let end = self.cursor.expect(&Token::RBrace)?.1;
        Ok(ExprNode::Value(ValueNode::Struct(name, Vec::new(), fields, self.location(start, end))))
    }

    fn parse_tuple_or_parentheses(&mut self) -> Result<ExprNode<F>> {
        let start = self.cursor.expect(&Token::LParen)?.0;
        if self.cursor.at(&Token::RParen) {
            return Err(self.unexpected(vec![ExpectedToken::Expression]));
        }
        let first = self.parse_expression_struct_allowed()?;
        if self.cursor.eat(&Token::Comma).is_none() {
            self.cursor.expect(&Token::RParen)?;
            return Ok(ExprNode::Parentheses(self.alloc_expr(first)));
        }

        let mut elements = vec![self.alloc_expr(first)];
        while !self.cursor.at(&Token::RParen) {
            let element = self.parse_expression_struct_allowed()?;
            elements.push(self.alloc_expr(element));
            if self.cursor.eat(&Token::Comma).is_none() {
                break;
            }
        }
        let end = self.cursor.expect(&Token::RParen)?.1;
        Ok(ExprNode::Tuple(TupleExprNode {
            elements,
            location: self.location(start, end),
        }))
    }

    fn parse_call_postfix(&mut self, callee: ExprNode<F>, generic_parameters: Vec<UncheckedType>, start: usize) -> Result<ExprNode<F>> {
        self.cursor.expect(&Token::LParen)?;
        let mut args = Vec::new();
        while !self.cursor.at(&Token::RParen) {
            let arg = self.parse_expression_struct_allowed()?;
            args.push(self.alloc_expr(arg));
            if self.cursor.eat(&Token::Comma).is_none() {
                break;
            }
            if self.cursor.at(&Token::RParen) {
                break;
            }
        }
        let end = self.cursor.expect(&Token::RParen)?.1;
        let receiver = match &callee {
            ExprNode::MemberAccess(node) => Some(node.target),
            _ => None,
        };
        let callee = self.alloc_expr(callee);
        let location = self.location(start, end);
        Ok(if let Some(receiver) = receiver {
            ExprNode::MemberCall(MemberCallNode {
                receiver,
                callee,
                generic_parameters,
                args,
                location,
            })
        } else {
            ExprNode::Call(CallNode {
                callee,
                generic_parameters,
                args,
                location,
            })
        })
    }

    /// Parse `::<...>` turbofish call monomorphization arguments, consuming
    /// the closing `>` (with contextual `>>` splitting). Rejects empty lists.
    fn parse_turbofish_call_args(&mut self) -> Result<Vec<UncheckedType>> {
        self.cursor.expect(&Token::DoubleColon)?;
        self.cursor.expect(&Token::OperatorLt)?;
        self.parse_generic_args()
    }

    fn parse_path_component(&mut self) -> Result<UncheckedType> {
        match self.cursor.peek() {
            Some(Token::KeywordSuper) => {
                let start = self.cursor.peek_start();
                let end = self.cursor.peek_end();
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(IdentId::SUPER, self.location(start, end))))
            }
            Some(Token::KeywordCrate) => {
                let start = self.cursor.peek_start();
                let end = self.cursor.peek_end();
                self.cursor.advance();
                Ok(UncheckedType::Basic(Identifier::new(IdentId::CRATE, self.location(start, end))))
            }
            Some(Token::OperatorLt) => self.parse_basic_ident_or_generic_ty(),
            _ => self.parse_basic_ident_ty(),
        }
    }

    fn parse_lambda_parameter(&mut self) -> Result<FunctionParameter> {
        let start = self.cursor.peek_start();
        let qualifier_start = start;
        let is_mutable = self.cursor.eat(&Token::KeywordMut).is_some();
        let qualifier_end = self.cursor.peek_start();
        let name = self.parse_identifier_or_self()?;
        let ty = if self.cursor.eat(&Token::Colon).is_some() {
            self.parse_path_ty()?
        } else if name.id == IdentId::SELF {
            UncheckedType::Basic(Identifier::new(IdentId::TYPE_SELF, self.location(start, self.cursor.peek_start())))
        } else {
            return Err(self.unexpected(vec![ExpectedToken::Symbol(":".into())]));
        };
        let end = self.cursor.peek_start();
        Ok(FunctionParameter::new(
            name,
            TypeQualifier::new(is_mutable, self.location(qualifier_start, qualifier_end)),
            ty,
            self.location(start, end),
        ))
    }

    fn binary_operator(token: Option<&Token<'src>>) -> Option<(BinaryOperator, u8, bool)> {
        let result = match token? {
            Token::OperatorOr => (BinaryOperator::Or, 1, false),
            Token::OperatorAnd => (BinaryOperator::And, 2, false),
            Token::OperatorEq => (BinaryOperator::Eq, 3, false),
            Token::OperatorNeq => (BinaryOperator::Neq, 3, false),
            Token::OperatorGt => (BinaryOperator::Gt, 4, false),
            Token::OperatorLt => (BinaryOperator::Lt, 4, false),
            Token::OperatorGte => (BinaryOperator::Gte, 4, false),
            Token::OperatorLte => (BinaryOperator::Lte, 4, false),
            Token::OperatorBitOr => (BinaryOperator::BitOr, 5, false),
            Token::OperatorBitXor => (BinaryOperator::BitXor, 6, false),
            Token::OperatorBitAnd => (BinaryOperator::BitAnd, 7, false),
            Token::OperatorShl => (BinaryOperator::BitShl, 8, false),
            Token::OperatorShr => (BinaryOperator::BitShr, 8, false),
            Token::OperatorAdd => (BinaryOperator::Add, 9, false),
            Token::OperatorSub => (BinaryOperator::Sub, 9, false),
            Token::OperatorMul => (BinaryOperator::Mul, 10, false),
            Token::OperatorDiv => (BinaryOperator::Div, 10, false),
            Token::OperatorMod => (BinaryOperator::Mod, 10, false),
            Token::OperatorPow => (BinaryOperator::Pow, 11, true),
            _ => return None,
        };
        Some(result)
    }

    pub(crate) fn expression_location(&self, expr: &ExprNode<F>) -> Location {
        match expr {
            ExprNode::Path(node) => node.location,
            ExprNode::Value(ValueNode::Felt(_, loc))
            | ExprNode::Value(ValueNode::Bool(_, loc))
            | ExprNode::Value(ValueNode::U32(_, loc))
            | ExprNode::Value(ValueNode::Array(_, _, loc))
            | ExprNode::Value(ValueNode::ArrayRepeat(_, _, loc))
            | ExprNode::Value(ValueNode::Struct(_, _, _, loc)) => *loc,
            ExprNode::Binary(node) => node.location,
            ExprNode::Unary(node) => node.location,
            ExprNode::Call(node) => node.location,
            ExprNode::MemberCall(node) => node.location,
            ExprNode::Cast(node) => node.location,
            ExprNode::IndexAccess(node) => node.location,
            ExprNode::MemberAccess(node) => node.location,
            ExprNode::BlockExpr(node) => node.location,
            ExprNode::IfExpr(node) => node.location,
            ExprNode::Intrinsic(node) => node.location(),
            ExprNode::LambdaFunction(node) => node.location,
            ExprNode::Tuple(node) => node.location,
            ExprNode::TupleAccess(node) => node.location,
            ExprNode::Match(node) => node.location,
            ExprNode::Parentheses(id) => self.expression_location(&self.program.exprs[*id]),
        }
    }


    fn split_path_components(components: Vec<UncheckedType>) -> (Option<UncheckedType>, Vec<UncheckedType>) {
        let mut components = components.into_iter();
        let root = components.next();
        (root, components.collect())
    }

    fn unexpected_eof(&self, expected: Vec<ExpectedToken>) -> Error {
        Error::UnexpectedEof {
            expected,
            location: self.cursor.eof_location(),
        }
    }
}
