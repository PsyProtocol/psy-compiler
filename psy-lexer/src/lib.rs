mod error;
mod token;

use logos::Logos;

pub use crate::{error::*, token::Token};

/// A token with its byte span in the source text.
#[derive(Clone, Debug, PartialEq)]
pub struct SpannedToken<'src> {
    pub kind: Token<'src>,
    pub start: usize,
    pub end: usize,
}

/// A lexer error with its byte span in the source text.
#[derive(Clone, Debug, PartialEq)]
pub struct LocatedError {
    pub kind: error::Error,
    pub start: usize,
    pub end: usize,
}

/// Result of collecting all tokens from a source string.
pub type LexResult<'src> = std::result::Result<Vec<SpannedToken<'src>>, LocatedError>;

/// Collect all tokens from source and stop at the first located lexer error.
pub fn lex_all<'src>(input: &'src str) -> LexResult<'src> {
    let mut tokens = Vec::new();
    for (result, span) in Token::lexer(input).spanned() {
        match result {
            Ok(kind) => tokens.push(SpannedToken {
                kind,
                start: span.start,
                end: span.end,
            }),
            Err(kind) => {
                return Err(LocatedError {
                    kind,
                    start: span.start,
                    end: span.end,
                });
            }
        }
    }
    Ok(tokens)
}
