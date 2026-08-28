//! Token cursor for the recursive-descent parser.
//!
//! Provides token navigation, contextual `>` splitting for nested generics,
//! checkpoints for bounded backtracking, and comment collection.

use psy_ast::{Comment, Location};
use psy_common::FileId;
use psy_lexer::{LocatedError, SpannedToken, Token};

use crate::error::{Error, ExpectedToken};

/// Internal state for split `>>` tokens.
#[derive(Copy, Clone, Debug, PartialEq)]
enum SplitState {
    /// No split in progress.
    None,
    /// A `>>` was split into `>` + remaining `>` at byte offset.
    SplitRightShift { second_start: usize, second_end: usize },
    /// A `>>=` was split into `>` + remaining `>=` at byte offset.
    SplitRightShiftAssign { second_start: usize, second_end: usize },
}

/// A save point for bounded backtracking (recognition-only, no allocations).
#[derive(Copy, Clone)]
pub struct CursorCheckpoint {
    pos: usize,
    split: SplitState,
}

/// Token cursor over a pre-lexed token vector.
///
/// The cursor handles:
/// - `peek`/`eat`/`expect` navigation
/// - Contextual splitting of `>>` into two `>` when parsing generic type
///   closers
/// - Comment collection for AST attachment
/// - Checkpoints for bounded ambiguity resolution
pub struct TokenCursor<'src> {
    tokens: Vec<SpannedToken<'src>>,
    pos: usize,
    file_id: FileId,
    split: SplitState,
}

impl<'src> TokenCursor<'src> {
    /// Create a cursor from a lexed token stream.
    pub fn new(tokens: Vec<SpannedToken<'src>>, file_id: FileId) -> Self {
        Self {
            tokens,
            pos: 0,
            file_id,
            split: SplitState::None,
        }
    }

    /// Lex source and create a cursor, returning a located error on failure.
    pub fn from_source(source: &'src str, file_id: FileId) -> Result<Self, LocatedError> {
        let tokens = psy_lexer::lex_all(source)?;
        Ok(Self::new(tokens, file_id))
    }

    /// The file ID for location construction.
    pub fn file_id(&self) -> FileId {
        self.file_id
    }

    /// Create a location spanning byte offsets.
    pub fn location(&self, start: usize, end: usize) -> Location {
        Location::new(self.file_id, start, end)
    }

    // ─── Core navigation ──────────────────────────────────────────────────

    fn split_token(&self) -> Option<&Token<'src>> {
        match self.split {
            SplitState::None => None,
            SplitState::SplitRightShift { .. } => Some(&Token::OperatorGt),
            SplitState::SplitRightShiftAssign { .. } => Some(&Token::OperatorGte),
        }
    }

    /// Peek at the current token kind, accounting for split state.
    pub fn peek(&self) -> Option<&Token<'src>> {
        if let Some(token) = self.split_token() {
            return Some(token);
        }
        self.tokens.get(self.pos).map(|st| &st.kind)
    }

    /// Peek at the current token's start byte offset.
    pub fn peek_start(&self) -> usize {
        if let SplitState::SplitRightShift { second_start, .. } | SplitState::SplitRightShiftAssign { second_start, .. } = self.split {
            return second_start;
        }
        self.tokens.get(self.pos).map_or_else(|| self.eof_offset(), |token| token.start)
    }

    /// Peek at the current token's end byte offset.
    pub fn peek_end(&self) -> usize {
        if let SplitState::SplitRightShift { second_end, .. } | SplitState::SplitRightShiftAssign { second_end, .. } = self.split {
            return second_end;
        }
        self.tokens.get(self.pos).map_or_else(|| self.eof_offset(), |token| token.end)
    }

    /// Check if current token matches the given kind.
    pub fn at(&self, kind: &Token<'src>) -> bool {
        self.peek() == Some(kind)
    }

    /// Check if at EOF.
    pub fn at_end(&self) -> bool {
        self.peek().is_none()
    }

    /// Advance and return the consumed token + span.
    /// Handles split state: if a split is active, consumes the logical split
    /// token.
    fn advance_raw(&mut self) -> Option<(Token<'src>, usize, usize)> {
        match self.split {
            SplitState::SplitRightShift { second_start, second_end } => {
                self.split = SplitState::None;
                Some((Token::OperatorGt, second_start, second_end))
            }
            SplitState::SplitRightShiftAssign { second_start, second_end } => {
                self.split = SplitState::None;
                Some((Token::OperatorGte, second_start, second_end))
            }
            SplitState::None => {
                let st = self.tokens.get(self.pos)?;
                self.pos += 1;
                Some((st.kind.clone(), st.start, st.end))
            }
        }
    }

    /// Advance and return the consumed token and span.
    pub fn advance(&mut self) -> Option<(Token<'src>, usize, usize)> {
        self.advance_raw()
    }

    /// Eat a token if it matches, returning the span if consumed.
    pub fn eat(&mut self, kind: &Token<'src>) -> Option<(usize, usize)> {
        if self.at(kind) {
            let (_, s, e) = self.advance_raw()?;
            Some((s, e))
        } else {
            None
        }
    }

    /// Expect a specific token or return an error.
    pub fn expect(&mut self, kind: &Token<'src>) -> Result<(usize, usize), Error> {
        if let Some((s, e)) = self.eat(kind) {
            return Ok((s, e));
        }
        match self.peek() {
            Some(found) => Err(Error::UnexpectedToken {
                found: format!("{found:?}"),
                expected: vec![ExpectedToken::from_token(kind)],
                location: self.current_location(),
            }),
            None => Err(Error::UnexpectedEof {
                expected: vec![ExpectedToken::from_token(kind)],
                location: self.eof_location(),
            }),
        }
    }

    /// The EOF location (end of last token or start of file).
    pub fn eof_location(&self) -> Location {
        let offset = self.eof_offset();
        self.location(offset, offset)
    }

    // ─── Contextual > splitting ───────────────────────────────────────────

    /// Try to consume a `>` as a generic type closer.
    /// If the current token is `>>`, split it into `>` + `>` and consume one.
    /// If it's `>>=`, split into `>` + `>=` and consume one.
    /// If it's already `>`, consume normally.
    /// Returns true if a `>` was consumed.
    pub fn eat_type_gt(&mut self) -> bool {
        match self.split {
            SplitState::SplitRightShift { .. } => {
                self.advance_raw();
                true
            }
            SplitState::SplitRightShiftAssign { .. } => false,
            SplitState::None => {
                let Some(st) = self.tokens.get(self.pos) else { return false };
                match &st.kind {
                    Token::OperatorGt => {
                        self.pos += 1;
                        true
                    }
                    Token::OperatorShr => {
                        // The lexer span is byte-based and both operators are ASCII.
                        self.split = SplitState::SplitRightShift {
                            second_start: st.start + 1,
                            second_end: st.end,
                        };
                        self.pos += 1;
                        true
                    }
                    Token::OperatorBitShrAssign => {
                        // The lexer span is byte-based and both operators are ASCII.
                        self.split = SplitState::SplitRightShiftAssign {
                            second_start: st.start + 1,
                            second_end: st.end,
                        };
                        self.pos += 1;
                        true
                    }
                    _ => false,
                }
            }
        }
    }

    /// Expect a `>` as a generic type closer, returning an error if not found.
    pub fn expect_type_gt(&mut self) -> Result<Location, Error> {
        let start = self.peek_start();
        if self.eat_type_gt() {
            let end = self.peek_start();
            Ok(self.location(start, end))
        } else {
            match self.peek() {
                Some(found) => Err(Error::UnexpectedToken {
                    found: format!("{found:?}"),
                    expected: vec![ExpectedToken::Symbol(">".into())],
                    location: self.current_location(),
                }),
                None => Err(Error::UnexpectedEof {
                    expected: vec![ExpectedToken::Symbol(">".into())],
                    location: self.eof_location(),
                }),
            }
        }
    }

    // ─── Checkpoints ──────────────────────────────────────────────────────

    /// Save current position for bounded backtracking.
    /// Only use for recognition-only probes — never roll back past allocations.
    pub fn checkpoint(&self) -> CursorCheckpoint {
        CursorCheckpoint {
            pos: self.pos,
            split: self.split,
        }
    }

    /// Restore to a checkpoint.
    pub fn rewind(&mut self, cp: CursorCheckpoint) {
        self.pos = cp.pos;
        self.split = cp.split;
    }

    // ─── Comment collection ───────────────────────────────────────────────

    /// Collect leading comments (line and block) at the current position.
    /// Returns the collected comments and advances past them.
    pub fn take_leading_comments(&mut self) -> Vec<Comment> {
        let mut comments = Vec::new();
        loop {
            let comment = match self.peek() {
                Some(Token::LineComment(text)) => Comment::new_line(text.to_string(), self.current_location()),
                Some(Token::BlockComment(text)) => Comment::new_block(text.to_string(), self.current_location()),
                _ => break,
            };
            self.advance_raw();
            comments.push(comment);
        }
        comments
    }

    /// Peek ahead past comments to see the next non-comment token.
    pub fn peek_past_comments(&self) -> Option<&Token<'src>> {
        let mut i = self.pos;
        if let Some(token) = self.split_token() {
            return Some(token);
        }
        while let Some(st) = self.tokens.get(i) {
            match &st.kind {
                Token::LineComment(_) | Token::BlockComment(_) => i += 1,
                other => return Some(other),
            }
        }
        None
    }

    /// Look ahead N tokens (skipping comments) to see what's coming.
    /// Returns the token kind at the given non-comment offset.
    pub fn lookahead(&self, n: usize) -> Option<&Token<'src>> {
        let mut i = self.pos;
        let mut count = 0;
        if let Some(token) = self.split_token() {
            if n == 0 {
                return Some(token);
            }
            count = 1;
        }
        while let Some(st) = self.tokens.get(i) {
            match &st.kind {
                Token::LineComment(_) | Token::BlockComment(_) => i += 1,
                _ => {
                    if count == n {
                        return Some(&st.kind);
                    }
                    count += 1;
                    i += 1;
                }
            }
        }
        None
    }

    // ─── Locations ────────────────────────────────────────────────────────

    /// Current span as a Location.
    pub fn current_location(&self) -> Location {
        let start = self.peek_start();
        let end = self.peek_end();
        self.location(start, end)
    }

    /// All remaining tokens from the physical cursor position.
    pub fn remaining(&self) -> &[SpannedToken<'src>] {
        &self.tokens[self.pos..]
    }

    fn eof_offset(&self) -> usize {
        self.tokens.last().map_or(0, |token| token.end)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(src: &str) -> TokenCursor<'_> {
        let tokens = psy_lexer::lex_all(src).unwrap();
        TokenCursor::new(tokens, FileId(0))
    }

    #[test]
    fn test_basic_navigation() {
        let mut c = cursor("let x = 42;");
        assert!(c.at(&Token::KeywordLet));
        c.advance();
        assert!(c.at(&Token::Ident("x")));
        c.advance();
        assert!(c.at(&Token::Assign));
    }

    #[test]
    fn test_nested_generics_split() {
        let mut c = cursor("A<B<C>>");
        // A
        c.advance();
        // <
        c.advance();
        // B
        c.advance();
        // <
        c.advance();
        // C
        c.advance();
        // First > (from >>)
        assert!(c.eat_type_gt());
        // Second > (from >>)
        assert!(c.eat_type_gt());
        // EOF
        assert!(c.at_end());
    }

    #[test]
    fn test_right_shift_assign_split_exposes_gte_remainder() {
        let mut c = cursor("T>>=x");
        c.advance();
        assert!(c.eat_type_gt());
        assert_eq!(c.peek(), Some(&Token::OperatorGte));
        assert_eq!(c.peek_past_comments(), Some(&Token::OperatorGte));
        assert_eq!(c.lookahead(0), Some(&Token::OperatorGte));
        assert_eq!(c.lookahead(1), Some(&Token::Ident("x")));
        assert!(!c.eat_type_gt());
        assert_eq!(c.advance().map(|(token, _, _)| token), Some(Token::OperatorGte));
        assert_eq!(c.peek(), Some(&Token::Ident("x")));
    }

    #[test]
    fn test_split_remainder_counts_in_deep_lookahead() {
        let mut c = cursor("T>> // comment\nx");
        c.advance();
        assert!(c.eat_type_gt());
        assert_eq!(c.lookahead(0), Some(&Token::OperatorGt));
        assert_eq!(c.lookahead(1), Some(&Token::Ident("x")));
    }

    #[test]
    fn test_split_remainder_deep_lookahead_skips_multiple_comments() {
        let mut c = cursor("T>> /* first */ // second\nx + y");
        c.advance();
        assert!(c.eat_type_gt());
        assert_eq!(c.lookahead(0), Some(&Token::OperatorGt));
        assert_eq!(c.lookahead(1), Some(&Token::Ident("x")));
        assert_eq!(c.lookahead(2), Some(&Token::OperatorAdd));
        assert_eq!(c.lookahead(3), Some(&Token::Ident("y")));
        assert_eq!(c.lookahead(4), None);
        assert!(c.eat_type_gt());
        assert_eq!(c.peek_past_comments(), Some(&Token::Ident("x")));
    }

    #[test]
    fn test_checkpoint_restores_active_gte_remainder() {
        let mut c = cursor("T>>=x + y");
        c.advance();
        assert!(c.eat_type_gt());
        let split_checkpoint = c.checkpoint();
        assert_eq!(c.advance().map(|(token, _, _)| token), Some(Token::OperatorGte));
        assert_eq!(c.peek(), Some(&Token::Ident("x")));
        c.rewind(split_checkpoint);
        assert_eq!(c.peek(), Some(&Token::OperatorGte));
        assert_eq!(c.lookahead(1), Some(&Token::Ident("x")));
        assert_eq!(c.lookahead(2), Some(&Token::OperatorAdd));
        assert_eq!(c.lookahead(3), Some(&Token::Ident("y")));
    }

    #[test]
    fn test_checkpoint_before_split_restores_physical_shift_assign() {
        let mut c = cursor("T>>=x");
        c.advance();
        let physical_checkpoint = c.checkpoint();
        assert!(c.eat_type_gt());
        assert_eq!(c.peek(), Some(&Token::OperatorGte));
        c.rewind(physical_checkpoint);
        assert_eq!(c.peek(), Some(&Token::OperatorBitShrAssign));
        assert_eq!(c.advance().map(|(token, _, _)| token), Some(Token::OperatorBitShrAssign));
    }

    #[test]
    fn test_gte_remainder_location_and_physical_remaining() {
        let mut c = cursor("T>>=x");
        c.advance();
        assert!(c.eat_type_gt());
        assert_eq!(c.peek_start(), 2);
        assert_eq!(c.peek_end(), 4);
        assert_eq!(c.current_location(), Location::new(FileId(0), 2, 4));
        assert_eq!(c.remaining().len(), 1);
        assert_eq!(c.remaining()[0].kind, Token::Ident("x"));
    }

    #[test]
    fn test_shift_not_split() {
        let mut c = cursor("a >> b");
        // a
        c.advance();
        // >> should be a single shift token in expression context (via peek/advance)
        assert!(c.at(&Token::OperatorShr));
        // advance() returns it as a single OperatorShr token
        let (tok, _, _) = c.advance().unwrap();
        assert_eq!(tok, Token::OperatorShr);
        // Next token should be 'b', not a split '>'
        assert!(c.at(&Token::Ident("b")));
    }

    #[test]
    fn test_comments_collected() {
        let mut c = cursor("// hello\nlet x = 1;");
        let comments = c.take_leading_comments();
        assert_eq!(comments.len(), 1);
        assert!(c.at(&Token::KeywordLet));
    }

    #[test]
    fn test_checkpoint_rewind() {
        let mut c = cursor("let x = 42;");
        let cp = c.checkpoint();
        c.advance();
        c.advance();
        assert!(c.at(&Token::Assign));
        c.rewind(cp);
        assert!(c.at(&Token::KeywordLet));
    }
}
