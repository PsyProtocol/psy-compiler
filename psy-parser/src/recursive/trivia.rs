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

#[cfg(test)]
mod tests {
    use psy_common::FileId;

    use super::*;

    fn cursor(src: &str) -> TokenCursor<'_> {
        let tokens = psy_lexer::lex_all(src).unwrap();
        TokenCursor::new(tokens, FileId(0))
    }

    #[test]
    fn leading_comments_are_collected_and_the_cursor_advances() {
        let mut c = cursor("// lead\n/* block */\nlet");
        let comments = collect_leading_comments(&mut c);
        assert_eq!(comments.len(), 2);
        assert!(matches!(comments[0], Comment::Line { .. }));
        assert!(matches!(comments[1], Comment::Block { .. }));
        assert!(comments[0].content().contains("lead"), "line comment content: {}", comments[0].content());
        assert!(comments[1].content().contains("block"), "block comment content: {}", comments[1].content());
        assert_eq!(c.peek(), Some(&psy_lexer::Token::KeywordLet));
    }

    #[test]
    fn trailing_comments_collect_until_the_closing_delimiter() {
        let mut c = cursor("// one\n/* two */\n}");
        let comments = collect_trailing_comments_until_close(&mut c);
        assert_eq!(comments.len(), 2);
        assert!(comments[0].content().contains("one"), "line comment content: {}", comments[0].content());
        assert!(comments[1].content().contains("two"), "block comment content: {}", comments[1].content());
        assert_eq!(c.peek(), Some(&psy_lexer::Token::RBrace));

        // No comments to consume leaves the cursor untouched.
        let mut c = cursor("let");
        assert!(collect_trailing_comments_until_close(&mut c).is_empty());
        assert_eq!(c.peek(), Some(&psy_lexer::Token::KeywordLet));
    }

    #[test]
    fn close_delimiter_detection_skips_comments() {
        assert!(at_close_delimiter(&cursor("// c\n}")));
        assert!(at_close_delimiter(&cursor(")")));
        assert!(at_close_delimiter(&cursor("]")));
        assert!(!at_close_delimiter(&cursor("// c\nlet")));
        assert!(!at_close_delimiter(&cursor("")));
    }

    #[test]
    fn comment_classification_splits_leading_from_trailing() {
        assert_eq!(classify_comments(&cursor("// c\n}")), CommentPosition::Trailing);
        assert_eq!(classify_comments(&cursor(")")), CommentPosition::Trailing);
        assert_eq!(classify_comments(&cursor("")), CommentPosition::Trailing);
        assert_eq!(classify_comments(&cursor("// c\nlet")), CommentPosition::Leading);
    }
}
