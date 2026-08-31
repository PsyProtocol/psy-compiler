use std::path::PathBuf;

use psy_ast::Location;
use thiserror::Error as ThisError;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("{0}")]
    CommonError(#[from] psy_common::Error),
    #[error("{0}")]
    IoError(#[from] std::io::Error),
    #[error("File could not be resolved")]
    FileUnresolved,
    #[error("File parsed multiple times: {0}")]
    FileParsedMultipleTimes(PathBuf),
    #[error("No entry module found in {0}")]
    NoEntryModule(PathBuf),
    #[error("Invalid module name")]
    InvalidModuleName,
    #[error("Extern function can only be defined in std")]
    ExternFnNotInStd,
    #[error("Missing function body")]
    FunctionBodyMissing,
    #[error("Invalid self parameter")]
    InvalidSelfParameter,
    #[error("unexpected end of file")]
    UnexpectedEof { expected: Vec<ExpectedToken>, location: Location },
    #[error("unexpected token {found}")]
    UnexpectedToken {
        found: String,
        expected: Vec<ExpectedToken>,
        location: Location,
    },
    #[error("unsupported syntax: {feature}")]
    UnsupportedSyntax { feature: String, location: Location },
    #[error("lexical error")]
    LexicalError { location: Location },
}

/// A displayable token expectation for error reporting.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpectedToken {
    Keyword(&'static str),
    Symbol(String),
    Literal,
    Ident,
    Type,
    Expression,
    Statement,
    Eof,
}

impl std::fmt::Display for ExpectedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyword(s) => write!(f, "{}", s),
            Self::Symbol(s) => write!(f, "{}", s),
            Self::Literal => write!(f, "literal"),
            Self::Ident => write!(f, "identifier"),
            Self::Type => write!(f, "type"),
            Self::Expression => write!(f, "expression"),
            Self::Statement => write!(f, "statement"),
            Self::Eof => write!(f, "end of file"),
        }
    }
}

impl ExpectedToken {
    /// Classify a concrete token for parser diagnostics.
    pub fn from_token<'src>(t: &psy_lexer::Token<'src>) -> Self {
        use psy_lexer::Token::*;
        match t {
            KeywordConst | KeywordLet | KeywordMut | KeywordFn | KeywordStruct | KeywordEnum | KeywordImpl | KeywordTrait | KeywordReturn
            | KeywordMatch | KeywordIf | KeywordElse | KeywordWhile | KeywordFor | KeywordIn | KeywordWhere | KeywordAs | KeywordType
            | KeywordExtern | KeywordMod | KeywordUse | KeywordSelf | KeywordCrate | KeywordSuper | KeywordPub => {
                Self::Keyword("keyword")
            }
            Ident(_) => Self::Ident,
            U64(_) | U32(_) | Bool(_) | String(_) => Self::Literal,
            TypeBool | TypeFelt | TypeU32 | TypeArray | TypeSelf => Self::Type,
            _ => Self::Symbol(format!("{t:?}")),
        }
    }
}
