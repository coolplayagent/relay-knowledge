//! Error contract shared by Knowledge Map workflow and artifact boundaries.

use std::{error::Error, fmt, path::PathBuf};

/// Error surfaced by the file-backed knowledge map service.
#[derive(Debug)]
pub enum KnowledgeMapServiceError {
    Io(std::io::Error),
    Yaml(String),
    Domain(crate::domain::DomainError),
    LockTimeout(PathBuf),
    Integrity(String),
    UnsafePath(String),
}

impl fmt::Display for KnowledgeMapServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Yaml(error) => write!(formatter, "invalid knowledge map YAML: {error}"),
            Self::Domain(error) => write!(formatter, "{error}"),
            Self::LockTimeout(path) => write!(
                formatter,
                "timed out waiting for knowledge map write lock '{}'",
                path.display()
            ),
            Self::Integrity(message) => write!(formatter, "invalid knowledge map: {message}"),
            Self::UnsafePath(path) => {
                write!(formatter, "unsafe knowledge map artifact path '{path}'")
            }
        }
    }
}

impl Error for KnowledgeMapServiceError {}

impl From<std::io::Error> for KnowledgeMapServiceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<crate::domain::DomainError> for KnowledgeMapServiceError {
    fn from(error: crate::domain::DomainError) -> Self {
        Self::Domain(error)
    }
}
