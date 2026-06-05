use std::{error::Error, fmt};

/// Blocking code index failure.
#[derive(Debug)]
pub enum CodeIndexError {
    Io(std::io::Error),
    Git { args: Vec<String>, message: String },
    TreeSitter(String),
    InvalidInput(String),
}

impl fmt::Display for CodeIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "code index I/O failed: {error}"),
            Self::Git { args, message } => {
                write!(formatter, "git command failed ({args:?}): {message}")
            }
            Self::TreeSitter(message) => write!(formatter, "tree-sitter parse failed: {message}"),
            Self::InvalidInput(message) => write!(formatter, "invalid code index input: {message}"),
        }
    }
}

impl Error for CodeIndexError {}

impl From<std::io::Error> for CodeIndexError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
