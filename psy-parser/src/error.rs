use std::path::PathBuf;

use psy_ast::Location;
use psy_common::FileId;
use psy_lexer::{Loc, Token};
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum UserError {
    #[error("{0}")]
    LexicalError(#[from] psy_lexer::Error),
    #[error("{0}")]
    CommonError(#[from] psy_common::Error),
    #[error("{0}")]
    IoError(#[from] std::io::Error),
    #[error("File could not be resolved")]
    FileUnresolved,
    #[error("Invalid module name")]
    InvalidModuleName,
    #[error("Extern function can only be defined in std")]
    ExternFnNotInStd,
    #[error("Missing function body")]
    FunctionBodyMissing,
    #[error("Invalid self parameter")]
    InvalidSelfParameter,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("{0}")]
    LexicalError(#[from] psy_lexer::Error),
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

    #[error("invalid token")]
    InvalidToken { location: Location },
    #[error("unrecognized eof")]
    UnrecognizedEof { expected: Vec<String>, location: Location },
    #[error("unrecognized token")]
    UnrecognizedToken {
        token: String,
        expected: Vec<String>,
        location: Location,
    },
    #[error("extra token")]
    ExtraToken { token: String, location: Location },
}

impl Error {
    pub fn from_lalrpop_error<'input>(error: lalrpop_util::ParseError<usize, Token<'input>, UserError>, file_id: FileId) -> Self {
        match error {
            lalrpop_util::ParseError::InvalidToken { location } => Error::InvalidToken {
                location: Location::new(file_id, location, location + 1),
            },
            lalrpop_util::ParseError::UnrecognizedEof { location, expected } => Error::UnrecognizedEof {
                location: Location::new(file_id, location, location + 1),
                expected,
            },
            lalrpop_util::ParseError::UnrecognizedToken {
                token: (start, token, end),
                expected,
            } => Error::UnrecognizedToken {
                token: token.to_string(),
                expected: expected,
                location: Location::new(file_id, start, end),
            },
            lalrpop_util::ParseError::ExtraToken { token: (start, token, end) } => Error::ExtraToken {
                token: token.to_string(),
                location: Location::new(file_id, start, end),
            },
            lalrpop_util::ParseError::User { error } => match error {
                UserError::LexicalError(error) => Error::LexicalError(error),
                UserError::CommonError(error) => Error::CommonError(error),
                UserError::IoError(error) => Error::IoError(error),
                UserError::FileUnresolved => Error::FileUnresolved,
                UserError::InvalidModuleName => Error::InvalidModuleName,
                UserError::ExternFnNotInStd => Error::ExternFnNotInStd,
                UserError::FunctionBodyMissing => Error::FunctionBodyMissing,
                UserError::InvalidSelfParameter => Error::InvalidSelfParameter,
            },
        }
    }
}
