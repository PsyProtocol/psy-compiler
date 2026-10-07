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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_all_returns_precise_byte_spans() {
        let tokens = lex_all("let value = 42u32;").unwrap();
        assert_eq!(
            tokens,
            vec![
                SpannedToken {
                    kind: Token::KeywordLet,
                    start: 0,
                    end: 3,
                },
                SpannedToken {
                    kind: Token::Ident("value"),
                    start: 4,
                    end: 9,
                },
                SpannedToken {
                    kind: Token::Assign,
                    start: 10,
                    end: 11,
                },
                SpannedToken {
                    kind: Token::U32(42),
                    start: 12,
                    end: 17,
                },
                SpannedToken {
                    kind: Token::Semicolon,
                    start: 17,
                    end: 18,
                },
            ]
        );
    }

    #[test]
    fn lex_all_stops_at_first_invalid_token_with_location() {
        let error = lex_all("let x = @;").unwrap_err();
        assert_eq!(error.kind, Error::InvalidToken);
        assert_eq!((error.start, error.end), (8, 9));
    }

    #[test]
    fn lex_all_reports_overflowing_integer_kind_and_span() {
        let input = "18446744073709551616";
        let error = lex_all(input).unwrap_err();
        assert!(matches!(error.kind, Error::InvalidInteger(_)));
        assert_eq!((error.start, error.end), (0, input.len()));
        assert!(error.kind.to_string().contains("invalid integer"));
    }

    #[test]
    fn lexer_prefers_longest_overlapping_operators() {
        let kinds = lex_all("** *= >> >>= >= :: .. -> =>")
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                Token::OperatorPow,
                Token::OperatorMulAssign,
                Token::OperatorShr,
                Token::OperatorBitShrAssign,
                Token::OperatorGte,
                Token::DoubleColon,
                Token::DoubleDot,
                Token::Arrow,
                Token::FatArrow,
            ]
        );
    }

    #[test]
    fn empty_and_whitespace_only_sources_have_no_tokens() {
        assert!(lex_all("").unwrap().is_empty());
        assert!(lex_all(" \t\r\n").unwrap().is_empty());
    }

    #[test]
    fn comments_at_eof_keep_exact_byte_spans() {
        let line = lex_all("// trailing").unwrap();
        assert_eq!(line[0].kind, Token::LineComment("// trailing"));
        assert_eq!((line[0].start, line[0].end), (0, 11));

        let block = lex_all("/* trailing */").unwrap();
        assert_eq!(block[0].kind, Token::BlockComment("/* trailing */"));
        assert_eq!((block[0].start, block[0].end), (0, 14));
    }

    #[test]
    fn unterminated_block_comment_reports_the_entire_remaining_input() {
        let input = "/* unterminated";
        let error = lex_all(input).unwrap_err();

        assert_eq!(error.kind, Error::InvalidToken);
        assert_eq!((error.start, error.end), (0, input.len()));
    }

    #[test]
    fn non_ascii_invalid_input_uses_byte_offsets() {
        let input = "let x = 中;";
        let error = lex_all(input).unwrap_err();

        assert_eq!(error.kind, Error::InvalidToken);
        assert_eq!(&input[error.start..error.end], "中");
        assert_eq!((error.start, error.end), (8, 11));
    }

    #[test]
    fn integer_suffix_boundaries_are_distinct_tokens() {
        let tokens = lex_all("0 0u32 4294967295u32 18446744073709551615")
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            vec![Token::U64(0), Token::U32(0), Token::U32(u32::MAX), Token::U64(u64::MAX)]
        );
    }

    #[test]
    fn overflowing_u32_suffix_reports_invalid_integer() {
        let input = "4294967296u32";
        let error = lex_all(input).unwrap_err();

        assert!(matches!(error.kind, Error::InvalidInteger(_)));
        assert_eq!((error.start, error.end), (0, input.len()));
    }

    #[test]
    fn leading_zeros_split_into_adjacent_integer_tokens() {
        let tokens = lex_all("007")
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(tokens, vec![Token::U64(0), Token::U64(0), Token::U64(7)]);
    }

    #[test]
    fn keyword_prefixed_identifiers_lex_as_idents() {
        let tokens = lex_all("letter letx trueish _x")
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            vec![
                Token::Ident("letter"),
                Token::Ident("letx"),
                Token::Ident("trueish"),
                Token::Ident("_x"),
            ]
        );
    }

    #[test]
    fn lone_underscore_is_placeholder_not_ident() {
        let tokens = lex_all("let _ = 1;")
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            vec![
                Token::KeywordLet,
                Token::Placeholder,
                Token::Assign,
                Token::U64(1),
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn string_literals_support_empty_and_escaped_contents() {
        let tokens = lex_all(r#""\"q\"""#)
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(tokens, vec![Token::String(r#"\"q\""#)]);
    }

    #[test]
    fn unterminated_string_consumes_the_rest_of_the_input() {
        let input = r#"let s = "abc"#;
        let error = lex_all(input).unwrap_err();

        assert_eq!(error.kind, Error::InvalidToken);
        assert_eq!((error.start, error.end), (8, input.len()));
    }

    #[test]
    fn block_comments_close_at_the_first_terminator() {
        let tokens = lex_all("/* /* */ */")
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            vec![Token::BlockComment("/* /* */"), Token::OperatorMul, Token::OperatorDiv]
        );
    }
}
