use std::{borrow::Cow, path::PathBuf};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum QLspError {
    #[error("Invalid URI: {0}")]
    InvalidUri(String),

    #[error("Failed to convert URI to file path: {0}")]
    UriToPathError(String),

    #[error("Failed to resolve file_id for path: {0:?}")]
    FileIdNotFound(PathBuf),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl From<QLspError> for tower_lsp::jsonrpc::Error {
    fn from(err: QLspError) -> Self {
        use tower_lsp::jsonrpc::Error as LspError;
        match &err {
            QLspError::InvalidUri(_) | QLspError::UriToPathError(_) | QLspError::FileIdNotFound(_) => LspError::invalid_params(err.to_string()),
            _ => LspError {
                code: tower_lsp::jsonrpc::ErrorCode::InternalError,
                message: Cow::from(err.to_string()),
                data: None,
            },
        }
    }
}

pub type QLspResult<T> = Result<T, QLspError>;
