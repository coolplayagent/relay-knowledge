use std::{error::Error, fmt, future::Future, pin::Pin};

pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

/// Storage boundary failure.
#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Join(tokio::task::JoinError),
    LockPoisoned,
    Busy(String),
    InvalidInput(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "storage I/O failed: {error}"),
            Self::Sqlite(error) => write!(formatter, "sqlite operation failed: {error}"),
            Self::Join(error) => write!(formatter, "storage worker failed: {error}"),
            Self::LockPoisoned => write!(formatter, "sqlite connection lock was poisoned"),
            Self::Busy(message) => write!(formatter, "storage busy: {message}"),
            Self::InvalidInput(message) => write!(formatter, "invalid storage input: {message}"),
        }
    }
}

impl Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<tokio::task::JoinError> for StorageError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::Join(error)
    }
}

#[cfg(test)]
#[path = "boundary_tests.rs"]
mod tests;
