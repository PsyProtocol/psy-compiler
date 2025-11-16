mod error;
mod token;
mod transformer;

use logos::{Logos, SpannedIter};

pub use crate::{error::*, token::Token, transformer::*};

pub type Loc = usize;
pub type Spanned<Tok> = Result<(Loc, Tok, Loc)>;

pub struct Lexer<'input> {
    token_stream: SpannedIter<'input, Token<'input>>,
}

impl<'input> Lexer<'input> {
    pub fn new(input: &'input str) -> Self {
        Self {
            token_stream: Token::lexer(input).spanned(),
        }
    }
}

impl<'input> Iterator for Lexer<'input> {
    type Item = Spanned<Token<'input>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.token_stream
            .next()
            .map(|(token, location)| Ok((location.start, token?, location.end)))
    }
}
