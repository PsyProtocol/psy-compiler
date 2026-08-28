//! Trivia (comment) attachment for the recursive-descent parser.
//!
//! Handles deterministic comment collection and attachment to AST nodes.
//! Comments are collected via `cursor.take_leading_comments()` and attached
//! to the following node. Trailing comments before a closing delimiter
//! are attached to the preceding node or block tail.

use psy_ast::Comment;

use crate::recursive::cursor::TokenCursor;

/// Collect comments that appear before a specific token type.
/// Advances the cursor past the comments.
pub fn collect_leading_comments<'src>(cursor: &mut TokenCursor<'src>) -> Vec<Comment> {
    cursor.take_leading_comments()
}

/// Collect comments that appear between the current position and a closing
/// delimiter (RBrace, RParen, RBracket, or EOF). These are "trailing" comments
/// that belong to the preceding block or expression.
///
/// In the LALRPOP grammar, these become `expr_comments` on a `BlockExprNode`
/// or `back_comments` on a module/impl/trait.
pub fn collect_trailing_comments_until_close<'src>(cursor: &mut TokenCursor<'src>) -> Vec<Comment> {
    let mut comments = Vec::new();
    loop {
        let comment_data: Option<Comment> = match cursor.peek() {
            Some(psy_lexer::Token::LineComment(s)) => {
                let start = cursor.peek_start();
                let end = cursor.peek_end();
                Some(Comment::new_line(s.to_string(), cursor.location(start, end)))
            }
            Some(psy_lexer::Token::BlockComment(s)) => {
                let start = cursor.peek_start();
                let end = cursor.peek_end();
                Some(Comment::new_block(s.to_string(), cursor.location(start, end)))
            }
            _ => None,
        };
        match comment_data {
            Some(comment) => {
                cursor.advance();
                comments.push(comment);
            }
            None => break,
        }
    }
    comments
}

/// Check if the next non-comment token is a closing delimiter.
pub fn at_close_delimiter<'src>(cursor: &TokenCursor<'src>) -> bool {
    matches!(
        cursor.peek_past_comments(),
        Some(psy_lexer::Token::RBrace) | Some(psy_lexer::Token::RParen) | Some(psy_lexer::Token::RBracket)
    )
}

/// Peek at the next non-comment token to determine if comments are
/// "leading" (before a node) or "trailing" (before a close delimiter).
pub fn classify_comments<'src>(cursor: &TokenCursor<'src>) -> CommentPosition {
    match cursor.peek_past_comments() {
        None => CommentPosition::Trailing, // EOF
        Some(psy_lexer::Token::RBrace) | Some(psy_lexer::Token::RParen) | Some(psy_lexer::Token::RBracket) => CommentPosition::Trailing,
        _ => CommentPosition::Leading,
    }
}

/// Position classification for comment attachment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommentPosition {
    /// Comments before a real node — attach to that node.
    Leading,
    /// Comments before a closing delimiter or EOF — attach to preceding node.
    Trailing,
}
