//! Defines blocking code-index boundary failures and I/O conversion.

use std::{error::Error, fmt};

const INCREMENTAL_CHANGED_PATH_LIMIT_PREFIX: &str =
    "incremental Git diff changed-path budget exceeded:";
const GITLINK_EXPANSION_LIMIT_PREFIX: &str = "gitlink path ";

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

impl CodeIndexError {
    pub(in crate::code) fn incremental_changed_path_limit(observed: usize, limit: usize) -> Self {
        Self::InvalidInput(format!(
            "{INCREMENTAL_CHANGED_PATH_LIMIT_PREFIX} reached {observed} changed paths, exceeding the bounded limit of {limit}; run a full code index"
        ))
    }

    pub(in crate::code) fn is_incremental_changed_path_limit(&self) -> bool {
        matches!(self, Self::InvalidInput(message) if message.starts_with(INCREMENTAL_CHANGED_PATH_LIMIT_PREFIX))
    }

    pub(in crate::code) fn is_gitlink_expansion_limit(&self) -> bool {
        matches!(self, Self::InvalidInput(message)
            if message.starts_with(GITLINK_EXPANSION_LIMIT_PREFIX)
                && message.contains(" expands to "))
    }
}

impl From<std::io::Error> for CodeIndexError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
