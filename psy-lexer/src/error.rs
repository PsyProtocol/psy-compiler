use std::num::ParseIntError;

use thiserror::Error;

#[derive(Error, Default, Debug, Clone, PartialEq)]
pub enum Error {
    #[error("invalid integer: `{0}`")]
    InvalidInteger(#[from] ParseIntError),
    #[error("invalid token")]
    #[default]
    InvalidToken,
}

pub type Result<T> = std::result::Result<T, Error>;
