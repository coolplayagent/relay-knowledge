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
    CapacityExceeded(String),
    DurableStagingRequired(String),
    DurableStagingPending {
        completed_steps: usize,
        max_steps: usize,
    },
    DurableFinalizationRequired {
        checkpoint_state: String,
    },
    InvalidInput(String),
    Invariant(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "storage I/O failed: {error}"),
            Self::Sqlite(error) => write!(formatter, "sqlite operation failed: {error}"),
            Self::Join(error) => write!(formatter, "storage worker failed: {error}"),
            Self::LockPoisoned => write!(formatter, "sqlite connection lock was poisoned"),
            Self::Busy(message) => write!(formatter, "storage busy: {message}"),
            Self::CapacityExceeded(message) => {
                write!(formatter, "storage capacity exceeded: {message}")
            }
            Self::DurableStagingRequired(message) => {
                write!(formatter, "durable staging required: {message}")
            }
            Self::DurableStagingPending {
                completed_steps,
                max_steps,
            } => write!(
                formatter,
                "durable staging pending after step {completed_steps} of at most {max_steps}"
            ),
            Self::DurableFinalizationRequired { checkpoint_state } => write!(
                formatter,
                "durable incremental delta committed; finalization must resume from '{checkpoint_state}'"
            ),
            Self::InvalidInput(message) => write!(formatter, "invalid storage input: {message}"),
            Self::Invariant(message) => write!(formatter, "storage invariant failed: {message}"),
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
