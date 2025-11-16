use crate::{Spanned, Token};

pub struct GenericTokenTransformer<'input, I>
where
    I: Iterator<Item = Spanned<Token<'input>>>,
{
    underlying: I,
    in_generics: i32,
    buffered: Option<Spanned<Token<'input>>>,
}

impl<'input, I> GenericTokenTransformer<'input, I>
where
    I: Iterator<Item = Spanned<Token<'input>>>,
{
    pub fn new(lexer: I) -> Self {
        Self {
            underlying: lexer,
            in_generics: 0,
            buffered: None,
        }
    }
}

impl<'input, I> Iterator for GenericTokenTransformer<'input, I>
where
    I: Iterator<Item = Spanned<Token<'input>>>,
{
    type Item = Spanned<Token<'input>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(buffered) = self.buffered.take() {
            return Some(buffered);
        }

        let next = self.underlying.next()?;
        match next {
            Ok((start, token, end)) => {
                // Check if current token definitely indicates we're NOT in generics
                let definitely_not_generic = match token {
                    // These tokens clearly indicate we're in expression/statement context
                    Token::KeywordLet
                    | Token::KeywordIf
                    | Token::KeywordWhile
                    | Token::KeywordFor
                    | Token::KeywordReturn
                    | Token::KeywordMatch
                    | Token::Assign
                    | Token::Semicolon
                    | Token::LBrace
                    | Token::OperatorEq
                    | Token::OperatorNeq
                    | Token::OperatorLte
                    | Token::OperatorGte
                    | Token::OperatorAdd
                    | Token::OperatorSub
                    | Token::OperatorMul
                    | Token::OperatorDiv
                    | Token::OperatorMod
                    | Token::OperatorAnd
                    | Token::OperatorOr
                    | Token::U64(_)
                    | Token::U32(_)
                    | Token::Bool(_) => true,
                    _ => false,
                };

                // If we encounter a token that definitely indicates non-generic context, reset
                if self.in_generics > 0 && definitely_not_generic {
                    self.in_generics = 0;
                }

                match token {
                    Token::OperatorLt => {
                        self.in_generics += 1;
                        Some(Ok((start, Token::OperatorLt, end)))
                    }
                    Token::OperatorGt => {
                        self.in_generics -= 1;
                        if self.in_generics < 0 {
                            self.in_generics = 0;
                        }
                        Some(Ok((start, Token::OperatorGt, end)))
                    }
                    Token::OperatorShr => {
                        if self.in_generics > 0 {
                            let mid = start + 1;
                            self.buffered = Some(Ok((mid, Token::OperatorGt, end)));
                            Some(Ok((start, Token::OperatorGt, mid)))
                        } else {
                            Some(Ok((start, Token::OperatorShr, end)))
                        }
                    }
                    _ => Some(Ok((start, token, end))),
                }
            }
            Err(e) => Some(Err(e)),
        }
    }
}
